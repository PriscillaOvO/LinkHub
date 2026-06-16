use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::discovery::DiscoveryEndpoint;
use crate::identity::TrustedDevice;
use crate::presence::ConnectionState;
use crate::routing::select_best_route;
use crate::transport::{HeartbeatUpdate, TransportHealth, TransportKind};

#[derive(Clone, Debug)]
pub struct DeviceNode {
    id: String,
    name: String,
    state: ConnectionState,
    transports: HashMap<TransportKind, TransportHealth>,
    active_route: Option<TransportKind>,
}

impl DeviceNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            state: ConnectionState::Discovered,
            transports: HashMap::new(),
            active_route: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn active_route(&self) -> Option<TransportKind> {
        self.active_route
    }

    pub fn heartbeat(&mut self, health: TransportHealth) {
        self.transports.insert(health.kind, health);
    }

    pub fn refresh_state(&mut self, now: Instant, timeout: Duration) {
        let best = select_best_route(&self.transports, now, timeout);

        self.active_route = best;
        self.state = match best {
            Some(TransportKind::LanQuic | TransportKind::LanTcp | TransportKind::WebRtc) => {
                ConnectionState::Connected
            }
            Some(TransportKind::BleControl | TransportKind::CloudRelay) => {
                ConnectionState::Degraded
            }
            None if self.transports.is_empty() => ConnectionState::Discovered,
            None => ConnectionState::Offline,
        };
    }
}

pub struct DeviceAgent {
    local_name: String,
    trusted_devices: HashMap<String, DeviceNode>,
    heartbeat_timeout: Duration,
}

impl DeviceAgent {
    pub fn new(local_name: impl Into<String>) -> Self {
        Self {
            local_name: local_name.into(),
            trusted_devices: HashMap::new(),
            heartbeat_timeout: Duration::from_secs(8),
        }
    }

    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    pub fn device(&self, device_id: &str) -> Option<&DeviceNode> {
        self.trusted_devices.get(device_id)
    }

    pub fn trust_device(&mut self, device: DeviceNode) {
        self.trusted_devices.insert(device.id.clone(), device);
    }

    pub fn trust_paired_device(&mut self, device: &TrustedDevice) {
        self.trust_device(device.to_device_node());
    }

    pub fn receive_heartbeat(&mut self, device_id: &str, update: HeartbeatUpdate, now: Instant) {
        if let Some(device) = self.trusted_devices.get_mut(device_id) {
            device.heartbeat(update.into_health(now));
            device.refresh_state(now, self.heartbeat_timeout);
        }
    }

    pub fn observe_discovery(&mut self, endpoint: &DiscoveryEndpoint, now: Instant) -> bool {
        let Some(device) = self.trusted_devices.get_mut(endpoint.device_id()) else {
            return false;
        };

        device.heartbeat(TransportHealth {
            kind: endpoint.transport(),
            last_seen: now,
            latency_ms: 10,
            bandwidth_score: 250,
            battery_cost: 8,
            metered_cost: 0,
        });
        device.refresh_state(now, self.heartbeat_timeout);
        true
    }

    pub fn tick(&mut self, now: Instant) {
        for device in self.trusted_devices.values_mut() {
            let previous_route = device.active_route;
            device.refresh_state(now, self.heartbeat_timeout);

            if previous_route.is_some() && device.active_route.is_none() {
                device.state = ConnectionState::Reconnecting;
            }
        }
    }

    pub fn print_status(&self) {
        println!("Local agent: {}", self.local_name);
        let mut devices = self.trusted_devices.values().collect::<Vec<_>>();
        devices.sort_by(|left, right| left.id.cmp(&right.id));

        for device in devices {
            let route = device
                .active_route
                .map(|transport| transport.to_string())
                .unwrap_or_else(|| "NO_ROUTE".to_string());
            println!(
                "- {} ({}) state={} route={}",
                device.name, device.id, device.state, route
            );
        }
        println!();
    }
}
