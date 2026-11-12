//! owner: 003-document-model
//!
//! Batch command-buffer decoding (T077; contracts/batch-command-buffer.md sections 1-3; FR-071-FR-075).
//! The whole buffer is decoded before anything is applied. Decode errors are `INVALID_ARGUMENT` with
//! the `byte_offset` of the offending field; limits are `SIZE_LIMIT`. Operand encodings:
//! `NODE` = `tag:u32` then for tag 1 `doc:u64, node:u64` (the 004 handle words, decoded here as the
//! opaque `generation << 32 | index` forms), for tag 2 `local:u32` + 4 zero bytes, for tag 0 nothing
//! (null). `STR` = `len:u32` + bytes + zero padding to 4, `0xFFFF_FFFF` = null where nullable.
//! `TRISTATE`: 0 absent, 1 false, 2 true.

use crate::errors::size_limit;
use crate::limits::{DomLimits, LimitName};
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

pub const MAGIC: &[u8; 4] = b"U4MB";
pub const VERSION: u16 = 1;
pub const NULL_STR: u32 = 0xFFFF_FFFF;

/// A decoded NODE operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Null,
    /// An existing handle: `(document word, node word)`.
    Existing(u64, u64),
    /// A batch-local id created earlier in the buffer.
    Local(u32),
}

/// One decoded command; `LOCAL_OUT` ids are implicit (the n-th creating command defines local n).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    CreateElement {
        ns: Option<String>,
        qname: String,
    },
    CreateText {
        data: String,
    },
    CreateComment {
        data: String,
    },
    CreateFragment,
    Clone {
        node: Operand,
        deep: bool,
    },
    AppendChild {
        parent: Operand,
        child: Operand,
    },
    InsertBefore {
        parent: Operand,
        child: Operand,
        reference: Operand,
    },
    Remove {
        node: Operand,
    },
    ReplaceChild {
        parent: Operand,
        new: Operand,
        old: Operand,
    },
    SetAttribute {
        el: Operand,
        ns: Option<String>,
        qname: String,
        value: String,
    },
    RemoveAttribute {
        el: Operand,
        ns: Option<String>,
        name: String,
    },
    ToggleAttribute {
        el: Operand,
        qname: String,
        force: Option<bool>,
    },
    ClassAdd {
        el: Operand,
        token: String,
    },
    ClassRemove {
        el: Operand,
        token: String,
    },
    ClassToggle {
        el: Operand,
        token: String,
        force: Option<bool>,
    },
    SetTextContent {
        node: Operand,
        text: Option<String>,
    },
    SetData {
        node: Operand,
        data: String,
    },
    SetInnerHtml {
        el: Operand,
        html: String,
    },
}

