//! Windows Server 2022 native facilities. All pipe handles are local, session
//! restricted and overlapped so shutdown can cancel pending I/O. Backend
//! containment is established atomically during CreateProcessW.
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::ffi::{c_void, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::{size_of, ManuallyDrop};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::FromRawHandle;
use std::os::windows::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::*;

#[derive(Debug)]
struct Handle(HANDLE);
// Kernel handles can be used from any thread. This wrapper never contains an
// HWND or a borrowed handle; every value is owned and closed exactly once.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Handle {
    fn checked(raw: HANDLE) -> io::Result<Self> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
    fn into_raw(self) -> HANDLE {
        ManuallyDrop::new(self).0
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "embedded NUL in Windows string",
        ));
    }
    result.push(0);
    Ok(result)
}
fn wide_str(value: &str) -> io::Result<Vec<u16>> {
    wide(OsStr::new(value))
}
fn bool_result(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct LocalAllocation(*mut c_void);
unsafe impl Send for LocalAllocation {}
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// The logon SID, rather than only the user SID, separates concurrent RDP
/// desktops belonging to the same Windows account.
fn logon_sid() -> Result<String> {
    let mut raw = null_mut();
    bool_result(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) })?;
    let token = Handle::checked(raw)?;
    let mut needed = 0_u32;
    unsafe {
        GetTokenInformation(token.0, TokenGroups, null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut storage = vec![0_usize; (needed as usize).div_ceil(size_of::<usize>())];
    bool_result(unsafe {
        GetTokenInformation(
            token.0,
            TokenGroups,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    })?;
    let groups = storage.as_ptr().cast::<TOKEN_GROUPS>();
    let items = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            (*groups).GroupCount as usize,
        )
    };
    const LOGON_ID: u32 = 0xc0000000;
    let sid = items
        .iter()
        .find(|item| item.Attributes & LOGON_ID == LOGON_ID)
        .context("process has no logon SID; start the GUI inside the interactive desktop session")?
        .Sid;
    let mut text = null_mut();
    bool_result(unsafe { ConvertSidToStringSidW(sid, &mut text) })?;
    let _allocation = LocalAllocation(text.cast());
    let mut length = 0;
    unsafe {
        while *text.add(length) != 0 {
            length += 1;
        }
    }
    Ok(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(text, length)
    }))
}

fn session_id() -> Result<u32> {
    let mut value = 0;
    bool_result(unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut value) })?;
    Ok(value)
}

struct Descriptor(LocalAllocation);
impl Descriptor {
    fn for_logon() -> Result<Self> {
        // Protected DACL, only this logon. The creator retains normal owner
        // rights, but we do not grant Everyone/Anonymous access.
        let text = wide_str(&format!("D:P(A;;GA;;;{})", logon_sid()?))?;
        let mut descriptor = null_mut();
        bool_result(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        })?;
        Ok(Self(LocalAllocation(descriptor)))
    }
    fn attributes(&self, inherit: bool) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: i32::from(inherit),
        }
    }
}

pub struct InstanceLock {
    pub instance_key: String,
    pub primary: bool,
    pub endpoint: String,
    handle: Handle,
    // Windows mutex ownership belongs to the acquiring thread. Keeping the
    // guard there prevents a silent ERROR_NOT_OWNER during Drop.
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
fn owned_instance_keys() -> &'static Mutex<HashSet<String>> {
    static OWNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    OWNED.get_or_init(|| Mutex::new(HashSet::new()))
}
impl InstanceLock {
    pub fn acquire(app_id: &str) -> Result<Self> {
        let key = super::namespace_hash(
            format!("{app_id}\0{}\0{}", session_id()?, logon_sid()?).as_bytes(),
        );
        let name = wide_str(&format!("Local\\UI4ME-Host-{key}"))?;
        let descriptor = Descriptor::for_logon()?;
        let mut owned = owned_instance_keys()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let handle = Handle::checked(unsafe {
            CreateMutexW(&descriptor.attributes(false), 0, name.as_ptr())
        })?;
        // WaitForSingleObject recursively acquires a mutex on the same thread.
        // Treat a second guard in this process as a secondary instance too.
        let result = if owned.contains(&key) {
            WAIT_TIMEOUT
        } else {
            unsafe { WaitForSingleObject(handle.0, 0) }
        };
        let primary = match result {
            WAIT_OBJECT_0 | WAIT_ABANDONED => true,
            WAIT_TIMEOUT => false,
            _ => return Err(io::Error::last_os_error().into()),
        };
        if primary {
            owned.insert(key.clone());
        }
        Ok(Self {
            endpoint: endpoint_for(&key)?,
            instance_key: key,
            primary,
            handle,
            _thread_bound: std::marker::PhantomData,
        })
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        if self.primary {
            let mut owned = owned_instance_keys()
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            unsafe {
                ReleaseMutex(self.handle.0);
            }
            owned.remove(&self.instance_key);
        }
    }
}

