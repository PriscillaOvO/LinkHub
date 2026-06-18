use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::transport::{TransportHealth, TransportKind};

// ── Single-hop direct route (existing behavior) ────────────────────

/// Select the single best direct transport to a peer.
pub fn select_best_route(
    transports: &HashMap<TransportKind, TransportHealth>,
    now: Instant,
    timeout: Duration,
) -> Option<TransportKind> {
    transports
        .values()
        .filter(|transport| transport.is_fresh(now, timeout))
        .max_by_key(|transport| transport.score(now))
        .map(|transport| transport.kind)
}

// ── Multi-hop route table (Stage 6 Mesh) ───────────────────────────

/// A route through the mesh: a path of device IDs from source to destination.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshRoute {
    /// The full path: [origin, hop1, hop2, ..., destination]
    pub path: Vec<String>,
    /// The next device to send to (path[1]).
    pub next_hop: String,
    /// Number of hops (path.len() - 1).
    pub hop_count: u8,
    /// Aggregated quality of this path.
    pub quality: RouteQuality,
    /// Transport to use for the first hop.
    pub first_hop_transport: TransportKind,
    /// When this route was last confirmed fresh (unix timestamp seconds).
    pub last_seen_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteQuality {
    pub latency_ms: u32,
    pub bandwidth_score: u32,
    pub reliability: u8,
}

impl RouteQuality {
    /// Composite score — higher is better.
    pub fn score(&self) -> i32 {
        (self.bandwidth_score as i32) - (self.latency_ms as i32) + (self.reliability as i32 * 5)
    }
}

/// Route table: destination device_id -> available routes.
#[derive(Clone, Debug, Default)]
pub struct RouteTable {
    pub routes: HashMap<String, Vec<MeshRoute>>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a route to a destination.
    pub fn upsert_route(&mut self, destination: &str, route: MeshRoute) {
        let entry = self.routes.entry(destination.to_string()).or_default();
        // Replace existing route with same path, or add new
        if let Some(existing) = entry.iter_mut().find(|r| r.path == route.path) {
            existing.quality = route.quality;
            existing.last_seen_secs = route.last_seen_secs;
        } else {
            entry.push(route);
        }
        // Keep only the best 5 routes per destination
        entry.sort_by_key(|r| -r.quality.score());
        entry.truncate(5);
    }

    /// Select the best route to a destination considering hop count and quality.
    /// Prefers direct routes (1 hop) over multi-hop.
    pub fn best_route(
        &self,
        destination: &str,
        now_secs: u64,
        max_age_secs: u64,
    ) -> Option<&MeshRoute> {
        let routes = self.routes.get(destination)?;
        routes
            .iter()
            .filter(|r| now_secs.saturating_sub(r.last_seen_secs) <= max_age_secs)
            .min_by_key(|r| (r.hop_count, -r.quality.score()))
    }

    /// Remove stale routes.
    pub fn purge_stale(&mut self, now_secs: u64, max_age_secs: u64) {
        for routes in self.routes.values_mut() {
            routes.retain(|r| now_secs.saturating_sub(r.last_seen_secs) <= max_age_secs);
        }
        self.routes.retain(|_, v| !v.is_empty());
    }

    /// All known destinations.
    pub fn destinations(&self) -> Vec<&str> {
        self.routes.keys().map(|k| k.as_str()).collect()
    }

    /// Number of known destinations.
    pub fn len(&self) -> usize {
        self.routes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

/// Build a route entry from gossip reachability info.
pub fn route_from_reachability(
    path: Vec<String>,
    latency_ms: u32,
    bandwidth_score: u32,
    reliability: u8,
) -> Option<MeshRoute> {
    if path.len() < 2 {
        return None;
    }
    Some(MeshRoute {
        next_hop: path[1].clone(),
        hop_count: (path.len() - 1) as u8,
        path,
        quality: RouteQuality {
            latency_ms,
            bandwidth_score,
            reliability,
        },
        first_hop_transport: TransportKind::LanTcp,
        last_seen_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

// ── Relay policy ───────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RelayPolicy {
    Deny,
    #[default]
    AllowKnownOnly,
    AllowAll,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_finds_best_route() {
        let mut table = RouteTable::new();

        let direct = MeshRoute {
            path: vec!["me".into(), "peer-a".into()],
            next_hop: "peer-a".into(),
            hop_count: 1,
            quality: RouteQuality {
                latency_ms: 10,
                bandwidth_score: 500,
                reliability: 90,
            },
            first_hop_transport: TransportKind::LanTcp,
            last_seen_secs: 0,
        };
        let multi_hop = MeshRoute {
            path: vec!["me".into(), "relay".into(), "peer-a".into()],
            next_hop: "relay".into(),
            hop_count: 2,
            quality: RouteQuality {
                latency_ms: 20,
                bandwidth_score: 400,
                reliability: 80,
            },
            first_hop_transport: TransportKind::LanTcp,
            last_seen_secs: 0,
        };

        table.upsert_route("peer-a", multi_hop);
        table.upsert_route("peer-a", direct);

        let best = table.best_route("peer-a", 0, 30).unwrap();
        assert_eq!(best.hop_count, 1);
        assert_eq!(best.next_hop, "peer-a");
    }

    #[test]
    fn route_table_purges_stale() {
        let mut table = RouteTable::new();
        table.upsert_route(
            "peer-a",
            MeshRoute {
                path: vec!["me".into(), "peer-a".into()],
                next_hop: "peer-a".into(),
                hop_count: 1,
                quality: RouteQuality {
                    latency_ms: 10,
                    bandwidth_score: 300,
                    reliability: 50,
                },
                first_hop_transport: TransportKind::LanTcp,
                last_seen_secs: 0,
            },
        );
        table.purge_stale(100, 30);
        assert!(table.is_empty());
    }
}
