//! Device identity types: the public [`DeviceIdentity`] descriptor and the
//! private-key-bearing [`LocalIdentity`], plus their on-disk text encodings
//! (plaintext and secure-store backed).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use x25519_dalek::{PublicKey as DhPublicKey, StaticSecret as DhStaticSecret};

use super::onion::{derive_onion_hs_seed, onion_address_v3};
use super::secure_store::{
    format_secure_local_identity_file, parse_secure_local_identity_file,
    protect_local_identity_bytes, unprotect_local_identity_bytes,
};
use super::{
    decode_hex, decode_hex_string, encode_hex, grouped_uppercase, handshake_challenge,
    identity_binding_message, sha256_hex, signaling_sdp_message, system_time_to_unix_seconds,
    LOCAL_IDENTITY_HEADER,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceIdentity {
    device_id: String,
    device_name: String,
    public_key: String,
    dh_public_key: String,
    onion_address: Option<String>,
}

impl DeviceIdentity {
    pub fn new(
        device_id: impl Into<String>,
        device_name: impl Into<String>,
        public_key: impl Into<String>,
        dh_public_key: impl Into<String>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: device_name.into(),
            public_key: public_key.into(),
            dh_public_key: dh_public_key.into(),
            onion_address: None,
        }
    }

    /// Attach a peer's advertised v3 `.onion` address (learned in the identity
    /// exchange and persisted in the trust store) so it can later be reconnected
    /// over Tor with no signaling server. Whitespace-only / empty becomes `None`.
    /// Advisory only: the address is never re-derivable from the peer's public key
    /// (it comes from the peer's secret hidden-service seed), so it cannot be
    /// verified here — but a forged address merely points a dialer at the wrong
    /// onion, where the Noise KK handshake fails closed. The real authentication
    /// stays the static-key Noise KK session, never this field.
    pub fn with_onion_address(mut self, onion_address: Option<String>) -> Self {
        self.onion_address = onion_address
            .map(|address| address.trim().to_string())
            .filter(|address| !address.is_empty());
        self
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The peer's stored v3 `.onion` address, if one was exchanged. See
    /// [`Self::with_onion_address`].
    pub fn onion_address(&self) -> Option<&str> {
        self.onion_address.as_deref()
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn dh_public_key(&self) -> &str {
        &self.dh_public_key
    }

    pub fn fingerprint(&self) -> String {
        let digest = sha256_hex(format!("{}\0{}", self.device_id, self.public_key).as_bytes());
        grouped_uppercase(&digest[..16], 4)
    }

    /// Whether `device_id` is the stable hash of `public_key` (the binding every
    /// honest identity satisfies, since `device_id = "lh-" + sha256(public_key)`).
    /// A wire-transmitted identity claiming a `device_id` that does not derive
    /// from its own Ed25519 key must be rejected before it is trusted.
    pub fn has_consistent_device_id(&self) -> bool {
        stable_device_id(&self.public_key) == self.device_id
    }

    /// Verify a first-contact identity-binding signature (see
    /// [`super::identity_binding_message`]): that this identity's Ed25519 key
    /// vouches for its X25519 `dh_public_key`. Combined with
    /// [`Self::has_consistent_device_id`] and the handshake challenge/response,
    /// this lets a peer accept the wire-transmitted DH key for a Noise KK session
    /// without prior pairing and without an active MITM being able to swap it.
    pub fn verify_identity_binding(&self, signature_hex: &str) -> Result<bool, String> {
        let public_key_bytes = hex_array::<32>(self.public_key())?;
        let verifying_key =
            VerifyingKey::from_bytes(&public_key_bytes).map_err(|err| err.to_string())?;
        let signature_bytes = decode_hex(signature_hex)?;
        let signature = Signature::from_slice(&signature_bytes).map_err(|err| err.to_string())?;
        let message = identity_binding_message(self.device_id(), self.dh_public_key());

        Ok(verifying_key.verify(&message, &signature).is_ok())
    }

    pub fn verify_handshake_signature(
        &self,
        peer_device_id: &str,
        nonce: &str,
        signature_hex: &str,
    ) -> Result<bool, String> {
        let public_key_bytes = hex_array::<32>(self.public_key())?;
        let verifying_key =
            VerifyingKey::from_bytes(&public_key_bytes).map_err(|err| err.to_string())?;
        let signature_bytes = decode_hex(signature_hex)?;
        let signature = Signature::from_slice(&signature_bytes).map_err(|err| err.to_string())?;
        let challenge = handshake_challenge(self.device_id(), peer_device_id, nonce);

        Ok(verifying_key
            .verify(challenge.as_bytes(), &signature)
            .is_ok())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalIdentity {
    identity: DeviceIdentity,
    signing_key_hex: String,
    static_dh_key_hex: String,
    created_at: SystemTime,
}

impl LocalIdentity {
    pub fn generate(device_name: impl Into<String>, created_at: SystemTime) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let dh_static = DhStaticSecret::random_from_rng(OsRng);

        Self::from_keys(
            device_name,
            signing_key.to_bytes(),
            dh_static.to_bytes(),
            created_at,
        )
    }

    pub fn from_keys(
        device_name: impl Into<String>,
        signing_key_bytes: [u8; 32],
        dh_static_bytes: [u8; 32],
        created_at: SystemTime,
    ) -> Self {
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let public_key_hex = encode_hex(&signing_key.verifying_key().to_bytes());
        let device_id = stable_device_id(&public_key_hex);
        let dh_static = DhStaticSecret::from(dh_static_bytes);
        let dh_public_key_hex = encode_hex(&DhPublicKey::from(&dh_static).to_bytes());

        Self {
            identity: DeviceIdentity::new(
                device_id,
                device_name,
                public_key_hex,
                dh_public_key_hex,
            ),
            signing_key_hex: encode_hex(&signing_key_bytes),
            static_dh_key_hex: encode_hex(&dh_static_bytes),
            created_at,
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

    pub fn public_key(&self) -> &str {
        self.identity.public_key()
    }

    pub fn dh_public_key(&self) -> &str {
        self.identity.dh_public_key()
    }

    pub fn signing_key_hex(&self) -> &str {
        &self.signing_key_hex
    }

    pub fn static_dh_key_hex(&self) -> &str {
        &self.static_dh_key_hex
    }

    pub fn static_dh_key_bytes(&self) -> Result<[u8; 32], String> {
        hex_array::<32>(&self.static_dh_key_hex)
    }

    /// This device's stable v3 onion (`.onion`) address, derived deterministically
    /// from its Ed25519 signing key (via a domain-separated hidden-service key, so
    /// the signing key itself is never reused). Lets an already-paired peer
    /// reconnect over Tor with no signaling server: the address is shared in the
    /// identity exchange and stored in the peer's trust store. The matching
    /// hidden-service secret (needed to *host* the service) is derived from the
    /// same seed by the Tor transport (feature-gated); this returns only the
    /// public address, so it stays in the default build.
    pub fn onion_address(&self) -> Result<String, String> {
        let hs_seed = self.onion_hs_seed()?;
        let hs_public_key = SigningKey::from_bytes(&hs_seed).verifying_key().to_bytes();
        Ok(onion_address_v3(&hs_public_key))
    }

    /// The 32-byte seed for this device's hidden-service key, derived from the
    /// Ed25519 signing key (domain-separated; see [`super::onion`]). The Tor
    /// transport (feature-gated) builds the HS keypair from this to *host* the
    /// onion service at [`Self::onion_address`]. Secret-derived — never share it;
    /// peers only ever receive the public `onion_address`.
    pub fn onion_hs_seed(&self) -> Result<[u8; 32], String> {
        let signing_key_seed = hex_array::<32>(&self.signing_key_hex)?;
        Ok(derive_onion_hs_seed(&signing_key_seed))
    }

    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;

        parse_local_identity(&content)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, format_local_identity(self))
    }

    pub fn load_or_generate(
        path: impl AsRef<Path>,
        device_name: impl Into<String>,
        created_at: SystemTime,
    ) -> io::Result<Self> {
        let path = path.as_ref();

        match Self::load_from_path(path) {
            Ok(identity) => Ok(identity),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let identity = Self::generate(device_name, created_at);
                identity.save_to_path(path)?;
                Ok(identity)
            }
            Err(err) => Err(err),
        }
    }

    pub fn load_from_secure_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        let encrypted = parse_secure_local_identity_file(&content)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let plaintext = unprotect_local_identity_bytes(&encrypted)?;
        let plaintext = String::from_utf8(plaintext)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        parse_local_identity(&plaintext)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn save_to_secure_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let encrypted = protect_local_identity_bytes(format_local_identity(self).as_bytes())?;
        fs::write(path, format_secure_local_identity_file(&encrypted))
    }

    pub fn load_or_generate_secure(
        path: impl AsRef<Path>,
        device_name: impl Into<String>,
        created_at: SystemTime,
    ) -> io::Result<Self> {
        let path = path.as_ref();

        match Self::load_from_secure_path(path) {
            Ok(identity) => Ok(identity),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let identity = Self::generate(device_name, created_at);
                identity.save_to_secure_path(path)?;
                Ok(identity)
            }
            Err(err) => Err(err),
        }
    }

    pub fn sign_handshake_challenge(
        &self,
        peer_device_id: &str,
        nonce: &str,
    ) -> Result<String, String> {
        let signing_key_bytes = hex_array::<32>(&self.signing_key_hex)?;
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let challenge = handshake_challenge(self.device_id(), peer_device_id, nonce);
        let signature = signing_key.sign(challenge.as_bytes());

        Ok(encode_hex(&signature.to_bytes()))
    }

    /// Sign a signaling-server login challenge with this device's Ed25519
    /// identity key. Domain-separated from [`Self::sign_handshake_challenge`]
    /// (which binds two device ids for p2p) so a signature gathered for server
    /// login can never be replayed as a peer handshake, and vice versa. Must
    /// stay byte-for-byte in sync with the server's `auth::challenge_string`
    /// (`signaling-server/src/auth.rs`).
    pub fn sign_signaling_login(&self, nonce: &str) -> Result<String, String> {
        let signing_key_bytes = hex_array::<32>(&self.signing_key_hex)?;
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let challenge = format!("linkhub-signaling-auth-v1\0{}", nonce.trim());
        let signature = signing_key.sign(challenge.as_bytes());

        Ok(encode_hex(&signature.to_bytes()))
    }

    /// Sign this device's own static-key binding (Ed25519 over
    /// `device_id` + X25519 `dh_public_key`, see [`super::identity_binding_message`])
    /// so a peer accepting us at first contact can trust our wire-transmitted DH
    /// key. Domain-separated from handshake/login/SDP signatures. Pairs with
    /// [`DeviceIdentity::verify_identity_binding`].
    pub fn sign_identity_binding(&self) -> Result<String, String> {
        let signing_key_bytes = hex_array::<32>(&self.signing_key_hex)?;
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let message = identity_binding_message(self.device_id(), self.dh_public_key());
        let signature = signing_key.sign(&message);

        Ok(encode_hex(&signature.to_bytes()))
    }

    /// Sign a WebRTC SDP signal (offer/answer) with this device's identity key so
    /// the receiving peer can detect a signaling server tampering with or
    /// substituting the SDP it forwards (connection-redirection, design §7).
    /// Domain-separated from handshake and login signatures (see
    /// [`super::signaling_sdp_message`]); pairs with
    /// [`crate::net::verify_signaling_sdp`] / [`crate::net::seal_sdp`].
    pub fn sign_signaling_sdp(
        &self,
        session_id: &str,
        kind: &str,
        sdp: &str,
    ) -> Result<String, String> {
        let signing_key_bytes = hex_array::<32>(&self.signing_key_hex)?;
        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let signature = signing_key.sign(&signaling_sdp_message(session_id, kind, sdp));

        Ok(encode_hex(&signature.to_bytes()))
    }
}