pub fn endpoint_for(instance_key: &str) -> Result<String> {
    if instance_key.is_empty() || !instance_key.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid pipe instance key");
    }
    Ok(format!(r"\\.\pipe\ui4me-host-{instance_key}"))
}

pub struct NativeListener {
    name: Vec<u16>,
    descriptor: Descriptor,
    pending: Option<Handle>,
}

impl NativeListener {
    pub fn bind(endpoint: &str) -> Result<Self> {
        let mut listener = Self {
            name: wide_str(endpoint)?,
            descriptor: Descriptor::for_logon()?,
            pending: None,
        };
        listener.pending = Some(listener.create_pipe(true)?);
        Ok(listener)
    }
    fn create_pipe(&self, first: bool) -> io::Result<Handle> {
        Handle::checked(unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | FILE_FLAG_OVERLAPPED
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                9,
                65536,
                65536,
                3000,
                &self.descriptor.attributes(false),
            )
        })
    }
    pub fn accept(&mut self, stop: &AtomicBool) -> io::Result<Option<NativeStream>> {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let handle = match self.pending.take() {
            Some(handle) => handle,
            None => self.create_pipe(false)?,
        };
        let event = Handle::checked(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let connected = unsafe { ConnectNamedPipe(handle.0, &mut overlapped) };
        if connected == 0 {
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => (),
                ERROR_IO_PENDING => {
                    if let Err(error) = wait_pending(handle.0, &mut overlapped, stop, None) {
                        if stop.load(Ordering::Acquire) {
                            return Ok(None);
                        }
                        return Err(error);
                    }
                }
                error => return Err(io::Error::from_raw_os_error(error as i32)),
            }
        }
        if stop.load(Ordering::Acquire) {
            unsafe {
                DisconnectNamedPipe(handle.0);
            }
            return Ok(None);
        }
        Ok(Some(NativeStream::new(handle)))
    }
}

/// One end of a connected pipe shared by reader, writer and control clones.
/// Shutdown must be observable by the peer, as Unix shutdown(2) is: the
/// handle is closed as soon as no overlapped operation still uses it, and no
/// operation can start afterwards, so a closed or reused handle is never used.
struct Pipe {
    raw: HANDLE,
    stopped: AtomicBool,
    usage: std::sync::Mutex<PipeUsage>,
}
#[derive(Default)]
struct PipeUsage {
    in_flight: usize,
    closed: bool,
}
// The raw value is an owned kernel handle; access is serialized by `usage`.
unsafe impl Send for Pipe {}
unsafe impl Sync for Pipe {}
impl Pipe {
    fn usage(&self) -> std::sync::MutexGuard<'_, PipeUsage> {
        self.usage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn begin(&self) -> io::Result<()> {
        let mut usage = self.usage();
        if usage.closed || self.stopped.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
        }
        usage.in_flight += 1;
        Ok(())
    }
    fn end(&self) {
        let mut usage = self.usage();
        usage.in_flight -= 1;
        self.close_if_idle(&mut usage);
    }
    fn shutdown(&self) {
        let mut usage = self.usage();
        self.stopped.store(true, Ordering::Release);
        if !usage.closed && usage.in_flight > 0 {
            // Pending operations observe `stopped`, complete their
            // cancellation and the last one closes the handle.
            unsafe {
                CancelIoEx(self.raw, null());
            }
        }
        self.close_if_idle(&mut usage);
    }
    fn close_if_idle(&self, usage: &mut PipeUsage) {
        if self.stopped.load(Ordering::Acquire) && usage.in_flight == 0 && !usage.closed {
            usage.closed = true;
            unsafe {
                CloseHandle(self.raw);
            }
        }
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        let usage = self
            .usage
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !usage.closed {
            usage.closed = true;
            unsafe {
                CloseHandle(self.raw);
            }
        }
    }
}

