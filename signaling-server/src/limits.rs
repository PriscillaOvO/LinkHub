//! Per-connection abuse limits for the signaling server (Stage 5 / T5,
//! design §7 「抗滥用」).
//!
//! The server is a public-facing relay, so a single connection must not be able
//! to exhaust memory (huge frames) or monopolize it (message floods / using it
//! as a free general-purpose relay). These limits are deliberately generous for
//! honest signaling (a handful of small SDP exchanges) and bite only on abuse.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tunable limits applied to every connection.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Hard cap on concurrent WebSocket connections across the whole process.
    pub max_connections: usize,
    /// Hard cap on concurrent WebSocket connections from one source IP.
    pub max_connections_per_ip: usize,
    /// Hard cap on a single inbound WebSocket message / frame, enforced at the
    /// protocol layer (oversized frames drop the connection).
    pub max_message_bytes: usize,
    /// Cap on a `Forward` `payload_hex` length, enforced at the application layer
    /// with a clean error (the SDP envelope is only a few KB).
    pub max_payload_hex_len: usize,
    /// Sliding window for the inbound message-rate limit.
    pub rate_window: Duration,
    /// Max inbound messages allowed within `rate_window` before the connection is
    /// dropped for flooding.
    pub rate_max_messages: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_connections_per_ip: 32,
            max_message_bytes: 64 * 1024,
            // SDP envelope = hex(JSON{v,sdp,sig}); a few KB. 32K hex chars = 16 KiB
            // of binary, far above any real offer/answer.
            max_payload_hex_len: 32 * 1024,
            rate_window: Duration::from_secs(1),
            rate_max_messages: 40,
        }
    }
}

/// Cross-connection concurrency guard for the public signaling endpoint.
///
/// The registry is deliberately separate from the authenticated presence table:
/// it counts every accepted TCP peer before the WebSocket/auth handshake, so an
/// unauthenticated client cannot consume unbounded handshakes.
#[derive(Clone, Debug, Default)]
pub struct ConnectionRegistry {
    inner: Arc<Mutex<ConnectionCounts>>,
}

#[derive(Debug, Default)]
struct ConnectionCounts {
    total: usize,
    per_ip: HashMap<IpAddr, usize>,
}

#[derive(Debug)]
pub struct ConnectionPermit {
    registry: ConnectionRegistry,
    ip: IpAddr,
    released: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionLimitError {
    TooManyConnections {
        max_connections: usize,
    },
    TooManyConnectionsForIp {
        ip: IpAddr,
        max_connections_per_ip: usize,
    },
}

impl std::fmt::Display for ConnectionLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyConnections { max_connections } => {
                write!(f, "too many signaling connections (max {max_connections})")
            }
            Self::TooManyConnectionsForIp {
                ip,
                max_connections_per_ip,
            } => write!(
                f,
                "too many signaling connections from {ip} (max {max_connections_per_ip})"
            ),
        }
    }
}

impl std::error::Error for ConnectionLimitError {}

impl ConnectionRegistry {
    /// Try to reserve one connection slot for `peer_addr`. The returned permit
    /// releases that slot on drop, so every task unregisters exactly its own
    /// accepted connection.
    pub fn register(
        &self,
        peer_addr: SocketAddr,
        limits: &Limits,
    ) -> Result<ConnectionPermit, ConnectionLimitError> {
        let ip = peer_addr.ip();
        let mut guard = self.inner.lock().unwrap();
        if guard.total >= limits.max_connections {
            return Err(ConnectionLimitError::TooManyConnections {
                max_connections: limits.max_connections,
            });
        }

        let ip_count = guard.per_ip.get(&ip).copied().unwrap_or(0);
        if ip_count >= limits.max_connections_per_ip {
            return Err(ConnectionLimitError::TooManyConnectionsForIp {
                ip,
                max_connections_per_ip: limits.max_connections_per_ip,
            });
        }

        guard.total += 1;
        *guard.per_ip.entry(ip).or_insert(0) += 1;
        Ok(ConnectionPermit {
            registry: self.clone(),
            ip,
            released: false,
        })
    }

