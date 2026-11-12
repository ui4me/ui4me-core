//! Host controller. The GUI thread owns every window and policy transition.
//! Workers can only send bounded messages back to this controller.
mod native;
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
}

struct WindowRecord {
    title: String,
    width: u32,
    height: u32,
    visible: bool,
    probe: Probe,
    native: Option<NativeWindow>,
    closing: bool,
}

enum JobKind {
    Dialog,
    External,
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
}
struct PendingTransfer {
    target: TransferTarget,
    deadline: Instant,
}

pub struct Host {
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
    token: String,
    wake: Wake,
    windows: BTreeMap<String, WindowRecord>,
    native_ids: HashMap<WindowId, String>,
    next_window: u64,
    started: Instant,
    quit_deadline: Option<Instant>,
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
}

/// Launch a manifest, forwarding secondary launches to the existing instance.
pub fn run(config: AppConfig, headless: bool) -> Result<i32> {
    platform::initialize_application(&config.app.id)?;
    let instance = if config.host.instance == InstancePolicy::Single {
        Some(InstanceLock::acquire(&config.app.id)?)
    } else {
        None
    };
    if let Some(lock) = &instance {
        if !lock.primary {
            forward_activation(&lock.endpoint, &config)?;
            return Ok(0);
        }
    }
    let result = Arc::new(AtomicI32::new(0));
    if headless {
        let mut host = Host::new(
            config,
            true,
            false,
            instance,
            Arc::new(|| {}),
            result.clone(),
        )?;
        while host.lifecycle.phase() != Phase::Stopped {
            host.pump(None);
            std::thread::sleep(Duration::from_millis(5));
        }
    } else {
        let event_loop = EventLoop::new().context(
            "cannot create native event loop (Wayland or interactive Windows desktop required)",
        )?;
        let proxy = event_loop.create_proxy();
        let wake: Wake = Arc::new(move || proxy.wake_up());
        let host = Host::new(config, false, false, instance, wake, result.clone())?;
        event_loop.run_app(host)?;
    }
    Ok(result.load(Ordering::Relaxed))
}

