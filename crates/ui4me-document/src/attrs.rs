//! owner: 003-document-model
//!
//! Node and attribute reads (T027) and attribute mutations (T043): Table A rows A-20, A-26, A-40,
//! A-41, A-42, A-43, A-44, A-46, A-47 (FR-009, FR-011, FR-013, FR-032–FR-040).
//!
//! Attributes are an ordered list of `(namespace, prefix, local name, value)` (no Attr nodes,
//! FR-036). Rules (FR-032–FR-035): `setAttribute` on an HTML element lowercases the name and keeps an
//! existing attribute's position; a new attribute is appended; removal keeps the order of the rest;
//! an invalid name is `INVALID_CHARACTER` and a bad namespace `INVALID_NAMESPACE`, with nothing
//! changed; any Unicode value up to the string limit is accepted. Writes of an unchanged value are no
//! mutation. Each write is one call: prior-value journaling, side effects (stylesheet sources,
//! requester notifications, base URL, form owner) and the FR-099 hook call all happen inside it.

use crate::document::Document;
use crate::errors::{invalid_character, syntax_error, wrong_node_kind};
use crate::hooks::{
    AttributeChange, Notification, REQUEST_ATTRIBUTES, RequesterChange, attribute_hook_applies,
    requester_kind,
};
use crate::journal::EntryKind;
use crate::names::{is_name, validate_and_extract};
use crate::node::{Attribute, ElementData, NodeData};
use crate::stylesheets::is_stylesheet_source;
use crate::ui_state::UiStateWriter;
use html5ever::{LocalName, Namespace, Prefix, QualName, ns};
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::status::Status;

/// Which attribute a write targets.
enum Slot {
    Existing(usize),
    New(QualName),
}

/// `prefix:local` (or `local`).
pub fn qualified_name(n: &QualName) -> String {
    match &n.prefix {
        Some(p) if !p.is_empty() => format!("{}:{}", p, n.local),
        _ => n.local.to_string(),
    }
}

impl Document {
    fn element_ref(&self, n: NodeId) -> Result<&ElementData, Status> {
        self.check(n)?.as_element().ok_or_else(wrong_node_kind)
    }

    // ------------------------------------------------------------ node reads (A-20, A-26, A-40)

    /// `nodeName` (FR-013): the uppercased qualified name for HTML-namespace elements, the qualified
    /// name unchanged otherwise; `#text`, `#comment`, `#document`, `#document-fragment`; a DOCTYPE's name.
    pub fn node_name(&self, n: NodeId) -> Result<String, Status> {
        Ok(match &self.check(n)?.data {
            NodeData::Element(e) => {
                let q = qualified_name(e.name());
                if e.is_html() {
                    q.to_ascii_uppercase()
                } else {
                    q
                }
            }
            NodeData::Text(_) => "#text".into(),
            NodeData::Comment(_) => "#comment".into(),
            NodeData::Document => "#document".into(),
            NodeData::DocumentFragment { .. } => "#document-fragment".into(),
            NodeData::Doctype(d) => d.name.clone(),
        })
    }

    /// `tagName` (A-40); elements only.
    pub fn tag_name(&self, n: NodeId) -> Result<String, Status> {
        self.element_ref(n)?;
        self.node_name(n)
    }

    /// `namespaceURI` (A-40): `None` for the null namespace.
    pub fn namespace_uri(&self, n: NodeId) -> Result<Option<String>, Status> {
        let e = self.element_ref(n)?;
        Ok(Some(e.name().ns.to_string()).filter(|s| !s.is_empty()))
    }

    /// `prefix` (A-40).
    pub fn prefix(&self, n: NodeId) -> Result<Option<String>, Status> {
        let e = self.element_ref(n)?;
        Ok(e.name()
            .prefix
            .as_ref()
            .map(|p| p.to_string())
            .filter(|s| !s.is_empty()))
    }

