//! Owner-thread document lifetime and the renderer-facing protocol boundary.
//!
//! A host document identity is never reused, even after unload/reload. A command
//! names both its window and document, so a delayed backend callback cannot
//! mutate a replacement document that happens to reuse the same HTML ids.
use super::*;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use ui4me_renderer::Document;

#[path = "document_commands.rs"]
mod commands;

const MAX_HTML_BYTES: usize = 1024 * 1024;
pub(super) const MAX_DOCUMENTS: usize = 16;
static NEXT_HOST_DOCUMENT: AtomicU64 = AtomicU64::new(1);

pub(super) struct HostedDocument {
    pub(super) id: String,
    pub(super) engine: Document,
    pub(super) a11y: Option<ui4me_renderer::a11y::A11yTree>,
    pub(super) source: Option<PathBuf>,
    pub(super) asset_root: Option<PathBuf>,
    pub(super) viewport: (u32, u32, f64),
    pub(super) native_refresh_pending: bool,
}

impl HostedDocument {
    pub(super) fn describe(&self, window_id: &str) -> Value {
        json!({"window_id":window_id,"document_id":self.id,"loaded":true,
            "source":self.source,"asset_root":self.asset_root,
            "renderer":"cpu","pending_resources":self.engine.has_pending_resources()})
    }
}