fn stable_device_id(public_key_hex: &str) -> String {
    let digest = sha256_hex(public_key_hex.as_bytes());

    format!("lh-{}", &digest[..16])
}

fn required_field<'a>(fields: &'a HashMap<&str, &'a str>, key: &str) -> Result<&'a str, String> {
    fields
        .get(key)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing local identity field: {key}"))
}

fn hex_array<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let bytes = decode_hex(value)?;

    bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!("hex value must decode to {N} bytes, got {}", bytes.len())
    })
}

fn format_local_identity(identity: &LocalIdentity) -> String {
    [
        LOCAL_IDENTITY_HEADER.to_string(),
        format!("device_id={}", encode_hex(identity.device_id().as_bytes())),
        format!(
            "device_name={}",
            encode_hex(identity.device_name().as_bytes())
        ),
        format!("public_key={}", identity.public_key()),
        format!("dh_key={}", identity.static_dh_key_hex()),
        format!("signing_key={}", identity.signing_key_hex()),
        format!(
            "created_at={}",
            system_time_to_unix_seconds(identity.created_at())
        ),
        String::new(),
    ]
    .join("\n")
}

fn parse_local_identity(value: &str) -> Result<LocalIdentity, String> {
    let mut lines = value.lines();
    let Some(header) = lines.next() else {
        return Err("missing local identity header".to_string());
    };

    if header.trim().trim_start_matches('\u{feff}') != LOCAL_IDENTITY_HEADER {
        return Err(format!("invalid local identity header: {header}"));
    }

    let mut fields = HashMap::new();
    for (index, line) in lines.enumerate() {
        let line_number = index + 2;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid local identity line {line_number}: {line}"));
        };

        fields.insert(key, value);
    }

    let device_id = decode_hex_string(required_field(&fields, "device_id")?)?;
    let device_name = decode_hex_string(required_field(&fields, "device_name")?)?;
    let public_key = required_field(&fields, "public_key")?.to_string();
    let dh_key_hex = required_field(&fields, "dh_key")
        .map(|v| v.to_string())
        .map_err(|err| {
            format!("{err} — reinitialize with `identity init` to generate X25519 DH key")
        })?;
    let signing_key_hex = required_field(&fields, "signing_key")?.to_string();
    let created_at = UNIX_EPOCH
        + Duration::from_secs(
            required_field(&fields, "created_at")?
                .parse::<u64>()
                .map_err(|_| "invalid local identity created_at".to_string())?,
        );
    let signing_key_bytes = decode_hex(&signing_key_hex)?;
    let signing_key_bytes: [u8; 32] = signing_key_bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("signing key must be 32 bytes, got {}", bytes.len()))?;
    let dh_key_bytes = decode_hex(&dh_key_hex)?;
    let dh_key_bytes: [u8; 32] = dh_key_bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("dh key must be 32 bytes, got {}", bytes.len()))?;
    let expected = LocalIdentity::from_keys(
        device_name.clone(),
        signing_key_bytes,
        dh_key_bytes,
        created_at,
    );

    if device_id != expected.device_id() {
        return Err("local identity device_id does not match signing key".to_string());
    }

    if public_key != expected.public_key() {
        return Err("local identity public_key does not match signing key".to_string());
    }

    if dh_key_hex != expected.static_dh_key_hex() {
        return Err("local identity dh_key does not match X25519 static secret".to_string());
    }

    Ok(expected)
}

