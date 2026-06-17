pub mod ble;
pub mod crypto;
pub use crypto::NoiseTransport;
pub mod device;
pub mod discovery;
pub mod gossip;
pub mod identity;
#[cfg(target_os = "ios")]
pub mod ios_bridge;
#[cfg(target_os = "android")]
pub mod jni_bridge;
pub mod mdns_runtime;
pub mod media_sync;
pub mod net;
pub mod nfc;
pub mod offline_queue;
pub mod presence;
pub mod relay;
pub mod routing;
pub mod transport;
pub mod webrtc;

pub use device::{DeviceAgent, DeviceNode};
pub use discovery::{
    DiscoveryEndpoint, DiscoveryRegistry, MdnsAdvertisement, LINKHUB_MDNS_SERVICE,
};
pub use identity::{
    decode_hex, handshake_challenge, new_handshake_nonce, new_pairing_nonce, DeviceIdentity,
    LocalIdentity, PairingError, PairingInvitation, PairingSession, TrustStore, TrustedDevice,
};
pub use mdns_runtime::{MdnsRegistration, MdnsRuntime};
pub use net::{
    run_authenticated_file_sender, run_authenticated_listener_on,
    run_authenticated_listener_on_with_callback, run_authenticated_listener_until,
    run_authenticated_listener_with_receive_dir, run_authenticated_text_listener,
    run_authenticated_text_sender, run_connector, run_connector_with_receive_dir,
    run_file_control_sender, run_file_sender, run_listener, run_listener_with_receive_dir,
    run_text_sender, FileReceivedCallback, LocalDevice, ReceivedFileEvent,
};
pub use presence::ConnectionState;
pub use transport::{HeartbeatUpdate, TransportHealth, TransportKind};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant, SystemTime};

    fn heartbeat(
        transport: TransportKind,
        latency_ms: u32,
        bandwidth_score: u32,
    ) -> HeartbeatUpdate {
        HeartbeatUpdate {
            transport,
            latency_ms,
            bandwidth_score,
            battery_cost: 0,
            metered_cost: 0,
        }
    }

    #[test]
    fn selects_highest_scoring_fresh_transport() {
        let now = Instant::now();
        let mut device = DeviceNode::new("phone-001", "Phone");

        device.heartbeat(heartbeat(TransportKind::CloudRelay, 120, 50).into_health(now));
        device.heartbeat(heartbeat(TransportKind::LanQuic, 15, 400).into_health(now));
        device.refresh_state(now, Duration::from_secs(8));

        assert_eq!(device.active_route(), Some(TransportKind::LanQuic));
        assert_eq!(device.state(), ConnectionState::Connected);
    }

    #[test]
    fn falls_back_to_ble_when_lan_transport_is_stale() {
        let start = Instant::now();
        let mut device = DeviceNode::new("phone-001", "Phone");

        device.heartbeat(heartbeat(TransportKind::LanQuic, 15, 400).into_health(start));
        device.heartbeat(
            heartbeat(TransportKind::BleControl, 80, 20)
                .into_health(start + Duration::from_secs(9)),
        );
        device.refresh_state(start + Duration::from_secs(9), Duration::from_secs(8));

        assert_eq!(device.active_route(), Some(TransportKind::BleControl));
        assert_eq!(device.state(), ConnectionState::Degraded);
    }

    #[test]
    fn tick_marks_device_reconnecting_when_active_route_disappears() {
        let start = Instant::now();
        let mut agent = DeviceAgent::new("Windows-PC");
        agent.trust_device(DeviceNode::new("phone-001", "Phone"));

        agent.receive_heartbeat(
            "phone-001",
            heartbeat(TransportKind::LanTcp, 20, 300),
            start,
        );
        agent.tick(start + Duration::from_secs(9));

        let device = agent.device("phone-001").unwrap();
        assert_eq!(device.active_route(), None);
        assert_eq!(device.state(), ConnectionState::Reconnecting);
    }

    #[test]
    fn discovery_updates_trusted_device_lan_route() {
        let now = Instant::now();
        let endpoint =
            DiscoveryEndpoint::lan_tcp("phone-001", "Phone", ([127, 0, 0, 1], 8787).into(), now);
        let mut agent = DeviceAgent::new("Windows-PC");
        agent.trust_device(DeviceNode::new("phone-001", "Phone"));

        assert!(agent.observe_discovery(&endpoint, now));

        let device = agent.device("phone-001").unwrap();
        assert_eq!(device.active_route(), Some(TransportKind::LanTcp));
        assert_eq!(device.state(), ConnectionState::Connected);
    }

    #[test]
    fn discovery_ignores_untrusted_devices_until_paired() {
        let now = Instant::now();
        let endpoint = DiscoveryEndpoint::lan_tcp(
            "stranger-001",
            "Unknown Phone",
            ([127, 0, 0, 1], 8787).into(),
            now,
        );
        let mut agent = DeviceAgent::new("Windows-PC");

        assert!(!agent.observe_discovery(&endpoint, now));
        assert!(agent.device("stranger-001").is_none());
    }

    #[test]
    fn confirmed_pairing_allows_later_discovery() {
        let now = Instant::now();
        let pairing_now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let local = DeviceIdentity::new(
            "windows-001",
            "Windows PC",
            "windows-public-key",
            "00".repeat(32),
        );
        let peer = DeviceIdentity::new(
            "phone-001",
            "Android Phone",
            "phone-public-key",
            "00".repeat(32),
        );
        let session = PairingSession::new(
            local,
            PairingInvitation::new(peer, pairing_now, Duration::from_secs(60)),
        );
        let trusted = session
            .confirm(
                &session.confirmation_code(),
                pairing_now,
                SystemTime::UNIX_EPOCH,
            )
            .unwrap();
        let endpoint = DiscoveryEndpoint::lan_tcp(
            "phone-001",
            "Android Phone",
            ([127, 0, 0, 1], 8787).into(),
            now,
        );
        let mut agent = DeviceAgent::new("Windows-PC");

        agent.trust_paired_device(&trusted);

        assert!(agent.observe_discovery(&endpoint, now));
        assert_eq!(
            agent.device("phone-001").unwrap().active_route(),
            Some(TransportKind::LanTcp)
        );
    }
}
