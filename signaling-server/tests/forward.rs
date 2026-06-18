//! Integration test for the signaling server (M2 acceptance):
//! two authenticated clients exchange a relayed SIGNALING envelope through the
//! server. This is the "ping/pong through the server" milestone — WebRTC is not
//! wired yet (that's M3); here we only prove the signaling link works.

use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use linkhub_signaling_server::auth::challenge_string;
use linkhub_signaling_server::limits::Limits;
use linkhub_signaling_server::protocol::{ClientMsg, ServerMsg};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> String {
    start_server_with_limits(Limits::default()).await
}

async fn start_server_with_limits(limits: Limits) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = linkhub_signaling_server::serve_with_limits(listener, limits).await;
    });
    format!("ws://{addr}")
}

fn key_from_seed(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn pubkey_hex(sk: &SigningKey) -> String {
    hex::encode(sk.verifying_key().to_bytes())
}

async fn next_server_msg(ws: &mut Ws) -> ServerMsg {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("server replied within timeout")
            .expect("stream not closed")
            .expect("no ws error");
        if let Message::Text(text) = frame {
            return ServerMsg::from_json(&text).expect("valid ServerMsg");
        }
        // ignore ping/pong/binary control frames
    }
}

/// Connect, complete the Ed25519 challenge, return the live authenticated socket.
async fn connect_and_login(url: &str, sk: &SigningKey, device_id: &str) -> Ws {
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connect");

    let nonce = match next_server_msg(&mut ws).await {
        ServerMsg::Challenge { nonce } => nonce,
        other => panic!("expected challenge, got {other:?}"),
    };

    let signature = sk.sign(challenge_string(&nonce).as_bytes());
    let auth = ClientMsg::Auth {
        device_id: device_id.to_string(),
        public_key_hex: pubkey_hex(sk),
        signature_hex: hex::encode(signature.to_bytes()),
    };
    ws.send(Message::Text(auth.to_json())).await.unwrap();

    match next_server_msg(&mut ws).await {
        ServerMsg::Welcome { .. } => {}
        other => panic!("expected welcome, got {other:?}"),
    }
    ws
}

#[tokio::test]
async fn relays_signaling_between_two_authenticated_clients() {
    let url = start_server().await;
    let alice = key_from_seed(1);
    let bob = key_from_seed(2);

    let mut alice_ws = connect_and_login(&url, &alice, "lh-alice").await;
    let mut bob_ws = connect_and_login(&url, &bob, "lh-bob").await;

    // Alice sends a WebRTC "offer" addressed to Bob's identity public key.
    let forward = ClientMsg::Forward {
        to_public_key_hex: pubkey_hex(&bob),
        session_id: "sess-1".to_string(),
        kind: "offer".to_string(),
        payload_hex: "deadbeef".to_string(),
    };
    alice_ws
        .send(Message::Text(forward.to_json()))
        .await
        .unwrap();

    // Bob receives it, tagged with Alice's proven public key (not a claim).
    match next_server_msg(&mut bob_ws).await {
        ServerMsg::Deliver {
            from_public_key_hex,
            session_id,
            kind,
            payload_hex,
            ..
        } => {
            assert_eq!(from_public_key_hex, pubkey_hex(&alice));
            assert_eq!(session_id, "sess-1");
            assert_eq!(kind, "offer");
            assert_eq!(payload_hex, "deadbeef");
        }
        other => panic!("expected deliver, got {other:?}"),
    }
}

#[tokio::test]
async fn forward_to_offline_peer_reports_error() {
    let url = start_server().await;
    let alice = key_from_seed(3);
    let absent = key_from_seed(4);

    let mut alice_ws = connect_and_login(&url, &alice, "lh-alice").await;

    let forward = ClientMsg::Forward {
        to_public_key_hex: pubkey_hex(&absent),
        session_id: "sess-x".to_string(),
        kind: "offer".to_string(),
        payload_hex: "00".to_string(),
    };
    alice_ws
        .send(Message::Text(forward.to_json()))
        .await
        .unwrap();

    match next_server_msg(&mut alice_ws).await {
        ServerMsg::Error { reason } => assert!(reason.contains("offline"), "reason: {reason}"),
        other => panic!("expected offline error, got {other:?}"),
    }
}

#[tokio::test]
async fn ping_is_answered_with_pong() {
    let url = start_server().await;
    let sk = key_from_seed(5);
    let mut ws = connect_and_login(&url, &sk, "lh-ping").await;

    ws.send(Message::Text(ClientMsg::Ping.to_json()))
        .await
        .unwrap();

    match next_server_msg(&mut ws).await {
        ServerMsg::Pong => {}
        other => panic!("expected pong, got {other:?}"),
    }
}

#[tokio::test]
async fn oversized_payload_is_rejected() {
    let limits = Limits {
        max_payload_hex_len: 16,
        ..Limits::default()
    };
    let url = start_server_with_limits(limits).await;
    let sk = key_from_seed(7);
    let absent = key_from_seed(8);
    let mut ws = connect_and_login(&url, &sk, "lh-big").await;

    let forward = ClientMsg::Forward {
        to_public_key_hex: pubkey_hex(&absent),
        session_id: "sess-big".to_string(),
        kind: "offer".to_string(),
        payload_hex: "ab".repeat(64), // 128 hex chars, over the 16-char cap
    };
    ws.send(Message::Text(forward.to_json())).await.unwrap();

    match next_server_msg(&mut ws).await {
        ServerMsg::Error { reason } => assert!(reason.contains("too large"), "reason: {reason}"),
        other => panic!("expected payload-too-large error, got {other:?}"),
    }
}

#[tokio::test]
async fn message_flood_is_rate_limited_and_dropped() {
    // Tight limit, wide window so all messages land in the same window.
    let limits = Limits {
        rate_max_messages: 3,
        rate_window: Duration::from_secs(30),
        ..Limits::default()
    };
    let url = start_server_with_limits(limits).await;
    let sk = key_from_seed(9);
    let mut ws = connect_and_login(&url, &sk, "lh-flood").await;

    // Send well past the limit; the first few Pings are answered, then the
    // server emits a rate-limit error and closes.
    for _ in 0..10 {
        ws.send(Message::Text(ClientMsg::Ping.to_json()))
            .await
            .unwrap();
    }

    let mut saw_rate_limit = false;
    for _ in 0..12 {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
        match frame {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(ServerMsg::Error { reason }) = ServerMsg::from_json(&text) {
                    assert!(reason.contains("rate limit"), "reason: {reason}");
                    saw_rate_limit = true;
                    break;
                }
            }
            // Connection closed right after the error is also acceptable.
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => break,
            _ => continue,
        }
    }
    assert!(saw_rate_limit, "expected a rate-limit error before close");
}

#[tokio::test]
async fn bad_signature_is_rejected() {
    let url = start_server().await;
    let sk = key_from_seed(6);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    // Read challenge but sign a *different* nonce, so the proof must fail.
    match next_server_msg(&mut ws).await {
        ServerMsg::Challenge { .. } => {}
        other => panic!("expected challenge, got {other:?}"),
    }
    let forged = sk.sign(challenge_string("not-the-nonce").as_bytes());
    let auth = ClientMsg::Auth {
        device_id: "lh-bad".to_string(),
        public_key_hex: pubkey_hex(&sk),
        signature_hex: hex::encode(forged.to_bytes()),
    };
    ws.send(Message::Text(auth.to_json())).await.unwrap();

    match next_server_msg(&mut ws).await {
        ServerMsg::Error { .. } => {}
        other => panic!("expected auth error, got {other:?}"),
    }
}
