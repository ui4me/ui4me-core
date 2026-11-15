//! Host controller. The GUI thread owns every window and policy transition.
//! Workers can only send bounded messages back to this controller.
mod bitmap;
#[cfg(test)]
mod document_integration_tests;
mod document_native;
mod documents;
mod headless;
mod native;
use documents::HostedDocument;
mod requests;

use crate::{
    accessibility::{ActionEvent, NativeAccessibility},
    backend::Backend,
    config::{AppConfig, AppInfo, BackendConfig, HostConfig, InstancePolicy},
    lifecycle::{DecisionKind, DecisionTracker, Lifecycle, Phase},
    platform::{self, InstanceLock},
    presentation::Probe,
    protocol::{self, RpcError},
    transport::{self, Server, TransportEvent},
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use softbuffer::Surface;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicI32, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};
use winit::{
    event_loop::{ActiveEventLoop, AsyncRequestSerial, EventLoop, OwnedDisplayHandle},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

type Wake = Arc<dyn Fn() + Send + Sync>;
type Reply = (u64, String);

struct NativeWindow {
    // Adapter must be removed before the underlying HWND is destroyed.
    accessibility: NativeAccessibility,
    surface: Surface<OwnedDisplayHandle, Arc<dyn Window>>,
    window: Arc<dyn Window>,
    modifiers: ModifiersState,
    ime_enabled: bool,
    drag_selecting: bool,
    /// Whether a frame has been presented (logged once for startup timing).
    presented: bool,
    /// When the compositor last resized the surface and no frame of that
    /// size has been presented yet (debug-level resize-to-present timing).
    resized_at: Option<Instant>,
    pointer: (f64, f64),
    last_titlebar_press: Option<(Instant, f64, f64)>,
    document_epoch: Option<String>,
    composing: bool,
    ime_resets: u64,
    ime_quiet_until: Option<Instant>,
    preedit_cleared: bool,
    /// The IME state last sent while enabled. An unchanged state is not sent again: input
    /// methods answer every text-input update (for example by re-sending a pending dead-key
    /// preedit), and re-sending on each redraw turned that into an unbounded IME/redraw loop.
    ime_sent: Option<ImeState>,
    incoming_drop: Option<IncomingDrop>,
}

#[derive(Clone)]
struct IncomingDrop {
    transfer: winit::data_transfer::DataTransferId,
    document_id: Option<String>,
    target: Option<Value>,
    /// None means rejected, true selects local-file URIs, false selects text.
    files: Option<bool>,
    point: (f64, f64),
}

/// IME-visible state of the focused editor: surrounding text, cursor and anchor (bytes), the
/// cursor area in physical pixels and whether the field is a password.
#[derive(Clone, Debug, PartialEq)]
struct ImeState {
    text: String,
    cursor: usize,
    anchor: usize,
    area: (i64, i64, i64, i64),
    password: bool,
    hints: Value,
}

impl ImeState {
    fn new(
        text: &str,
        cursor: usize,
        anchor: usize,
        area: (f64, f64, f64, f64),
        password: bool,
    ) -> Self {
        // Sub-pixel jitter of the caret rectangle is not a change an input method can use.
        let px = |v: f64| v.round() as i64;
        Self {
            text: text.to_owned(),
            cursor,
            anchor,
            area: (px(area.0), px(area.1), px(area.2), px(area.3)),
            password,
            hints: Value::Null,
        }
    }
}

struct WindowRecord {
    title: String,
    width: u32,
    height: u32,
    visible: bool,
    resizable: bool,
    decorations: bool,
    limits: requests::WindowLimits,
    probe: Probe,
    document: Option<HostedDocument>,
    native: Option<NativeWindow>,
    closing: bool,
}

enum JobKind {
    Dialog,
    External,
    DocumentDialog {
        document_id: String,
        node: ui4me_renderer::NodeId,
    },
}
impl JobKind {
    fn is_dialog(&self) -> bool {
        matches!(self, Self::Dialog | Self::DocumentDialog { .. })
    }
}
struct PendingJob {
    reply: Option<Reply>,
    parent: Option<String>,
    kind: JobKind,
    cancelled: bool,
    dialog: Option<platform::dialogs::DialogHandle>,
}
struct JobResult {
    id: u64,
    value: Result<Value, RpcError>,
}

enum TransferTarget {
    Rpc(Reply),
    Paste(String),
    Drop(String),
    DropText(String),
    DocumentClipboard {
        window_id: String,
        document_id: String,
        request_id: u64,
    },
    DocumentDrop {
        window_id: String,
        document_id: String,
        target: Value,
        files: bool,
        x: f64,
        y: f64,
    },
}
struct PendingTransfer {
    target: TransferTarget,
    deadline: Instant,
}

pub struct Host {
    #[cfg(test)]
    observed_notifications: Vec<Value>,
    config: AppConfig,
    icon: winit::icon::Icon,
    headless: bool,
    standalone: bool,
    lifecycle: Lifecycle,
    decisions: DecisionTracker,
    server: Option<Server>,
    backend: Option<Backend>,
    _instance: Option<InstanceLock>,
    client: Option<u64>,
    connected: HashMap<u64, Instant>,
    /// HOST-08: connections whose output was refused once. They are being
    /// disconnected; further messages to them are counted, not sent or logged.
    undeliverable: HashMap<u64, u64>,
    token: String,
    wake: Wake,
    windows: BTreeMap<String, WindowRecord>,
    native_ids: HashMap<WindowId, String>,
    next_window: u64,
    started: Instant,
    quit_deadline: Option<Instant>,
    /// The quit deadline was set by a force quit, not by a slow backend.
    force_quit: bool,
    next_job: u64,
    jobs: HashMap<u64, PendingJob>,
    job_tx: mpsc::SyncSender<JobResult>,
    job_rx: mpsc::Receiver<JobResult>,
    action_tx: mpsc::SyncSender<ActionEvent>,
    action_rx: mpsc::Receiver<ActionEvent>,
    transfers: HashMap<AsyncRequestSerial, PendingTransfer>,
    /// Wayland selection id observed just before this host's last clipboard
    /// write, and when it was written (see read_clipboard_now).
    clipboard_written: Option<(Option<winit::data_transfer::DataTransferId>, Instant)>,
    /// Clipboard reads waiting for the compositor to announce our selection.
    deferred_clipboard_reads: Vec<TransferTarget>,
    exit_code: Arc<AtomicI32>,
    /// Set when a supervising launcher process has exited.
    launcher_gone: Option<Arc<AtomicBool>>,
    /// Shared native presentation context, created with the first window.
    presentation: Option<softbuffer::Context<OwnedDisplayHandle>>,
    preferences: platform::preferences::PreferenceWatch,
    preference_revision: u64,
    /// F-12: `None` until the first `test.advance_time`; then the virtual time advanced so far.
    virtual_time_ms: Option<f64>,
    /// HOST-07: termination requests (signals, console control, session end) and how many of
    /// them this host has handled.
    termination: Option<Arc<crate::termination::Termination>>,
    termination_seen: usize,
    termination_started: Option<Instant>,
    /// HOST-15: the policy requests sent for pending decisions (for a retry after "busy"), and
    /// the retries waiting for their backoff.
    decision_requests: HashMap<String, (&'static str, Value)>,
    decision_retries: Vec<(Instant, String)>,
    /// The last pump yielded with backend messages possibly left (HOST-05).
    pump_more: bool,
    pump_stats: PumpStats,
}

/// HOST-05/HOST-06: one pump handles backend messages for at most this long, or at most
/// [`PUMP_MESSAGES`] of them, before it yields to native events and frames (and re-wakes).
const PUMP_BUDGET: Duration = Duration::from_millis(8);
const PUMP_MESSAGES: usize = 64;
/// A single backend message that blocks the GUI thread at least this long is logged.
const SLOW_COMMAND: Duration = Duration::from_millis(100);

/// GUI-thread time accounting reported by `host.diagnostics` (`pump`), since the last report.
#[derive(Default)]
struct PumpStats {
    longest_turn: Duration,
    budget_yields: u64,
    slow_commands: u64,
}

/// The operation a backend message names, for the slow-command log: the `document.command`
/// `op`, otherwise the method (responses have none).
fn slow_command_name(value: &Value) -> String {
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("response");
    match value.pointer("/params/command/op").and_then(Value::as_str) {
        Some(op) if method == "document.command" => format!("document.command {op}"),
        _ => method.chars().take(128).collect(),
    }
}

/// HOST-15: how often a decision answered with "busy" is asked again (100 ms … 1.6 s backoff,
/// about 3 s in total) before it counts as no decision.
const MAX_DECISION_RETRIES: u32 = 5;

/// Launch a manifest, forwarding secondary launches to the existing instance.
pub fn run(config: AppConfig, headless: bool) -> Result<i32> {
    platform::initialize_application(&config.app.id)?;
    let instance = if config.host.instance == InstancePolicy::Single {
        match primary_or_forward(&config)? {
            Some(lock) => Some(lock),
            None => return Ok(0),
        }
    } else {
        None
    };
    let result = Arc::new(AtomicI32::new(0));
    let termination = crate::termination::Termination::global();
    if headless {
        let mut host = Host::new(
            config,
            true,
            false,
            instance,
            Arc::new(|| {}),
            result.clone(),
        )?;
        host.watch_termination(termination.clone());
        while host.lifecycle.phase() != Phase::Stopped {
            host.pump(None);
            if !host.pump_more {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    } else {
        let event_loop = event_loop().context(
            "cannot create native event loop (Wayland or interactive Windows desktop required)",
        )?;
        let proxy = event_loop.create_proxy();
        let wake: Wake = Arc::new(move || proxy.wake_up());
        let mut host = Host::new(config, false, false, instance, wake, result.clone())?;
        host.watch_termination(termination.clone());
        let outcome = event_loop.run_app(host);
        termination.finished();
        outcome?;
    }
    termination.finished();
    Ok(result.load(Ordering::Relaxed))
}

/// The native event loop, built on the calling thread. `main` runs the host on a dedicated
/// engine thread with an explicit stack (F-01), so the event loop must be allowed off the process
/// main thread; the vendored winit backends (Win32, Wayland) support that. Windows and their
/// surfaces are created and used only on this same thread.
fn event_loop() -> Result<EventLoop, winit::error::EventLoopError> {
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "linux")]
    winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    #[cfg(windows)]
    winit::platform::windows::EventLoopBuilderExtWindows::with_any_thread(&mut builder, true);
    builder.build()
}

/// The primary instance lock, or `None` once this launch was delivered to the running primary.
///
/// HOST-10: a primary that is shutting down answers `-32004`, and one that has already closed
/// its endpoint cannot be reached; either is about to release the lock. This launch then waits
/// for the lock (bounded by the primary's shutdown deadline plus a margin) and becomes the new
/// primary, instead of failing the user's relaunch. If another launch became the primary in the
/// meantime, this one is forwarded to it.
fn primary_or_forward(config: &AppConfig) -> Result<Option<InstanceLock>> {
    primary_or_forward_within(config, FORWARD_ATTEMPT)
}

/// How long the first delivery attempt to a running primary may take.
const FORWARD_ATTEMPT: Duration = Duration::from_secs(5);

fn primary_or_forward_within(
    config: &AppConfig,
    first_attempt: Duration,
) -> Result<Option<InstanceLock>> {
    let lock = InstanceLock::acquire(&config.app.id)?;
    if lock.primary {
        return Ok(Some(lock));
    }
    let error = match forward_activation(&lock.endpoint, config, Some(first_attempt)) {
        Ok(()) => return Ok(None),
        Err(error) if primary_is_ending(&error) => error,
        Err(error) => return Err(error),
    };
    drop(lock);
    tracing::info!(
        reason = format!("{error:#}"),
        "primary instance is shutting down; waiting to take over"
    );
    let deadline = Instant::now()
        + Duration::from_millis(config.host.shutdown_timeout_ms)
        + Duration::from_secs(2);
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let lock = InstanceLock::acquire(&config.app.id)?;
        if lock.primary {
            tracing::info!("previous primary instance has exited; this launch is the primary");
            return Ok(Some(lock));
        }
        // Still held: by the ending primary, or by a launch that took over first.
        match forward_activation(&lock.endpoint, config, Some(Duration::from_millis(250))) {
            Ok(()) => return Ok(None),
            Err(error) if primary_is_ending(&error) && Instant::now() < deadline => {}
            Err(error) => return Err(error),
        }
    }
}

/// The primary answered "shutting down" (`-32004`), its endpoint cannot be connected to (it was
/// removed, refuses connections, or stays busy for the whole attempt), or it closed the
/// connection before answering: it closes its endpoint, and then its connections, before it
/// exits and releases the lock. Any other outcome from a live primary is final — an error
/// answer (for example invalid parameters), and also a request that was sent but not answered
/// in time: the primary has queued it and may still deliver it, so retrying would turn one
/// launch into many `app.activated` notifications (F-25 review #2).
fn primary_is_ending(error: &anyhow::Error) -> bool {
    use crate::transport::EndpointLost;
    use std::io::ErrorKind;
    if let Some(rpc) = error.downcast_ref::<RpcError>() {
        return rpc.code == protocol::code::CLOSED;
    }
    match error.downcast_ref::<EndpointLost>() {
        Some(EndpointLost::Disconnected(_)) => true,
        // `NativeStream::connect` retries a missing or refusing endpoint (Linux) and a missing or
        // busy pipe (Windows ERROR_FILE_NOT_FOUND / ERROR_PIPE_BUSY) until its timeout.
        Some(EndpointLost::Connect(error)) => matches!(
            error.kind(),
            ErrorKind::NotFound
                | ErrorKind::ConnectionRefused
                | ErrorKind::TimedOut
                | ErrorKind::WouldBlock
        ),
        None => false,
    }
}

/// Deliver this launch to the primary instance. A primary that is still
/// starting (for example two simultaneous launches) cannot yet deliver the
/// activation to a ready backend, so retry within the manifest's bounded
/// startup deadline instead of losing the user's launch.
/// `attempt` caps the time of each connection attempt (default 5 s).
fn forward_activation(endpoint: &str, config: &AppConfig, attempt: Option<Duration>) -> Result<()> {
    let request = json!({"jsonrpc":"2.0","id":"activation","method":"app.activate",
        "params":{"args":std::env::args().collect::<Vec<_>>(),"cwd":std::env::current_dir()?,
            "activation_token":std::env::var("XDG_ACTIVATION_TOKEN").ok()}});
    let deadline = Instant::now()
        + Duration::from_millis(config.host.startup_timeout_ms)
        + Duration::from_secs(5);
    let attempt = attempt.unwrap_or(Duration::from_secs(5));
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now()).clamp(
            Duration::from_millis(100),
            attempt.max(Duration::from_millis(100)),
        );
        let response = transport::request(endpoint, request.clone(), timeout)?;
        match protocol::parse_response(response)?.outcome {
            Ok(_) => return Ok(()),
            Err(error)
                if error.code == protocol::code::UNAVAILABLE && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// The native probe works without a language runtime, so failures can be
/// isolated to the host/OS boundary before testing Python.
pub fn run_probe() -> Result<i32> {
    let config = AppConfig {
        app: AppInfo {
            id: "org.ui4me.HostProbe".into(),
            name: "UI4ME native host probe".into(),
            version: None,
            icon: None,
        },
        backend: BackendConfig {
            program: PathBuf::new(),
            args: Vec::new(),
            cwd: std::env::current_dir()?,
            env: BTreeMap::new(),
        },
        host: HostConfig {
            instance: InstancePolicy::Multiple,
            ..HostConfig::default()
        },
    };
    platform::initialize_application(&config.app.id)?;
    let event_loop = event_loop()?;
    let proxy = event_loop.create_proxy();
    let result = Arc::new(AtomicI32::new(0));
    let mut host = Host::new(
        config,
        false,
        true,
        None,
        Arc::new(move || proxy.wake_up()),
        result.clone(),
    )?;
    let termination = crate::termination::Termination::global();
    host.watch_termination(termination.clone());
    let outcome = event_loop.run_app(host);
    termination.finished();
    outcome?;
    Ok(result.load(Ordering::Relaxed))
}

pub fn doctor() -> Value {
    let environment: serde_json::Map<String, Value> = [
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_ID",
        "SESSIONNAME",
        "UI4ME_SYSTEMD_UNIT",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var(key)
            .ok()
            .map(|v| (key.to_owned(), Value::String(v)))
    })
    .collect();
    json!({"host_version":env!("CARGO_PKG_VERSION"),"protocol_version":protocol::PROTOCOL_VERSION,
        "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"pid":std::process::id(),
        "environment":environment,"containment":platform::containment_description(),
        "run_containment":platform::run_containment_description(),
        "paths":platform::standard_paths("org.ui4me.HostProbe").ok(),
        "native_tests":"not run by doctor","renderer":"CPU HTML/CSS desktop profile ui4me-desktop-1.0 (plus diagnostic probe)",
        "windowing_dependency":"winit 0.31.0-beta.4 (exactly pinned)"})
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("OS random source: {e}"))?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}

