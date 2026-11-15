//! Native facilities whose lifetime is independent of the presentation engine.
//!
//! Only this module chooses the operating-system implementation. The same
//! transport and backend state machines are used on both supported platforms.

pub mod dialogs;
pub mod external;
pub mod preferences;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("ui4me-host 0.1 implements Linux and Windows; this target is not qualified");

/// HOST-14: variables that describe *this* host process (its systemd unit, its launcher and the
/// desktop realm it arbitrates in). A backend never inherits them: a UI4ME application that the
/// backend starts must be supervised, watched and arbitrated as its own instance, not as a part
/// of this one. A manifest cannot set them either (`config.rs`).
pub const HOST_PRIVATE_ENV: [&str; 3] = [
    "UI4ME_SYSTEMD_UNIT",
    "UI4ME_LAUNCHER",
    "UI4ME_DESKTOP_REALM",
];

/// Stable, non-secret namespace hash. This is not an authentication primitive.
pub(crate) fn namespace_hash(value: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_stable_and_distinguish_inputs() {
        assert_eq!(namespace_hash(b"hello"), "a430d84680aabd0b");
        assert_ne!(
            namespace_hash(b"app/session-a"),
            namespace_hash(b"app/session-b")
        );
    }
}
