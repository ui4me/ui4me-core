//! owner: 008-layout-engine (lane L-LIST-r9, L-LAY2-r12) - CSS counters and quotes (FR-085,
//! FR-086; T113).
//!
//! Counters are evaluated in tree order while the box tree is built, following CSS Lists 3
//! section 4.4: every element has a *counters set* (a list of counters, outermost first). The set
//! of an element is a copy of its parent's set, plus the counters of its preceding sibling's set
//! whose names the parent's set does not hold (a counter an element creates is therefore visible
//! to its later siblings only when no counter of that name is in scope from the parent: a nested
//! `counter-reset` of an enclosing counter's name is private to the element and its descendants).
//! Counters are shared by reference, so an increment made anywhere is seen by every set that holds
//! the counter (the spec's "value source" step). Instantiating a counter replaces the innermost
//! counter of that name when that one was created by the element or a previous sibling (CSS Lists
//! 3 section 4.4.2); otherwise the new counter nests inside it (`counters()` lists both).
//! A reversed counter (`reversed(name)`, `ol reversed`) counts down by default: its implicit
//! list-item increment is -1.
//!
//! Style containment (CSS Containment 2 section 3.3; 015 CQ-2): `counter-increment` and
//! `counter-set` are scoped to the contained element's subtree and create a new counter. Inside a
//! style-contained element (its pseudo-elements and descendants; the element's own counter properties
//! still act outside, as in the reference browser), an increment or set whose nearest counter of that
//! name was created outside instantiates a new counter (value 0) on the box doing it, so nothing inside
//! changes a counter outside; `counter()` and `counters()` still read the counters in scope. Quote
//! depth changes inside the subtree do not leak out (`QuoteState::enter`/`leave`).

use crate::style::view::generated::{ContentItem, Quotes};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
struct CounterData {
    name: String,
    value: i32,
    reversed: bool,
    /// The sibling group (the children of one element) of the element that created the counter.
    group: u64,
    /// Creation order (style-containment boundaries compare against it).
    seq: u64,
}

type Counter = Rc<RefCell<CounterData>>;
type Set = Vec<Counter>;

/// A style-contained element being processed.
#[derive(Debug)]
struct Boundary {
    /// Counters created before this (in creation order) are outside.
    seq: u64,
    /// The element's counters set at entry: what its following siblings inherit.
    set: Set,
    /// The element's index in `elements` and the index of the frame of its children in `frames`.
    element: usize,
    frame: usize,
}

/// The children of one element being built.
#[derive(Debug)]
struct Frame {
    /// The counters set of the element whose children are built.
    parent: Set,
    /// The counters set of the previous sibling, once there is one.
    prev: Option<Set>,
    group: u64,
}

/// The counter sets of the element being built.
#[derive(Debug)]
pub(crate) struct CounterScopes {
    frames: Vec<Frame>,
    /// The counters sets of the elements (or pseudo-elements) being processed, innermost last.
    elements: Vec<Set>,
    next_group: u64,
    next_seq: u64,
    /// For each element being processed: its style-containment boundary (`None` when it is not
    /// style-contained).
    boundaries: Vec<Option<Boundary>>,
    /// How many times a counter value was read and written (the incremental box build, 015 PERF-1).
    pub(crate) reads: u64,
    pub(crate) writes: u64,
}

impl Default for CounterScopes {
    fn default() -> Self {
        CounterScopes {
            frames: vec![Frame {
                parent: Vec::new(),
                prev: None,
                group: 0,
            }],
            elements: Vec::new(),
            next_group: 1,
            next_seq: 0,
            boundaries: Vec::new(),
            reads: 0,
            writes: 0,
        }
    }
}

impl CounterScopes {
    /// Start an element (or a generated box): its counters set is inherited from its parent and
    /// its previous sibling.
    pub fn begin(&mut self) {
        let f = self.frames.last().expect("one frame always exists");
        let mut set = f.parent.clone();
        if let Some(prev) = &f.prev {
            for c in prev {
                let name = c.borrow().name.clone();
                if !set.iter().any(|x| x.borrow().name == name) {
                    set.push(c.clone());
                }
            }
        }
        self.elements.push(set);
    }

