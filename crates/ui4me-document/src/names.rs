//! owner: 003-document-model
//!
//! Name validation (T041; FR-034; DOM `dom.bs:191-290`): the XML `Name` / `QName` productions and
//! DOM "validate and extract" for namespace-aware creation and attributes.

use crate::errors::{invalid_character, namespace_error};
use html5ever::{LocalName, Namespace, Prefix, QualName, ns};
use ui4me_engine_types::status::Status;

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

fn name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}'
        | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

fn name_char(c: char) -> bool {
    name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// The XML `Name` production.
pub fn is_name(s: &str) -> bool {
    let mut it = s.chars();
    matches!(it.next(), Some(c) if name_start(c)) && it.all(name_char)
}

/// The XML `NCName` production (a `Name` without colons).
pub fn is_ncname(s: &str) -> bool {
    is_name(s) && !s.contains(':')
}

/// The XML `QName` production: `NCName (":" NCName)?`.
pub fn is_qname(s: &str) -> bool {
    match s.split_once(':') {
        None => is_ncname(s),
        Some((p, l)) => is_ncname(p) && is_ncname(l),
    }
}

/// Result of "validate and extract": the namespace (None = null), prefix and local name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extracted {
    pub namespace: Option<String>,
    pub prefix: Option<String>,
    pub local: String,
}

/// DOM "validate and extract" (`InvalidCharacterError` -> `INVALID_CHARACTER`,
/// `NamespaceError` -> `INVALID_NAMESPACE`).
pub fn validate_and_extract(
    namespace: Option<&str>,
    qualified_name: &str,
) -> Result<Extracted, Status> {
    let namespace = namespace.filter(|n| !n.is_empty());
    if !is_name(qualified_name) || !is_qname(qualified_name) {
        return Err(invalid_character());
    }
    let (prefix, local) = match qualified_name.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, qualified_name),
    };
    if prefix.is_some() && namespace.is_none() {
        return Err(namespace_error());
    }
    if prefix == Some("xml") && namespace != Some(XML_NS) {
        return Err(namespace_error());
    }
    if (prefix == Some("xmlns") || qualified_name == "xmlns") && namespace != Some(XMLNS_NS) {
        return Err(namespace_error());
    }
    if namespace == Some(XMLNS_NS) && prefix != Some("xmlns") && qualified_name != "xmlns" {
        return Err(namespace_error());
    }
    Ok(Extracted {
        namespace: namespace.map(str::to_owned),
        prefix: prefix.map(str::to_owned),
        local: local.to_owned(),
    })
}

impl Extracted {
    pub fn to_qual_name(&self) -> QualName {
        QualName::new(
            self.prefix.as_deref().map(Prefix::from),
            Namespace::from(self.namespace.as_deref().unwrap_or("")),
            LocalName::from(self.local.as_str()),
        )
    }
}

/// Is `ns` the HTML namespace URI?
pub fn is_html_ns(ns_uri: &str) -> bool {
    *ns_uri == ns!(html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::Code;

    #[test]
    fn name_productions() {
        for ok in [
            "a",
            "_a",
            ":a",
            "a-b.c1",
            "div",
            "\u{e9}t\u{e9}",
            "a\u{b7}",
            "x:y",
        ] {
            assert!(is_name(ok), "{ok}");
        }
        for bad in ["", "1a", "-a", ".a", "a b", "a>", "a\"", "\u{d7}"] {
            assert!(!is_name(bad), "{bad:?}");
        }
        assert!(
            is_qname("a:b")
                && is_qname("a")
                && !is_qname("a:b:c")
                && !is_qname(":a")
                && !is_qname("a:")
        );
    }

    #[test]
    fn validate_and_extract_follows_the_dom_steps() {
        let e = validate_and_extract(Some("urn:x"), "p:l").unwrap();
        assert_eq!(
            (
                e.namespace.as_deref(),
                e.prefix.as_deref(),
                e.local.as_str()
            ),
            (Some("urn:x"), Some("p"), "l")
        );
        assert_eq!(validate_and_extract(Some(""), "a").unwrap().namespace, None);
        assert_eq!(
            validate_and_extract(None, "p:l").unwrap_err().code,
            Code::InvalidNamespace
        );
        assert_eq!(
            validate_and_extract(Some("urn:x"), "xml:l")
                .unwrap_err()
                .code,
            Code::InvalidNamespace
        );
        assert!(validate_and_extract(Some(XML_NS), "xml:lang").is_ok());
        assert_eq!(
            validate_and_extract(Some("urn:x"), "xmlns")
                .unwrap_err()
                .code,
            Code::InvalidNamespace
        );
        assert!(validate_and_extract(Some(XMLNS_NS), "xmlns:a").is_ok());
        assert_eq!(
            validate_and_extract(Some(XMLNS_NS), "a").unwrap_err().code,
            Code::InvalidNamespace
        );
        assert_eq!(
            validate_and_extract(Some("urn:x"), "a b").unwrap_err().code,
            Code::InvalidCharacter
        );
        assert_eq!(
            validate_and_extract(Some("urn:x"), "a:b:c")
                .unwrap_err()
                .code,
            Code::InvalidCharacter
        );
    }
}
