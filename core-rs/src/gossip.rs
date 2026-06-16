//! Gossip protocol — propagates device reachability information
//! through the trusted device mesh.
//!
//! Each device periodically broadcasts which peers it can reach
//! and at what quality. Receiving devices merge this into their
//! route tables (see routing.rs).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often gossip messages are broadcast.
pub const GOSSIP_INTERVAL: Duration = Duration::from_secs(10);

/// Maximum hops a gossip message can traverse.
pub const GOSSIP_MAX_TTL: u8 = 3;

/// A single entry in a gossip message: "I can reach device X via path Y".
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReachabilityEntry {
    pub device_id: String,
    pub device_name: String,
    /// The hop path from the origin to this device (including origin and target).
    /// e.g. ["origin", "hop1", "target"]
    pub path: Vec<String>,
    /// Number of hops (path.len() - 1).
    pub hop_count: u8,
    pub quality: GossipRouteQuality,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipRouteQuality {
    pub latency_ms: u32,
    pub bandwidth_score: u32,
    pub reliability: u8, // 0-100
}

/// A gossip message broadcast by a device.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipMessage {
    pub origin_device: String,
    pub origin_name: String,
    pub sequence: u64,
    pub ttl: u8,
    pub reachable_devices: Vec<ReachabilityEntry>,
}

impl GossipMessage {
    pub fn new(origin_device: &str, origin_name: &str, sequence: u64) -> Self {
        Self {
            origin_device: origin_device.to_string(),
            origin_name: origin_name.to_string(),
            sequence,
            ttl: GOSSIP_MAX_TTL,
            reachable_devices: Vec::new(),
        }
    }

    /// Decrement TTL. Returns false if the message should be dropped.
    pub fn decrement_ttl(&mut self) -> bool {
        if self.ttl == 0 {
            return false;
        }
        self.ttl -= 1;
        true
    }

    /// Serialize to a compact wire format (base64 of JSON).
    pub fn to_wire(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    /// Deserialize from wire format.
    pub fn from_wire(data: &str) -> Result<Self, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("{e}"))?;
        let json = std::str::from_utf8(&bytes).map_err(|e| format!("{e}"))?;
        serde_json::from_str(json).map_err(|e| format!("{e}"))
    }
}

/// Tracks gossip state: sequence numbers, seen messages, last broadcast time.
#[derive(Default)]
pub struct GossipState {
    pub last_broadcast: Option<Instant>,
    pub sequence: u64,
    /// Set of (origin_device, sequence) pairs already received (dedup).
    pub seen: HashMap<String, u64>,
}

impl GossipState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we've already seen this gossip message.
    pub fn is_duplicate(&self, origin: &str, sequence: u64) -> bool {
        self.seen.get(origin).map_or(false, |&s| s >= sequence)
    }

    /// Mark a gossip message as seen.
    pub fn mark_seen(&mut self, origin: &str, sequence: u64) {
        self.seen.insert(origin.to_string(), sequence);
    }

    /// Check if it's time to broadcast.
    pub fn should_broadcast(&self, now: Instant) -> bool {
        self.last_broadcast
            .map_or(true, |t| now.duration_since(t) >= GOSSIP_INTERVAL)
    }

    /// Get the next sequence number.
    pub fn next_sequence(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gossip_message_wire_format_round_trips() {
        let mut msg = GossipMessage::new("dev-a", "Device A", 1);
        msg.reachable_devices.push(ReachabilityEntry {
            device_id: "dev-b".into(),
            device_name: "Device B".into(),
            path: vec!["dev-a".into(), "dev-b".into()],
            hop_count: 1,
            quality: GossipRouteQuality {
                latency_ms: 10,
                bandwidth_score: 500,
                reliability: 90,
            },
        });
        let wire = msg.to_wire();
        let parsed = GossipMessage::from_wire(&wire).unwrap();
        assert_eq!(parsed.origin_device, "dev-a");
        assert_eq!(parsed.reachable_devices.len(), 1);
    }

    #[test]
    fn gossip_dedup_rejects_older_sequence() {
        let mut state = GossipState::new();
        state.mark_seen("dev-a", 5);
        assert!(state.is_duplicate("dev-a", 5));
        assert!(state.is_duplicate("dev-a", 3));
        assert!(!state.is_duplicate("dev-a", 6));
        assert!(!state.is_duplicate("dev-b", 1));
    }

    #[test]
    fn ttl_decrements_and_expires() {
        let mut msg = GossipMessage::new("a", "A", 0);
        assert_eq!(msg.ttl, 3);
        assert!(msg.decrement_ttl());
        assert_eq!(msg.ttl, 2);
        msg.ttl = 0;
        assert!(!msg.decrement_ttl());
    }
}
