//! Deterministic UUIDs for new copper (K08.3).
//!
//! A new item's UUID is the 128-bit FNV-1a hash of the input file's own
//! FNV-1a hash, the item kind, net, layer, nanometre coordinates and its
//! index in emission order, formatted as a version-8 UUID (RFC 9562: version
//! nibble `8`, variant bits `10`). A UUID already in the file, or already
//! handed out, is hashed again with a counter appended. No randomness and no
//! clock (D2): the same input and the same routed board give the same file.
//!
//! KiCad tie-breaks overlapping zones of equal priority by KIID, so random
//! UUIDs changed its fill between runs on identical copper (a
//! KiCadRoutingTools finding, credited in [`crate::write`]); stable UUIDs
//! keep the fill stable too.

use std::collections::BTreeSet;

/// The FNV-1a 128 offset basis.
pub const FNV128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
/// The FNV-1a 128 prime.
pub const FNV128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// FNV-1a 128 state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv128(u128);

impl Default for Fnv128 {
    fn default() -> Self {
        Fnv128(FNV128_OFFSET)
    }
}

impl Fnv128 {
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        let mut h = self.0;
        for &b in bytes {
            h ^= u128::from(b);
            h = h.wrapping_mul(FNV128_PRIME);
        }
        self.0 = h;
        self
    }

    pub fn finish(&self) -> u128 {
        self.0
    }
}

/// FNV-1a 128 of `bytes`.
pub fn fnv1a128(bytes: &[u8]) -> u128 {
    let mut h = Fnv128::default();
    h.update(bytes);
    h.finish()
}

/// `h` as a lowercase version-8 UUID.
pub fn format_v8(h: u128) -> String {
    let mut b = h.to_be_bytes();
    b[6] = (b[6] & 0x0f) | 0x80;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What a new item's UUID is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UuidKey<'a> {
    /// `segment` or `via`.
    pub kind: &'a str,
    pub net: u32,
    /// The layer (segment) or the top layer (via), by copper position.
    pub layer: u32,
    /// The nanometre coordinates: start and end, or the via centre.
    pub coords: &'a [i64],
    /// The index of the item in emission order.
    pub index: u64,
}

/// Hands out UUIDs that are unique in the file.
#[derive(Clone, Debug)]
pub struct UuidGen {
    file_hash: u128,
    taken: BTreeSet<String>,
}

impl UuidGen {
    /// `file_hash` is the FNV-1a 128 of the input file; `existing` are the
    /// UUIDs (and tstamps) the file already has, in any case.
    pub fn new<'a>(file_hash: u128, existing: impl IntoIterator<Item = &'a str>) -> UuidGen {
        UuidGen {
            file_hash,
            taken: existing.into_iter().map(str::to_ascii_lowercase).collect(),
        }
    }

    /// Marks `uuid` as taken (a test hook for forced collisions).
    pub fn reserve(&mut self, uuid: &str) {
        self.taken.insert(uuid.to_ascii_lowercase());
    }

    /// The UUID `key` would get with no collision.
    pub fn candidate(&self, key: &UuidKey<'_>, counter: u64) -> String {
        let mut h = Fnv128::default();
        h.update(&self.file_hash.to_le_bytes());
        h.update(key.kind.as_bytes());
        h.update(&[0]);
        h.update(&key.net.to_le_bytes());
        h.update(&key.layer.to_le_bytes());
        for c in key.coords {
            h.update(&c.to_le_bytes());
        }
        h.update(&key.index.to_le_bytes());
        if counter > 0 {
            h.update(&counter.to_le_bytes());
        }
        format_v8(h.finish())
    }

    /// The UUID of the next item: the first candidate not taken.
    pub fn next(&mut self, key: &UuidKey<'_>) -> String {
        let mut counter = 0u64;
        loop {
            let uuid = self.candidate(key, counter);
            if self.taken.insert(uuid.clone()) {
                return uuid;
            }
            counter += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a128_known_vectors() {
        // The empty input is the offset basis; "a" from the FNV reference
        // (draft-eastlake-fnv, test vector for FNV-1a 128).
        assert_eq!(fnv1a128(b""), FNV128_OFFSET);
        assert_eq!(fnv1a128(b"a"), 0xd228_cb69_6f1a_8caf_7891_2b70_4e4a_8964);
    }

    #[test]
    fn version_and_variant_bits() {
        for h in [0u128, u128::MAX, 0x1234_5678_9abc_def0_1234_5678_9abc_def0] {
            let u = format_v8(h);
            assert_eq!(u.len(), 36);
            assert_eq!(&u[14..15], "8", "{u}");
            assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"), "{u}");
        }
    }

    #[test]
    fn forced_collision_is_resolved_deterministically() {
        let key = UuidKey {
            kind: "segment",
            net: 3,
            layer: 0,
            coords: &[1, 2, 3, 4],
            index: 0,
        };
        let free = UuidGen::new(7, []).next(&key);
        let mut a = UuidGen::new(7, [free.as_str()]);
        let mut b = UuidGen::new(7, []);
        b.reserve(&free.to_ascii_uppercase());
        let (ua, ub) = (a.next(&key), b.next(&key));
        assert_ne!(ua, free);
        assert_eq!(ua, ub);
        assert_eq!(ua, a.candidate(&key, 1));
        // a second item with the same key gets yet another one
        assert_ne!(a.next(&key), ua);
    }
}
