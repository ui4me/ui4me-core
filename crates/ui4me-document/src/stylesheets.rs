//! owner: 003-document-model
//!
//! Stylesheet source list (T031; research R-17; FR-087–FR-088; AD-28). DocumentCore owns the sources
//! and their order: connected `style` elements and connected `link` elements whose `rel` contains
//! the `stylesheet` keyword, in tree order, followed by host-attached author sheets in attachment
//! order. Attachments have stable ids. Fetching and parsing are 007's and 006's; this module only
//! lists sources and bumps the stylesheet revision (and journals `stylesheets`) when the list or a
//! source's text changes.

use crate::document::Document;
use crate::document::err;
use crate::node::{ElementData, NodeData};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Stable id of a host-attached sheet (never reused within a document).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttachmentId(pub u64);

/// Where a sheet comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetOrigin {
    Style(NodeId),
    Link(NodeId),
    Attached(AttachmentId),
}

/// One entry of the stylesheet source list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StylesheetEntry {
    pub origin: SheetOrigin,
    /// Inline text (`style` content or attached CSS); `None` for a `link` (007 supplies the bytes).
    pub text: Option<String>,
    /// `link href`.
    pub href: Option<String>,
    /// `media` attribute, if any.
    pub media: Option<String>,
}

/// Host-attached sheets: `(id, css)` in attachment order.
#[derive(Clone, Debug, Default)]
pub struct Attachments {
    pub(crate) next: u64,
    pub(crate) sheets: Vec<(AttachmentId, String)>,
}

/// Is this element a stylesheet source: an HTML `style`, or a `link` whose `rel` has `stylesheet`?
pub fn is_stylesheet_source(e: &ElementData) -> bool {
    if !e.is_html() {
        return false;
    }
    match e.local_name() {
        "style" => true,
        "link" => e.attr("rel").is_some_and(|r| {
            r.split_ascii_whitespace()
                .any(|t| t.eq_ignore_ascii_case("stylesheet"))
        }),
        _ => false,
    }
}

impl Document {
    /// Is `n` a `style` element or inside one (its text children are the sheet text)?
    pub(crate) fn inside_style_element(&self, n: NodeId) -> bool {
        let mut cur = Some(n);
        while let Some(x) = cur {
            if self
                .arena
                .node(x)
                .as_element()
                .is_some_and(|e| e.is_html_named("style"))
            {
                return true;
            }
            cur = self.arena.node(x).parent;
        }
        false
    }