    /// Finish the element: its set is what its next sibling inherits.
    pub fn end(&mut self) {
        let set = self.elements.pop().unwrap_or_default();
        if let Some(f) = self.frames.last_mut() {
            f.prev = Some(set);
        }
    }

    /// Open the frame for the children of the element just begun.
    pub fn push(&mut self) {
        let parent = self.elements.last().cloned().unwrap_or_default();
        let group = self.next_group;
        self.next_group += 1;
        self.frames.push(Frame {
            parent,
            prev: None,
            group,
        });
    }

    /// Enter an element: `contained` when it has style containment (module docs).
    pub fn enter_element(&mut self, contained: bool) {
        let entry = contained.then(|| Boundary {
            seq: self.next_seq,
            set: self.current().clone(),
            element: self.elements.len() - 1,
            frame: self.frames.len(),
        });
        self.boundaries.push(entry);
    }

    /// Leave an element (before [`Self::end`]): a style-contained element hands its following
    /// siblings the set it entered with, so no counter it or its subtree created leaks out.
    pub fn leave_element(&mut self) {
        if let Some(Some(b)) = self.boundaries.pop()
            && let Some(cur) = self.elements.last_mut()
        {
            *cur = b.set;
        }
    }

    /// The innermost style-containment boundary in effect.
    fn boundary(&self) -> Option<&Boundary> {
        self.boundaries.iter().rev().find_map(|b| b.as_ref())
    }

    /// Close it: the counters created by the children go out of scope.
    pub fn pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    fn current(&mut self) -> &mut Set {
        if self.elements.is_empty() {
            self.elements.push(Vec::new());
        }
        self.elements.last_mut().expect("just ensured")
    }

    fn find(&mut self, name: &str) -> Option<Counter> {
        self.current()
            .iter()
            .rev()
            .find(|c| c.borrow().name == name)
            .cloned()
    }

