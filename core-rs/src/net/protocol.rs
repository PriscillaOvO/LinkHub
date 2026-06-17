use crate::{HeartbeatUpdate, TransportKind};

use super::LocalDevice;

#[derive(Debug, Eq, PartialEq)]
pub(in crate::net) enum WireMessage {
    Hello {
        device_id: String,
        name: String,
    },
    AuthChallenge {
        nonce: String,
    },
    AuthSignature {
        device_id: String,
        signature_hex: String,
    },
    NoiseHs {
        payload_hex: String,
    },
    Heartbeat(HeartbeatUpdate),
    Text {
        message_id: String,
        content: String,
    },
    FileStart {
        transfer_id: String,
        filename: String,
        size_bytes: u64,
        sha256_hex: Option<String>,
    },
    FileChunk {
        transfer_id: String,
        chunk_index: u64,
        data_hex: String,
    },
    FileEnd {
        transfer_id: String,
    },
    Ack {
        message_id: String,
        status: String,
    },
    // ── WebRTC Signaling (Stage 5) ──
    Signaling {
        session_id: String,
        kind: String, // "offer" | "answer" | "ice-candidate" | "done" | "error"
        payload_hex: String,
    },
    // ── Relay / Mesh (Stage 6) ──
    RelayRequest {
        session_id: String,
        target_device: String,
    },
    RelayResponse {
        session_id: String,
        accepted: bool,
        reason: String,
    },
    RelayForward {
        session_id: String,
        from_device: String,
        to_device: String,
        payload_hex: String,
    },
    // ── Media Call (Stage 7) ──
    CallInvite {
        call_id: String,
        media_kind: String, // "audio" | "video" | "both"
    },
    CallAccept {
        call_id: String,
    },
    CallReject {
        call_id: String,
    },
    CallEnd {
        call_id: String,
    },
    // ── Media Sync (Stage 8) ──
    MediaControl {
        session_id: String,
        command: String, // "play" | "pause" | "seek" | "volume" | "switch"
        payload_hex: String,
    },
}

// Some constructors cover planned but not-yet-wired protocol messages
// (signaling/relay/call/media). Keeping them next to parsing/serialization
// preserves the wire contract while those features are staged.
#[allow(dead_code)]
impl WireMessage {
    pub(in crate::net) fn hello(local: &LocalDevice) -> Self {
        WireMessage::Hello {
            device_id: sanitize_field(&local.id),
            name: sanitize_field(&local.name),
        }
    }

    pub(in crate::net) fn heartbeat() -> Self {
        WireMessage::Heartbeat(HeartbeatUpdate {
            transport: TransportKind::LanTcp,
            latency_ms: 1,
            bandwidth_score: 300,
            battery_cost: 8,
            metered_cost: 0,
        })
    }

    pub(in crate::net) fn auth_challenge(nonce: &str) -> Self {
        WireMessage::AuthChallenge {
            nonce: sanitize_field(nonce),
        }
    }

    pub(in crate::net) fn auth_signature(device_id: &str, signature_hex: &str) -> Self {
        WireMessage::AuthSignature {
            device_id: sanitize_field(device_id),
            signature_hex: sanitize_field(signature_hex),
        }
    }

    pub(in crate::net) fn noise_hs(payload_hex: &str) -> Self {
        WireMessage::NoiseHs {
            payload_hex: sanitize_field(payload_hex),
        }
    }

    pub(in crate::net) fn text(message_id: &str, content: &str) -> Self {
        WireMessage::Text {
            message_id: sanitize_field(message_id),
            content: sanitize_field(content),
        }
    }

    pub(in crate::net) fn file_start(transfer_id: &str, filename: &str, size_bytes: u64) -> Self {
        WireMessage::FileStart {
            transfer_id: sanitize_field(transfer_id),
            filename: sanitize_field(filename),
            size_bytes,
            sha256_hex: None,
        }
    }