pub struct NativeStream {
    pipe: Arc<Pipe>,
    deadline: Option<Instant>,
}

impl NativeStream {
    fn new(handle: Handle) -> Self {
        Self {
            pipe: Arc::new(Pipe {
                raw: handle.into_raw(),
                stopped: AtomicBool::new(false),
                usage: Default::default(),
            }),
            deadline: None,
        }
    }
    pub fn connect(endpoint: &str, timeout: Duration) -> io::Result<Self> {
        let name = wide_str(endpoint)?;
        let start = Instant::now();
        loop {
            let raw = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };
            match Handle::checked(raw) {
                Ok(handle) => return Ok(Self::new(handle)),
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|v| v as u32),
                        Some(ERROR_PIPE_BUSY | ERROR_FILE_NOT_FOUND)
                    ) =>
                {
                    let remaining = timeout.saturating_sub(start.elapsed());
                    if remaining.is_zero() {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "connect to local pipe timed out",
                        ));
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(20)));
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            pipe: self.pipe.clone(),
            deadline: self.deadline,
        })
    }
    /// Close this connection for both directions; the peer observes it.
    pub fn shutdown(&self) {
        self.pipe.shutdown();
    }
    pub fn set_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.deadline = timeout.map(|value| Instant::now() + value);
        Ok(())
    }
    fn transfer(&self, bytes: *mut u8, length: usize, write: bool) -> io::Result<usize> {
        self.pipe.begin()?;
        let result = self.transfer_started(bytes, length, write);
        self.pipe.end();
        result
    }
    fn transfer_started(&self, bytes: *mut u8, length: usize, write: bool) -> io::Result<usize> {
        let handle = self.pipe.raw;
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "pipe operation timed out",
            ));
        }
        if length == 0 {
            return Ok(0);
        }
        let event = Handle::checked(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let mut count = 0;
        let result = unsafe {
            if write {
                WriteFile(
                    handle,
                    bytes.cast_const(),
                    length.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    handle,
                    bytes,
                    length.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut overlapped,
                )
            }
        };
        if result != 0 {
            bool_result(unsafe { GetOverlappedResult(handle, &overlapped, &mut count, 0) })?;
            return Ok(count as usize);
        }
        match unsafe { GetLastError() } {
            ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED if !write => Ok(0),
            ERROR_IO_PENDING => {
                wait_pending(handle, &mut overlapped, &self.pipe.stopped, self.deadline)
                    .map(|value| value as usize)
            }
            error => Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
}
impl Read for NativeStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.transfer(bytes.as_mut_ptr(), bytes.len(), false)
    }
}
impl Write for NativeStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.transfer(bytes.as_ptr().cast_mut(), bytes.len(), true)
    }
    // FlushFileBuffers waits until the client drains all data; that would add
    // unnecessary cross-process synchronization. WriteFile supplies delivery.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn wait_pending(
    handle: HANDLE,
    overlapped: &mut OVERLAPPED,
    stopped: &AtomicBool,
    deadline: Option<Instant>,
) -> io::Result<u32> {
    loop {
        if stopped.load(Ordering::Acquire) || deadline.is_some_and(|value| Instant::now() >= value)
        {
            let mut transferred = 0;
            unsafe {
                CancelIoEx(handle, overlapped);
                // Keep OVERLAPPED and its buffers alive until cancellation has
                // completed; returning early would let the kernel use freed memory.
                GetOverlappedResult(handle, overlapped, &mut transferred, 1);
            }
            return Err(io::Error::new(
                if stopped.load(Ordering::Acquire) {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::TimedOut
                },
                "pipe operation cancelled",
            ));
        }
        let wait_ms = deadline
            .map(|value| {
                value
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .clamp(1, 100) as u32
            })
            .unwrap_or(100);
        match unsafe { WaitForSingleObject(overlapped.hEvent, wait_ms) } {
            WAIT_OBJECT_0 => {
                let mut transferred = 0;
                if unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 0) } != 0 {
                    return Ok(transferred);
                }
                let error = unsafe { GetLastError() };
                if error == ERROR_BROKEN_PIPE {
                    return Ok(0);
                }
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            WAIT_TIMEOUT => continue,
            _ => {
                let error = io::Error::last_os_error();
                let mut transferred = 0;
                unsafe {
                    CancelIoEx(handle, overlapped);
                    GetOverlappedResult(handle, overlapped, &mut transferred, 1);
                }
                return Err(error);
            }
        }
    }
}

