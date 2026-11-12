use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct InstanceLock {
    pub instance_key: String,
    pub primary: bool,
    pub endpoint: String,
    _file: File,
}

impl InstanceLock {
    pub fn acquire(app_id: &str) -> Result<Self> {
        // Preserve the original desktop realm when systemd launches the host.
        let realm = std::env::var("UI4ME_DESKTOP_REALM").unwrap_or_else(|_| desktop_realm());
        let key = super::namespace_hash(format!("{app_id}\0{realm}").as_bytes());
        let directory = runtime_directory()?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(directory.join(format!("{key}.lock")))?;
        if file.metadata()?.uid() != unsafe { libc::geteuid() } {
            bail!("instance lock is not owned by this user");
        }
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        let primary = if rc == 0 {
            true
        } else {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error.into());
            }
            false
        };
        let endpoint = endpoint_for(&key)?;
        Ok(Self {
            instance_key: key,
            primary,
            endpoint,
            _file: file,
        })
    }
}

fn desktop_realm() -> String {
    let uid = unsafe { libc::geteuid() };
    let base = runtime_base(uid);
    let display = std::env::var_os("WAYLAND_DISPLAY");
    realm_for_compositor(
        uid,
        &base,
        display.as_deref(),
        std::env::var_os("UI4ME_TEST_MODE").is_some(),
    )
}

fn runtime_base(uid: u32) -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("ui4me-runtime-{uid}")))
}

fn realm_for_compositor(
    uid: u32,
    runtime: &Path,
    display: Option<&OsStr>,
    headless: bool,
) -> String {
    let runtime = runtime
        .canonicalize()
        .unwrap_or_else(|_| runtime.to_owned());
    if headless {
        return format!("uid={uid};headless;runtime={runtime:?}");
    }
    let display = Path::new(display.unwrap_or_else(|| OsStr::new("wayland-0")));
    let path = if display.is_absolute() {
        display.to_owned()
    } else {
        runtime.join(display)
    };
    let path = path.canonicalize().unwrap_or(path);
    let identity = std::fs::metadata(&path)
        .map(|metadata| format!("{}:{}", metadata.dev(), metadata.ino()))
        .unwrap_or_else(|_| "not-present".into());
    // XDG_SESSION_ID may be absent in desktop/systemd activation but present
    // in a terminal attached to the very same compositor. Its socket identity
    // is the stable realm shared by both launch routes; inode changes separate
    // compositor restarts even when WAYLAND_DISPLAY is reused.
    format!("uid={uid};wayland={path:?};identity={identity}")
}

pub fn runtime_directory() -> Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    let base = runtime_base(uid);
    if !base.is_absolute() {
        bail!("runtime directory must be an absolute path");
    }
    ensure_private_directory(&base)?;
    let path = base.join("ui4me-host");
    ensure_private_directory(&path)?;
    Ok(path)
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!(
            "private runtime directory has unsafe ownership or permissions: {}",
            path.display()
        );
    }
    Ok(())
}

pub fn endpoint_for(instance_key: &str) -> Result<String> {
    if instance_key.is_empty() || !instance_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid socket instance key");
    }
    let path = runtime_directory()?.join(format!("{instance_key}.sock"));
    let value = path.to_str().context("runtime socket path is not UTF-8")?;
    if value.len() >= 104 {
        bail!("runtime directory is too long for a Unix-domain socket");
    }
    Ok(value.to_owned())
}

pub struct NativeListener {
    listener: UnixListener,
    path: PathBuf,
}