impl Cmd {
    /// Does this command define a new batch-local id?
    pub fn defines_local(&self) -> bool {
        matches!(
            self,
            Cmd::CreateElement { .. }
                | Cmd::CreateText { .. }
                | Cmd::CreateComment { .. }
                | Cmd::CreateFragment
                | Cmd::Clone { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub cmds: Vec<Cmd>,
    pub local_count: u32,
}

fn bad(at: usize, why: &str) -> Status {
    Status::new(Code::InvalidArgument, Stage::Host)
        .with_byte_offset(at as u64)
        .with_message(why.to_owned())
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> Result<(), Status> {
        if self.pos + n > self.b.len() {
            Err(bad(self.pos, "buffer ends inside a field"))
        } else {
            Ok(())
        }
    }
    fn u16(&mut self) -> Result<u16, Status> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.b[self.pos], self.b[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, Status> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.b[self.pos..self.pos + 4].try_into().expect("4 bytes"));
        self.pos += 4;
        Ok(v)
    }
    fn u64(&mut self) -> Result<u64, Status> {
        self.need(8)?;
        let v = u64::from_le_bytes(self.b[self.pos..self.pos + 8].try_into().expect("8 bytes"));
        self.pos += 8;
        Ok(v)
    }
    fn zero_pad(&mut self, n: usize) -> Result<(), Status> {
        self.need(n)?;
        if self.b[self.pos..self.pos + n].iter().any(|&x| x != 0) {
            return Err(bad(self.pos, "padding bytes must be zero"));
        }
        self.pos += n;
        Ok(())
    }
    fn str_(&mut self, nullable: bool) -> Result<Option<String>, Status> {
        let at = self.pos;
        let len = self.u32()?;
        if len == NULL_STR {
            return if nullable {
                Ok(None)
            } else {
                Err(bad(at, "null string where a string is required"))
            };
        }
        let len = len as usize;
        self.need(len)?;
        let s = std::str::from_utf8(&self.b[self.pos..self.pos + len])
            .map_err(|_| bad(self.pos, "string is not valid UTF-8"))?
            .to_owned();
        self.pos += len;
        self.zero_pad((4 - len % 4) % 4)?;
        Ok(Some(s))
    }
    fn string(&mut self) -> Result<String, Status> {
        Ok(self.str_(false)?.expect("non-nullable"))
    }
    fn bool_(&mut self) -> Result<bool, Status> {
        let at = self.pos;
        match self.u32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(bad(at, "BOOL must be 0 or 1")),
        }
    }
    fn tristate(&mut self) -> Result<Option<bool>, Status> {
        let at = self.pos;
        match self.u32()? {
            0 => Ok(None),
            1 => Ok(Some(false)),
            2 => Ok(Some(true)),
            _ => Err(bad(at, "TRISTATE must be 0, 1 or 2")),
        }
    }
}

/// Decode and validate a whole buffer (pure: no document access).
pub fn decode(buf: &[u8], limits: &DomLimits) -> Result<Batch, Status> {
    // Limits first: they do not need the body to be well formed (FR-093).
    limits.check(LimitName::BatchBytes, buf.len() as u64)?;
    if buf.len() < 16 {
        return Err(bad(0, "buffer shorter than the 16-byte header"));
    }
    let mut r = Reader { b: buf, pos: 0 };
    if &buf[0..4] != MAGIC {
        return Err(bad(0, "bad magic"));
    }
    r.pos = 4;
    let version = r.u16()?;
    if version != VERSION {
        return Err(bad(4, "unsupported version"));
    }
    let flags = r.u16()?;
    if flags != 0 {
        return Err(bad(6, "reserved flags must be zero"));
    }
    let count = r.u32()?;
    let local_count = r.u32()?;
    if u64::from(count) > limits.batch_commands {
        return Err(size_limit(LimitName::BatchCommands.name()));
    }
    let mut cmds = Vec::with_capacity((count as usize).min(1 << 16));
    let mut defined: u32 = 0;
    for _ in 0..count {
        if !r.pos.is_multiple_of(4) {
            return Err(bad(r.pos, "command not 4-byte aligned"));
        }
        let start = r.pos;
        let opcode = r.u16()?;
        let cmd_flags = r.u16()?;
        let length = r.u32()? as usize;
        if cmd_flags != 0 {
            return Err(bad(start + 2, "unknown command flag bits"));
        }
        if length < 8 || !length.is_multiple_of(4) || start + length > buf.len() {
            return Err(bad(start + 4, "bad command length"));
        }
        let end = start + length;
        let node = |r: &mut Reader, nullable: bool, defined: u32| -> Result<Operand, Status> {
            let at = r.pos;
            let tag = r.u32()?;
            match tag {
                0 if nullable => Ok(Operand::Null),
                0 => Err(bad(at, "null NODE where a node is required")),
                1 => {
                    let d = r.u64()?;
                    let n = r.u64()?;
                    Ok(Operand::Existing(d, n))
                }
                2 => {
                    let l = r.u32()?;
                    r.zero_pad(4)?;
                    if l >= defined {
                        return Err(bad(at + 4, "local id used before its definition"));
                    }
                    Ok(Operand::Local(l))
                }
                _ => Err(bad(at, "unknown NODE tag")),
            }
        };
        let local_out = |r: &mut Reader, defined: u32| -> Result<(), Status> {
            let at = r.pos;
            let l = r.u32()?;
            if l != defined {
                return Err(bad(
                    at,
                    "LOCAL_OUT ids must be defined in increasing order from 0",
                ));
            }
            Ok(())
        };
        let cmd = match opcode {
            0x01 => {
                local_out(&mut r, defined)?;
                let ns = r.str_(true)?;
                Cmd::CreateElement {
                    ns,
                    qname: r.string()?,
                }
            }
            0x02 => {
                local_out(&mut r, defined)?;
                Cmd::CreateText { data: r.string()? }
            }
            0x03 => {
                local_out(&mut r, defined)?;
                Cmd::CreateComment { data: r.string()? }
            }
            0x04 => {
                local_out(&mut r, defined)?;
                Cmd::CreateFragment
            }
            0x05 => {
                local_out(&mut r, defined)?;
                let n = node(&mut r, false, defined)?;
                Cmd::Clone {
                    node: n,
                    deep: r.bool_()?,
                }
            }
            0x10 => Cmd::AppendChild {
                parent: node(&mut r, false, defined)?,
                child: node(&mut r, false, defined)?,
            },
            0x11 => Cmd::InsertBefore {
                parent: node(&mut r, false, defined)?,
                child: node(&mut r, false, defined)?,
                reference: node(&mut r, true, defined)?,
            },
            0x12 => Cmd::Remove {
                node: node(&mut r, false, defined)?,
            },
            0x13 => Cmd::ReplaceChild {
                parent: node(&mut r, false, defined)?,
                new: node(&mut r, false, defined)?,
                old: node(&mut r, false, defined)?,
            },
            0x20 => {
                let el = node(&mut r, false, defined)?;
                let ns = r.str_(true)?;
                let qname = r.string()?;
                Cmd::SetAttribute {
                    el,
                    ns,
                    qname,
                    value: r.string()?,
                }
            }
            0x21 => {
                let el = node(&mut r, false, defined)?;
                let ns = r.str_(true)?;
                Cmd::RemoveAttribute {
                    el,
                    ns,
                    name: r.string()?,
                }
            }
            0x22 => Cmd::ToggleAttribute {
                el: node(&mut r, false, defined)?,
                qname: r.string()?,
                force: r.tristate()?,
            },
            0x30 => Cmd::ClassAdd {
                el: node(&mut r, false, defined)?,
                token: r.string()?,
            },
            0x31 => Cmd::ClassRemove {
                el: node(&mut r, false, defined)?,
                token: r.string()?,
            },
            0x32 => Cmd::ClassToggle {
                el: node(&mut r, false, defined)?,
                token: r.string()?,
                force: r.tristate()?,
            },
            0x40 => Cmd::SetTextContent {
                node: node(&mut r, false, defined)?,
                text: r.str_(true)?,
            },
            0x41 => Cmd::SetData {
                node: node(&mut r, false, defined)?,
                data: r.string()?,
            },
            0x50 => Cmd::SetInnerHtml {
                el: node(&mut r, false, defined)?,
                html: r.string()?,
            },
            _ => return Err(bad(start, "unknown opcode")),
        };
        if r.pos != end {
            return Err(bad(start + 4, "command length does not match its operands"));
        }
        if cmd.defines_local() {
            defined += 1;
        }
        cmds.push(cmd);
    }
    if r.pos != buf.len() {
        return Err(bad(r.pos, "trailing bytes after the last command"));
    }
    if defined != local_count {
        return Err(bad(
            12,
            "local_count does not match the number of defined ids",
        ));
    }
    Ok(Batch { cmds, local_count })
}

/// Test and host-side helper: an encoder for the same format (bindings build buffers client-side;
/// this is the reference used by the round-trip and fuzz tests).
#[derive(Default)]
pub struct Encoder {
    body: Vec<u8>,
    count: u32,
    locals: u32,
}

impl Encoder {
    pub fn new() -> Encoder {
        Encoder::default()
    }
    fn start(&mut self, opcode: u16, f: impl FnOnce(&mut Vec<u8>)) {
        let mut ops = vec![];
        f(&mut ops);
        let len = 8 + ops.len();
        self.body.extend_from_slice(&opcode.to_le_bytes());
        self.body.extend_from_slice(&0u16.to_le_bytes());
        self.body.extend_from_slice(&(len as u32).to_le_bytes());
        self.body.extend_from_slice(&ops);
        self.count += 1;
    }
    fn s(v: &mut Vec<u8>, s: Option<&str>) {
        match s {
            None => v.extend_from_slice(&NULL_STR.to_le_bytes()),
            Some(s) => {
                v.extend_from_slice(&(s.len() as u32).to_le_bytes());
                v.extend_from_slice(s.as_bytes());
                v.extend(std::iter::repeat_n(0u8, (4 - s.len() % 4) % 4));
            }
        }
    }
    fn n(v: &mut Vec<u8>, o: Operand) {
        match o {
            Operand::Null => v.extend_from_slice(&0u32.to_le_bytes()),
            Operand::Existing(d, n) => {
                v.extend_from_slice(&1u32.to_le_bytes());
                v.extend_from_slice(&d.to_le_bytes());
                v.extend_from_slice(&n.to_le_bytes());
            }
            Operand::Local(l) => {
                v.extend_from_slice(&2u32.to_le_bytes());
                v.extend_from_slice(&l.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }
    fn tri(f: Option<bool>) -> u32 {
        match f {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        }
    }
    /// Append a command; returns the local id it defines, if any.
    pub fn push(&mut self, c: &Cmd) -> Option<u32> {
        let out = c.defines_local().then(|| {
            let l = self.locals;
            self.locals += 1;
            l
        });
        let lo = out.unwrap_or(0).to_le_bytes();
        match c {
            Cmd::CreateElement { ns, qname } => self.start(0x01, |v| {
                v.extend_from_slice(&lo);
                Self::s(v, ns.as_deref());
                Self::s(v, Some(qname));
            }),
            Cmd::CreateText { data } => self.start(0x02, |v| {
                v.extend_from_slice(&lo);
                Self::s(v, Some(data));
            }),
            Cmd::CreateComment { data } => self.start(0x03, |v| {
                v.extend_from_slice(&lo);
                Self::s(v, Some(data));
            }),
            Cmd::CreateFragment => self.start(0x04, |v| v.extend_from_slice(&lo)),
            Cmd::Clone { node, deep } => self.start(0x05, |v| {
                v.extend_from_slice(&lo);
                Self::n(v, *node);
                v.extend_from_slice(&u32::from(*deep).to_le_bytes());
            }),
            Cmd::AppendChild { parent, child } => self.start(0x10, |v| {
                Self::n(v, *parent);
                Self::n(v, *child);
            }),
            Cmd::InsertBefore {
                parent,
                child,
                reference,
            } => self.start(0x11, |v| {
                Self::n(v, *parent);
                Self::n(v, *child);
                Self::n(v, *reference);
            }),
            Cmd::Remove { node } => self.start(0x12, |v| Self::n(v, *node)),
            Cmd::ReplaceChild { parent, new, old } => self.start(0x13, |v| {
                Self::n(v, *parent);
                Self::n(v, *new);
                Self::n(v, *old);
            }),
            Cmd::SetAttribute {
                el,
                ns,
                qname,
                value,
            } => self.start(0x20, |v| {
                Self::n(v, *el);
                Self::s(v, ns.as_deref());
                Self::s(v, Some(qname));
                Self::s(v, Some(value));
            }),
            Cmd::RemoveAttribute { el, ns, name } => self.start(0x21, |v| {
                Self::n(v, *el);
                Self::s(v, ns.as_deref());
                Self::s(v, Some(name));
            }),
            Cmd::ToggleAttribute { el, qname, force } => self.start(0x22, |v| {
                Self::n(v, *el);
                Self::s(v, Some(qname));
                v.extend_from_slice(&Self::tri(*force).to_le_bytes());
            }),
            Cmd::ClassAdd { el, token } => self.start(0x30, |v| {
                Self::n(v, *el);
                Self::s(v, Some(token));
            }),
            Cmd::ClassRemove { el, token } => self.start(0x31, |v| {
                Self::n(v, *el);
                Self::s(v, Some(token));
            }),
            Cmd::ClassToggle { el, token, force } => self.start(0x32, |v| {
                Self::n(v, *el);
                Self::s(v, Some(token));
                v.extend_from_slice(&Self::tri(*force).to_le_bytes());
            }),
            Cmd::SetTextContent { node, text } => self.start(0x40, |v| {
                Self::n(v, *node);
                Self::s(v, text.as_deref());
            }),
            Cmd::SetData { node, data } => self.start(0x41, |v| {
                Self::n(v, *node);
                Self::s(v, Some(data));
            }),
            Cmd::SetInnerHtml { el, html } => self.start(0x50, |v| {
                Self::n(v, *el);
                Self::s(v, Some(html));
            }),
        }
        out
    }

    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.body.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.locals.to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(cmds: &[Cmd]) -> Vec<u8> {
        let mut e = Encoder::new();
        for c in cmds {
            e.push(c);
        }
        e.finish()
    }

    #[test]
    fn encoder_and_decoder_round_trip_every_opcode() {
        let n = Operand::Existing(1 << 32, (1 << 32) | 5);
        let cmds = vec![
            Cmd::CreateElement {
                ns: None,
                qname: "div".into(),
            },
            Cmd::CreateElement {
                ns: Some("http://www.w3.org/2000/svg".into()),
                qname: "svg".into(),
            },
            Cmd::CreateText {
                data: "t\u{e9}".into(),
            },
            Cmd::CreateComment { data: "c".into() },
            Cmd::CreateFragment,
            Cmd::Clone {
                node: Operand::Local(0),
                deep: true,
            },
            Cmd::AppendChild {
                parent: n,
                child: Operand::Local(0),
            },
            Cmd::InsertBefore {
                parent: n,
                child: Operand::Local(1),
                reference: Operand::Null,
            },
            Cmd::Remove {
                node: Operand::Local(2),
            },
            Cmd::ReplaceChild {
                parent: n,
                new: Operand::Local(3),
                old: n,
            },
            Cmd::SetAttribute {
                el: n,
                ns: None,
                qname: "a".into(),
                value: "".into(),
            },
            Cmd::RemoveAttribute {
                el: n,
                ns: Some("urn:x".into()),
                name: "a".into(),
            },
            Cmd::ToggleAttribute {
                el: n,
                qname: "hidden".into(),
                force: Some(true),
            },
            Cmd::ClassAdd {
                el: n,
                token: "x".into(),
            },
            Cmd::ClassRemove {
                el: n,
                token: "x".into(),
            },
            Cmd::ClassToggle {
                el: n,
                token: "x".into(),
                force: None,
            },
            Cmd::SetTextContent {
                node: n,
                text: None,
            },
            Cmd::SetData {
                node: n,
                data: "d".into(),
            },
            Cmd::SetInnerHtml {
                el: n,
                html: "<b>x</b>".into(),
            },
        ];
        let b = enc(&cmds);
        assert_eq!(b.len() % 4, 0);
        let d = decode(&b, &DomLimits::default()).unwrap();
        assert_eq!(d.cmds, cmds);
        assert_eq!(d.local_count, 6);
    }

    #[test]
    fn every_malformation_is_invalid_argument_with_a_byte_offset() {
        let good = enc(&[
            Cmd::CreateElement {
                ns: None,
                qname: "p".into(),
            },
            Cmd::SetAttribute {
                el: Operand::Local(0),
                ns: None,
                qname: "a".into(),
                value: "v".into(),
            },
        ]);
        assert!(decode(&good, &DomLimits::default()).is_ok());
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("short", good[..10].to_vec()),
            ("magic", {
                let mut b = good.clone();
                b[0] = b'X';
                b
            }),
            ("version", {
                let mut b = good.clone();
                b[4] = 9;
                b
            }),
            ("flags", {
                let mut b = good.clone();
                b[6] = 1;
                b
            }),
            ("opcode", {
                let mut b = good.clone();
                b[16] = 0x77;
                b
            }),
            ("cmd_flags", {
                let mut b = good.clone();
                b[18] = 1;
                b
            }),
            ("length", {
                let mut b = good.clone();
                b[20] = 0xF0;
                b
            }),
            ("local_count", {
                let mut b = good.clone();
                b[12] = 5;
                b
            }),
            ("trailing", {
                let mut b = good.clone();
                b.extend_from_slice(&[0, 0, 0, 0]);
                b
            }),
            ("truncated", good[..good.len() - 4].to_vec()),
        ];
        for (name, b) in cases {
            let e = decode(&b, &DomLimits::default()).expect_err(name);
            assert_eq!(e.code, Code::InvalidArgument, "{name}");
            assert!(e.byte_offset.is_some(), "{name} carries a byte offset");
        }
        // undefined local id; local defined out of order; non-zero padding; bad UTF-8
        let undefined = enc(&[Cmd::Remove {
            node: Operand::Local(0),
        }]);
        assert!(decode(&undefined, &DomLimits::default()).is_err());
        let mut padded = enc(&[Cmd::CreateText { data: "ab".into() }]);
        let n = padded.len();
        padded[n - 1] = 7;
        assert!(decode(&padded, &DomLimits::default()).is_err());
        let mut utf = enc(&[Cmd::CreateText {
            data: "abcd".into(),
        }]);
        let n = utf.len();
        utf[n - 1] = 0xFF;
        assert!(decode(&utf, &DomLimits::default()).is_err());
    }

    #[test]
    fn limits_are_size_limit() {
        let b = enc(&[
            Cmd::CreateFragment,
            Cmd::CreateFragment,
            Cmd::CreateFragment,
        ]);
        let e = decode(
            &b,
            &DomLimits {
                batch_commands: 2,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.code, Code::SizeLimit);
        assert!(e.message.contains("dom.batch_commands"));
        let e = decode(
            &b,
            &DomLimits {
                batch_bytes: 20,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.code, Code::SizeLimit);
        assert!(e.message.contains("dom.batch_bytes"));
    }
}