    pub(in crate::net) fn file_start_with_hash(
        transfer_id: &str,
        filename: &str,
        size_bytes: u64,
        sha256_hex: &str,
    ) -> Self {
        WireMessage::FileStart {
            transfer_id: sanitize_field(transfer_id),
            filename: sanitize_field(filename),
            size_bytes,
            sha256_hex: Some(sanitize_field(sha256_hex)),
        }
    }

    pub(in crate::net) fn file_chunk(transfer_id: &str, chunk_index: u64, data_hex: &str) -> Self {
        WireMessage::FileChunk {
            transfer_id: sanitize_field(transfer_id),
            chunk_index,
            data_hex: sanitize_field(data_hex),
        }
    }

    pub(in crate::net) fn file_end(transfer_id: &str) -> Self {
        WireMessage::FileEnd {
            transfer_id: sanitize_field(transfer_id),
        }
    }

    pub(in crate::net) fn ack(message_id: &str, status: &str) -> Self {
        WireMessage::Ack {
            message_id: sanitize_field(message_id),
            status: sanitize_field(status),
        }
    }

    pub(in crate::net) fn signaling(session_id: &str, kind: &str, payload_hex: &str) -> Self {
        WireMessage::Signaling {
            session_id: sanitize_field(session_id),
            kind: sanitize_field(kind),
            payload_hex: sanitize_field(payload_hex),
        }
    }

    pub(in crate::net) fn relay_request(session_id: &str, target_device: &str) -> Self {
        WireMessage::RelayRequest {
            session_id: sanitize_field(session_id),
            target_device: sanitize_field(target_device),
        }
    }

    pub(in crate::net) fn relay_response(session_id: &str, accepted: bool, reason: &str) -> Self {
        WireMessage::RelayResponse {
            session_id: sanitize_field(session_id),
            accepted,
            reason: sanitize_field(reason),
        }
    }

    pub(in crate::net) fn relay_forward(
        session_id: &str,
        from_device: &str,
        to_device: &str,
        payload_hex: &str,
    ) -> Self {
        WireMessage::RelayForward {
            session_id: sanitize_field(session_id),
            from_device: sanitize_field(from_device),
            to_device: sanitize_field(to_device),
            payload_hex: sanitize_field(payload_hex),
        }
    }

    pub(in crate::net) fn call_invite(call_id: &str, media_kind: &str) -> Self {
        WireMessage::CallInvite {
            call_id: sanitize_field(call_id),
            media_kind: sanitize_field(media_kind),
        }
    }

    pub(in crate::net) fn call_accept(call_id: &str) -> Self {
        WireMessage::CallAccept {
            call_id: sanitize_field(call_id),
        }
    }

    pub(in crate::net) fn call_reject(call_id: &str) -> Self {
        WireMessage::CallReject {
            call_id: sanitize_field(call_id),
        }
    }

    pub(in crate::net) fn call_end(call_id: &str) -> Self {
        WireMessage::CallEnd {
            call_id: sanitize_field(call_id),
        }
    }

    pub(in crate::net) fn media_control(
        session_id: &str,
        command: &str,
        payload_hex: &str,
    ) -> Self {
        WireMessage::MediaControl {
            session_id: sanitize_field(session_id),
            command: sanitize_field(command),
            payload_hex: sanitize_field(payload_hex),
        }
    }
}