impl NativeListener {
    /// Caller must hold the primary InstanceLock for this endpoint.
    pub fn bind(endpoint: &str) -> Result<Self> {
        let path = PathBuf::from(endpoint);
        if path.exists() {
            let metadata = std::fs::symlink_metadata(&path)?;
            use std::os::unix::fs::FileTypeExt;
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                bail!("refusing to replace a non-owned or non-socket endpoint");
            }
            // A live listener must never be stolen, including accidental misuse
            // of Server::bind by a caller that did not obtain the instance lock.
            match NativeStream::connect_with_retry(endpoint, Duration::from_millis(200), false) {
                Ok(_) => bail!("instance endpoint is already active"),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) => {}
                Err(error) => {
                    return Err(error).context("cannot prove that the old endpoint is stale");
                }
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener, path })
    }

    pub fn accept(&mut self, stop: &AtomicBool) -> io::Result<Option<NativeStream>> {
        while !stop.load(Ordering::Acquire) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    return Ok(Some(NativeStream {
                        stream,
                        deadline: None,
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
}

impl Drop for NativeListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct NativeStream {
    stream: UnixStream,
    deadline: Option<Instant>,
}

impl NativeStream {
    pub fn connect(endpoint: &str, timeout: Duration) -> io::Result<Self> {
        Self::connect_with_retry(endpoint, timeout, true)
    }
    fn connect_with_retry(
        endpoint: &str,
        timeout: Duration,
        retry_missing: bool,
    ) -> io::Result<Self> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "timeout is too large"))?;
        let bytes = endpoint.as_bytes();
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if bytes.is_empty() || bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid local socket path",
            ));
        }
        address.sun_family = libc::AF_UNIX as _;
        for (destination, byte) in address.sun_path.iter_mut().zip(bytes) {
            *destination = *byte as _;
        }
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "local socket connection deadline exceeded",
                ));
            }
            // A blocking UnixStream::connect can hang on a full listener
            // backlog. Use a nonblocking descriptor even for the connection
            // attempt, then restore blocking I/O with explicit deadlines.
            let raw = unsafe {
                libc::socket(
                    libc::AF_UNIX,
                    libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                    0,
                )
            };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
            let result = unsafe {
                libc::connect(
                    descriptor.as_raw_fd(),
                    (&address as *const libc::sockaddr_un).cast(),
                    std::mem::size_of::<libc::sockaddr_un>() as _,
                )
            };
            let connected = if result == 0 {
                true
            } else {
                let error = io::Error::last_os_error();
                match error.raw_os_error() {
                    Some(libc::EINPROGRESS) | Some(libc::EALREADY) => {
                        let mut poll = libc::pollfd {
                            fd: descriptor.as_raw_fd(),
                            events: libc::POLLOUT,
                            revents: 0,
                        };
                        loop {
                            let remaining = deadline.saturating_duration_since(Instant::now());
                            if remaining.is_zero() {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "local connection timed out",
                                ));
                            }
                            let milliseconds =
                                remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
                            let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
                            if result == 0 {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "local connection timed out",
                                ));
                            }
                            if result < 0 {
                                let error = io::Error::last_os_error();
                                if error.kind() == io::ErrorKind::Interrupted {
                                    continue;
                                }
                                return Err(error);
                            }
                            let mut code = 0_i32;
                            let mut size = std::mem::size_of::<i32>() as libc::socklen_t;
                            if unsafe {
                                libc::getsockopt(
                                    descriptor.as_raw_fd(),
                                    libc::SOL_SOCKET,
                                    libc::SO_ERROR,
                                    (&mut code as *mut i32).cast(),
                                    &mut size,
                                )
                            } < 0
                            {
                                return Err(io::Error::last_os_error());
                            }
                            if code != 0 {
                                return Err(io::Error::from_raw_os_error(code));
                            }
                            break true;
                        }
                    }
                    // For Unix sockets EAGAIN means the listener backlog is
                    // full; this descriptor has not begun connecting.
                    Some(libc::EAGAIN) => false,
                    Some(libc::ENOENT) | Some(libc::ECONNREFUSED) if retry_missing => false,
                    _ => return Err(error),
                }
            };
            if connected {
                let stream = UnixStream::from(descriptor);
                stream.set_nonblocking(false)?;
                return Ok(Self {
                    stream,
                    deadline: Some(deadline),
                });
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(20)),
            );
        }
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            deadline: self.deadline,
        })
    }
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
    pub fn set_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.deadline = timeout
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "timeout is too large")
                })
            })
            .transpose()?;
        Ok(())
    }
    fn remaining(&self) -> io::Result<Option<Duration>> {
        match self.deadline {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "local I/O deadline exceeded",
                    ));
                }
                Ok(Some(remaining))
            }
            None => Ok(None),
        }
    }
}
impl Read for NativeStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        self.stream.set_read_timeout(self.remaining()?)?;
        self.stream.read(bytes)
    }
}
impl Write for NativeStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        self.stream.set_write_timeout(self.remaining()?)?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// Re-enter the same binary under a transient user service. No global manager
/// environment is modified. Environment *names*, never secret values, occur in
/// the systemd-run command line. argv expansion is explicitly disabled.
pub fn bootstrap_supervised(args: &[OsString]) -> Result<Option<i32>> {
    if is_systemd_managed()
        || std::env::var_os("UI4ME_TEST_MODE").is_some()
        || std::env::var_os("UI4ME_ALLOW_UNSUPERVISED").is_some()
    {
        return Ok(None);
    }
    let unit = format!(
        "ui4me-host-{}-{}.service",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    );
    let mut command = Command::new("systemd-run");
    command
        .args([
            "--user",
            "--wait",
            "--pipe",
            "--collect",
            "--quiet",
            "--service-type=exec",
            "--expand-environment=no",
            "--property=ExitType=main",
            "--property=KillMode=control-group",
            "--property=TimeoutStopSec=5s",
            "--same-dir",
        ])
        .arg(format!("--unit={unit}"))
        .env("UI4ME_SYSTEMD_UNIT", &unit)
        .env("UI4ME_DESKTOP_REALM", desktop_realm());
    for (key, _) in std::env::vars_os() {
        if key != "UI4ME_SYSTEMD_UNIT" && key != "UI4ME_DESKTOP_REALM" {
            let mut argument = OsString::from("--setenv=");
            argument.push(key);
            command.arg(argument);
        }
    }
    command
        .args([
            "--setenv=UI4ME_SYSTEMD_UNIT",
            "--setenv=UI4ME_DESKTOP_REALM",
        ])
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(args);
    let status = command.status().context("cannot start systemd user supervision; run doctor, or explicitly choose --allow-unsupervised for diagnostics")?;
    Ok(Some(status.code().unwrap_or(1)))
}

