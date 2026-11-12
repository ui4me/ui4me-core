//! owner: 006-style-engine (lane L-LIST-r9) - the generated-content and list properties of a
//! `ComputedStyleView`: `content`, `counter-reset`, `counter-increment`, `quotes` and the
//! `list-style-*` longhands (CSS Lists 3, CSS Content 3, CSS Counter Styles 3), as typed values
//! for the layout stage (008 `box_tree/{pseudo,counters,lists}.rs`). No upstream type appears.

use super::paint_groups::Image;
use style::properties::ComputedValues;
use style::values::computed::Content as StyloContent;
use style::values::generics::counters::ContentItem as StyloItem;

/// A reference to a counter style: `<counter-style-name>`, a `<string>` (list-style-type only),
/// `none`, or a form this engine does not type (`symbols()`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CounterStyleRef {
    None,
    /// Lower-case name (`decimal`, `lower-roman`, `disc`, ...).
    Name(String),
    /// A literal marker string (`list-style-type: "-"`).
    String(String),
    /// `symbols(...)`: not rendered, reported by the consumer.
    Symbols,
}

impl CounterStyleRef {
    fn of(c: &style::counter_style::CounterStyle) -> CounterStyleRef {
        use style::counter_style::CounterStyle as C;
        match c {
            C::None => CounterStyleRef::None,
            C::Name(n) => CounterStyleRef::Name(n.0.to_string().to_ascii_lowercase()),
            C::String(s) => CounterStyleRef::String(s.to_string()),
            C::Symbols { .. } => CounterStyleRef::Symbols,
        }
    }
    /// `decimal`.
    pub fn decimal() -> CounterStyleRef {
        CounterStyleRef::Name("decimal".to_string())
    }
}

/// One item of a `content` value (CSS Content 3 section 2). `attr()` has no item: Stylo substitutes
/// the attribute's value into the declaration when the style is computed (arbitrary substitution
/// functions, like `var()`), so the computed value holds a string.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentItem {
    String(String),
    Counter {
        name: String,
        style: CounterStyleRef,
    },
    Counters {
        name: String,
        separator: String,
        style: CounterStyleRef,
    },
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
    Image(Image),
}

/// Computed `content`.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Content {
    #[default]
    Normal,
    None,
    Items(Vec<ContentItem>),
}

/// One `<counter-name> <integer>` of `counter-reset` / `counter-increment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterOp {
    pub name: String,
    /// The explicit or default integer; `None` for `reversed(name)` without an integer (the
    /// start is the number of items in the counter's scope, CSS Lists 3 section 4.4.1).
    pub value: Option<i32>,
    /// `reversed(name)`.
    pub reversed: bool,
}

/// Computed `quotes`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Quotes {
    /// Language-dependent quotation marks.
    #[default]
    Auto,
    /// `none`, or an empty list: the quote keywords produce nothing but still nest.
    None,
    Pairs(Vec<(String, String)>),
}

/// Computed `list-style-*`.
#[derive(Clone, Debug, PartialEq)]
pub struct ListStyle {
    pub style_type: CounterStyleRef,
    /// `list-style-position: inside`.
    pub inside: bool,
    pub image: Image,
}

/// The generated-content group of one element or pseudo-element.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub content: Content,
    pub counter_reset: Vec<CounterOp>,
    pub counter_increment: Vec<CounterOp>,
    pub quotes: Quotes,
    pub list: ListStyle,
}

impl Generated {
    /// The initial values: no content, no counter operations, `quotes: auto`, `disc` outside.
    pub fn initial() -> Generated {
        Generated {
            content: Content::Normal,
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            quotes: Quotes::Auto,
            list: ListStyle {
                style_type: CounterStyleRef::Name("disc".to_string()),
                inside: false,
                image: Image::None,
            },
        }
    }
}

fn ops(
    list: &[style::values::generics::counters::CounterPair<i32>],
    reset: bool,
) -> Vec<CounterOp> {
    list.iter()
        .map(|p| CounterOp {
            name: p.name.0.to_string(),
            // `i32::MIN` is the parser's marker for `reversed(name)` without an integer.
            value: if reset && p.is_reversed && p.value == i32::MIN {
                None
            } else {
                Some(p.value)
            },
            reversed: p.is_reversed,
        })
        .collect()
}

pub(crate) fn generated(epoch: u64, cv: &ComputedValues) -> Generated {
    let c = cv.get_counters();
    let content = match &c.content {
        StyloContent::Normal => Content::Normal,
        StyloContent::None => Content::None,
        StyloContent::Items(items) => Content::Items(
            items
                .items
                .iter()
                // Past the alt-text marker the items are the accessible text, not the content.
                .take(items.alt_start)
                .map(|i| match i {
                    StyloItem::String(s) => ContentItem::String(s.to_string()),
                    StyloItem::Counter(n, s) => ContentItem::Counter {
                        name: n.0.to_string(),
                        style: CounterStyleRef::of(s),
                    },
                    StyloItem::Counters(n, sep, s) => ContentItem::Counters {
                        name: n.0.to_string(),
                        separator: sep.to_string(),
                        style: CounterStyleRef::of(s),
                    },
                    StyloItem::OpenQuote => ContentItem::OpenQuote,
                    StyloItem::CloseQuote => ContentItem::CloseQuote,
                    StyloItem::NoOpenQuote => ContentItem::NoOpenQuote,
                    StyloItem::NoCloseQuote => ContentItem::NoCloseQuote,
                    StyloItem::Image(img) => {
                        ContentItem::Image(super::paint_groups::image(cv, epoch, img))
                    }
                })
                .collect(),
        ),
    };
    let l = cv.get_list();
    let quotes = match &l.quotes {
        style::values::computed::Quotes::Auto => Quotes::Auto,
        style::values::computed::Quotes::QuoteList(list) if list.0.is_empty() => Quotes::None,
        style::values::computed::Quotes::QuoteList(list) => Quotes::Pairs(
            list.0
                .iter()
                .map(|p| (p.opening.to_string(), p.closing.to_string()))
                .collect(),
        ),
    };
    Generated {
        content,
        counter_reset: ops(&c.counter_reset, true),
        counter_increment: ops(&c.counter_increment, false),
        quotes,
        list: ListStyle {
            style_type: CounterStyleRef::of(&l.list_style_type.0),
            inside: matches!(
                l.list_style_position,
                style::values::computed::ListStylePosition::Inside
            ),
            image: super::paint_groups::image(cv, epoch, &l.list_style_image),
        },
    }
}
