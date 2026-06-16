//! Relay forwarding — allows device C to reach device B through device A
//! when C cannot directly connect to B.
//!
//! The relay device (A) forwards encrypted payloads between C and B
//! without being able to read them (end-to-end encryption).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use crate::routing::RelayPolicy;

/// A relay session: A forwards traffic between initiator (C) and target (B).
#[derive(Clone, Debug)]
pub struct RelaySession {
    pub session_id: String,
    pub initiator: String, // C (who requested the relay)
    pub target: String,    // B (the destination)
    pub relay: String,     // A (this device, if we're the relay)
    pub established: Instant,
    pub bytes_forwarded: u64,
}

/// Manages active relay sessions.
#[derive(Default)]
pub struct RelayManager {
    pub policy: RelayPolicy,
    /// Active sessions where we are the relay node.
    pub active_relays: HashMap<String, RelaySession>,
    /// Sessions where we are the initiator (using a relay).
    pub outgoing_relays: HashMap<String, RelaySession>,
}

impl RelayManager {
    pub fn new(policy: RelayPolicy) -> Self {
        Self {
            policy,
            ..Default::default()
        }
    }

    /// Check if we are willing to relay for the given initiator→target pair.
    /// Both must be trusted devices (checked by caller).
    pub fn can_relay(&self, _initiator: &str, _target: &str) -> bool {
        match self.policy {
            RelayPolicy::Deny => false,
            RelayPolicy::AllowKnownOnly => true, // trust check done upstream
            RelayPolicy::AllowAll => true,
        }
    }

    /// Create a new relay session where we (relay) forward between initiator and target.
    pub fn create_relay(&mut self, session_id: &str, initiator: &str, target: &str) {
        self.active_relays.insert(
            session_id.to_string(),
            RelaySession {
                session_id: session_id.to_string(),
                initiator: initiator.to_string(),
                target: target.to_string(),
                relay: "me".to_string(),
                established: Instant::now(),
                bytes_forwarded: 0,
            },
        );
    }

    /// Track bytes forwarded through a relay session.
    pub fn record_forward(&mut self, session_id: &str, bytes: u64) {
        if let Some(s) = self.active_relays.get_mut(session_id) {
            s.bytes_forwarded += bytes;
        }
    }

    /// Remove a relay session.
    pub fn remove_relay(&mut self, session_id: &str) {
        self.active_relays.remove(session_id);
        self.outgoing_relays.remove(session_id);
    }

    /// Number of active relay sessions.
    pub fn active_count(&self) -> usize {
        self.active_relays.len()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayRequest {
    pub session_id: String,
    pub target_device: String,
    pub max_hops: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayResponse {
    pub session_id: String,
    pub accepted: bool,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_policy_deny_blocks_all() {
        let mgr = RelayManager::new(RelayPolicy::Deny);
        assert!(!mgr.can_relay("dev-c", "dev-b"));
    }

    #[test]
    fn relay_policy_allow_known() {
        let mgr = RelayManager::new(RelayPolicy::AllowKnownOnly);
        assert!(mgr.can_relay("dev-c", "dev-b"));
    }

    #[test]
    fn relay_session_tracks_bytes() {
        let mut mgr = RelayManager::new(RelayPolicy::AllowKnownOnly);
        mgr.create_relay("s1", "c", "b");
        mgr.record_forward("s1", 1024);
        assert_eq!(mgr.active_relays.get("s1").unwrap().bytes_forwarded, 1024);
    }
}
