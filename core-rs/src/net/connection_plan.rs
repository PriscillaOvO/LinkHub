//! Connection orchestration (Stage 5 / M4-lite): decide, in priority order,
//! which transports to try when reaching a peer, and fall back on failure.
//!
//! Priority is LAN direct → WebRTC hole-punch → cloud relay, which is exactly
//! the order of the base scores in [`crate::transport::TransportHealth::score`]
//! (LanQuic 1000 > LanTcp 900 > WebRtc 780 > CloudRelay 420) — so the fixed
//! policy and the health scoring agree. Scoring additionally decides *which
//! already-established* transport is preferred via
//! [`crate::routing::select_best_route`]; this module decides *what to attempt*
//! when nothing is connected yet.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use crate::routing::select_best_route;
use crate::transport::{TransportHealth, TransportKind};

/// One concrete way to reach a peer, in the order it should be attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionPath {
    /// Direct LAN TCP to a discovered address (fastest, no servers).
    LanTcp { addr: String },
    /// WebRTC P2P established via the signaling server (crosses NAT).
    WebRtc,
    /// Tor onion service at a peer's identity-derived `.onion` address — crosses
    /// NAT with no signaling/relay server, anonymously, but slowly (Phase 2/3).
    Onion { addr: String },
    /// TURN/relay fallback when hole-punching fails (Stage 5 tail; placeholder).
    CloudRelay,
}

impl ConnectionPath {
    /// The transport kind this path establishes (for health/scoring).
    pub fn transport_kind(&self) -> TransportKind {
        match self {
            ConnectionPath::LanTcp { .. } => TransportKind::LanTcp,
            ConnectionPath::WebRtc => TransportKind::WebRtc,
            ConnectionPath::Onion { .. } => TransportKind::Onion,
            ConnectionPath::CloudRelay => TransportKind::CloudRelay,
        }
    }
}

/// What the orchestrator currently knows about how a peer can be reached.
#[derive(Clone, Debug, Default)]
pub struct PeerReachability {
    /// A discovered LAN address (mDNS), if any.
    pub lan_addr: Option<String>,
    /// Whether a signaling server connection is available for WebRTC.
    pub signaling_available: bool,
    /// A paired peer's `.onion` address (from the trust store), if any — lets us
    /// reach it over Tor with no server.
    pub onion_addr: Option<String>,
    /// Whether a relay (TURN) fallback is configured.
    pub relay_available: bool,
}

/// An ordered list of paths to try.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionPlan {
    pub paths: Vec<ConnectionPath>,
}

impl ConnectionPlan {
    pub fn primary(&self) -> Option<&ConnectionPath> {
        self.paths.first()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Build the ordered attempt plan from what we know about a peer.
pub fn plan_connection(reachability: &PeerReachability) -> ConnectionPlan {
    let mut paths = Vec::new();

    if let Some(addr) = &reachability.lan_addr {
        paths.push(ConnectionPath::LanTcp { addr: addr.clone() });
    }
    if reachability.signaling_available {
        paths.push(ConnectionPath::WebRtc);
    }
    if let Some(addr) = &reachability.onion_addr {
        paths.push(ConnectionPath::Onion { addr: addr.clone() });
    }
    if reachability.relay_available {
        paths.push(ConnectionPath::CloudRelay);
    }

    ConnectionPlan { paths }
}

/// The preferred *already-established* transport, by health score — for when
/// several routes are live at once and we want the best, not the first tried.
pub fn preferred_established_route(
    transports: &HashMap<TransportKind, TransportHealth>,
    now: Instant,
    timeout: Duration,
) -> Option<TransportKind> {
    select_best_route(transports, now, timeout)
}

/// Try each path in order, returning the first success. `attempt` performs the
/// actual connect for one path (open a TCP stream, establish a DataChannel, …).
pub fn attempt_with_fallback<T, F>(plan: &ConnectionPlan, mut attempt: F) -> io::Result<T>
where
    F: FnMut(&ConnectionPath) -> io::Result<T>,
{
    let mut last_error = None;

    for path in &plan.paths {
        match attempt(path) {
            Ok(value) => return Ok(value),
            Err(err) => {
                eprintln!("Connection path {path:?} failed, falling back: {err}");
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "no viable connection path to peer",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_prefers_lan_then_webrtc_then_onion_then_relay() {
        let plan = plan_connection(&PeerReachability {
            lan_addr: Some("192.168.1.5:8787".into()),
            signaling_available: true,
            onion_addr: Some("abc.onion".into()),
            relay_available: true,
        });

        assert_eq!(
            plan.paths,
            vec![
                ConnectionPath::LanTcp {
                    addr: "192.168.1.5:8787".into()
                },
                ConnectionPath::WebRtc,
                ConnectionPath::Onion {
                    addr: "abc.onion".into()
                },
                ConnectionPath::CloudRelay,
            ]
        );
    }

    #[test]
    fn plan_uses_onion_when_only_onion_known() {
        let plan = plan_connection(&PeerReachability {
            lan_addr: None,
            signaling_available: false,
            onion_addr: Some("xyz.onion".into()),
            relay_available: false,
        });

        assert_eq!(
            plan.paths,
            vec![ConnectionPath::Onion {
                addr: "xyz.onion".into()
            }]
        );
    }

    #[test]
    fn plan_skips_lan_when_no_address_known() {
        let plan = plan_connection(&PeerReachability {
            lan_addr: None,
            signaling_available: true,
            onion_addr: None,
            relay_available: false,
        });

        assert_eq!(plan.paths, vec![ConnectionPath::WebRtc]);
    }

    #[test]
    fn plan_is_empty_when_peer_unreachable() {
        let plan = plan_connection(&PeerReachability::default());
        assert!(plan.is_empty());
    }

    #[test]
    fn fallback_uses_next_path_when_first_fails() {
        let plan = plan_connection(&PeerReachability {
            lan_addr: Some("10.0.0.9:8787".into()),
            signaling_available: true,
            onion_addr: None,
            relay_available: false,
        });

        let chosen = attempt_with_fallback(&plan, |path| match path {
            ConnectionPath::LanTcp { .. } => {
                Err(io::Error::new(io::ErrorKind::ConnectionRefused, "lan down"))
            }
            other => Ok(other.transport_kind()),
        })
        .unwrap();

        assert_eq!(chosen, TransportKind::WebRtc);
    }

    #[test]
    fn fallback_errors_when_all_paths_fail() {
        let plan = plan_connection(&PeerReachability {
            lan_addr: Some("10.0.0.9:8787".into()),
            signaling_available: true,
            onion_addr: None,
            relay_available: false,
        });

        let result: io::Result<()> = attempt_with_fallback(&plan, |_| {
            Err(io::Error::new(io::ErrorKind::TimedOut, "nope"))
        });
        assert!(result.is_err());
    }

    #[test]
    fn preferred_established_route_picks_highest_score() {
        let now = Instant::now();
        let mut transports = HashMap::new();
        transports.insert(
            TransportKind::CloudRelay,
            TransportHealth {
                kind: TransportKind::CloudRelay,
                last_seen: now,
                latency_ms: 120,
                bandwidth_score: 80,
                battery_cost: 20,
                metered_cost: 35,
            },
        );
        transports.insert(
            TransportKind::WebRtc,
            TransportHealth {
                kind: TransportKind::WebRtc,
                last_seen: now,
                latency_ms: 30,
                bandwidth_score: 240,
                battery_cost: 12,
                metered_cost: 0,
            },
        );

        assert_eq!(
            preferred_established_route(&transports, now, Duration::from_secs(8)),
            Some(TransportKind::WebRtc)
        );
    }
}