    /// `nodeValue` (A-26): the data of a Text or Comment, else `None`.
    pub fn node_value(&self, n: NodeId) -> Result<Option<String>, Status> {
        Ok(self.check(n)?.character_data().map(str::to_owned))
    }

    /// `nodeValue` setter (A-26): sets the data of a Text or Comment; no effect on other kinds.
    pub fn set_node_value(&mut self, n: NodeId, v: &str) -> Result<(), Status> {
        if self.check(n)?.character_data().is_some() {
            self.set_data(n, v)
        } else {
            Ok(())
        }
    }

    /// DOCTYPE `name`, `publicId`, `systemId` (A-03, FR-009).
    pub fn doctype_info(&self, n: NodeId) -> Result<(String, String, String), Status> {
        match &self.check(n)?.data {
            NodeData::Doctype(d) => Ok((d.name.clone(), d.public_id.clone(), d.system_id.clone())),
            _ => Err(wrong_node_kind()),
        }
    }

    // ------------------------------------------------------------ attribute reads (A-42, A-44)

    fn lookup_name(e: &ElementData, name: &str) -> String {
        if e.is_html() {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }
    }

    fn find_by_qualified_name(e: &ElementData, name: &str) -> Option<usize> {
        let q = Self::lookup_name(e, name);
        e.attrs.iter().position(|a| qualified_name(&a.name) == q)
    }

    fn find_by_ns(e: &ElementData, namespace: Option<&str>, local: &str) -> Option<usize> {
        let ns_want = Namespace::from(namespace.filter(|s| !s.is_empty()).unwrap_or(""));
        e.attrs
            .iter()
            .position(|a| a.name.ns == ns_want && &*a.name.local == local)
    }

    /// `getAttribute(qualifiedName)` (A-42): the first attribute with that qualified name (the name
    /// is lowercased for an HTML element).
    pub fn get_attribute(&self, n: NodeId, name: &str) -> Result<Option<&str>, Status> {
        let e = self.element_ref(n)?;
        Ok(Self::find_by_qualified_name(e, name).map(|i| e.attrs[i].value.as_str()))
    }

