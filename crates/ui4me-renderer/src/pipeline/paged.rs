//! owner: 015-core-completion (PAGE-1, lane L-PAGE) — paginated output of a document (CSS Paged Media 3).
//!
//! [`DocumentPipeline::paginate`] is the stable entry of the paged path; the screen pipeline (the committed set,
//! the frames) is not touched. It is a **cold** computation over the live DOM: a fresh style stage with the
//! `print` media type and the first page area as viewport, the page contexts of the `@page` rules
//! (`crate::style::page`), the paged layout of `crate::layout::coordinator::layout_paged` (struts and slicing on the
//! shared fragmentation engine), then per page
//!
//! * a **content scene**: the page's fragments in the page area (the root background fills the page area);
//! * a **margin scene**: a synthetic document whose elements are the generated page-margin boxes, placed by the
//!   sizing rules of section 5.3 (the margin boxes' text and boxes are laid out and painted by the ordinary code,
//!   their computed values injected through a synthetic style snapshot), with `counter(page)` / `counter(pages)`
//!   resolved after pagination.
//!
//! [`PagedDocument::render_page`] composes the page: paper white, the page context's background over the page
//! box, the content scene at the page area, the margin scene on top (measured order in the reference: the page
//! background covers the page box, the root background only the page area).
//!
//! Re-pagination after a DOM or style change is a new call: every call is cold, so it equals a cold run by
//! construction (015 PAGE-1 "dynamic updates").

use std::collections::HashMap;
use std::sync::Arc;

use crate::layout::box_tree::StyleBridgeSource;
use crate::layout::coordinator::{LayoutReport, Viewport};
use crate::layout::fragment::FragmentTree;
use crate::layout::fragmentation::paged::{PageBox, PageGeometry, PageInfo};
use crate::raster::headless::{PixelBuffer, PixelFormat};
use crate::scene::appendix_e::{SceneFrame, SceneSnapshot, prepare_scene_scrolled};
use crate::scene::display_list::Color;
use crate::style::device::StyleEnvironment;
use crate::style::page::{
    MARGIN_BOX_NAMES, MarginAlign, MarginBoxStyle, PageQuery, PageStyle, PageValues,
};
use crate::style::traverse::StyleBridge;
use crate::style::view::generated::ContentItem;
use crate::text::fonts::TextServices;
use ui4me_document::document::Document;
use ui4me_document::journal::EpochSegment;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::ids::NodeId;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use crate::pipeline::state::DocumentPipeline;

/// CSS px per inch.
pub const CSS_DPI: f32 = 96.0;

/// Options of a pagination: the default page size and margins (used where `@page` sets none; CSS px), and the
/// media type the document is styled for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageOptions {
    pub width: f32,
    pub height: f32,
    /// Top, right, bottom, left.
    pub margin: [f32; 4],
    /// Style for `print` (default) rather than `screen`.
    pub print: bool,
}

impl PageOptions {
    /// ISO A4 (210 x 297 mm) with 0.5 in margins.
    pub const A4: PageOptions = PageOptions {
        width: 210.0 * CSS_DPI / 25.4,
        height: 297.0 * CSS_DPI / 25.4,
        margin: [48.0; 4],
        print: true,
    };
    /// US Letter (8.5 x 11 in) with 0.5 in margins.
    pub const LETTER: PageOptions = PageOptions {
        width: 8.5 * CSS_DPI,
        height: 11.0 * CSS_DPI,
        margin: [48.0; 4],
        print: true,
    };

