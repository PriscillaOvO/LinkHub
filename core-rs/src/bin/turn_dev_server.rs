//! Dev-only standalone TURN relay server for **local** cross-network validation.
//!
//! Reuses webrtc-rs's `turn` crate (the same code path proven by
//! `tests/webrtc_turn_e2e.rs`) to run a real TURN server on the host, so two
//! emulators — or two machines — can be forced through a relay by enabling the
//! app's "仅使用 TURN 中继" / `relay_only`. Because relay-only ICE refuses
//! host/srflx candidates, a successful transfer proves traffic actually went
//! through this relay: the same path real cross-network (CGNAT cellular) needs,
//! without yet deploying a public coturn.
//!
//! Usage:
//!   cargo run --features webrtc --bin turn-dev-server -- [LISTEN_PORT] [RELAY_IP]
//! Defaults: LISTEN_PORT=3478, RELAY_IP=10.0.2.2 (Android emulator's host alias).
//! Credentials: realm=linkhub.local user=linkhub pass=relay-pass
//!   → app TURN URL `turn:10.0.2.2:3478`, username `linkhub`, credential
//!     `relay-pass`, and tick relay-only.
//!
//! Not part of the default build (gated by `required-features = ["webrtc"]`).

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use webrtc::turn::auth::{generate_auth_key, AuthHandler};
use webrtc::turn::relay::relay_static::RelayAddressGeneratorStatic;
use webrtc::turn::server::config::{ConnConfig, ServerConfig};
use webrtc::turn::server::Server;
use webrtc::turn::Error as TurnError;
use webrtc::util::vnet::net::Net;

const REALM: &str = "linkhub.local";
const USERNAME: &str = "linkhub";
const PASSWORD: &str = "relay-pass";

/// Accepts exactly our one static long-term credential.
struct StaticCredential {
    username: String,
    key: Vec<u8>,
}

impl AuthHandler for StaticCredential {
    fn auth_handle(
        &self,
        username: &str,
        _realm: &str,
        _src_addr: SocketAddr,
    ) -> Result<Vec<u8>, TurnError> {
        if username == self.username {
            Ok(self.key.clone())
        } else {
            Err(TurnError::ErrFakeErr)
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3478);
    let relay_ip = args.next().unwrap_or_else(|| "10.0.2.2".to_string());

    // The control socket. Emulators reach it via `10.0.2.2:PORT` (host loopback
    // alias); other machines via the host LAN IP.
    let socket = Arc::new(
        UdpSocket::bind(("0.0.0.0", port))
            .await
            .expect("bind TURN udp"),
    );
    let key = generate_auth_key(USERNAME, REALM, PASSWORD);

    // `relay_address` is what the server advertises to peers as the allocation
    // address, so it must be reachable *by the other peer*; `address` is the
    // local bind for relay sockets.
    let _server = Server::new(ServerConfig {
        conn_configs: vec![ConnConfig {
            conn: socket,
            relay_addr_generator: Box::new(RelayAddressGeneratorStatic {
                relay_address: IpAddr::from_str(&relay_ip).expect("invalid relay ip"),
                address: "0.0.0.0".to_owned(),
                net: Arc::new(Net::new(None)),
            }),
        }],
        realm: REALM.to_owned(),
        auth_handler: Arc::new(StaticCredential {
            username: USERNAME.to_owned(),
            key,
        }),
        channel_bind_timeout: Duration::from_secs(0),
        alloc_close_notify: None,
    })
    .await
    .expect("start TURN server");

    eprintln!(
        "linkhub turn-dev-server: udp 0.0.0.0:{port}  relay_ip={relay_ip}  realm={REALM} user={USERNAME} pass={PASSWORD}"
    );
    eprintln!("app TURN URL: turn:{relay_ip}:{port}  (set username/credential, enable relay-only)");
    eprintln!("Ctrl-C to stop.");

    // Keep the process (and the server's background tasks) alive until killed.
    std::future::pending::<()>().await;
}
