//! Wire protocol between a LinkHub device and the signaling server.
//!
//! This is a *separate* envelope from the device-to-device wire protocol in
//! `core-rs` (`net::protocol`). The server only ever sees the routing envelope
//! and an opaque `payload_hex` — never the Noise plaintext inside. JSON is used
//! here (not the tab-separated p2p format) because the envelope needs nested,
//! self-describing fields and the server already depends on serde.
//!
//! Auth is server-first: on connect the server sends [`ServerMsg::Challenge`],
//! the device replies with [`ClientMsg::Auth`] carrying its Ed25519 identity
//! public key + a signature over the challenge (see [`crate::auth`]).
//! Presence and routing are keyed by the *proven* identity public key, which is
//! 1:1 with `device_id` (`device_id = "lh-" + sha256(pubkey_hex)[..16]`), so a
//! client cannot register or be addressed under a key it does not own.

use serde::{Deserialize, Serialize};

/// Messages sent by a device to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Answer the server's challenge: prove ownership of `public_key_hex` by
    /// signing the challenge nonce. `device_id` is a display label only — the
    /// server trusts `public_key_hex` (cryptographically proven) for routing.
    Auth {
        device_id: String,
        public_key_hex: String,
        signature_hex: String,
    },
    /// Ask the server to relay a signaling payload to another online device,
    /// addressed by that device's identity public key.
    Forward {
        to_public_key_hex: String,
        session_id: String,
        /// "offer" | "answer" | "ice-candidate" | "done" | "error"
        kind: String,
        /// Opaque to the server (SDP/ICE, optionally end-to-end signed).
        payload_hex: String,
    },
    /// Liveness check; server replies [`ServerMsg::Pong`].
    Ping,
}

/// Messages sent by the server to a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// First frame after connect. The device must sign `nonce` (domain-separated,
    /// see [`crate::auth::challenge_string`]) to authenticate.
    Challenge { nonce: String },
    /// Authentication succeeded; the device is now present and routable.
    Welcome { device_id: String },
    /// A signaling payload relayed from another device.
    Deliver {
        from_public_key_hex: String,
        from_device_id: String,
        session_id: String,
        kind: String,
        payload_hex: String,
    },
    /// Liveness reply.
    Pong,
    /// A recoverable or terminal error (auth failure, peer offline, bad frame).
    Error { reason: String },
}

impl ClientMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ClientMsg serializes")
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

impl ServerMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ServerMsg serializes")
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_round_trips_through_json() {
        let msg = ClientMsg::Auth {
            device_id: "lh-abc".into(),
            public_key_hex: "00ff".into(),
            signature_hex: "dead".into(),
        };
        assert_eq!(ClientMsg::from_json(&msg.to_json()).unwrap(), msg);
    }

    #[test]
    fn forward_and_deliver_carry_payload_opaque() {
        let fwd = ClientMsg::Forward {
            to_public_key_hex: "aa".into(),
            session_id: "s1".into(),
            kind: "offer".into(),
            payload_hex: "beef".into(),
        };
        assert_eq!(ClientMsg::from_json(&fwd.to_json()).unwrap(), fwd);

        let del = ServerMsg::Deliver {
            from_public_key_hex: "bb".into(),
            from_device_id: "lh-x".into(),
            session_id: "s1".into(),
            kind: "offer".into(),
            payload_hex: "beef".into(),
        };
        assert_eq!(ServerMsg::from_json(&del.to_json()).unwrap(), del);
    }

    #[test]
    fn tagged_representation_is_snake_case() {
        let json = ServerMsg::Challenge {
            nonce: "abc".into(),
        }
        .to_json();
        assert!(json.contains("\"type\":\"challenge\""), "{json}");
    }
}