#[cfg(test)]
mod first_contact_binding_tests {
    //! The crypto that makes AirDrop-style first contact safe without pairing:
    //! a device's Ed25519 key signs its X25519 DH key, and its device_id is the
    //! hash of its Ed25519 key — so a wire-transmitted identity cannot be forged
    //! or have its DH key swapped by an active MITM.

    use super::*;

    #[test]
    fn identity_binding_round_trips_and_rejects_dh_swap() {
        let now = SystemTime::now();
        let alice = LocalIdentity::generate("Alice", now);
        let mallory = LocalIdentity::generate("Mallory", now);

        let sig = alice.sign_identity_binding().unwrap();

        // Alice's own identity verifies the binding she signed.
        assert!(alice.identity().verify_identity_binding(&sig).unwrap());

        // Alice's device_id + Ed25519 key but Mallory's DH key swapped in must
        // FAIL the binding check — this is the man-in-the-middle defense.
        let swapped = DeviceIdentity::new(
            alice.device_id(),
            alice.device_name(),
            alice.public_key(),
            mallory.dh_public_key(),
        );
        assert!(!swapped.verify_identity_binding(&sig).unwrap());
    }

    #[test]
    fn detects_inconsistent_device_id() {
        let now = SystemTime::now();
        let alice = LocalIdentity::generate("Alice", now);
        assert!(alice.identity().has_consistent_device_id());

        // A forged identity claiming a device_id not derived from its own key.
        let forged = DeviceIdentity::new(
            "lh-0000000000000000",
            alice.device_name(),
            alice.public_key(),
            alice.dh_public_key(),
        );
        assert!(!forged.has_consistent_device_id());
    }
}

