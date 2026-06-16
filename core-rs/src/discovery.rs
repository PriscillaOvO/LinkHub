use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use crate::identity::DeviceIdentity;
use crate::transport::TransportKind;

pub const LINKHUB_MDNS_SERVICE: &str = "_linkhub._tcp.local.";
const LINKHUB_TXT_VERSION: &str = "1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryEndpoint {
    device_id: String,
    device_name: String,
    addr: SocketAddr,
    transport: TransportKind,
    discovered_at: Instant,
}

impl DiscoveryEndpoint {
    pub fn new(
        device_id: impl Into<String>,
        device_name: impl Into<String>,
        addr: SocketAddr,
        transport: TransportKind,
        discovered_at: Instant,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: device_name.into(),
            addr,
            transport,
            discovered_at,
        }
    }

    pub fn lan_tcp(
        device_id: impl Into<String>,
        device_name: impl Into<String>,
        addr: SocketAddr,
        discovered_at: Instant,
    ) -> Self {
        Self::new(
            device_id,
            device_name,
            addr,
            TransportKind::LanTcp,
            discovered_at,
        )
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn transport(&self) -> TransportKind {
        self.transport
    }

    pub fn discovered_at(&self) -> Instant {
        self.discovered_at
    }

    pub fn is_fresh(&self, now: Instant, ttl: Duration) -> bool {
        now.duration_since(self.discovered_at) <= ttl
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MdnsAdvertisement {
    device_id: String,
    device_name: String,
    fingerprint: String,
    port: u16,
}

impl MdnsAdvertisement {
    pub fn from_identity(identity: &DeviceIdentity, port: u16) -> Self {
        Self {
            device_id: identity.device_id().to_string(),
            device_name: identity.device_name().to_string(),
            fingerprint: identity.fingerprint(),
            port,
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn instance_name(&self) -> String {
        format!(
            "{}-{}",
            sanitize_mdns_label(&self.device_name),
            sanitize_mdns_label(&self.device_id)
        )
    }

    pub fn service_name(&self) -> &'static str {
        LINKHUB_MDNS_SERVICE
    }

    pub fn txt_records(&self) -> Vec<String> {
        vec![
            format!("lh={LINKHUB_TXT_VERSION}"),
            format!("id={}", self.device_id),
            format!("name={}", self.device_name),
            format!("fp={}", self.fingerprint),
            format!("port={}", self.port),
        ]
    }

    pub fn txt_record_map(&self) -> HashMap<String, String> {
        [
            ("lh".to_string(), LINKHUB_TXT_VERSION.to_string()),
            ("id".to_string(), self.device_id.clone()),
            ("name".to_string(), self.device_name.clone()),
            ("fp".to_string(), self.fingerprint.clone()),
            ("port".to_string(), self.port.to_string()),
        ]
        .into()
    }

    pub fn from_txt_records(records: &[String]) -> Result<Self, String> {
        let fields = records
            .iter()
            .filter_map(|record| record.split_once('='))
            .collect::<HashMap<_, _>>();

        let version = required_txt_field(&fields, "lh")?;
        if version != LINKHUB_TXT_VERSION {
            return Err(format!("unsupported LinkHub TXT version: {version}"));
        }

        let port = required_txt_field(&fields, "port")?
            .parse::<u16>()
            .map_err(|_| "invalid LinkHub TXT port".to_string())?;

        Ok(Self {
            device_id: required_txt_field(&fields, "id")?.to_string(),
            device_name: required_txt_field(&fields, "name")?.to_string(),
            fingerprint: required_txt_field(&fields, "fp")?.to_string(),
            port,
        })
    }

    pub fn to_endpoint(&self, ip: IpAddr, discovered_at: Instant) -> DiscoveryEndpoint {
        DiscoveryEndpoint::lan_tcp(
            self.device_id.clone(),
            self.device_name.clone(),
            SocketAddr::new(ip, self.port),
            discovered_at,
        )
    }
}

#[derive(Debug)]
pub struct DiscoveryRegistry {
    ttl: Duration,
    endpoints: HashMap<String, DiscoveryEndpoint>,
}

impl DiscoveryRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            endpoints: HashMap::new(),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn upsert(&mut self, endpoint: DiscoveryEndpoint) {
        self.endpoints
            .insert(endpoint.device_id().to_string(), endpoint);
    }

    pub fn get(&self, device_id: &str) -> Option<&DiscoveryEndpoint> {
        self.endpoints.get(device_id)
    }

    pub fn active_endpoints(&self, now: Instant) -> Vec<&DiscoveryEndpoint> {
        let mut endpoints = self
            .endpoints
            .values()
            .filter(|endpoint| endpoint.is_fresh(now, self.ttl))
            .collect::<Vec<_>>();
        endpoints.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        endpoints
    }

    pub fn expire_stale(&mut self, now: Instant) -> Vec<DiscoveryEndpoint> {
        let ttl = self.ttl;
        let stale_ids = self
            .endpoints
            .iter()
            .filter(|(_, endpoint)| !endpoint.is_fresh(now, ttl))
            .map(|(device_id, _)| device_id.clone())
            .collect::<Vec<_>>();

        stale_ids
            .into_iter()
            .filter_map(|device_id| self.endpoints.remove(&device_id))
            .collect()
    }
}

fn required_txt_field<'a>(
    fields: &'a HashMap<&str, &'a str>,
    key: &str,
) -> Result<&'a str, String> {
    fields
        .get(key)
        .copied()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing LinkHub TXT field: {key}"))
}

