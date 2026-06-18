//! WebRTC multi-path connectivity layer.
//!
//! Handles:
//! - STUN/TURN configuration
//! - Signaling message exchange over existing TCP encrypted channel
//! - DataChannel abstraction for running LinkHub wire protocol over WebRTC
//! - Connection lifecycle (offer/answer/ICE candidate exchange)
//! - Audio/video media tracks (Stage 7)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Configuration ──────────────────────────────────────────────────

/// STUN/TURN server configuration for ICE gathering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServer {
    /// Google's public STUN server — sufficient for development.
    pub fn google_stun() -> Self {
        Self {
            urls: vec!["stun:stun.l.google.com:19302".into()],
            username: None,
            credential: None,
        }
    }
}

/// WebRTC connection configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebRtcConfig {
    pub ice_servers: Vec<IceServer>,
    pub ice_transport_policy: IceTransportPolicy,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            ice_servers: vec![IceServer::google_stun()],
            ice_transport_policy: IceTransportPolicy::All,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IceTransportPolicy {
    All,
    Relay,
}

// ── Signaling Messages ─────────────────────────────────────────────

/// A signaling message exchanged over the existing TCP encrypted channel
/// to negotiate a WebRTC peer connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalingMessage {
    pub session_id: String,
    pub kind: SignalingKind,
    pub from_device: String,
    pub to_device: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SignalingKind {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    IceCandidate {
        candidate: String,
        sdp_mid: String,
        sdp_mline_index: u16,
    },
    Done,
    Error {
        reason: String,
    },
}

// ── Connection State ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum WebRtcConnectionState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed(String),
    Closed,
}

// ── Session ────────────────────────────────────────────────────────

/// Represents an active or in-progress WebRTC connection.
#[derive(Clone, Debug)]
pub struct WebRtcSession {
    pub session_id: String,
    pub peer_device_id: String,
    pub config: WebRtcConfig,
    pub state: WebRtcConnectionState,
    pub transport_quality: TransportQualityStats,
}

impl WebRtcSession {
    pub fn new(peer_device_id: &str, config: WebRtcConfig) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            peer_device_id: peer_device_id.to_string(),
            config,
            state: WebRtcConnectionState::New,
            transport_quality: TransportQualityStats::default(),
        }
    }

    /// Create an SDP offer signaling message.
    pub fn create_offer(&self, local_device: &str, sdp: String) -> SignalingMessage {
        SignalingMessage {
            session_id: self.session_id.clone(),
            kind: SignalingKind::Offer { sdp },
            from_device: local_device.to_string(),
            to_device: self.peer_device_id.clone(),
        }
    }

    /// Create an SDP answer signaling message.
    pub fn create_answer(&self, local_device: &str, sdp: String) -> SignalingMessage {
        SignalingMessage {
            session_id: self.session_id.clone(),
            kind: SignalingKind::Answer { sdp },
            from_device: local_device.to_string(),
            to_device: self.peer_device_id.clone(),
        }
    }

    /// Create an ICE candidate signaling message.
    pub fn create_ice_candidate(
        &self,
        local_device: &str,
        candidate: &str,
        sdp_mid: &str,
        sdp_mline_index: u16,
    ) -> SignalingMessage {
        SignalingMessage {
            session_id: self.session_id.clone(),
            kind: SignalingKind::IceCandidate {
                candidate: candidate.to_string(),
                sdp_mid: sdp_mid.to_string(),
                sdp_mline_index,
            },
            from_device: local_device.to_string(),
            to_device: self.peer_device_id.clone(),
        }
    }
}

// ── Transport Quality ──────────────────────────────────────────────

/// Per-connection quality metrics (for multi-path routing decisions).
#[derive(Clone, Debug)]
pub struct TransportQualityStats {
    pub latency_ms: u32,
    pub jitter_ms: u32,
    pub packets_sent: u64,
    pub packets_lost: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub bandwidth_estimate_kbps: u32,
    pub last_updated: std::time::Instant,
}

impl Default for TransportQualityStats {
    fn default() -> Self {
        Self {
            latency_ms: 0,
            jitter_ms: 0,
            packets_sent: 0,
            packets_lost: 0,
            bytes_sent: 0,
            bytes_received: 0,
            bandwidth_estimate_kbps: 0,
            last_updated: std::time::Instant::now(),
        }
    }
}

impl TransportQualityStats {
    /// Packet loss ratio (0.0 - 1.0).
    pub fn loss_ratio(&self) -> f64 {
        if self.packets_sent == 0 {
            0.0
        } else {
            self.packets_lost as f64 / self.packets_sent as f64
        }
    }

