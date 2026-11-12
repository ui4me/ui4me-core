//! Parented, cancellable native file dialogs. The worker retains the real
//! parent until the native dialog is closed and its resources are released.
//! Completion always runs off the GUI thread; callers marshal it to their
//! event loop. Dropping the handle initiates cancellation without blocking.

use anyhow::{bail, Result};
use std::path::PathBuf;
use std::sync::Arc;
use winit::window::Window;

#[cfg(target_os = "linux")]
#[path = "dialogs_linux.rs"]
mod native;
#[cfg(target_os = "windows")]
#[path = "dialogs_windows.rs"]
mod native;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogKind {
    OpenFiles,
    SaveFile,
    ChooseFolder,
}

#[derive(Clone, Debug)]
pub struct DialogFilter {
    pub name: String,
    /// Literal suffixes without the leading dot, or `*` for all files.
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DialogOptions {
    pub kind: DialogKind,
    pub title: Option<String>,
    pub directory: Option<PathBuf>,
    pub file_name: Option<String>,
    pub filters: Vec<DialogFilter>,
}

impl DialogOptions {
    fn validate(&self) -> Result<()> {
        if self
            .title
            .as_ref()
            .is_some_and(|v| v.len() > 4096 || v.contains('\0'))
        {
            bail!("dialog title is too long or contains NUL");
        }
        if self.file_name.as_ref().is_some_and(|v| {
            v.len() > 4096 || v.contains(['\0', '/', '\\']) || v == "." || v == ".."
        }) {
            bail!("dialog file_name must be a filename, not a path");
        }
        if self.filters.len() > 64 {
            bail!("at most 64 dialog filters are supported");
        }
        for filter in &self.filters {
            if filter.name.is_empty() || filter.name.len() > 256 || filter.name.contains('\0') {
                bail!("dialog filter name is empty, too long or contains NUL");
            }
            if filter.extensions.is_empty() || filter.extensions.len() > 64 {
                bail!("dialog filter requires between 1 and 64 extensions");
            }
            for extension in &filter.extensions {
                if extension != "*"
                    && (extension.is_empty()
                        || extension.len() > 64
                        || extension.starts_with('.')
                        || !extension
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c)))
                {
                    bail!("dialog extensions must be literal suffixes without a leading dot, or *");
                }
            }
        }
        Ok(())
    }

    fn title(&self) -> &str {
        self.title.as_deref().unwrap_or(match self.kind {
            DialogKind::OpenFiles => "Open files",
            DialogKind::SaveFile => "Save file",
            DialogKind::ChooseFolder => "Choose folder",
        })
    }
}

pub struct DialogHandle {
    state: Arc<native::Cancellation>,
}

impl DialogHandle {
    /// Request native cancellation. Completion reports `Ok(None)` once the
    /// dialog is actually closed; cancellation itself never blocks the GUI.
    pub fn cancel(&self) {
        self.state.cancel();
    }
}

impl Drop for DialogHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub type DialogResult = Result<Option<Vec<PathBuf>>>;

pub fn start(
    parent: Arc<dyn Window>,
    options: DialogOptions,
    completion: Box<dyn FnOnce(DialogResult) + Send>,
) -> Result<DialogHandle> {
    options.validate()?;
    // Some platforms (Win32 in winit) only expose the parent's handle on the
    // GUI thread, which is where this function runs.
    let captured = native::capture_parent(&parent)?;
    let state = Arc::new(native::Cancellation::default());
    let worker_state = Arc::clone(&state);
    std::thread::Builder::new()
        .name("ui4me-dialog".into())
        .spawn(move || {
            // The Arc also makes raw window/display handles valid throughout the
            // platform call, including exporting/unexporting a Wayland surface.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                native::run(&parent, captured, &options, &worker_state)
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("native dialog worker panicked")));
            completion(result);
            drop(parent);
        })?;
    Ok(DialogHandle { state })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_filter_globs_and_filename_paths() {
        let mut options = DialogOptions {
            kind: DialogKind::OpenFiles,
            title: None,
            directory: None,
            file_name: None,
            filters: vec![DialogFilter {
                name: "Images".into(),
                extensions: vec!["png".into(), "tar.gz".into()],
            }],
        };
        assert!(options.validate().is_ok());
        options.filters[0].extensions.push("*;*.exe".into());
        assert!(options.validate().is_err());
        options.filters.clear();
        options.file_name = Some("../outside".into());
        assert!(options.validate().is_err());
    }
}
