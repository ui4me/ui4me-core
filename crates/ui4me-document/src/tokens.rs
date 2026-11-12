//! owner: 003-document-model
//!
//! DOMTokenList over `class` (T044; A-45; FR-038). The token set is the ordered set of the
//! attribute's ASCII-whitespace-separated tokens (first occurrence kept). Mutating operations run the
//! DOM "update steps": the attribute is written with the serialized set (single spaces), except that a
//! missing attribute with an empty set stays missing. An unchanged serialization is no mutation.
//! An empty token is `SYNTAX`; a token containing ASCII whitespace is `INVALID_CHARACTER`.

use crate::document::Document;
use crate::errors::{invalid_character, syntax_error, wrong_node_kind};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Parse an attribute value into the ordered token set.
pub fn parse_tokens(value: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in value
        .split([' ', '\t', '\n', '\x0c', '\r'])
        .filter(|t| !t.is_empty())
    {
        if !out.iter().any(|x| x == t) {
            out.push(t.to_owned());
        }
    }
    out
}

fn validate_token(t: &str) -> Result<(), Status> {
    if t.is_empty() {
        return Err(syntax_error());
    }
    if t.chars()
        .any(|c| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'))
    {
        return Err(invalid_character());
    }
    Ok(())
}

impl Document {
    fn class_tokens(&self, el: NodeId) -> Result<Vec<String>, Status> {
        let e = self.check(el)?.as_element().ok_or_else(wrong_node_kind)?;
        Ok(parse_tokens(e.attr("class").unwrap_or("")))
    }

    fn class_update(&mut self, el: NodeId, tokens: &[String]) {
        let has_attr = self
            .arena
            .node(el)
            .as_element()
            .is_some_and(|e| e.attr("class").is_some());
        if !has_attr && tokens.is_empty() {
            return;
        }
        self.write_class(el, &tokens.join(" "));
    }

    /// `classList.value` (the attribute text).
    pub fn class_list_value(&self, el: NodeId) -> Result<String, Status> {
        self.class_name(el)
    }
    pub fn class_list_length(&self, el: NodeId) -> Result<usize, Status> {
        Ok(self.class_tokens(el)?.len())
    }
    pub fn class_list_item(&self, el: NodeId, index: usize) -> Result<Option<String>, Status> {
        Ok(self.class_tokens(el)?.get(index).cloned())
    }
    pub fn class_list_contains(&self, el: NodeId, token: &str) -> Result<bool, Status> {
        Ok(self.class_tokens(el)?.iter().any(|t| t == token))
    }

    /// `classList.add(...tokens)`: every token is validated before anything changes.
    pub fn class_list_add(&mut self, el: NodeId, tokens: &[&str]) -> Result<(), Status> {
        let mut set = self.class_tokens(el)?;
        for t in tokens {
            validate_token(t)?;
        }
        for t in tokens {
            if !set.iter().any(|x| x == t) {
                set.push((*t).to_owned());
            }
        }
        self.class_update(el, &set);
        Ok(())
    }

    /// `classList.remove(...tokens)`.
    pub fn class_list_remove(&mut self, el: NodeId, tokens: &[&str]) -> Result<(), Status> {
        let mut set = self.class_tokens(el)?;
        for t in tokens {
            validate_token(t)?;
        }
        set.retain(|x| !tokens.iter().any(|t| t == x));
        self.class_update(el, &set);
        Ok(())
    }

    /// `classList.toggle(token, force?)`: returns whether the token is in the set afterwards.
    pub fn class_list_toggle(
        &mut self,
        el: NodeId,
        token: &str,
        force: Option<bool>,
    ) -> Result<bool, Status> {
        let mut set = self.class_tokens(el)?;
        validate_token(token)?;
        if set.iter().any(|x| x == token) {
            if force != Some(true) {
                set.retain(|x| x != token);
                self.class_update(el, &set);
                return Ok(false);
            }
            return Ok(true);
        }
        if force != Some(false) {
            set.push(token.to_owned());
            self.class_update(el, &set);
            return Ok(true);
        }
        Ok(false)
    }

    /// `classList.replace(token, newToken)`: returns whether a replacement happened.
    pub fn class_list_replace(
        &mut self,
        el: NodeId,
        token: &str,
        new_token: &str,
    ) -> Result<bool, Status> {
        let set = self.class_tokens(el)?;
        validate_token(token)?;
        validate_token(new_token)?;
        if !set.iter().any(|x| x == token) {
            return Ok(false);
        }
        // DOM: replace the first instance of either token with newToken, remove all other instances.
        let mut out: Vec<String> = vec![];
        let mut placed = false;
        for t in &set {
            if t == token || t == new_token {
                if !placed {
                    out.push(new_token.to_owned());
                    placed = true;
                }
            } else {
                out.push(t.clone());
            }
        }
        self.class_update(el, &out);
        Ok(true)
    }

    /// Slice-era entry point kept for callers: `toggle(token)` with no `force`.
    pub fn toggle_class(&mut self, el: NodeId, token: &str) -> Result<bool, Status> {
        self.class_list_toggle(el, token, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::codes_generated::{Code, SubReason};

    fn el(html: &str) -> (Document, NodeId) {
        let d = Document::parse_html(html).unwrap();
        let e = d.element_by_id("e").unwrap();
        (d, e)
    }

    #[test]
    fn ordered_set_semantics_and_update_steps() {
        let (mut d, e) = el("<p id=e class='  a  b a '>");
        assert_eq!(d.class_list_length(e).unwrap(), 2);
        assert_eq!(d.class_list_item(e, 1).unwrap().as_deref(), Some("b"));
        assert_eq!(d.class_list_item(e, 5).unwrap(), None);
        assert!(d.class_list_contains(e, "a").unwrap());
        let r = d.revision();
        d.class_list_add(e, &["c", "a"]).unwrap();
        assert_eq!(
            d.class_name(e).unwrap(),
            "a b c",
            "update steps serialize the set"
        );
        assert_eq!(d.revision(), r + 1);
        d.class_list_add(e, &["c"]).unwrap();
        assert_eq!(
            d.revision(),
            r + 1,
            "unchanged serialization is no mutation"
        );
        d.class_list_remove(e, &["b", "zz"]).unwrap();
        assert_eq!(d.class_name(e).unwrap(), "a c");
        assert!(d.class_list_toggle(e, "x", None).unwrap());
        assert!(!d.class_list_toggle(e, "x", None).unwrap());
        assert!(d.class_list_toggle(e, "y", Some(true)).unwrap());
        assert!(d.class_list_toggle(e, "y", Some(true)).unwrap());
        assert!(!d.class_list_toggle(e, "y", Some(false)).unwrap());
        assert!(!d.class_list_toggle(e, "nope", Some(false)).unwrap());
        assert_eq!(d.class_name(e).unwrap(), "a c");
        assert!(d.class_list_replace(e, "a", "q").unwrap());
        assert_eq!(d.class_name(e).unwrap(), "q c");
        assert!(!d.class_list_replace(e, "zz", "q").unwrap());
    }

    #[test]
    fn a_missing_attribute_with_an_empty_set_stays_missing_and_bad_tokens_change_nothing() {
        let (mut d, e) = el("<p id=e>");
        let r = d.revision();
        d.class_list_remove(e, &["a"]).unwrap();
        d.class_list_add(e, &[]).unwrap();
        assert!(!d.has_attribute(e, "class").unwrap());
        assert_eq!(d.revision(), r);
        let err = d.class_list_add(e, &["ok", ""]).unwrap_err();
        assert_eq!(
            (err.code, err.sub_reason),
            (Code::InvalidArgument, Some(SubReason::Syntax))
        );
        let err = d.class_list_add(e, &["ok", "a b"]).unwrap_err();
        assert_eq!(err.code, Code::InvalidCharacter);
        assert!(
            !d.has_attribute(e, "class").unwrap(),
            "validation precedes the first change"
        );
        assert_eq!(d.revision(), r);
    }

    #[test]
    fn replace_collapses_duplicates() {
        let (mut d, e) = el("<p id=e class='a b c'>");
        assert!(d.class_list_replace(e, "a", "c").unwrap());
        assert_eq!(d.class_name(e).unwrap(), "c b");
    }
}
