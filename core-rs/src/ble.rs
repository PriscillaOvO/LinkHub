//! Bluetooth Low Energy (BLE) discovery and control channel.
//!
//! Defines the BLE GATT service for LinkHub device advertisement
//! and a platform-agnostic scanner trait.

use serde::{Deserialize, Serialize};

/// LinkHub BLE Service UUID (custom 128-bit).
pub const LINKHUB_BLE_SERVICE_UUID: &str = "0000a001-0000-1000-8000-00805f9b34fb";

/// BLE characteristics under the LinkHub service.
pub mod characteristics {
    pub const DEVICE_ID: &str = "0000a002-0000-1000-8000-00805f9b34fb";
    pub const DEVICE_NAME: &str = "0000a003-0000-1000-8000-00805f9b34fb";
    pub const FINGERPRINT: &str = "0000a004-0000-1000-8000-00805f9b34fb";
    pub const TCP_PORT: &str = "0000a005-0000-1000-8000-00805f9b34fb";
    pub const STATUS: &str = "0000a006-0000-1000-8000-00805f9b34fb";
}

/// A device discovered via BLE scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BleDiscovery {
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
    pub tcp_port: u16,
    pub rssi: i16,
    pub status: BleStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BleStatus {
    Available,
    Busy,
    Sleeping,
}

/// Errors that can occur during BLE operations.
#[derive(Clone, Debug)]
pub enum BleError {
    NotSupported,
    BluetoothOff,
    PermissionDenied,
    ScanTimeout,
    ConnectionFailed(String),
}

/// Platform-agnostic BLE scanner trait.
/// Each platform implements this using its native BLE stack.
pub trait BleScanner: Send {
    fn start_scan(&mut self) -> Result<(), BleError>;
    fn stop_scan(&mut self);
    fn discovered_devices(&self) -> Vec<BleDiscovery>;
}

/// Placeholder scanner that returns empty — used when BLE is unavailable.
pub struct NoopBleScanner;

impl BleScanner for NoopBleScanner {
    fn start_scan(&mut self) -> Result<(), BleError> {
        Err(BleError::NotSupported)
    }
    fn stop_scan(&mut self) {}
    fn discovered_devices(&self) -> Vec<BleDiscovery> {
        vec![]
    }
}

/// Convert RSSI to approximate distance in meters.
pub fn rssi_to_distance_meters(rssi: i16, tx_power: i16) -> f64 {
    if rssi == 0 {
        return -1.0;
    }
    let ratio = (rssi as f64) / (tx_power as f64);
    if ratio < 1.0 {
        ratio.powi(10)
    } else {
        (0.89976) * ratio.powi(7).powf(0.111)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_scanner_returns_empty() {
        let mut scanner = NoopBleScanner;
        assert!(scanner.start_scan().is_err());
        assert!(scanner.discovered_devices().is_empty());
    }

    #[test]
    fn ble_service_uuid_is_128_bit() {
        assert_eq!(LINKHUB_BLE_SERVICE_UUID.len(), 36);
        assert!(LINKHUB_BLE_SERVICE_UUID.starts_with("0000a001"));
    }

    #[test]
    fn rssi_distance_estimation() {
        let dist = rssi_to_distance_meters(-60, -59);
        assert!(dist > 0.5 && dist < 5.0);
    }
}