/// HOST-11: the endpoint key of an `instance = "multiple"` host. It is public (socket path,
/// pipe name, `UI4ME_ENDPOINT`, logs), so it is drawn independently of the secret token.
fn multiple_instance_key() -> Result<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("OS random source: {e}"))?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}

impl Host {
    fn new(
        config: AppConfig,
        headless: bool,
        standalone: bool,
        instance: Option<InstanceLock>,
        wake: Wake,
        exit_code: Arc<AtomicI32>,
    ) -> Result<Self> {
        #[cfg(target_os = "linux")]
        if !headless {
            accesskit_atspi_common::set_application_name(config.app.name.clone())
                .map_err(anyhow::Error::msg)?;
        }
        let icon = crate::icons::load(config.app.icon.as_deref())?;
        let token = random_token()?;
        let instance_key = match &instance {
            Some(lock) => lock.instance_key.clone(),
            None => multiple_instance_key()?,
        };
        let server = if standalone {
            None
        } else {
            Some(Server::bind(&config.app.id, &instance_key, wake.clone())?)
        };
        let backend = match &server {
            Some(server) => Some(Backend::spawn(
                &config.backend,
                &server.endpoint,
                &token,
                &config.app.id,
                wake.clone(),
            )?),
            None => None,
        };
        let (job_tx, job_rx) = mpsc::sync_channel(protocol::MAX_PENDING);
        let (action_tx, action_rx) = mpsc::sync_channel(128);
        let launcher_gone = platform::watch_launcher(wake.clone());
        let preferences = platform::preferences::PreferenceWatch::new(!headless, wake.clone());
        let mut lifecycle = Lifecycle::new();
        lifecycle.begin_connecting()?;
        if standalone {
            lifecycle.begin_initializing()?;
            lifecycle.ready()?;
        }
        tracing::info!(
            headless,
            standalone,
            containment = platform::containment_description(),
            "host started"
        );
        Ok(Self {
            config,
            icon,
            headless,
            standalone,
            lifecycle,
            decisions: DecisionTracker::default(),
            server,
            backend,
            _instance: instance,
            client: None,
            connected: HashMap::new(),
            undeliverable: HashMap::new(),
            token,
            wake,
            windows: BTreeMap::new(),
            native_ids: HashMap::new(),
            next_window: 1,
            started: Instant::now(),
            quit_deadline: None,
            force_quit: false,
            next_job: 1,
            jobs: HashMap::new(),
            job_tx,
            job_rx,
            action_tx,
            action_rx,
            transfers: HashMap::new(),
            clipboard_written: None,
            deferred_clipboard_reads: Vec::new(),
            exit_code,
            launcher_gone,
            presentation: None,
            preferences,
            preference_revision: 0,
            virtual_time_ms: None,
            termination: None,
            termination_seen: 0,
            termination_started: None,
            decision_requests: HashMap::new(),
            decision_retries: Vec::new(),
            pump_more: false,
            pump_stats: PumpStats::default(),
            #[cfg(test)]
            observed_notifications: Vec::new(),
        })
    }