    /// Create a counter (instantiate a counter, CSS Lists 3 section 4.4.2).
    pub fn instantiate(&mut self, name: &str, value: i32, reversed: bool) {
        self.writes += 1;
        let group = self.frames.last().map_or(0, |f| f.group);
        let set = self.current();
        if let Some(pos) = set.iter().rposition(|c| c.borrow().name == name)
            && set[pos].borrow().group == group
        {
            set.remove(pos);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let set = self.current();
        set.push(Rc::new(RefCell::new(CounterData {
            name: name.to_string(),
            value,
            reversed,
            group,
            seq,
        })));
    }

    fn ensure(&mut self, name: &str) -> Counter {
        if self.find(name).is_none() {
            self.instantiate(name, 0, false);
        }
        self.find(name).expect("just instantiated")
    }

    /// The counter an increment or set changes: the nearest one, unless it was created outside the
    /// innermost style-containment boundary, in which case a new counter is created here.
    ///
    /// The new counter belongs to the contained element (the reference browser's behaviour: every
    /// box of the subtree after it sees it), so it is added to the sets of that element and of
    /// everything open below it.
    fn ensure_writable(&mut self, name: &str) -> Counter {
        self.writes += 1;
        let found = self.find(name);
        let Some((element, frame)) = self
            .boundary()
            .filter(|b| found.as_ref().is_some_and(|c| c.borrow().seq < b.seq))
            .map(|b| (b.element, b.frame))
        else {
            return self.ensure(name);
        };
        let group = self.frames.get(frame).map_or(0, |f| f.group);
        let seq = self.next_seq;
        self.next_seq += 1;
        let c: Counter = Rc::new(RefCell::new(CounterData {
            name: name.to_string(),
            value: 0,
            reversed: false,
            group,
            seq,
        }));
        for set in self.elements.iter_mut().skip(element) {
            set.push(c.clone());
        }
        for f in self.frames.iter_mut().skip(frame) {
            f.parent.push(c.clone());
        }
        c
    }

    /// `counter-increment`: add `by` to the nearest counter of that name, creating it with 0
    /// when there is none.
    pub fn increment(&mut self, name: &str, by: i32) {
        let c = self.ensure_writable(name);
        let mut c = c.borrow_mut();
        c.value = c.value.saturating_add(by);
    }

    /// The increment of a list item that has none of its own: 1, or -1 on a reversed counter
    /// (CSS Lists 3 section 4.4.3: a reversed counter counts down by default).
    pub fn increment_implicit(&mut self, name: &str) {
        let by = if self.ensure_writable(name).borrow().reversed {
            -1
        } else {
            1
        };
        self.increment(name, by);
    }

    /// `counter-set` for the ordinal value of a list item: set the nearest counter so that the
    /// item's own increments (`by`, or the implicit one) leave it on `value`.
    pub fn set_before_increment(&mut self, name: &str, value: i32, by: Option<i32>) {
        let c = self.ensure_writable(name);
        let mut c = c.borrow_mut();
        let by = by.unwrap_or(if c.reversed { -1 } else { 1 });
        c.value = value.saturating_sub(by);
    }

    /// The value of the nearest counter of that name (created with 0 when there is none, CSS 2.1
    /// section 12.4).
    pub fn value(&mut self, name: &str) -> i32 {
        self.reads += 1;
        self.ensure(name).borrow().value
    }

    /// The values of every counter of that name in scope, outermost first (`counters()`).
    pub fn values(&mut self, name: &str) -> Vec<i32> {
        self.reads += 1;
        self.ensure(name);
        self.current()
            .iter()
            .filter(|c| c.borrow().name == name)
            .map(|c| c.borrow().value)
            .collect()
    }
}

/// The quotation nesting depth (CSS 2.1 section 12.3.1) and the `quotes` pair it selects.
#[derive(Debug, Default)]
pub(crate) struct QuoteState {
    /// Quote keywords used (each reads and moves the depth; the incremental box build, 015 PERF-1).
    pub(crate) reads: u64,
    depth: i32,
    /// The depth at the entry of each element being processed that is style-contained.
    saved: Vec<Option<i32>>,
}

/// Quotation marks of `quotes: auto`. The reference browser (Chromium 156 on Debian 13, measured)
/// uses the same two levels, \u{201C} \u{201D} and \u{2018} \u{2019}, for every content language: the
/// language is accepted so that a locale table can be added without changing callers.
fn auto_pairs(_lang: &str) -> &'static [(&'static str, &'static str)] {
    &[("\u{201C}", "\u{201D}"), ("\u{2018}", "\u{2019}")]
}

impl QuoteState {
    /// Enter an element: `contained` when it has style containment (the depth changes of its
    /// subtree do not leak out, CSS Containment 2 section 3.3).
    pub fn enter(&mut self, contained: bool) {
        self.saved.push(contained.then_some(self.depth));
    }

    pub fn leave(&mut self) {
        if let Some(Some(d)) = self.saved.pop() {
            self.depth = d;
        }
    }

    fn pair(quotes: &Quotes, lang: &str, depth: i32) -> Option<(String, String)> {
        let i = depth.max(0) as usize;
        match quotes {
            Quotes::None => None,
            Quotes::Pairs(p) if p.is_empty() => None,
            Quotes::Pairs(p) => Some(p[i.min(p.len() - 1)].clone()),
            Quotes::Auto => {
                let t = auto_pairs(lang);
                let (o, c) = t[i.min(t.len() - 1)];
                Some((o.to_string(), c.to_string()))
            }
        }
    }

