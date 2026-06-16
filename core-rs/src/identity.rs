use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as DhPublicKey, StaticSecret as DhStaticSecret};

use crate::device::DeviceNode;

const LOCAL_IDENTITY_HEADER: &str = "linkhub_local_identity_v1";
const SECURE_LOCAL_IDENTITY_HEADER: &str = "linkhub_secure_local_identity_v1";
const SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI: &str = "windows-dpapi-user";
const SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN: &str = "macos-keychain";
const SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE: &str = "linux-secret-service";
const HANDSHAKE_CHALLENGE_HEADER: &str = "linkhub-auth-v1";
const PAIRING_PAYLOAD_HEADER: &str = "linkhub-pair-v1";
const TRUST_STORE_HEADER: &str = "linkhub_trust_store_v1";

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
        confirmation_code(
            &self.local_identity,
            self.invitation.identity(),
            self.invitation.nonce(),
        )
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

#[derive(Debug, Default)]
pub struct TrustStore {
    trusted_devices: HashMap<String, TrustedDevice>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trust(&mut self, device: TrustedDevice) {
        self.trusted_devices
            .insert(device.device_id().to_string(), device);
    }

    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.trusted_devices.contains_key(device_id)
    }

    pub fn trusted_device(&self, device_id: &str) -> Option<&TrustedDevice> {
        self.trusted_devices.get(device_id)
    }

    pub fn trusted_devices(&self) -> Vec<&TrustedDevice> {
        let mut devices = self.trusted_devices.values().collect::<Vec<_>>();
        devices.sort_by(|left, right| left.device_id().cmp(right.device_id()));
        devices
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();

        match fs::read_to_string(path) {
            Ok(content) => parse_trust_store(&content)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(err) => Err(err),
        }
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, format_trust_store(self))
    }
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

fn format_secure_local_identity_file(encrypted_bytes: &[u8]) -> String {
    let platform = secure_platform_label();
    [
        SECURE_LOCAL_IDENTITY_HEADER.to_string(),
        format!("platform={platform}"),
        format!("ciphertext={}", encode_hex(encrypted_bytes)),
    ]
    .join("\n")
}

fn secure_platform_label() -> &'static str {
    #[cfg(windows)]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
    }
    #[cfg(target_os = "macos")]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN
    }
    #[cfg(target_os = "linux")]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
    } // fallback for unknown platforms, will fail at runtime
}

fn parse_secure_local_identity_file(value: &str) -> Result<Vec<u8>, String> {
    let mut lines = value.lines();
    let Some(header) = lines.next() else {
        return Err("missing secure local identity header".to_string());
    };

    if header.trim_start_matches('\u{feff}') != SECURE_LOCAL_IDENTITY_HEADER {
        return Err("unsupported secure local identity file".to_string());
    }

    let fields = lines
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect::<HashMap<_, _>>();
    let platform = fields
        .get("platform")
        .ok_or_else(|| "secure local identity missing platform".to_string())?;

    if platform != SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
        && platform != SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN
        && platform != SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE
    {
        return Err(format!(
            "unsupported secure local identity platform: {platform}"
        ));
    }

    let ciphertext = fields
        .get("ciphertext")
        .ok_or_else(|| "secure local identity missing ciphertext".to_string())?;

    decode_hex(ciphertext)
}

#[cfg(windows)]
fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len().try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "local identity is too large")
        })?,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        CryptProtectData(
            &mut input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let encrypted = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let encrypted = slice.to_vec();
        LocalFree(output.pbData as *mut _);
        encrypted
    };

    Ok(encrypted)
}

#[cfg(target_os = "macos")]
fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    let hash_prefix = &sha256_hex(plaintext)[..8];
    let service = format!("linkhub-identity-{hash_prefix}");
    let account = "linkhub-local-identity";
    security_framework::passwords::set_generic_password(&service, account, plaintext)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("keychain write: {err}")))?;
    let ref_json = serde_json::json!({"service": service, "account": account}).to_string();
    Ok(ref_json.into_bytes())
}

#[cfg(target_os = "macos")]
fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    let ref_str = std::str::from_utf8(encrypted)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let v: serde_json::Value =
        serde_json::from_str(ref_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let service = v["service"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing service"))?;
    let account = v["account"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing account"))?;
    let (password, _) = security_framework::passwords::get_generic_password(service, account)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("keychain read: {err}")))?;
    Ok(password)
}

#[cfg(target_os = "linux")]
fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    // Linux: store via Secret Service using async block_on
    let rt = tokio::runtime::Runtime::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    rt.block_on(async {
        let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let collection = ss
            .get_default_collection()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let hash_prefix = &sha256_hex(plaintext)[..8];
        let label = format!("linkhub-identity-{hash_prefix}");
        let mut props = std::collections::HashMap::new();
        props.insert("application", "linkhub-desktop");
        let item = collection
            .create_item(&label, props, plaintext, false, "text/plain")
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let path = item.get_path().unwrap_or_default();
        let ref_json = serde_json::json!({"item_path": path}).to_string();
        Ok(ref_json.into_bytes())
    })
}