    /// Queue one message. The transport bounds output by bytes only, so this
    /// fails only for a peer that has stopped reading (or is already closed).
    /// HOST-08: the first failure logs once and disconnects; every later
    /// message to that connection is counted and reported once when the
    /// disconnect is processed, instead of one warning (and one disconnect
    /// attempt) per message.
    fn send(&mut self, connection: u64, value: Value) {
        let Some(server) = &self.server else {
            return;
        };
        if let Some(suppressed) = self.undeliverable.get_mut(&connection) {
            *suppressed += 1;
            return;
        }
        if !self.connected.contains_key(&connection) {
            // The disconnect was already processed (e.g. a late job result).
            tracing::debug!(connection, "message for a closed connection not sent");
            return;
        }
        if let Err(error) = server.reply(connection, value) {
            tracing::warn!(
                connection,
                error = format!("{error:#}"),
                "connection cannot receive output; disconnecting it"
            );
            self.undeliverable.insert(connection, 0);
            server.disconnect(connection);
        }
    }
    /// Forget a connection whose `Disconnected` event has been processed.
    fn connection_closed(&mut self, connection: u64) {
        self.connected.remove(&connection);
        if let Some(suppressed) = self.undeliverable.remove(&connection) {
            if suppressed > 0 {
                tracing::warn!(
                    connection,
                    suppressed,
                    "further messages were not sent to the disconnected connection"
                );
            }
        }
    }
    fn respond(&mut self, reply: &Reply, value: Result<Value, RpcError>) {
        let mut message = match value {
            Ok(value) => protocol::success(&reply.1, value),
            Err(error) => protocol::error(Some(&reply.1), &error),
        };
        if let Err(error) = protocol::encoded_size(&message) {
            tracing::warn!(%error, "response could not fit the protocol frame");
            message = protocol::error(
                Some(&reply.1),
                &RpcError::unavailable("Result exceeds the encoded protocol frame limit"),
            );
        }
        self.send(reply.0, message);
    }
    fn notify(&mut self, method: &str, params: Value) {
        if let Some(client) = self.client {
            let message = protocol::notification(method, params);
            if let Err(error) = protocol::encoded_size(&message) {
                // A notification has no caller awaiting an error response.
                // Preserve the session and make this dropped event visible in
                // diagnostics rather than disconnecting a healthy backend.
                tracing::warn!(method, %error, "oversized host notification dropped");
                return;
            }
            #[cfg(test)]
            self.observed_notifications.push(message.clone());
            self.send(client, message);
        }
    }
    /// `host.diagnostics` `pump`: GUI-thread turn statistics since the previous report. The
    /// longest turn bounds how long frames and input waited for backend work (gate R5(c)).
    fn take_pump_stats(&mut self) -> Value {
        let stats = std::mem::take(&mut self.pump_stats);
        json!({"longest_turn_ms":stats.longest_turn.as_secs_f64() * 1000.0,
            "budget_yields":stats.budget_yields,"slow_commands":stats.slow_commands,
            "budget_ms":PUMP_BUDGET.as_millis() as u64,"budget_messages":PUMP_MESSAGES})
    }

    fn window_event_message(&mut self, id: &str, kind: &str, data: Value) {
        self.notify(
            "window.event",
            json!({"window_id":id,"type":kind,"data":data}),
        );
    }