pub(in crate::net) fn serialize_message(message: &WireMessage) -> String {
    match message {
        WireMessage::Hello { device_id, name } => format!("HELLO\t{device_id}\t{name}"),
        WireMessage::AuthChallenge { nonce } => format!("AUTH_CHALLENGE\t{nonce}"),
        WireMessage::AuthSignature {
            device_id,
            signature_hex,
        } => {
            format!("AUTH_SIGNATURE\t{device_id}\t{signature_hex}")
        }
        WireMessage::NoiseHs { payload_hex } => format!("NOISE_HS\t{payload_hex}"),
        WireMessage::Heartbeat(update) => format!(
            "HEARTBEAT\t{}\t{}\t{}\t{}\t{}",
            update.transport,
            update.latency_ms,
            update.bandwidth_score,
            update.battery_cost,
            update.metered_cost
        ),
        WireMessage::Text {
            message_id,
            content,
        } => format!("TEXT\t{message_id}\t{content}"),
        WireMessage::FileStart {
            transfer_id,
            filename,
            size_bytes,
            sha256_hex,
        } => match sha256_hex {
            Some(sha256_hex) => {
                format!("FILE_START\t{transfer_id}\t{filename}\t{size_bytes}\t{sha256_hex}")
            }
            None => format!("FILE_START\t{transfer_id}\t{filename}\t{size_bytes}"),
        },
        WireMessage::FileChunk {
            transfer_id,
            chunk_index,
            data_hex,
        } => format!("FILE_CHUNK\t{transfer_id}\t{chunk_index}\t{data_hex}"),
        WireMessage::FileEnd { transfer_id } => format!("FILE_END\t{transfer_id}"),
        WireMessage::Ack { message_id, status } => format!("ACK\t{message_id}\t{status}"),
        WireMessage::Signaling {
            session_id,
            kind,
            payload_hex,
        } => {
            format!("SIGNALING\t{session_id}\t{kind}\t{payload_hex}")
        }
        WireMessage::RelayRequest {
            session_id,
            target_device,
        } => {
            format!("RELAY_REQUEST\t{session_id}\t{target_device}")
        }
        WireMessage::RelayResponse {
            session_id,
            accepted,
            reason,
        } => {
            format!("RELAY_RESPONSE\t{session_id}\t{accepted}\t{reason}")
        }
        WireMessage::RelayForward {
            session_id,
            from_device,
            to_device,
            payload_hex,
        } => {
            format!("RELAY_FORWARD\t{session_id}\t{from_device}\t{to_device}\t{payload_hex}")
        }
        WireMessage::CallInvite {
            call_id,
            media_kind,
        } => {
            format!("CALL_INVITE\t{call_id}\t{media_kind}")
        }
        WireMessage::CallAccept { call_id } => format!("CALL_ACCEPT\t{call_id}"),
        WireMessage::CallReject { call_id } => format!("CALL_REJECT\t{call_id}"),
        WireMessage::CallEnd { call_id } => format!("CALL_END\t{call_id}"),
        WireMessage::MediaControl {
            session_id,
            command,
            payload_hex,
        } => {
            format!("MEDIA_CONTROL\t{session_id}\t{command}\t{payload_hex}")
        }
    }
}

