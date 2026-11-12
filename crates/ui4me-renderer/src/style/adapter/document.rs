//! owner: 006-style-engine — created by 001 slice (R-28)
//!
//! `TDocument` (T041) and the uninhabited shadow-root type (no shadow DOM in v1).

use super::handle::StyleNode;
use style::context::QuirksMode;
use style::dom::{TDocument, TShadowRoot};
use style::shared_lock::SharedRwLock;
use ui4me_document::document::DocumentMode;

pub(crate) fn quirks(mode: DocumentMode) -> QuirksMode {
    match mode {
        DocumentMode::NoQuirks => QuirksMode::NoQuirks,
        DocumentMode::LimitedQuirks => QuirksMode::LimitedQuirks,
        DocumentMode::Quirks => QuirksMode::Quirks,
    }
}

/// Uninhabited: v1 has no shadow DOM.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoShadowRoot<'a>(std::convert::Infallible, std::marker::PhantomData<&'a ()>);

impl<'a> TShadowRoot for NoShadowRoot<'a> {
    type ConcreteNode = StyleNode<'a>;
    fn as_node(&self) -> StyleNode<'a> {
        match self.0 {}
    }
    fn host(&self) -> StyleNode<'a> {
        match self.0 {}
    }
    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        match self.0 {}
    }
}

impl<'a> TDocument for StyleNode<'a> {
    type ConcreteNode = StyleNode<'a>;
    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }
    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> QuirksMode {
        quirks(self.view().mode())
    }
    fn shared_lock(&self) -> &SharedRwLock {
        &self.sidecars().lock
    }
}