    /// A page size by name (`A3`, `A4`, `A5`, `B4`, `B5`, `JIS-B4`, `JIS-B5`, `letter`, `legal`, `ledger`, each
    /// optionally followed by ` landscape`), case-insensitive.
    pub fn named(name: &str) -> Option<(f32, f32)> {
        let lower = name.trim().to_ascii_lowercase();
        let mut parts = lower.split_whitespace();
        let size = parts.next()?;
        let landscape = match parts.next() {
            None | Some("portrait") => false,
            Some("landscape") => true,
            Some(_) => return None,
        };
        let mm = |w: f32, h: f32| (w * CSS_DPI / 25.4, h * CSS_DPI / 25.4);
        let (w, h) = match size {
            "a5" => mm(148.0, 210.0),
            "a4" => mm(210.0, 297.0),
            "a3" => mm(297.0, 420.0),
            "b5" => mm(176.0, 250.0),
            "b4" => mm(250.0, 353.0),
            "jis-b5" => mm(182.0, 257.0),
            "jis-b4" => mm(257.0, 364.0),
            "letter" => (8.5 * CSS_DPI, 11.0 * CSS_DPI),
            "legal" => (8.5 * CSS_DPI, 14.0 * CSS_DPI),
            "ledger" => (11.0 * CSS_DPI, 17.0 * CSS_DPI),
            _ => return None,
        };
        Some(if landscape { (h, w) } else { (w, h) })
    }

    fn validate(&self) -> Result<(), Status> {
        let ok = |v: f32| v.is_finite() && v >= 0.0;
        if !(self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 1.0
            && self.height >= 1.0)
            || !self.margin.iter().all(|m| ok(*m))
        {
            return Err(
                Status::new(Code::InvalidArgument, Stage::Layout).with_message(format!(
                    "page {}x{} with margins {:?} is not a valid page",
                    self.width, self.height, self.margin
                )),
            );
        }
        Ok(())
    }
}

impl Default for PageOptions {
    fn default() -> Self {
        PageOptions::A4
    }
}

/// A generated page-margin box of one page (its border box in page-box coordinates and its resolved text).
#[derive(Clone, Debug, PartialEq)]
pub struct MarginBoxOutput {
    /// `top-left`, `bottom-center`, ... (CSS Paged Media 3 section 5).
    pub name: &'static str,
    /// x, y, width, height (CSS px, page box).
    pub rect: [f32; 4],
    pub text: String,
}

/// One page of a [`PagedDocument`].
#[derive(Debug)]
pub struct PageOutput {
    /// 0-based (`counter(page)` is `index + 1`).
    pub index: u32,
    pub name: Option<String>,
    pub first: bool,
    pub blank: bool,
    pub left: bool,
    /// The page box (CSS px) and its margins (top, right, bottom, left).
    pub width: f32,
    pub height: f32,
    pub margin: [f32; 4],
    /// The content fragments in page-area coordinates (`content_origin` is the area's origin in the page box).
    pub fragments: Arc<FragmentTree>,
    pub margin_boxes: Vec<MarginBoxOutput>,
    background: Option<[u8; 4]>,
    content_scene: SceneSnapshot,
    margin_scene: Option<SceneSnapshot>,
}

impl PageOutput {
    /// The page area's origin in the page box.
    pub fn content_origin(&self) -> (f32, f32) {
        (self.margin[3], self.margin[0])
    }
    /// The page area's size.
    pub fn content_size(&self) -> (f32, f32) {
        (
            (self.width - self.margin[1] - self.margin[3]).max(0.0),
            (self.height - self.margin[0] - self.margin[2]).max(0.0),
        )
    }
}

/// The paginated output of a document ([`DocumentPipeline::paginate`]).
#[derive(Debug)]
pub struct PagedDocument {
    pub pages: Vec<PageOutput>,
    /// Layout-stage `UNSUPPORTED_FEATURE` reports of the paged layout.
    pub reports: Vec<LayoutReport>,
    /// Layouts of the whole flow the pagination took.
    pub layouts: u32,
    /// The DOM revision the pages show.
    pub dom_revision: u64,
}

