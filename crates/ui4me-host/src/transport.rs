//! A bounded, full-duplex local transport. Native operations are implemented in
//! platform modules; messages and framing are identical on Linux and Windows.
use crate::platform::{NativeListener, NativeStream};
use crate::protocol::{read_frame, write_frame, MAX_FRAME_BYTES};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 8;
const MAX_EVENTS: usize = 256;
const MAX_OUTBOUND_MESSAGES: usize = 128;
const MAX_OUTBOUND_BYTES: usize = 8 * 1024 * 1024;
const MAX_INBOUND_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum TransportEvent {
    Connected(u64),
    Message(u64, Value),
    Disconnected(u64, String),
}

#[derive(Debug)]
struct QueuedEvent {
    event: TransportEvent,
    bytes: usize,
}

/// The receiving API matches the useful standard Receiver methods, but releases
/// byte-budget reservations as messages leave the queue.
pub struct Incoming {
    receiver: Receiver<QueuedEvent>,
    queued_bytes: Arc<AtomicUsize>,
}

impl Incoming {
    fn take(&self, queued: QueuedEvent) -> TransportEvent {
        self.queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
        queued.event
    }
    pub fn try_recv(&self) -> std::result::Result<TransportEvent, mpsc::TryRecvError> {
        self.receiver.try_recv().map(|queued| self.take(queued))
    }
    pub fn recv(&self) -> std::result::Result<TransportEvent, mpsc::RecvError> {
        self.receiver.recv().map(|queued| self.take(queued))
    }
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<TransportEvent, mpsc::RecvTimeoutError> {
        self.receiver
            .recv_timeout(timeout)
            .map(|queued| self.take(queued))
    }
    pub fn try_iter(&self) -> impl Iterator<Item = TransportEvent> + '_ {
        std::iter::from_fn(move || self.try_recv().ok())
    }
}

struct Connection {
    outgoing: SyncSender<Vec<u8>>,
    queued_bytes: Arc<AtomicUsize>,
    control: NativeStream,
    stopped: Arc<AtomicBool>,
}

struct Shared {
    stop: AtomicBool,
    next_id: AtomicU64,
    connections: Mutex<HashMap<u64, Connection>>,
    events: SyncSender<QueuedEvent>,
    inbound_bytes: Arc<AtomicUsize>,
    terminal_events: mpsc::Sender<TransportEvent>,
    pending_terminals: AtomicUsize,
    wake: Arc<dyn Fn() + Send + Sync>,
}

