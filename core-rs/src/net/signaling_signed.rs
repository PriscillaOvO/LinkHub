//! End-to-end signing of WebRTC SDP signaling (Stage 5 / T3, design §7).
//!
//! The signaling server relays an opaque `payload_hex` and never holds key
//! material, but it (or anything that can write to the relay path) could still
//! **tamper with or substitute** the SDP offer/answer it forwards — pinning both
//! peers onto a relay it controls, redirecting candidates, or downgrading the
//! connection (a connection-redirection attack). The Noise KK session on top
//! still protects the *payload*, but the *transport* must not be silently
//! steerable by an untrusted middlebox.
//!
//! So the sender signs each SDP with its Ed25519 identity key, binding it to the
//! session id and role (offer/answer); the receiver verifies that signature
//! against the identity key it *already expects* this peer to use (the trusted
//! device it is establishing the connection with) before handing the SDP to
//! webrtc. A server that flips a byte, swaps an offer for an answer, or replays
//! an SDP from another session can no longer be trusted by the receiver.
//!
//! Pure Ed25519 + serde (no webrtc/tokio), so it compiles and is unit-tested in
//! the default build; the WebRTC CLI/bridge wires it in behind the `webrtc`
//! feature. The signed bytes are produced by
//! [`crate::LocalIdentity::sign_signaling_sdp`] /
//! [`crate::identity::signaling_sdp_message`] — keep verification in sync.

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::identity::{decode_hex, signaling_sdp_message};
use crate::LocalIdentity;

/// Current `payload_hex` envelope version. Bump if the shape changes.
const ENVELOPE_VERSION: u8 = 1;

/// What gets hex-encoded into the signaling `payload_hex` field: the SDP plus a
/// detached signature over [`signaling_sdp_message`].
#[derive(Debug, Serialize, Deserialize)]
struct SignedSdpEnvelope {
    v: u8,
    sdp: String,
    sig: String,
}

/// Sign `sdp` for `(session_id, kind)` with `identity` and return the
/// `payload_hex` to hand to [`crate::net::SignalingClient::send_signaling`].
pub fn seal_sdp(
    identity: &LocalIdentity,
    session_id: &str,
    kind: &str,
    sdp: &str,
) -> Result<String, String> {
    let sig = identity.sign_signaling_sdp(session_id, kind, sdp)?;
    let envelope = SignedSdpEnvelope {
        v: ENVELOPE_VERSION,
        sdp: sdp.to_string(),
        sig,
    };
    let json = serde_json::to_string(&envelope).map_err(|err| format!("seal sdp: {err}"))?;
    Ok(encode_hex(json.as_bytes()))
}

/// Verify and unwrap a `payload_hex` produced by [`seal_sdp`]. The signature
/// must come from `expected_signer_public_key_hex` (the identity key the receiver
/// already expects this peer to use — vetted upstream against the trust store)
/// and be bound to the same `session_id`/`kind`. Returns the SDP only when the
/// proof holds.
pub fn open_sdp(
    expected_signer_public_key_hex: &str,
    session_id: &str,
    kind: &str,
    payload_hex: &str,
) -> Result<String, String> {
    let json_bytes =
        decode_hex(payload_hex).map_err(|err| format!("bad signaling payload hex: {err}"))?;
    let envelope: SignedSdpEnvelope = serde_json::from_slice(&json_bytes)
        .map_err(|err| format!("bad signaling envelope: {err}"))?;
    if envelope.v != ENVELOPE_VERSION {
        return Err(format!(
            "unsupported signaling envelope version {}",
            envelope.v
        ));
    }
    verify_signaling_sdp(
        expected_signer_public_key_hex,
        session_id,
        kind,
        &envelope.sdp,
        &envelope.sig,
    )?;
    Ok(envelope.sdp)
}

/// Verify a detached Ed25519 signature over an SDP signal. The signed bytes are
/// [`signaling_sdp_message`]; this MUST mirror
/// [`crate::LocalIdentity::sign_signaling_sdp`]. Uses `verify_strict` to reject
/// signatures under malleable / small-order keys.
pub fn verify_signaling_sdp(
    public_key_hex: &str,
    session_id: &str,
    kind: &str,
    sdp: &str,
    signature_hex: &str,
) -> Result<(), String> {
    let key_bytes: [u8; 32] = decode_hex(public_key_hex)
        .map_err(|err| format!("bad public key hex: {err}"))?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|err| format!("bad public key: {err}"))?;
    let sig_bytes = decode_hex(signature_hex).map_err(|err| format!("bad signature hex: {err}"))?;
    let signature =
        Signature::from_slice(&sig_bytes).map_err(|err| format!("bad signature: {err}"))?;

    verifying_key
        .verify_strict(&signaling_sdp_message(session_id, kind, sdp), &signature)
        .map_err(|_| "signaling signature does not match".to_string())
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn identity(name: &str) -> LocalIdentity {
        LocalIdentity::generate(name, SystemTime::UNIX_EPOCH)
    }

    const SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";

    #[test]
    fn seal_then_open_round_trips() {
        let alice = identity("Alice");
        let payload = seal_sdp(&alice, "sess-1", "offer", SDP).unwrap();
        let sdp = open_sdp(alice.public_key(), "sess-1", "offer", &payload).unwrap();
        assert_eq!(sdp, SDP);
    }

    #[test]
    fn open_rejects_signature_from_a_different_identity() {
        let alice = identity("Alice");
        let mallory = identity("Mallory");
        let payload = seal_sdp(&alice, "sess-1", "offer", SDP).unwrap();
        // Receiver expects Mallory's key but the SDP was signed by Alice.
        assert!(open_sdp(mallory.public_key(), "sess-1", "offer", &payload).is_err());
    }

    #[test]
    fn open_rejects_tampered_sdp() {
        let alice = identity("Alice");
        let payload = seal_sdp(&alice, "sess-1", "offer", SDP).unwrap();
        // A middlebox swaps the SDP but keeps the original (now invalid) signature.
        let json = String::from_utf8(decode_hex(&payload).unwrap()).unwrap();
        let tampered = json.replace("IP4 0.0.0.0", "IP4 10.0.0.9");
        assert_ne!(tampered, json);
        let tampered_payload = encode_hex(tampered.as_bytes());
        assert!(open_sdp(alice.public_key(), "sess-1", "offer", &tampered_payload).is_err());
    }

    #[test]
    fn open_rejects_role_swap() {
        let alice = identity("Alice");
        let payload = seal_sdp(&alice, "sess-1", "offer", SDP).unwrap();
        // Replaying an offer as an answer must fail (kind is bound into the sig).
        assert!(open_sdp(alice.public_key(), "sess-1", "answer", &payload).is_err());
    }

    #[test]
    fn open_rejects_cross_session_replay() {
        let alice = identity("Alice");
        let payload = seal_sdp(&alice, "sess-1", "offer", SDP).unwrap();
        assert!(open_sdp(alice.public_key(), "sess-2", "offer", &payload).is_err());
    }

    #[test]
    fn open_rejects_malformed_payload() {
        let alice = identity("Alice");
        assert!(open_sdp(alice.public_key(), "sess-1", "offer", "not-hex!!").is_err());
        assert!(open_sdp(alice.public_key(), "sess-1", "offer", "00ff").is_err());
    }
}
