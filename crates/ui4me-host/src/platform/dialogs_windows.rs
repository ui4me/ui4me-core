use super::{DialogKind, DialogOptions, DialogResult};
use anyhow::{bail, Context, Result};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::{Interface, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND as ComHwnd;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    FOS_NOCHANGEDIR, FOS_NOREADONLYRETURN, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
    SIGDN_FILESYSPATH,
};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use winit::window::Window;

const CANCEL_MESSAGE: u32 = WM_APP + 19;
const CANCELLED: HRESULT = HRESULT(0x800704c7_u32 as i32);

#[derive(Default)]
pub(super) struct Cancellation {
    requested: AtomicBool,
    // Protected by the same mutex while publishing, posting and destroying,
    // so a reused HWND cannot receive a late cancellation message.
    window: Mutex<Option<isize>>,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
        let window = self
            .window
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(window) = *window {
            unsafe {
                PostMessageW(window as HWND, CANCEL_MESSAGE, 0, 0);
            }
        }
    }
    fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

struct DialogState {
    dialog: IFileDialog,
    cancellation: Arc<Cancellation>,
}

// All COM calls in this procedure execute in the dialog's owning STA. No COM
// interface pointer is invoked from the GUI or cancellation caller's thread.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == CANCEL_MESSAGE || (message == WM_TIMER && wparam == 1) {
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const DialogState;
        if !pointer.is_null() {
            let state = unsafe { &*pointer };
            if state.cancellation.requested() {
                let _ = unsafe { state.dialog.Close(CANCELLED) };
            }
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct MessageWindow {
    hwnd: HWND,
    cancellation: Arc<Cancellation>,
}
impl MessageWindow {
    fn new(state: &DialogState) -> Result<Self> {
        static CLASS: OnceLock<std::result::Result<u16, u32>> = OnceLock::new();
        let class: Vec<u16> = "UI4ME.DialogCancellation\0".encode_utf16().collect();
        let instance = unsafe { GetModuleHandleW(null()) };
        let registered = CLASS.get_or_init(|| {
            let attributes = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            let atom = unsafe { RegisterClassW(&attributes) };
            if atom != 0 {
                Ok(atom)
            } else {
                let error = unsafe { GetLastError() };
                if error == ERROR_CLASS_ALREADY_EXISTS {
                    Ok(1)
                } else {
                    Err(error)
                }
            }
        });
        if let Err(error) = registered {
            return Err(std::io::Error::from_raw_os_error(*error as i32).into());
        }
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                instance,
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let window = Self {
            hwnd,
            cancellation: Arc::clone(&state.cancellation),
        };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as *const DialogState as isize);
        }
        // A timer covers cancellation arriving while COM is preparing Show,
        // before its modal loop begins. PostMessage handles the usual case.
        if unsafe { SetTimer(hwnd, 1, 50, None) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        *window
            .cancellation
            .window
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(hwnd as isize);
        Ok(window)
    }
}
impl Drop for MessageWindow {
    fn drop(&mut self) {
        let mut published = self
            .cancellation
            .window
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *published = None;
        unsafe {
            KillTimer(self.hwnd, 1);
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.hwnd);
        }
    }
}

fn item_path(item: IShellItem) -> Result<PathBuf> {
    let pointer = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
    // GetDisplayName allocates with the COM task allocator, including paths
    // with non-Unicode Windows code units. Preserve them in OsString.
    let path = PathBuf::from(OsString::from_wide(unsafe { pointer.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(pointer.0.cast()));
    }
    Ok(path)
}

pub(super) fn run(
    parent: &Arc<dyn Window>,
    options: &DialogOptions,
    cancellation: &Arc<Cancellation>,
) -> DialogResult {
    if cancellation.requested() {
        return Ok(None);
    }
    let owner = match parent.window_handle()?.as_raw() {
        RawWindowHandle::Win32(handle) => ComHwnd(handle.hwnd.get() as *mut _),
        _ => bail!("Windows file dialog requires a Win32 parent window"),
    };
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .context("initialize file dialog STA")?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _apartment = Apartment;
    let class = if options.kind == DialogKind::SaveFile {
        &FileSaveDialog
    } else {
        &FileOpenDialog
    };
    let dialog: IFileDialog = unsafe { CoCreateInstance(class, None, CLSCTX_INPROC_SERVER) }?;
    let mut flags =
        unsafe { dialog.GetOptions() }? | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_PATHMUSTEXIST;
    flags |= match options.kind {
        DialogKind::OpenFiles => FOS_FILEMUSTEXIST | FOS_ALLOWMULTISELECT,
        DialogKind::SaveFile => FOS_OVERWRITEPROMPT | FOS_NOREADONLYRETURN,
        DialogKind::ChooseFolder => FOS_PICKFOLDERS,
    };
    unsafe {
        dialog.SetOptions(flags)?;
        dialog.SetTitle(&HSTRING::from(options.title()))?;
    }
    if let Some(directory) = &options.directory {
        let units: Vec<u16> = directory.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            bail!("dialog directory contains NUL");
        }
        let item: IShellItem =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from_wide(&units), None) }
                .context("resolve initial dialog directory")?;
        unsafe {
            dialog.SetFolder(&item)?;
        }
    }
    if let Some(name) = &options.file_name {
        unsafe {
            dialog.SetFileName(&HSTRING::from(name))?;
        }
    }
    let filter_strings: Vec<(HSTRING, HSTRING)> = options
        .filters
        .iter()
        .map(|filter| {
            let extensions = filter
                .extensions
                .iter()
                .map(|extension| {
                    if extension == "*" {
                        "*.*".to_owned()
                    } else {
                        format!("*.{extension}")
                    }
                })
                .collect::<Vec<_>>()
                .join(";");
            (HSTRING::from(&filter.name), HSTRING::from(extensions))
        })
        .collect();
    let filters: Vec<COMDLG_FILTERSPEC> = filter_strings
        .iter()
        .map(|(name, pattern)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        })
        .collect();
    if options.kind != DialogKind::ChooseFolder && !filters.is_empty() {
        unsafe {
            dialog.SetFileTypes(&filters)?;
        }
    }
    if options.kind == DialogKind::SaveFile {
        if let Some(extension) = options
            .filters
            .first()
            .and_then(|filter| filter.extensions.first())
            .filter(|v| v.as_str() != "*")
        {
            unsafe {
                dialog.SetDefaultExtension(&HSTRING::from(extension))?;
            }
        }
    }

    let state = Box::new(DialogState {
        dialog,
        cancellation: Arc::clone(cancellation),
    });
    let _message_window = MessageWindow::new(&state)?;
    if cancellation.requested() {
        return Ok(None);
    }
    let shown = unsafe { state.dialog.Show(Some(owner)) };
    if cancellation.requested() || shown.as_ref().is_err_and(|error| error.code() == CANCELLED) {
        return Ok(None);
    }
    shown.context("show native Windows file dialog")?;
    let paths = if options.kind == DialogKind::SaveFile {
        vec![item_path(unsafe { state.dialog.GetResult() }?)?]
    } else {
        let open: IFileOpenDialog = state.dialog.cast()?;
        let items = unsafe { open.GetResults() }?;
        let count = unsafe { items.GetCount() }?;
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            paths.push(item_path(unsafe { items.GetItemAt(index) }?)?);
        }
        paths
    };
    Ok(if paths.is_empty() { None } else { Some(paths) })
}