    fn pump(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        let turn = Instant::now();
        self.apply_native_preferences(event_loop);
        // HOST-05/HOST-06: an input/log flood or a slow command must not monopolize native
        // event dispatch. Backend messages are processed until PUMP_BUDGET has elapsed or
        // PUMP_MESSAGES were handled; then the loop yields (frames, input) and is woken again
        // at once for the rest instead of waiting for the next timer or input event.
        self.pump_more = false;
        let mut handled = 0;
        loop {
            if handled == PUMP_MESSAGES || turn.elapsed() >= PUMP_BUDGET {
                self.pump_more = true;
                self.pump_stats.budget_yields += 1;
                (self.wake)();
                break;
            }
            let event = self
                .server
                .as_ref()
                .and_then(|s| s.incoming.try_recv().ok());
            let Some(event) = event else { break };
            handled += 1;
            match event {
                TransportEvent::Connected(id) => {
                    self.connected.insert(id, Instant::now());
                }
                TransportEvent::Message(id, value) => {
                    let operation = slow_command_name(&value);
                    let started = Instant::now();
                    self.message(id, value, event_loop);
                    let took = started.elapsed();
                    if took >= SLOW_COMMAND {
                        self.pump_stats.slow_commands += 1;
                        tracing::warn!(
                            operation,
                            duration_ms = took.as_millis() as u64,
                            "backend message took long on the GUI thread"
                        );
                    }
                }
                TransportEvent::Disconnected(0, reason) => {
                    // HOST-04: connection 0 is the listener itself (or a connection that could
                    // not be attached). Transient accept errors are retried by the transport;
                    // what reaches this point is visible instead of silently ignored.
                    if reason.starts_with("listener:") {
                        tracing::error!(
                            reason,
                            "IPC listener stopped; this instance can no longer be reached"
                        );
                    } else {
                        tracing::warn!(reason, "incoming IPC connection could not be attached");
                    }
                }
                TransportEvent::Disconnected(id, reason) => {
                    self.connection_closed(id);
                    if self.client == Some(id) {
                        self.client = None;
                        if !matches!(
                            self.lifecycle.phase(),
                            Phase::Quitting | Phase::Stopped | Phase::Failed
                        ) {
                            self.fail(&format!("Backend connection lost: {reason}"), event_loop);
                        }
                    }
                }
            }
        }
        for _ in 0..32 {
            let Ok(result) = self.job_rx.try_recv() else {
                break;
            };
            if let Some(job) = self.jobs.remove(&result.id) {
                if !job.cancelled {
                    if let JobKind::DocumentDialog {
                        ref document_id,
                        node,
                    } = job.kind
                    {
                        if let Some(parent) = &job.parent {
                            self.complete_document_dialog(
                                parent,
                                document_id,
                                node,
                                result.value,
                                event_loop,
                            );
                        }
                    } else if let Some(reply) = job.reply {
                        self.respond(&reply, result.value);
                    } else if let Some(parent) = &job.parent {
                        if let Some(window) = self.windows.get_mut(parent) {
                            window.probe.status = match result.value {
                                Ok(value) => format!("Result: {value}").chars().take(180).collect(),
                                Err(error) => error.to_string().chars().take(180).collect(),
                            };
                        }
                    }
                }
                if let Some(parent) = job.parent {
                    let close = self.windows.get(&parent).is_some_and(|w| w.closing)
                        && !self
                            .jobs
                            .values()
                            .any(|j| j.parent.as_ref() == Some(&parent));
                    if close {
                        self.remove_window(&parent);
                    } else {
                        self.refresh(&parent);
                    }
                }
            }
        }
        for _ in 0..32 {
            let Ok(action) = self.action_rx.try_recv() else {
                break;
            };
            self.accessibility_action(action, event_loop);
        }
        let now = Instant::now();
        let expired: Vec<_> = self
            .connected
            .iter()
            .filter(|(id, at)| {
                Some(**id) != self.client && now.duration_since(**at) > Duration::from_secs(5)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in expired {
            if let Some(server) = &self.server {
                server.disconnect(id);
            }
            self.connected.remove(&id);
        }
        let expired: Vec<_> = self
            .transfers
            .iter()
            .filter(|(_, t)| now >= t.deadline)
            .map(|(id, _)| *id)
            .collect();
        for serial in expired {
            if let Some(transfer) = self.transfers.remove(&serial) {
                self.finish_transfer(
                    transfer.target,
                    Err(RpcError::timeout("Native data transfer timed out")),
                );
            }
        }
        if self
            .launcher_gone
            .as_ref()
            .is_some_and(|gone| gone.load(Ordering::SeqCst))
        {
            // The launcher was terminated (signal, closed terminal or a killed
            // test harness). This is an external termination, not an answer
            // to any pending decision: cancel those and shut down boundedly.
            self.launcher_gone = None;
            if !matches!(self.lifecycle.phase(), Phase::Quitting | Phase::Stopped) {
                self.exit_code.store(1, Ordering::Relaxed);
                self.begin_shutdown();
            }
        }
        self.send_due_decision_retries();
        let requests = self
            .termination
            .as_ref()
            .map(|t| t.take_new(&mut self.termination_seen))
            .unwrap_or_default();
        for request in requests {
            self.terminate(request);
        }
        if let Some(event_loop) = event_loop {
            for target in std::mem::take(&mut self.deferred_clipboard_reads) {
                self.read_clipboard_now(event_loop, target);
            }
        }
        self.poll_documents(event_loop);
        if let Some(backend) = &self.backend {
            crate::logging::backend_logs(backend.drain_logs());
        }
        let backend_exit = self.backend.as_mut().and_then(|b| match b.try_wait() {
            Ok(exit) => exit.map(Ok),
            Err(error) => Some(Err(error)),
        });
        if let Some(exit) = backend_exit {
            self.backend.take();
            match exit {
                Ok(status) if self.lifecycle.phase() == Phase::Quitting => {
                    if !status.success() {
                        self.exit_code.store(1, Ordering::Relaxed);
                        tracing::warn!(%status, "backend exited unsuccessfully");
                    }
                    self.stop(event_loop);
                }
                Ok(status) => self.fail(&format!("Backend exited with {status}"), event_loop),
                Err(error) => self.fail(&format!("Cannot observe backend: {error}"), event_loop),
            }
        }
        if matches!(
            self.lifecycle.phase(),
            Phase::Connecting | Phase::Initializing
        ) && self.started.elapsed() >= Duration::from_millis(self.config.host.startup_timeout_ms)
        {
            self.fail(
                "Backend did not authenticate and become ready before the startup deadline",
                event_loop,
            );
        }
        if self.lifecycle.phase() == Phase::Quitting {
            if self.backend.is_none() {
                self.stop(event_loop);
            } else if self.quit_deadline.is_some_and(|deadline| now >= deadline) {
                if let Some(backend) = &mut self.backend {
                    let _ = backend.kill();
                }
                self.exit_code.store(1, Ordering::Relaxed);
                if self.force_quit {
                    // HOST-08: a force quit is a deliberate request, not a
                    // backend that missed its deadline. The exit status stays 1.
                    tracing::warn!("force quit: backend and managed workers are being terminated");
                } else {
                    tracing::error!(
                        "backend exceeded shutdown deadline; managed workers are being terminated"
                    );
                }
                self.stop(event_loop);
            }
        }
        self.flush_document_refreshes();
        self.headless_frames(false);
        let took = turn.elapsed();
        if took > self.pump_stats.longest_turn {
            self.pump_stats.longest_turn = took;
        }
    }

    fn message(&mut self, connection: u64, value: Value, event_loop: Option<&dyn ActiveEventLoop>) {
        if value.get("method").is_none() && self.client == Some(connection) {
            match protocol::parse_response(value) {
                Ok(response) => {
                    // HOST-15: only an explicit boolean is a decision; `{"allow": false}` is
                    // the only veto. An error answer is no decision: "busy" is asked again
                    // after a backoff, anything else leaves the window open / the app running
                    // (never an implicit allow) and is reported.
                    let (allowed, problem) = match &response.outcome {
                        Ok(result) => match result.get("allow").and_then(Value::as_bool) {
                            Some(allowed) => (Some(allowed), None),
                            None => (None, Some("answer without a boolean allow".to_owned())),
                        },
                        Err(error) => (None, Some(format!("{} ({})", error.message, error.code))),
                    };
                    let busy = response
                        .outcome
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code == protocol::code::BUSY);
                    if busy
                        && self
                            .decisions
                            .get(&response.id)
                            .is_some_and(|d| d.retries < MAX_DECISION_RETRIES)
                    {
                        self.retry_decision(&response.id);
                        return;
                    }
                    let Some(allowed) = allowed else {
                        if let Ok(resolved) = self.decisions.resolve(&response.id, false) {
                            self.no_decision(resolved.decision, problem.unwrap_or_default());
                        } else {
                            tracing::debug!("ignored stale decision response");
                        }
                        return;
                    };
                    if let Ok(resolved) = self.decisions.resolve(&response.id, allowed) {
                        self.decision_requests.remove(&response.id);
                        match resolved.decision.kind {
                            DecisionKind::CloseWindow { window_id } if resolved.allowed => {
                                // A window awaiting cancellation of its native
                                // dialog is already committed to closing. It
                                // must not make the final usable window look
                                // like an ordinary non-final close.
                                if self
                                    .windows
                                    .values()
                                    .filter(|window| !window.closing)
                                    .count()
                                    <= 1
                                {
                                    let _ = self.request_quit("last_window");
                                } else {
                                    self.remove_window(&window_id);
                                }
                            }
                            DecisionKind::QuitApplication if resolved.allowed => {
                                self.begin_shutdown()
                            }
                            DecisionKind::QuitApplication => {
                                let _ = self.lifecycle.cancel_quit();
                                self.status_all("Quit cancelled. Application is still running.");
                            }
                            DecisionKind::CloseWindow { window_id } => {
                                if let Some(window) = self.windows.get_mut(&window_id) {
                                    window.probe.status =
                                        "Close cancelled. Window remains open.".into();
                                }
                                self.refresh(&window_id);
                            }
                        }
                    } else {
                        tracing::debug!("ignored stale decision response");
                    }
                }
                Err(error) => tracing::warn!(%error, "invalid backend response"),
            }
            return;
        }
        let request = match protocol::parse_request(value) {
            Ok(request) => request,
            Err(error) => {
                self.send(connection, protocol::error(None, &error));
                return;
            }
        };
        let reply = request.id.as_ref().map(|id| (connection, id.clone()));
        if request.method == "host.hello" {
            let outcome = self.hello(connection, &request.params);
            if let Some(reply) = reply {
                self.respond(&reply, outcome);
            }
            return;
        }
        // app.activate is only reachable on the current user's private,
        // desktop-session-scoped endpoint, and never controls native resources.
        if request.method == "app.activate" && self.client != Some(connection) {
            let outcome = if matches!(
                self.lifecycle.phase(),
                Phase::Starting | Phase::Connecting | Phase::Initializing
            ) {
                // The backend has not called app.ready, so it may not have
                // registered for app.activated yet. The launcher retries.
                Err(RpcError::unavailable("Primary instance is still starting"))
            } else if matches!(
                self.lifecycle.phase(),
                Phase::Quitting | Phase::Stopped | Phase::Failed
            ) {
                Err(RpcError::closed("Primary instance is shutting down"))
            } else if request.params.to_string().len() > 32 * 1024 {
                Err(RpcError::invalid_params("Activation payload too large"))
            } else {
                for record in self.windows.values() {
                    if let Some(native) = &record.native {
                        native.window.focus_window();
                        break;
                    }
                }
                self.notify("app.activated", request.params);
                Ok(json!({"accepted":true}))
            };
            if let Some(reply) = reply {
                self.respond(&reply, outcome);
            }
            return;
        }
        if self.client != Some(connection) {
            if let Some(reply) = reply {
                self.respond(
                    &reply,
                    Err(RpcError::new(
                        protocol::code::UNAUTHENTICATED,
                        "host.hello required",
                    )),
                );
            }
            return;
        }
        if let Some(ref reply) = reply {
            let duplicate = self.jobs.values().any(|j| j.reply.as_ref() == Some(reply))
                || self
                    .transfers
                    .values()
                    .any(|t| matches!(&t.target, TransferTarget::Rpc(r) if r == reply));
            if duplicate {
                // Replying to the duplicate would consume the client's future
                // for the original request. Its eventual completion would
                // then produce a second, ambiguous response under the same
                // ID. End the invalid session and cancel all native work.
                if let Some(server) = &self.server {
                    server.disconnect(connection);
                }
                self.client = None;
                self.fail(
                    "Backend reused a request ID while its original request was pending",
                    event_loop,
                );
                return;
            }
        }
        match self.dispatch(&request.method, request.params, reply.clone(), event_loop) {
            Ok(Some(result)) => {
                if let Some(reply) = reply {
                    self.respond(&reply, Ok(result));
                }
            }
            Ok(None) => {}
            Err(error) => {
                if let Some(reply) = reply {
                    self.respond(&reply, Err(error));
                }
            }
        }
    }

    /// HOST-15: the backend answered "busy" (for example a full callback queue): ask the same
    /// decision again under a new ID after an exponential backoff.
    fn retry_decision(&mut self, id: &str) {
        let Some(request) = self.decision_requests.remove(id) else {
            return;
        };
        let Ok(decision) = self.decisions.reissue(id) else {
            return;
        };
        let delay = Duration::from_millis(100 << decision.retries.min(5).saturating_sub(1));
        tracing::info!(
            retry = decision.retries,
            delay_ms = delay.as_millis() as u64,
            "backend was busy; asking the close/quit decision again"
        );
        self.decision_retries
            .push((Instant::now() + delay, decision.id.clone()));
        self.decision_requests.insert(decision.id, request);
    }

    /// Send a host -> backend policy request and remember it for a possible retry.
    fn send_decision_request(&mut self, id: &str, method: &'static str, params: Value) {
        let decisions = &self.decisions;
        self.decision_requests
            .retain(|pending, _| decisions.get(pending).is_some());
        self.decision_requests
            .insert(id.to_owned(), (method, params.clone()));
        if let Some(client) = self.client {
            self.send(
                client,
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            );
        }
    }

    fn send_due_decision_retries(&mut self) {
        if self.decision_retries.is_empty() {
            return;
        }
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.decision_retries)
            .into_iter()
            .partition(|(at, _)| *at <= now);
        self.decision_retries = later;
        for (_, id) in due {
            // A decision cancelled meanwhile (window closed, quit superseded) is not re-sent.
            if self.decisions.get(&id).is_none() {
                continue;
            }
            if let Some((method, params)) = self.decision_requests.get(&id).cloned() {
                self.send_decision_request(&id, method, params);
            }
        }
    }