#[cfg(target_os = "linux")]
fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    let ref_str = std::str::from_utf8(encrypted)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let v: serde_json::Value =
        serde_json::from_str(ref_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let item_path = v["item_path"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing item_path"))?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    rt.block_on(async {
        let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut props = std::collections::HashMap::new();
        props.insert("path", item_path);
        let items = ss
            .search_items(props)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let item = items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "secret not found"))?;
        let secret = item
            .get_secret()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(secret)
    })
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn protect_local_identity_bytes(_plaintext: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure local identity storage is not available on this platform",
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn unprotect_local_identity_bytes(_encrypted: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure local identity storage is not available on this platform",
    ))
}

#[cfg(windows)]
fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len().try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "secure local identity is too large",
            )
        })?,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        CryptUnprotectData(
            &mut input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let plaintext = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let plaintext = slice.to_vec();
        LocalFree(output.pbData as *mut _);
        plaintext
    };

    Ok(plaintext)
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

fn format_trust_store(store: &TrustStore) -> String {
    let mut lines = vec![TRUST_STORE_HEADER.to_string()];

    for device in store.trusted_devices() {
        lines.push(format!(
            "device={}|{}|{}|{}|{}",
            encode_hex(device.device_id().as_bytes()),
            encode_hex(device.device_name().as_bytes()),
            encode_hex(device.identity().public_key().as_bytes()),
            encode_hex(device.identity().dh_public_key().as_bytes()),
            system_time_to_unix_seconds(device.paired_at())
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn parse_trust_store(value: &str) -> Result<TrustStore, String> {
    let mut lines = value.lines();
    let Some(header) = lines.next() else {
        return Ok(TrustStore::new());
    };

    if header.trim().trim_start_matches('\u{feff}') != TRUST_STORE_HEADER {
        return Err(format!("invalid trust store header: {header}"));
    }

    let mut store = TrustStore::new();

    for (index, line) in lines.enumerate() {
        let line_number = index + 2;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let Some(record) = line.strip_prefix("device=") else {
            return Err(format!("invalid trust store line {line_number}: {line}"));
        };
        let fields = record.split('|').collect::<Vec<_>>();

        if fields.len() != 5 {
            return Err(format!(
                "invalid trust store device field count ({}) on line {line_number}; \
                 expected 5 fields (device_id, device_name, public_key, dh_public_key, paired_at)",
                fields.len()
            ));
        }

        let device_id = decode_hex_string(fields[0])
            .map_err(|err| format!("invalid device id on line {line_number}: {err}"))?;
        let device_name = decode_hex_string(fields[1])
            .map_err(|err| format!("invalid device name on line {line_number}: {err}"))?;
        let public_key = decode_hex_string(fields[2])
            .map_err(|err| format!("invalid public key on line {line_number}: {err}"))?;
        let dh_public_key = decode_hex_string(fields[3])
            .map_err(|err| format!("invalid dh_public_key on line {line_number}: {err}"))?;
        let paired_at_seconds = fields[4]
            .parse::<u64>()
            .map_err(|_| format!("invalid paired_at timestamp on line {line_number}"))?;

        store.trust(TrustedDevice::new(
            DeviceIdentity::new(device_id, device_name, public_key, dh_public_key),
            UNIX_EPOCH + Duration::from_secs(paired_at_seconds),
        ));
    }

    Ok(store)
}

fn confirmation_code(local: &DeviceIdentity, peer: &DeviceIdentity, nonce: &str) -> String {
    let mut fingerprints = [local.fingerprint(), peer.fingerprint()];
    fingerprints.sort();
    let digest = sha256_hex(
        format!("{}\0{}\0{}", fingerprints[0], fingerprints[1], nonce.trim()).as_bytes(),
    );

    grouped_uppercase(&digest[..6], 3)
}

fn stable_device_id(public_key_hex: &str) -> String {
    let digest = sha256_hex(public_key_hex.as_bytes());

    format!("lh-{}", &digest[..16])
}

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

fn required_field<'a>(fields: &'a HashMap<&str, &'a str>, key: &str) -> Result<&'a str, String> {
    fields
        .get(key)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing local identity field: {key}"))
}

fn normalize_pairing_code(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
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

fn hex_array<const N: usize>(value: &str) -> Result<[u8; N], String> {
    let bytes = decode_hex(value)?;

    bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!("hex value must decode to {N} bytes, got {}", bytes.len())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