impl PagedDocument {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The border-box rectangles of element `node` on each page it appears on: (page index, x, y, width, height)
    /// in page-box coordinates (CSS px).
    pub fn element_rects(&self, node: NodeId) -> Vec<(u32, [f32; 4])> {
        let mut out = Vec::new();
        for p in &self.pages {
            let (cx, cy) = p.content_origin();
            for &f in p.fragments.fragments_of(node) {
                if matches!(
                    p.fragments.get(f).kind,
                    crate::layout::fragment::FragmentKind::Box(_)
                ) {
                    let r = p.fragments.document_rect(f).rect;
                    out.push((
                        p.index,
                        [
                            r.origin.x.to_f64() as f32 + cx,
                            r.origin.y.to_f64() as f32 + cy,
                            r.size.width.to_f64() as f32,
                            r.size.height.to_f64() as f32,
                        ],
                    ));
                }
            }
            if !out.iter().any(|(i, _)| *i == p.index)
                && let Some(r) = p.fragments.element_border_box(node)
            {
                let r = r.rect;
                out.push((
                    p.index,
                    [
                        r.origin.x.to_f64() as f32 + cx,
                        r.origin.y.to_f64() as f32 + cy,
                        r.size.width.to_f64() as f32,
                        r.size.height.to_f64() as f32,
                    ],
                ));
            }
        }
        out
    }

    /// The text runs of page `index` in paint order: (text, x, y, width, height) of each text fragment's content area
    /// in page-box coordinates (CSS px). Margin boxes are not included ([`PageOutput::margin_boxes`]).
    pub fn text_runs(&self, index: usize) -> Vec<(String, [f32; 4])> {
        let Some(p) = self.pages.get(index) else {
            return Vec::new();
        };
        let (cx, cy) = p.content_origin();
        let t = &p.fragments;
        let mut out = Vec::new();
        for (i, f) in t.fragments.iter().enumerate() {
            if let crate::layout::fragment::FragmentKind::Text(tf) = &f.kind {
                let s = t
                    .paragraphs
                    .get(tf.paragraph_index)
                    .and_then(|para| para.text().get(tf.text_range.clone()))
                    .unwrap_or("")
                    .to_string();
                let r = t
                    .document_rect(crate::layout::fragment::FragmentId(i as u32))
                    .rect;
                out.push((
                    s,
                    [
                        r.origin.x.to_f64() as f32 + cx,
                        r.origin.y.to_f64() as f32 + cy,
                        r.size.width.to_f64() as f32,
                        r.size.height.to_f64() as f32,
                    ],
                ));
            }
        }
        out
    }

    /// The page holding the first box of element `node`.
    pub fn page_of(&self, node: NodeId) -> Option<u32> {
        self.element_rects(node).first().map(|(i, _)| *i)
    }

    /// Render page `index` at `dpi` (96 = one device pixel per CSS px) to straight-alpha RGBA8 (opaque: the paper
    /// is white): width, height and the pixels.
    pub fn render_page(&self, index: usize, dpi: f32) -> Result<PixelBuffer, Status> {
        let p = self.pages.get(index).ok_or_else(|| {
            Status::new(Code::InvalidArgument, Stage::Raster)
                .with_message(format!("page {index} of {}", self.pages.len()))
        })?;
        if !(dpi.is_finite() && dpi > 0.0 && dpi <= 2400.0) {
            return Err(Status::new(Code::InvalidArgument, Stage::Raster)
                .with_message(format!("dpi {dpi}")));
        }
        let scale = dpi / CSS_DPI;
        let w = (p.width * scale).ceil().max(1.0) as u32;
        let h = (p.height * scale).ceil().max(1.0) as u32;
        let mut data = vec![255u8; (w as usize) * (h as usize) * 4];
        if let Some(bg) = p.background {
            fill(&mut data, w, bg);
        }
        let content = crate::pipeline::frame::raster_scratch(&p.content_scene, scale as f64)?;
        let (cx, cy) = p.content_origin();
        composite(
            &mut data,
            w,
            h,
            &content,
            (cx * scale).round() as i64,
            (cy * scale).round() as i64,
        );
        if let Some(m) = &p.margin_scene {
            let margins = crate::pipeline::frame::raster_scratch(m, scale as f64)?;
            composite(&mut data, w, h, &margins, 0, 0);
        }
        Ok(PixelBuffer {
            width: w,
            height: h,
            scale,
            format: PixelFormat::Rgba8Unpremultiplied,
            data,
        })
    }
}

/// Source-over of straight colour `c` on every (opaque) pixel.
fn fill(data: &mut [u8], _w: u32, c: [u8; 4]) {
    let a = c[3] as u32;
    for px in data.as_chunks_mut::<4>().0 {
        for k in 0..3 {
            px[k] = ((c[k] as u32 * a + px[k] as u32 * (255 - a) + 127) / 255) as u8;
        }
    }
}