    /// HOST-15: the backend gave no decision. Nothing is closed or quit (data-loss protection);
    /// the user can request it again or force the quit.
    fn no_decision(&mut self, decision: crate::lifecycle::Decision, problem: String) {
        self.decision_requests.remove(&decision.id);
        tracing::warn!(
            decision = decision.id,
            problem,
            "backend gave no close/quit decision; nothing was closed"
        );
        match decision.kind {
            DecisionKind::QuitApplication => {
                let _ = self.lifecycle.cancel_quit();
                self.status_all(
                    "The application gave no quit decision. It keeps running; quit again to retry, or press Ctrl+Shift+Q to force.",
                );
            }
            DecisionKind::CloseWindow { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.probe.status = "The application gave no close decision. The window stays open; close it again to retry.".into();
                }
                self.refresh(&window_id);
            }
        }
    }

    fn hello(&mut self, connection: u64, params: &Value) -> Result<Value, RpcError> {
        if self.client.is_some() {
            return Err(RpcError::busy("Backend is already connected"));
        }
        if self.lifecycle.phase() != Phase::Connecting {
            return Err(RpcError::invalid_state("Host no longer accepts a backend"));
        }
        if params.get("protocol_version").and_then(Value::as_u64)
            != Some(protocol::PROTOCOL_VERSION.into())
        {
            return Err(RpcError::new(
                protocol::code::VERSION_MISMATCH,
                "Protocol version 1 is required",
            ));
        }
        let supplied = params
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut different = supplied.len() ^ self.token.len();
        for (a, b) in supplied.as_bytes().iter().zip(self.token.as_bytes()) {
            different |= usize::from(a ^ b);
        }
        if different != 0 {
            return Err(RpcError::new(
                protocol::code::UNAUTHENTICATED,
                "Invalid backend credential",
            ));
        }
        self.client = Some(connection);
        self.lifecycle
            .begin_initializing()
            .map_err(|e| RpcError::invalid_state(e.to_string()))?;
        Ok(
            json!({"protocol_version":1,"host_version":env!("CARGO_PKG_VERSION"),
            "app_id":self.config.app.id,"capabilities":self.capabilities()}),
        )
    }

    fn capabilities(&self) -> Value {
        let native = !self.headless;
        let preferences = self.preferences.snapshot();
        let observed_service =
            |available: Option<bool>| if native { available } else { Some(false) };
        let feature = |implemented: bool, available: Option<bool>, scope: &str| {
            json!({
            "implemented":implemented,"available":available,
            "qualification":if implemented {"requires_current_native_evidence"}else{"out_of_scope"},"scope":scope})
        };
        let features = json!({
            "window_constraints":feature(true,Some(native),"Paired min/max CSS-pixel bounds; OS may impose additional limits"),
            "window_observation":feature(true,Some(native),"Native observed state; unavailable properties are null, separate from requested state"),
                "window_visibility":feature(cfg!(target_os="windows"),Some(native && cfg!(target_os="windows")),"Pinned Wayland backend cannot create hidden windows or show/hide existing top levels"),
                "window_focus":feature(cfg!(target_os="windows"),Some(native && cfg!(target_os="windows")),"Wayland focus is compositor-owned; use advisory attention"),
            "window_position":feature(cfg!(target_os="windows"),Some(native && cfg!(target_os="windows")),"Compositor owns Wayland top-level placement"),
            "window_restore":feature(cfg!(target_os="windows"),Some(native && cfg!(target_os="windows")),"Wayland has no un-minimize request"),
            "attention":feature(true,observed_service(if cfg!(target_os="windows") {Some(true)}else{None}),"Window-manager policy; Wayland requires xdg-activation and cannot cancel urgency"),
            "clipboard_text":feature(true,observed_service(None),"Text only; availability is established by the native request"),
            "file_dialogs":feature(true,observed_service(None),"Parented native picker; Linux requires the desktop portal"),
            "external_open":feature(true,observed_service(None),"Optional parent forwarded to OpenURI or Windows Shell; accepted is not proof of a presented app"),
            "reveal_file":feature(true,observed_service(None),"Local path selection via FileManager1.ShowItems or Windows Shell; these services have no parent argument"),
            "native_preferences":feature(true,observed_service(preferences.availability()),"Observed color scheme, reduced motion and high contrast; missing per-setting values stay null"),
            "ime":feature(true,observed_service(None),"Composition, surrounding text and candidate area depend on the active input method"),
            "ime_purpose_hints":feature(true,Some(native && cfg!(target_os="linux")),"Advisory Wayland input purpose/hints; pinned winit does not implement these hints on Windows"),
            "incoming_drop":feature(true,observed_service(None),"Bounded local file or text data, with document target policy"),
            "accessibility":feature(true,observed_service(None),"AccessKit AT-SPI or UI Automation; presence does not qualify screen-reader journeys"),
            "printing":feature(false,Some(false),"Outside the desktop core contract"),
            "tray":feature(false,Some(false),"Outside the current core contract")
        });
        let mut result = json!({"native":!self.headless,"headless":self.headless,"windows":true,"max_windows":self.config.host.max_windows,
            "clipboard_text":!self.headless,"file_dialogs":!self.headless,"external_open":!self.headless,
            "drag_drop":!self.headless,"ime":!self.headless,"accessibility":!self.headless,
            "probe":true,"html_css":true,"renderer":"cpu","css_profile":"ui4me-desktop-1.0","max_documents":documents::MAX_DOCUMENTS,"printing":false,"tray":false,
            "platform":std::env::consts::OS,"containment":platform::containment_description(),
            "limits":{"frame_bytes":protocol::MAX_FRAME_BYTES,"pending_requests":protocol::MAX_PENDING,"probe_text_bytes":crate::presentation::MAX_TEXT_BYTES},
            "window_positioning":native && cfg!(target_os="windows"),"native_qualification":"requires local evidence"});
        result["test_clock"] = json!(self.headless);
        result["capability_schema"] = json!(2);
        result["qualification"] = json!({"status":"unqualified_candidate","evidence":"No native qualification is inferred from compiled code, a previous release, or an available service"});
        result["legacy_flags"] = json!("Top-level service booleans describe enabled API surfaces; feature availability below is authoritative and null means unobserved");
        result["features"] = features;
        result["native_preferences"] = preferences.value();
        result
    }

    fn request_close(&mut self, window_id: &str, reason: &str) -> Result<(), RpcError> {
        if !self.windows.contains_key(window_id) {
            return Err(RpcError::closed("Window does not exist"));
        }
        if self
            .windows
            .get(window_id)
            .is_some_and(|window| window.closing)
        {
            return Ok(());
        }
        if matches!(
            self.lifecycle.phase(),
            Phase::Starting | Phase::Connecting | Phase::Initializing
        ) {
            // The app is not ready to answer policy callbacks yet. Closing its
            // startup window is a cancellation of startup, with the same
            // bounded cooperative shutdown used after an accepted quit.
            self.begin_shutdown();
            return Ok(());
        }
        if self.standalone || self.lifecycle.phase() == Phase::Failed {
            self.remove_window(window_id);
            if self.windows.is_empty()
                && !matches!(self.lifecycle.phase(), Phase::Quitting | Phase::Stopped)
            {
                self.begin_shutdown();
            }
            return Ok(());
        }
        if self.lifecycle.phase() != Phase::Running {
            return Err(RpcError::busy(
                "Application is not ready for a close decision",
            ));
        }
        let decision = self
            .decisions
            .begin_close(window_id)
            .map_err(|e| RpcError::busy(e.to_string()))?;
        if decision.is_new {
            if let Some(window) = self.windows.get_mut(window_id) {
                window.probe.status =
                    "Waiting for application close decision. Ctrl+Shift+Q forces shutdown.".into();
            }
            self.refresh(window_id);
            self.window_event_message(window_id, "close_requested", json!({"reason":reason}));
            self.send_decision_request(
                &decision.decision.id,
                "window.before_close",
                json!({"window_id":window_id,"reason":reason}),
            );
        }
        Ok(())
    }

    fn request_quit(&mut self, reason: &str) -> Result<(), RpcError> {
        if self.standalone || self.lifecycle.phase() == Phase::Failed {
            self.begin_shutdown();
            return Ok(());
        }
        if matches!(
            self.lifecycle.phase(),
            Phase::Starting | Phase::Connecting | Phase::Initializing
        ) {
            self.begin_shutdown();
            return Ok(());
        }
        if matches!(
            self.lifecycle.phase(),
            Phase::QuitPending | Phase::Quitting | Phase::Stopped
        ) {
            return Ok(());
        }
        self.lifecycle
            .request_quit()
            .map_err(|e| RpcError::invalid_state(e.to_string()))?;
        let start = match self.decisions.begin_quit() {
            Ok(start) => start,
            Err(error) => {
                let _ = self.lifecycle.cancel_quit();
                return Err(RpcError::busy(error.to_string()));
            }
        };
        for stale in start.superseded {
            self.notify(
                "request.cancelled",
                json!({"id":stale,"reason":"application_quit"}),
            );
        }
        self.send_decision_request(
            &start.decision.id,
            "app.before_quit",
            json!({"reason":reason}),
        );
        self.status_all("Waiting for application quit decision. Ctrl+Shift+Q forces shutdown.");
        Ok(())
    }

    /// Receive this process's termination requests (signals, console control, session end).
    fn watch_termination(&mut self, termination: Arc<crate::termination::Termination>) {
        termination.set_wake(self.wake.clone());
        if let Err(error) = platform::install_termination_handlers(termination.clone()) {
            tracing::warn!(
                error = format!("{error:#}"),
                "termination requests keep their default effect"
            );
        }
        self.termination = Some(termination);
    }

    /// HOST-07: an external termination request (signal, console control, session end) is not
    /// an answer to a pending decision: it cancels them and starts the orderly shutdown with a
    /// deadline short enough for process managers and session ends. A second termination
    /// request while already shutting down forces the quit.
    fn terminate(&mut self, request: crate::termination::Request) {
        let first = self.termination_started;
        if first.is_none() {
            self.termination_started = Some(Instant::now());
        }
        match (self.lifecycle.phase(), first) {
            (Phase::Stopped, _) => {}
            // The first termination request while an allowed quit is already running: it is
            // not a repeat, but it must still bound the (possibly long) configured deadline.
            (Phase::Quitting, None) => {
                tracing::warn!(
                    source = request.source,
                    "termination requested during shutdown; shortening the deadline"
                );
                let bound = Instant::now() + crate::termination::DEADLINE;
                self.quit_deadline = Some(self.quit_deadline.map_or(bound, |d| d.min(bound)));
                if !self.force_quit {
                    self.exit_code.store(request.exit_code, Ordering::Relaxed);
                }
            }
            // One Ctrl+C often arrives twice: the terminal signals the host's process group and
            // a direct-start launcher forwards its own SIGINT as SIGTERM a moment later. Only a
            // request after this grace period is the user insisting.
            (Phase::Quitting, Some(first))
                if first.elapsed() < crate::termination::REPEAT_GRACE =>
            {
                tracing::debug!(
                    source = request.source,
                    "repeated termination request ignored"
                );
            }
            (Phase::Quitting, Some(_)) => {
                tracing::warn!(
                    source = request.source,
                    "termination requested again; forcing shutdown"
                );
                self.force_shutdown();
            }
            (phase, _) => {
                tracing::warn!(
                    source = request.source,
                    "termination requested; shutting down"
                );
                if phase != Phase::Failed {
                    self.exit_code.store(request.exit_code, Ordering::Relaxed);
                }
                let configured = Duration::from_millis(self.config.host.shutdown_timeout_ms);
                self.begin_shutdown_within(configured.min(crate::termination::DEADLINE));
            }
        }
    }

    fn begin_shutdown(&mut self) {
        self.begin_shutdown_within(Duration::from_millis(self.config.host.shutdown_timeout_ms));
    }

    fn begin_shutdown_within(&mut self, deadline: Duration) {
        if self.lifecycle.phase() == Phase::QuitPending {
            let _ = self.lifecycle.commit_quit();
        } else {
            let _ = self.lifecycle.force_quit();
        }
        self.decisions.cancel_all();
        self.cancel_native_jobs("Application is shutting down");
        self.notify(
            "app.shutdown",
            json!({"deadline_ms":deadline.as_millis() as u64}),
        );
        self.quit_deadline = Some(Instant::now() + deadline);
        self.status_all("Shutting down application...");
    }

    fn force_shutdown(&mut self) {
        self.begin_shutdown();
        if let Some(backend) = &mut self.backend {
            let _ = backend.kill();
        }
        self.quit_deadline = Some(Instant::now());
        self.force_quit = true;
    }

    fn stop(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        if self.lifecycle.phase() == Phase::Stopped {
            return;
        }
        let _ = self.lifecycle.stop();
        if let Some(server) = &mut self.server {
            server.close();
        }
        if let Some(termination) = &self.termination {
            termination.finished();
        }
        self.cancel_native_jobs("Host stopped");
        self.native_ids.clear();
        self.windows.clear();
        if let Some(event_loop) = event_loop {
            event_loop.exit();
        }
        tracing::info!(
            exit_code = self.exit_code.load(Ordering::Relaxed),
            "host stopped"
        );
    }

    fn fail(&mut self, reason: &str, event_loop: Option<&dyn ActiveEventLoop>) {
        if matches!(self.lifecycle.phase(), Phase::Stopped | Phase::Failed) {
            return;
        }
        tracing::error!(reason, "application failed");
        let _ = self.lifecycle.fail(reason);
        self.exit_code.store(1, Ordering::Relaxed);
        self.decisions.cancel_all();
        self.cancel_native_jobs("Backend failed");
        if let Some(backend) = &mut self.backend {
            let _ = backend.kill();
        }
        if self.headless {
            self.stop(event_loop);
            return;
        }
        if self.windows.is_empty() {
            if let Err(error) = self.create_window(
                json!({"title":format!("{} — application error",self.config.app.name),
                "width":820,"height":460,"visible":true}),
                event_loop,
            ) {
                // There is no native recovery/quit surface left. Keeping the
                // event loop alive would leave an unreachable failed host.
                tracing::error!(%error, "cannot present application failure");
                self.stop(event_loop);
                return;
            }
        }
        for record in self.windows.values_mut() {
            record.document = None;
            record.probe.failure = Some(reason.chars().take(160).collect());
        }
        for id in self.windows.keys().cloned().collect::<Vec<_>>() {
            self.refresh(&id);
        }
    }

    fn status_all(&mut self, status: &str) {
        for window in self.windows.values_mut() {
            window.probe.status = status.into();
        }
        for id in self.windows.keys().cloned().collect::<Vec<_>>() {
            self.refresh(&id);
        }
    }

    fn remove_window(&mut self, id: &str) {
        self.decisions.cancel_for_window(id);
        // A late clipboard/drop completion must not modify a parent whose
        // close has already been accepted, even if a dialog defers destruction.
        self.transfers.retain(|_, pending| match &pending.target {
            TransferTarget::Paste(parent)
            | TransferTarget::Drop(parent)
            | TransferTarget::DropText(parent) => parent != id,
            TransferTarget::DocumentClipboard { window_id, .. }
            | TransferTarget::DocumentDrop { window_id, .. } => window_id != id,
            TransferTarget::Rpc(_) => true,
        });
        self.deferred_clipboard_reads.retain(|target| match target {
            TransferTarget::Paste(parent)
            | TransferTarget::Drop(parent)
            | TransferTarget::DropText(parent) => parent != id,
            TransferTarget::DocumentClipboard { window_id, .. }
            | TransferTarget::DocumentDrop { window_id, .. } => window_id != id,
            TransferTarget::Rpc(_) => true,
        });
        if self
            .jobs
            .values()
            .any(|j| j.parent.as_deref() == Some(id) && (j.kind.is_dialog() || j.dialog.is_some()))
        {
            if let Some(window) = self.windows.get_mut(id) {
                window.closing = true;
                window.probe.status = "Waiting for the native request to close.".into();
            }
            let replies: Vec<_> = self
                .jobs
                .values_mut()
                .filter(|j| j.parent.as_deref() == Some(id))
                .filter_map(|j| {
                    j.cancelled = true;
                    if let Some(dialog) = &j.dialog {
                        dialog.cancel();
                    }
                    j.reply.take()
                })
                .collect();
            for reply in replies {
                self.respond(&reply, Err(RpcError::closed("Dialog parent is closing")));
            }
            self.refresh(id);
            return;
        }
        if let Some(window) = self.windows.remove(id) {
            if let Some(native) = window.native {
                self.native_ids.remove(&native.window.id());
            }
            self.window_event_message(id, "closed", json!({}));
            if self.windows.is_empty()
                && (self.standalone || self.lifecycle.phase() == Phase::Failed)
                && !matches!(self.lifecycle.phase(), Phase::Quitting | Phase::Stopped)
            {
                // A parented dialog may have delayed this removal beyond the
                // original request_close call. Complete the lifecycle here.
                self.begin_shutdown();
            }
        }
    }

    fn refresh(&mut self, id: &str) {
        if self.windows.get(id).is_some_and(|w| w.document.is_some()) {
            self.refresh_document(id);
            return;
        }
        if let Some(record) = self.windows.get_mut(id) {
            if let Some(native) = &mut record.native {
                if native.document_epoch.take().is_some() {
                    let _ = native
                        .window
                        .request_ime_update(winit::window::ImeRequest::Disable);
                    native.ime_enabled = false;
                    native.composing = false;
                    native.preedit_cleared = false;
                    native.ime_quiet_until = Some(Instant::now() + Duration::from_millis(250));
                }
                let size = native.window.surface_size();
                native.accessibility.update(record.probe.tree(
                    &record.title,
                    size.width,
                    size.height,
                    native.window.scale_factor(),
                ));
                native::update_ime(&record.probe, native);
                native.window.request_redraw();
            }
        }
    }

    fn cancel_native_jobs(&mut self, reason: &str) {
        let replies: Vec<_> = self
            .jobs
            .values_mut()
            .filter_map(|job| {
                job.cancelled = true;
                if let Some(dialog) = &job.dialog {
                    dialog.cancel();
                }
                job.reply.take()
            })
            .collect();
        for reply in replies {
            self.respond(&reply, Err(RpcError::cancelled(reason)));
        }
        let pending = std::mem::take(&mut self.transfers);
        for transfer in pending.into_values() {
            if let TransferTarget::Rpc(reply) = transfer.target {
                self.respond(&reply, Err(RpcError::cancelled(reason)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A controller with no transport/process/native resources. This exercises
    /// lifecycle policy independently of desktop or IPC availability.
    pub(super) fn controller(ready: bool) -> Host {
        let (job_tx, job_rx) = mpsc::sync_channel(protocol::MAX_PENDING);
        let (action_tx, action_rx) = mpsc::sync_channel(128);
        let mut lifecycle = Lifecycle::new();
        lifecycle.begin_connecting().unwrap();
        lifecycle.begin_initializing().unwrap();
        if ready {
            lifecycle.ready().unwrap();
        }
        Host {
            icon: crate::icons::load(None).unwrap(),
            config: AppConfig {
                app: AppInfo {
                    id: "org.ui4me.ControllerTest".into(),
                    name: "Controller".into(),
                    version: None,
                    icon: None,
                },
                backend: BackendConfig {
                    program: PathBuf::new(),
                    args: Vec::new(),
                    cwd: PathBuf::new(),
                    env: BTreeMap::new(),
                },
                host: HostConfig::default(),
            },
            headless: true,
            standalone: false,
            lifecycle,
            decisions: DecisionTracker::default(),
            server: None,
            backend: None,
            _instance: None,
            client: Some(7),
            connected: HashMap::new(),
            undeliverable: HashMap::new(),
            token: String::new(),
            wake: Arc::new(|| {}),
            windows: BTreeMap::new(),
            native_ids: HashMap::new(),
            next_window: 1,
            started: Instant::now(),
            quit_deadline: None,
            force_quit: false,
            next_job: 1,
            jobs: HashMap::new(),
            job_tx,
            job_rx,
            action_tx,
            action_rx,
            transfers: HashMap::new(),
            clipboard_written: None,
            deferred_clipboard_reads: Vec::new(),
            exit_code: Arc::new(AtomicI32::new(0)),
            launcher_gone: None,
            presentation: None,
            preferences: platform::preferences::PreferenceWatch::new(false, Arc::new(|| {})),
            preference_revision: 0,
            virtual_time_ms: None,
            termination: None,
            termination_seen: 0,
            termination_started: None,
            decision_requests: HashMap::new(),
            decision_retries: Vec::new(),
            pump_more: false,
            pump_stats: PumpStats::default(),
            #[cfg(test)]
            observed_notifications: Vec::new(),
        }
    }

    pub(super) fn add_window(host: &mut Host, id: &str) {
        host.windows.insert(
            id.into(),
            WindowRecord {
                title: id.into(),
                width: 800,
                height: 500,
                visible: true,
                resizable: true,
                decorations: true,
                limits: requests::WindowLimits::default(),
                probe: Probe::default(),
                document: None,
                native: None,
                closing: false,
            },
        );
    }

    /// HOST-08: after the first refused message the connection is marked dead
    /// and disconnected once; the rest of the burst is only counted.
    #[test]
    fn refused_output_disconnects_once_and_counts_the_rest() {
        let app = format!("org.ui4me.host08.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut host = controller(true);
        let server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        // A peer that never reads.
        let _peer =
            crate::platform::NativeStream::connect(&server.endpoint, Duration::from_secs(5))
                .unwrap();
        let connection = match server
            .incoming
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        {
            TransportEvent::Connected(id) => id,
            other => panic!("unexpected {other:?}"),
        };
        host.server = Some(server);
        host.connected.insert(connection, Instant::now());
        host.client = Some(connection);
        let payload = "x".repeat(256 * 1024);
        let mut sent = 0;
        while !host.undeliverable.contains_key(&connection) {
            host.notify("test.fill", json!({"payload": payload}));
            sent += 1;
            assert!(sent < 1000, "the byte cap never refused output");
        }
        assert!(sent > 24, "refused well before the 8 MiB budget: {sent}");
        for _ in 0..500 {
            host.notify("document.event", json!({"type":"transitionrun"}));
        }
        assert_eq!(host.undeliverable[&connection], 500);
        let server = host.server.as_ref().unwrap();
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
        host.connection_closed(connection);
        assert!(host.undeliverable.is_empty());
        host.notify("document.event", json!({"type":"late"}));
        assert!(
            host.undeliverable.is_empty(),
            "closed connection is not retried"
        );
        host.server.as_mut().unwrap().close();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_unminimize_is_rejected_instead_of_reporting_success() {
        let mut host = controller(true);
        add_window(&mut host, "one");
        let error = host
            .set_window(json!({"window_id":"one","minimized":false,"title":"changed"}))
            .unwrap_err();
        assert_eq!(error.code, protocol::code::UNSUPPORTED);
        assert_eq!(
            host.windows["one"].title, "one",
            "rejected request changes nothing"
        );
        assert!(host
            .set_window(json!({"window_id":"one","minimized":true}))
            .is_ok());
    }

    #[test]
    fn launcher_exit_during_pending_quit_shuts_down_without_approving_it() {
        let mut host = controller(true);
        add_window(&mut host, "one");
        host.request_quit("native_close").unwrap();
        assert_eq!(host.lifecycle.phase(), Phase::QuitPending);
        assert_eq!(host.decisions.pending_count(), 1);
        let gone = Arc::new(AtomicBool::new(false));
        host.launcher_gone = Some(gone.clone());
        host.pump(None);
        assert_eq!(
            host.lifecycle.phase(),
            Phase::QuitPending,
            "launcher still alive"
        );
        gone.store(true, Ordering::SeqCst);
        host.pump(None);
        // No backend remains in this fixture, so the bounded shutdown completes.
        assert_eq!(host.lifecycle.phase(), Phase::Stopped);
        assert_eq!(host.decisions.pending_count(), 0);
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 1);
    }

    /// HOST-07 (T15): a termination request cancels a pending decision, delivers
    /// `app.shutdown` with the short deadline and sets the 128 + signal exit status; a second
    /// request while shutting down forces the quit.
    #[test]
    fn termination_request_starts_an_orderly_shutdown_and_a_second_forces_it() {
        let mut host = controller(true);
        add_window(&mut host, "one");
        let termination = Arc::new(crate::termination::Termination::default());
        host.termination = Some(termination.clone());
        host.request_quit("backend_request").unwrap();
        assert_eq!(host.decisions.pending_count(), 1);
        // Keep the fixture in Quitting: a live backend has not exited yet.
        host.quit_deadline = None;
        termination.request(crate::termination::Request {
            source: "SIGTERM",
            exit_code: 143,
        });
        host.observed_notifications.clear();
        let before = Instant::now();
        host.pump(None);
        // No backend in this fixture, so the orderly shutdown completes at once.
        assert_eq!(host.lifecycle.phase(), Phase::Stopped);
        assert_eq!(host.decisions.pending_count(), 0);
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 143);
        let shutdown: Vec<_> = host
            .observed_notifications
            .iter()
            .filter(|n| n["method"] == "app.shutdown")
            .collect();
        assert_eq!(shutdown.len(), 1);
        assert_eq!(shutdown[0]["params"]["deadline_ms"], 3000);
        assert!(host.quit_deadline.unwrap() <= before + Duration::from_millis(3100));

        let mut host = controller(true);
        let termination = Arc::new(crate::termination::Termination::default());
        host.termination = Some(termination.clone());
        host.begin_shutdown();
        assert_eq!(host.lifecycle.phase(), Phase::Quitting);
        termination.request(crate::termination::Request {
            source: "SIGINT",
            exit_code: 130,
        });
        let request = termination.take_new(&mut host.termination_seen).remove(0);
        host.terminate(request.clone());
        assert!(
            !host.force_quit,
            "an immediate repeat (terminal + launcher) is the same request"
        );
        host.termination_started = Some(Instant::now() - crate::termination::REPEAT_GRACE);
        host.terminate(request);
        assert!(host.force_quit, "a later second request forces the quit");
    }

    /// F-25 review #1: the first termination request while an allowed quit is already running
    /// (with a long `shutdown_timeout_ms`) is not an "immediate repeat": it clamps the deadline
    /// to the termination deadline and records its exit status. A repeat of that termination
    /// request within the grace period is still not a force quit.
    #[test]
    fn first_termination_request_during_a_long_shutdown_clamps_the_deadline() {
        let mut host = controller(true);
        host.config.host.shutdown_timeout_ms = 60_000;
        let termination = Arc::new(crate::termination::Termination::default());
        host.termination = Some(termination.clone());
        host.begin_shutdown();
        assert_eq!(host.lifecycle.phase(), Phase::Quitting);
        assert!(host.quit_deadline.unwrap() > Instant::now() + Duration::from_secs(50));
        let request = crate::termination::Request {
            source: "SIGTERM",
            exit_code: 143,
        };
        let before = Instant::now();
        host.terminate(request.clone());
        let deadline = host.quit_deadline.unwrap();
        assert!(
            deadline <= before + crate::termination::DEADLINE + Duration::from_millis(100),
            "deadline clamped to the termination deadline"
        );
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 143);
        assert!(!host.force_quit);
        host.terminate(request);
        assert!(!host.force_quit, "an immediate repeat is the same request");
        assert_eq!(host.quit_deadline.unwrap(), deadline);
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 143);
    }

    /// F-25 review #2: a primary that accepts the connection but does not answer in time (its
    /// GUI thread stalls) is not "ending". Retrying would queue one `app.activate` per attempt
    /// in the primary, so the backend would get a burst of `app.activated` for one launch. The
    /// launch fails instead, and the primary has received exactly one request.
    #[test]
    fn an_unanswering_primary_receives_one_activation_per_launch() {
        let app = format!("org.ui4me.forward.stalled.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        assert!(lock.primary);
        let server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let mut config = controller(true).config.clone();
        config.app.id = app;
        config.host.instance = InstancePolicy::Single;
        config.host.startup_timeout_ms = 1_000;
        config.host.shutdown_timeout_ms = 1_000;
        let result = primary_or_forward_within(&config, Duration::from_millis(300));
        assert!(result.is_err(), "an unanswered launch fails");
        let mut activations = 0;
        while let Ok(event) = server.incoming.recv_timeout(Duration::from_millis(300)) {
            if let TransportEvent::Message(_, value) = event {
                if value["method"] == "app.activate" {
                    activations += 1;
                }
            }
        }
        assert_eq!(activations, 1, "one launch, one activation request");
        drop(server);
        drop(lock);
    }

    /// F-25 review #2: what counts as "the primary is ending" (HOST-10): its `-32004` answer,
    /// an endpoint that cannot be connected to, or a connection it closed before answering.
    #[test]
    fn only_closed_answers_and_lost_endpoints_mean_the_primary_is_ending() {
        use crate::transport::EndpointLost;
        use std::io::{Error, ErrorKind};
        let ending = |e: anyhow::Error| primary_is_ending(&e);
        assert!(ending(
            RpcError::new(protocol::code::CLOSED, "closing").into()
        ));
        assert!(!ending(RpcError::invalid_params("bad").into()));
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::ConnectionRefused,
            ErrorKind::TimedOut,
        ] {
            let lost = EndpointLost::Connect(Error::from(kind));
            assert!(
                ending(anyhow::Error::new(lost).context("connect")),
                "{kind:?}"
            );
        }
        let denied = EndpointLost::Connect(Error::from(ErrorKind::PermissionDenied));
        assert!(!ending(anyhow::Error::new(denied)));
        assert!(ending(anyhow::Error::new(EndpointLost::Disconnected(
            "reset".into()
        ))));
        // A response timeout on an established connection.
        assert!(!ending(anyhow::Error::new(Error::from(
            ErrorKind::WouldBlock
        ))));
        assert!(!ending(anyhow::anyhow!(
            "existing instance response timed out"
        )));
    }

    /// HOST-15 (T15): an error answer to a policy request is no decision, never a veto and
    /// never an implicit allow. "Busy" is asked again after a backoff; other errors leave the
    /// window open / the application running; only `{"allow": false}` vetoes.
    #[test]
    fn error_answers_to_policy_requests_are_no_decision() {
        let mut host = controller(true);
        add_window(&mut host, "one");
        add_window(&mut host, "two");
        let pending = |host: &Host| host.decision_requests.keys().next().cloned().unwrap();
        host.request_close("one", "native_close").unwrap();
        let first = pending(&host);
        host.message(
            7,
            protocol::error(Some(&first), &RpcError::busy("Callback queue is full")),
            None,
        );
        assert!(host.windows.contains_key("one"), "busy is not an allow");
        assert_eq!(
            host.decisions.pending_count(),
            1,
            "busy is not a veto either"
        );
        assert_eq!(host.decision_retries.len(), 1);
        let retried = pending(&host);
        assert_ne!(retried, first, "asked again under a new ID");
        // The late answer to the old ID is stale.
        host.message(7, protocol::success(&first, json!({"allow":true})), None);
        assert!(host.windows.contains_key("one"));
        // Backoff elapsed: the retry is sent (no transport in this fixture) and stays pending.
        host.decision_retries[0].0 = Instant::now();
        host.pump(None);
        assert!(host.decision_retries.is_empty());
        assert_eq!(host.decisions.pending_count(), 1);
        // A failing policy callback: no decision, the window stays open, nothing pending.
        host.message(
            7,
            protocol::error(
                Some(&retried),
                &RpcError::new(protocol::code::INTERNAL_ERROR, "Callback failed"),
            ),
            None,
        );
        assert!(host.windows.contains_key("one"));
        assert_eq!(host.decisions.pending_count(), 0);
        assert!(host.windows["one"]
            .probe
            .status
            .contains("no close decision"));
        // Asking again works, and an explicit allow closes.
        host.request_close("one", "native_close").unwrap();
        let again = pending(&host);
        host.message(7, protocol::success(&again, json!({"allow":true})), None);
        assert!(!host.windows.contains_key("one"));

        // Busy beyond the retry budget is no decision as well.
        host.request_close("two", "native_close").unwrap();
        for _ in 0..=MAX_DECISION_RETRIES {
            let id = pending(&host);
            host.message(7, protocol::error(Some(&id), &RpcError::busy("full")), None);
        }
        assert!(host.windows.contains_key("two"));
        assert_eq!(host.decisions.pending_count(), 0);

        // Quit: an error answer keeps the application running (no veto status, no shutdown).
        host.request_quit("backend_request").unwrap();
        assert_eq!(host.lifecycle.phase(), Phase::QuitPending);
        let quit = pending(&host);
        host.message(
            7,
            protocol::error(
                Some(&quit),
                &RpcError::new(protocol::code::INTERNAL_ERROR, "x"),
            ),
            None,
        );
        assert_eq!(host.lifecycle.phase(), Phase::Running);
        // A malformed answer is no decision too; an explicit false still vetoes.
        host.request_quit("backend_request").unwrap();
        let quit = pending(&host);
        host.message(7, protocol::success(&quit, json!({"allow":"yes"})), None);
        assert_eq!(host.lifecycle.phase(), Phase::Running);
        host.request_quit("backend_request").unwrap();
        let quit = pending(&host);
        host.message(7, protocol::success(&quit, json!({"allow":false})), None);
        assert_eq!(host.lifecycle.phase(), Phase::Running);
        assert!(host.windows["two"].probe.status.contains("Quit cancelled"));
    }

    /// HOST-05 (T15): a pump that stops at its message budget re-wakes the loop for the rest
    /// instead of leaving it queued until the next timer or input event.
    #[test]
    fn pump_budget_yields_and_rewakes_until_the_queue_is_drained() {
        let app = format!("org.ui4me.host05.{}", std::process::id());
        let lock = crate::platform::InstanceLock::acquire(&app).unwrap();
        let mut host = controller(true);
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = wakes.clone();
        host.wake = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let server = Server::bind(&app, &lock.instance_key, Arc::new(|| {})).unwrap();
        let mut peer =
            crate::platform::NativeStream::connect(&server.endpoint, Duration::from_secs(5))
                .unwrap();
        host.server = Some(server);
        let total = 3 * PUMP_MESSAGES + 10;
        for _ in 0..total {
            protocol::write_frame(&mut peer, &protocol::notification("test.noop", json!({})))
                .unwrap();
        }
        // Let the reader thread queue everything (Connected + messages) before the first
        // pump; a pump racing the reader drains fewer than PUMP_MESSAGES and never yields.
        std::thread::sleep(Duration::from_millis(500));
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut pumps = 0;
        let mut seen = 0;
        while Instant::now() < deadline {
            let before = wakes.load(Ordering::SeqCst);
            host.pump(None);
            pumps += 1;
            if host.pump_more {
                assert_eq!(wakes.load(Ordering::SeqCst), before + 1, "a yield re-wakes");
                seen += 1;
            } else if seen >= 3 {
                break;
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(
            seen >= 3,
            "at least three budget yields for {total} messages"
        );
        assert!(pumps < 100);
        assert_eq!(
            slow_command_name(&json!({"method":"document.command",
                "params":{"command":{"op":"request_submit"}}})),
            "document.command request_submit"
        );
        assert_eq!(
            slow_command_name(&json!({"id":"x","result":{}})),
            "response"
        );
        host.server.as_mut().unwrap().close();
    }

    /// HOST-11: no part of the secret token becomes a public endpoint name.
    #[test]
    fn multiple_instance_endpoint_key_is_independent_of_the_token() {
        let key = multiple_instance_key().unwrap();
        assert_eq!(key.len(), 24);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(key, multiple_instance_key().unwrap());
        let token = random_token().unwrap();
        assert!(!token.contains(&key));
        assert!(crate::platform::endpoint_for(&key).is_ok());
    }

    #[test]
    fn startup_window_close_cancels_startup_without_a_policy_callback() {
        let mut host = controller(false);
        add_window(&mut host, "one");
        host.request_close("one", "native_close").unwrap();
        assert_eq!(host.lifecycle.phase(), Phase::Quitting);
        assert_eq!(host.decisions.pending_count(), 0);
        assert!(host.quit_deadline.is_some());
    }

    #[test]
    fn deferred_dialog_close_cannot_hide_the_final_usable_window() {
        let mut host = controller(true);
        add_window(&mut host, "one");
        add_window(&mut host, "two");
        host.jobs.insert(
            1,
            PendingJob {
                reply: None,
                parent: Some("one".into()),
                kind: JobKind::Dialog,
                cancelled: false,
                dialog: None,
            },
        );
        let first = host.decisions.begin_close("one").unwrap().decision;
        host.message(7, protocol::success(&first.id, json!({"allow":true})), None);
        assert!(host.windows["one"].closing);
        assert!(host.jobs[&1].cancelled);
        let final_close = host.decisions.begin_close("two").unwrap().decision;
        host.message(
            7,
            protocol::success(&final_close.id, json!({"allow":true})),
            None,
        );
        assert_eq!(host.lifecycle.phase(), Phase::QuitPending);
        assert!(!host.windows["two"].closing);
        let quit = host.decisions.begin_quit().unwrap().decision;
        host.message(7, protocol::success(&quit.id, json!({"allow":false})), None);
        assert!(host
            .job_tx
            .send(JobResult {
                id: 1,
                value: Ok(json!(null))
            })
            .is_ok());
        host.pump(None);
        assert_eq!(host.lifecycle.phase(), Phase::Running);
        assert_eq!(host.windows.len(), 1);
        assert!(host.windows.contains_key("two"));
    }

    #[test]
    fn deferred_final_probe_close_completes_the_host_lifecycle() {
        let mut host = controller(true);
        host.standalone = true;
        add_window(&mut host, "one");
        host.jobs.insert(
            1,
            PendingJob {
                reply: None,
                parent: Some("one".into()),
                kind: JobKind::Dialog,
                cancelled: false,
                dialog: None,
            },
        );
        host.request_close("one", "native_close").unwrap();
        assert!(host.windows["one"].closing);
        assert!(host
            .job_tx
            .send(JobResult {
                id: 1,
                value: Ok(json!(null))
            })
            .is_ok());
        host.pump(None);
        assert_eq!(host.lifecycle.phase(), Phase::Stopped);
        assert!(host.windows.is_empty());
    }

    #[test]
    fn duplicate_pending_id_fails_the_session_and_cancels_original_work() {
        let mut host = controller(true);
        host.jobs.insert(
            1,
            PendingJob {
                reply: Some((7, "duplicate".into())),
                parent: None,
                kind: JobKind::External,
                cancelled: false,
                dialog: None,
            },
        );
        host.message(
            7,
            protocol::request("duplicate", "host.diagnostics", json!({})),
            None,
        );
        assert_eq!(host.lifecycle.phase(), Phase::Stopped);
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 1);
        assert!(host.client.is_none());
        assert!(host.jobs[&1].cancelled);
        assert!(host.jobs[&1].reply.is_none());
    }

    #[test]
    fn native_failure_without_a_diagnostic_surface_stops_instead_of_hanging() {
        let mut host = controller(false);
        host.headless = false;
        host.fail("Backend did not start", None);
        assert_eq!(host.lifecycle.phase(), Phase::Stopped);
        assert_eq!(host.exit_code.load(Ordering::Relaxed), 1);
        assert!(host.windows.is_empty());
    }

    /// Redraws re-derive the IME state; only a real change may be sent to the input method
    /// (an unchanged resend made fcitx5 answer with its pending dead-key preedit forever).
    #[test]
    fn ime_state_ignores_subpixel_jitter_but_not_real_changes() {
        let base = ImeState::new("ab", 1, 1, (10.2, 20.0, 2.0, 26.0), false);
        assert_eq!(
            base,
            ImeState::new("ab", 1, 1, (10.4, 19.8, 2.0, 26.0), false)
        );
        assert_ne!(
            base,
            ImeState::new("ab", 2, 2, (10.2, 20.0, 2.0, 26.0), false)
        );
        assert_ne!(
            base,
            ImeState::new("abc", 1, 1, (10.2, 20.0, 2.0, 26.0), false)
        );
        assert_ne!(
            base,
            ImeState::new("ab", 1, 1, (30.0, 20.0, 2.0, 26.0), false)
        );
        assert_ne!(
            base,
            ImeState::new("ab", 1, 1, (10.2, 20.0, 2.0, 26.0), true)
        );
    }
}
