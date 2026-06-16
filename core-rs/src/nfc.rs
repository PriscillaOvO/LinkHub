//! NFC pairing — NDEF payload format for quick device pairing via tap.
//!
//! An NFC tag or peer-to-peer NDEF message carries a LinkHub pairing
//! payload, enabling "tap to pair" without scanning a QR code.

/// NFC record type for LinkHub pairing.
pub const LINKHUB_NFC_RECORD_TYPE: &str = "linkhub.com:pair";

/// Wraps a pairing payload in an NDEF-like structure.
/// The payload is the standard `linkhub-pair-v1|...` string.
#[derive(Clone, Debug)]
pub struct NfcPairingRecord {
    pub payload: String, // linkhub-pair-v1|...
    pub ttl_seconds: u64,
}

impl NfcPairingRecord {
    pub fn new(payload: impl Into<String>, ttl_seconds: u64) -> Self {
        Self {
            payload: payload.into(),
            ttl_seconds,
        }
    }

    /// Serialize to an NDEF message (simplified format for prototyping).
    /// Real NDEF encoding would add TNF + type length + payload length headers.
    pub fn to_ndef_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Simplified NDEF record:
        // [TNF=0x04 (NFC RTD External Type)] [type_len] [type] [payload_len] [payload]
        let type_name = LINKHUB_NFC_RECORD_TYPE.as_bytes();
        let payload_bytes = self.payload.as_bytes();
        bytes.push(0x04); // TNF: External Type
        bytes.push(type_name.len() as u8);
        bytes.extend_from_slice(type_name);
        bytes.push((payload_bytes.len() >> 8) as u8);
        bytes.push((payload_bytes.len() & 0xFF) as u8);
        bytes.extend_from_slice(payload_bytes);
        bytes
    }

    /// Parse an NDEF-encoded pairing record.
    pub fn from_ndef_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("NDEF record too short".into());
        }
        let tnf = data[0];
        if tnf != 0x04 {
            return Err(format!("unexpected TNF: {tnf}"));
        }
        let type_len = data[1] as usize;
        if data.len() < 2 + type_len + 2 {
            return Err("truncated NDEF header".into());
        }
        let type_start = 2;
        let type_end = type_start + type_len;
        let record_type =
            std::str::from_utf8(&data[type_start..type_end]).map_err(|e| format!("{e}"))?;
        if record_type != LINKHUB_NFC_RECORD_TYPE {
            return Err(format!("unexpected record type: {record_type}"));
        }
        let payload_len = ((data[type_end] as usize) << 8) | (data[type_end + 1] as usize);
        let payload_start = type_end + 2;
        if data.len() < payload_start + payload_len {
            return Err("truncated NDEF payload".into());
        }
        let payload = std::str::from_utf8(&data[payload_start..payload_start + payload_len])
            .map_err(|e| format!("{e}"))?
            .to_string();
        Ok(Self {
            payload,
            ttl_seconds: 120,
        }) // TTL not encoded in NDEF; caller sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndef_pairing_record_round_trips() {
        let record = NfcPairingRecord::new("linkhub-pair-v1|abcdef|...", 60);
        let bytes = record.to_ndef_bytes();
        let parsed = NfcPairingRecord::from_ndef_bytes(&bytes).unwrap();
        assert_eq!(parsed.payload, "linkhub-pair-v1|abcdef|...");
    }

    #[test]
    fn ndef_parse_rejects_invalid_record() {
        assert!(NfcPairingRecord::from_ndef_bytes(&[]).is_err());
        assert!(NfcPairingRecord::from_ndef_bytes(&[0x01, 0, 0, 0]).is_err());
    }
}