/// Source-over of a premultiplied raster at (x, y) on the opaque page.
fn composite(dst: &mut [u8], w: u32, h: u32, src: &PixelBuffer, x: i64, y: i64) {
    let premul = src.format == PixelFormat::Rgba8Premultiplied;
    for sy in 0..src.height as i64 {
        let dy = sy + y;
        if dy < 0 || dy >= h as i64 {
            continue;
        }
        for sx in 0..src.width as i64 {
            let dx = sx + x;
            if dx < 0 || dx >= w as i64 {
                continue;
            }
            let s = ((sy * src.width as i64 + sx) * 4) as usize;
            let d = ((dy * w as i64 + dx) * 4) as usize;
            let a = src.data[s + 3] as u32;
            if a == 0 {
                continue;
            }
            for k in 0..3 {
                let sc = src.data[s + k] as u32;
                let sc = if premul { sc } else { (sc * a + 127) / 255 };
                dst[d + k] = (sc + (dst[d + k] as u32 * (255 - a) + 127) / 255).min(255) as u8;
            }
            dst[d + 3] = 255;
        }
    }
}

/// The page geometry of the document from its `@page` rules (cached per selector set).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PageSelectorKey {
    first: bool,
    blank: bool,
    left: bool,
    name: Option<Arc<str>>,
}

struct StylePages<'a> {
    style: &'a StyleBridge,
    doc: &'a Document,
    opts: PageOptions,
    cache: HashMap<PageSelectorKey, (PageBox, PageStyle)>,
}

impl StylePages<'_> {
    fn page(&mut self, info: &PageInfo) -> (PageBox, PageStyle) {
        let key = PageSelectorKey {
            first: info.first,
            blank: info.blank,
            left: info.left,
            name: info.name.clone(),
        };
        if let Some(v) = self.cache.get(&key) {
            return v.clone();
        }
        let ps = self.style.page_style(
            self.doc,
            PageQuery {
                first: info.first,
                blank: info.blank,
                left: info.left,
                name: info.name.as_deref().unwrap_or(""),
            },
        );
        let (w, h) = ps.page_size((self.opts.width, self.opts.height));
        let m = |k: usize, basis: f32| {
            if ps.margin_declared[k] {
                ps.margin[k].resolve(basis)
            } else {
                self.opts.margin[k]
            }
        };
        let pb = PageBox {
            width: w,
            height: h,
            margin: [m(0, h), m(1, w), m(2, h), m(3, w)],
        };
        self.cache.insert(key, (pb, ps.clone()));
        (pb, ps)
    }
}

impl PageGeometry for StylePages<'_> {
    fn page_box(&mut self, info: &PageInfo) -> PageBox {
        self.page(info).0
    }
}