impl From<ui4me_renderer::Error> for RpcError {
    fn from(error: ui4me_renderer::Error) -> Self {
        let code = match error.code {
            1 => protocol::code::INVALID_PARAMS,
            2 => protocol::code::INVALID_STATE,
            3 => protocol::code::UNSUPPORTED,
            4 => protocol::code::CLOSED,
            5 => protocol::code::BUSY,
            6 | 9 => protocol::code::UNAVAILABLE,
            _ => protocol::code::INTERNAL_ERROR,
        };
        let mut result = RpcError::new(code, error.message);
        result.data = Some(json!({"engine_code":error.code}));
        result
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadDocument {
    window_id: String,
    #[serde(default)]
    html: Option<String>,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    asset_root: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowDocument {
    window_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentKey {
    window_id: String,
    document_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    window_id: String,
    document_id: String,
    command: commands::DocumentCommand,
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RpcError> {
    serde_json::from_value(value).map_err(|error| RpcError::invalid_params(error.to_string()))
}

fn canonical_path(path: &Path, cwd: &Path) -> Result<PathBuf, RpcError> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let path = path.canonicalize().map_err(|error| {
        RpcError::unavailable(format!(
            "Cannot resolve document resource path {}: {error}",
            path.display()
        ))
    })?;
    if path.to_str().is_none() {
        return Err(RpcError::unsupported(
            "Document protocol paths must be Unicode",
        ));
    }
    Ok(path)
}

/// Read at most the HTML limit plus one byte. File metadata is advisory; the
/// bounded reader remains authoritative if the file grows during the read.
fn read_html(path: &Path) -> Result<String, RpcError> {
    let preflight = std::fs::metadata(path)
        .map_err(|error| RpcError::unavailable(format!("Cannot inspect HTML file: {error}")))?;
    check_html_metadata(&preflight)?;
    let mut options = File::options();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A source replaced with a FIFO between metadata and open must not
        // stall the owner thread waiting for a writer. Regular files ignore it.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| RpcError::unavailable(format!("Cannot open HTML file: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| RpcError::unavailable(format!("Cannot inspect HTML file: {error}")))?;
    check_html_metadata(&metadata)?;
    let mut bytes = Vec::new();
    file.take((MAX_HTML_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| RpcError::unavailable(format!("Cannot read HTML file: {error}")))?;
    if bytes.len() > MAX_HTML_BYTES {
        return Err(RpcError::busy("HTML source exceeds 1 MiB"));
    }
    String::from_utf8(bytes)
        .map_err(|_| RpcError::invalid_params("HTML source must be valid UTF-8"))
}

fn check_html_metadata(metadata: &std::fs::Metadata) -> Result<(), RpcError> {
    if !metadata.is_file() {
        return Err(RpcError::invalid_params(
            "HTML source must be a regular file",
        ));
    }
    if metadata.len() > MAX_HTML_BYTES as u64 {
        return Err(RpcError::busy("HTML source exceeds 1 MiB"));
    }
    Ok(())
}

fn bounded_result(value: Value) -> Result<Value, RpcError> {
    // Reserve the largest legal request id. Count encoded bytes, including JSON
    // escaping, rather than assuming a source-string byte count is the wire size.
    // Control characters are forbidden in request IDs; a backslash needs the
    // largest legal expansion (two JSON bytes for one source byte).
    let mut envelope = protocol::success(&"\\".repeat(protocol::MAX_ID_BYTES), value);
    protocol::encoded_size(&envelope).map_err(|_| RpcError::unavailable(
        "Document result exceeds the encoded 2 MiB frame limit; use a smaller query or paginated HTML/node inspection",
    ))?;
    Ok(envelope["result"].take())
}

impl Host {
    pub(super) fn dispatch_document(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        match method {
            "document.load" => self.load_document(decode(params)?),
            "document.get" => {
                let request: WindowDocument = decode(params)?;
                let window = self
                    .windows
                    .get(&request.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                Ok(window.document.as_ref().map_or_else(
                    || json!({"window_id":request.window_id,"document_id":null,"loaded":false}),
                    |document| document.describe(&request.window_id),
                ))
            }
            "document.unload" => {
                let request: DocumentKey = decode(params)?;
                let window = self
                    .windows
                    .get_mut(&request.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                check_document(window, &request.document_id)?;
                window.document.take();
                self.refresh(&request.window_id);
                Ok(json!({"window_id":request.window_id,"document_id":null,"loaded":false}))
            }
            "document.command" => {
                let request: CommandRequest = decode(params)?;
                let window = self
                    .windows
                    .get_mut(&request.window_id)
                    .ok_or_else(|| RpcError::closed("Window does not exist"))?;
                let document = check_document(window, &request.document_id)?;
                let result = request.command.execute(&mut document.engine);
                // Some commands can emit error/validation events without changing
                // layout. Always service the native/semantic state after an operation;
                // refresh itself uses the renderer's invalidation and frame scheduling.
                self.refresh(&request.window_id);
                bounded_result(result?)
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    fn load_document(&mut self, request: LoadDocument) -> Result<Value, RpcError> {
        let window = self
            .windows
            .get(&request.window_id)
            .ok_or_else(|| RpcError::closed("Window does not exist"))?;
        if window.closing {
            return Err(RpcError::closed("Window is closing"));
        }
        if window.document.is_none()
            && self
                .windows
                .values()
                .filter(|window| window.document.is_some())
                .count()
                >= MAX_DOCUMENTS
        {
            return Err(RpcError::busy(
                "At most 16 live documents are admitted per host",
            ));
        }
        let (width, height, scale) =
            window
                .native
                .as_ref()
                .map_or((window.width, window.height, 1.0), |native| {
                    let size = native.window.surface_size();
                    (
                        size.width.max(1),
                        size.height.max(1),
                        native.window.scale_factor(),
                    )
                });
        let cwd = &self.config.backend.cwd;
        let (html, source, default_root) = match (request.html, request.path) {
            (Some(html), None) => {
                if html.len() > MAX_HTML_BYTES {
                    return Err(RpcError::busy("HTML source exceeds 1 MiB"));
                }
                (html, None, None)
            }
            (None, Some(path)) => {
                let path = canonical_path(&path, cwd)?;
                let html = read_html(&path)?;
                let base = path.parent().map(Path::to_owned);
                (html, Some(path), base)
            }
            _ => return Err(RpcError::invalid_params("Give exactly one of html or path")),
        };
        let asset_root = request
            .asset_root
            .as_deref()
            .map(|path| canonical_path(path, cwd))
            .transpose()?
            .or(default_root);
        if asset_root.as_ref().is_some_and(|root| !root.is_dir()) {
            return Err(RpcError::invalid_params("asset_root must be a directory"));
        }

        // The previous document stays installed until parsing, validation and
        // viewport setup all succeed. Existing user state survives a rejected load.
        let mut engine = Document::parse_hosted(&html, asset_root.as_deref(), width, height)?;
        engine.set_viewport(width, height, scale)?;
        engine.set_system_color_scheme(
            window
                .native
                .as_ref()
                .and_then(|native| native.window.theme())
                .map(|theme| theme == winit::window::Theme::Dark),
        )?;
        if let Some(native) = &window.native {
            // A replacement in an already unfocused window may receive no new
            // Focused event. Initialize editing/caret state from the real window.
            let _ = engine.handle_input(ui4me_renderer::Input::Focus(native.window.has_focus()))?;
        }
        engine.set_waker(self.wake.clone());
        if !self.headless {
            engine.enable_deferred_clipboard();
        }
        let serial = NEXT_HOST_DOCUMENT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| RpcError::busy("Host document identity space exhausted"))?;
        let document = HostedDocument {
            id: format!("d{serial}"),
            engine,
            a11y: None,
            viewport: (width, height, scale),
            native_refresh_pending: false,
            source,
            asset_root,
        };
        let result = document.describe(&request.window_id);
        self.windows
            .get_mut(&request.window_id)
            .expect("window retained on owner thread")
            .document = Some(document);
        self.refresh(&request.window_id);
        Ok(result)
    }
}

fn check_document<'a>(
    window: &'a mut WindowRecord,
    id: &str,
) -> Result<&'a mut HostedDocument, RpcError> {
    if window.closing {
        return Err(RpcError::closed("Window is closing"));
    }
    window
        .document
        .as_mut()
        .filter(|document| document.id == id)
        .ok_or_else(|| RpcError::closed("Document is unloaded or the document_id is stale"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> Host {
        let mut host = super::super::tests::controller(true);
        super::super::tests::add_window(&mut host, "w1");
        host
    }

    fn load(host: &mut Host, html: &str) -> String {
        host.dispatch_document("document.load", json!({"window_id":"w1","html":html}))
            .unwrap()["document_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn command(host: &mut Host, id: &str, command: Value) -> Result<Value, RpcError> {
        host.dispatch_document(
            "document.command",
            json!({"window_id":"w1","document_id":id,"command":command}),
        )
    }

    #[test]
    fn rejected_load_preserves_document_and_replacement_rejects_stale_commands() {
        let mut host = controller();
        let first = load(&mut host, "<p id='status'>original</p>");
        assert!(host
            .dispatch_document(
                "document.load",
                json!({"window_id":"w1","html":"<script>bad()</script>"})
            )
            .is_err());
        assert_eq!(
            command(
                &mut host,
                &first,
                json!({"op":"get_text","element":"status"})
            )
            .unwrap()["text"],
            "original"
        );
        let second = load(&mut host, "<p id='status'>replacement</p>");
        assert_ne!(first, second);
        let error = command(
            &mut host,
            &first,
            json!({"op":"set_text","element":"status","text":"stale"}),
        )
        .unwrap_err();
        assert_eq!(error.code, protocol::code::CLOSED);
        assert_eq!(
            command(
                &mut host,
                &second,
                json!({"op":"get_text","element":"status"})
            )
            .unwrap()["text"],
            "replacement"
        );
    }

    #[test]
    fn atomic_batch_rejects_invalid_operation_without_partial_mutation() {
        let mut host = controller();
        let id = load(
            &mut host,
            "<p id='status'>original</p><div id='items'></div>",
        );
        assert!(command(
            &mut host,
            &id,
            json!({"op":"batch","ops":[
                {"op":"set_text","element":"status","text":"changed"},
                {"op":"remove_element","element":"missing"}
            ]})
        )
        .is_err());
        assert_eq!(
            command(&mut host, &id, json!({"op":"get_text","element":"status"})).unwrap()["text"],
            "original"
        );
        command(&mut host, &id, json!({"op":"batch","ops":[
            {"op":"set_text","element":"status","text":"changed"},
            {"op":"insert_html","element":"items","position":"beforeend","html":"<b id='new'>item</b>"}
        ]})).unwrap();
        assert_eq!(
            command(&mut host, &id, json!({"op":"get_text","element":"new"})).unwrap()["text"],
            "item"
        );
    }

    #[test]
    fn selectors_page_identified_matches_and_node_references_are_generation_checked() {
        let mut host = controller();
        let id = load(
            &mut host,
            "<main id='root'><p id='a'>A</p><p>Anonymous</p><p id='b'>B</p></main>",
        );
        let result = command(
            &mut host,
            &id,
            json!({"op":"query","selector":"p","offset":1,"limit":1}),
        )
        .unwrap();
        assert_eq!(result["ids"], json!(["b"]));
        assert_eq!(result["total"], 2);
        assert_eq!(result["matched_nodes"], 3);
        assert_eq!(result["truncated"], false);
        let node =
            command(&mut host, &id, json!({"op":"node_of","element":"a"})).unwrap()["node"].clone();
        command(&mut host, &id, json!({"op":"remove_element","element":"a"})).unwrap();
        command(&mut host, &id, json!({"op":"insert_html","element":"root","position":"beforeend","html":"<p id='a'>new</p>"})).unwrap();
        assert_eq!(
            command(&mut host, &id, json!({"op":"get_node","node":node}))
                .unwrap_err()
                .code,
            protocol::code::CLOSED
        );
    }

    #[test]
    fn file_loading_is_bounded_and_uses_file_directory_as_asset_root() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("interface.html");
        std::fs::write(&file, "<p id='name'>Café 👋</p>").unwrap();
        let mut host = controller();
        host.config.backend.cwd = temp.path().to_owned();
        let loaded = host
            .dispatch_document(
                "document.load",
                json!({"window_id":"w1","path":"interface.html"}),
            )
            .unwrap();
        assert_eq!(
            loaded["asset_root"],
            json!(temp.path().canonicalize().unwrap())
        );
        assert_eq!(loaded["source"], json!(file.canonicalize().unwrap()));
        let old = loaded["document_id"].as_str().unwrap();
        std::fs::write(&file, vec![b'x'; MAX_HTML_BYTES + 1]).unwrap();
        assert_eq!(
            host.dispatch_document(
                "document.load",
                json!({"window_id":"w1","path":"interface.html"})
            )
            .unwrap_err()
            .code,
            protocol::code::BUSY
        );
        assert_eq!(
            command(&mut host, old, json!({"op":"get_text","element":"name"})).unwrap()["text"],
            "Café 👋"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn html_fifo_is_rejected_without_waiting_for_a_writer() {
        use std::ffi::CString;
        use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("source.html");
        let native = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: native is a live NUL-terminated path, and mkfifo retains no
        // pointer. The fresh private test directory owns the created FIFO.
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = std::sync::mpsc::channel();
        let source = fifo.clone();
        let worker = std::thread::spawn(move || sender.send(read_html(&source)).unwrap());
        let outcome = receiver.recv_timeout(Duration::from_secs(2));
        if outcome.is_err() {
            // Unblock a regressed blocking open before failing the assertion,
            // so the test does not leave an indefinitely hung worker behind.
            let _ = File::options()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&fifo);
        }
        worker.join().unwrap();
        assert_eq!(
            outcome
                .expect("HTML read blocked waiting for a FIFO writer")
                .unwrap_err()
                .code,
            protocol::code::INVALID_PARAMS
        );
        assert!(crate::snapshot::render(&fifo, &temp.path().join("out.png"), 16, 16, 1.0).is_err());
    }

    #[test]
    fn html_pagination_preserves_utf8_and_outgoing_limit_counts_escaping() {
        let mut host = controller();
        let id = load(&mut host, "<p id='text'>é日👋tail</p>");
        let first = command(
            &mut host,
            &id,
            json!({"op":"get_html","element":"text","limit":4}),
        )
        .unwrap();
        assert_eq!(first["html"], "é");
        assert_eq!(first["next_offset"], 2);
        let next = command(
            &mut host,
            &id,
            json!({"op":"get_html","element":"text","offset":2,"limit":4}),
        )
        .unwrap();
        assert_eq!(next["html"], "日");
        assert!(command(
            &mut host,
            &id,
            json!({"op":"get_html","element":"text","offset":1,"limit":4})
        )
        .is_err());
        assert!(bounded_result(json!({"value":"\u{0001}".repeat(400_000)})).is_err());
    }

    #[test]
    fn unload_and_closed_window_reject_document_operations() {
        let mut host = controller();
        let id = load(&mut host, "<p id='x'>value</p>");
        assert!(host
            .dispatch_document(
                "document.command",
                json!({"window_id":"w1","command":{"op":"get_text","element":"x"}})
            )
            .is_err());
        host.dispatch_document(
            "document.unload",
            json!({"window_id":"w1","document_id":id}),
        )
        .unwrap();
        assert_eq!(
            command(&mut host, &id, json!({"op":"get_text","element":"x"}))
                .unwrap_err()
                .code,
            protocol::code::CLOSED
        );
        assert_eq!(
            host.dispatch_document("document.get", json!({"window_id":"w1"}))
                .unwrap()["loaded"],
            false
        );
        host.windows.get_mut("w1").unwrap().closing = true;
        assert_eq!(
            host.dispatch_document(
                "document.load",
                json!({"window_id":"w1","html":"<p>new</p>"})
            )
            .unwrap_err()
            .code,
            protocol::code::CLOSED
        );
    }

    #[test]
    fn operation_decoder_rejects_unknown_fields_and_excluded_subsystems() {
        for command in [
            json!({"op":"set_text","element":"x","text":"value","window":3}),
            json!({"op":"print"}),
            json!({"op":"exec_edit","element":"x","command":"bold"}),
            json!({"op":"set_clipboard_html","html":"<b>bold</b>"}),
        ] {
            assert!(decode::<commands::DocumentCommand>(command).is_err());
        }
    }
}
