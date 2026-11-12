//! owner: 015-core-completion (FRAG-2b, lane L-FRAG) — the block fragmentation engine (plan §A3).
//!
//! **Model: flow thread + pagination struts + slicing** (the legacy WebKit/Blink multicol model). The
//! content of a fragmentation root is laid out once, at one inline size, as one tall strip: the *flow
//! thread*. Fragmentainers are consecutive bands of that strip, `block_size` tall. When a unit (a
//! block-level box that may not be split, a line box, a box whose break is forced) would cross a
//! fragmentainer boundary, a **strut** pushes it to the boundary. The fragment builder then **slices**
//! the strip into fragmentainers (`slice.rs`): every box that crosses a boundary becomes one piece per
//! fragmentainer (`FragmentPiece`), and every piece is placed in its fragmentainer.
//!
//! * The struts of block-level boxes come from Taffy's block algorithm through the hook of patch P-008-3
//!   (`LayoutBlockContainer::begin_block_child` / `end_block_child`), which the host tree forwards to the
//!   [`FragContext`] of the root being laid out (`breaks.rs`).
//! * The struts of lines come from a [`lines::FragmentingSpace`], which wraps the inline formatting
//!   context's float space: a line that would cross a boundary finds no room, and the shift-down loop of
//!   the line breaker moves it to the boundary.
//! * Monolithic content (replaced boxes, atomic inlines, scroll containers, out-of-flow boxes, nested multicol
//!   containers) is pushed whole, or overflows the fragmentainer when it already starts at its top.
//!   Fragmentation is suspended inside it. Flex and grid containers break between their runs and inside their
//!   items (015 STAB-1, `flexgrid.rs`).
//! * Inside a root, a box's layout depends on its block offset, which Taffy's cache key does not
//!   hold: the `PerformLayout` cache entries are bypassed while a root is being laid out.
//! * The block offset a child is laid out at is an estimate (margins that collapse through it are known
//!   only afterwards, and a strut moves it after its content was laid out): the root is laid out again
//!   with the corrected offsets while they change, at most [`MAX_PASSES`] times.
//!
//! **Zero cost when nothing fragments:** no root exists, `HostTree::frag` stays `None`, every hook returns
//! at once, and the fragment builder takes the slicing branch only for a flow thread with an outcome.
//!
//! **Roots:** the rows of a multicol container (`multicol.rs`, MC-1): `column-count` / `column-width`,
//! balanced or filled column heights, spanners, rules; columns past the count overflow in the inline
//! direction. Paged media is PAGE-1. Break quality (FRAG-2c): the appeal of breaks (CSS Break 3
//! section 4.4) with backtracking, orphans and widows on lines, floats pushed with their line, out-of-flow
//! boxes in the fragmentainer of their top, table rows with repeated headers and footers.

pub mod breaks;
pub mod flexgrid;
pub mod lines;
pub mod multicol;
pub mod paged;
pub mod slice;

use std::collections::{HashMap, HashSet};

/// Layout passes of one root at most (the first and the corrected re-layouts; plan §A3: ≤ 3).
pub const MAX_PASSES: usize = 6;

/// Tolerance of block offsets (1/64 px, one layout unit).
pub(crate) const EPS: f32 = 1.0 / 64.0;

/// The fragmentainer size of balancing's measuring layout (MC-1): effectively unbounded, still finite.
pub(crate) const UNBOUNDED: f32 = 1.0e9;

pub use multicol::{MulticolSpec, multicol};

/// How a break was taken (CSS Break 3 section 4.4). Ordered from worst to best: FRAG-2c's backtracking
/// keeps the most appealing break of a fragmentainer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BreakAppeal {
    /// A break that violates everything (inside monolithic overflow, or between units that avoid it).
    LastResort,
    /// A break that leaves fewer lines than `orphans` / `widows` (FRAG-2c).
    ViolatingOrphansAndWidows,
    /// A break inside or beside a box that asks to avoid it.
    ViolatingBreakAvoid,
    Perfect,
}

