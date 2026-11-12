//! Canonical control classification and current facts. Interaction, layout and
//! paint consume this projection of the model instead of interpreting `type`,
//! disabled state, or current control values independently.
use crate::control_state::Selection;
use crate::document::Document;
use crate::node::ElementData;
use ui4me_engine_types::ids::NodeId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InputType {
    #[default]
    Text,
    Search,
    Telephone,
    Url,
    Email,
    Password,
    Number,
    Hidden,
    Checkbox,
    Radio,
    Range,
    Color,
    Button,
    Submit,
    Reset,
    Image,
    File,
    Date,
    Time,
    Month,
    Week,
    DateTimeLocal,
}
impl InputType {
    pub fn parse(attribute: Option<&str>) -> Self {
        match attribute.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "search" => Self::Search,
            "tel" => Self::Telephone,
            "url" => Self::Url,
            "email" => Self::Email,
            "password" => Self::Password,
            "number" => Self::Number,
            "hidden" => Self::Hidden,
            "checkbox" => Self::Checkbox,
            "radio" => Self::Radio,
            "range" => Self::Range,
            "color" => Self::Color,
            "button" => Self::Button,
            "submit" => Self::Submit,
            "reset" => Self::Reset,
            "image" => Self::Image,
            "file" => Self::File,
            "date" => Self::Date,
            "time" => Self::Time,
            "month" => Self::Month,
            "week" => Self::Week,
            "datetime-local" => Self::DateTimeLocal,
            // HTML's missing and invalid value defaults are both Text.
            _ => Self::Text,
        }
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Search => "search",
            Self::Telephone => "tel",
            Self::Url => "url",
            Self::Email => "email",
            Self::Password => "password",
            Self::Number => "number",
            Self::Hidden => "hidden",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::Range => "range",
            Self::Color => "color",
            Self::Button => "button",
            Self::Submit => "submit",
            Self::Reset => "reset",
            Self::Image => "image",
            Self::File => "file",
            Self::Date => "date",
            Self::Time => "time",
            Self::Month => "month",
            Self::Week => "week",
            Self::DateTimeLocal => "datetime-local",
        }
    }
    pub const fn accepts_text(self) -> bool {
        matches!(
            self,
            Self::Text
                | Self::Search
                | Self::Telephone
                | Self::Url
                | Self::Email
                | Self::Password
                | Self::Number
        )
    }
    pub const fn checkable(self) -> bool {
        matches!(self, Self::Checkbox | Self::Radio)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Input(InputType),
    Textarea,
    Select,
    Button,
    Option,
    OptionGroup,
    Fieldset,
    Progress,
    Meter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlFacts<'a> {
    pub kind: ControlKind,
    pub value: Option<&'a str>,
    pub checked: Option<bool>,
    pub indeterminate: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub selection: Option<Selection>,
}
impl ControlFacts<'_> {
    pub fn accepts_text(self) -> bool {
        match self.kind {
            ControlKind::Textarea => true,
            ControlKind::Input(kind) => kind.accepts_text(),
            _ => false,
        }
    }
}
impl ElementData {
    pub fn input_type(&self) -> Option<InputType> {
        self.is_html_named("input")
            .then(|| InputType::parse(self.attr("type")))
    }
}
impl Document {
    pub fn input_type(&self, node: NodeId) -> Option<InputType> {
        self.arena().get(node)?.as_element()?.input_type()
    }
    pub fn control_facts(&self, node: NodeId) -> Option<ControlFacts<'_>> {
        let e = self
            .arena()
            .get(node)?
            .as_element()
            .filter(|e| e.is_html())?;
        let kind = match e.local_name() {
            "input" => ControlKind::Input(e.input_type()?),
            "textarea" => ControlKind::Textarea,
            "select" => ControlKind::Select,
            "button" => ControlKind::Button,
            "option" => ControlKind::Option,
            "optgroup" => ControlKind::OptionGroup,
            "fieldset" => ControlKind::Fieldset,
            "progress" => ControlKind::Progress,
            "meter" => ControlKind::Meter,
            _ => return None,
        };
        let disabled = match kind {
            ControlKind::Input(_)
            | ControlKind::Textarea
            | ControlKind::Select
            | ControlKind::Button => e.attr("disabled").is_some() || self.fieldset_disabled(node),
            ControlKind::Option => {
                e.attr("disabled").is_some()
                    || self
                        .parent(node)
                        .ok()
                        .flatten()
                        .and_then(|parent| self.arena().get(parent)?.as_element())
                        .is_some_and(|parent| {
                            parent.is_html_named("optgroup") && parent.attr("disabled").is_some()
                        })
            }
            ControlKind::OptionGroup | ControlKind::Fieldset => e.attr("disabled").is_some(),
            _ => false,
        };
        let state = e.control_state();
        Some(ControlFacts {
            kind,
            value: state.and_then(|state| state.value.as_deref()),
            checked: state.and_then(|state| state.checkedness),
            indeterminate: state.is_some_and(|state| state.indeterminate),
            disabled,
            read_only: e.attr("readonly").is_some(),
            selection: state.and_then(|state| state.selection),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_types_default_to_text_and_fieldset_disabling_is_shared() {
        let d = Document::parse_html("<fieldset disabled><legend><input id=legend></legend><input id=blocked type=bogus></fieldset>").unwrap();
        let blocked = d.element_by_id("blocked").unwrap();
        assert_eq!(d.input_type(blocked), Some(InputType::Text));
        assert!(d.control_facts(blocked).unwrap().disabled);
        assert!(
            !d.control_facts(d.element_by_id("legend").unwrap())
                .unwrap()
                .disabled
        );
    }
}
