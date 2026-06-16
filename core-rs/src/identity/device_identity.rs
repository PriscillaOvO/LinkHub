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

use super::secure_store::{
    format_secure_local_identity_file, parse_secure_local_identity_file,
    protect_local_identity_bytes, unprotect_local_identity_bytes,
};
use super::{
    decode_hex, decode_hex_string, encode_hex, grouped_uppercase, handshake_challenge, sha256_hex,
    system_time_to_unix_seconds, LOCAL_IDENTITY_HEADER,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceIdentity {
    device_id: String,
    device_name: String,
    public_key: String,
    dh_public_key: String,
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
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
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
        let dh_static = DhStaticSecret::random_from_rng(&mut OsRng);

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