pub(in crate::net) fn parse_message(line: &str) -> Result<WireMessage, String> {
    let parts = line.split('\t').collect::<Vec<_>>();

    match parts.as_slice() {
        ["HELLO", device_id, name] if !device_id.is_empty() && !name.is_empty() => {
            Ok(WireMessage::Hello {
                device_id: (*device_id).to_string(),
                name: (*name).to_string(),
            })
        }
        ["AUTH_CHALLENGE", nonce] if !nonce.is_empty() => Ok(WireMessage::AuthChallenge {
            nonce: (*nonce).to_string(),
        }),
        ["AUTH_SIGNATURE", device_id, signature_hex]
            if !device_id.is_empty() && !signature_hex.is_empty() =>
        {
            Ok(WireMessage::AuthSignature {
                device_id: (*device_id).to_string(),
                signature_hex: (*signature_hex).to_string(),
            })
        }
        ["NOISE_HS", payload_hex] if !payload_hex.is_empty() => Ok(WireMessage::NoiseHs {
            payload_hex: (*payload_hex).to_string(),
        }),
        ["HEARTBEAT", transport, latency_ms, bandwidth_score, battery_cost, metered_cost] => {
            Ok(WireMessage::Heartbeat(HeartbeatUpdate {
                transport: transport.parse()?,
                latency_ms: parse_u32(latency_ms, "latency_ms")?,
                bandwidth_score: parse_u32(bandwidth_score, "bandwidth_score")?,
                battery_cost: parse_u32(battery_cost, "battery_cost")?,
                metered_cost: parse_u32(metered_cost, "metered_cost")?,
            }))
        }
        ["TEXT", message_id, content] if !message_id.is_empty() && !content.is_empty() => {
            Ok(WireMessage::Text {
                message_id: (*message_id).to_string(),
                content: (*content).to_string(),
            })
        }
        ["FILE_START", transfer_id, filename, size_bytes]
            if !transfer_id.is_empty() && !filename.is_empty() =>
        {
            Ok(WireMessage::FileStart {
                transfer_id: (*transfer_id).to_string(),
                filename: (*filename).to_string(),
                size_bytes: parse_u64(size_bytes, "size_bytes")?,
                sha256_hex: None,
            })
        }
        ["FILE_START", transfer_id, filename, size_bytes, sha256_hex]
            if !transfer_id.is_empty() && !filename.is_empty() && !sha256_hex.is_empty() =>
        {
            Ok(WireMessage::FileStart {
                transfer_id: (*transfer_id).to_string(),
                filename: (*filename).to_string(),
                size_bytes: parse_u64(size_bytes, "size_bytes")?,
                sha256_hex: Some((*sha256_hex).to_string()),
            })
        }
        ["FILE_CHUNK", transfer_id, chunk_index, data_hex]
            if !transfer_id.is_empty() && !data_hex.is_empty() =>
        {
            Ok(WireMessage::FileChunk {
                transfer_id: (*transfer_id).to_string(),
                chunk_index: parse_u64(chunk_index, "chunk_index")?,
                data_hex: (*data_hex).to_string(),
            })
        }
        ["FILE_END", transfer_id] if !transfer_id.is_empty() => Ok(WireMessage::FileEnd {
            transfer_id: (*transfer_id).to_string(),
        }),
        ["ACK", message_id, status] if !message_id.is_empty() && !status.is_empty() => {
            Ok(WireMessage::Ack {
                message_id: (*message_id).to_string(),
                status: (*status).to_string(),
            })
        }
        ["SIGNALING", session_id, kind, payload_hex]
            if !session_id.is_empty() && !kind.is_empty() =>
        {
            Ok(WireMessage::Signaling {
                session_id: (*session_id).to_string(),
                kind: (*kind).to_string(),
                payload_hex: (*payload_hex).to_string(),
            })
        }
        ["RELAY_REQUEST", session_id, target_device]
            if !session_id.is_empty() && !target_device.is_empty() =>
        {
            Ok(WireMessage::RelayRequest {
                session_id: (*session_id).to_string(),
                target_device: (*target_device).to_string(),
            })
        }
        ["RELAY_RESPONSE", session_id, accepted, reason] if !session_id.is_empty() => {
            Ok(WireMessage::RelayResponse {
                session_id: (*session_id).to_string(),
                accepted: *accepted == "true",
                reason: (*reason).to_string(),
            })
        }
        ["RELAY_FORWARD", session_id, from_device, to_device, payload_hex]
            if !session_id.is_empty() =>
        {
            Ok(WireMessage::RelayForward {
                session_id: (*session_id).to_string(),
                from_device: (*from_device).to_string(),
                to_device: (*to_device).to_string(),
                payload_hex: (*payload_hex).to_string(),
            })
        }
        ["CALL_INVITE", call_id, media_kind] if !call_id.is_empty() => {
            Ok(WireMessage::CallInvite {
                call_id: (*call_id).to_string(),
                media_kind: (*media_kind).to_string(),
            })
        }
        ["CALL_ACCEPT", call_id] if !call_id.is_empty() => Ok(WireMessage::CallAccept {
            call_id: (*call_id).to_string(),
        }),
        ["CALL_REJECT", call_id] if !call_id.is_empty() => Ok(WireMessage::CallReject {
            call_id: (*call_id).to_string(),
        }),
        ["CALL_END", call_id] if !call_id.is_empty() => Ok(WireMessage::CallEnd {
            call_id: (*call_id).to_string(),
        }),
        ["MEDIA_CONTROL", session_id, command, payload_hex]
            if !session_id.is_empty() && !command.is_empty() =>
        {
            Ok(WireMessage::MediaControl {
                session_id: (*session_id).to_string(),
                command: (*command).to_string(),
                payload_hex: (*payload_hex).to_string(),
            })
        }
        _ => Err(format!("unsupported wire message: {line}")),
    }
}