/// What made a break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BreakKind {
    /// `break-before` / `break-after` (or a legacy `page-break-*`) forced it.
    Forced,
    /// A block-level box was pushed whole to the next fragmentainer (monolithic, `break-inside: avoid`).
    PushedBox,
    /// A line box was pushed to the next fragmentainer.
    PushedLine,
    /// An unforced break fell inside a margin, which is truncated.
    TruncatedMargin,
}

/// One break of a fragmentation root's flow (an output: geometry queries, incremental reuse, and the
/// seam for fragmentainers of different sizes later).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakToken {
    /// The host node (box) that starts after the break: the pushed box, or the inline root of a line.
    pub node: usize,
    /// The fragmentainer boundary in the flow thread (CSS px from its top).
    pub offset: f32,
    pub kind: BreakKind,
    pub appeal: BreakAppeal,
}

/// One fragmentainer of a root: a band of the flow thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fragmentainer {
    pub index: u32,
    /// Top of the band in the flow thread.
    pub block_offset: f32,
    pub block_size: f32,
}

/// Fragmentainer block sizes along a flow thread (015 PAGE-1: pages of different heights). `sizes[i]` is the
/// size of the i-th fragmentainer, the last one repeating; with no sizes every fragmentainer is as tall as the
/// context's uniform size (the multicol case).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Bands {
    pub sizes: Option<std::sync::Arc<Vec<f32>>>,
    /// The page index of the first fragmentainer (pages: a later width run starts with a virtual band, `-1`
    /// relative to its first page); page `k + first_page` is a right page when even.
    pub first_page: i32,
    /// The first fragmentainer is the virtual leading band of a later width run (015 PAGE-1): what lies in it is
    /// laid out as the run before laid it out at this width, unfragmented: no forced break is taken inside it.
    pub lead: bool,
}

impl Bands {
    /// (index, top, size) of the fragmentainer holding flow offset `y`.
    pub(crate) fn locate(&self, y: f32, uniform: f32) -> (u32, f32, f32) {
        let Some(sizes) = self.sizes.as_deref().filter(|s| !s.is_empty()) else {
            let k = ((y + EPS) / uniform).floor().max(0.0);
            return (k as u32, k * uniform, uniform);
        };
        let mut acc = 0.0f32;
        for (i, s) in sizes.iter().enumerate() {
            let s = s.max(EPS);
            if y + EPS < acc + s {
                return (i as u32, acc, s);
            }
            acc += s;
        }
        let last = sizes[sizes.len() - 1].max(EPS);
        let k = ((y + EPS - acc) / last).floor().max(0.0);
        (sizes.len() as u32 + k as u32, acc + k * last, last)
    }

    /// The top of the fragmentainer with index `k`.
    pub(crate) fn top_of(&self, k: u32, uniform: f32) -> f32 {
        let Some(sizes) = self.sizes.as_deref().filter(|s| !s.is_empty()) else {
            return k as f32 * uniform;
        };
        let n = (k as usize).min(sizes.len());
        let acc: f32 = sizes[..n].iter().map(|s| s.max(EPS)).sum();
        acc + (k as usize - n) as f32 * sizes[sizes.len() - 1].max(EPS)
    }

    /// The size of the fragmentainer with index `k`.
    pub(crate) fn size_of(&self, k: u32, uniform: f32) -> f32 {
        match self.sizes.as_deref().filter(|s| !s.is_empty()) {
            None => uniform,
            Some(s) => s[(k as usize).min(s.len() - 1)].max(EPS),
        }
    }
}

/// A forced break's side (CSS Break 3 section 3.1, 015 PAGE-1): `left` / `verso`, `right` / `recto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum PageSide {
    #[default]
    Any,
    Left,
    Right,
}

