//! Ed25519 challenge/response for signaling-server login.
//!
//! Distinct from the device-to-device handshake in `core-rs`
//! (`identity::handshake_challenge`, which binds *two* device ids): here the
//! device only proves to the *server* that it owns an identity key. We use a
//! dedicated domain-separation header so a signature gathered here can never be
//! replayed as a p2p handshake signature, and vice versa.

use ed25519_dalek::{Signature, VerifyingKey};

/// Domain-separation prefix for signaling login signatures. Bump the version
/// suffix if the signed-message shape ever changes.
const SIGNALING_AUTH_HEADER: &str = "linkhub-signaling-auth-v1";

/// The exact byte string a device signs to answer a login challenge.
pub fn challenge_string(nonce: &str) -> String {
    format!("{SIGNALING_AUTH_HEADER}\0{}", nonce.trim())
}

/// Generate a fresh 16-byte (32 hex char) challenge nonce — same shape as
/// core's `new_handshake_nonce`.
pub fn new_nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS RNG available");
    hex::encode(bytes)
}

/// Verify that `signature_hex` is a valid signature of `challenge_string(nonce)`
/// under `public_key_hex`. Returns `Ok(())` only when the proof holds.
pub fn verify_login(public_key_hex: &str, nonce: &str, signature_hex: &str) -> Result<(), String> {
    let key_bytes: [u8; 32] = hex::decode(public_key_hex)
        .map_err(|e| format!("bad public key hex: {e}"))?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|e| format!("bad public key: {e}"))?;

    let sig_bytes = hex::decode(signature_hex).map_err(|e| format!("bad signature hex: {e}"))?;
    let signature = Signature::from_slice(&sig_bytes).map_err(|e| format!("bad signature: {e}"))?;

    verifying_key
        .verify_strict(challenge_string(nonce).as_bytes(), &signature)
        .map_err(|_| "signature does not match challenge".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).unwrap();
        SigningKey::from_bytes(&seed)
    }

    #[test]
    fn valid_signature_is_accepted() {
        let sk = signing_key();
        let pk_hex = hex::encode(sk.verifying_key().to_bytes());
        let nonce = new_nonce();
        let sig = sk.sign(challenge_string(&nonce).as_bytes());
        assert!(verify_login(&pk_hex, &nonce, &hex::encode(sig.to_bytes())).is_ok());
    }

    #[test]
    fn signature_over_wrong_nonce_is_rejected() {
        let sk = signing_key();
        let pk_hex = hex::encode(sk.verifying_key().to_bytes());
        let sig = sk.sign(challenge_string("aaaa").as_bytes());
        assert!(verify_login(&pk_hex, "bbbb", &hex::encode(sig.to_bytes())).is_err());
    }

    #[test]
    fn signature_from_other_key_is_rejected() {
        let signer = signing_key();
        let other_pk_hex = hex::encode(signing_key().verifying_key().to_bytes());
        let nonce = new_nonce();
        let sig = signer.sign(challenge_string(&nonce).as_bytes());
        assert!(verify_login(&other_pk_hex, &nonce, &hex::encode(sig.to_bytes())).is_err());
    }

    #[test]
    fn nonce_is_32_hex_chars_and_unique() {
        let a = new_nonce();
        let b = new_nonce();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
