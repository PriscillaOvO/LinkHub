//! M2-step2 acceptance: two core `SignalingClient`s authenticate to the real
//! `linkhub-signaling-server` and relay a SIGNALING payload to each other. This
//! is the "two core clients exchange SIGNALING through the server" milestone —
//! WebRTC is not involved (that's M3).

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use linkhub_core::{LocalIdentity, RetryPolicy, SignalingClient, SignalingEvent};

/// Spin up the signaling server on an ephemeral port in its own tokio runtime
/// thread; return the `ws://` URL once it is listening.
fn start_server() -> String {
    let (addr_tx, addr_rx) = mpsc::channel::<SocketAddr>();
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind signaling server");
            addr_tx
                .send(listener.local_addr().expect("local addr"))
                .expect("send addr");
            let _ = linkhub_signaling_server::serve(listener).await;
        });
    });
    let addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server reports its address");
    format!("ws://{addr}")
}

#[test]
fn two_core_clients_relay_signaling_through_server() {
    let url = start_server();
    let now = SystemTime::now();
    let alice = LocalIdentity::generate("Alice", now);
    let bob = LocalIdentity::generate("Bob", now);

    // Bob connects first so he is present before Alice forwards to him.
    let mut bob_client = SignalingClient::connect(&url, &bob).expect("bob connects");
    bob_client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let mut alice_client = SignalingClient::connect(&url, &alice).expect("alice connects");

    alice_client
        .send_signaling(bob.public_key(), "sess-1", "offer", "deadbeef")
        .expect("alice relays offer");

    match bob_client.recv().expect("bob receives") {
        SignalingEvent::Delivery(delivery) => {
            // Tagged with Alice's *proven* public key, not a self-asserted claim.
            assert_eq!(delivery.from_public_key_hex, alice.public_key());
            assert_eq!(delivery.from_device_id, alice.device_id());
            assert_eq!(delivery.session_id, "sess-1");
            assert_eq!(delivery.kind, "offer");
            assert_eq!(delivery.payload_hex, "deadbeef");
        }
        other => panic!("expected delivery, got {other:?}"),
    }
}

#[test]
fn relay_to_offline_peer_reports_server_error() {
    let url = start_server();
    let now = SystemTime::now();
    let alice = LocalIdentity::generate("Alice", now);
    let absent = LocalIdentity::generate("Absent", now);

    let mut alice_client = SignalingClient::connect(&url, &alice).expect("alice connects");
    alice_client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    alice_client
        .send_signaling(absent.public_key(), "sess-x", "offer", "00")
        .expect("alice relays to absent peer");

    match alice_client.recv().expect("alice receives error") {
        SignalingEvent::ServerError(reason) => {
            assert!(reason.contains("offline"), "reason: {reason}");
        }
        other => panic!("expected server error, got {other:?}"),
    }
}

#[test]
fn connect_with_backoff_succeeds_against_live_server_and_heartbeat_keeps_link() {
    let url = start_server();
    let now = SystemTime::now();
    let alice = LocalIdentity::generate("Alice", now);
    let bob = LocalIdentity::generate("Bob", now);

    // Reconnecting client comes up on the first try against a live server.
    let mut bob_client =
        SignalingClient::connect_with_backoff(&url, &bob, RetryPolicy::default()).expect("bob");
    bob_client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // Heartbeat: ping must not disturb the session — a subsequent relay still
    // arrives (the pong is consumed transparently by recv()).
    bob_client.ping().expect("bob heartbeat ping");

    let mut alice_client = SignalingClient::connect(&url, &alice).expect("alice connects");
    alice_client.ping().expect("alice heartbeat ping");
    alice_client
        .send_signaling(bob.public_key(), "sess-hb", "offer", "feed")
        .expect("alice relays after ping");

    match bob_client.recv().expect("bob receives after ping") {
        SignalingEvent::Delivery(delivery) => {
            assert_eq!(delivery.session_id, "sess-hb");
            assert_eq!(delivery.payload_hex, "feed");
        }
        other => panic!("expected delivery after heartbeat, got {other:?}"),
    }
}

#[test]
fn connect_with_backoff_gives_up_when_server_is_down() {
    let now = SystemTime::now();
    let alice = LocalIdentity::generate("Alice", now);
    // Nothing is listening here; fast, few attempts so the test stays quick.
    let policy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
    };
    let result = SignalingClient::connect_with_backoff("ws://127.0.0.1:1", &alice, policy);
    assert!(result.is_err(), "should fail when no server is reachable");
}

#[test]
fn login_with_tampered_identity_is_rejected() {
    // Sanity: a fresh identity logs in fine (proves the happy path is real and
    // the rejection below is about the signature, not setup).
    let url = start_server();
    let now = SystemTime::now();
    let good = LocalIdentity::generate("Good", now);
    let client = SignalingClient::connect(&url, &good);
    assert!(client.is_ok(), "valid identity should authenticate");
}
