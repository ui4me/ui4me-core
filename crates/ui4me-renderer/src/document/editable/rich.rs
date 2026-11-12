//! Rich hosts: edits through the `Rich` tree (see the `editable` module docs).

use super::*;

impl Document {
    /// One rich edit: `beforeinput`, `op` on the host's tree for the flat range `s..e` (it returns
    /// the new selection, or `None` for no change), the write-back, the selection, `input`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rich_edit(
        &mut self,
        host: NodeId,
        s: usize,
        e: usize,
        input_type: &str,
        data: Option<&str>,
        record: bool,
        op: impl FnOnce(&mut Rich, usize, usize, &[(Fmt, bool)]) -> Option<(usize, usize)>,
    ) -> Result<bool> {
        let composing = self.editable.comp.is_some();
        if self.fire_input(host, "beforeinput", input_type, data, !composing, composing)
            && !composing
        {
            return Ok(false);
        }
        let d = self.pipeline.document();
        if !d.arena().contains(host) || !d.is_connected(host).unwrap_or(false) {
            return Ok(false);
        }
        let mut rich = self.rich_load(host);
        let before = rich.html();
        let len = rich.flat_len();
        let (s, e) = (s.min(e).min(len), s.max(e).min(len));
        let pend = match self.editable.pending.take() {
            Some((h, at, f)) if h == host && at == s && s == e => f,
            _ => Vec::new(),
        };
        let sel_before = self.host_range(host);
        let Some(sel) = op(&mut rich, s, e, &pend) else {
            return Ok(false);
        };
        let html = rich.html();
        if html.len() > MAX_HOST_TEXT * 4 {
            return Ok(false);
        }
        if record {
            self.editable.rich_undo.push(RichSnap {
                host,
                html: before,
                sel: sel_before,
            });
            self.trim_rich_history();
            self.editable.rich_redo.clear();
        }
        self.pipeline.mutate(|d| d.set_inner_html(host, &html))?;
        self.after_mutation(true);
        let fresh = self.rich_load(host);
        self.set_rich_selection(host, &fresh, sel.0, sel.1);
        self.fire_input(host, "input", input_type, data, false, composing);
        Ok(true)
    }
    fn trim_rich_history(&mut self) {
        let st = &mut self.editable;
        while st.rich_undo.len() > MAX_UNDO || snap_bytes(&st.rich_undo) > MAX_RICH_UNDO_BYTES {
            st.rich_undo.remove(0);
        }
        while st.rich_redo.len() > MAX_UNDO || snap_bytes(&st.rich_redo) > MAX_RICH_UNDO_BYTES {
            st.rich_redo.remove(0);
        }
    }
    pub(super) fn rich_undo(&mut self, host: NodeId, undo: bool) -> Result<bool> {
        let from = if undo {
            &mut self.editable.rich_undo
        } else {
            &mut self.editable.rich_redo
        };
        let Some(pos) = from.iter().rposition(|x| x.host == host) else {
            return Ok(false);
        };
        let snap = from.remove(pos);
        let input_type = if undo { "historyUndo" } else { "historyRedo" };
        if self.fire_input(host, "beforeinput", input_type, None, true, false) {
            // Canceled: keep the step available.
            let from = if undo {
                &mut self.editable.rich_undo
            } else {
                &mut self.editable.rich_redo
            };
            from.insert(pos, snap);
            return Ok(false);
        }
        let now = RichSnap {
            host,
            html: self.rich_load(host).html(),
            sel: self.host_range(host),
        };
        let to = if undo {
            &mut self.editable.rich_redo
        } else {
            &mut self.editable.rich_undo
        };
        to.push(now);
        self.trim_rich_history();
        self.editable.pending = None;
        self.pipeline
            .mutate(|d| d.set_inner_html(host, &snap.html))?;
        self.after_mutation(true);
        let fresh = self.rich_load(host);
        let len = fresh.flat_len();
        self.set_rich_selection(host, &fresh, snap.sel.0.min(len), snap.sel.1.min(len));
        self.fire_input(host, "input", input_type, None, false, false);
        Ok(true)
    }
    pub(super) fn rich_line_break(&mut self, host: NodeId, s: usize, e: usize) -> Result<bool> {
        self.rich_edit(host, s, e, "insertLineBreak", None, true, |r, s, e, _| {
            r.delete_range(s, e);
            let c = r.insert_br(s);
            Some((c, c))
        })
    }
    /// Toggle `fmt` over the selection, or for the next typed text at a caret.
    pub(super) fn rich_format(&mut self, host: NodeId, fmt: Fmt) -> Result<bool> {
        let (a, f) = self.host_range(host);
        let (s, e) = (a.min(f), a.max(f));
        let st = self.rich_load(host).state(s, e);
        let current = |st: &EditState| match fmt {
            Fmt::Bold => st.bold,
            Fmt::Italic => st.italic,
            Fmt::Underline => st.underline,
            Fmt::Strike => st.strikethrough,
            Fmt::Code => st.code,
            Fmt::Link => st.link.is_some(),
        };
        if s == e {
            let mut list = match self.editable.pending.take() {
                Some((h, at, l)) if h == host && at == s => l,
                _ => Vec::new(),
            };
            let eff = list
                .iter()
                .find(|x| x.0 == fmt)
                .map_or(current(&st), |x| x.1);
            list.retain(|x| x.0 != fmt);
            list.push((fmt, !eff));
            self.editable.pending = Some((host, s, list));
            return Ok(true);
        }
        let on = !current(&st);
        self.rich_edit(
            host,
            s,
            e,
            fmt.input_type(),
            None,
            true,
            move |r, s, e, _| {
                r.set_format(s, e, fmt, on);
                Some((a, f))
            },
        )
    }
    fn rich_state(&self, host: NodeId) -> EditState {
        let (a, f) = self.host_range(host);
        let (s, e) = (a.min(f), a.max(f));
        let mut st = self.rich_load(host).state(s, e);
        if let Some((h, at, list)) = &self.editable.pending
            && *h == host
            && *at == s
            && s == e
        {
            for &(fmt, on) in list {
                match fmt {
                    Fmt::Bold => st.bold = on,
                    Fmt::Italic => st.italic = on,
                    Fmt::Underline => st.underline = on,
                    Fmt::Strike => st.strikethrough = on,
                    Fmt::Code => st.code = on,
                    Fmt::Link => {}
                }
            }
        }
        st
    }
    /// `execCommand`-style editing commands (F1003): `bold`, `italic`, `underline`,
    /// `strikethrough`, `code`, `link` (value = http/https/mailto URL), `unlink`,
    /// `remove_format`, `insert_text`, `insert_paragraph`, `undo`, `redo`. They act on the
    /// element's selection (a caret at the end when it has none) and return whether anything
    /// changed; formatting needs a rich host.
    pub fn exec_edit(&mut self, element: &str, command: &str, value: Option<&str>) -> Result<bool> {
        let host = self.node(element)?;
        if !is_editing_host(self.pipeline.document(), host) {
            return Err(arg("exec_edit requires a contenteditable host"));
        }
        let rich = self.is_rich_host(host);
        let (a, f) = self.host_range(host);
        let (lo, hi) = (a.min(f), a.max(f));
        let ok = match command {
            "bold" | "italic" | "underline" | "strikethrough" | "code" => {
                let fmt = match command {
                    "bold" => Fmt::Bold,
                    "italic" => Fmt::Italic,
                    "underline" => Fmt::Underline,
                    "strikethrough" => Fmt::Strike,
                    _ => Fmt::Code,
                };
                rich && self.rich_format(host, fmt)?
            }
            "link" => {
                let href = value.unwrap_or_default().trim().to_owned();
                if !safe_href(&href) {
                    return Err(arg("link requires an http, https or mailto URL"));
                }
                rich && lo < hi
                    && self.rich_edit(
                        host,
                        lo,
                        hi,
                        "insertLink",
                        Some(&href),
                        true,
                        |r, s, e, _| {
                            r.set_link(s, e, &href);
                            Some((a, f))
                        },
                    )?
            }
            "unlink" => {
                rich && self.rich_edit(host, lo, hi, "deleteLink", None, true, |r, s, e, _| {
                    r.unlink(s, e);
                    Some((a, f))
                })?
            }
            "remove_format" => {
                rich && lo < hi
                    && self.rich_edit(host, lo, hi, "formatRemove", None, true, |r, s, e, _| {
                        r.remove_format(s, e);
                        Some((a, f))
                    })?
            }
            "insert_text" => {
                let t: String = value
                    .unwrap_or_default()
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                    .collect();
                !t.is_empty() && self.host_edit(host, lo, hi, &t, "insertText", true)?
            }
            "insert_paragraph" => self.host_edit(host, lo, hi, "\n", "insertParagraph", true)?,
            "undo" => self.host_undo(host, true)?,
            "redo" => self.host_undo(host, false)?,
            _ => return Err(arg(format!("Unknown edit command {command:?}"))),
        };
        if ok && self.focused_host() == Some(host) {
            self.reveal_caret(host)?;
        }
        Ok(ok)
    }
    /// The formats at the selection of a rich host (all `false` for a plain-text host).
    pub fn get_edit_state(&mut self, element: &str) -> Result<Value> {
        let host = self.node(element)?;
        if !is_editing_host(self.pipeline.document(), host) {
            return Err(arg("get_edit_state requires a contenteditable host"));
        }
        let st = if self.is_rich_host(host) {
            self.rich_state(host)
        } else {
            EditState::default()
        };
        Ok(json!({
            "bold": st.bold, "italic": st.italic, "underline": st.underline,
            "strikethrough": st.strikethrough, "code": st.code, "link": st.link,
        }))
    }
}
