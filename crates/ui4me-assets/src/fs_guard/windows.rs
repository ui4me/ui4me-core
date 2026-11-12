//! owner: 007-resources (T040, partial) — Windows half of the file guard.
//!
//! The lexical rejection is pure and compiled on every OS so it is unit-tested on Linux. The
//! open-then-verify half (`GetFinalPathNameByHandleW`) is compiled only on Windows and has NOT
//! been run yet: T040 stays open until the Windows suite runs on a Windows machine (T044).

use ui4me_engine_types::codes_generated::Code;

const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"];

fn is_reserved_device(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
                return true;
            }
            if matches!(rest, "\u{b9}" | "\u{b2}" | "\u{b3}") {
                return true;
            }
        }
    }
    false
}

/// Lexical rejection (`RESOURCE_DENIED`) of a Windows path before it is opened: device paths
/// (`\\.\`), verbatim paths (`\\?\`), alternate data streams, reserved device names, components
/// with a trailing dot or space (Windows silently strips them) and wildcard characters.
/// `unc` paths pass here; they are accepted only when an explicit UNC root covers the final path.
pub fn lexical_check(path: &str, unc: bool) -> Result<(), Code> {
    if path.starts_with("\\\\.\\")
        || path.starts_with("\\\\?\\")
        || path.starts_with("//./")
        || path.starts_with("//?/")
    {
        return Err(Code::ResourceDenied);
    }
    if path.contains('\0') {
        return Err(Code::ResourceDenied);
    }
    let (after_prefix, components): (&str, Vec<&str>) = if unc {
        let rest = path.trim_start_matches('\\');
        ("", rest.split('\\').collect())
    } else {
        // `C:\a\b`: skip the two-character drive designator.
        let rest = path.get(2..).unwrap_or("");
        (path.get(..2).unwrap_or(""), rest.split('\\').collect())
    };
    let _ = after_prefix;
    for c in components.iter().filter(|c| !c.is_empty()) {
        if c.contains(':') || c.contains(['<', '>', '"', '|', '?', '*']) {
            return Err(Code::ResourceDenied);
        }
        if c.ends_with('.') || c.ends_with(' ') {
            return Err(Code::ResourceDenied);
        }
        if is_reserved_device(c) {
            return Err(Code::ResourceDenied);
        }
    }
    Ok(())
}

#[cfg(windows)]
pub use imp::open_with_final_path;

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};

    // The binding is `windows-sys` (locked; features Win32_Foundation + Win32_Storage_FileSystem).
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    /// FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: do not block other readers.
    const SHARE_ALL: u32 = 7;

    pub fn open_with_final_path(path: &Path) -> std::io::Result<(File, PathBuf)> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(SHARE_ALL)
            .open(path)?;
        let handle = file.as_raw_handle();
        let mut buf = vec![0u16; 1024];
        loop {
            // SAFETY: `handle` is a valid open file handle owned by `file`; `buf` has `buf.len()` u16s.
            let n = unsafe {
                GetFinalPathNameByHandleW(handle.cast(), buf.as_mut_ptr(), buf.len() as u32, 0)
            };
            if n == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if (n as usize) < buf.len() {
                buf.truncate(n as usize);
                break;
            }
            buf.resize(n as usize + 1, 0);
        }
        let s = String::from_utf16_lossy(&buf);
        let p = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            PathBuf::from(s)
        };
        Ok((file, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_rejections() {
        for p in [
            r"\\.\C:\x",
            r"\\?\C:\x",
            r"C:\app\a.png:stream",
            r"C:\app\NUL",
            r"C:\app\nul.png",
            r"C:\app\COM1.txt",
            r"C:\app\LPT9",
            r"C:\app\a.png.",
            r"C:\app\a.png ",
            r"C:\app\a*.png",
            r"C:\app\CONIN$",
        ] {
            assert_eq!(lexical_check(p, false), Err(Code::ResourceDenied), "{p}");
        }
        for p in [
            r"C:\app\ui\a.png",
            r"C:\app\console.png",
            r"C:\app\COM10.png",
            r"C:\app\PROGRA~1\a.png",
        ] {
            assert_eq!(lexical_check(p, false), Ok(()), "{p}");
        }
        assert_eq!(lexical_check(r"\\server\share\a.png", true), Ok(()));
        assert_eq!(
            lexical_check(r"\\server\share\a:b", true),
            Err(Code::ResourceDenied)
        );
    }
}
