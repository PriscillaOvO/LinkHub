//! Device identity, pairing, trust and secure-storage primitives.
//!
//! This module is split by concern across submodules; this root keeps the small
//! shared helpers (hex/sha encoding, nonce/challenge generation), the shared
//! file-format header constants, and re-exports the public types so the crate
//! API (`linkhub_core::identity::*`) stays stable:
//!
//! - [`device_identity`] — [`DeviceIdentity`] / [`LocalIdentity`] and their encodings
//! - [`pairing`] — [`PairingInvitation`] / [`PairingSession`] / [`TrustedDevice`] / [`PairingError`]
//! - [`trust_store`] — [`TrustStore`]
//! - [`secure_store`] — platform-specific at-rest key protection (DPAPI/Keychain/Secret Service)

use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

mod device_identity;
mod pairing;
mod secure_store;
mod trust_store;

pub use device_identity::{DeviceIdentity, LocalIdentity};
pub use pairing::{PairingError, PairingInvitation, PairingSession, TrustedDevice};
pub use trust_store::TrustStore;

const LOCAL_IDENTITY_HEADER: &str = "linkhub_local_identity_v1";
const SECURE_LOCAL_IDENTITY_HEADER: &str = "linkhub_secure_local_identity_v1";
const SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI: &str = "windows-dpapi-user";
const SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN: &str = "macos-keychain";
const SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE: &str = "linux-secret-service";
const HANDSHAKE_CHALLENGE_HEADER: &str = "linkhub-auth-v1";
const PAIRING_PAYLOAD_HEADER: &str = "linkhub-pair-v1";
const TRUST_STORE_HEADER: &str = "linkhub_trust_store_v1";

pub fn new_pairing_nonce() -> String {
    let mut bytes = [0; 16];
    OsRng.fill_bytes(&mut bytes);

    encode_hex(&bytes)
}

pub fn new_handshake_nonce() -> String {
    new_pairing_nonce()
}

pub fn handshake_challenge(signer_device_id: &str, peer_device_id: &str, nonce: &str) -> String {
    format!(
        "{HANDSHAKE_CHALLENGE_HEADER}\0{}\0{}\0{}",
        signer_device_id.trim(),
        peer_device_id.trim(),
        nonce.trim()
    )
}