/// The fragmentation-relevant facts of a host node, fixed when the host tree is built.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FragFacts {
    /// Never split: pushed whole or overflowing; fragmentation is suspended inside it.
    pub monolithic: bool,
    /// An in-flow item of a flex or grid container (015 STAB-1, `flexgrid.rs`): laid out at the offset its
    /// container's run gives it, its growth measured.
    pub fc_item: bool,
    /// A float that is not monolithic otherwise (015 MC-2): a breakable block container, laid out with the
    /// fragmentation context at the offset its placement gave in the previous pass (`FragContext::float_at`).
    pub float: bool,
    /// A piece of an element split around spanners that lies between two of them (015 MC-2): the room its row
    /// keeps (the element's block-end border and padding; 0 otherwise).
    pub split_reserve: f32,
    /// `clear` is not `none`: the box is placed below floats, which may continue into the fragmentainer it
    /// starts in (015 MC-2): its clearance is not truncated at a break.
    pub clears: bool,
    /// `break-inside: avoid | avoid-column | avoid-page`.
    pub avoid_inside: bool,
    pub break_before_forced: bool,
    pub break_after_forced: bool,
    /// `break-before` / `break-after: avoid | avoid-column` (FRAG-2c: the break moves before the previous
    /// sibling when it would fall between the two).
    pub break_before_avoid: bool,
    pub break_after_avoid: bool,
    /// `orphans` / `widows` of the box's lines (inherited; initial 2).
    pub orphans: u16,
    pub widows: u16,
    /// The side a forced page break before / after the box asks for (015 PAGE-1).
    pub before_side: PageSide,
    pub after_side: PageSide,
    /// The used `page` value (CSS Paged Media 3 section 6.1: `auto` takes the parent's): the interned name.
    pub page: Option<u32>,
}

impl FragFacts {
    /// A float broken across fragmentainers like a block (015 MC-2): not monolithic, not `break-inside: avoid`.
    pub(crate) fn breakable_float(&self) -> bool {
        self.float && !self.monolithic && !self.avoid_inside
    }
}

/// The role of a host node in a fragmentation root.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum FragRole {
    #[default]
    None,
    /// A multicol container (`multicol.rs`): its in-flow children are its rows' flow threads `flows` and
    /// its spanners.
    Multicol {
        spec: MulticolSpec,
        gap: GapSpec,
        flows: Vec<usize>,
    },
    /// The anonymous flow-thread box: its layout is the fragmentation root.
    FlowThread,
}

/// `column-gap` of a multicol container: `normal` is 1em (CSS Multicol 1 section 4.1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GapSpec {
    Px(f32),
    /// A fraction of the content width.
    Percent(f32),
    Calc(std::sync::Arc<crate::layout::coordinator::calc::CalcExpr>),
}

impl GapSpec {
    pub(crate) fn resolve(&self, content_width: f32) -> f32 {
        match self {
            GapSpec::Px(p) => *p,
            GapSpec::Percent(p) => p * content_width,
            GapSpec::Calc(e) => e.eval(content_width as f64) as f32,
        }
        .max(0.0)
    }
}

/// The geometry a flow thread was laid out with and its result, kept on the flow thread's host node for
/// the fragment builder.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FlowThreadGeometry {
    /// Fragmentainer block size (the multicol container's content height).
    pub block_size: f32,
    /// Column (flow thread) width and the gap between columns.
    pub column_width: f32,
    pub gap: f32,
    /// The tokens of the last pass.
    pub tokens: Vec<BreakToken>,
    /// Layout passes the root took (the last layout; balancing runs several).
    pub passes: usize,
    /// Used column count of the container (MC-1).
    pub count: u32,
    /// The container's definite content height, if any (the cap of the column height).
    pub avail: Option<f32>,
    /// `column-fill: auto`.
    pub fill_auto: bool,
    /// The last row of the container (the only one `column-fill: auto` fills).
    pub last_row: bool,
    /// The row's top in the container's content box (where Taffy last placed the flow thread).
    pub row_y: f32,
    /// The container's padding and border above its content box.
    pub content_top: f32,
    /// The height of the laid-out strip (the flow thread's own layout reports the column height).
    pub flow_height: f32,
    /// Layouts the balancing of the row took (1 when not balanced).
    pub balance_iterations: u32,
    /// Pages (015 PAGE-1): the page area heights, the last repeating (`block_size` is the first's).
    pub bands: Bands,
    /// Pages: the pages left blank by `left` / `right` breaks and the start of each page's content, from the
    /// last pass (`FragContext::blank`, `FragContext::starts`).
    pub blank: Vec<u32>,
    pub starts: Vec<(f32, Option<u32>, usize)>,
}