pub fn is_systemd_managed() -> bool {
    let Ok(unit) = std::env::var("UI4ME_SYSTEMD_UNIT") else {
        return false;
    };
    if !unit.starts_with("ui4me-host-") || !unit.ends_with(".service") {
        return false;
    }
    std::fs::read_to_string("/proc/self/cgroup")
        .map(|s| s.lines().any(|line| line.ends_with(&unit)))
        .unwrap_or(false)
}

pub fn containment_description() -> &'static str {
    if is_systemd_managed() {
        "systemd-user-service"
    } else {
        "process-group-only (diagnostic/test mode; hard-host-death descendant cleanup is not guaranteed)"
    }
}

pub struct NativeChild {
    child: Option<Child>,
    group: Option<i32>,
    pid: u32,
    status: Option<ExitStatus>,
}

pub struct SpawnedChild {
    pub child: NativeChild,
    pub stdout: Box<dyn Read + Send>,
    pub stderr: Box<dyn Read + Send>,
}

pub fn spawn_backend(
    config: &crate::config::BackendConfig,
    endpoint: &str,
    token: &str,
    app_id: &str,
) -> Result<SpawnedChild> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(&config.program);
    command
        .args(&config.args)
        .current_dir(&config.cwd)
        .envs(&config.env)
        .env("UI4ME_ENDPOINT", endpoint)
        .env("UI4ME_TOKEN", token)
        .env("UI4ME_APP_ID", app_id)
        .env(
            "UI4ME_PROTOCOL_VERSION",
            crate::protocol::PROTOCOL_VERSION.to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                // pre_exec runs after fork in a multithreaded host. Avoid
                // allocating an error message while allocator locks may have
                // been inherited in a locked state from vanished threads.
                return Err(io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("start backend executable")?;
    let stdout = child.stdout.take().context("backend stdout pipe missing")?;
    let stderr = child.stderr.take().context("backend stderr pipe missing")?;
    let pid = child.id();
    Ok(SpawnedChild {
        child: NativeChild {
            group: Some(pid as i32),
            child: Some(child),
            pid,
            status: None,
        },
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

impl NativeChild {
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let Some(child) = self.child.as_mut() else {
            return Ok(self.status);
        };
        // Observe exit without reaping: the zombie keeps its PID/process-group
        // identity reserved until remaining group members have been killed.
        // Signalling the old group after Child::try_wait reaped the leader
        // would introduce a PID-reuse race against an unrelated process.
        let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut information,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.group = None;
            }
            return Err(error);
        }
        if unsafe { information.si_pid() } == 0 {
            return Ok(None);
        }
        self.signal(libc::SIGKILL)?;
        let status = self
            .child
            .as_mut()
            .expect("child retained until reaping")
            .try_wait()?;
        if let Some(status) = status {
            self.status = Some(status);
            self.group = None;
        }
        Ok(status)
    }
    pub fn terminate(&mut self) -> io::Result<()> {
        self.signal(libc::SIGTERM)
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }
    fn signal(&self, signal: i32) -> io::Result<()> {
        let Some(group) = self.group else {
            return Ok(());
        };
        if unsafe { libc::kill(-group, signal) } == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }
}
impl Drop for NativeChild {
    fn drop(&mut self) {
        let _ = self.kill();
        if let Some(mut child) = self.child.take() {
            if self.status.is_none() {
                // The GUI must not wait for a process to acknowledge SIGKILL.
                // The reaper owns Child so no exited child becomes a zombie
                // while a host-owned diagnostic window remains open.
                let _ = std::thread::Builder::new()
                    .name("ui4me-child-reaper".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
        }
    }
}

/// Use the desktop broker so launched viewers do not enter the host's cgroup.
/// The result acknowledges the request, not proof that another app presented UI.
pub fn open_external(target: &str) -> Result<()> {
    use std::os::fd::AsFd;
    let connection = zbus::blocking::Connection::session()?;
    let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    let parsed = url::Url::parse(target);
    if let Ok(uri) = &parsed {
        if uri.scheme() != "file" {
            connection.call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.OpenURI"),
                "OpenURI",
                &("", target, options),
            )?;
            return Ok(());
        }
    }
    let path = match parsed {
        Ok(uri) => uri
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file URL"))?,
        Err(_) => PathBuf::from(target),
    };
    let file = File::open(&path)?;
    let descriptor = zbus::zvariant::Fd::from(file.as_fd());
    let method = if path.is_dir() {
        "OpenDirectory"
    } else {
        "OpenFile"
    };
    connection.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.OpenURI"),
        method,
        &("", descriptor, options),
    )?;
    Ok(())
}

pub fn standard_paths(app_id: &str) -> Result<serde_json::Value> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let path = |name, fallback| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(fallback))
            .join(app_id)
    };
    Ok(
        serde_json::json!({"config": path("XDG_CONFIG_HOME", ".config"), "data": path("XDG_DATA_HOME", ".local/share"),
        "cache": path("XDG_CACHE_HOME", ".cache"), "state": path("XDG_STATE_HOME", ".local/state"), "runtime": runtime_directory()?}),
    )
}

