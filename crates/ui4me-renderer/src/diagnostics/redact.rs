//! owner: 005-pipeline-diagnostics — created by 001 slice (R-28)
//!
//! Redaction policy (diagnostics-schema-v0.md §8; AD-35).
//! Implemented by 001 T051.
//!
//! Path rules: an absolute path inside a declared root is rewritten relative to the longest
//! matching root (with `/` separators); any other absolute path becomes [`REDACTED_PATH`];
//! relative paths are kept. Path rewriting applies under both policies — `Disabled` lifts only
//! the value redactions (password controls stay redacted without the password-values flag), so a
//! user-name path segment is never emitted (§8 "User-name path segments: never emitted").

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redaction {
    /// Envelope `"redaction": "default"`.
    Default,
    /// Envelope `"redaction": "disabled"`; requires an explicit host flag.
    Disabled,
}

impl Redaction {
    pub const fn name(self) -> &'static str {
        match self {
            Redaction::Default => "default",
            Redaction::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedactionPolicy {
    pub mode: Redaction,
    /// The separate password-values flag; valid only with `Redaction::Disabled`.
    pub password_values: bool,
    /// Declared roots (fixture root, document base). Absolute paths inside one are rewritten
    /// relative to it; any other absolute path becomes [`REDACTED_PATH`].
    pub roots: Vec<PathBuf>,
}

/// Replacement for an absolute path outside every declared root.
pub const REDACTED_PATH: &str = "<redacted-path>";

impl RedactionPolicy {
    /// The default policy with the given declared roots.
    pub fn default_with_roots(roots: Vec<PathBuf>) -> RedactionPolicy {
        RedactionPolicy {
            mode: Redaction::Default,
            password_values: false,
            roots,
        }
    }

    /// Password values are exported only with redaction disabled *and* the separate flag (§8).
    pub fn password_values_included(&self) -> bool {
        self.mode == Redaction::Disabled && self.password_values
    }

    /// A path as it may appear in the export.
    pub fn path(&self, path: &Path) -> String {
        let text = path.to_string_lossy();
        if !cfg!(windows) && is_windows_absolute(&text) {
            // A foreign (Windows) absolute path: match the roots textually.
            let norm = |s: &str| {
                s.replace('\\', "/")
                    .trim_end_matches('/')
                    .to_ascii_lowercase()
            };
            let t = norm(&text);
            let best = self
                .roots
                .iter()
                .map(|r| norm(&r.to_string_lossy()))
                .filter(|r| t.starts_with(&format!("{r}/")))
                .max_by_key(|r| r.len());
            return match best {
                Some(r) => text.replace('\\', "/")[r.len() + 1..].to_string(),
                None => REDACTED_PATH.to_string(),
            };
        }
        if !is_absolute(path) {
            return slashes(path);
        }
        let best = self
            .roots
            .iter()
            .filter(|r| is_absolute(r) && path.starts_with(r))
            .max_by_key(|r| r.components().count());
        match best {
            Some(root) => {
                let rel = path.strip_prefix(root).unwrap_or(path);
                let s = slashes(rel);
                if s.is_empty() { ".".to_string() } else { s }
            }
            None => REDACTED_PATH.to_string(),
        }
    }

    /// Content text (DOM attribute values and character data, resolved style values) as it may
    /// appear in the export: with the default policy every absolute path in it is rewritten
    /// (schema §8, "anywhere"); with redaction disabled the content is exported verbatim.
    pub fn scrub(&self, text: &str) -> String {
        match self.mode {
            Redaction::Default => self.text(text),
            Redaction::Disabled => text.to_string(),
        }
    }

    /// Free text (error and parser messages) with every absolute path rewritten.
    pub fn text(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            if let Some((start, end)) = path_token_at(&chars, i) {
                let token: String = chars[start..end].iter().collect();
                // A `file://` scheme is dropped with the absolute prefix.
                out.push_str(&self.path(Path::new(&token)));
                i = end;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }
}

fn is_absolute(p: &Path) -> bool {
    p.has_root() || matches!(p.components().next(), Some(Component::Prefix(_)))
}

fn is_windows_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn slashes(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".into()),
            Component::CurDir => None,
            Component::RootDir | Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn is_boundary(c: Option<&char>) -> bool {
    match c {
        None => true,
        Some(c) => {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | '`' | '(' | '[' | '<' | '=' | ',' | ';' | ':'
                )
        }
    }
}

fn is_terminator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']' | '>' | ',' | ';')
}

/// An absolute path (POSIX `/…`, Windows `X:\…`/`X:/…`, or a `file://` URL) starting at `i`:
/// `(path start, end)`; the `file://` scheme, if any, spans `i..path start`.
fn path_token_at(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let prev = if i == 0 { None } else { chars.get(i - 1) };
    let starts_with = |s: &str| {
        s.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k).is_some_and(|x| x.eq_ignore_ascii_case(&c)))
    };
    let posix = chars[i] == '/'
        && chars
            .get(i + 1)
            .is_some_and(|c| !c.is_whitespace() && *c != '/');
    let windows = chars[i].is_ascii_alphabetic()
        && chars.get(i + 1) == Some(&':')
        && matches!(chars.get(i + 2), Some('\\') | Some('/'));
    let start = if !is_boundary(prev) {
        return None;
    } else if starts_with("file://") {
        i + "file://".len()
    } else if posix || windows {
        i
    } else {
        return None;
    };
    let mut end = start;
    while end < chars.len() && !is_terminator(chars[end]) {
        end += 1;
    }
    // A trailing sentence period is not part of the path.
    while end > start + 1 && matches!(chars[end - 1], '.' | ':') {
        end -= 1;
    }
    (end > start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> RedactionPolicy {
        RedactionPolicy::default_with_roots(vec![
            PathBuf::from("/srv/fx"),
            PathBuf::from("/srv/fx/deep"),
        ])
    }

    #[test]
    fn paths() {
        assert_eq!(p().path(Path::new("/srv/fx/a/b.css")), "a/b.css");
        assert_eq!(p().path(Path::new("/srv/fx/deep/c.css")), "c.css");
        assert_eq!(p().path(Path::new("/srv/fxother/c.css")), REDACTED_PATH);
        assert_eq!(p().path(Path::new("rel/x.css")), "rel/x.css");
    }

    #[test]
    fn text() {
        assert_eq!(
            p().text("failed to read /home/alice/x.css."),
            "failed to read <redacted-path>."
        );
        assert_eq!(
            p().text("see file:///srv/fx/a.html line 3"),
            "see a.html line 3"
        );
        assert_eq!(
            p().text("bad url(file:///home/bob/x.png)"),
            "bad url(<redacted-path>)"
        );
        assert_eq!(p().text("ratio 1/2 and a/b"), "ratio 1/2 and a/b");
        assert_eq!(p().text("at C:\\Users\\carol\\x.ttf"), "at <redacted-path>");
    }
}