pub fn bootstrap_supervised(_args: &[OsString]) -> Result<Option<i32>> {
    Ok(None)
}
/// A GUI launch from Explorer or the Start Menu gives this console program a
/// console window of its own, shown next to the application windows and on the
/// taskbar. Release it when no other process shares it. Consoles inherited
/// from a terminal, and redirected output, are left untouched.
pub fn detach_owned_console() {
    use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_CHAR};
    use windows_sys::Win32::System::Console::{
        FreeConsole, GetConsoleProcessList, GetStdHandle, SetStdHandle, STD_ERROR_HANDLE,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    let mut processes = [0_u32; 2];
    if unsafe { GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32) } != 1 {
        return;
    }
    let console_streams: Vec<_> = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .into_iter()
        .filter(|&stream| {
            let handle = unsafe { GetStdHandle(stream) };
            !handle.is_null()
                && handle != INVALID_HANDLE_VALUE
                && unsafe { GetFileType(handle) } == FILE_TYPE_CHAR
        })
        .collect();
    if unsafe { FreeConsole() } != 0 {
        // Writes to a null standard handle are discarded by std.
        for stream in console_streams {
            unsafe {
                SetStdHandle(stream, null_mut());
            }
        }
    }
}

/// Windows hosts are not re-launched under a separate supervisor; the
/// launcher is the host itself, so there is nothing to watch.
pub fn watch_launcher(
    _wake: std::sync::Arc<dyn Fn() + Send + Sync>,
) -> Option<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    None
}

/// What `run` uses, independent of how this diagnostic process was started.
pub fn run_containment_description() -> &'static str {
    "Windows Job Object (backend assigned during creation)"
}

pub fn containment_description() -> &'static str {
    "Windows Job Object (backend assigned during creation)"
}

pub struct NativeChild {
    process: Handle,
    job: Handle,
    id: u32,
    status: Option<ExitStatus>,
}
pub struct SpawnedChild {
    pub child: NativeChild,
    pub stdout: Box<dyn Read + Send>,
    pub stderr: Box<dyn Read + Send>,
}

struct Attributes {
    _storage: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0_usize; size.div_ceil(size_of::<usize>())];
        let pointer = storage.as_mut_ptr().cast();
        bool_result(unsafe { InitializeProcThreadAttributeList(pointer, count, 0, &mut size) })?;
        Ok(Self {
            _storage: storage,
            pointer,
        })
    }
    fn set(&mut self, kind: u32, value: *const c_void, length: usize) -> io::Result<()> {
        bool_result(unsafe {
            UpdateProcThreadAttribute(
                self.pointer,
                0,
                kind as usize,
                value,
                length,
                null_mut(),
                null(),
            )
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.pointer);
        }
    }
}

