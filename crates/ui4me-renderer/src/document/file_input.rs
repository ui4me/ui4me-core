//! `<input type=file>` (0.6 F0606): activation (click, Enter/Space) asks the host to open its
//! native picker; the host completes the request with the chosen paths (the input gets them, and
//! `input` then `change` fire) or with nothing (`cancel` fires). Paths reach the app only after
//! the user chose them in the OS picker; Core reads no file.

use super::*;
use crate::interaction::frame::EventInit;

/// A picker request: the input, whether it takes several files, and its `accept` list.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInputRequest {
    pub input: NodeId,
    pub element: Option<String>,
    pub multiple: bool,
    pub accept: Vec<String>,
}

/// Bytes of one path / paths per input the host may write.
const MAX_PATH: usize = 4096;
const MAX_FILES: usize = 1024;

impl Document {
    /// The picker requests of activated file inputs since the last call (still connected).
    pub fn take_file_requests(&mut self) -> Vec<FileInputRequest> {
        let requests = std::mem::take(&mut self.file_requests);
        let d = self.pipeline.document();
        requests
            .into_iter()
            .filter(|&n| valid_picker_target(d, n))
            .map(|n| FileInputRequest {
                input: n,
                element: d.get_attribute(n, "id").ok().flatten().map(str::to_owned),
                multiple: d.get_attribute(n, "multiple").ok().flatten().is_some(),
                accept: d
                    .get_attribute(n, "accept")
                    .ok()
                    .flatten()
                    .unwrap_or("")
                    .split(',')
                    .map(|a| a.trim().to_ascii_lowercase())
                    .filter(|a| !a.is_empty())
                    .collect(),
            })
            .collect()
    }

    /// Complete a picker request: `Some(paths)` sets the input's files and fires `input` and
    /// `change`; `None` (canceled) fires `cancel`. A removed, disabled, or
    /// no-longer-file input completes silently. A single-file input rejects
    /// multiple returned paths even if a native adapter failed to enforce it.
    pub fn complete_file_input(&mut self, input: NodeId, paths: Option<Vec<String>>) -> Result<()> {
        {
            let d = self.pipeline.document();
            if !valid_picker_target(d, input) {
                return Ok(());
            }
            if paths.as_ref().is_some_and(|p| p.len() > 1)
                && d.get_attribute(input, "multiple")?.is_none()
            {
                return Err(arg(
                    "A file input without multiple accepts at most one file",
                ));
            }
        }
        if let Some(p) = &paths
            && (p.len() > MAX_FILES || p.iter().any(|x| x.len() > MAX_PATH || x.contains('\0')))
        {
            return Err(arg("invalid file paths"));
        }
        let id = self.id();
        let events: Vec<EventInit> = match paths {
            None => vec![EventInit::new("cancel").bubbles(true)],
            Some(paths) => {
                let names: Vec<_> = paths
                    .iter()
                    .map(|p| super::application_api::file_basename(p).to_owned())
                    .collect();
                self.pipeline.set_file_input_names(input, &names)?;
                self.input_files.insert(input, paths);
                vec![
                    EventInit::new("input").bubbles(true),
                    EventInit::new("change").bubbles(true),
                ]
            }
        };
        for init in events {
            let _ =
                crate::interaction::element_events::dispatch_element_task(self, id, input, init);
        }
        self.after_mutation(false);
        Ok(())
    }

    /// The files chosen in a file input (empty before a choice, after a reset or a cancel that
    /// followed no choice).
    pub fn get_files(&self, element: &str) -> Result<Vec<String>> {
        let n = self.node(element)?;
        let d = self.pipeline.document();
        let is_file = d.local_name(n).is_ok_and(|t| t == "input")
            && d.get_attribute(n, "type")
                .ok()
                .flatten()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("file"));
        if !is_file {
            return Err(arg("get_files requires an input of type file"));
        }
        Ok(self.input_files.get(&n).cloned().unwrap_or_default())
    }

    /// Forget the files of inputs reset by a form reset or removed (called after mutations).
    pub(super) fn prune_input_files(&mut self) {
        let d = self.pipeline.document();
        let removed: Vec<_> = self
            .input_files
            .keys()
            .copied()
            .filter(|&n| {
                !d.arena().contains(n)
                    || !d.is_connected(n).unwrap_or(false)
                    || d.input_type(n) != Some(InputType::File)
            })
            .collect();
        self.input_files.retain(|&n, _| {
            d.arena().contains(n)
                && d.is_connected(n).unwrap_or(false)
                && d.input_type(n) == Some(InputType::File)
        });
        for n in removed {
            let _ = self.pipeline.set_file_input_names(n, &[]);
        }
    }
}

fn valid_picker_target(d: &Model, node: NodeId) -> bool {
    d.arena().is_connected(node)
        && !d.is_inert(node)
        && d.input_type(node) == Some(InputType::File)
        && d.control_facts(node).is_some_and(|f| !f.disabled)
}