fn sanitize_mdns_label(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if sanitized.is_empty() {
        "device".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        ([127, 0, 0, 1], port).into()
    }

    fn identity() -> DeviceIdentity {
        DeviceIdentity::new(
            "phone-001",
            "Android Phone",
            "phone-public-key",
            "00".repeat(32),
        )
    }

    #[test]
    fn lan_tcp_endpoint_uses_linkhub_tcp_transport() {
        let now = Instant::now();
        let endpoint = DiscoveryEndpoint::lan_tcp("phone-001", "Android Phone", addr(8787), now);

        assert_eq!(endpoint.device_id(), "phone-001");
        assert_eq!(endpoint.device_name(), "Android Phone");
        assert_eq!(endpoint.addr(), addr(8787));
        assert_eq!(endpoint.transport(), TransportKind::LanTcp);
        assert_eq!(endpoint.discovered_at(), now);
    }

    #[test]
    fn mdns_advertisement_exports_linkhub_txt_records() {
        let advertisement = MdnsAdvertisement::from_identity(&identity(), 8787);

        assert_eq!(advertisement.service_name(), LINKHUB_MDNS_SERVICE);
        assert_eq!(
            advertisement.instance_name(),
            "Android-Phone-phone-001".to_string()
        );
        assert_eq!(advertisement.device_id(), "phone-001");
        assert_eq!(advertisement.device_name(), "Android Phone");
        assert_eq!(advertisement.fingerprint(), "3C5E-00FB-7731-6134");
        assert_eq!(
            advertisement.txt_records(),
            vec![
                "lh=1".to_string(),
                "id=phone-001".to_string(),
                "name=Android Phone".to_string(),
                "fp=3C5E-00FB-7731-6134".to_string(),
                "port=8787".to_string(),
            ]
        );
    }

    #[test]
    fn mdns_advertisement_round_trips_to_discovery_endpoint() {
        let now = Instant::now();
        let advertisement = MdnsAdvertisement::from_identity(&identity(), 8787);
        let parsed = MdnsAdvertisement::from_txt_records(&advertisement.txt_records()).unwrap();
        let endpoint = parsed.to_endpoint(IpAddr::from([192, 168, 1, 20]), now);

        assert_eq!(parsed, advertisement);
        assert_eq!(endpoint.device_id(), "phone-001");
        assert_eq!(endpoint.device_name(), "Android Phone");
        assert_eq!(endpoint.addr(), ([192, 168, 1, 20], 8787).into());
        assert_eq!(endpoint.transport(), TransportKind::LanTcp);
        assert_eq!(endpoint.discovered_at(), now);
    }

    #[test]
    fn mdns_advertisement_rejects_missing_or_invalid_txt_fields() {
        let missing_id = vec![
            "lh=1".to_string(),
            "name=Android Phone".to_string(),
            "fp=3C5E-00FB-7731-6134".to_string(),
            "port=8787".to_string(),
        ];
        let wrong_version = vec![
            "lh=2".to_string(),
            "id=phone-001".to_string(),
            "name=Android Phone".to_string(),
            "fp=3C5E-00FB-7731-6134".to_string(),
            "port=8787".to_string(),
        ];
        let bad_port = vec![
            "lh=1".to_string(),
            "id=phone-001".to_string(),
            "name=Android Phone".to_string(),
            "fp=3C5E-00FB-7731-6134".to_string(),
            "port=not-a-port".to_string(),
        ];

        assert!(MdnsAdvertisement::from_txt_records(&missing_id)
            .unwrap_err()
            .contains("missing LinkHub TXT field: id"));
        assert!(MdnsAdvertisement::from_txt_records(&wrong_version)
            .unwrap_err()
            .contains("unsupported LinkHub TXT version"));
        assert!(MdnsAdvertisement::from_txt_records(&bad_port)
            .unwrap_err()
            .contains("invalid LinkHub TXT port"));
    }

    #[test]
    fn registry_keeps_latest_endpoint_by_device_id() {
        let now = Instant::now();
        let mut registry = DiscoveryRegistry::new(Duration::from_secs(3));

        registry.upsert(DiscoveryEndpoint::lan_tcp(
            "phone-001",
            "Android Phone",
            addr(8787),
            now,
        ));
        registry.upsert(DiscoveryEndpoint::lan_tcp(
            "phone-001",
            "Android Phone",
            addr(8788),
            now + Duration::from_secs(1),
        ));

        let endpoint = registry.get("phone-001").unwrap();
        assert_eq!(endpoint.addr(), addr(8788));
        assert_eq!(
            registry
                .active_endpoints(now + Duration::from_secs(1))
                .len(),
            1
        );
    }

    #[test]
    fn registry_filters_and_expires_stale_endpoints() {
        let now = Instant::now();
        let mut registry = DiscoveryRegistry::new(Duration::from_secs(3));

        registry.upsert(DiscoveryEndpoint::lan_tcp(
            "fresh-001",
            "Fresh",
            addr(8787),
            now,
        ));
        registry.upsert(DiscoveryEndpoint::lan_tcp(
            "stale-001",
            "Stale",
            addr(8788),
            now - Duration::from_secs(4),
        ));

        let active = registry.active_endpoints(now);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].device_id(), "fresh-001");

        let expired = registry.expire_stale(now);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].device_id(), "stale-001");
        assert!(registry.get("stale-001").is_none());
    }
}
