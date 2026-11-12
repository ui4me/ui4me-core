//! owner: 015-core-completion (PAGE-1, lane L-PAGE) — paged media on the shared fragmentation engine (plan §A3
//! "Paged").
//!
//! [`crate::layout::coordinator::layout_paged`] lays a document out as pages beside the screen path (`run_pass` is
//! unchanged): the root element's box becomes the content of a **page flow** (a host node above it with the
//! flow-thread role), laid out once at the page area's width as one strip, with **pagination struts** at the page
//! boundaries (`breaks.rs`, `lines.rs`, table rows) exactly as a multicol row is, except that
//!
//! * fragmentainers may differ in height ([`super::Bands`]: `:first` margins, named pages);
//! * `break-before|after: page | left | right | recto | verso | always` (and the legacy `page-break-*`) force a
//!   break, `avoid-page` avoids one, and boxes whose used `page` values differ are separated by a forced break
//!   (CSS Paged Media 3 section 6.1);
//! * a `left` / `right` break that lands on a page of the wrong side leaves that page **blank** (`:blank`);
//! * every `thead` / `tfoot` repeats on the pages a table is broken across.
//!
//! The page geometry (size and margins of each page, from the `@page` rules the caller cascades) is asked for
//! per page ([`PageGeometry`]); since it depends on which page is first, blank, left or named, the layout is
//! repeated until the pages it produces are the pages it assumed (bounded, [`MAX_PAGE_PASSES`]).
//!
//! **Pages of different widths** (015 PAGE-1 decision D-LPAGE-03): the flow is laid out once per run of pages
//! with the same content width. A run after the first resumes at the block-level box that starts its first page
//! (the break token of the run before it): the strip is laid out again at the new width and that box is moved to
//! the top of the run's first page (a leading strut: everything before it falls into one virtual page that is not
//! output). A break inside a paragraph is not a resumption point (the engine's line breaker cannot restart at a
//! text offset): the run boundary moves forward to the next page that starts at a block-level box, and the pages
//! in between keep the previous width (reported `css.at-rule.page-width-change`).
//!
//! The fragment builder then builds one fragment tree per page (`fragment::builder::build_page`): the root element's
//! piece in that band, placed at the page area's origin, the page box as the viewport.

use std::sync::{Arc, Mutex};

use crate::layout::box_tree::{BoxId, BoxTag, BoxTree};

/// Layouts of a paged document at most, while the page geometry it assumed changes (named / blank pages).
pub const MAX_PAGE_PASSES: usize = 6;

/// One page box (CSS px): its size and margins (top, right, bottom, left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageBox {
    pub width: f32,
    pub height: f32,
    pub margin: [f32; 4],
}

impl PageBox {
    /// The page area's width and height (at least one layout unit tall).
    pub fn content_width(&self) -> f32 {
        (self.width - self.margin[1] - self.margin[3]).max(0.0)
    }
    pub fn content_height(&self) -> f32 {
        (self.height - self.margin[0] - self.margin[2]).max(super::EPS)
    }
    /// The page area's top-left corner in the page box.
    pub fn content_origin(&self) -> (f32, f32) {
        (self.margin[3], self.margin[0])
    }
}

/// What selects a page's `@page` rules (CSS Paged Media 3 section 4.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PageInfo {
    /// 0-based page index (`counter(page)` is `index + 1`).
    pub index: u32,
    pub first: bool,
    pub blank: bool,
    /// A left page (`:left`); the first page is a right page (left-to-right page progression).
    pub left: bool,
    /// The page's name (the used `page` value of the first box that starts on it), `None` = unnamed.
    pub name: Option<Arc<str>>,
}

impl PageInfo {
    /// The page `index` before its content is known: unnamed, not blank.
    pub fn assumed(index: u32) -> PageInfo {
        PageInfo {
            index,
            first: index == 0,
            blank: false,
            left: index % 2 == 1,
            name: None,
        }
    }
}

/// The page geometry of a document: the page box of each page (the caller cascades the `@page` rules).
pub trait PageGeometry {
    fn page_box(&mut self, info: &PageInfo) -> PageBox;
}

/// A fixed geometry: every page the same (no `@page` rules).
impl PageGeometry for PageBox {
    fn page_box(&mut self, _: &PageInfo) -> PageBox {
        *self
    }
}

/// One laid-out page.
#[derive(Debug)]
pub struct PagedPage {
    pub info: PageInfo,
    pub page_box: PageBox,
    /// The page's content fragments in page-area coordinates (the viewport is the page area, whose origin in the
    /// page box is `page_box.content_origin()`).
    pub fragments: crate::layout::fragment::FragmentTree,
    /// The band of the page flow this page shows (flow offsets of the run it belongs to, CSS px).
    pub flow_top: f32,
    pub flow_bottom: f32,
    /// The content width run this page belongs to (0 for the first; 015 PAGE-1 D-LPAGE-03).
    pub run: u32,
}