/// Deliver this launch to the primary instance. A primary that is still
/// starting (for example two simultaneous launches) cannot yet deliver the
/// activation to a ready backend, so retry within the manifest's bounded
/// startup deadline instead of losing the user's launch.
fn forward_activation(endpoint: &str, config: &AppConfig) -> Result<()> {
    let request = json!({"jsonrpc":"2.0","id":"activation","method":"app.activate",
        "params":{"args":std::env::args().collect::<Vec<_>>(),"cwd":std::env::current_dir()?,
            "activation_token":std::env::var("XDG_ACTIVATION_TOKEN").ok()}});
    let deadline = Instant::now()
        + Duration::from_millis(config.host.startup_timeout_ms)
        + Duration::from_secs(5);
    loop {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(100), Duration::from_secs(5));
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
    let event_loop = EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    let result = Arc::new(AtomicI32::new(0));
    let host = Host::new(
        config,
        false,
        true,
        None,
        Arc::new(move || proxy.wake_up()),
        result.clone(),
    )?;
    event_loop.run_app(host)?;
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
        "native_tests":"not run by doctor","renderer":"CPU integration probe; HTML/CSS renderer not part of v0.1",
        "windowing_dependency":"winit 0.31.0-beta.4 (exactly pinned)"})
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
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
        let icon = crate::icons::load(config.app.icon.as_deref())?;
        let token = random_token()?;
        let instance_key = instance
            .as_ref()
            .map(|i| i.instance_key.clone())
            .unwrap_or_else(|| token[..24].to_owned());
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
            token,
            wake,
            windows: BTreeMap::new(),
            native_ids: HashMap::new(),
            next_window: 1,
            started: Instant::now(),
            quit_deadline: None,
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
        })
    }

    fn send(&mut self, connection: u64, value: Value) {
        if let Some(server) = &self.server {
            if let Err(error) = server.reply(connection, value) {
                tracing::warn!(%error, "peer cannot receive bounded output");
                server.disconnect(connection);
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
            self.send(client, message);
        }
    }
    fn window_event_message(&mut self, id: &str, kind: &str, data: Value) {
        self.notify(
            "window.event",
            json!({"window_id":id,"type":kind,"data":data}),
        );
    }

    fn pump(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        // An input/log flood must not monopolize native event dispatch.
        for _ in 0..64 {
            let event = self
                .server
                .as_ref()
                .and_then(|s| s.incoming.try_recv().ok());
            let Some(event) = event else { break };
            match event {
                TransportEvent::Connected(id) => {
                    self.connected.insert(id, Instant::now());
                }
                TransportEvent::Message(id, value) => self.message(id, value, event_loop),
                TransportEvent::Disconnected(id, reason) => {
                    self.connected.remove(&id);
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
                    if let Some(reply) = job.reply {
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
        if let Some(event_loop) = event_loop {
            for target in std::mem::take(&mut self.deferred_clipboard_reads) {
                self.read_clipboard_now(event_loop, target);
            }
        }
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
                tracing::error!(
                    "backend exceeded shutdown deadline; managed workers are being terminated"
                );
                self.stop(event_loop);
            }
        }
    }

    fn message(&mut self, connection: u64, value: Value, event_loop: Option<&dyn ActiveEventLoop>) {
        if value.get("method").is_none() && self.client == Some(connection) {
            match protocol::parse_response(value) {
                Ok(response) => {
                    let allowed = response
                        .outcome
                        .as_ref()
                        .ok()
                        .and_then(|v| v.get("allow"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if let Ok(resolved) = self.decisions.resolve(&response.id, allowed) {
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
        json!({"native":!self.headless,"headless":self.headless,"windows":true,"max_windows":self.config.host.max_windows,
            "clipboard_text":!self.headless,"file_dialogs":!self.headless,"external_open":!self.headless,
            "drag_drop":!self.headless,"ime":!self.headless,"accessibility":!self.headless,
            "probe":true,"html_css":false,"printing":false,"tray":false,
            "platform":std::env::consts::OS,"containment":platform::containment_description(),
            "limits":{"frame_bytes":protocol::MAX_FRAME_BYTES,"pending_requests":protocol::MAX_PENDING,"probe_text_bytes":crate::presentation::MAX_TEXT_BYTES},
            "window_positioning":cfg!(target_os="windows"),"native_qualification":"requires local evidence"})
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
            if let Some(client) = self.client {
                self.send(
                    client,
                    json!({"jsonrpc":"2.0","id":decision.decision.id,"method":"window.before_close",
                    "params":{"window_id":window_id,"reason":reason}}),
                );
            }
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
        if let Some(client) = self.client {
            self.send(client, json!({"jsonrpc":"2.0","id":start.decision.id,"method":"app.before_quit","params":{"reason":reason}}));
        }
        self.status_all("Waiting for application quit decision. Ctrl+Shift+Q forces shutdown.");
        Ok(())
    }

    fn begin_shutdown(&mut self) {
        if self.lifecycle.phase() == Phase::QuitPending {
            let _ = self.lifecycle.commit_quit();
        } else {
            let _ = self.lifecycle.force_quit();
        }
        self.decisions.cancel_all();
        self.cancel_native_jobs("Application is shutting down");
        self.notify(
            "app.shutdown",
            json!({"deadline_ms":self.config.host.shutdown_timeout_ms}),
        );
        self.quit_deadline =
            Some(Instant::now() + Duration::from_millis(self.config.host.shutdown_timeout_ms));
        self.status_all("Shutting down application...");
    }

    fn force_shutdown(&mut self) {
        self.begin_shutdown();
        if let Some(backend) = &mut self.backend {
            let _ = backend.kill();
        }
        self.quit_deadline = Some(Instant::now());
    }

    fn stop(&mut self, event_loop: Option<&dyn ActiveEventLoop>) {
        if self.lifecycle.phase() == Phase::Stopped {
            return;
        }
        let _ = self.lifecycle.stop();
        if let Some(server) = &mut self.server {
            server.close();
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
            TransferTarget::Rpc(_) => true,
        });
        self.deferred_clipboard_reads.retain(|target| match target {
            TransferTarget::Paste(parent)
            | TransferTarget::Drop(parent)
            | TransferTarget::DropText(parent) => parent != id,
            TransferTarget::Rpc(_) => true,
        });
        if self
            .jobs
            .values()
            .any(|j| j.parent.as_deref() == Some(id) && matches!(j.kind, JobKind::Dialog))
        {
            if let Some(window) = self.windows.get_mut(id) {
                window.closing = true;
                window.probe.status = "Waiting for the native dialog to close.".into();
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
        if let Some(record) = self.windows.get_mut(id) {
            if let Some(native) = &mut record.native {
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
    fn controller(ready: bool) -> Host {
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
            token: String::new(),
            wake: Arc::new(|| {}),
            windows: BTreeMap::new(),
            native_ids: HashMap::new(),
            next_window: 1,
            started: Instant::now(),
            quit_deadline: None,
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
        }
    }

    fn add_window(host: &mut Host, id: &str) {
        host.windows.insert(
            id.into(),
            WindowRecord {
                title: id.into(),
                width: 800,
                height: 500,
                visible: true,
                probe: Probe::default(),
                native: None,
                closing: false,
            },
        );
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
}
