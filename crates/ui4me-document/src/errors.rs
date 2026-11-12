//! owner: 003-document-model
//!
//! Spec Table B and the fixed error precedence (T011; FR-021, FR-049, FR-095; AD-34; R-15).
//!
//! Precedence, identical in every host (and equal to 005's precondition order):
//! `WRONG_THREAD` (the ABI layer's, before DocumentCore is reached) -> `STALE_HANDLE` (handle
//! arguments in parameter order) -> `WRONG_DOCUMENT` -> `DOCUMENT_CLOSED` -> DOM validation
//! order -> `SIZE_LIMIT` / `OUT_OF_MEMORY`. Read-only cross-document comparisons are exempt from
//! `WRONG_DOCUMENT` (spec Clarifications Q2, Q3).

use crate::document::{Document, err};
use ui4me_engine_types::codes_generated::{Code, SubReason};
use ui4me_engine_types::ids::{NodeId, NodeKey};
use ui4me_engine_types::status::Status;

// ---------------------------------------------------------------- Table B constructors

/// `HierarchyRequestError` -> `INVALID_HIERARCHY`.
pub fn hierarchy_request() -> Status {
    err(Code::InvalidHierarchy)
}
/// `NotFoundError` (reference or removed child is not a child) -> `INVALID_HIERARCHY / NOT_A_CHILD`.
pub fn not_found() -> Status {
    err(Code::InvalidHierarchy).with_sub_reason(SubReason::NotAChild)
}
/// `NoModificationAllowedError` -> `INVALID_HIERARCHY / NO_MODIFICATION`.
pub fn no_modification_allowed() -> Status {
    err(Code::InvalidHierarchy).with_sub_reason(SubReason::NoModification)
}
/// `InvalidCharacterError` -> `INVALID_CHARACTER`.
pub fn invalid_character() -> Status {
    err(Code::InvalidCharacter)
}
/// `NamespaceError` -> `INVALID_NAMESPACE`.
pub fn namespace_error() -> Status {
    err(Code::InvalidNamespace)
}
/// `SyntaxError` (selector, empty token, adjacent position) -> `INVALID_ARGUMENT / SYNTAX`.
pub fn syntax_error() -> Status {
    err(Code::InvalidArgument).with_sub_reason(SubReason::Syntax)
}
/// Valid selector using a construct the CSS profile excludes -> `INVALID_ARGUMENT / PROFILE_EXCLUDED`.
pub fn profile_excluded() -> Status {
    err(Code::InvalidArgument).with_sub_reason(SubReason::ProfileExcluded)
}
/// `IndexSizeError` -> `INVALID_ARGUMENT / INDEX_SIZE`.
pub fn index_size() -> Status {
    err(Code::InvalidArgument).with_sub_reason(SubReason::IndexSize)
}
/// Offset would split a surrogate pair (FR-046) -> `INVALID_ARGUMENT / SURROGATE_SPLIT`.
pub fn surrogate_split() -> Status {
    err(Code::InvalidArgument).with_sub_reason(SubReason::SurrogateSplit)
}
/// Operation not applicable to the node kind -> `INVALID_ARGUMENT / WRONG_NODE_KIND`.
pub fn wrong_node_kind() -> Status {
    err(Code::InvalidArgument).with_sub_reason(SubReason::WrongNodeKind)
}
/// `WrongDocumentError`, implicit adoption, `adoptNode` -> `WRONG_DOCUMENT`.
pub fn wrong_document() -> Status {
    err(Code::WrongDocument)
}
/// `InvalidStateError` -> `INVALID_STATE`.
pub fn invalid_state() -> Status {
    err(Code::InvalidState)
}
/// Limit exceeded -> `SIZE_LIMIT`, naming the limit.
pub fn size_limit(limit: &str) -> Status {
    err(Code::SizeLimit).with_message(format!("limit exceeded: {limit}"))
}
/// Allocation failure -> `OUT_OF_MEMORY`.
pub fn out_of_memory() -> Status {
    err(Code::OutOfMemory)
}

/// Every Table B row as `(condition, code, sub_reason)` for the unit tests and 004's registry check.
pub fn table_b() -> Vec<(&'static str, Status)> {
    vec![
        ("HierarchyRequestError", hierarchy_request()),
        ("NotFoundError", not_found()),
        ("NoModificationAllowedError", no_modification_allowed()),
        ("InvalidCharacterError", invalid_character()),
        ("NamespaceError", namespace_error()),
        ("SyntaxError", syntax_error()),
        ("ProfileExcluded", profile_excluded()),
        ("IndexSizeError", index_size()),
        ("SurrogateSplit", surrogate_split()),
        ("WrongNodeKind", wrong_node_kind()),
        ("WrongDocumentError", wrong_document()),
        ("InvalidStateError", invalid_state()),
        ("LimitExceeded", size_limit("nodes")),
    ]
}

