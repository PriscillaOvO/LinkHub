//! The [`TrustStore`]: a persisted set of [`TrustedDevice`]s keyed by device id,
//! with its line-oriented text encoding.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::{
    decode_hex_string, encode_hex, system_time_to_unix_seconds, DeviceIdentity, TrustedDevice,
    TRUST_STORE_HEADER,
};

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
