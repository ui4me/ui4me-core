//! F1008 `explain`: why a CSS property has (or has not) an effect on an element.
//!
//! The answer is built from the computed-style lookup, the profile's property table and the
//! box tree facts that make a property not apply (`display: none` on the element or an
//! ancestor, `content-visibility: hidden` on an ancestor, `display: inline` for box-size
//! properties, `position: static` for offsets, non-flex containers for flex properties). A stale
//! or unknown element yields a structured `stale_reference` answer, never a bare error. Node
//! pointers and arena indices are never part of the answer (element ids and `node:` references
//! are the public names).

use super::*;

/// Properties that do not apply to a non-replaced inline box.
const INLINE_NOT_APPLICABLE: &[&str] = &[
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "margin-top",
    "margin-bottom",
    "margin-block",
    "margin-block-start",
    "margin-block-end",
];
const FLEX_CONTAINER_ONLY: &[&str] = &[
    "flex-direction",
    "flex-wrap",
    "flex-flow",
    "justify-content",
    "align-items",
    "align-content",
];
const FLEX_ITEM_ONLY: &[&str] = &["flex-grow", "flex-shrink", "flex-basis", "flex", "order"];
const OFFSETS: &[&str] = &["top", "right", "bottom", "left", "inset"];
/// Declarations listed when no property is given.
const MAX_LISTED: usize = 16;

impl Document {
    fn explain_value(&self, n: NodeId, name: &str) -> Option<String> {
        self.pipeline
            .style_bridge()
            .computed_value(self.pipeline.document(), n, name)
            .ok()
            .filter(|v| !v.is_empty())
    }

    fn describe(&self, n: NodeId) -> String {
        let d = self.pipeline.document();
        match d.get_attribute(n, "id").ok().flatten() {
            Some(id) if !id.is_empty() => format!("#{id}"),
            _ => format!("<{}>", d.local_name(n).unwrap_or("element")),
        }
    }

    /// `{display: "none"}` / `{content_visibility: "hidden"}` blockers on the element or an ancestor.
    fn box_blocker(&self, n: NodeId) -> Option<String> {
        let d = self.pipeline.document();
        let mut cur = Some(n);
        let mut depth = 0;
        while let Some(x) = cur {
            if depth > 256 {
                break;
            }
            if d.node_kind(x).ok() == Some(NodeKind::Element) {
                if self.explain_value(x, "display").as_deref() == Some("none") {
                    return Some(if x == n {
                        "the element has display: none, so it generates no box".to_owned()
                    } else {
                        format!(
                            "ancestor {} has display: none, so the element generates no box",
                            self.describe(x)
                        )
                    });
                }
                if x != n
                    && self.explain_value(x, "content-visibility").as_deref() == Some("hidden")
                {
                    return Some(format!(
                        "ancestor {} has content-visibility: hidden, so its contents are skipped for layout and paint",
                        self.describe(x)
                    ));
                }
            }
            cur = d.parent(x).ok().flatten();
            depth += 1;
        }
        None
    }

    fn inline_declarations(&self, n: NodeId) -> Vec<(String, String)> {
        let Some(style) = self
            .pipeline
            .document()
            .get_attribute(n, "style")
            .ok()
            .flatten()
        else {
            return Vec::new();
        };
        style
            .split(';')
            .filter_map(|decl| {
                let (name, value) = decl.split_once(':')?;
                let value = value.replace("!important", "");
                Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            })
            .filter(|(name, value)| !name.is_empty() && !value.is_empty())
            .collect()
    }