// ---------------------------------------------------------------- precedence

/// The ordered precondition levels that precede DOM validation (FR-021). `Ord` is the precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    WrongThread,
    StaleHandle,
    WrongDocument,
    DocumentClosed,
    Validation,
    Limit,
}

impl Level {
    pub fn code(self) -> Option<Code> {
        match self {
            Level::WrongThread => Some(Code::WrongThread),
            Level::StaleHandle => Some(Code::StaleHandle),
            Level::WrongDocument => Some(Code::WrongDocument),
            Level::DocumentClosed => Some(Code::DocumentClosed),
            Level::Validation => None,
            Level::Limit => Some(Code::SizeLimit),
        }
    }
}

/// Which precondition level a status belongs to (for tests and for batch error mapping).
pub fn level_of(s: &Status) -> Level {
    match s.code {
        Code::WrongThread => Level::WrongThread,
        Code::StaleHandle => Level::StaleHandle,
        Code::WrongDocument => Level::WrongDocument,
        Code::DocumentClosed => Level::DocumentClosed,
        Code::SizeLimit | Code::OutOfMemory => Level::Limit,
        _ => Level::Validation,
    }
}

/// Collects the handle arguments of one call and applies the fixed precedence.
///
/// ```ignore
/// let ids = Precheck::new(doc).node(a).key(b).read_only(false).finish()?;
/// ```
pub struct Precheck<'d> {
    doc: &'d Document,
    stale: Option<Status>,
    wrong_document: bool,
    ids: Vec<NodeId>,
    read_only_cross_document: bool,
}

impl<'d> Precheck<'d> {
    pub fn new(doc: &'d Document) -> Self {
        Precheck {
            doc,
            stale: None,
            wrong_document: false,
            ids: vec![],
            read_only_cross_document: false,
        }
    }

    /// A handle argument of this document (parameter order matters: the first stale one wins).
    pub fn node(mut self, n: NodeId) -> Self {
        if !self.doc.arena.contains(n) && self.stale.is_none() {
            self.stale = Some(err(Code::StaleHandle));
        }
        self.ids.push(n);
        self
    }

    /// A handle argument that carries its document (boundary form). A foreign handle is
    /// `WRONG_DOCUMENT` after every own-document staleness check, in parameter order.
    pub fn key(mut self, k: NodeKey) -> Self {
        if k.document == self.doc.id {
            return self.node(k.node);
        }
        self.wrong_document = true;
        self
    }

    /// Read-only cross-document comparisons (`contains`, `compareDocumentPosition`,
    /// `isEqualNode`, `isSameNode`) return the DOM result instead of `WRONG_DOCUMENT`.
    pub fn read_only(mut self, yes: bool) -> Self {
        self.read_only_cross_document = yes;
        self
    }