fn grouped_uppercase(value: &str, group_len: usize) -> String {
    value
        .as_bytes()
        .chunks(group_len)
        .map(|chunk| std::str::from_utf8(chunk).unwrap().to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn system_time_to_unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex_string(value: &str) -> Result<String, String> {
    let bytes = decode_hex(value)?;

    String::from_utf8(bytes).map_err(|err| err.to_string())
}

pub fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex value must have an even number of characters".to_string());
    }

    value
        .as_bytes()
        .chunks(2)
        .map(|chunk| {
            let hex = std::str::from_utf8(chunk).map_err(|err| err.to_string())?;
            u8::from_str_radix(hex, 16).map_err(|_| format!("invalid hex byte: {hex}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant, SystemTime};
    use std::{fs, io};

    fn identity(device_id: &str, device_name: &str, key: &str) -> DeviceIdentity {
        DeviceIdentity::new(device_id, device_name, key, "00".repeat(32))
    }

    #[test]
    fn identity_fingerprint_is_stable_and_grouped() {
        let identity = identity("phone-001", "Android Phone", "phone-public-key");

        assert_eq!(identity.fingerprint(), "3C5E-00FB-7731-6134");
        assert_eq!(identity.fingerprint(), identity.fingerprint());
    }

    #[test]
    fn identity_fingerprint_changes_when_key_changes() {
        let first = identity("phone-001", "Android Phone", "phone-public-key");
        let second = identity("phone-001", "Android Phone", "new-phone-public-key");

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn pairing_code_is_same_on_both_devices() {
        let now = Instant::now();
        let ttl = Duration::from_secs(60);
        let phone = identity("phone-001", "Android Phone", "phone-public-key");
        let windows = identity("windows-001", "Windows PC", "windows-public-key");

        let phone_session = PairingSession::new(
            phone.clone(),
            PairingInvitation::new(windows.clone(), "pairing-nonce-001", now, ttl),
        );
        let windows_session = PairingSession::new(
            windows,
            PairingInvitation::new(phone, "pairing-nonce-001", now, ttl),
        );

        assert_eq!(
            phone_session.confirmation_code(),
            windows_session.confirmation_code()
        );
    }

    #[test]
    fn pairing_confirm_accepts_normalized_code() {
        let now = Instant::now();
        let paired_at = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let phone = identity("phone-001", "Android Phone", "phone-public-key");
        let windows = identity("windows-001", "Windows PC", "windows-public-key");
        let session = PairingSession::new(
            windows,
            PairingInvitation::new(
                phone.clone(),
                "pairing-nonce-001",
                now,
                Duration::from_secs(60),
            ),
        );
        let entered_code = session.confirmation_code().replace('-', " ");

        let trusted = session.confirm(&entered_code, now, paired_at).unwrap();

        assert_eq!(trusted.device_id(), "phone-001");
        assert_eq!(trusted.device_name(), "Android Phone");
        assert_eq!(trusted.fingerprint(), phone.fingerprint());
        assert_eq!(trusted.paired_at(), paired_at);
    }

    #[test]
    fn pairing_confirm_rejects_wrong_or_expired_code() {
        let now = Instant::now();
        let phone = identity("phone-001", "Android Phone", "phone-public-key");
        let windows = identity("windows-001", "Windows PC", "windows-public-key");
        let session = PairingSession::new(
            windows,
            PairingInvitation::new(phone, "pairing-nonce-001", now, Duration::from_secs(60)),
        );

        assert_eq!(
            session
                .confirm("BAD-000", now, SystemTime::UNIX_EPOCH)
                .unwrap_err(),
            PairingError::CodeMismatch
        );
        assert_eq!(
            session
                .confirm(
                    &session.confirmation_code(),
                    now + Duration::from_secs(61),
                    SystemTime::UNIX_EPOCH,
                )
                .unwrap_err(),
            PairingError::Expired
        );
    }

    #[test]
    fn pairing_invitation_payload_round_trips() {
        let now = Instant::now();
        let invitation = PairingInvitation::new(
            identity("phone-001", "Android Phone", "phone-public-key"),
            "nonce-001",
            now,
            Duration::from_secs(120),
        );

        let payload = invitation.to_payload();
        let parsed = PairingInvitation::from_payload(&payload, now).unwrap();

        assert_eq!(parsed.identity(), invitation.identity());
        assert_eq!(parsed.nonce(), "nonce-001");
        assert_eq!(parsed.ttl(), Duration::from_secs(120));
        assert!(!parsed.is_expired(now + Duration::from_secs(119)));
        assert!(parsed.is_expired(now + Duration::from_secs(121)));
    }

    #[test]
    fn pairing_invitation_payload_rejects_invalid_values() {
        let now = Instant::now();

        assert!(PairingInvitation::from_payload("not-linkhub", now).is_err());
        assert!(PairingInvitation::from_payload(
            "linkhub-pair-v1|70686f6e652d303031|416e64726f69642050686f6e65||6e6f6e6365|120",
            now
        )
        .is_err());
        assert!(PairingInvitation::from_payload(
            "linkhub-pair-v1|70686f6e652d303031|416e64726f69642050686f6e65|key|6e6f6e6365|0",
            now
        )
        .is_err());
    }

    #[test]
    fn new_pairing_nonce_is_hex_and_unique() {
        let first = new_pairing_nonce();
        let second = new_pairing_nonce();

        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn handshake_signature_verifies_for_expected_peer_and_nonce() {
        let local = LocalIdentity::from_keys(
            "Windows PC",
            [31; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        let nonce = "handshake-nonce-001";
        let signature = local.sign_handshake_challenge("phone-001", nonce).unwrap();

        assert!(local
            .identity()
            .verify_handshake_signature("phone-001", nonce, &signature)
            .unwrap());
        assert!(!local
            .identity()
            .verify_handshake_signature("phone-002", nonce, &signature)
            .unwrap());
        assert!(!local
            .identity()
            .verify_handshake_signature("phone-001", "different-nonce", &signature)
            .unwrap());
    }

    #[test]
    fn handshake_signature_rejects_invalid_public_key_or_signature() {
        let identity = DeviceIdentity::new("phone-001", "Phone", "not-hex", "00".repeat(32));

        assert!(identity
            .verify_handshake_signature("windows-001", "nonce", "abcd")
            .is_err());

        let local =
            LocalIdentity::from_keys("Windows PC", [37; 32], [0u8; 32], SystemTime::UNIX_EPOCH);

        assert!(local
            .identity()
            .verify_handshake_signature("phone-001", "nonce", "abcd")
            .is_err());
    }

    #[test]
    fn new_handshake_nonce_is_hex_and_unique() {
        let first = new_handshake_nonce();
        let second = new_handshake_nonce();

        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn trust_store_keeps_devices_sorted_and_replaces_by_id() {
        let paired_at = SystemTime::UNIX_EPOCH;
        let mut store = TrustStore::new();

        store.trust(TrustedDevice::new(
            identity("phone-002", "Second Phone", "second-key"),
            paired_at,
        ));
        store.trust(TrustedDevice::new(
            identity("phone-001", "Android Phone", "phone-public-key"),
            paired_at,
        ));
        store.trust(TrustedDevice::new(
            identity("phone-002", "Updated Phone", "updated-key"),
            paired_at,
        ));

        let devices = store.trusted_devices();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].device_id(), "phone-001");
        assert_eq!(devices[1].device_id(), "phone-002");
        assert_eq!(devices[1].device_name(), "Updated Phone");
        assert!(store.is_trusted("phone-001"));
        assert!(store.trusted_device("missing").is_none());
    }

    #[test]
    fn trust_store_saves_and_loads_paired_devices() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-trust-store-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let paired_at = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let mut store = TrustStore::new();
        store.trust(TrustedDevice::new(
            identity("phone-001", "Android Phone", "phone-public-key"),
            paired_at,
        ));
        store.trust(TrustedDevice::new(
            identity("ipad-001", "iPad Pro", "ipad-public-key"),
            paired_at + Duration::from_secs(1),
        ));

        store.save_to_path(&path).unwrap();
        let loaded = TrustStore::load_from_path(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(loaded.trusted_devices().len(), 2);
        assert_eq!(
            loaded.trusted_device("phone-001").unwrap().fingerprint(),
            store.trusted_device("phone-001").unwrap().fingerprint()
        );
        assert_eq!(
            loaded.trusted_device("ipad-001").unwrap().paired_at(),
            paired_at + Duration::from_secs(1)
        );
    }

    #[test]
    fn trust_store_loads_missing_file_as_empty_store() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-missing-trust-store-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let _ = fs::remove_file(&path);

        let store = TrustStore::load_from_path(&path).unwrap();

        assert!(store.trusted_devices().is_empty());
    }

    #[test]
    fn trust_store_rejects_invalid_file_content() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-invalid-trust-store-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        fs::write(&path, "not-a-linkhub-trust-store").unwrap();

        let err = TrustStore::load_from_path(&path).unwrap_err();
        let _ = fs::remove_file(&path);

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn trust_store_accepts_utf8_bom_header() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-bom-trust-store-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        fs::write(
            &path,
            "\u{feff}linkhub_trust_store_v1\ndevice=70686f6e652d303031|416e64726f69642050686f6e65|70686f6e652d7075626c69632d6b6579|0000000000000000000000000000000000000000000000000000000000000000|0\n",
        )
        .unwrap();

        let store = TrustStore::load_from_path(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert!(store.is_trusted("phone-001"));
    }

    #[test]
    fn local_identity_derives_stable_device_id_from_signing_key() {
        let signing_key = [7; 32];
        let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(7);

        let first = LocalIdentity::from_keys("Windows PC", signing_key, [0u8; 32], created_at);
        let second = LocalIdentity::from_keys("Windows PC", signing_key, [0u8; 32], created_at);

        assert_eq!(first.device_id(), second.device_id());
        assert_eq!(first.public_key(), second.public_key());
        assert_eq!(first.signing_key_hex(), second.signing_key_hex());
        assert_eq!(first.created_at(), created_at);
        assert!(first.device_id().starts_with("lh-"));
        assert_eq!(first.device_id().len(), 19);
    }

    #[test]
    fn local_identity_saves_and_loads() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-local-identity-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let identity = LocalIdentity::from_keys(
            "Windows PC",
            [11; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(42),
        );

        identity.save_to_path(&path).unwrap();
        let loaded = LocalIdentity::load_from_path(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(loaded, identity);
        assert_eq!(
            loaded.identity().fingerprint(),
            identity.identity().fingerprint()
        );
    }

    #[test]
    fn local_identity_load_or_generate_reuses_existing_identity() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-local-identity-reuse-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let first = LocalIdentity::load_or_generate(
            &path,
            "Windows PC",
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        )
        .unwrap();
        let second = LocalIdentity::load_or_generate(
            &path,
            "Renamed PC",
            SystemTime::UNIX_EPOCH + Duration::from_secs(2),
        )
        .unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(second, first);
        assert_eq!(second.device_name(), "Windows PC");
    }

    #[cfg(windows)]
    #[test]
    fn secure_local_identity_uses_dpapi_and_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-secure-local-identity-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let identity = LocalIdentity::from_keys(
            "Windows PC",
            [12; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(42),
        );

        identity.save_to_secure_path(&path).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let loaded = LocalIdentity::load_from_secure_path(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(loaded, identity);
        assert!(content.starts_with(SECURE_LOCAL_IDENTITY_HEADER));
        assert!(content.contains(SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI));
        assert!(!content.contains(identity.signing_key_hex()));
        assert!(!content.contains(identity.public_key()));
    }

    #[test]
    fn local_identity_rejects_public_key_mismatch() {
        let path = std::env::temp_dir().join(format!(
            "linkhub-local-identity-invalid-{}.txt",
            sha256_hex(format!("{:?}", SystemTime::now()).as_bytes())
        ));
        let identity =
            LocalIdentity::from_keys("Windows PC", [13; 32], [0u8; 32], SystemTime::UNIX_EPOCH);
        identity.save_to_path(&path).unwrap();
        let content = fs::read_to_string(&path)
            .unwrap()
            .replace(identity.public_key(), "00");
        fs::write(&path, content).unwrap();

        let err = LocalIdentity::load_from_path(&path).unwrap_err();
        let _ = fs::remove_file(&path);

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn trusted_device_can_seed_device_agent_node() {
        let trusted = TrustedDevice::new(
            identity("phone-001", "Android Phone", "phone-public-key"),
            SystemTime::UNIX_EPOCH,
        );

        let node = trusted.to_device_node();

        assert_eq!(node.id(), "phone-001");
        assert_eq!(node.name(), "Android Phone");
    }
}