pub(in crate::net) fn sanitize_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

pub(in crate::net) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }

    encoded
}

pub(in crate::net) fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex payload must have an even length".to_string());
    }

    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_hex_nibble(pair[0])?;
            let low = decode_hex_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn parse_u32(value: &str, field: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("invalid {field}: {value}"))
}

fn parse_u64(value: &str, field: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("invalid {field}: {value}"))
}

fn decode_hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(format!("invalid hex character: {}", value as char)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hello_message() {
        let parsed = parse_message("HELLO\tphone-001\tAndroid Phone").unwrap();

        assert_eq!(
            parsed,
            WireMessage::Hello {
                device_id: "phone-001".to_string(),
                name: "Android Phone".to_string()
            }
        );
    }

    #[test]
    fn parses_heartbeat_message() {
        let parsed = parse_message("HEARTBEAT\tLAN_TCP\t5\t300\t8\t0").unwrap();

        assert_eq!(
            parsed,
            WireMessage::Heartbeat(HeartbeatUpdate {
                transport: TransportKind::LanTcp,
                latency_ms: 5,
                bandwidth_score: 300,
                battery_cost: 8,
                metered_cost: 0,
            })
        );
    }

    #[test]
    fn serializes_heartbeat_message() {
        let message = WireMessage::Heartbeat(HeartbeatUpdate {
            transport: TransportKind::LanTcp,
            latency_ms: 5,
            bandwidth_score: 300,
            battery_cost: 8,
            metered_cost: 0,
        });

        assert_eq!(
            serialize_message(&message),
            "HEARTBEAT\tLAN_TCP\t5\t300\t8\t0"
        );
    }

    #[test]
    fn parses_auth_challenge_message() {
        let parsed = parse_message("AUTH_CHALLENGE\tabc123").unwrap();

        assert_eq!(
            parsed,
            WireMessage::AuthChallenge {
                nonce: "abc123".to_string()
            }
        );
    }

    #[test]
    fn serializes_auth_challenge_message() {
        let message = WireMessage::auth_challenge("abc\t123");

        assert_eq!(serialize_message(&message), "AUTH_CHALLENGE\tabc 123");
    }

    #[test]
    fn parses_auth_signature_message() {
        let parsed = parse_message("AUTH_SIGNATURE\tphone-001\tabcdef").unwrap();

        assert_eq!(
            parsed,
            WireMessage::AuthSignature {
                device_id: "phone-001".to_string(),
                signature_hex: "abcdef".to_string()
            }
        );
    }

    #[test]
    fn serializes_auth_signature_message() {
        let message = WireMessage::auth_signature("phone\t001", "abc\ndef");

        assert_eq!(
            serialize_message(&message),
            "AUTH_SIGNATURE\tphone 001\tabc def"
        );
    }

    #[test]
    fn parses_noise_handshake_message() {
        let parsed = parse_message("NOISE_HS\t0123abcdef").unwrap();

        assert_eq!(
            parsed,
            WireMessage::NoiseHs {
                payload_hex: "0123abcdef".to_string()
            }
        );
    }

    #[test]
    fn serializes_noise_handshake_message() {
        let message = WireMessage::noise_hs("deadbeef");

        assert_eq!(serialize_message(&message), "NOISE_HS\tdeadbeef");
    }

    #[test]
    fn parses_text_message() {
        let parsed = parse_message("TEXT\tphone-001-100\thello from phone").unwrap();

        assert_eq!(
            parsed,
            WireMessage::Text {
                message_id: "phone-001-100".to_string(),
                content: "hello from phone".to_string()
            }
        );
    }

    #[test]
    fn serializes_text_message_with_sanitized_content() {
        let message = WireMessage::text("phone-001-100", "hello\tfrom\nphone");

        assert_eq!(
            serialize_message(&message),
            "TEXT\tphone-001-100\thello from phone"
        );
    }

    #[test]
    fn parses_ack_message() {
        let parsed = parse_message("ACK\tphone-001-100\tTEXT_RECEIVED").unwrap();

        assert_eq!(
            parsed,
            WireMessage::Ack {
                message_id: "phone-001-100".to_string(),
                status: "TEXT_RECEIVED".to_string()
            }
        );
    }

    #[test]
    fn serializes_ack_message() {
        let message = WireMessage::ack("phone-001-100", "TEXT_RECEIVED");

        assert_eq!(
            serialize_message(&message),
            "ACK\tphone-001-100\tTEXT_RECEIVED"
        );
    }

    #[test]
    fn parses_file_start_message() {
        let parsed = parse_message("FILE_START\tphone-001-100\tnotes.txt\t42").unwrap();

        assert_eq!(
            parsed,
            WireMessage::FileStart {
                transfer_id: "phone-001-100".to_string(),
                filename: "notes.txt".to_string(),
                size_bytes: 42,
                sha256_hex: None,
            }
        );
    }

    #[test]
    fn serializes_file_start_message_with_sanitized_filename() {
        let message = WireMessage::file_start("phone-001-100", "my\tnotes.txt", 42);

        assert_eq!(
            serialize_message(&message),
            "FILE_START\tphone-001-100\tmy notes.txt\t42"
        );
    }

    #[test]
    fn parses_file_start_message_with_hash() {
        let parsed = parse_message("FILE_START\tphone-001-100\tnotes.txt\t42\tabc123").unwrap();

        assert_eq!(
            parsed,
            WireMessage::FileStart {
                transfer_id: "phone-001-100".to_string(),
                filename: "notes.txt".to_string(),
                size_bytes: 42,
                sha256_hex: Some("abc123".to_string()),
            }
        );
    }

    #[test]
    fn serializes_file_start_message_with_hash() {
        let message = WireMessage::file_start_with_hash("phone-001-100", "notes.txt", 42, "abc123");

        assert_eq!(
            serialize_message(&message),
            "FILE_START\tphone-001-100\tnotes.txt\t42\tabc123"
        );
    }

    #[test]
    fn parses_file_chunk_message() {
        let parsed = parse_message("FILE_CHUNK\tphone-001-100\t2\t6869").unwrap();

        assert_eq!(
            parsed,
            WireMessage::FileChunk {
                transfer_id: "phone-001-100".to_string(),
                chunk_index: 2,
                data_hex: "6869".to_string(),
            }
        );
    }

    #[test]
    fn serializes_file_chunk_message() {
        let message = WireMessage::file_chunk("phone-001-100", 2, "6869");

        assert_eq!(
            serialize_message(&message),
            "FILE_CHUNK\tphone-001-100\t2\t6869"
        );
    }

    #[test]
    fn parses_file_end_message() {
        let parsed = parse_message("FILE_END\tphone-001-100").unwrap();

        assert_eq!(
            parsed,
            WireMessage::FileEnd {
                transfer_id: "phone-001-100".to_string(),
            }
        );
    }

    #[test]
    fn serializes_file_end_message() {
        let message = WireMessage::file_end("phone-001-100");

        assert_eq!(serialize_message(&message), "FILE_END\tphone-001-100");
    }

    #[test]
    fn hex_encoding_round_trips_bytes() {
        let bytes = b"hello\x00world";
        let encoded = encode_hex(bytes);

        assert_eq!(encoded, "68656c6c6f00776f726c64");
        assert_eq!(decode_hex(&encoded).unwrap(), bytes);
    }

    #[test]
    fn invalid_hex_is_rejected() {
        assert!(decode_hex("abc").is_err());
        assert!(decode_hex("zz").is_err());
    }
}
