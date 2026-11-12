//! owner: 006-style-engine — L-GEO-r9
//!
//! The form-control selector states: `:required`, `:optional`, `:valid`, `:invalid` and
//! `:placeholder-shown` (HTML sections 4.10.5.3 and 4.10.18, Selectors 4 / HTML "pseudo-classes").
//! Found by T2 `corpora-bootstrap/forms`: Bootstrap's `.was-validated :invalid ~ .invalid-feedback`
//! shows a feedback block, a layout-visible effect.
//!
//! Validity is the stored `validity_flags` when 012 has computed them (non-zero), else it is derived
//! here from the one constraint that needs only the element and its value facts: **value missing**
//! (`required`). `pattern`, length, range, step and type-mismatch constraints, `fieldset disabled`
//! ancestry, radio groups (a required radio is judged on its own checkedness) and the `form`-level
//! `:valid` / `:invalid` are not computed.

use stylo_dom::ElementState;
use ui4me_document::control_state::{ControlFlags, validity};
use ui4me_document::node::Attribute;

/// Facts about a form control that its own state does not carry: they come from the element's
/// children, which only the adapter can read. `Default` is used when the prior state of a snapshot is
/// composed (their prior value is not journaled).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FormFacts {
    /// A clean `textarea`'s value is its child text: whether that is empty.
    pub(crate) text_empty: Option<bool>,
    /// A required `select` that has no selected option, or whose selected option is the placeholder
    /// label option (HTML "value missing" for `select`).
    pub(crate) select_missing: bool,
    /// Inside a disabled `fieldset` and not in its first `legend` (HTML 4.10.18.2; 015 HTML-3).
    pub(crate) fieldset_disabled: bool,
}

fn attr<'a>(attrs: &'a [Attribute], local: &str) -> Option<&'a str> {
    attrs.iter().find(|a| a.is(local)).map(|a| a.value.as_str())
}

/// `input` types the `required` attribute applies to (HTML section 4.10.5.3).
fn required_applies(ty: &str) -> bool {
    !matches!(
        ty,
        "hidden" | "range" | "color" | "button" | "reset" | "submit" | "image"
    )
}

/// `input` types that are barred from constraint validation (HTML section 4.10.18.3).
fn barred_type(ty: &str) -> bool {
    matches!(ty, "hidden" | "reset" | "button")
}

/// Add the form-control states of the HTML element `local` to `s`.
pub(crate) fn compose(
    s: &mut ElementState,
    local: &str,
    attrs: &[Attribute],
    control: Option<ControlFlags>,
    facts: FormFacts,
) {
    let ty = if local == "input" {
        attr(attrs, "type").map_or_else(|| "text".to_string(), str::to_ascii_lowercase)
    } else {
        String::new()
    };
    let applies = match local {
        "input" => required_applies(&ty),
        "textarea" | "select" => true,
        _ => return,
    };
    let required = applies && attr(attrs, "required").is_some();
    if applies {
        *s |= if required {
            ElementState::REQUIRED
        } else {
            ElementState::OPTIONAL_
        };
    }
    let checkable = local == "input" && matches!(ty.as_str(), "checkbox" | "radio");
    let textual =
        local == "textarea" || (local == "input" && !checkable && applies && ty != "file");
    let value_empty = match (local, facts.text_empty) {
        ("textarea", Some(e)) => e,
        _ => control.map_or_else(
            || attr(attrs, "value").is_none_or(str::is_empty),
            |c| c.value_empty,
        ),
    };
    if textual && value_empty && attr(attrs, "placeholder").is_some_and(|p| !p.is_empty()) {
        *s |= ElementState::PLACEHOLDER_SHOWN;
    }
    // Barred from constraint validation: neither :valid nor :invalid.
    let readonly = (textual && attr(attrs, "readonly").is_some())
        || attr(attrs, "disabled").is_some()
        || facts.fieldset_disabled;
    if (local == "input" && barred_type(&ty)) || readonly {
        return;
    }
    let flags = control.map_or(0, |c| c.validity_flags);
    let invalid = if flags != 0 {
        flags & validity::VALID == 0
    } else {
        required
            && match local {
                "select" => facts.select_missing,
                _ if checkable => control.and_then(|c| c.checked) != Some(true),
                _ => value_empty,
            }
    };
    *s |= if invalid {
        ElementState::INVALID
    } else {
        ElementState::VALID
    };
}
