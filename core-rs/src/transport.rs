use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    LanQuic,
    LanTcp,
    WebRtc,
    BleControl,
    CloudRelay,
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            TransportKind::LanQuic => "LAN_QUIC",
            TransportKind::LanTcp => "LAN_TCP",
            TransportKind::WebRtc => "WEBRTC",
            TransportKind::BleControl => "BLE_CONTROL",
            TransportKind::CloudRelay => "CLOUD_RELAY",
        };
        write!(f, "{value}")
    }
}

impl FromStr for TransportKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "LAN_QUIC" => Ok(TransportKind::LanQuic),
            "LAN_TCP" => Ok(TransportKind::LanTcp),
            "WEBRTC" => Ok(TransportKind::WebRtc),
            "BLE_CONTROL" => Ok(TransportKind::BleControl),
            "CLOUD_RELAY" => Ok(TransportKind::CloudRelay),
            _ => Err(format!("unknown transport kind: {value}")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransportHealth {
    pub kind: TransportKind,
    pub last_seen: Instant,
    pub latency_ms: u32,
    pub bandwidth_score: u32,
    pub battery_cost: u32,
    pub metered_cost: u32,
}

impl TransportHealth {
    pub fn score(&self, now: Instant) -> i32 {
        let age_penalty = now.duration_since(self.last_seen).as_secs() as i32 * 25;
        let base = match self.kind {
            TransportKind::LanQuic => 1_000,
            TransportKind::LanTcp => 900,
            TransportKind::WebRtc => 780,
            TransportKind::BleControl => 280,
            TransportKind::CloudRelay => 420,
        };

        base + self.bandwidth_score as i32
            - self.latency_ms as i32
            - self.battery_cost as i32
            - self.metered_cost as i32
            - age_penalty
    }

    pub fn is_fresh(&self, now: Instant, timeout: Duration) -> bool {
        now.duration_since(self.last_seen) <= timeout
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeartbeatUpdate {
    pub transport: TransportKind,
    pub latency_ms: u32,
    pub bandwidth_score: u32,
    pub battery_cost: u32,
    pub metered_cost: u32,
}

impl HeartbeatUpdate {
    pub fn into_health(self, now: Instant) -> TransportHealth {
        TransportHealth {
            kind: self.transport,
            last_seen: now,
            latency_ms: self.latency_ms,
            bandwidth_score: self.bandwidth_score,
            battery_cost: self.battery_cost,
            metered_cost: self.metered_cost,
        }
    }
}
