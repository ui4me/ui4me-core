//! owner: 001-vertical-slice (file); descriptor 006-style-engine — created by 001 slice (R-28)
//!
//! `CssFeatureProfile` descriptor (core-types.md §6; AD-06; R-37). Immutable flag set fixed
//! before the first stylesheet parse. Identifier is 006's `ui4me-css/1`; no local alias.
//! 006 owns the flag table and the digest algorithm (it supplies the digest string).

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CssFeatureProfile {
    version: u32,
    digest: String,
    flags: Vec<&'static str>,
}

impl CssFeatureProfile {
    /// 006's profile identifier.
    pub const ID: &'static str = "ui4me-css/1";

    /// Build an immutable profile; flags are sorted and de-duplicated.
    pub fn new(
        version: u32,
        digest: impl Into<String>,
        flags: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        let mut flags: Vec<_> = flags.into_iter().collect();
        flags.sort_unstable();
        flags.dedup();
        CssFeatureProfile {
            version,
            digest: digest.into(),
            flags,
        }
    }
    pub fn id(&self) -> &'static str {
        Self::ID
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn flags(&self) -> &[&'static str] {
        &self.flags
    }
    pub fn is_enabled(&self, flag: &str) -> bool {
        self.flags.binary_search(&flag).is_ok()
    }
}

/// Dotted CSS feature id under 005's registered roots (`css.property|at-rule|selector|pseudo|
/// value|display|media|unit|function|syntax`, `html`, ...); the registry is `diag::FeatureId`
/// (R-37), so every id validates against 005's root list. Includes `css.display.*` ids for
/// display values that layout rejects (XR-025).
pub type CssFeatureId = crate::diag::FeatureId;

/// Public state of a feature in the profile (css-feature-profile.md section 7). No upstream
/// types appear here (AD-33).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeatureState {
    On,
    Off,
    /// Enabled only when `gate` passes (e.g. `G-STYLE-HAS`).
    Gated(&'static str),
    /// The engine build cannot parse it.
    EngineUnavailable,
}

/// The profile identity published in reports: `{id, version, digest, engine_revision}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDescriptor {
    /// Always [`CssFeatureProfile::ID`] (`ui4me-css/1`); 006 is the identifier owner (R-37).
    pub id: &'static str,
    /// Semantic version, e.g. `1.0.0`.
    pub version: String,
    pub digest: String,
    pub engine_revision: String,
}

impl ProfileDescriptor {
    pub fn new(
        version: impl Into<String>,
        digest: impl Into<String>,
        engine_revision: impl Into<String>,
    ) -> Self {
        ProfileDescriptor {
            id: CssFeatureProfile::ID,
            version: version.into(),
            digest: digest.into(),
            engine_revision: engine_revision.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_sorted_deduplicated() {
        let p = CssFeatureProfile::new(1, "d", ["b", "a", "b"]);
        assert_eq!(p.flags(), &["a", "b"]);
        assert!(p.is_enabled("a") && !p.is_enabled("c"));
        assert_eq!(p.id(), "ui4me-css/1");
    }

    #[test]
    fn ids_validate_against_the_root_list() {
        assert!(CssFeatureId::new("css.display.ruby").is_some());
        assert!(CssFeatureId::new("css.property.backdrop-filter").is_some());
        assert!(CssFeatureId::new("bogus.property.x").is_none());
        let d = ProfileDescriptor::new("1.0.0", "ab", "rev");
        assert_eq!(d.id, "ui4me-css/1");
        assert_eq!(
            FeatureState::Gated("G-STYLE-HAS"),
            FeatureState::Gated("G-STYLE-HAS")
        );
    }
}
