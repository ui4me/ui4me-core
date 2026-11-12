//! owner: 003-document-model
//!
//! Character data (T045; FR-044–FR-046, FR-098; research R-08; OD-22): `data`, `length`,
//! `substringData`, `appendData`, `insertData`, `deleteData`, `replaceData`, `splitText`,
//! `wholeText`, with UTF-16 code-unit offsets over UTF-8 storage. `INDEX_SIZE` and `SURROGATE_SPLIT`
//! are checked before any change. ASCII text takes a fast path (offsets are byte offsets). Counts
//! are clamped to the data's end. Every change is one call and one revision step; a change that
//! leaves the data equal is no mutation.

use crate::document::Document;
use crate::errors::{index_size, surrogate_split, wrong_node_kind};
use crate::journal::EntryKind;
use crate::node::{NodeData, NodeKind};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// UTF-16 length of `s`.
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().map(char::len_utf16).sum()
    }
}

/// Byte offset of UTF-16 offset `off` in `s`: `Err(IndexSize)` past the end, `Err(SurrogateSplit)`
/// inside a surrogate pair.
pub(crate) fn byte_offset(s: &str, off: usize) -> Result<usize, Status> {
    if s.is_ascii() {
        return if off <= s.len() {
            Ok(off)
        } else {
            Err(index_size())
        };
    }
    let mut units = 0usize;
    for (b, c) in s.char_indices() {
        if units == off {
            return Ok(b);
        }
        let w = c.len_utf16();
        if off < units + w {
            return Err(surrogate_split());
        }
        units += w;
    }
    if units == off {
        Ok(s.len())
    } else {
        Err(index_size())
    }
}

impl Document {
    fn cdata(&self, n: NodeId) -> Result<&str, Status> {
        match &self.check(n)?.data {
            NodeData::Text(t) | NodeData::Comment(t) => Ok(t),
            _ => Err(wrong_node_kind()),
        }
    }

    /// `length` in UTF-16 code units (A-60).
    pub fn data_length(&self, n: NodeId) -> Result<usize, Status> {
        Ok(utf16_len(self.cdata(n)?))
    }

    /// `substringData(offset, count)` (A-60).
    pub fn substring_data(&self, n: NodeId, offset: usize, count: usize) -> Result<String, Status> {
        let s = self.cdata(n)?;
        let start = byte_offset(s, offset)?;
        let end = byte_offset_clamped(s, offset, count)?;
        Ok(s[start..end].to_owned())
    }

    /// Replace the whole data (`data` setter / `nodeValue` setter / text-content setter).
    pub fn set_data(&mut self, n: NodeId, data: &str) -> Result<(), Status> {
        let cur = self.cdata(n)?;
        self.check_string(data)?;
        if cur == data {
            return Ok(());
        }
        let new = data.to_owned();
        self.commit_data(n, new);
        Ok(())
    }

    /// `replaceData(offset, count, data)` (A-60).
    pub fn replace_data(
        &mut self,
        n: NodeId,
        offset: usize,
        count: usize,
        data: &str,
    ) -> Result<(), Status> {
        let s = self.cdata(n)?;
        let start = byte_offset(s, offset)?;
        let end = byte_offset_clamped(s, offset, count)?;
        let mut new = String::with_capacity(s.len() - (end - start) + data.len());
        new.push_str(&s[..start]);
        new.push_str(data);
        new.push_str(&s[end..]);
        self.check_string(&new)?;
        if new == s {
            return Ok(());
        }
        self.commit_data(n, new);
        Ok(())
    }

    pub fn append_data(&mut self, n: NodeId, data: &str) -> Result<(), Status> {
        let len = self.data_length(n)?;
        self.replace_data(n, len, 0, data)
    }

    pub fn insert_data(&mut self, n: NodeId, offset: usize, data: &str) -> Result<(), Status> {
        self.replace_data(n, offset, 0, data)
    }

    pub fn delete_data(&mut self, n: NodeId, offset: usize, count: usize) -> Result<(), Status> {
        self.replace_data(n, offset, count, "")
    }

    fn commit_data(&mut self, n: NodeId, new: String) {
        let in_style = self.touches_style_sources(n);
        let connected = self.arena.is_connected(n);
        self.begin_call();
        let rev = self.pending_revision();
        if let Some(NodeData::Text(t) | NodeData::Comment(t)) =
            self.arena.get_mut(n).map(|x| &mut x.data)
        {
            *t = new;
        }
        self.journal.push(rev, EntryKind::CharacterData { node: n });
        if in_style && connected {
            self.note_stylesheets_changed(rev);
        }
        self.end_call();
    }

    /// `splitText(offset)` (A-61): truncates the node and inserts a new Text node with the tail
    /// after it (when it has a parent). Returns the new node.
    pub fn split_text(&mut self, n: NodeId, offset: usize) -> Result<NodeId, Status> {
        if self.check(n)?.kind() != NodeKind::Text {
            return Err(wrong_node_kind());
        }
        let s = self.cdata(n)?;
        let at = byte_offset(s, offset)?;
        let tail = s[at..].to_owned();
        self.check_node_budget(1)?;
        let head = s[..at].to_owned();
        self.begin_call();
        let rev = self.pending_revision();
        let new = self.arena.alloc(NodeData::Text(tail));
        if let Some(parent) = self.arena.node(n).parent {
            let next = self.next_sibling(n).expect("live");
            self.insert_step(rev, parent, vec![new], next);
        }
        if let Some(NodeData::Text(t)) = self.arena.get_mut(n).map(|x| &mut x.data) {
            *t = head;
        }
        self.journal.push(rev, EntryKind::CharacterData { node: n });
        self.end_call();
        Ok(new)
    }