pub struct Server {
    pub endpoint: String,
    pub incoming: Incoming,
    shared: Arc<Shared>,
    accept_thread: Option<JoinHandle<()>>,
    terminal_thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Hold the primary InstanceLock for the server's entire lifetime.
    pub fn bind(
        _app_id: &str,
        instance_key: &str,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let endpoint = crate::platform::endpoint_for(instance_key)?;
        let mut listener = NativeListener::bind(&endpoint)?;
        let (sender, receiver) = mpsc::sync_channel(MAX_EVENTS);
        // Admission accounts for pending terminal events, so this channel has
        // at most MAX_CONNECTIONS entries even though send() never blocks.
        let (terminal_events, terminal_receiver) = mpsc::channel();
        let inbound_bytes = Arc::new(AtomicUsize::new(0));
        let incoming = Incoming {
            receiver,
            queued_bytes: inbound_bytes.clone(),
        };
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            connections: Mutex::new(HashMap::new()),
            events: sender,
            inbound_bytes,
            terminal_events,
            pending_terminals: AtomicUsize::new(0),
            wake,
        });
        let terminal_state = shared.clone();
        let terminal_thread = thread::Builder::new()
            .name("ui4me-ipc-terminal".into())
            .spawn(move || {
                while !terminal_state.stop.load(Ordering::Acquire) {
                    match terminal_receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(event) => {
                            let delivered = emit(&terminal_state, event);
                            terminal_state
                                .pending_terminals
                                .fetch_sub(1, Ordering::AcqRel);
                            if !delivered {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        let state = shared.clone();
        let accept_result = thread::Builder::new()
            .name("ui4me-ipc-accept".into())
            .spawn(move || {
                while !state.stop.load(Ordering::Acquire) {
                    match listener.accept(&state.stop) {
                        Ok(Some(stream)) => {
                            if state
                                .connections
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .len()
                                + state.pending_terminals.load(Ordering::Acquire)
                                >= MAX_CONNECTIONS
                            {
                                stream.shutdown();
                                continue;
                            }
                            if let Err(error) = attach(&state, stream) {
                                emit(
                                    &state,
                                    TransportEvent::Disconnected(0, format!("accept: {error:#}")),
                                );
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            if !state.stop.load(Ordering::Acquire) {
                                emit(
                                    &state,
                                    TransportEvent::Disconnected(0, format!("listener: {error}")),
                                );
                            }
                            break;
                        }
                    }
                }
            });
        let accept_thread = match accept_result {
            Ok(thread) => thread,
            Err(error) => {
                shared.stop.store(true, Ordering::Release);
                let _ = terminal_thread.join();
                return Err(error.into());
            }
        };
        Ok(Self {
            endpoint,
            incoming,
            shared,
            accept_thread: Some(accept_thread),
            terminal_thread: Some(terminal_thread),
        })
    }

    /// Queue without waiting for the client to read. The GUI can call this.
    pub fn reply(&self, connection: u64, value: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&value)?;
        if bytes.len() > MAX_FRAME_BYTES {
            bail!("outgoing frame exceeds limit");
        }
        let map = self
            .shared
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let peer = map.get(&connection).context("connection is closed")?;
        if peer.stopped.load(Ordering::Acquire) {
            bail!("connection is closing");
        }
        let length = bytes.len() + 4;
        let previous = peer.queued_bytes.fetch_add(length, Ordering::AcqRel);
        if previous + length > MAX_OUTBOUND_BYTES {
            peer.queued_bytes.fetch_sub(length, Ordering::AcqRel);
            bail!("connection outgoing byte limit exceeded");
        }
        if let Err(error) = peer.outgoing.try_send(bytes) {
            peer.queued_bytes.fetch_sub(length, Ordering::AcqRel);
            return Err(anyhow::anyhow!(
                "connection outgoing queue unavailable: {error}"
            ));
        }
        Ok(())
    }

    pub fn disconnect(&self, connection: u64) {
        end_connection(&self.shared, connection, "closed by host");
    }

    pub fn is_connected(&self, connection: u64) -> bool {
        self.shared
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&connection)
    }

    pub fn close(&mut self) {
        if self.shared.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut peers = self
            .shared
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_, peer) in peers.drain() {
            peer.stopped.store(true, Ordering::Release);
            peer.control.shutdown();
        }
        drop(peers);
        // Native accept checks the stop flag at a bounded polling interval.
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.terminal_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}

fn attach(shared: &Arc<Shared>, mut reader: NativeStream) -> Result<()> {
    let mut writer = reader.try_clone()?;
    let control = reader.try_clone()?;
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let stopped = Arc::new(AtomicBool::new(false));
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(MAX_OUTBOUND_MESSAGES);
    {
        let mut peers = shared.connections.lock().unwrap_or_else(|e| e.into_inner());
        if peers.len() + shared.pending_terminals.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            reader.shutdown();
            return Ok(());
        }
        peers.insert(
            id,
            Connection {
                outgoing: sender,
                queued_bytes: queued_bytes.clone(),
                control,
                stopped: stopped.clone(),
            },
        );
    }
    if !emit(shared, TransportEvent::Connected(id)) {
        end_connection(shared, id, "server stopped");
        return Ok(());
    }

    let write_state = shared.clone();
    let write_stop = stopped.clone();
    if let Err(error) = thread::Builder::new()
        .name(format!("ui4me-ipc-write-{id}"))
        .spawn(move || {
            while !write_state.stop.load(Ordering::Acquire) && !write_stop.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(bytes) => {
                        let result = writer
                            .set_timeout(Some(Duration::from_secs(10)))
                            .and_then(|_| writer.write_all(&(bytes.len() as u32).to_be_bytes()))
                            .and_then(|_| writer.write_all(&bytes))
                            .and_then(|_| writer.flush());
                        queued_bytes.fetch_sub(bytes.len() + 4, Ordering::AcqRel);
                        if let Err(error) = result {
                            end_connection(&write_state, id, &format!("write: {error}"));
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            writer.shutdown();
        })
    {
        end_connection(shared, id, "cannot start writer");
        return Err(error.into());
    }
    let read_state = shared.clone();
    if let Err(error) = thread::Builder::new()
        .name(format!("ui4me-ipc-read-{id}"))
        .spawn(move || {
            // A peer that connects and never sends its first frame cannot reserve
            // an admission slot forever. After the first frame, authenticated idle
            // applications may stay connected without a heartbeat requirement.
            let _ = reader.set_timeout(Some(Duration::from_secs(15)));
            let mut first = true;
            while !read_state.stop.load(Ordering::Acquire) && !stopped.load(Ordering::Acquire) {
                match read_frame(&mut reader) {
                    Ok(Some(value)) => {
                        if first {
                            let _ = reader.set_timeout(None);
                            first = false;
                        }
                        if !emit(&read_state, TransportEvent::Message(id, value)) {
                            break;
                        }
                    }
                    Ok(None) => {
                        end_connection(&read_state, id, "peer disconnected");
                        break;
                    }
                    Err(error) => {
                        end_connection(&read_state, id, &format!("invalid/failed frame: {error}"));
                        break;
                    }
                }
            }
            end_connection(&read_state, id, "reader stopped");
            reader.shutdown();
        })
    {
        end_connection(shared, id, "cannot start reader");
        return Err(error.into());
    }
    Ok(())
}

fn emit(shared: &Shared, event: TransportEvent) -> bool {
    let message_connection = match &event {
        TransportEvent::Message(id, _) => Some(*id),
        _ => None,
    };
    // Count JSON bytes without allocating a second copy of a large frame.
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let bytes = match &event {
        TransportEvent::Message(_, value) => {
            let mut counter = Counter(4);
            if serde_json::to_writer(&mut counter, value).is_err() {
                return false;
            }
            counter.0
        }
        _ => 0,
    };
    if bytes > MAX_INBOUND_BYTES {
        return false;
    }
    // This function runs only on I/O/relay workers, never on the GUI thread.
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return false;
        }
        if message_connection.is_some_and(|id| {
            !shared
                .connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&id)
        }) {
            return false;
        }
        let current = shared.inbound_bytes.load(Ordering::Acquire);
        if current + bytes <= MAX_INBOUND_BYTES
            && shared
                .inbound_bytes
                .compare_exchange_weak(
                    current,
                    current + bytes,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        {
            break;
        }
        (shared.wake)();
        thread::sleep(Duration::from_millis(5));
    }
    let mut event = QueuedEvent { event, bytes };
    loop {
        if shared.stop.load(Ordering::Acquire)
            || message_connection.is_some_and(|id| {
                !shared
                    .connections
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains_key(&id)
            })
        {
            shared.inbound_bytes.fetch_sub(bytes, Ordering::AcqRel);
            return false;
        }
        match shared.events.try_send(event) {
            Ok(()) => {
                (shared.wake)();
                return true;
            }
            Err(TrySendError::Full(value)) => {
                event = value;
                // Wake the consumer before applying backpressure to this I/O
                // worker. Never retain a connections lock while waiting.
                (shared.wake)();
                thread::sleep(Duration::from_millis(5));
            }
            Err(TrySendError::Disconnected(_)) => {
                shared.inbound_bytes.fetch_sub(bytes, Ordering::AcqRel);
                return false;
            }
        }
    }
}

fn end_connection(shared: &Shared, id: u64, reason: &str) {
    let peer = {
        let mut peers = shared.connections.lock().unwrap_or_else(|e| e.into_inner());
        let peer = peers.remove(&id);
        // Account while holding the same lock used by connection admission.
        if peer.is_some() {
            shared.pending_terminals.fetch_add(1, Ordering::AcqRel);
        }
        peer
    };
    if let Some(peer) = peer {
        peer.stopped.store(true, Ordering::Release);
        peer.control.shutdown();
        let reason: String = reason.chars().take(1024).collect();
        if shared
            .terminal_events
            .send(TransportEvent::Disconnected(id, reason))
            .is_err()
        {
            shared.pending_terminals.fetch_sub(1, Ordering::AcqRel);
        }
        (shared.wake)();
    }
}

/// A short-lived secondary-launch client. No TCP or protocol-specific stdout.
pub fn request(endpoint: &str, message: Value, timeout: Duration) -> Result<Value> {
    let start = Instant::now();
    let mut stream =
        NativeStream::connect(endpoint, timeout).context("connect to existing instance")?;
    stream.set_timeout(Some(
        timeout
            .saturating_sub(start.elapsed())
            .max(Duration::from_millis(1)),
    ))?;
    write_frame(&mut stream, &message)?;
    let expected = message.get("id").cloned();
    loop {
        let value =
            read_frame(&mut stream)?.context("existing instance disconnected before replying")?;
        if expected.is_none() || value.get("id") == expected.as_ref() {
            return Ok(value);
        }
        if start.elapsed() >= timeout {
            bail!("existing instance response timed out");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_round_trip_and_clean_disconnect() {
        let app = format!("org.ui4me.transport.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        assert!(lock.primary);
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let endpoint = server.endpoint.clone();
        let client = thread::spawn(move || {
            request(
                &endpoint,
                serde_json::json!({"jsonrpc":"2.0","id":"one","method":"ping"}),
                Duration::from_secs(3),
            )
            .unwrap()
        });
        let connection = match server
            .incoming
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
        {
            TransportEvent::Connected(id) => id,
            other => panic!("unexpected {other:?}"),
        };
        match server
            .incoming
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
        {
            TransportEvent::Message(id, value) => {
                assert_eq!(id, connection);
                assert_eq!(value["method"], "ping");
            }
            other => panic!("unexpected {other:?}"),
        }
        server
            .reply(
                connection,
                serde_json::json!({"jsonrpc":"2.0","id":"one","result":"pong"}),
            )
            .unwrap();
        assert_eq!(client.join().unwrap()["result"], "pong");
        server.close();
    }

    #[test]
    fn disconnect_never_waits_for_a_full_gui_queue() {
        let app = format!("org.ui4me.transport.disconnect.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let _client = NativeStream::connect(&server.endpoint, Duration::from_secs(2)).unwrap();
        let connection = match server
            .incoming
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
        {
            TransportEvent::Connected(id) => id,
            other => panic!("unexpected {other:?}"),
        };
        for _ in 0..MAX_EVENTS {
            server
                .shared
                .events
                .try_send(QueuedEvent {
                    event: TransportEvent::Connected(0),
                    bytes: 0,
                })
                .unwrap();
        }
        let start = Instant::now();
        server.disconnect(connection);
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(!server.is_connected(connection));
        // A full GUI queue must delay delivery, never lose the terminal event.
        loop {
            if let TransportEvent::Disconnected(id, _) = server
                .incoming
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
            {
                assert_eq!(id, connection);
                break;
            }
        }
        server.close();
    }

    #[test]
    fn incoming_large_frames_obey_the_byte_budget() {
        let app = format!("org.ui4me.transport.budget.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let mut client = NativeStream::connect(&server.endpoint, Duration::from_secs(5)).unwrap();
        assert!(matches!(
            server
                .incoming
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            TransportEvent::Connected(_)
        ));
        let writer = thread::spawn(move || {
            let value = serde_json::json!({"payload": "x".repeat(MAX_FRAME_BYTES - 64)});
            for _ in 0..6 {
                write_frame(&mut client, &value).unwrap();
            }
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while server.shared.inbound_bytes.load(Ordering::Acquire) < 6 * 1024 * 1024 {
            assert!(Instant::now() < deadline, "incoming frames were not queued");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(server.shared.inbound_bytes.load(Ordering::Acquire) <= MAX_INBOUND_BYTES);
        let mut received = 0;
        while received < 6 {
            if let TransportEvent::Message(_, _) = server
                .incoming
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                received += 1;
            }
            assert!(server.shared.inbound_bytes.load(Ordering::Acquire) <= MAX_INBOUND_BYTES);
        }
        writer.join().unwrap();
        assert_eq!(server.shared.inbound_bytes.load(Ordering::Acquire), 0);
        server.close();
    }
}