impl FlowThreadGeometry {
    /// The fragmentainers holding a flow thread `height` tall (at least one).
    pub fn fragmentainers(&self, height: f32) -> Vec<Fragmentainer> {
        let h = self.block_size.max(EPS);
        if self.bands.sizes.is_none() {
            let n = ((height - EPS) / h).ceil().max(1.0) as u32;
            return (0..n)
                .map(|i| Fragmentainer {
                    index: i,
                    block_offset: i as f32 * h,
                    block_size: h,
                })
                .collect();
        }
        let (last, _, _) = self.bands.locate((height - EPS).max(0.0), h);
        (0..=last)
            .map(|i| Fragmentainer {
                index: i,
                block_offset: self.bands.top_of(i, h),
                block_size: self.bands.size_of(i, h),
            })
            .collect()
    }
}

/// The last final layout of a node inside a fragmentation root that was entirely inside one fragmentainer
/// (015 MC-2, balancing cost): such a layout is the node's unfragmented layout, whatever its offset, so a later
/// pass or balancing iteration that lays the node out with the same inputs at an offset where it again stays
/// inside one fragmentainer takes it as is (its descendants still hold that layout: any other final layout of
/// the node drops the entry). Nodes whose subtree has a decision of the root (a carry entry) or a float of the
/// block formatting context are never kept; one laid out at the top of a fragmentainer is taken again only at
/// the top of one.
#[derive(Clone, Debug)]
pub(crate) struct FragCacheEntry {
    pub inputs: taffy::LayoutInput,
    /// Laid out at the top of a fragmentainer (`FragContext::inside_one`).
    pub at_start: bool,
    /// How far the subtree reaches below the node's border-box top (border box, in-flow descendants, lines).
    pub extent: f32,
    /// Balancing's tallest unbreakable unit inside the subtree, and the node's leading unit.
    pub tallest: f32,
    pub first_unit: Option<f32>,
    pub out: taffy::LayoutOutput,
}

/// The root's state when a cacheable node's final layout started (`HostTree::cache_get`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CacheSnap {
    pub node: usize,
    pub ok: bool,
    pub abs: f32,
    /// `inside_one` of the node's offset when its layout started.
    pub at_start: bool,
    pub tokens: usize,
    pub forced_marks: usize,
    pub tallest: f32,
    pub violated: bool,
    pub orphans_violated: bool,
}

/// What one layout pass of a root hands the next (`FragContext::carry`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Carry {
    /// Balancing measures the tallest unit and the shortages (MC-1).
    pub measure: bool,
    pub corrections: HashMap<usize, (f32, f32)>,
    pub avoid_overflow: HashSet<usize>,
    /// Boxes pushed whole, with the flow offset the decision was taken at: it holds in a later pass only
    /// while the box starts there (a decision taken on a layout that a correction changed is taken again).
    pub pushed_whole: HashMap<usize, f32>,
    /// Lines that start a new fragmentainer (an inline root's content-box offsets), moved there to keep
    /// `widows` lines after a break (FRAG-2c), with the content-box top they were decided at.
    pub line_breaks: HashMap<usize, (f32, Vec<f32>)>,
    /// Balancing: per box pushed whole because its first content (or its first `orphans` lines) did not
    /// fit, what that content lacked.
    pub lead_lack: HashMap<usize, f32>,
    /// Breakable floats (015 MC-2): the flow offset of the border-box top their placement gave.
    pub float_at: HashMap<usize, f32>,
    /// Flex and grid items (015 STAB-1): what breaks inside each added to its block size.
    pub grow: HashMap<usize, f32>,
}

