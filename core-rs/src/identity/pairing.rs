//! Pairing flow types: a [`PairingInvitation`] (transferred as a QR/text
//! payload), the [`PairingSession`] that derives a confirmation code, and the
//! [`TrustedDevice`] produced once both sides confirm.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::device::DeviceNode;

use super::{
    decode_hex_string, encode_hex, grouped_uppercase, sha256_hex, DeviceIdentity,
    PAIRING_PAYLOAD_HEADER,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingInvitation {
    identity: DeviceIdentity,
    issued_at: SystemTime,
    ttl: Duration,
}

impl PairingInvitation {
    pub fn new(identity: DeviceIdentity, issued_at: SystemTime, ttl: Duration) -> Self {
        Self {
            identity,
            issued_at,
            ttl,
        }
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub fn issued_at(&self) -> SystemTime {
        self.issued_at
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn is_expired(&self, now: SystemTime) -> bool {
        now.duration_since(self.issued_at)
            .map(|age| age > self.ttl)
            .unwrap_or(false)
    }

    pub fn to_payload(&self) -> String {
        [
            PAIRING_PAYLOAD_HEADER.to_string(),
            encode_hex(self.identity.device_id().as_bytes()),
            encode_hex(self.identity.device_name().as_bytes()),
            self.identity.public_key().to_string(),
            self.identity.dh_public_key().to_string(),
            system_time_to_unix_seconds(self.issued_at).to_string(),
            self.ttl.as_secs().to_string(),
        ]
        .join("|")
    }

    pub fn from_payload(payload: &str, now: SystemTime) -> Result<Self, String> {
        let fields = payload.trim().split('|').collect::<Vec<_>>();

        match fields.as_slice() {
            [PAIRING_PAYLOAD_HEADER, device_id, device_name, public_key, dh_public_key, issued_at_seconds, ttl_seconds] =>
            {
                let device_id = decode_hex_string(device_id)
                    .map_err(|err| format!("invalid pairing payload device_id: {err}"))?;
                let device_name = decode_hex_string(device_name)
                    .map_err(|err| format!("invalid pairing payload device_name: {err}"))?;
                let issued_at = issued_at_seconds
                    .parse::<u64>()
                    .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
                    .map_err(|_| "invalid pairing payload issued_at".to_string())?;
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

                if ttl.is_zero() {
                    return Err("pairing payload ttl must be greater than zero".to_string());
                }

                let invitation = Self::new(
                    DeviceIdentity::new(
                        device_id,
                        device_name,
                        (*public_key).to_string(),
                        (*dh_public_key).to_string(),
                    ),
                    issued_at,
                    ttl,
                );

                if invitation.is_expired(now) {
                    return Err("pairing invitation has expired".to_string());
                }

                Ok(invitation)
            }
            ["linkhub-pair-v1", ..] | ["linkhub-pair", ..] => {
                Err("unsupported pairing payload version; please regenerate a linkhub-pair-v2 payload and re-pair".to_string())
            }
            [PAIRING_PAYLOAD_HEADER, ..] => {
                let field_count = fields.len() - 1; // exclude header
                Err(format!(
                    "pairing payload has {field_count} fields (expected 6); \
                     the v2 format requires issued_at and ttl, and no nonce; \
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
        now: SystemTime,
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
/// It depends only on the two device fingerprints, sorted so both peers compute
/// the same value regardless of direction. Directional randomness is excluded
/// because each peer scans the other's payload; mixing it in made the two sides
/// display different codes. The widened 40-bit display keeps the stable safety
/// number property while making offline short-code collisions substantially
/// harder than the old 24-bit code.
fn confirmation_code(local: &DeviceIdentity, peer: &DeviceIdentity) -> String {
    let mut fingerprints = [local.fingerprint(), peer.fingerprint()];
    fingerprints.sort();
    let digest = sha256_hex(format!("{}\0{}", fingerprints[0], fingerprints[1]).as_bytes());

    grouped_uppercase(&digest[..10], 5)
}

fn normalize_pairing_code(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

fn system_time_to_unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
