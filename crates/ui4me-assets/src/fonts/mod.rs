//! owner: 007-resources — created by 001 slice (R-28)
//!
//! Font sources (U4C-RES; 007 `contracts/resource-view.md` §5). Registered blobs, the pinned
//! fonts-only mode (R-05), the embedded last-resort font (OD-24), the single generic-family list
//! (XR-046), per-document `@font-face` faces and the runtime-wide font epoch. No Fontique or
//! Parley type appears here (AD-33); 009 builds its collection from the view.

pub mod faces;
pub mod generic;
pub mod last_resort;
pub mod pinned;
pub mod registry;
pub mod validate;
pub mod woff2;

pub use faces::{
    FaceId, FaceSet, FaceSettings, FaceSourceSpec, FaceState, FaceStyle, FaceView, FontFaceRule,
};
pub use last_resort::{LastResortInfo, LastResortStatus};
pub use pinned::PinnedFontConfig;
pub use registry::{
    FontBlob, FontBlobId, FontConfigReport, FontEpoch, FontMode, FontRegistry, FontSourcesView,
};