/// The state of the fragmentation root being laid out (`HostTree::frag`).
#[derive(Debug)]
pub(crate) struct FragContext {
    /// The flow thread's host node.
    pub root: usize,
    /// Fragmentainer block size (every fragmentainer of the entry point is as tall; with `bands`, the first's).
    pub block_size: f32,
    /// Fragmentainers of different sizes (015 PAGE-1).
    pub bands: Bands,
    /// A page context (015 PAGE-1): page breaks are forced, headers repeat; `None` for columns.
    pub paged: bool,
    /// Pages left blank by a `left` / `right` break (015 PAGE-1), and the flow offsets where boxes start with
    /// their used page name (the name of a page is that of the first box starting on it).
    pub blank: Vec<u32>,
    pub starts: Vec<(f32, Option<u32>, usize)>,
    /// Border-box top of each node being laid out, in flow-thread coordinates.
    pub abs: HashMap<usize, f32>,
    /// The (uncorrected) estimate Taffy gave at `begin_block_child`, per child.
    est: HashMap<usize, f32>,
    /// Offset corrections from the previous pass, per child: (Taffy's estimate, the final offset in the
    /// parent). A correction holds only while the estimate is the same (an earlier sibling that moved
    /// moves the estimate too: the child is then laid out at the new estimate and corrected again).
    pub corrections: HashMap<usize, (f32, f32)>,
    /// The corrections this pass found.
    pub next_corrections: HashMap<usize, (f32, f32)>,
    /// `break-inside: avoid` boxes taller than a fragmentainer: they are fragmented after all.
    pub avoid_overflow: HashSet<usize>,
    /// Boxes pushed whole to the next fragmentainer because their first content was (kept across passes).
    pub pushed_whole: HashMap<usize, f32>,
    /// Boxes whose first content (a line or a child) was moved past a break in this pass.
    pub leading_pushed: HashSet<usize>,
    /// Parents whose last ended child asked for a forced break after it.
    pending_after: HashMap<usize, PageSide>,
    /// Per parent, its last placed in-flow child, that child's facts and its flow offset (FRAG-2c:
    /// `break-before|after: avoid` move a break before the previous sibling).
    pub(crate) last_child: HashMap<usize, (usize, FragFacts, f32)>,
    /// Lines that start a new fragmentainer (see `Carry::line_breaks`).
    pub line_breaks: HashMap<usize, (f32, Vec<f32>)>,
    /// See `Carry::lead_lack`.
    pub lead_lack: HashMap<usize, f32>,
    /// Breakable floats (015 MC-2): the flow offset each was laid out at in this pass (from the previous
    /// pass's placement), and where this pass placed them. A float is laid out with the context (struts in
    /// its content) only at a known offset; one first met, or placed elsewhere than assumed while it
    /// crosses a boundary, makes the root stale.
    pub float_at: HashMap<usize, f32>,
    pub next_float_at: HashMap<usize, f32>,
    /// Inline roots laid out in this pass: the flow offset of their content-box top (floats in their lines).
    pub ifc_base: HashMap<usize, f32>,
    /// Flex and grid items (015 STAB-1, `flexgrid.rs`): growth from the previous pass, the growth this pass
    /// measured, the flow offset of the run each item lies in, and the item whose final layout is running without
    /// the item wrapper (`HostTree::dispatch`).
    pub grow: HashMap<usize, f32>,
    pub next_grow: HashMap<usize, f32>,
    pub run_at: HashMap<usize, f32>,
    pub item_bypass: Option<usize>,
    /// Final layouts in progress that may be kept (`FragCacheEntry`), innermost last.
    pub cache_stack: Vec<CacheSnap>,
    /// Final layouts taken from `FragCacheEntry`s (diagnostics).
    pub cache_hits: u32,
    /// A node was laid out at an offset that turned out wrong: the root is laid out again.
    pub stale: bool,
    /// A break of this pass violates `orphans` / `widows` (no better one existed).
    pub violated: bool,
    /// ... `orphans` in particular, or an avoided break fell between two boxes: balancing (MC-1) takes a taller
    /// column (the reference's behaviour; a `widows` violation is accepted).
    pub orphans_violated: bool,
    pub tokens: Vec<BreakToken>,
    /// Balancing (MC-1): the tallest unbreakable unit seen and the minimum shortage of a pushed unit.
    pub measure: bool,
    pub tallest: f32,
    pub shortage: Option<f32>,
    /// The measuring layout: flow offsets where forced breaks would start a fragmentainer.
    pub forced_marks: Vec<f32>,
    /// Balancing (015 MC-2): per box, its leading unbreakable unit (from its border-box top to the end of its
    /// first line or first monolithic box): a box's block-start border and padding, and the margins between it
    /// and its first content, are not broken from that content (measured).
    pub first_unit: HashMap<usize, f32>,
    /// Per parent, its last placed in-flow child when that child is monolithic: (top, bottom) in the flow (015
    /// STAB-1: in columns, content after monolithic overflow starts in the next column, measured).
    pub mono_last: HashMap<usize, (f32, f32)>,
}

