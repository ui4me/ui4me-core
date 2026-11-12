//! owner: 015-core-completion (FRAG-2b, lane L-FRAG) — slicing a laid-out flow thread into
//! fragmentainers (plan §A3 "Slicing").
//!
//! The fragment builder (`fragment/builder.rs`) builds a flow thread once per fragmentainer: a
//! `Fragmentainer` fragment, then the flow thread's piece for that band, then, recursively, its children:
//!
//! * a child entirely inside the band is built whole (its subtree is not looked at again);
//! * a child that may be split (a block container in the flow, not monolithic) and crosses the band's
//!   edges becomes a **piece**: a box fragment as tall as its part inside the band, with
//!   [`crate::layout::fragment::FragmentPiece`] (index, last, block offset in the whole box), the margins, borders
//!   and padding of a sliced edge dropped (`box-decoration-break: slice`), and its own children sliced the
//!   same way;
//! * a child that may not be split belongs to the band its top is in (monolithic overflow paints past
//!   the band; struts make sure it starts in one);
//! * a line box belongs to the band its top is in (line struts move every line to start in one).
//!
//! Out-of-flow boxes whose containing block is split go with its first piece (split abspos is deferred).

use super::EPS;

/// The part of a box that one piece covers, in the box's own border-box coordinates (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PieceReq {
    pub top: f32,
    pub bottom: f32,
    /// The band is the last one: content below it (overflow) belongs to it too.
    pub last_band: bool,
}

/// What a child of a piece becomes in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ChildSlice {
    /// Not in this piece.
    Skip,
    /// Built whole.
    Whole,
    /// Built as a piece covering this part of it (its own border-box coordinates).
    Piece(PieceReq),
}

/// Decide the fate of a child spanning `[top, top + height)` (the parent's border-box coordinates) in a
/// parent piece `p`. `splittable`: the child is a block container in the flow that is not monolithic.
pub(crate) fn child_slice(p: PieceReq, top: f32, height: f32, splittable: bool) -> ChildSlice {
    let bottom = top + height;
    let starts_here = top >= p.top - EPS && (top < p.bottom - EPS || p.last_band);
    if !splittable {
        return if starts_here {
            ChildSlice::Whole
        } else {
            ChildSlice::Skip
        };
    }
    let inside = top >= p.top - EPS && (bottom <= p.bottom + EPS || p.last_band);
    if inside {
        return ChildSlice::Whole;
    }
    let overlaps = top < p.bottom - EPS && bottom > p.top + EPS;
    if !overlaps {
        // A zero-height box on the band's top edge, or one that starts below the last band.
        return if starts_here {
            ChildSlice::Whole
        } else {
            ChildSlice::Skip
        };
    }
    let piece_top = (p.top - top).max(0.0);
    let piece_bottom = if p.last_band {
        height
    } else {
        (p.bottom - top).min(height)
    };
    ChildSlice::Piece(PieceReq {
        top: piece_top,
        bottom: piece_bottom,
        last_band: p.last_band || bottom <= p.bottom + EPS,
    })
}

/// A line (top `y` in the parent's border-box coordinates) belongs to the piece holding its top.
pub(crate) fn line_in_piece(p: PieceReq, y: f32) -> bool {
    y >= p.top - EPS && (y < p.bottom - EPS || p.last_band)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAND: PieceReq = PieceReq {
        top: 100.0,
        bottom: 200.0,
        last_band: false,
    };

    #[test]
    fn children_are_whole_skipped_or_pieces() {
        assert_eq!(child_slice(BAND, 120.0, 30.0, true), ChildSlice::Whole);
        assert_eq!(child_slice(BAND, 20.0, 30.0, true), ChildSlice::Skip);
        assert_eq!(child_slice(BAND, 220.0, 30.0, true), ChildSlice::Skip);
        assert_eq!(
            child_slice(BAND, 50.0, 100.0, true),
            ChildSlice::Piece(PieceReq {
                top: 50.0,
                bottom: 100.0,
                last_band: true
            })
        );
        assert_eq!(
            child_slice(BAND, 150.0, 100.0, true),
            ChildSlice::Piece(PieceReq {
                top: 0.0,
                bottom: 50.0,
                last_band: false
            })
        );
        // A box over three bands has a middle piece as tall as the band.
        assert_eq!(
            child_slice(BAND, 50.0, 250.0, true),
            ChildSlice::Piece(PieceReq {
                top: 50.0,
                bottom: 150.0,
                last_band: false
            })
        );
    }

    #[test]
    fn monolithic_children_belong_to_the_band_of_their_top() {
        assert_eq!(child_slice(BAND, 150.0, 100.0, false), ChildSlice::Whole);
        assert_eq!(child_slice(BAND, 50.0, 100.0, false), ChildSlice::Skip);
        assert_eq!(
            child_slice(
                PieceReq {
                    last_band: true,
                    ..BAND
                },
                250.0,
                10.0,
                false
            ),
            ChildSlice::Whole
        );
    }

    #[test]
    fn lines_belong_to_the_band_of_their_top() {
        assert!(line_in_piece(BAND, 100.0));
        assert!(!line_in_piece(BAND, 200.0));
        assert!(line_in_piece(
            PieceReq {
                last_band: true,
                ..BAND
            },
            260.0
        ));
    }
}
