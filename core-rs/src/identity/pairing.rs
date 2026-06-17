//! Pairing flow types: a [`PairingInvitation`] (transferred as a QR/text
//! payload), the [`PairingSession`] that derives a confirmation code, and the
//! [`TrustedDevice`] produced once both sides confirm.

use std::fmt;
use std::time::{Duration, Instant, SystemTime};

use crate::device::DeviceNode;

use super::{
    decode_hex_string, encode_hex, grouped_uppercase, sha256_hex, DeviceIdentity,
    PAIRING_PAYLOAD_HEADER,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingInvitation {
    identity: DeviceIdentity,
    nonce: String,
    created_at: Instant,
    ttl: Duration,
}

impl PairingInvitation {
    pub fn new(
        identity: DeviceIdentity,
        nonce: impl Into<String>,
        created_at: Instant,
        ttl: Duration,
    ) -> Self {
        Self {
            identity,
            nonce: nonce.into(),
            created_at,
            ttl,
        }
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.created_at) > self.ttl
    }

    pub fn to_payload(&self) -> String {
        [
            PAIRING_PAYLOAD_HEADER.to_string(),
            encode_hex(self.identity.device_id().as_bytes()),
            encode_hex(self.identity.device_name().as_bytes()),
            self.identity.public_key().to_string(),
            self.identity.dh_public_key().to_string(),
            encode_hex(self.nonce.as_bytes()),
            self.ttl.as_secs().to_string(),
        ]
        .join("|")
    }

    pub fn from_payload(payload: &str, created_at: Instant) -> Result<Self, String> {
        let fields = payload.trim().split('|').collect::<Vec<_>>();

        match fields.as_slice() {
            [PAIRING_PAYLOAD_HEADER, device_id, device_name, public_key, dh_public_key, nonce, ttl_seconds] =>
            {
                let device_id = decode_hex_string(device_id)
                    .map_err(|err| format!("invalid pairing payload device_id: {err}"))?;
                let device_name = decode_hex_string(device_name)
                    .map_err(|err| format!("invalid pairing payload device_name: {err}"))?;
                let nonce = decode_hex_string(nonce)
                    .map_err(|err| format!("invalid pairing payload nonce: {err}"))?;
                let ttl = ttl_seconds
                    .parse::<u64>()
                    .map(Duration::from_secs)
                    .map_err(|_| "invalid pairing payload ttl".to_string())?;

                if device_id.trim().is_empty() {
                    return Err("pairing payload device_id must not be empty".to_string());
                }

                if device_name.trim().is_empty() {
                    return Err("pairing payload device_name must not be empty".to_string());
                }

                if public_key.trim().is_empty() {
                    return Err("pairing payload public_key must not be empty".to_string());
                }

                if dh_public_key.trim().is_empty() {
                    return Err("pairing payload dh_public_key must not be empty".to_string());
                }

                if nonce.trim().is_empty() {
                    return Err("pairing payload nonce must not be empty".to_string());
                }

                if ttl.is_zero() {
                    return Err("pairing payload ttl must be greater than zero".to_string());
                }

                Ok(Self::new(
                    DeviceIdentity::new(
                        device_id,
                        device_name,
                        (*public_key).to_string(),
                        (*dh_public_key).to_string(),
                    ),
                    nonce,
                    created_at,
                    ttl,
                ))
            }
            [PAIRING_PAYLOAD_HEADER, ..] => {
                let field_count = fields.len() - 1; // exclude header
                Err(format!(
                    "pairing payload has {field_count} fields (expected 6); \
                     the v1 format now requires dh_public_key — \
                     please regenerate the pairing payload with the latest version"
                ))
            }
            _ => Err("unsupported pairing payload".to_string()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingSession {
    local_identity: DeviceIdentity,
    invitation: PairingInvitation,
}

impl PairingSession {
    pub fn new(local_identity: DeviceIdentity, invitation: PairingInvitation) -> Self {
        Self {
            local_identity,
            invitation,
        }
    }

    pub fn local_identity(&self) -> &DeviceIdentity {
        &self.local_identity
    }

    pub fn peer_identity(&self) -> &DeviceIdentity {
        self.invitation.identity()
    }

    pub fn confirmation_code(&self) -> String {
        confirmation_code(&self.local_identity, self.invitation.identity())
    }

    pub fn confirm(
        &self,
        entered_code: &str,
        now: Instant,
        paired_at: SystemTime,
    ) -> Result<TrustedDevice, PairingError> {
        if self.invitation.is_expired(now) {
            return Err(PairingError::Expired);
        }

        let expected = normalize_pairing_code(&self.confirmation_code());
        let entered = normalize_pairing_code(entered_code);

        if expected != entered {
            return Err(PairingError::CodeMismatch);
        }

        Ok(TrustedDevice::new(
            self.invitation.identity().clone(),
            paired_at,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedDevice {
    identity: DeviceIdentity,
    fingerprint: String,
    paired_at: SystemTime,
}

impl TrustedDevice {
    pub fn new(identity: DeviceIdentity, paired_at: SystemTime) -> Self {
        let fingerprint = identity.fingerprint();

        Self {
            identity,
            fingerprint,
            paired_at,
        }
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub fn device_id(&self) -> &str {
        self.identity.device_id()
    }

    pub fn device_name(&self) -> &str {
        self.identity.device_name()
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn paired_at(&self) -> SystemTime {
        self.paired_at
    }

    pub fn to_device_node(&self) -> DeviceNode {
        DeviceNode::new(self.device_id(), self.device_name())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum PairingError {
    CodeMismatch,
    Expired,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            PairingError::CodeMismatch => "pairing confirmation code does not match",
            PairingError::Expired => "pairing invitation has expired",
        };
        write!(f, "{message}")
    }
}

impl std::error::Error for PairingError {}

/// Derives the short confirmation code (SAS) shown to the user during pairing.
///
/// It depends ONLY on the two device fingerprints, sorted so both peers compute
/// the same value regardless of direction. It deliberately does NOT mix in the
/// invitation nonce: in the app's two-way flow each device generates its own
/// payload (its own nonce) and inspects the peer's, so a session only ever holds
/// the peer's nonce — including it made the two devices show different codes,
/// defeating the cross-device comparison the code exists for. MITM protection
/// comes from binding both fingerprints (i.e. both public keys), like a stable
/// safety number; the nonce added no protection here.
fn confirmation_code(local: &DeviceIdentity, peer: &DeviceIdentity) -> String {
    let mut fingerprints = [local.fingerprint(), peer.fingerprint()];
    fingerprints.sort();
    let digest = sha256_hex(format!("{}\0{}", fingerprints[0], fingerprints[1]).as_bytes());

    grouped_uppercase(&digest[..6], 3)
}

fn normalize_pairing_code(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}