    fn explain_property(&mut self, n: NodeId, element: &str, name: &str) -> Value {
        let property = name.trim().to_ascii_lowercase();
        let base = |status: &str, reason: String| json!({"property": property, "element": element, "status": status, "reason": reason});
        // Profile and name lookup first: an excluded property never applies anywhere.
        let entry = match self.computed_style(
            None,
            Some(&node_ref(self.id(), n)),
            std::slice::from_ref(&property),
        ) {
            Ok(v) => v["properties"][property.as_str()].clone(),
            Err(e) => return base("unknown_property", e.message),
        };
        match entry["status"].as_str() {
            Some("unsupported") => {
                let feature = entry["feature"].as_str().unwrap_or("").to_owned();
                let mut out = base(
                    "unsupported",
                    format!(
                        "the CSS profile excludes this property (feature {feature}); declarations are parsed and ignored"
                    ),
                );
                out["feature"] = json!(feature);
                return out;
            }
            Some("invalid") => {
                return base(
                    "unknown_property",
                    entry["reason"]
                        .as_str()
                        .unwrap_or("unknown property")
                        .to_owned(),
                );
            }
            _ => {}
        }
        let value = entry["value"].as_str().unwrap_or("").to_owned();
        let with_value = |mut v: Value| {
            v["value"] = json!(value);
            v
        };
        if property != "display"
            && let Some(reason) = self.box_blocker(n)
        {
            return with_value(base("not_applicable", reason));
        }
        let display = self.explain_value(n, "display").unwrap_or_default();
        let parent_display = self
            .pipeline
            .document()
            .parent(n)
            .ok()
            .flatten()
            .and_then(|p| self.explain_value(p, "display"))
            .unwrap_or_default();
        let is_flexish = |d: &str| d.contains("flex") || d.contains("grid");
        let blocked = if display == "inline" && INLINE_NOT_APPLICABLE.contains(&property.as_str()) {
            Some("display is inline: width, height and vertical margins do not apply to a non-replaced inline box".to_owned())
        } else if display == "contents"
            && matches!(
                property.as_str(),
                "width" | "height" | "padding" | "margin" | "border" | "background-color"
            )
        {
            Some("display is contents: the element generates no box of its own".to_owned())
        } else if OFFSETS.contains(&property.as_str())
            && self.explain_value(n, "position").as_deref() == Some("static")
        {
            Some("position is static: offsets only apply to positioned elements".to_owned())
        } else if FLEX_CONTAINER_ONLY.contains(&property.as_str()) && !is_flexish(&display) {
            Some(format!(
                "display is {display}: this property only applies to flex and grid containers"
            ))
        } else if FLEX_ITEM_ONLY.contains(&property.as_str()) && !is_flexish(&parent_display) {
            Some("the parent is not a flex or grid container: this property only applies to flex and grid items".to_owned())
        } else {
            None
        };
        if let Some(reason) = blocked {
            return with_value(base("not_applicable", reason));
        }
        // A keyword declared inline that the cascade or an adjustment replaced with another keyword.
        let pure_keyword =
            |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
        if let Some((_, declared)) = self
            .inline_declarations(n)
            .into_iter()
            .find(|(p, _)| *p == property)
        {
            let declared_l = declared.to_ascii_lowercase();
            if pure_keyword(&declared_l)
                && pure_keyword(&value)
                && declared_l != value
                && !matches!(
                    declared_l.as_str(),
                    "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "currentcolor"
                )
            {
                return with_value(base(
                    "overridden",
                    format!(
                        "the inline declaration {property}: {declared} was replaced; the computed value is {value} (a later cascade layer, !important rule, or display adjustment such as blockification wins)"
                    ),
                ));
            }
        }
        with_value(base(
            "applied",
            "the declaration takes effect on this element".to_owned(),
        ))
    }

    /// Why a property has no effect on an element (schema 1). Without `property`, explains the
    /// element's state and every property declared in its `style` attribute.
    pub fn explain(
        &mut self,
        element: Option<&str>,
        node: Option<&str>,
        property: Option<&str>,
    ) -> Result<Value> {
        let label = element.or(node).unwrap_or("").to_owned();
        let n = match self.target_of(element, node) {
            Ok(n) => n,
            Err(e) if e.code == 4 || e.message.starts_with("Unknown element ID") => {
                let mut out = json!({
                    "schema": 1,
                    "status": "stale_reference",
                    "element": label,
                    "reason": if e.code == 4 {
                        "the node was removed from the document; references are generation-checked and never reused"
                    } else {
                        "no element with this id exists: it was never created, was removed, or its id changed"
                    },
                });
                if let Some(p) = property {
                    out["property"] = json!(p);
                }
                return Ok(out);
            }
            Err(e) => return Err(e),
        };
        if self.pipeline.document().node_kind(n)? != NodeKind::Element {
            return Err(arg("explain needs an element"));
        }
        self.pipeline.flush()?;
        if let Some(p) = property {
            let mut out = self.explain_property(n, &label, p);
            out["schema"] = json!(1);
            return Ok(out);
        }
        let blocker = self.box_blocker(n);
        let declared: Vec<String> = self
            .inline_declarations(n)
            .into_iter()
            .map(|(p, _)| p)
            .take(MAX_LISTED)
            .collect();
        let properties: Vec<Value> = declared
            .iter()
            .map(|p| self.explain_property(n, &label, p))
            .collect();
        Ok(json!({
            "schema": 1,
            "status": if blocker.is_some() { "not_applicable" } else { "applied" },
            "element": label,
            "reason": blocker.unwrap_or_else(|| "the element generates a box".to_owned()),
            "properties": properties,
        }))
    }
}

impl Document {
    /// Bring style and layout up to date so the diagnostics stores describe the current document.
    /// A failure is itself a diagnostic (it lands in the error store) and is not raised here.
    pub fn refresh_for_diagnostics(&mut self) {
        let _ = self.pipeline.flush();
    }
}
