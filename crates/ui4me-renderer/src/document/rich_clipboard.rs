//! Rich clipboard formats (0.7 F0706): an HTML flavour with its plain-text alternative, and a
//! paste sanitizer. Clipboard HTML comes from other applications and is untrusted: the
//! sanitized form keeps text and ordinary formatting and drops scripts, styles, embedded
//! content, event-handler attributes and non-http(s)/mailto URLs, so an app may insert it.

use super::*;
use ui4me_document::node::NodeKind;

/// Clipboard HTML Core reads or writes (bytes).
const MAX_CLIPBOARD_HTML: usize = 4 * 1024 * 1024;

const DROPPED_ELEMENTS: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "frame", "frameset", "applet", "link", "meta",
    "base", "form", "input", "button", "select", "textarea", "template", "noscript", "svg", "math",
];

fn safe_url(v: &str) -> bool {
    let v = v.trim().to_ascii_lowercase();
    !v.contains(':')
        || v.starts_with("https:")
        || v.starts_with("http:")
        || v.starts_with("mailto:")
}

/// Sanitize untrusted HTML into a fragment safe to insert (see the module docs).
pub fn sanitize_html(html: &str) -> Result<String> {
    if html.len() > MAX_CLIPBOARD_HTML {
        return Err(limit_error(format!(
            "Clipboard HTML exceeds {MAX_CLIPBOARD_HTML} bytes"
        )));
    }
    let id = DocumentId::from_raw(RawId {
        index: u32::MAX - 7,
        generation: NonZeroU32::MIN,
    });
    let mut d = Model::create(id, html, "about:blank", DomLimits::default())?;
    let Some(body) = d.body() else {
        return Ok(String::new());
    };
    let mut doomed = Vec::new();
    let mut attrs = Vec::new();
    for n in d.arena().subtree(body) {
        match d.node_kind(n)? {
            NodeKind::Element => {
                let name = d.local_name(n)?.to_owned();
                if DROPPED_ELEMENTS.contains(&name.as_str()) {
                    doomed.push(n);
                    continue;
                }
                for a in d.attributes(n)? {
                    let an = a.name.local.to_string().to_ascii_lowercase();
                    let unsafe_attr = an.starts_with("on")
                        || an == "style"
                        || an == "srcdoc"
                        || ((an == "href"
                            || an == "src"
                            || an == "action"
                            || an == "formaction"
                            || an == "xlink:href")
                            && !safe_url(&a.value));
                    if unsafe_attr {
                        attrs.push((n, a.name.local.to_string()));
                    }
                }
            }
            NodeKind::Comment => doomed.push(n),
            _ => {}
        }
    }
    for (n, a) in attrs {
        if d.arena().contains(n) {
            let _ = d.remove_attribute(n, &a);
        }
    }
    for n in doomed {
        if d.arena().contains(n) && d.is_connected(n).unwrap_or(false) {
            let _ = d.remove(n);
        }
    }
    Ok(d.inner_html(body)?)
}

impl Document {
    /// Put HTML with its plain-text alternative on the clipboard (0.7 F0706).
    pub fn set_clipboard_html(&mut self, html: &str, text: &str) -> Result<()> {
        if html.len() > MAX_CLIPBOARD_HTML
            || html.contains('\0')
            || text.len() > 16 * 1024 * 1024
            || text.contains('\0')
        {
            return Err(arg(
                "Clipboard HTML is at most 4 MiB and text at most 16 MiB, without NUL",
            ));
        }
        self.clipboard.write_html(html, text).map_err(|e| Error {
            code: 6,
            message: format!("Clipboard write failed: {e}"),
        })
    }
    /// The clipboard's HTML flavour, sanitized unless `raw` (0.7 F0706). `None` when it has none.
    pub fn clipboard_html(&mut self, raw: bool) -> Result<Option<String>> {
        let html = self.clipboard.read_html().map_err(|e| Error {
            code: 6,
            message: format!("Clipboard read failed: {e}"),
        })?;
        match html {
            None => Ok(None),
            Some(h) if h.len() > MAX_CLIPBOARD_HTML => {
                Err(limit_error("Clipboard HTML exceeds 4 MiB"))
            }
            Some(h) if raw => Ok(Some(h)),
            Some(h) => Ok(Some(sanitize_html(&h)?)),
        }
    }
}