/// The result of [`crate::layout::coordinator::layout_paged`].
#[derive(Debug)]
pub struct PagedLayout {
    pub boxes: BoxTree,
    pub pages: Vec<PagedPage>,
    pub reports: Vec<crate::layout::coordinator::LayoutReport>,
    pub diagnostics: Vec<ui4me_engine_types::diag::ErrorRecord>,
    /// Layouts of the whole flow it took (geometry passes times width runs).
    pub layouts: u32,
}

impl PagedLayout {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

static NAMES: Mutex<Vec<Arc<str>>> = Mutex::new(Vec::new());

/// The id of a page name (process-wide, 1-based; the box tree keeps ids so `FragStyle` stays `Copy`).
pub fn intern(name: &str) -> u32 {
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = names.iter().position(|n| &**n == name) {
        return i as u32 + 1;
    }
    names.push(Arc::from(name));
    names.len() as u32
}

/// The page name of an id from [`intern`].
pub fn name_of(id: u32) -> Option<Arc<str>> {
    let names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    id.checked_sub(1)
        .and_then(|i| names.get(i as usize).cloned())
}

/// The used `page` value of box `id` (CSS Paged Media 3 section 6.1): its own, else its parent's used value.
pub(crate) fn used_page(boxes: &BoxTree, id: BoxId) -> Option<u32> {
    let mut cur = Some(id);
    while let Some(b) = cur {
        let lb = boxes.get(b);
        if let Some(p) = lb.style.frag.page {
            return Some(p);
        }
        cur = lb.parent;
    }
    None
}

/// Whether box `id` lies inside a multicol container's flow thread (a column context, not the page flow).
pub(crate) fn in_flow_thread(boxes: &BoxTree, id: BoxId) -> bool {
    let mut cur = boxes.get(id).parent;
    while let Some(b) = cur {
        if boxes.get(b).tag == BoxTag::FlowThread {
            return true;
        }
        cur = boxes.get(b).parent;
    }
    false
}

/// The name of each page from where boxes start (`starts`: flow offset and used page name, in the order the
/// layout placed them, innermost first): the first box starting on the page, else the box that continues there.
pub(crate) fn page_names(
    starts: &[(f32, Option<u32>, usize)],
    bands: &[(f32, f32)],
    root_name: Option<u32>,
) -> Vec<Option<u32>> {
    let eps = super::EPS;
    let mut out = Vec::with_capacity(bands.len());
    let mut carry = root_name;
    for &(top, bottom) in bands {
        let mut best: Option<(f32, Option<u32>)> = None;
        for &(y, n, _) in starts {
            if y >= top - eps && y < bottom - eps && best.is_none_or(|(by, _)| y < by - eps) {
                best = Some((y, n));
            }
        }
        if let Some((_, n)) = best {
            carry = n;
        }
        out.push(carry);
        // The name of the content that continues onto the next page: the last box starting before its end.
        let mut last: Option<(f32, Option<u32>)> = None;
        for &(y, n, _) in starts {
            if y < bottom - eps && last.is_none_or(|(ly, _)| y > ly + eps) {
                last = Some((y, n));
            }
        }
        if let Some((_, n)) = last {
            carry = n;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_interned_once() {
        let a = intern("chapter-test");
        assert_eq!(intern("chapter-test"), a);
        assert_eq!(name_of(a).as_deref(), Some("chapter-test"));
        assert_ne!(intern("other-test"), a);
        assert_eq!(name_of(0), None);
    }

    #[test]
    fn page_names_follow_the_first_box_on_each_page() {
        let wide = intern("wide-test");
        let starts = [(0.0, None, 1), (150.0, Some(wide), 2), (260.0, None, 3)];
        let bands = [(0.0, 100.0), (100.0, 200.0), (200.0, 300.0), (300.0, 400.0)];
        assert_eq!(
            page_names(&starts, &bands, None),
            vec![None, Some(wide), None, None]
        );
    }

    #[test]
    fn bands_locate_pages_of_different_heights() {
        let b = super::super::Bands {
            sizes: Some(Arc::new(vec![100.0, 50.0])),
            first_page: 0,
            lead: false,
        };
        assert_eq!(b.locate(0.0, 100.0), (0, 0.0, 100.0));
        assert_eq!(b.locate(99.0, 100.0), (0, 0.0, 100.0));
        assert_eq!(b.locate(100.0, 100.0), (1, 100.0, 50.0));
        assert_eq!(b.locate(175.0, 100.0), (2, 150.0, 50.0));
        assert_eq!(b.top_of(3, 100.0), 200.0);
        assert_eq!(b.size_of(5, 100.0), 50.0);
    }
}
