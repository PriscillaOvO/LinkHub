//! Per-connection abuse limits for the signaling server (Stage 5 / T5,
//! design §7 「抗滥用」).
//!
//! The server is a public-facing relay, so a single connection must not be able
//! to exhaust memory (huge frames) or monopolize it (message floods / using it
//! as a free general-purpose relay). These limits are deliberately generous for
//! honest signaling (a handful of small SDP exchanges) and bite only on abuse.

use std::time::{Duration, Instant};

/// Tunable limits applied to every connection.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
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
            max_message_bytes: 64 * 1024,
            // SDP envelope = hex(JSON{v,sdp,sig}); a few KB. 32K hex chars = 16 KiB
            // of binary, far above any real offer/answer.
            max_payload_hex_len: 32 * 1024,
            rate_window: Duration::from_secs(1),
            rate_max_messages: 40,
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
        assert!(limits.max_payload_hex_len < limits.max_message_bytes);
        assert!(limits.rate_max_messages > 0);
    }
}