    /// `wholeText` (A-61): the data of the node's contiguous Text siblings, in order.
    pub fn whole_text(&self, n: NodeId) -> Result<String, Status> {
        if self.check(n)?.kind() != NodeKind::Text {
            return Err(wrong_node_kind());
        }
        let Some(p) = self.arena.node(n).parent else {
            return Ok(self.cdata(n)?.to_owned());
        };
        let kids = &self.arena.node(p).children;
        let i = kids.iter().position(|c| *c == n).expect("tree invariant");
        let is_text = |c: NodeId| matches!(self.arena.node(c).data, NodeData::Text(_));
        let mut a = i;
        while a > 0 && is_text(kids[a - 1]) {
            a -= 1;
        }
        let mut b = i;
        while b + 1 < kids.len() && is_text(kids[b + 1]) {
            b += 1;
        }
        Ok(kids[a..=b]
            .iter()
            .filter_map(|&c| self.arena.node(c).character_data())
            .collect())
    }
}

fn byte_offset_clamped(s: &str, offset: usize, count: usize) -> Result<usize, Status> {
    let total = utf16_len(s);
    // offset was validated by the caller; clamp the end to the data's length.
    let end_units = offset.saturating_add(count).min(total);
    byte_offset(s, end_units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::{Code, SubReason};

    fn doc_with(text: &str) -> (Document, NodeId) {
        let mut d = Document::parse_html("<p></p>").unwrap();
        let p = d.element_by_id("nope").unwrap_or_else(|| d.body().unwrap());
        let t = d.create_text_node(text).unwrap();
        d.append_child(p, t).unwrap();
        (d, t)
    }

    #[test]
    fn utf16_offsets_over_utf8_storage() {
        let (mut d, t) = doc_with("a\u{1F600}b");
        assert_eq!(d.data_length(t).unwrap(), 4, "the emoji is two code units");
        assert_eq!(d.substring_data(t, 0, 1).unwrap(), "a");
        assert_eq!(d.substring_data(t, 1, 2).unwrap(), "\u{1F600}");
        assert_eq!(d.substring_data(t, 3, 100).unwrap(), "b", "counts clamp");
        d.insert_data(t, 3, "X").unwrap();
        assert_eq!(d.text_data(t).unwrap(), "a\u{1F600}Xb");
        d.delete_data(t, 1, 2).unwrap();
        assert_eq!(d.text_data(t).unwrap(), "aXb");
        d.replace_data(t, 1, 1, "YY").unwrap();
        assert_eq!(d.text_data(t).unwrap(), "aYYb");
        d.append_data(t, "!").unwrap();
        assert_eq!(d.text_data(t).unwrap(), "aYYb!");
    }

    #[test]
    fn index_size_and_surrogate_split_leave_everything_unchanged() {
        let (mut d, t) = doc_with("a\u{1F600}b");
        let rev = d.revision();
        let e = d.insert_data(t, 5, "x").unwrap_err();
        assert_eq!(
            (e.code, e.sub_reason),
            (Code::InvalidArgument, Some(SubReason::IndexSize))
        );
        let e = d.delete_data(t, 2, 1).unwrap_err();
        assert_eq!(
            (e.code, e.sub_reason),
            (Code::InvalidArgument, Some(SubReason::SurrogateSplit))
        );
        let e = d.replace_data(t, 1, 1, "x").unwrap_err();
        assert_eq!(
            e.sub_reason,
            Some(SubReason::SurrogateSplit),
            "the end offset splits the pair"
        );
        let e = d.substring_data(t, 9, 1).unwrap_err();
        assert_eq!(e.sub_reason, Some(SubReason::IndexSize));
        assert_eq!(d.text_data(t).unwrap(), "a\u{1F600}b");
        assert_eq!(d.revision(), rev);
        // wrong node kind
        let el = d.body().unwrap();
        assert_eq!(
            d.data_length(el).unwrap_err().sub_reason,
            Some(SubReason::WrongNodeKind)
        );
    }

    #[test]
    fn no_change_is_no_mutation_and_each_change_is_one_revision() {
        let (mut d, t) = doc_with("abc");
        let r0 = d.revision();
        d.replace_data(t, 1, 0, "").unwrap();
        d.set_data(t, "abc").unwrap();
        assert_eq!(d.revision(), r0);
        d.set_data(t, "abd").unwrap();
        assert_eq!(d.revision(), r0 + 1);
    }

    #[test]
    fn split_text_and_whole_text() {
        let (mut d, t) = doc_with("hello world");
        let tail = d.split_text(t, 6).unwrap();
        assert_eq!(d.text_data(t).unwrap(), "hello ");
        assert_eq!(d.text_data(tail).unwrap(), "world");
        assert_eq!(d.next_sibling(t).unwrap(), Some(tail));
        assert_eq!(d.whole_text(t).unwrap(), "hello world");
        assert_eq!(
            d.split_text(t, 99).unwrap_err().sub_reason,
            Some(SubReason::IndexSize)
        );
        let (mut d2, t2) = doc_with("\u{1F600}");
        assert_eq!(
            d2.split_text(t2, 1).unwrap_err().sub_reason,
            Some(SubReason::SurrogateSplit)
        );
    }
}