    /// The first failing level in precedence order; otherwise the own-document ids.
    pub fn finish(self) -> Result<Vec<NodeId>, Status> {
        if self.doc.lifecycle() == crate::document::Lifecycle::Closed && !self.wrong_document {
            return Err(err(Code::DocumentClosed));
        }
        if let Some(s) = self.stale {
            return Err(s);
        }
        if self.wrong_document && !self.read_only_cross_document {
            return Err(wrong_document());
        }
        if self.doc.is_closing() {
            return Err(err(Code::DocumentClosed));
        }
        Ok(self.ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui4me_engine_types::ids::{DocumentId, RawId};

    fn other_doc_key(doc: &Document) -> NodeKey {
        let other = DocumentId::from_raw(RawId {
            index: 7,
            generation: std::num::NonZeroU32::MIN,
        });
        NodeKey {
            document: other,
            node: doc.document_node(),
        }
    }

    #[test]
    fn table_b_rows_map_to_their_codes_and_sub_reasons() {
        let expect: Vec<(Code, Option<SubReason>)> = vec![
            (Code::InvalidHierarchy, None),
            (Code::InvalidHierarchy, Some(SubReason::NotAChild)),
            (Code::InvalidHierarchy, Some(SubReason::NoModification)),
            (Code::InvalidCharacter, None),
            (Code::InvalidNamespace, None),
            (Code::InvalidArgument, Some(SubReason::Syntax)),
            (Code::InvalidArgument, Some(SubReason::ProfileExcluded)),
            (Code::InvalidArgument, Some(SubReason::IndexSize)),
            (Code::InvalidArgument, Some(SubReason::SurrogateSplit)),
            (Code::InvalidArgument, Some(SubReason::WrongNodeKind)),
            (Code::WrongDocument, None),
            (Code::InvalidState, None),
            (Code::SizeLimit, None),
        ];
        let rows = table_b();
        assert_eq!(rows.len(), expect.len());
        for ((name, s), (code, sub)) in rows.iter().zip(expect) {
            assert_eq!((s.code, s.sub_reason), (code, sub), "{name}");
            assert_eq!(s.stage, ui4me_engine_types::stage::Stage::Host, "{name}");
        }
        assert!(
            size_limit("nodes").message.contains("nodes"),
            "the limit is named"
        );
        assert_eq!(out_of_memory().code, Code::OutOfMemory);
    }

    #[test]
    fn levels_are_ordered_by_precedence() {
        let order = [
            Level::WrongThread,
            Level::StaleHandle,
            Level::WrongDocument,
            Level::DocumentClosed,
            Level::Validation,
            Level::Limit,
        ];
        for (i, a) in order.iter().enumerate() {
            for b in &order[i + 1..] {
                assert!(a < b, "{a:?} must outrank {b:?}");
            }
        }
    }

    /// Every ordered pair of the levels `Precheck` decides (stale, wrong document, closed): the
    /// earlier level wins when both apply.
    #[test]
    fn precheck_returns_the_earliest_applicable_level_for_every_pair() {
        let mut doc = Document::parse_html("<p>x</p>").unwrap();
        let live = doc.document_element().unwrap();
        let stale = {
            let t = doc.create_text_node("t").unwrap();
            doc.collect(); // detached and unreferenced: freed
            t
        };
        let foreign = other_doc_key(&doc);
        // stale beats wrong document
        let r = Precheck::new(&doc).key(foreign).node(stale).finish();
        assert_eq!(r.unwrap_err().code, Code::StaleHandle);
        // wrong document alone
        let r = Precheck::new(&doc).key(foreign).node(live).finish();
        assert_eq!(r.unwrap_err().code, Code::WrongDocument);
        // closed alone: while `Closing` the nodes still exist
        doc.request_close(true);
        let r = Precheck::new(&doc).node(live).finish();
        assert_eq!(r.unwrap_err().code, Code::DocumentClosed);
        // stale beats closed; wrong document beats closed
        assert_eq!(
            Precheck::new(&doc).node(stale).finish().unwrap_err().code,
            Code::StaleHandle
        );
        assert_eq!(
            Precheck::new(&doc).key(foreign).finish().unwrap_err().code,
            Code::WrongDocument
        );
        // read-only comparison across documents is exempt from WRONG_DOCUMENT (but not from closed)
        let doc2 = Document::parse_html("<p>y</p>").unwrap();
        let ok = Precheck::new(&doc2).key(foreign).read_only(true).finish();
        assert!(ok.is_ok());
        // once `Closed`, every handle of the document is DOCUMENT_CLOSED, stale or not
        doc.outermost_commit();
        assert_eq!(doc.lifecycle(), crate::document::Lifecycle::Closed);
        assert_eq!(
            Precheck::new(&doc).node(stale).finish().unwrap_err().code,
            Code::DocumentClosed
        );
        assert_eq!(
            Precheck::new(&doc).node(live).finish().unwrap_err().code,
            Code::DocumentClosed
        );
    }

    #[test]
    fn first_stale_handle_in_parameter_order_wins_and_codes_do_not_depend_on_later_arguments() {
        let mut doc = Document::parse_html("<p>x</p>").unwrap();
        let a = doc.create_text_node("a").unwrap();
        let b = doc.create_text_node("b").unwrap();
        doc.collect();
        let live = doc.document_element().unwrap();
        assert_eq!(
            Precheck::new(&doc)
                .node(live)
                .node(a)
                .node(b)
                .finish()
                .unwrap_err()
                .code,
            Code::StaleHandle
        );
        assert_eq!(
            Precheck::new(&doc)
                .node(a)
                .node(live)
                .finish()
                .unwrap_err()
                .code,
            Code::StaleHandle
        );
    }

    #[test]
    fn level_of_classifies_statuses() {
        assert_eq!(level_of(&err(Code::StaleHandle)), Level::StaleHandle);
        assert_eq!(level_of(&wrong_document()), Level::WrongDocument);
        assert_eq!(level_of(&err(Code::DocumentClosed)), Level::DocumentClosed);
        assert_eq!(level_of(&hierarchy_request()), Level::Validation);
        assert_eq!(level_of(&size_limit("x")), Level::Limit);
        assert_eq!(level_of(&out_of_memory()), Level::Limit);
        assert_eq!(Level::Validation.code(), None);
    }
}