    /// The text of a quote keyword and the depth change it causes (CSS 2.1 section 12.3.1: an
    /// `open-quote` uses the pair of the current depth and then deepens; a `close-quote`
    /// first returns, and uses the pair of the depth it returns to; depth never goes below 0).
    pub fn quote(&mut self, item: &ContentItem, quotes: &Quotes, lang: &str) -> String {
        self.reads += 1;
        match item {
            ContentItem::OpenQuote => {
                let s = Self::pair(quotes, lang, self.depth)
                    .map(|p| p.0)
                    .unwrap_or_default();
                self.depth += 1;
                s
            }
            ContentItem::CloseQuote => {
                if self.depth > 0 {
                    self.depth -= 1;
                    Self::pair(quotes, lang, self.depth)
                        .map(|p| p.1)
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            }
            ContentItem::NoOpenQuote => {
                self.depth += 1;
                String::new()
            }
            ContentItem::NoCloseQuote => {
                if self.depth > 0 {
                    self.depth -= 1;
                }
                String::new()
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_continue_and_children_nest() {
        let mut c = CounterScopes::default();
        // ol { counter-reset: item }
        c.begin();
        c.instantiate("item", 0, false);
        c.push();
        // li { counter-increment: item } x2
        c.begin();
        c.increment("item", 1);
        c.end();
        c.begin();
        c.increment("item", 1);
        assert_eq!(c.value("item"), 2);
        // a nested ol inside the second li
        c.push();
        c.begin();
        c.instantiate("item", 0, false);
        c.push();
        c.begin();
        c.increment("item", 1);
        assert_eq!(c.values("item"), vec![2, 1]);
        c.end();
        c.pop();
        c.end();
        c.pop();
        c.end();
        // a third li: the counter the outer list created stays in scope
        c.begin();
        c.increment("item", 1);
        assert_eq!(c.values("item"), vec![3]);
    }

    #[test]
    fn a_sibling_reset_replaces() {
        let mut c = CounterScopes::default();
        c.begin();
        c.instantiate("n", 5, false);
        c.end();
        c.begin();
        c.instantiate("n", 1, false);
        assert_eq!(c.values("n"), vec![1]);
        c.end();
        // the counter of the last sibling is visible to the following sibling
        c.begin();
        assert_eq!(c.values("n"), vec![1]);
    }

    #[test]
    fn a_nested_reset_of_an_enclosing_name_is_private_to_its_element() {
        let mut c = CounterScopes::default();
        c.begin();
        c.instantiate("c", 0, false); // #test
        c.push();
        c.begin();
        c.increment("c", 1); // span
        c.end();
        c.begin();
        c.instantiate("c", 48, false); // span with counter-reset
        c.increment("c", 1);
        assert_eq!(c.values("c"), vec![1, 49]);
        c.end();
        c.begin(); // the next span sees the enclosing counter only
        c.increment("c", 1);
        assert_eq!(c.values("c"), vec![2]);
    }

    #[test]
    fn reversed_counts_down() {
        let mut c = CounterScopes::default();
        c.begin();
        c.instantiate("list-item", 4, true);
        c.increment_implicit("list-item");
        assert_eq!(c.value("list-item"), 3);
        c.increment_implicit("list-item");
        assert_eq!(c.value("list-item"), 2);
        // an explicit increment is added as written
        c.increment("list-item", -2);
        assert_eq!(c.value("list-item"), 0);
        c.set_before_increment("list-item", 7, None);
        c.increment_implicit("list-item");
        assert_eq!(c.value("list-item"), 7);
    }

    #[test]
    fn quotes_nest_and_clamp() {
        let mut q = QuoteState::default();
        let pairs = Quotes::Pairs(vec![("<".into(), ">".into()), ("[".into(), "]".into())]);
        assert_eq!(q.quote(&ContentItem::OpenQuote, &pairs, "en"), "<");
        assert_eq!(q.quote(&ContentItem::OpenQuote, &pairs, "en"), "[");
        assert_eq!(q.quote(&ContentItem::OpenQuote, &pairs, "en"), "[");
        assert_eq!(q.quote(&ContentItem::CloseQuote, &pairs, "en"), "]");
        assert_eq!(q.quote(&ContentItem::CloseQuote, &pairs, "en"), "]");
        assert_eq!(q.quote(&ContentItem::CloseQuote, &pairs, "en"), ">");
        assert_eq!(q.quote(&ContentItem::CloseQuote, &pairs, "en"), "");
        assert_eq!(
            q.quote(&ContentItem::OpenQuote, &Quotes::Auto, "en"),
            "\u{201C}"
        );
        assert_eq!(q.quote(&ContentItem::OpenQuote, &Quotes::None, "en"), "");
    }
}