    /// The ordered source list (A-15): connected sources in tree order, then attached sheets.
    pub fn stylesheets(&self) -> Vec<StylesheetEntry> {
        let mut out = vec![];
        for n in self.arena.subtree(self.arena.document()) {
            let Some(e) = self.arena.node(n).as_element() else {
                continue;
            };
            if !is_stylesheet_source(e) {
                continue;
            }
            if e.local_name() == "style" {
                let text: String = self
                    .arena
                    .node(n)
                    .children
                    .iter()
                    .filter_map(|&c| match &self.arena.node(c).data {
                        NodeData::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                out.push(StylesheetEntry {
                    origin: SheetOrigin::Style(n),
                    text: Some(text),
                    href: None,
                    media: e.attr("media").map(str::to_owned),
                });
            } else {
                out.push(StylesheetEntry {
                    origin: SheetOrigin::Link(n),
                    text: None,
                    href: e.attr("href").map(str::to_owned),
                    media: e.attr("media").map(str::to_owned),
                });
            }
        }
        for (id, css) in &self.attachments.sheets {
            out.push(StylesheetEntry {
                origin: SheetOrigin::Attached(*id),
                text: Some(css.clone()),
                href: None,
                media: None,
            });
        }
        out
    }

    /// Attach an author sheet (A-15). One revision step, a `stylesheets` journal entry.
    pub fn attach_stylesheet(&mut self, css: &str) -> Result<AttachmentId, Status> {
        self.check_open()?;
        self.check_string(css)?;
        self.begin_call();
        let rev = self.pending_revision();
        self.attachments.next += 1;
        let id = AttachmentId(self.attachments.next);
        self.attachments.sheets.push((id, css.to_owned()));
        self.note_stylesheets_changed(rev);
        self.end_call();
        Ok(id)
    }

    /// Replace an attached sheet's text (A-15). Unknown id: `INVALID_ARGUMENT`.
    pub fn replace_stylesheet(&mut self, id: AttachmentId, css: &str) -> Result<(), Status> {
        self.check_open()?;
        self.check_string(css)?;
        let Some(i) = self.attachments.sheets.iter().position(|(a, _)| *a == id) else {
            return Err(err(ui4me_engine_types::codes_generated::Code::StaleHandle));
        };
        if self.attachments.sheets[i].1 == css {
            return Ok(());
        }
        self.begin_call();
        let rev = self.pending_revision();
        self.attachments.sheets[i].1 = css.to_owned();
        self.note_stylesheets_changed(rev);
        self.end_call();
        Ok(())
    }

    /// Detach an attached sheet (A-15).
    pub fn detach_stylesheet(&mut self, id: AttachmentId) -> Result<(), Status> {
        self.check_open()?;
        let Some(i) = self.attachments.sheets.iter().position(|(a, _)| *a == id) else {
            return Err(err(ui4me_engine_types::codes_generated::Code::StaleHandle));
        };
        self.begin_call();
        let rev = self.pending_revision();
        self.attachments.sheets.remove(i);
        self.note_stylesheets_changed(rev);
        self.end_call();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_list_is_tree_order_then_attachments_with_stable_ids() {
        let mut d = Document::parse_html("<head><style>a{}</style><link rel='stylesheet alternate' href='x.css'><link rel=icon href=i></head><body><style media=print>b{}</style></body>").unwrap();
        let a1 = d.attach_stylesheet("c{}").unwrap();
        let a2 = d.attach_stylesheet("d{}").unwrap();
        assert_ne!(a1, a2);
        let l = d.stylesheets();
        assert_eq!(l.len(), 5);
        assert!(
            matches!(l[0].origin, SheetOrigin::Style(_)) && l[0].text.as_deref() == Some("a{}")
        );
        assert!(
            matches!(l[1].origin, SheetOrigin::Link(_)) && l[1].href.as_deref() == Some("x.css")
        );
        assert!(
            matches!(l[2].origin, SheetOrigin::Style(_)) && l[2].media.as_deref() == Some("print")
        );
        assert_eq!(l[3].origin, SheetOrigin::Attached(a1));
        let rev = d.stylesheet_revision();
        d.replace_stylesheet(a1, "c{color:red}").unwrap();
        assert_eq!(d.stylesheet_revision(), rev + 1);
        d.detach_stylesheet(a1).unwrap();
        assert_eq!(d.stylesheets().len(), 4);
        assert!(d.detach_stylesheet(a1).is_err());
    }

    #[test]
    fn changing_the_text_of_a_connected_style_element_bumps_the_revision() {
        let mut d = Document::parse_html("<head><style id=s>a{}</style></head>").unwrap();
        let s = d.element_by_id("s").unwrap();
        let rev = d.stylesheet_revision();
        d.set_text_content(s, "b{}").unwrap();
        assert_eq!(
            d.stylesheet_revision(),
            rev + 1,
            "replace-all of the children"
        );
        let t = d.children(s).unwrap()[0];
        d.set_data(t, "c{}").unwrap();
        assert_eq!(
            d.stylesheet_revision(),
            rev + 2,
            "character data of the text child"
        );
        d.append_data(t, "d{}").unwrap();
        assert_eq!(d.stylesheet_revision(), rev + 3);
        let extra = d.create_text_node("e{}").unwrap();
        d.append_child(s, extra).unwrap();
        assert_eq!(d.stylesheet_revision(), rev + 4, "a new text child");
        assert_eq!(d.stylesheets()[0].text.as_deref(), Some("c{}d{}e{}"));
        // a detached style element does not count
        let det = d.create_element("style").unwrap();
        let rev2 = d.stylesheet_revision();
        d.set_text_content(det, "x{}").unwrap();
        assert_eq!(d.stylesheet_revision(), rev2);
    }

    #[test]
    fn connecting_and_disconnecting_sources_bumps_the_revision_only_when_connected() {
        let mut d = Document::parse_html("<body></body>").unwrap();
        let body = d.body().unwrap();
        let s = d.create_element("style").unwrap();
        let rev = d.stylesheet_revision();
        d.append_child(body, s).unwrap();
        assert_eq!(d.stylesheet_revision(), rev + 1, "connect");
        d.remove_child(body, s).unwrap();
        assert_eq!(d.stylesheet_revision(), rev + 2, "disconnect");
        let div = d.create_element("div").unwrap();
        let s2 = d.create_element("style").unwrap();
        d.append_child(div, s2).unwrap();
        assert_eq!(
            d.stylesheet_revision(),
            rev + 2,
            "a detached tree is not in the list"
        );
        d.append_child(body, div).unwrap();
        assert_eq!(
            d.stylesheet_revision(),
            rev + 3,
            "connecting a subtree that holds a source"
        );
    }
}
