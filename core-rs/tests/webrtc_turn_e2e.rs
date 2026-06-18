//! T4 acceptance (feature `webrtc`): force the WebRTC connection through a TURN
//! relay and prove the authenticated Noise file transfer still works.
//!
//! A real in-process TURN server (webrtc-rs's own `turn` crate, re-exported as
//! `webrtc::turn`) is started on loopback with a long-term credential. Both peers
//! are configured with `force_relay = true` (ICE transport policy = Relay), so
//! they refuse host/srflx candidates — the DataChannel can only establish via the
//! TURN allocation. A successful 40KB transfer with a matching SHA-256 therefore
//! exercises the actual relay path, not a direct connection.
//!
//! Compiled out unless `--features webrtc` is set.
#![cfg(feature = "webrtc")]

use std::fs;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use linkhub_core::net::webrtc_transport::{
    accept_responder, connect_initiator, IceConfig, IceServer, SdpSignal,
};
use linkhub_core::{
    decode_hex, run_authenticated_file_sender_over, run_authenticated_responder_over,
    LocalIdentity, TrustStore, TrustedDevice,
};
use sha2::{Digest, Sha256};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::unbounded_channel;
use webrtc::turn::auth::{generate_auth_key, AuthHandler};
use webrtc::turn::relay::relay_static::RelayAddressGeneratorStatic;
use webrtc::turn::server::config::{ConnConfig, ServerConfig};
use webrtc::turn::server::Server;
use webrtc::turn::Error as TurnError;
use webrtc::util::vnet::net::Net;

const TURN_REALM: &str = "linkhub.test";
const TURN_USERNAME: &str = "linkhub";
const TURN_PASSWORD: &str = "relay-pass";

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

async fn start_turn_server() -> (Server, u16) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("bind turn udp"));
    let port = socket.local_addr().expect("turn local addr").port();
    let key = generate_auth_key(TURN_USERNAME, TURN_REALM, TURN_PASSWORD);

    let server = Server::new(ServerConfig {
        conn_configs: vec![ConnConfig {
            conn: socket,
            relay_addr_generator: Box::new(RelayAddressGeneratorStatic {
                relay_address: IpAddr::from_str("127.0.0.1").unwrap(),
                address: "127.0.0.1".to_owned(),
                net: Arc::new(Net::new(None)),
            }),
        }],
        realm: TURN_REALM.to_owned(),
        auth_handler: Arc::new(StaticCredential {
            username: TURN_USERNAME.to_owned(),
            key,
        }),
        channel_bind_timeout: Duration::from_secs(0),
        alloc_close_notify: None,
    })
    .await
    .expect("start in-process TURN server");

    (server, port)
}

#[test]
fn noise_file_transfer_over_forced_turn_relay() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();

    let (turn_server, turn_port) = rt.block_on(start_turn_server());

    // Both peers: only this TURN server, relay-only — no direct path allowed.
    let ice = IceConfig {
        servers: vec![IceServer::turn(
            format!("turn:127.0.0.1:{turn_port}?transport=udp"),
            TURN_USERNAME,
            TURN_PASSWORD,
        )],
        force_relay: true,
    };

    let (i2r_tx, i2r_rx) = unbounded_channel::<SdpSignal>();
    let (r2i_tx, r2i_rx) = unbounded_channel::<SdpSignal>();

    let init_fut = connect_initiator(ice.clone(), i2r_tx, r2i_rx, handle.clone());
    let resp_fut = accept_responder(ice.clone(), r2i_tx, i2r_rx, handle.clone());
    let (init_duplex, resp_duplex) = rt.block_on(async move { tokio::join!(init_fut, resp_fut) });
    let init_duplex = init_duplex.expect("initiator establishes DataChannel via TURN");
    let resp_duplex = resp_duplex.expect("responder establishes DataChannel via TURN");

    let now = SystemTime::now();
    let sender = LocalIdentity::generate("Sender", now);
    let receiver = LocalIdentity::generate("Receiver", now);
    let receiver_device_id = receiver.device_id().to_string();
    let receiver_dh = dh_bytes(&receiver);

    let mut trust = TrustStore::new();
    trust.trust(TrustedDevice::new(sender.identity().clone(), now));
    let trust = Arc::new(trust);

    let receive_dir = unique_dir("turn-recv");
    let send_dir = unique_dir("turn-send");
    let payload = deterministic_bytes(40_000);
    let source = send_dir.join("turn-sample.bin");
    fs::write(&source, &payload).unwrap();
    let expected_hash = sha256_hex(&payload);

    let resp_writer = resp_duplex.clone();
    let resp_reader = BufReader::new(resp_duplex.clone());
    let receive_dir_for_thread = receive_dir.clone();
    let responder_thread = thread::spawn(move || {
        run_authenticated_responder_over(
            resp_writer,
            resp_reader,
            receiver,
            trust,
            receive_dir_for_thread,
            None,
        )
    });

    let init_writer = init_duplex.clone();
    let init_reader = BufReader::new(init_duplex.clone());
    run_authenticated_file_sender_over(
        init_writer,
        init_reader,
        &sender,
        &receiver_device_id,
        &receiver_dh,
        &source,
    )
    .expect("authenticated file send over TURN relay");

    init_duplex.close();
    let responder_result = responder_thread.join().expect("responder thread panicked");
    assert!(
        responder_result.is_ok(),
        "responder session should end Ok, got {responder_result:?}"
    );

    let received = find_received(&receive_dir, "turn-sample.bin");
    assert_eq!(
        sha256_hex(&fs::read(&received).unwrap()),
        expected_hash,
        "file relayed through TURN must match source SHA-256"
    );

    let _ = fs::remove_dir_all(&receive_dir);
    let _ = fs::remove_dir_all(&send_dir);
    rt.block_on(async {
        let _ = turn_server.close().await;
    });
    drop(rt);
}

fn find_received(dir: &Path, filename: &str) -> PathBuf {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == filename || name.ends_with(&format!("_{filename}")) {
            return path;
        }
    }
    panic!("{filename} not found in {}", dir.display());
}

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("linkhub-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 17) % 251) as u8).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn dh_bytes(identity: &LocalIdentity) -> [u8; 32] {
    decode_hex(identity.dh_public_key())
        .unwrap()
        .try_into()
        .unwrap()
}