fn capture_pipe() -> io::Result<(Handle, Handle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    bool_result(unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) })?;
    let reader = Handle::checked(read)?;
    let writer = Handle::checked(write)?;
    bool_result(unsafe { SetHandleInformation(reader.0, HANDLE_FLAG_INHERIT, 0) })?;
    Ok((reader, writer))
}

fn command_argument(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units = wide(value)?;
    units.pop();
    let mut result = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in units {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        result.push(unit);
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    result.push(b'"' as u16);
    Ok(result)
}

fn resolve_executable(config: &crate::config::BackendConfig) -> Result<PathBuf> {
    let program = &config.program;
    let check = |candidate: PathBuf| -> Option<PathBuf> {
        if candidate
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        {
            return None;
        }
        if candidate.is_file() {
            Some(candidate)
        } else if candidate.extension().is_none() && candidate.with_extension("exe").is_file() {
            Some(candidate.with_extension("exe"))
        } else {
            None
        }
    };
    if program.is_absolute() || program.components().count() > 1 {
        return check(if program.is_absolute() {
            program.clone()
        } else {
            config.cwd.join(program)
        })
        .context("backend executable does not exist (shell scripts are not executed implicitly)");
    }
    let search = config
        .env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| OsString::from(value))
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    for directory in std::env::split_paths(&search) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let directory = if directory.is_absolute() {
            directory
        } else {
            config.cwd.join(directory)
        };
        if let Some(path) = check(directory.join(program)) {
            return Ok(path);
        }
    }
    bail!("backend executable {} not found in PATH", program.display())
}

pub fn spawn_backend(
    config: &crate::config::BackendConfig,
    endpoint: &str,
    token: &str,
    app_id: &str,
) -> Result<SpawnedChild> {
    let executable = resolve_executable(config)?;
    let application = wide(executable.as_os_str())?;
    let directory = wide(config.cwd.as_os_str())?;
    let mut command_line = command_argument(executable.as_os_str())?;
    for argument in &config.args {
        command_line.push(b' ' as u16);
        command_line.extend(command_argument(OsStr::new(argument))?);
    }
    command_line.push(0);
    if command_line.len() > 32767 {
        bail!("backend command line exceeds Windows limit");
    }

    // Build a sorted, case-insensitive environment without exposing values in
    // a command line, shell invocation or log message.
    let mut environment: BTreeMap<String, (OsString, OsString)> = BTreeMap::new();
    for (key, value) in std::env::vars_os() {
        environment.insert(key.to_string_lossy().to_uppercase(), (key, value));
    }
    for (key, value) in &config.env {
        environment.insert(key.to_uppercase(), (key.into(), value.into()));
    }
    for (key, value) in [
        ("UI4ME_ENDPOINT", endpoint),
        ("UI4ME_TOKEN", token),
        ("UI4ME_APP_ID", app_id),
        ("UI4ME_PROTOCOL_VERSION", "1"),
    ] {
        environment.insert(key.into(), (key.into(), value.into()));
    }
    let mut block = Vec::<u16>::new();
    for (_, (key, value)) in environment {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        block.extend(wide(&entry)?);
    }
    block.push(0);

    let job = Handle::checked(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    bool_result(unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    })?;
    let (stdout_read, stdout_write) = capture_pipe()?;
    let (stderr_read, stderr_write) = capture_pipe()?;
    let inherit = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let input_name = wide_str("NUL")?;
    let stdin = Handle::checked(unsafe {
        CreateFileW(
            input_name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &inherit,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    })?;
    let inherited_handles = [stdin.0, stdout_write.0, stderr_write.0];
    let jobs = [job.0];
    let mut attributes = Attributes::new(2)?;
    attributes.set(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        inherited_handles.as_ptr().cast(),
        size_of_val(&inherited_handles),
    )?;
    attributes.set(
        PROC_THREAD_ATTRIBUTE_JOB_LIST,
        jobs.as_ptr().cast(),
        size_of_val(&jobs),
    )?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin.0;
    startup.StartupInfo.hStdOutput = stdout_write.0;
    startup.StartupInfo.hStdError = stderr_write.0;
    startup.lpAttributeList = attributes.pointer;
    let mut process = PROCESS_INFORMATION::default();
    bool_result(unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            block.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        )
    })
    .context("start backend in Windows Job Object")?;
    let process_handle = Handle::checked(process.hProcess)?;
    let _thread_handle = Handle::checked(process.hThread)?;
    // Parent copies of child write ends close now, so EOF is observable after
    // the backend and any inherited worker handles actually close.
    drop(stdin);
    drop(stdout_write);
    drop(stderr_write);
    let stdout = unsafe { File::from_raw_handle(stdout_read.into_raw()) };
    let stderr = unsafe { File::from_raw_handle(stderr_read.into_raw()) };
    Ok(SpawnedChild {
        child: NativeChild {
            process: process_handle,
            job,
            id: process.dwProcessId,
            status: None,
        },
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

impl NativeChild {
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            WAIT_TIMEOUT => return Ok(None),
            WAIT_OBJECT_0 => (),
            _ => return Err(io::Error::last_os_error()),
        }
        let mut code = 0;
        bool_result(unsafe { GetExitCodeProcess(self.process.0, &mut code) })?;
        // The backend's exit ends the lifetime of its managed workers too.
        bool_result(unsafe { TerminateJobObject(self.job.0, 1) })?;
        let status = ExitStatus::from_raw(code);
        self.status = Some(status);
        Ok(Some(status))
    }
    /// Graceful shutdown is negotiated over IPC before this escalation step.
    pub fn terminate(&mut self) -> io::Result<()> {
        bool_result(unsafe { TerminateJobObject(self.job.0, 1) })
    }
    pub fn kill(&mut self) -> io::Result<()> {
        bool_result(unsafe { TerminateJobObject(self.job.0, 137) })
    }
}