impl FragContext {
    pub(crate) fn new(root: usize, block_size: f32, carry: Carry) -> Self {
        let mut abs = HashMap::new();
        abs.insert(root, 0.0);
        FragContext {
            root,
            block_size: block_size.max(EPS),
            bands: Bands::default(),
            paged: false,
            blank: Vec::new(),
            starts: Vec::new(),
            abs,
            est: HashMap::new(),
            next_corrections: carry.corrections.clone(),
            corrections: carry.corrections,
            avoid_overflow: carry.avoid_overflow,
            pushed_whole: carry.pushed_whole,
            leading_pushed: HashSet::new(),
            pending_after: HashMap::new(),
            last_child: HashMap::new(),
            line_breaks: carry.line_breaks,
            lead_lack: carry.lead_lack,
            next_float_at: carry.float_at.clone(),
            float_at: carry.float_at,
            ifc_base: HashMap::new(),
            next_grow: carry.grow.clone(),
            grow: carry.grow,
            run_at: HashMap::new(),
            item_bypass: None,
            cache_stack: Vec::new(),
            cache_hits: 0,
            stale: false,
            violated: false,
            orphans_violated: false,
            tokens: Vec::new(),
            measure: carry.measure,
            tallest: 0.0,
            shortage: None,
            forced_marks: Vec::new(),
            first_unit: HashMap::new(),
            mono_last: HashMap::new(),
        }
    }

    /// Whether a decision of the root (this pass's or carried) concerns a node in `range` (a subtree). Offset
    /// corrections do not count: they only say where a box is, and a layout inside one fragmentainer does not
    /// depend on where that is.
    pub(crate) fn decided_in(&self, range: std::ops::Range<usize>) -> bool {
        let r = |k: &usize| range.contains(k);
        self.avoid_overflow.iter().any(r)
            || self.pushed_whole.keys().any(r)
            || self.line_breaks.keys().any(r)
            || self.lead_lack.keys().any(r)
            || self.float_at.keys().any(r)
            || self.next_float_at.keys().any(r)
            || self.grow.keys().any(r)
            || self.next_grow.keys().any(r)
            || self.leading_pushed.iter().any(r)
            || self.pending_after.keys().any(r)
    }

    /// Where `[a, a + extent]` lies: inside one fragmentainer, below its top (`Some(false)`) or at the top of
    /// one that is not the first (`Some(true)`: its first margins are truncated there, the same way in every
    /// such fragmentainer); `None` when it crosses a boundary or starts the flow.
    pub(crate) fn inside_one(&self, a: f32, extent: f32) -> Option<bool> {
        let (_, start, size) = self.bands.locate(a, self.block_size);
        if a <= EPS || a + extent >= start + size - EPS {
            return None;
        }
        Some(a <= start + EPS)
    }

