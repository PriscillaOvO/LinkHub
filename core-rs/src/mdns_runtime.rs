use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::discovery::{DiscoveryEndpoint, MdnsAdvertisement, LINKHUB_MDNS_SERVICE};

pub struct MdnsRuntime {
    daemon: ServiceDaemon,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MdnsRegistration {
    fullname: String,
}

impl MdnsRegistration {
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl MdnsRuntime {
    pub fn new() -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|err| err.to_string())?;

        Ok(Self { daemon })
    }

    pub fn register(&self, advertisement: &MdnsAdvertisement) -> Result<MdnsRegistration, String> {
        let service_info = service_info_from_advertisement(advertisement)?;
        let fullname = service_info.get_fullname().to_string();

        self.daemon
            .register(service_info)
            .map_err(|err| err.to_string())?;

        Ok(MdnsRegistration { fullname })
    }

    pub fn unregister(&self, registration: &MdnsRegistration) -> Result<(), String> {
        self.daemon
            .unregister(registration.fullname())
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    pub fn browse_for(&self, timeout: Duration) -> Result<Vec<DiscoveryEndpoint>, String> {
        let receiver = self
            .daemon
            .browse(LINKHUB_MDNS_SERVICE)
            .map_err(|err| err.to_string())?;
        let started_at = Instant::now();
        let deadline = started_at + timeout;
        let mut endpoints = Vec::new();

        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }

            match receiver.recv_timeout(deadline - now) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    endpoints.extend(endpoints_from_resolved_service(&info, Instant::now())?);
                }
                Ok(_) => {}
                Err(flume::RecvTimeoutError::Timeout) => break,
                Err(flume::RecvTimeoutError::Disconnected) => {
                    return Err("mDNS browse receiver disconnected".to_string());
                }
            }
        }

        endpoints.sort_by(|left, right| {
            left.device_id()
                .cmp(right.device_id())
                .then_with(|| left.addr().cmp(&right.addr()))
        });
        endpoints.dedup_by(|left, right| {
            left.device_id() == right.device_id() && left.addr() == right.addr()
        });

        Ok(endpoints)
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.daemon
            .shutdown()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}

fn service_info_from_advertisement(
    advertisement: &MdnsAdvertisement,
) -> Result<ServiceInfo, String> {
    let instance_name = advertisement.instance_name();
    let hostname = format!("{}.local.", instance_name);

    ServiceInfo::new(
        LINKHUB_MDNS_SERVICE,
        &instance_name,
        &hostname,
        "",
        advertisement.port(),
        advertisement.txt_record_map(),
    )
    .map(|info| info.enable_addr_auto())
    .map_err(|err| err.to_string())
}

fn endpoints_from_resolved_service(
    info: &ResolvedService,
    discovered_at: Instant,
) -> Result<Vec<DiscoveryEndpoint>, String> {
    let advertisement = MdnsAdvertisement::from_txt_records(&txt_records_from_resolved(info))?;
    let mut addresses = info
        .get_addresses()
        .iter()
        .map(|addr| addr.to_ip_addr())
        .collect::<Vec<_>>();
    addresses.sort();

    Ok(addresses
        .into_iter()
        .map(|ip| endpoint_with_srv_port(&advertisement, ip, info.get_port(), discovered_at))
        .collect())
}

fn txt_records_from_resolved(info: &ResolvedService) -> Vec<String> {
    ["lh", "id", "name", "fp", "port"]
        .into_iter()
        .filter_map(|key| {
            info.get_property_val_str(key)
                .map(|value| format!("{key}={value}"))
        })
        .collect()
}

fn endpoint_with_srv_port(
    advertisement: &MdnsAdvertisement,
    ip: IpAddr,
    srv_port: u16,
    discovered_at: Instant,
) -> DiscoveryEndpoint {
    let port = if srv_port == 0 {
        advertisement.port()
    } else {
        srv_port
    };

    DiscoveryEndpoint::lan_tcp(
        advertisement.device_id().to_string(),
        advertisement.device_name().to_string(),
        (ip, port).into(),
        discovered_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceIdentity;

    #[test]
    fn service_info_uses_linkhub_service_type_and_txt_payload() {
        let identity = DeviceIdentity::new(
            "phone-001",
            "Android Phone",
            "phone-public-key",
            "00".repeat(32),
        );
        let advertisement = MdnsAdvertisement::from_identity(&identity, 8787);

        let service_info = service_info_from_advertisement(&advertisement).unwrap();

        assert_eq!(service_info.get_type(), LINKHUB_MDNS_SERVICE);
        assert_eq!(
            service_info.get_fullname(),
            "Android-Phone-phone-001._linkhub._tcp.local."
        );
        assert_eq!(service_info.get_port(), 8787);
        assert_eq!(service_info.get_property_val_str("lh"), Some("1"));
        assert_eq!(service_info.get_property_val_str("id"), Some("phone-001"));
        assert_eq!(
            service_info.get_property_val_str("name"),
            Some("Android Phone")
        );
        assert_eq!(
            service_info.get_property_val_str("fp"),
            Some("3C5E-00FB-7731-6134")
        );
        assert_eq!(service_info.get_property_val_str("port"), Some("8787"));
    }

    #[test]
    fn endpoint_with_srv_port_prefers_resolved_srv_port() {
        let identity = DeviceIdentity::new(
            "phone-001",
            "Android Phone",
            "phone-public-key",
            "00".repeat(32),
        );
        let advertisement = MdnsAdvertisement::from_identity(&identity, 8787);
        let now = Instant::now();

        let endpoint =
            endpoint_with_srv_port(&advertisement, IpAddr::from([127, 0, 0, 1]), 9000, now);

        assert_eq!(endpoint.addr(), ([127, 0, 0, 1], 9000).into());
        assert_eq!(endpoint.device_id(), "phone-001");
        assert_eq!(endpoint.discovered_at(), now);
    }
}