pub fn initialize_application(_app_id: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compositor_identity_is_stable_across_launch_routes_and_changes_on_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wayland-0");
        File::create(&path).unwrap();
        let relative =
            realm_for_compositor(1000, directory.path(), Some(OsStr::new("wayland-0")), false);
        let absolute = realm_for_compositor(1000, directory.path(), Some(path.as_os_str()), false);
        assert_eq!(relative, absolute);
        assert_eq!(
            relative,
            realm_for_compositor(1000, directory.path(), None, false)
        );
        fs_rename_keep_identity(&path);
        assert_ne!(
            relative,
            realm_for_compositor(1000, directory.path(), None, false)
        );
        assert_ne!(
            relative,
            realm_for_compositor(1001, directory.path(), None, false)
        );
        assert_eq!(
            realm_for_compositor(1000, directory.path(), None, true),
            realm_for_compositor(
                1000,
                directory.path(),
                Some(OsStr::new("other-display")),
                true
            )
        );
    }

    fn fs_rename_keep_identity(path: &Path) {
        // Keep the old inode allocated so a filesystem cannot reuse it for
        // the replacement while this test checks compositor restarts.
        std::fs::rename(path, path.with_extension("old")).unwrap();
        File::create(path).unwrap();
    }

    #[test]
    fn socket_namespace_cannot_escape_the_private_directory() {
        for invalid in ["", "../escape", "/tmp/escape", "not-hex"] {
            assert!(endpoint_for(invalid).is_err());
        }
    }

    #[test]
    fn lock_excludes_other_instance_and_releases() {
        let app = format!("org.ui4me.test.{}", std::process::id());
        let one = InstanceLock::acquire(&app).unwrap();
        assert!(one.primary);
        let two = InstanceLock::acquire(&app).unwrap();
        assert!(!two.primary);
        assert_eq!(one.endpoint, two.endpoint);
        drop(one);
        drop(two);
        assert!(InstanceLock::acquire(&app).unwrap().primary);
    }

    #[test]
    fn deadline_applies_to_all_partial_reads() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut stream = NativeStream {
            stream: reader,
            deadline: None,
        };
        stream
            .set_timeout(Some(Duration::from_millis(180)))
            .unwrap();
        let writer = std::thread::spawn(move || {
            for _ in 0..12 {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        let start = Instant::now();
        let mut bytes = [0; 12];
        let error = stream.read_exact(&mut bytes).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        // The error (rather than a successful 12-byte read) proves that the
        // per-call deadline was not refreshed. Leave scheduler headroom for
        // overloaded CI machines when checking elapsed wall-clock time.
        assert!(start.elapsed() < Duration::from_secs(2));
        stream.shutdown();
        writer.join().unwrap();
    }

    #[test]
    fn active_listener_cannot_be_replaced_but_stale_socket_can() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sock");
        let endpoint = path.to_str().unwrap();
        let listener = NativeListener::bind(endpoint).unwrap();
        assert!(NativeListener::bind(endpoint).is_err());
        drop(listener);
        // UnixListener intentionally leaves its filesystem entry behind.
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        assert!(NativeListener::bind(endpoint).is_ok());
    }

    #[test]
    fn exited_child_is_reaped_and_exit_status_is_stable() {
        let config = crate::config::BackendConfig {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exit 7".into()],
            cwd: std::env::temp_dir(),
            env: Default::default(),
        };
        let mut native = spawn_backend(&config, "/unused", "test", "org.ui4me.test").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = native.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(7));
        assert_eq!(native.child.try_wait().unwrap().unwrap().code(), Some(7));
        assert!(
            native.child.group.is_none(),
            "a reaped PID must not remain a signal target"
        );
        let mut raw_status = 0;
        assert_eq!(
            unsafe { libc::waitpid(native.child.id() as i32, &mut raw_status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn dropping_live_child_reaps_without_blocking_the_gui() {
        let config = crate::config::BackendConfig {
            program: "/bin/sleep".into(),
            args: vec!["30".into()],
            cwd: std::env::temp_dir(),
            env: Default::default(),
        };
        let native = spawn_backend(&config, "/unused", "test", "org.ui4me.test").unwrap();
        let pid = native.child.id();
        let start = Instant::now();
        drop(native);
        assert!(start.elapsed() < Duration::from_millis(500));
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // Observe without racing the reaper for ownership of the exit.
            let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &mut information,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                break;
            }
            assert!(Instant::now() < deadline, "child was not reaped");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