    /// The measuring layout of balancing: one fragmentainer of unbounded height.
    pub(crate) fn unbounded(&self) -> bool {
        self.block_size >= UNBOUNDED
    }

    /// A unit `height` tall that started at `top` was pushed to `boundary` (balancing's minimum shortage).
    pub(crate) fn note_shortage(&mut self, top: f32, height: f32, boundary: f32) {
        self.note_lack(top + height - boundary);
    }

    /// A unit lacked `s` px of column height (balancing's minimum shortage).
    pub(crate) fn note_lack(&mut self, s: f32) {
        if s > EPS {
            self.shortage = Some(self.shortage.map_or(s, |x| x.min(s)));
        }
    }

    /// The top of the fragmentainer that holds flow offset `y`.
    pub(crate) fn fragmentainer_start(&self, y: f32) -> f32 {
        if self.bands.sizes.is_none() {
            return ((y + EPS) / self.block_size).floor() * self.block_size;
        }
        self.bands.locate(y, self.block_size).1
    }

    /// Whether `y` lies in the virtual leading band of a width run (015 PAGE-1, `Bands::lead`).
    pub(crate) fn in_lead(&self, y: f32) -> bool {
        self.bands.lead && y < self.bands.size_of(0, self.block_size) - EPS
    }

    /// The size of the fragmentainer holding `y` (015 PAGE-1: pages of different heights).
    pub(crate) fn size_at(&self, y: f32) -> f32 {
        self.bands.locate(y, self.block_size).2
    }

    /// The index of the fragmentainer holding `y`.
    pub(crate) fn index_at(&self, y: f32) -> u32 {
        self.bands.locate(y, self.block_size).0
    }

    /// The first fragmentainer boundary strictly below `y`.
    pub(crate) fn next_boundary(&self, y: f32) -> f32 {
        let (_, start, size) = self.bands.locate(y, self.block_size);
        start + size
    }

    /// `y` is (within a layout unit) the top of a fragmentainer.
    pub(crate) fn at_fragmentainer_start(&self, y: f32) -> bool {
        y <= self.fragmentainer_start(y) + EPS
    }

    /// The decisions the next pass starts from.
    pub(crate) fn carry(self) -> Carry {
        Carry {
            measure: self.measure,
            corrections: self.next_corrections,
            avoid_overflow: self.avoid_overflow,
            pushed_whole: self.pushed_whole,
            line_breaks: self.line_breaks,
            lead_lack: self.lead_lack,
            float_at: self.next_float_at,
            grow: self.next_grow,
        }
    }

    /// A breakable float (015 MC-2) was placed with its border-box top at flow offset `y`, `height` tall. It was
    /// laid out at the offset assumed for it (`float_at`), or unfragmented when none was: when that is not where
    /// it went and it would cross a fragmentainer boundary there, the next pass lays it out at `y`.
    pub(crate) fn note_float(&mut self, node: usize, y: f32, height: f32) {
        let assumed = self.float_at.get(&node).copied();
        if assumed.is_some_and(|a| (a - y).abs() <= EPS) {
            return;
        }
        let crosses = !self.unbounded() && y + height > self.next_boundary(y) + EPS;
        if assumed.is_none() && !crosses {
            return;
        }
        self.next_float_at.insert(node, y);
        self.stale = true;
    }

    /// Taffy is about to lay `child` out at `estimated_y` in `parent`'s border box.
    pub(crate) fn begin_child(&mut self, parent: usize, child: usize, estimated_y: f32) {
        let Some(&p) = self.abs.get(&parent) else {
            return;
        };
        let y = match self.corrections.get(&child) {
            Some(&(est, fin)) if (est - estimated_y).abs() <= EPS => fin,
            _ => estimated_y,
        };
        self.est.insert(child, estimated_y);
        self.abs.insert(child, p + y);
    }
}