    /// Simple quality score (0-1000), higher is better.
    pub fn score(&self) -> i32 {
        let bandwidth_score = (self.bandwidth_estimate_kbps as i32).min(1000);
        let latency_penalty = (self.latency_ms as i32).min(500);
        let loss_penalty = (self.loss_ratio() * 500.0) as i32;
        1000 - latency_penalty - loss_penalty + bandwidth_score / 10
    }
}

// ── Multi-path Session Manager ─────────────────────────────────────

/// Manages multiple concurrent transport sessions to the same peer.
#[derive(Default)]
pub struct MultiPathSession {
    pub tcp_active: bool,
    pub webrtc_sessions: Vec<WebRtcSession>,
    pub preferred_transport: PreferredTransport,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum PreferredTransport {
    #[default]
    Auto,
    LanTcp,
    WebRtc,
    AnyAvailable,
}

impl MultiPathSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pick the best available connection based on quality and preference.
    pub fn best_session(&self) -> Option<&WebRtcSession> {
        self.webrtc_sessions
            .iter()
            .filter(|s| s.state == WebRtcConnectionState::Connected)
            .max_by_key(|s| s.transport_quality.score())
    }

    pub fn has_any_connection(&self) -> bool {
        self.tcp_active
            || self
                .webrtc_sessions
                .iter()
                .any(|s| s.state == WebRtcConnectionState::Connected)
    }
}

// ── Media (Stage 7) ────────────────────────────────────────────────

/// Types of media in a call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MediaKind {
    Audio,
    Video,
    AudioVideo,
}

/// Call state machine.
#[derive(Clone, Debug, PartialEq)]
pub enum CallState {
    Idle,
    Ringing,
    Active,
    Ended,
}

/// Represents an active or pending media call.
#[derive(Clone, Debug)]
pub struct MediaCall {
    pub call_id: String,
    pub peer_device_id: String,
    pub kind: MediaKind,
    pub state: CallState,
    pub established_at: Option<std::time::Instant>,
}

// ── Weak Network Adaptation ────────────────────────────────────────

/// Quality levels for adaptive streaming.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum QualityLevel {
    High,   // 720p30 video, 64kbps audio
    Medium, // 480p30 video, 32kbps audio
    Low,    // 240p15 video, 16kbps audio
    AudioOnly,
}

impl QualityLevel {
    pub fn from_stats(stats: &TransportQualityStats) -> Self {
        if stats.loss_ratio() > 0.1 || stats.bandwidth_estimate_kbps < 100 {
            QualityLevel::AudioOnly
        } else if stats.loss_ratio() > 0.05 || stats.bandwidth_estimate_kbps < 500 {
            QualityLevel::Low
        } else if stats.loss_ratio() > 0.02 || stats.bandwidth_estimate_kbps < 1500 {
            QualityLevel::Medium
        } else {
            QualityLevel::High
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_server_default_uses_google_stun() {
        let config = WebRtcConfig::default();
        assert_eq!(config.ice_servers.len(), 1);
        assert!(config.ice_servers[0].urls[0].contains("stun.l.google.com"));
    }

    #[test]
    fn webrtc_session_creates_unique_ids() {
        let s1 = WebRtcSession::new("peer-a", WebRtcConfig::default());
        let s2 = WebRtcSession::new("peer-b", WebRtcConfig::default());
        assert_ne!(s1.session_id, s2.session_id);
    }

    #[test]
    fn quality_stats_loss_ratio_zero_when_no_packets() {
        let stats = TransportQualityStats::default();
        assert_eq!(stats.loss_ratio(), 0.0);
    }

    #[test]
    fn quality_stats_loss_ratio_correct() {
        let stats = TransportQualityStats {
            packets_sent: 100,
            packets_lost: 5,
            ..Default::default()
        };
        assert!((stats.loss_ratio() - 0.05).abs() < 0.001);
    }

    #[test]
    fn quality_level_degradation() {
        let mut stats = TransportQualityStats {
            bandwidth_estimate_kbps: 2000,
            ..Default::default()
        };
        assert_eq!(QualityLevel::from_stats(&stats), QualityLevel::High);

        stats.bandwidth_estimate_kbps = 300;
        stats.packets_sent = 100;
        stats.packets_lost = 6;
        assert_eq!(QualityLevel::from_stats(&stats), QualityLevel::Low);

        stats.bandwidth_estimate_kbps = 50;
        assert_eq!(QualityLevel::from_stats(&stats), QualityLevel::AudioOnly);
    }

    #[test]
    fn signaling_message_serialization_round_trips() {
        let msg = SignalingMessage {
            session_id: "test-session".into(),
            kind: SignalingKind::Offer {
                sdp: "v=0\r\n...".into(),
            },
            from_device: "dev-a".into(),
            to_device: "dev-b".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: SignalingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id, "test-session");
        assert_eq!(parsed.from_device, "dev-a");
    }
}