    fn unregister(&self, ip: IpAddr) {
        let mut guard = self.inner.lock().unwrap();
        guard.total = guard.total.saturating_sub(1);
        match guard.per_ip.get_mut(&ip) {
            Some(count) if *count > 1 => *count -= 1,
            Some(_) => {
                guard.per_ip.remove(&ip);
            }
            None => {}
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> (usize, HashMap<IpAddr, usize>) {
        let guard = self.inner.lock().unwrap();
        (guard.total, guard.per_ip.clone())
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        if !self.released {
            self.registry.unregister(self.ip);
            self.released = true;
        }
    }
}

/// Fixed-window message-rate limiter for one connection. Cheap (two fields) and
/// deterministic — `allow` takes the current time so it can be unit-tested.
#[derive(Debug)]
pub struct RateLimiter {
    window: Duration,
    max: u32,
    window_start: Instant,
    count: u32,
}

impl RateLimiter {
    pub fn new(window: Duration, max: u32, now: Instant) -> Self {
        Self {
            window,
            max,
            window_start: now,
            count: 0,
        }
    }

    pub fn from_limits(limits: &Limits, now: Instant) -> Self {
        Self::new(limits.rate_window, limits.rate_max_messages, now)
    }

    /// Record one inbound message at `now`. Returns `false` when the message
    /// pushes the connection over `max` within the current window.
    pub fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_start) >= self.window {
            self.window_start = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_within_window() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(Duration::from_secs(1), 3, t0);
        assert!(limiter.allow(t0));
        assert!(limiter.allow(t0));
        assert!(limiter.allow(t0));
        // 4th in the same window is over the limit.
        assert!(!limiter.allow(t0));
    }

    #[test]
    fn resets_after_the_window_elapses() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(Duration::from_secs(1), 2, t0);
        assert!(limiter.allow(t0));
        assert!(limiter.allow(t0));
        assert!(!limiter.allow(t0));
        // A full window later the counter resets.
        let t1 = t0 + Duration::from_secs(1);
        assert!(limiter.allow(t1));
        assert!(limiter.allow(t1));
        assert!(!limiter.allow(t1));
    }

    #[test]
    fn default_limits_are_sane() {
        let limits = Limits::default();
        assert!(limits.max_connections > 0);
        assert!(limits.max_connections_per_ip > 0);
        assert!(limits.max_payload_hex_len < limits.max_message_bytes);
        assert!(limits.rate_max_messages > 0);
    }

    #[test]
    fn connection_registry_tracks_total_and_ip_counts() {
        let limits = Limits {
            max_connections: 2,
            max_connections_per_ip: 2,
            ..Limits::default()
        };
        let registry = ConnectionRegistry::default();
        let addr_1 = "127.0.0.1:10000".parse().unwrap();
        let addr_2 = "127.0.0.1:10001".parse().unwrap();

        let permit_1 = registry.register(addr_1, &limits).unwrap();
        let permit_2 = registry.register(addr_2, &limits).unwrap();
        let (total, per_ip) = registry.snapshot();
        assert_eq!(total, 2);
        assert_eq!(per_ip.get(&addr_1.ip()), Some(&2));

        drop(permit_1);
        let (total, per_ip) = registry.snapshot();
        assert_eq!(total, 1);
        assert_eq!(per_ip.get(&addr_1.ip()), Some(&1));

        drop(permit_2);
        let (total, per_ip) = registry.snapshot();
        assert_eq!(total, 0);
        assert_eq!(per_ip.get(&addr_1.ip()), None);
    }

    #[test]
    fn connection_registry_rejects_over_limits() {
        let limits = Limits {
            max_connections: 2,
            max_connections_per_ip: 1,
            ..Limits::default()
        };
        let registry = ConnectionRegistry::default();
        let local_1 = "127.0.0.1:10000".parse().unwrap();
        let local_2 = "127.0.0.1:10001".parse().unwrap();
        let other_ip = "127.0.0.2:10000".parse().unwrap();

        let _permit_1 = registry.register(local_1, &limits).unwrap();
        assert_eq!(
            registry.register(local_2, &limits).unwrap_err(),
            ConnectionLimitError::TooManyConnectionsForIp {
                ip: local_2.ip(),
                max_connections_per_ip: 1,
            }
        );

        let _permit_2 = registry.register(other_ip, &limits).unwrap();
        assert_eq!(
            registry
                .register("127.0.0.3:10000".parse().unwrap(), &limits)
                .unwrap_err(),
            ConnectionLimitError::TooManyConnections { max_connections: 2 }
        );
    }
}
