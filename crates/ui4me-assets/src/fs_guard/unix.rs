//! owner: 007-resources (T011) — Unix half of the file guard.
//!
//! Linux: `readlink /proc/self/fd/<n>` yields the opened handle's final path. The file is opened
//! non-blocking so a FIFO cannot hang the open; `fstat` then rejects it as not a regular file.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// `O_NONBLOCK` on Linux (all architectures the project targets use the same value on x86_64).
const O_NONBLOCK: i32 = 0o4000;

/// Open read-only (links are followed) and return the handle with its final path.
pub fn open_with_final_path(path: &Path) -> std::io::Result<(File, PathBuf)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)?;
    let final_path = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    Ok((file, final_path))
}