    pub fn get_attribute_ns(
        &self,
        n: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<Option<&str>, Status> {
        let e = self.element_ref(n)?;
        Ok(Self::find_by_ns(e, namespace, local).map(|i| e.attrs[i].value.as_str()))
    }

    pub fn has_attribute(&self, n: NodeId, name: &str) -> Result<bool, Status> {
        let e = self.element_ref(n)?;
        Ok(Self::find_by_qualified_name(e, name).is_some())
    }

    pub fn has_attribute_ns(
        &self,
        n: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<bool, Status> {
        let e = self.element_ref(n)?;
        Ok(Self::find_by_ns(e, namespace, local).is_some())
    }

    pub fn has_attributes(&self, n: NodeId) -> Result<bool, Status> {
        Ok(!self.element_ref(n)?.attrs.is_empty())
    }

    /// `getAttributeNames()` (A-42): qualified names in attribute order.
    pub fn get_attribute_names(&self, n: NodeId) -> Result<Vec<String>, Status> {
        Ok(self
            .element_ref(n)?
            .attrs
            .iter()
            .map(|a| qualified_name(&a.name))
            .collect())
    }

    /// `id` (A-41).
    pub fn id_attr(&self, n: NodeId) -> Result<String, Status> {
        Ok(self.element_ref(n)?.attr("id").unwrap_or("").to_owned())
    }

    /// `className` (A-41).
    pub fn class_name(&self, n: NodeId) -> Result<String, Status> {
        Ok(self.element_ref(n)?.attr("class").unwrap_or("").to_owned())
    }

    // ------------------------------------------------------------ attribute writes (A-43)

    /// Write (or remove, `value = None`) one attribute inside its own call. Returns whether the
    /// stored value changed; an unchanged write is no mutation (no revision, no journal, no hooks).
    fn commit_attribute(&mut self, el: NodeId, slot: Slot, value: Option<&str>) -> bool {
        let e = self.arena.node(el).as_element().expect("validated element");
        let (name, old) = match &slot {
            Slot::Existing(i) => (e.attrs[*i].name.clone(), Some(e.attrs[*i].value.clone())),
            Slot::New(q) => (q.clone(), None),
        };
        if old.as_deref() == value {
            return false;
        }
        if old.is_none() && value.is_none() {
            return false;
        }
        let prior = e.attrs.clone();
        let kind_before = requester_kind(e);
        let was_sheet = is_stylesheet_source(e);
        let local = name.local.to_string();
        let null_ns = name.ns == ns!();
        let connected = self.arena.is_connected(el);
        let in_template = self.in_template_contents(el);
        let base_before =
            (connected && !in_template && null_ns && local == "href" && e.is_html_named("base"))
                .then(|| self.frozen_base_href());

        self.begin_call();
        let rev = self.pending_revision();
        self.journal.note_prior_attributes(rev, el, &prior);
        {
            let ed = self
                .arena
                .get_mut(el)
                .and_then(|n| n.as_element_mut())
                .expect("validated");
            match (&slot, value) {
                (Slot::Existing(i), Some(v)) => ed.attrs[*i].value = v.to_owned(),
                (Slot::Existing(i), None) => {
                    ed.attrs.remove(*i);
                }
                (Slot::New(q), Some(v)) => ed.attrs.push(Attribute {
                    name: q.clone(),
                    value: v.to_owned(),
                }),
                (Slot::New(_), None) => {}
            }
            // Control state follows the content attributes until it is dirty (HTML).
            if null_ns && let Some(c) = ed.control.as_mut() {
                match local.as_str() {
                    "checked" if !c.dirty_checkedness && c.checkedness.is_some() => {
                        c.checkedness = Some(value.is_some())
                    }
                    "selected" if !c.dirty_selectedness && c.selectedness.is_some() => {
                        c.selectedness = Some(value.is_some())
                    }
                    "value" if !c.dirty_value => c.value = Some(value.unwrap_or("").to_owned()),
                    _ => {}
                }
            }
        }
        self.journal.push(
            rev,
            EntryKind::Attribute {
                node: el,
                name: name.clone(),
            },
        );

        if !in_template {
            let (kind_after, is_sheet, is_link, href_after) = {
                let e = self.arena.node(el).as_element().expect("validated element");
                (
                    requester_kind(e),
                    is_stylesheet_source(e),
                    e.is_html_named("link"),
                    e.attr("href").map(str::to_owned),
                )
            };
            if connected {
                // Stylesheet source list.
                let style_attr = matches!(local.as_str(), "media" | "type" | "rel" | "href")
                    && (was_sheet || is_sheet);
                if was_sheet != is_sheet || (style_attr && null_ns) {
                    self.note_stylesheets_changed(rev);
                }
                // Requester notifications.
                let mut notifs = vec![];
                match (kind_before, kind_after) {
                    (None, Some(k)) => notifs.push(Notification::RequesterChanged {
                        element: el,
                        kind: k,
                        change: RequesterChange::Connected,
                    }),
                    (Some(k), None) => notifs.push(Notification::RequesterChanged {
                        element: el,
                        kind: k,
                        change: RequesterChange::Disconnected,
                    }),
                    (Some(k), Some(_))
                        if null_ns && REQUEST_ATTRIBUTES.contains(&local.as_str()) =>
                    {
                        notifs.push(Notification::RequesterChanged {
                            element: el,
                            kind: k,
                            change: RequesterChange::AttributeChanged(local.clone()),
                        })
                    }
                    _ => {}
                }
                if is_sheet && is_link && null_ns && matches!(local.as_str(), "href" | "rel") {
                    notifs.push(Notification::StylesheetLinkChanged {
                        element: el,
                        href: href_after,
                    });
                }
                if let Some(before) = base_before {
                    let after = self.frozen_base_href();
                    if after != before {
                        notifs.push(Notification::BaseUrlChanged { url: after });
                    }
                }
                for n in notifs {
                    self.queue_notification(n);
                }
            }
            if null_ns {
                self.form_owner_attr_changed(el, &local);
            }
            if null_ns
                && matches!(
                    local.as_str(),
                    "selected" | "multiple" | "size" | "disabled"
                )
            {
                if local == "selected" {
                    self.select_option_wins(el);
                }
                self.select_follow(el);
            }
            // HTML 4.11.1 the details element's attribute change steps (015 HTML-2): an exclusive
            // accordion (`name`) keeps at most one member open.
            if null_ns && matches!(local.as_str(), "open" | "name") {
                self.details_exclusivity(el, &local, old.is_none() && value.is_some());
            }
            let hooked = null_ns
                && self
                    .arena
                    .node(el)
                    .as_element()
                    .is_some_and(|e| attribute_hook_applies(e, &local));
            if hooked && let Some(mut h) = self.hooks.attribute_changed.take() {
                let change = AttributeChange {
                    element: el,
                    local_name: local,
                    old,
                    new: value.map(str::to_owned),
                };
                h(
                    &mut UiStateWriter {
                        doc: self,
                        in_call: true,
                    },
                    &change,
                );
                self.hooks.attribute_changed = Some(h);
            }
        }
        self.end_call();
        true
    }

    /// The details name group of `el` (HTML 4.11.1): the other `details` elements of the same tree with
    /// the same non-empty `name`, in tree order.
    pub fn details_name_group(&self, el: NodeId) -> Vec<NodeId> {
        let Some(e) = self
            .arena
            .get(el)
            .and_then(|n| n.as_element())
            .filter(|e| e.is_html_named("details"))
        else {
            return Vec::new();
        };
        let Some(name) = e.attr("name").filter(|n| !n.is_empty()).map(str::to_owned) else {
            return Vec::new();
        };
        let root = self.arena.root_of(el);
        self.arena
            .subtree(root)
            .into_iter()
            .filter(|&n| {
                n != el
                    && self.arena.node(n).as_element().is_some_and(|o| {
                        o.is_html_named("details") && o.attr("name") == Some(name.as_str())
                    })
            })
            .collect()
    }

    /// "Ensure details exclusivity" (HTML 4.11.1): `open` added to a grouped `details` closes the other
    /// open members ("closing other elements"); a `name` change of an open `details` closes it when its
    /// new group already has an open member ("closing the given element"). Runs inside the call.
    fn details_exclusivity(&mut self, el: NodeId, local: &str, opened: bool) {
        let open = |d: &Self, n: NodeId| {
            d.arena
                .node(n)
                .as_element()
                .is_some_and(|e| e.attr("open").is_some())
        };
        if !open(self, el) {
            return;
        }
        let group = self.details_name_group(el);
        match local {
            "open" if opened => {
                for o in group {
                    if open(self, o) {
                        let _ = self.remove_attribute(o, "open");
                    }
                }
            }
            "name" if group.iter().any(|&o| open(self, o)) => {
                let _ = self.remove_attribute(el, "open");
            }
            _ => {}
        }
    }

    fn checked_value(&self, v: &str) -> Result<(), Status> {
        self.check_string(v)
    }

    /// `setAttribute(qualifiedName, value)` (A-43).
    pub fn set_attribute(&mut self, el: NodeId, name: &str, value: &str) -> Result<(), Status> {
        let e = self.element_ref(el)?;
        if !is_name(name) {
            return Err(invalid_character());
        }
        self.checked_value(value)?;
        let slot = match Self::find_by_qualified_name(e, name) {
            Some(i) => Slot::Existing(i),
            None => Slot::New(QualName::new(
                None,
                ns!(),
                LocalName::from(Self::lookup_name(e, name)),
            )),
        };
        self.commit_attribute(el, slot, Some(value));
        Ok(())
    }

    /// `setAttributeNS(namespace, qualifiedName, value)` (A-43).
    pub fn set_attribute_ns(
        &mut self,
        el: NodeId,
        namespace: Option<&str>,
        qualified: &str,
        value: &str,
    ) -> Result<(), Status> {
        let e = self.element_ref(el)?;
        let x = validate_and_extract(namespace, qualified)?;
        self.checked_value(value)?;
        let slot = match Self::find_by_ns(e, x.namespace.as_deref(), &x.local) {
            Some(i) => Slot::Existing(i),
            None => Slot::New(QualName::new(
                x.prefix.as_deref().map(Prefix::from),
                Namespace::from(x.namespace.as_deref().unwrap_or("")),
                LocalName::from(x.local.as_str()),
            )),
        };
        self.commit_attribute(el, slot, Some(value));
        Ok(())
    }

    /// `removeAttribute(qualifiedName)` (A-43): no such attribute is no effect (not an error).
    pub fn remove_attribute(&mut self, el: NodeId, name: &str) -> Result<(), Status> {
        let e = self.element_ref(el)?;
        if !is_name(name) {
            return Err(invalid_character());
        }
        if let Some(i) = Self::find_by_qualified_name(e, name) {
            self.commit_attribute(el, Slot::Existing(i), None);
        }
        Ok(())
    }

    pub fn remove_attribute_ns(
        &mut self,
        el: NodeId,
        namespace: Option<&str>,
        local: &str,
    ) -> Result<(), Status> {
        let e = self.element_ref(el)?;
        if let Some(i) = Self::find_by_ns(e, namespace, local) {
            self.commit_attribute(el, Slot::Existing(i), None);
        }
        Ok(())
    }

    /// `toggleAttribute(name, force?)` (A-43): returns whether the attribute is present afterwards.
    pub fn toggle_attribute(
        &mut self,
        el: NodeId,
        name: &str,
        force: Option<bool>,
    ) -> Result<bool, Status> {
        let e = self.element_ref(el)?;
        if !is_name(name) {
            return Err(invalid_character());
        }
        match Self::find_by_qualified_name(e, name) {
            None => {
                if force == Some(false) {
                    return Ok(false);
                }
                let q = QualName::new(None, ns!(), LocalName::from(Self::lookup_name(e, name)));
                self.commit_attribute(el, Slot::New(q), Some(""));
                Ok(true)
            }
            Some(i) => {
                if force == Some(true) {
                    return Ok(true);
                }
                self.commit_attribute(el, Slot::Existing(i), None);
                Ok(false)
            }
        }
    }

    /// `id` setter (A-41).
    pub fn set_id(&mut self, el: NodeId, v: &str) -> Result<(), Status> {
        self.set_attribute(el, "id", v)
    }

    /// `className` setter (A-41).
    pub fn set_class_name(&mut self, el: NodeId, v: &str) -> Result<(), Status> {
        self.set_attribute(el, "class", v)
    }

    /// Write the `class` attribute (used by the token list's update steps).
    pub(crate) fn write_class(&mut self, el: NodeId, value: &str) {
        let e = self.arena.node(el).as_element().expect("validated element");
        let slot = match e
            .attrs
            .iter()
            .position(|a| a.name.ns == ns!() && &*a.name.local == "class")
        {
            Some(i) => Slot::Existing(i),
            None => Slot::New(QualName::new(None, ns!(), LocalName::from("class"))),
        };
        self.commit_attribute(el, slot, Some(value));
    }

    // ------------------------------------------------------------ HTMLElement reflection (A-47)

    /// `hidden` (A-47): presence of the `hidden` attribute.
    pub fn hidden(&self, el: NodeId) -> Result<bool, Status> {
        self.has_attribute(el, "hidden")
    }
    pub fn set_hidden(&mut self, el: NodeId, on: bool) -> Result<(), Status> {
        self.toggle_attribute(el, "hidden", Some(on)).map(|_| ())
    }
    /// `title`, `lang` (A-47): the attribute text, "" when absent.
    pub fn title_attr(&self, el: NodeId) -> Result<String, Status> {
        Ok(self.get_attribute(el, "title")?.unwrap_or("").to_owned())
    }
    pub fn set_title_attr(&mut self, el: NodeId, v: &str) -> Result<(), Status> {
        self.set_attribute(el, "title", v)
    }
    pub fn lang(&self, el: NodeId) -> Result<String, Status> {
        Ok(self.get_attribute(el, "lang")?.unwrap_or("").to_owned())
    }
    pub fn set_lang(&mut self, el: NodeId, v: &str) -> Result<(), Status> {
        self.set_attribute(el, "lang", v)
    }
    /// `dir` getter (A-47): the attribute limited to the known values `ltr`, `rtl`, `auto`, else "".
    pub fn dir(&self, el: NodeId) -> Result<String, Status> {
        let v = self
            .get_attribute(el, "dir")?
            .unwrap_or("")
            .to_ascii_lowercase();
        Ok(if matches!(v.as_str(), "ltr" | "rtl" | "auto") {
            v
        } else {
            String::new()
        })
    }
    pub fn set_dir(&mut self, el: NodeId, v: &str) -> Result<(), Status> {
        self.set_attribute(el, "dir", v)
    }

    // ------------------------------------------------------------ dataset (A-46, FR-039)

    /// camelCase property -> `data-*` attribute name; `None` when the property name is invalid
    /// (a hyphen followed by an ASCII lowercase letter) which the setter reports as `SYNTAX`.
    fn dataset_attr_name(prop: &str) -> Option<String> {
        let mut out = String::from("data-");
        let mut it = prop.chars().peekable();
        while let Some(c) = it.next() {
            if c == '-' && it.peek().is_some_and(char::is_ascii_lowercase) {
                return None;
            }
            if c.is_ascii_uppercase() {
                out.push('-');
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        Some(out)
    }

    /// `dataset[prop]` getter.
    pub fn dataset_get(&self, el: NodeId, prop: &str) -> Result<Option<String>, Status> {
        let e = self.element_ref(el)?;
        let Some(name) = Self::dataset_attr_name(prop) else {
            return Ok(None);
        };
        Ok(e.attrs
            .iter()
            .find(|a| a.name.ns == ns!() && *a.name.local == *name)
            .map(|a| a.value.clone()))
    }

    /// `dataset[prop] = value`: `SYNTAX` for a hyphen followed by a lowercase letter,
    /// `INVALID_CHARACTER` when the resulting attribute name is not a valid name.
    pub fn dataset_set(&mut self, el: NodeId, prop: &str, value: &str) -> Result<(), Status> {
        self.element_ref(el)?;
        let Some(name) = Self::dataset_attr_name(prop) else {
            return Err(syntax_error());
        };
        self.set_attribute(el, &name, value)
    }

    /// `delete dataset[prop]`.
    pub fn dataset_delete(&mut self, el: NodeId, prop: &str) -> Result<(), Status> {
        self.element_ref(el)?;
        if let Some(name) = Self::dataset_attr_name(prop) {
            self.remove_attribute(el, &name)?;
        }
        Ok(())
    }

    /// The `dataset` entries `(property, value)` in attribute order.
    pub fn dataset_list(&self, el: NodeId) -> Result<Vec<(String, String)>, Status> {
        let e = self.element_ref(el)?;
        Ok(e.attrs
            .iter()
            .filter(|a| {
                a.name.ns == ns!()
                    && a.name.local.starts_with("data-")
                    && !a.name.local.chars().any(|c| c.is_ascii_uppercase())
            })
            .map(|a| {
                let rest = &a.name.local["data-".len()..];
                let mut prop = String::new();
                let mut it = rest.chars().peekable();
                while let Some(c) = it.next() {
                    if c == '-' && it.peek().is_some_and(char::is_ascii_lowercase) {
                        prop.push(it.next().expect("peeked").to_ascii_uppercase());
                    } else {
                        prop.push(c);
                    }
                }
                (prop, a.value.clone())
            })
            .collect())
    }
}
