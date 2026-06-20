//! Deterministic v3 onion (.onion) address derivation for a device identity.
//!
//! A device's onion address is what lets an already-paired peer reconnect to it
//! over Tor from anywhere, through NAT/firewalls, with no signaling server — the
//! address is carried in the identity exchange and stored in the trust store, so
//! there is no lookup. This module is the **pure-Rust** core of that: it has no
//! dependency on Arti / the (feature-gated) Tor transport, so the address can be
//! computed and shared in the default build.
//!
//! The hidden-service key is **derived from, but not equal to**, the device's
//! Ed25519 signing key (domain-separated hash) so the same key is never reused
//! across two protocols. Because the derivation is deterministic, a device always
//! presents the same onion address; peers receive it once and store it.

use sha2::{Digest, Sha256};
use sha3::Sha3_256;

/// Domain separator for deriving the hidden-service key seed from the device's
/// Ed25519 signing-key seed. Distinct header => the HS key can never coincide
/// with the identity signing key (no cross-protocol key reuse).
const ONION_HS_DERIVATION_HEADER: &str = "linkhub-onion-hs-v1";

/// rend-spec-v3 §6 \[ONIONADDRESS] constants.
const ONION_VERSION: u8 = 0x03;
const ONION_CHECKSUM_PREFIX: &[u8] = b".onion checksum";

/// Derive the 32-byte ed25519 seed for this device's hidden-service key from its
/// Ed25519 signing-key seed. `SHA-256(header \0 signing_seed)` — one-way and
/// domain-separated, so the HS key is independent of the signing key while still
/// being deterministic (the device reproduces the same onion address every run).
pub(crate) fn derive_onion_hs_seed(signing_key_seed: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ONION_HS_DERIVATION_HEADER.as_bytes());
    hasher.update([0]);
    hasher.update(signing_key_seed);
    hasher.finalize().into()
}

/// Compute the v3 onion address (`"<56-char base32>.onion"`) for a hidden-service
/// Ed25519 public key, per Tor rend-spec-v3 §6:
///
/// ```text
/// checksum = SHA3-256(".onion checksum" || pubkey || version)[..2]
/// address  = base32_lower(pubkey || checksum || version) + ".onion"
/// ```
pub(crate) fn onion_address_v3(ed25519_public_key: &[u8; 32]) -> String {
    let mut checksum_hasher = Sha3_256::new();
    checksum_hasher.update(ONION_CHECKSUM_PREFIX);
    checksum_hasher.update(ed25519_public_key);
    checksum_hasher.update([ONION_VERSION]);
    let checksum = checksum_hasher.finalize();

    let mut binary = Vec::with_capacity(35);
    binary.extend_from_slice(ed25519_public_key);
    binary.extend_from_slice(&checksum[..2]);
    binary.push(ONION_VERSION);

    let mut address = base32_lower_nopad(&binary);
    address.push_str(".onion");
    address
}

/// RFC 4648 base32 with the lowercase alphabet and no padding (matching Tor's
/// onion-address encoding). For 35-byte input this yields exactly 56 chars.
fn base32_lower_nopad(data: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in data {
        acc = (acc << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 0x1f) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onion_address_matches_arti_reference_vector() {
        // Ground truth produced by Arti's `tor_hscrypto::pk::HsId` for the fixed
        // key bytes 0x00..0x20 (see the tor spike's `onion_vector` test).
        let mut pubkey = [0u8; 32];
        for (i, b) in pubkey.iter_mut().enumerate() {
            *b = i as u8;
        }
        assert_eq!(
            onion_address_v3(&pubkey),
            "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead.onion"
        );
    }

    #[test]
    fn onion_address_has_v3_shape() {
        let address = onion_address_v3(&[0u8; 32]);
        assert!(address.ends_with(".onion"));
        let label = address.trim_end_matches(".onion");
        assert_eq!(label.len(), 56);
        assert!(label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b)));
    }

    #[test]
    fn hs_seed_is_deterministic_and_separated_from_signing_key() {
        let signing_seed = [7u8; 32];
        let hs_seed = derive_onion_hs_seed(&signing_seed);

        assert_eq!(hs_seed, derive_onion_hs_seed(&signing_seed));
        // Must NOT equal the signing seed it was derived from (no key reuse).
        assert_ne!(hs_seed, signing_seed);
        // A different signing key yields a different HS seed.
        assert_ne!(hs_seed, derive_onion_hs_seed(&[8u8; 32]));
    }

    #[test]
    fn base32_lower_nopad_matches_known_values() {
        // RFC 4648 base32 of "foobar" is "MFQWS===" → lowercase, no pad "mzxw6ytboi" for "fooba"? verify simple cases.
        assert_eq!(base32_lower_nopad(b""), "");
        assert_eq!(base32_lower_nopad(b"f"), "my");
        assert_eq!(base32_lower_nopad(b"fo"), "mzxq");
        assert_eq!(base32_lower_nopad(b"foo"), "mzxw6");
        assert_eq!(base32_lower_nopad(b"foob"), "mzxw6yq");
        assert_eq!(base32_lower_nopad(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_lower_nopad(b"foobar"), "mzxw6ytboi");
    }
}
