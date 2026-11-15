//! A bounded, full-duplex local transport. Native operations are implemented in
//! platform modules; messages and framing are identical on Linux and Windows.
//!
//! # Limits and what they bound
//!
//! * **Outbound (host to peer): bytes only.** Each connection has one FIFO
//!   queue of encoded frames, bounded by [`MAX_OUTBOUND_BYTES`] (frame header
//!   plus body, counted until the frame has been handed to the OS). There is
//!   deliberately **no message-count limit**: the GUI thread can emit thousands
//!   of small `document.event` notifications in one turn (a colour-scheme
//!   switch over many transitioned controls), far faster than any reader, and a
//!   count limit turned that normal burst into a disconnect (F-02 / HOST-01).
//!   A peer is disconnected only when it stops reading: either the byte budget
//!   is exhausted ([`Server::reply`] fails and the host disconnects), or one
//!   frame cannot be written within [`WRITE_DEADLINE`].
//! * **Nothing outbound is ever dropped or reordered.** Responses, host-to-peer
//!   requests, lifecycle/control notifications (`app.shutdown`, `window.event`
//!   `closed`, `request.cancelled`, `document.error`, ...) and ordinary DOM
//!   events share the one queue in the order the host produced them. A
//!   lifecycle notification behind a DOM-event burst is delayed, never lost.
//!   Separate lanes, sequence numbers and loss markers are intentionally not
//!   implemented while nothing is dropped (audit 07-roadmap 1.2/3.1, 08).
//! * **Inbound (peer to host): bytes and messages.** All connections share one
//!   queue to the GUI thread of at most [`MAX_EVENTS`] events and
//!   [`MAX_INBOUND_BYTES`]. When it is full, reader threads stop reading
//!   (backpressure through the socket/pipe); a peer is never disconnected for
//!   it. The 128-pending-request limit is a protocol rule (`protocol.rs`), not
//!   a transport queue.
use crate::platform::{NativeListener, NativeStream};
use crate::protocol::{read_frame, write_frame, MAX_FRAME_BYTES};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 8;
/// Inbound message limit (shared queue to the GUI thread; backpressure).
const MAX_EVENTS: usize = 256;
/// Per-connection outbound byte budget; the only outbound limit. At the
/// measured 360-600 B per DOM event this holds roughly 14k-23k events. The
/// channel adds a small per-frame bookkeeping overhead on top of this figure.
const MAX_OUTBOUND_BYTES: usize = 8 * 1024 * 1024;
/// Inbound byte limit (shared queue to the GUI thread; backpressure).
const MAX_INBOUND_BYTES: usize = 8 * 1024 * 1024;
/// A single frame that the peer does not accept within this time ends the
/// connection: the peer has stopped reading.
const WRITE_DEADLINE: Duration = Duration::from_secs(10);
const FRAME_HEADER_BYTES: usize = 4;

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
    /// Unbounded on purpose: `queued_bytes` is the bound (see module docs).
    /// Each entry is one complete frame, header included.
    outgoing: Sender<Vec<u8>>,
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
                    match terminal_receiver.recv_timeout(Duration::from_millis(100)) {
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
                let mut retry = AcceptRetryLog::default();
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
                        Err(error)
                            if crate::platform::accept_error_is_transient(&error)
                                && !state.stop.load(Ordering::Acquire) =>
                        {
                            // HOST-04: the listener stays usable; back off and keep accepting.
                            retry.note(&error);
                            let until = Instant::now() + ACCEPT_RETRY;
                            while Instant::now() < until && !state.stop.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(10));
                            }
                        }
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
    ///
    /// Fails only when the frame is too large, the connection is closed or
    /// closing, or the connection's outbound byte budget is exhausted (the
    /// peer has stopped reading). The number of queued messages never matters.
    pub fn reply(&self, connection: u64, value: Value) -> Result<()> {
        let frame = encode_frame(&value)?;
        let map = self
            .shared
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let peer = map.get(&connection).context("connection is closed")?;
        if peer.stopped.load(Ordering::Acquire) {
            bail!("connection is closing");
        }
        let length = frame.len();
        let previous = peer.queued_bytes.fetch_add(length, Ordering::AcqRel);
        if previous + length > MAX_OUTBOUND_BYTES {
            peer.queued_bytes.fetch_sub(length, Ordering::AcqRel);
            bail!(
                "connection outgoing byte limit exceeded ({previous} bytes unsent; \
                 the peer is not reading)"
            );
        }
        if peer.outgoing.send(frame).is_err() {
            peer.queued_bytes.fetch_sub(length, Ordering::AcqRel);
            bail!("connection writer has stopped");
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
        // Native accept and the terminal thread observe the stop flag within
        // their bounded 100 ms waits.
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

/// HOST-04: how long the accept loop waits after a transient accept error (for example
/// `EMFILE`, which leaves the pending connection in the backlog, so retrying at once would spin).
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// Rate-limited logging of transient accept errors: the first one, then at most one summary per
/// [`AcceptRetryLog::INTERVAL`] with the number of retries since.
#[derive(Default)]
struct AcceptRetryLog {
    logged_at: Option<Instant>,
    suppressed: u64,
}

impl AcceptRetryLog {
    const INTERVAL: Duration = Duration::from_secs(10);
    /// Returns whether this error was logged.
    fn note(&mut self, error: &io::Error) -> bool {
        if self
            .logged_at
            .is_some_and(|at| at.elapsed() < Self::INTERVAL)
        {
            self.suppressed += 1;
            return false;
        }
        tracing::warn!(
            %error,
            retries_since_last_report = self.suppressed,
            "transient IPC accept error; the listener keeps accepting"
        );
        self.logged_at = Some(Instant::now());
        self.suppressed = 0;
        true
    }
}

fn attach(shared: &Arc<Shared>, mut reader: NativeStream) -> Result<()> {
    let mut writer = reader.try_clone()?;
    let control = reader.try_clone()?;
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let stopped = Arc::new(AtomicBool::new(false));
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    // Bounded by `queued_bytes` in `Server::reply`, not by a slot count. A
    // large bounded `sync_channel` would preallocate its slots.
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
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
                    Ok(frame) => {
                        let result = writer
                            .set_timeout(Some(WRITE_DEADLINE))
                            .and_then(|_| write_encoded_frame(&mut writer, &frame));
                        queued_bytes.fetch_sub(frame.len(), Ordering::AcqRel);
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

/// Encode one complete frame (four-byte big-endian body length, then the JSON
/// body) into a single buffer, so the writer hands it to the OS in one call.
/// The size is checked before anything reaches the wire.
fn encode_frame(value: &Value) -> Result<Vec<u8>> {
    let mut frame = Vec::with_capacity(512);
    frame.extend_from_slice(&[0; FRAME_HEADER_BYTES]);
    serde_json::to_writer(&mut frame, value)?;
    let body = frame.len() - FRAME_HEADER_BYTES;
    if body > MAX_FRAME_BYTES {
        bail!("outgoing frame exceeds limit");
    }
    frame[..FRAME_HEADER_BYTES].copy_from_slice(&(body as u32).to_be_bytes());
    Ok(frame)
}

/// Write an encoded frame with one write call (one `write(2)` on Linux, one
/// `WriteFile` on Windows) when the OS accepts it whole. A separate header
/// write cost one extra syscall and, on Linux, one extra socket-buffer
/// allocation per frame, which roughly halved the frames the kernel could
/// absorb (HOST-09). `write_all` continues a partial write from the first
/// unwritten byte and retries `Interrupted`, so frames are never torn.
fn write_encoded_frame<W: Write>(writer: &mut W, frame: &[u8]) -> io::Result<()> {
    writer.write_all(frame)?;
    writer.flush()
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
    /// HOST-04: resource exhaustion and aborted connections are retried; other errors end the
    /// listener. Repeated transient errors are logged once per interval, not per retry.
    #[test]
    fn transient_accept_errors_are_retried_and_logged_at_a_bounded_rate() {
        #[cfg(target_os = "linux")]
        let (transient, fatal) = (
            [
                libc::EMFILE,
                libc::ENFILE,
                libc::ECONNABORTED,
                libc::ENOBUFS,
            ],
            [libc::EBADF, libc::EINVAL, libc::ENOTSOCK],
        );
        #[cfg(windows)]
        let (transient, fatal) = (
            [232, 231, 109, 1450], // ERROR_NO_DATA, _PIPE_BUSY, _BROKEN_PIPE, _NO_SYSTEM_RESOURCES
            [5, 6, 123],           // ERROR_ACCESS_DENIED, _INVALID_HANDLE, _INVALID_NAME
        );
        for code in transient {
            let error = io::Error::from_raw_os_error(code);
            assert!(
                crate::platform::accept_error_is_transient(&error),
                "{error}"
            );
        }
        for code in fatal {
            let error = io::Error::from_raw_os_error(code);
            assert!(
                !crate::platform::accept_error_is_transient(&error),
                "{error}"
            );
        }
        let mut log = AcceptRetryLog::default();
        let error = io::Error::from_raw_os_error(transient[0]);
        assert!(log.note(&error));
        assert!((0..100).all(|_| !log.note(&error)));
        assert_eq!(log.suppressed, 100);
    }

    /// HOST-04: clients that connect and close at once (on Windows often before
    /// `ConnectNamedPipe`, `ERROR_NO_DATA`) never end the listener.
    #[test]
    fn listener_survives_clients_that_close_immediately() {
        let app = format!("org.ui4me.transport.churn.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        for _ in 0..300 {
            if let Ok(stream) = NativeStream::connect(&server.endpoint, Duration::from_secs(5)) {
                drop(stream);
            }
        }
        // Drain whatever the churn produced; the listener must not have reported its end.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if let Ok(TransportEvent::Disconnected(0, reason)) =
                server.incoming.recv_timeout(Duration::from_millis(20))
            {
                assert!(!reason.starts_with("listener:"), "{reason}");
            }
        }
        let endpoint = server.endpoint.clone();
        let client = thread::spawn(move || {
            request(
                &endpoint,
                serde_json::json!({"jsonrpc":"2.0","id":"late","method":"ping"}),
                Duration::from_secs(5),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut answered = false;
        while Instant::now() < deadline && !answered {
            if let Ok(TransportEvent::Message(id, value)) =
                server.incoming.recv_timeout(Duration::from_millis(100))
            {
                assert_eq!(value["id"], "late");
                server
                    .reply(
                        id,
                        serde_json::json!({"jsonrpc":"2.0","id":"late","result":1}),
                    )
                    .unwrap();
                answered = true;
            }
        }
        assert!(answered, "the listener still accepts");
        assert_eq!(client.join().unwrap().unwrap()["result"], 1);
        server.close();
    }

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

    /// Records each write call; accepts at most `chunk` bytes per call.
    struct RecordingWriter {
        calls: Vec<usize>,
        wire: Vec<u8>,
        chunk: usize,
    }
    impl Write for RecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = bytes.len().min(self.chunk);
            self.calls.push(count);
            self.wire.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn decode_all(mut wire: &[u8]) -> Vec<Value> {
        let mut values = Vec::new();
        while let Some(value) = read_frame(&mut wire).unwrap() {
            values.push(value);
        }
        values
    }

    /// HOST-09: each frame, header included, is one write call.
    #[test]
    fn each_frame_is_written_with_a_single_write_call() {
        let values = [
            burst_event(1),
            serde_json::json!({"jsonrpc":"2.0","id":"a","result":{"ok":true}}),
            serde_json::json!({"jsonrpc":"2.0","method":"app.shutdown","params":{"deadline_ms":5000}}),
        ];
        let mut writer = RecordingWriter {
            calls: Vec::new(),
            wire: Vec::new(),
            chunk: usize::MAX,
        };
        let mut lengths = Vec::new();
        for value in &values {
            let frame = encode_frame(value).unwrap();
            let body = serde_json::to_vec(value).unwrap();
            assert_eq!(&frame[..4], &(body.len() as u32).to_be_bytes());
            assert_eq!(&frame[4..], &body[..]);
            lengths.push(frame.len());
            write_encoded_frame(&mut writer, &frame).unwrap();
        }
        assert_eq!(writer.calls, lengths, "one write call per whole frame");
        assert_eq!(decode_all(&writer.wire), values);
    }

    /// A short write continues from the first unwritten byte; the frame is
    /// neither torn nor duplicated.
    #[test]
    fn partial_writes_complete_the_frame() {
        let values = [burst_event(1), burst_event(2)];
        let mut writer = RecordingWriter {
            calls: Vec::new(),
            wire: Vec::new(),
            chunk: 7,
        };
        for value in &values {
            write_encoded_frame(&mut writer, &encode_frame(value).unwrap()).unwrap();
        }
        assert!(writer.calls.len() > 2);
        assert!(writer.calls.iter().all(|&count| count <= 7));
        assert_eq!(decode_all(&writer.wire), values);
    }

    #[test]
    fn oversized_outgoing_frame_is_rejected_before_queueing() {
        let value = serde_json::json!({"p": "x".repeat(MAX_FRAME_BYTES)});
        assert!(encode_frame(&value).is_err());
        let fits = serde_json::json!({"p": "x".repeat(MAX_FRAME_BYTES - 16)});
        assert_eq!(
            encode_frame(&fits).unwrap().len(),
            MAX_FRAME_BYTES - 16 + 12
        );
    }

    /// A document.event of realistic size (~400 bytes, like the transition
    /// events a colour-scheme switch emits; BIND-19 measured 360-600 B).
    fn burst_event(seq: usize) -> Value {
        const TYPES: [&str; 3] = ["transitionrun", "transitionstart", "transitionend"];
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "document.event",
            "params": {
                "document_id": "doc-1",
                "type": TYPES[seq % TYPES.len()],
                "seq": seq,
                "target": {"id": format!("button-{}", seq % 100), "tag": "button", "classes": ["control", "primary"]},
                "detail": {"propertyName": "background-color", "elapsedTime": 0.0, "pseudoElement": ""},
                "padding": "x".repeat(160),
            }
        })
    }

    fn connected_client(server: &Server) -> (NativeStream, u64) {
        let client = NativeStream::connect(&server.endpoint, Duration::from_secs(5)).unwrap();
        match server
            .incoming
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        {
            TransportEvent::Connected(id) => (client, id),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// T2 (F-02 / HOST-01, issue #1): a healthy backend that reads at a fixed
    /// rate of about one frame per millisecond must survive a burst that the
    /// GUI thread produces in one turn: 10,000 DOM events with responses and
    /// lifecycle notifications interleaved. No message may be refused, dropped
    /// or reordered, and the connection must stay up.
    #[test]
    fn t2_burst_to_a_slow_reading_peer_is_delivered_in_order_without_disconnect() {
        const EVENTS: usize = 10_000;
        let app = format!("org.ui4me.transport.t2.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let (mut client, connection) = connected_client(&server);

        // Build the burst: every 50th message is a response, every 500th a
        // lifecycle notification (the kinds PR #3 would have dropped).
        let mut burst = Vec::new();
        for event in 0..EVENTS {
            let seq = burst.len();
            burst.push(burst_event(seq));
            if event % 50 == 49 {
                let seq = burst.len();
                burst.push(serde_json::json!({
                    "jsonrpc": "2.0", "id": format!("r{seq}"), "result": {"seq": seq}
                }));
            }
            if event % 500 == 499 {
                let seq = burst.len();
                burst.push(serde_json::json!({
                    "jsonrpc": "2.0", "method": "window.event",
                    "params": {"window_id": "w1", "type": "focused", "data": {"seq": seq}}
                }));
            }
        }
        let seq = burst.len();
        burst.push(serde_json::json!({
            "jsonrpc": "2.0", "method": "app.shutdown",
            "params": {"deadline_ms": 5000, "seq": seq}
        }));
        let total = burst.len();
        let wire_bytes: usize = burst
            .iter()
            .map(|value| serde_json::to_vec(value).unwrap().len() + 4)
            .sum();

        let reader = thread::spawn(
            move || -> std::result::Result<(Vec<Value>, NativeStream), String> {
                client
                    .set_timeout(Some(Duration::from_secs(60)))
                    .map_err(|e| e.to_string())?;
                let mut frames = Vec::with_capacity(total);
                let mut start: Option<Instant> = None;
                while frames.len() < total {
                    // Fixed-rate consumer: frame n is read no earlier than n ms
                    // after the first frame arrived.
                    if let Some(start) = start {
                        let due = start + Duration::from_millis(frames.len() as u64);
                        if let Some(wait) = due.checked_duration_since(Instant::now()) {
                            thread::sleep(wait);
                        }
                    }
                    match read_frame(&mut client) {
                        Ok(Some(value)) => {
                            start.get_or_insert_with(Instant::now);
                            frames.push(value);
                        }
                        Ok(None) => {
                            return Err(format!("host closed after {} frames", frames.len()))
                        }
                        Err(error) => {
                            return Err(format!(
                                "read failed after {} frames: {error}",
                                frames.len()
                            ))
                        }
                    }
                }
                // Return the stream: dropping it here would close the connection.
                Ok((frames, client))
            },
        );

        let produced = Instant::now();
        for (index, value) in burst.iter().enumerate() {
            if let Err(error) = server.reply(connection, value.clone()) {
                panic!("message {index} of {total} was refused: {error:#}");
            }
        }
        let burst_time = produced.elapsed();
        let peak_queued = server
            .shared
            .connections
            .lock()
            .unwrap()
            .get(&connection)
            .map(|peer| peer.queued_bytes.load(Ordering::Acquire))
            .unwrap_or(0);
        let (frames, _client) = reader.join().unwrap().expect("reader failed");
        assert_eq!(frames.len(), total);
        for (index, (received, sent)) in frames.iter().zip(&burst).enumerate() {
            assert_eq!(received, sent, "frame {index} out of order or altered");
        }
        assert!(
            server.is_connected(connection),
            "healthy peer was disconnected"
        );
        for event in server.incoming.try_iter() {
            assert!(
                !matches!(event, TransportEvent::Disconnected(id, _) if id == connection),
                "unexpected {event:?}"
            );
        }
        eprintln!(
            "T2: {total} frames, {wire_bytes} wire bytes, burst enqueued in {burst_time:?}, \
             {peak_queued} bytes queued after the burst"
        );
        server.close();
    }

    /// A peer that stops reading must still be disconnected; with the message
    /// count cap gone, the 8 MiB byte budget is what refuses further output.
    #[test]
    fn peer_that_stops_reading_is_refused_at_the_byte_cap() {
        let app = format!("org.ui4me.transport.bytecap.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let (_client, connection) = connected_client(&server);
        let value = burst_event(0);
        let frame = serde_json::to_vec(&value).unwrap().len() + 4;
        let mut accepted = 0;
        let error = loop {
            match server.reply(connection, value.clone()) {
                Ok(()) => accepted += 1,
                Err(error) => break error,
            }
            assert!(accepted * frame < 64 * 1024 * 1024, "no outbound limit");
        };
        assert!(
            format!("{error:#}").contains("byte limit"),
            "refused by something other than the byte cap after {accepted}: {error:#}"
        );
        // Everything up to the budget is accepted (frames already handed to
        // the kernel no longer count), and the kernel holds well under 8 MiB.
        assert!(
            accepted * frame >= MAX_OUTBOUND_BYTES - frame,
            "accepted {accepted}"
        );
        assert!(
            accepted * frame <= 2 * MAX_OUTBOUND_BYTES,
            "accepted {accepted}"
        );
        // The host reacts to the refusal by disconnecting; that is prompt.
        server.disconnect(connection);
        assert!(!server.is_connected(connection));
        loop {
            if let TransportEvent::Disconnected(id, _) = server
                .incoming
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                assert_eq!(id, connection);
                break;
            }
        }
        server.close();
    }

    /// A peer that stops reading with less than the byte budget outstanding is
    /// disconnected by the writer's per-frame deadline (10 s), not kept forever.
    #[test]
    fn peer_that_stops_reading_is_disconnected_by_the_write_deadline() {
        let app = format!("org.ui4me.transport.deadline.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let (_client, connection) = connected_client(&server);
        // 2 MiB exceeds any kernel/pipe buffer, far below the 8 MiB budget.
        let value =
            serde_json::json!({"jsonrpc":"2.0","method":"x","params":{"p":"x".repeat(64 * 1024)}});
        for _ in 0..32 {
            server.reply(connection, value.clone()).unwrap();
        }
        let start = Instant::now();
        let reason = loop {
            match server.incoming.recv_timeout(Duration::from_secs(20)) {
                Ok(TransportEvent::Disconnected(id, reason)) if id == connection => break reason,
                Ok(_) => continue,
                Err(error) => panic!("stalled peer was never disconnected: {error}"),
            }
        };
        assert!(reason.starts_with("write"), "{reason}");
        assert!(start.elapsed() < Duration::from_secs(15));
        assert!(!server.is_connected(connection));
        server.close();
    }
}