/// The text of a margin box's `content` on page `page` of `pages` (1-based page number).
fn content_text(items: &[ContentItem], page: u32, pages: u32) -> String {
    let mut s = String::new();
    for it in items {
        match it {
            ContentItem::String(t) => s.push_str(t),
            ContentItem::Counter { name, style } | ContentItem::Counters { name, style, .. } => {
                let v = match name.as_str() {
                    "page" => page as i32,
                    "pages" => pages as i32,
                    _ => 0,
                };
                s.push_str(&crate::layout::box_tree::lists::counter_text(style, v));
            }
            _ => {}
        }
    }
    s
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Resolve two auto sizes into `avail` (CSS Paged Media 3 section 5.3.2.1): by their max-content sizes when both
/// fit, else by the room between min- and max-content, else by their min-content sizes.
fn resolve_two(avail: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (amin, amax) = a;
    let (bmin, bmax) = b;
    let (smin, smax) = (amin + bmin, amax + bmax);
    if smax <= 0.0 {
        return (avail / 2.0, avail / 2.0);
    }
    if smax <= avail {
        let extra = avail - smax;
        (amax + extra * amax / smax, bmax + extra * bmax / smax)
    } else if smin <= avail {
        let flex = (amax - amin) + (bmax - bmin);
        let extra = avail - smin;
        if flex <= 0.0 {
            (amin + extra / 2.0, bmin + extra / 2.0)
        } else {
            (
                amin + extra * (amax - amin) / flex,
                bmin + extra * (bmax - bmin) / flex,
            )
        }
    } else if smin > 0.0 {
        (avail * amin / smin, avail * bmin / smin)
    } else {
        (avail / 2.0, avail / 2.0)
    }
}

/// The sizes of the three boxes of one edge (start, center, end) along `avail` (section 5.3.2): each `Some((min,
/// max, specified))` when generated. Returns (offset, size) per box.
fn resolve_edge(avail: f32, boxes: [Option<(f32, f32, Option<f32>)>; 3]) -> [(f32, f32); 3] {
    let [a, b, c] = boxes;
    let mm = |x: Option<(f32, f32, Option<f32>)>| {
        x.map_or((0.0, 0.0), |(mn, mx, sp)| sp.map_or((mn, mx), |s| (s, s)))
    };
    match b {
        None => {
            let (wa, wc) = match (a, c) {
                (Some((_, _, Some(sa))), Some((_, _, Some(sc)))) => (sa, sc),
                (Some((_, _, Some(sa))), _) => (sa, (avail - sa).max(0.0)),
                (_, Some((_, _, Some(sc)))) => ((avail - sc).max(0.0), sc),
                (None, None) => (0.0, 0.0),
                (Some(_), None) => (avail, 0.0),
                (None, Some(_)) => (0.0, avail),
                _ => resolve_two(avail, mm(a), mm(c)),
            };
            [(0.0, wa), (0.0, 0.0), (avail - wc, wc)]
        }
        Some((bmin, bmax, bspec)) => {
            let (amin, amax) = mm(a);
            let (cmin, cmax) = mm(c);
            let ac = (2.0 * amin.max(cmin), 2.0 * amax.max(cmax));
            let wb = match bspec {
                Some(s) => s,
                None if a.is_none() && c.is_none() => avail,
                None => resolve_two(avail, (bmin, bmax), ac).0,
            };
            let side = ((avail - wb) / 2.0).max(0.0);
            [(0.0, side), ((avail - wb) / 2.0, wb), (avail - side, side)]
        }
    }
}

/// Where a margin box sits (section 5.3): x, y, width, height in the page box.
fn place_boxes(
    pb: &PageBox,
    boxes: &[(usize, (f32, f32), f32)],
    specified: &HashMap<usize, (Option<f32>, Option<f32>)>,
) -> HashMap<usize, [f32; 4]> {
    let [mt, mr, mb, ml] = pb.margin;
    let (w, h) = (pb.width, pb.height);
    let get = |slot: usize, vertical: bool| {
        boxes
            .iter()
            .find(|b| b.0 == slot)
            .map(|&(_, (mn, mx), hgt)| {
                let sp = specified.get(&slot).copied().unwrap_or((None, None));
                if vertical {
                    (hgt, hgt, sp.1)
                } else {
                    (mn, mx, sp.0)
                }
            })
    };
    let mut out = HashMap::new();
    for (slot, rect) in [
        (0usize, [0.0, 0.0, ml, mt]),
        (4, [w - mr, 0.0, mr, mt]),
        (5, [0.0, h - mb, ml, mb]),
        (9, [w - mr, h - mb, mr, mb]),
    ] {
        if boxes.iter().any(|b| b.0 == slot) {
            out.insert(slot, rect);
        }
    }
    let hw = (w - ml - mr).max(0.0);
    let vh = (h - mt - mb).max(0.0);
    for (slots, y, bh) in [([1usize, 2, 3], 0.0, mt), ([6, 7, 8], h - mb, mb)] {
        let r = resolve_edge(
            hw,
            [
                get(slots[0], false),
                get(slots[1], false),
                get(slots[2], false),
            ],
        );
        for (k, s) in slots.iter().enumerate() {
            if boxes.iter().any(|b| b.0 == *s) {
                out.insert(*s, [ml + r[k].0, y, r[k].1, bh]);
            }
        }
    }
    for (slots, x, bw) in [([10usize, 11, 12], 0.0, ml), ([13, 14, 15], w - mr, mr)] {
        let r = resolve_edge(
            vh,
            [
                get(slots[0], true),
                get(slots[1], true),
                get(slots[2], true),
            ],
        );
        for (k, s) in slots.iter().enumerate() {
            if boxes.iter().any(|b| b.0 == *s) {
                out.insert(*s, [x, mt + r[k].0, bw, r[k].1]);
            }
        }
    }
    out
}

/// A synthetic document: `<html><head></head><body>` + one `<div>` per text, and its element ids in order
/// (html, head, body, divs).
fn synthetic_doc(
    texts: &[String],
) -> Result<(Document, NodeId, NodeId, NodeId, Vec<NodeId>), Status> {
    let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
    for t in texts {
        html.push_str("<div>");
        html.push_str(&esc(t));
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    let doc = Document::parse_html(&html)?;
    let root = doc.document_element().ok_or_else(|| {
        Status::new(Code::InternalInvariant, Stage::Layout)
            .with_message("synthetic margin document")
    })?;
    let head = doc
        .head()
        .ok_or_else(|| Status::new(Code::InternalInvariant, Stage::Layout))?;
    let body = doc
        .body()
        .ok_or_else(|| Status::new(Code::InternalInvariant, Stage::Layout))?;
    let divs: Vec<NodeId> = doc
        .children(body)?
        .iter()
        .copied()
        .filter(|n| doc.arena().node(*n).is_element())
        .collect();
    Ok((doc, root, head, body, divs))
}

/// The scene of a fragment tree with a transparent canvas default (the page composes the layers).
fn transparent_scene(
    doc: &Document,
    snap: &crate::style::view::StyleSnapshot,
    fragments: &FragmentTree,
    images: Option<&Arc<dyn crate::scene::inputs::ImageSource>>,
    generation: u64,
) -> SceneSnapshot {
    let cache = std::cell::RefCell::new(crate::pipeline::update::PaintStyleCache::default());
    let src = crate::pipeline::update::PaintStyles {
        doc,
        style: snap,
        images: images.map(|i| i.as_ref()),
        caret: (false, false),
        cache: &cache,
        scrollbars: false,
    };
    let revision = snap.revision_tuple();
    let frame = SceneFrame {
        generation,
        revision,
        default_canvas: Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        },
    };
    prepare_scene_scrolled(
        fragments,
        &src,
        &frame,
        crate::pipeline::update::SCENE_PAINT_OPTIONS,
        &crate::layout::coordinator::overflow::ScrollOffsets::new(),
    )
    .build()
}

/// Lay out a synthetic margin document (`entries`: element styles) in a `w` x `h` viewport.
fn layout_synthetic(
    doc: &Document,
    snap: &crate::style::view::StyleSnapshot,
    w: f32,
    h: f32,
    text: &mut TextServices,
) -> Result<FragmentTree, Status> {
    let src = StyleBridgeSource::new(doc, snap);
    let out = crate::layout::coordinator::try_layout_document(
        &src,
        Some(Viewport {
            width: w,
            height: h,
            scale: 1.0,
        }),
        text,
    )?;
    Ok(out.fragments)
}

fn rect_of(t: &FragmentTree, n: NodeId) -> [f32; 4] {
    t.element_border_box(n).map_or([0.0; 4], |r| {
        let r = r.rect;
        [
            r.origin.x.to_f64() as f32,
            r.origin.y.to_f64() as f32,
            r.size.width.to_f64() as f32,
            r.size.height.to_f64() as f32,
        ]
    })
}

/// The margin boxes of one page: their placement and the scene of the synthetic document that holds them.
fn margin_layer(
    style: &StyleBridge,
    ps: &PageStyle,
    pb: &PageBox,
    page: u32,
    pages: u32,
    text: &mut TextServices,
    generation: u64,
) -> Result<(Vec<MarginBoxOutput>, Option<SceneSnapshot>), Status> {
    if ps.boxes.is_empty() {
        return Ok((Vec::new(), None));
    }
    let texts: Vec<String> = ps
        .boxes
        .iter()
        .map(|b| content_text(&b.content, page, pages))
        .collect();
    let initial = style.initial_values();
    let root_v = style.synthetic_values(None, "display: block; margin: 0; padding: 0; border: 0");
    let head_v = style.synthetic_values(Some(&root_v), "display: none");
    let body_v = style.synthetic_values(
        Some(&root_v),
        "display: block; margin: 0; padding: 0; border: 0",
    );
    // Measure: max-content and min-content widths, and the height at the side margin's width (left / right boxes).
    let probe_texts: Vec<String> = texts
        .iter()
        .flat_map(|t| [t.clone(), t.clone(), t.clone()])
        .collect();
    let (pdoc, proot, phead, pbody, pdivs) = synthetic_doc(&probe_texts)?;
    let mut entries: Vec<(NodeId, PageValues)> = vec![
        (proot, root_v.clone()),
        (phead, head_v.clone()),
        (pbody, body_v.clone()),
    ];
    for (k, b) in ps.boxes.iter().enumerate() {
        let side_w = if (10..13).contains(&b.slot) {
            pb.margin[3]
        } else {
            pb.margin[1]
        };
        for (j, css) in [
            "position: absolute; left: 0; top: 0; width: max-content".to_string(),
            "position: absolute; left: 0; top: 0; width: min-content".to_string(),
            format!(
                "position: absolute; left: 0; top: 0; width: {side_w}px; box-sizing: border-box"
            ),
        ]
        .iter()
        .enumerate()
        {
            entries.push((pdivs[3 * k + j], style.margin_box_values(ps, b, css)));
        }
    }
    let psnap = crate::style::page::synthetic_snapshot(pdoc.id(), entries, &initial);
    let probe = layout_synthetic(&pdoc, &psnap, pb.width.max(1.0), pb.height.max(1.0), text)?;
    let measured: Vec<(usize, (f32, f32), f32)> = ps
        .boxes
        .iter()
        .enumerate()
        .map(|(k, b)| {
            (
                b.slot,
                (
                    rect_of(&probe, pdivs[3 * k + 1])[2],
                    rect_of(&probe, pdivs[3 * k])[2],
                ),
                rect_of(&probe, pdivs[3 * k + 2])[3],
            )
        })
        .collect();
    let specified: HashMap<usize, (Option<f32>, Option<f32>)> = ps
        .boxes
        .iter()
        .map(|b| (b.slot, (b.width, b.height)))
        .collect();
    let placed = place_boxes(pb, &measured, &specified);
    // The final document.
    let (doc, root, head, body, divs) = synthetic_doc(&texts)?;
    let mut entries: Vec<(NodeId, PageValues)> =
        vec![(root, root_v), (head, head_v), (body, body_v)];
    let mut out = Vec::new();
    for (k, b) in ps.boxes.iter().enumerate() {
        let r = placed.get(&b.slot).copied().unwrap_or([0.0; 4]);
        let justify = match b.align {
            MarginAlign::Top => "flex-start",
            MarginAlign::Middle => "center",
            MarginAlign::Bottom => "flex-end",
        };
        let css = format!(
            "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; box-sizing: border-box; display: flex; flex-direction: column; justify-content: {justify}; overflow: visible",
            r[0],
            r[1],
            r[2].max(0.0),
            r[3].max(0.0)
        );
        entries.push((divs[k], margin_values(style, ps, b, &css)));
        out.push(MarginBoxOutput {
            name: MARGIN_BOX_NAMES[b.slot],
            rect: r,
            text: texts[k].clone(),
        });
    }
    let snap = crate::style::page::synthetic_snapshot(doc.id(), entries, &initial);
    let tree = layout_synthetic(&doc, &snap, pb.width.max(1.0), pb.height.max(1.0), text)?;
    let scene = transparent_scene(&doc, &snap, &tree, None, generation);
    Ok((out, Some(scene)))
}

fn margin_values(style: &StyleBridge, ps: &PageStyle, b: &MarginBoxStyle, css: &str) -> PageValues {
    style.margin_box_values(ps, b, css)
}

impl DocumentPipeline {
    /// Paginate the document (015 PAGE-1): a cold style, paged layout and scene computation over the live DOM with
    /// the `print` media type (unless `opts.print` is false) and the `@page` rules; the screen pipeline is not
    /// changed. Each call reflects the DOM and style as they are now.
    pub fn paginate(&mut self, opts: &PageOptions) -> Result<PagedDocument, Status> {
        self.check_open()?;
        opts.validate()?;
        let doc = &self.doc;
        let text = TextServices::new(&self.fonts)?;
        let mut text = text;
        // The style stage of the paged output: `print` media, the first page area as the viewport (the initial
        // containing block of paged media, CSS Paged Media 3 section 3.2); restyled once more when `@page` gives the
        // first page another area than the options assumed.
        let mut area = (
            (opts.width - opts.margin[1] - opts.margin[3]).max(1.0),
            (opts.height - opts.margin[0] - opts.margin[2]).max(1.0),
        );
        let mut style;
        let mut round = 0;
        loop {
            let mut env = StyleEnvironment::new(area.0, area.1, 1.0);
            env.print = opts.print;
            env.reduced_motion = self.reduced_motion;
            env.color_scheme = if self.dark {
                crate::style::device::ColorScheme::Dark
            } else {
                crate::style::device::ColorScheme::Light
            };
            style = crate::pipeline::lifecycle::style_bridge_for(doc.id(), env, &text)?;
            crate::pipeline::resources::configure_style(
                &mut style,
                self.resources.as_ref(),
                doc,
                1.0,
                area,
            );
            style.restyle(doc, &EpochSegment::empty())?;
            let root_name = doc.document_element().and_then(|r| style.page_name(doc, r));
            let mut pages = StylePages {
                style: &style,
                doc,
                opts: *opts,
                cache: HashMap::new(),
            };
            let first = pages.page_box(&PageInfo {
                index: 0,
                first: true,
                blank: false,
                left: false,
                name: root_name.as_deref().map(Arc::from),
            });
            let a = (
                first.content_width().max(1.0),
                first.content_height().max(1.0),
            );
            round += 1;
            if ((a.0 - area.0).abs() < 0.01 && (a.1 - area.1).abs() < 0.01) || round >= 2 {
                break;
            }
            area = a;
        }
        let snap = style.snapshot();
        let natural = |n: NodeId| {
            self.resources
                .as_ref()
                .and_then(|r| r.image_element_state_in(doc, n))
        };
        let css_natural = |n: NodeId, property: &str| {
            self.resources
                .as_ref()
                .and_then(|r| r.css_image_state(n, property))
        };
        let src = StyleBridgeSource::new(doc, &snap)
            .with_images(&natural)
            .with_css_images(&css_natural);
        let mut geom = StylePages {
            style: &style,
            doc,
            opts: *opts,
            cache: HashMap::new(),
        };
        let layout = crate::layout::coordinator::layout_paged(&src, &mut geom, 1.0, &mut text)?;
        let total = layout.pages.len() as u32;
        let mut pages = Vec::with_capacity(layout.pages.len());
        for (k, p) in layout.pages.into_iter().enumerate() {
            let (pb, ps) = geom.page(&p.info);
            let generation = k as u64 + 1;
            let content_scene =
                transparent_scene(doc, &snap, &p.fragments, self.images.as_ref(), generation);
            let (margin_boxes, margin_scene) = margin_layer(
                &style,
                &ps,
                &p.page_box,
                p.info.index + 1,
                total,
                &mut text,
                generation,
            )?;
            let _ = pb;
            pages.push(PageOutput {
                index: p.info.index,
                name: p.info.name.as_deref().map(str::to_string),
                first: p.info.first,
                blank: p.info.blank,
                left: p.info.left,
                width: p.page_box.width,
                height: p.page_box.height,
                margin: p.page_box.margin,
                fragments: Arc::new(p.fragments),
                margin_boxes,
                background: ps.background,
                content_scene,
                margin_scene,
            });
        }
        Ok(PagedDocument {
            pages,
            reports: layout.reports,
            layouts: layout.layouts,
            dom_revision: doc.revision(),
        })
    }
}