pub fn initialize_application(app_id: &str) -> Result<()> {
    let name = wide_str(app_id)?;
    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(name.as_ptr())
    };
    if result < 0 {
        bail!("SetCurrentProcessExplicitAppUserModelID failed: {result:#x}");
    }
    Ok(())
}

pub fn runtime_directory() -> Result<PathBuf> {
    Ok(std::env::temp_dir().join("ui4me-host"))
}

pub fn standard_paths(app_id: &str) -> Result<serde_json::Value> {
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_RoamingAppData, SHGetKnownFolderPath,
    };
    fn known(id: &windows_sys::core::GUID) -> Result<PathBuf> {
        let mut pointer = null_mut();
        let result = unsafe { SHGetKnownFolderPath(id, 0, null_mut(), &mut pointer) };
        if result < 0 {
            bail!("SHGetKnownFolderPath failed: {result:#x}");
        }
        let mut length = 0;
        unsafe {
            while *pointer.add(length) != 0 {
                length += 1;
            }
        }
        let path = PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(pointer, length)
        }));
        unsafe {
            windows_sys::Win32::System::Com::CoTaskMemFree(pointer.cast());
        }
        Ok(path)
    }
    let local = known(&FOLDERID_LocalAppData)?.join(app_id);
    Ok(
        serde_json::json!({"config": known(&FOLDERID_RoamingAppData)?.join(app_id),
        "data": local.join("data"), "cache": local.join("cache"), "state": local.join("state"), "runtime": runtime_directory()?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_arguments_preserve_quotes_and_trailing_slashes() {
        let quoted =
            |value| String::from_utf16(&command_argument(OsStr::new(value)).unwrap()).unwrap();
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("hello world"), "\"hello world\"");
        assert_eq!(
            quoted("C:\\path with spaces\\"),
            "\"C:\\path with spaces\\\\\""
        );
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn instance_mutex_does_not_recursively_acquire_on_same_thread() {
        let app = format!("test.ui4me.instance.{}", std::process::id());
        let first = InstanceLock::acquire(&app).unwrap();
        let second = InstanceLock::acquire(&app).unwrap();
        assert!(first.primary);
        assert!(!second.primary);
        drop(first);
        let next = InstanceLock::acquire(&app).unwrap();
        assert!(next.primary);
    }
}