#[cfg(test)]
mod onion_address_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn onion_address_is_stable_and_well_formed() {
        let created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(7);
        let identity = LocalIdentity::from_keys("PC", [7u8; 32], [0u8; 32], created_at);

        let address = identity.onion_address().unwrap();
        // Same identity always yields the same address (carried + stored once).
        let rebuilt = LocalIdentity::from_keys("PC", [7u8; 32], [0u8; 32], created_at);
        assert_eq!(address, rebuilt.onion_address().unwrap());

        assert!(address.ends_with(".onion"));
        assert_eq!(address.trim_end_matches(".onion").len(), 56);
    }

    #[test]
    fn onion_address_differs_for_different_signing_keys() {
        let now = SystemTime::UNIX_EPOCH;
        let a = LocalIdentity::from_keys("PC", [7u8; 32], [0u8; 32], now);
        let b = LocalIdentity::from_keys("PC", [9u8; 32], [0u8; 32], now);
        assert_ne!(a.onion_address().unwrap(), b.onion_address().unwrap());
    }

    #[test]
    fn device_identity_with_onion_address_trims_and_drops_empty() {
        let base = DeviceIdentity::new("lh-x", "Name", "aa", "bb");
        assert_eq!(base.onion_address(), None);

        // Whitespace-only is normalized away to `None`.
        assert_eq!(
            base.clone()
                .with_onion_address(Some("   ".to_string()))
                .onion_address(),
            None
        );

        // A real address is trimmed and stored.
        let onion = "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead.onion";
        assert_eq!(
            base.with_onion_address(Some(format!("  {onion}  ")))
                .onion_address(),
            Some(onion)
        );
    }
}
