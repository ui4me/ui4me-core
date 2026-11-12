//! owner: 007-resources (T012) — `load_text_sync` for 006 (AD-P-006-a, accepted by R-09).
//!
//! A synchronous owner-thread read through the policy and the guard, for stylesheets (16 MiB by
//! default). It skips the Pending state: `Resolving -> Ready | Denied | Broken`.

use crate::bundles::BundleRegistry;
use crate::fs_guard::{self, FileIdentity};
use crate::policy::{self, Decision, ResourcePolicy};
use crate::uri::{self, ResourceKind, Scheme};
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;
use url::Url;

/// Content identity of a text resource (for the dependency cache of 006).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextIdentity {
    File(FileIdentity),
    Data,
    /// A `ui4me-res:` entry: the bundle's generation (changes when the bundle is replaced).
    Bundle(u64),
}

/// `{final_url, bytes, content_identity}` (`resource-view.md` §2).
#[derive(Clone, Debug)]
pub struct TextResource {
    pub final_url: Url,
    pub bytes: Arc<[u8]>,
    pub content_identity: TextIdentity,
}

/// Resolve `reference` against `base`, apply the policy and read it. Codes follow
/// `resource-policy.md` §3.
pub fn load_text_sync(
    bundles: &BundleRegistry,
    policy: &ResourcePolicy,
    base: &Url,
    reference: &str,
    kind: ResourceKind,
) -> Result<TextResource, Status> {
    let url = uri::resolve(base, reference)?;
    load_text_url(bundles, policy, &url, kind)
}

pub fn load_text_url(
    bundles: &BundleRegistry,
    policy: &ResourcePolicy,
    url: &Url,
    kind: ResourceKind,
) -> Result<TextResource, Status> {
    let denied = |code: Code, msg: &str| Status::new(code, Stage::Resource).with_message(msg);
    match policy::check(url, policy) {
        Decision::Deny(code) => Err(denied(code, "request denied by the resource policy")),
        Decision::Permit(Scheme::File) => {
            let path =
                uri::file_url_to_path_checked(url).map_err(|c| denied(c, "file URL denied"))?;
            let max = policy.max_encoded_bytes.for_kind(kind);
            let vf = fs_guard::open_verified(&path, &policy.file_roots, max)?;
            let bytes = fs_guard::read_limited(&vf.file, max)?;
            Ok(TextResource {
                final_url: uri::without_fragment(url),
                bytes: Arc::from(bytes),
                content_identity: TextIdentity::File(vf.identity),
            })
        }
        Decision::Permit(Scheme::UiRes) => {
            let hit = bundles.lookup(url)?;
            let max = policy.max_encoded_bytes.for_kind(kind);
            if hit.data.len() as u64 > max {
                return Err(denied(
                    Code::SizeLimit,
                    "bundle entry exceeds the encoded-size limit",
                ));
            }
            Ok(TextResource {
                final_url: uri::without_fragment(url),
                bytes: hit.data,
                content_identity: TextIdentity::Bundle(hit.generation),
            })
        }
        Decision::Permit(_) => Err(denied(
            Code::UnsupportedFeature,
            "this scheme is not loadable as text yet (data-url crate not locked)",
        )),
    }
}
