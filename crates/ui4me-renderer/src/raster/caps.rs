//! owner: 010-paint-render — created by 001 slice (R-28)
//!
//! Static capability table (010 `contracts/capability-table.md`; AD-17, AD-18). Slice subset:
//! the Mandatory entries the slice paints (solid rect fill, solid path fill, glyph run).
//! Implemented by 001 T048. The table key digest is SHA-256 over a canonical text form of the
//! key and entries (computed here: `ui4me-raster` has no hashing dependency).

use ui4me_engine_types::diag::CapabilityId;

/// Declared fallback id of a capability (render-receipt.md §1 `degraded[].fallback`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FallbackId(pub &'static str);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Mandatory,
    Optional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Availability {
    Native,
    Owned,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityEntry {
    pub id: CapabilityId,
    pub tier: Tier,
    pub availability: Availability,
    pub fallback: Option<FallbackId>,
}

/// The table selected by (backend revision, threading mode, features, precision).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityTable {
    pub entries: Vec<CapabilityEntry>,
}

/// The table key (capability-table.md §1): the table is a constant per key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableKey {
    pub backend: &'static str,
    /// Exact version from `Cargo.lock` (checked by `tests/caps_table.rs`).
    pub version: &'static str,
    pub threading: &'static str,
    /// Sorted cargo features of the backend crate.
    pub features: &'static [&'static str],
    /// Render mode (u8 pipeline = `optimize-speed`).
    pub precision: &'static str,
    /// Instruction-set level: the deterministic baseline level of the target architecture
    /// (`Level::baseline()`, never runtime detection; AD-19, Clarification Q2).
    pub simd: &'static str,
    /// Effect tier list the entries follow.
    pub tiers_version: &'static str,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const SIMD_BASELINE: &str = "baseline:sse2";
#[cfg(target_arch = "aarch64")]
const SIMD_BASELINE: &str = "baseline:neon";
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const SIMD_BASELINE: &str = "baseline";

/// The slice key: vello_cpu 0.3.0, single-threaded, u8 pipeline, deterministic SIMD level.
pub const SLICE_TABLE_KEY: TableKey = TableKey {
    backend: "vello_cpu",
    version: "=0.3.0",
    threading: "single-threaded",
    features: &["std", "text", "u8_pipeline"],
    precision: "optimize-speed",
    simd: SIMD_BASELINE,
    tiers_version: "ui4me-paint-tiers/1@1.0.0",
};

/// `paint.capability.solid-fill`: rects and paths, non-zero and even-odd fill.
pub const SOLID_FILL: CapabilityId = CapabilityId("paint.capability.solid-fill");
/// `paint.capability.glyph-outline`: monochrome glyph runs.
pub const GLYPH_OUTLINE: CapabilityId = CapabilityId("paint.capability.glyph-outline");

impl CapabilityTable {
    /// The slice table: vello_cpu, single-threaded (AD-19). The Mandatory entries the slice
    /// paints (solid rect and path fills, glyph runs), sorted by id; both `native`
    /// (capability-table.md §2 planning values, evidence `source-reading` until X-RND-CAPS).
    pub fn slice() -> CapabilityTable {
        CapabilityTable {
            entries: vec![
                CapabilityEntry {
                    id: GLYPH_OUTLINE,
                    tier: Tier::Mandatory,
                    availability: Availability::Native,
                    fallback: None,
                },
                CapabilityEntry {
                    id: SOLID_FILL,
                    tier: Tier::Mandatory,
                    availability: Availability::Native,
                    fallback: None,
                },
            ],
        }
    }

    pub fn key(&self) -> TableKey {
        SLICE_TABLE_KEY
    }

    pub fn get(&self, id: CapabilityId) -> Option<&CapabilityEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Mandatory ids that are neither `native` nor `owned` (non-empty ⇒ `BACKEND_UNSUPPORTED`).
    pub fn missing_mandatory(&self) -> Vec<CapabilityId> {
        self.entries
            .iter()
            .filter(|e| e.tier == Tier::Mandatory && e.availability == Availability::Unavailable)
            .map(|e| e.id)
            .collect()
    }

    /// Canonical text form hashed by [`CapabilityTable::digest`] (one `name=value` per line).
    pub fn canonical_text(&self) -> String {
        let k = self.key();
        let mut s = format!(
            "backend={}\nversion={}\nthreading={}\nfeatures={}\nprecision={}\nsimd={}\ntiers={}\n",
            k.backend,
            k.version,
            k.threading,
            k.features.join(","),
            k.precision,
            k.simd,
            k.tiers_version
        );
        let mut entries = self.entries.clone();
        entries.sort_by_key(|e| e.id);
        for e in entries {
            let tier = match e.tier {
                Tier::Mandatory => "mandatory",
                Tier::Optional => "optional",
            };
            let availability = match e.availability {
                Availability::Native => "native",
                Availability::Owned => "owned",
                Availability::Unavailable => "unavailable",
            };
            s.push_str(&format!(
                "entry={},{},{},{}\n",
                e.id.0,
                tier,
                availability,
                e.fallback.map(|f| f.0).unwrap_or("-")
            ));
        }
        s
    }

    /// Table-key digest `sha256:<hex>`, carried by every receipt as `capability_table`.
    pub fn digest(&self) -> String {
        let d = sha256(self.canonical_text().as_bytes());
        let mut out = String::from("sha256:");
        for b in d {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

/// FIPS 180-4 SHA-256.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256;

    fn hex(d: [u8; 32]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha256_known_answers() {
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(sha256(long)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
