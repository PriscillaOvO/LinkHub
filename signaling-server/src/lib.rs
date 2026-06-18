//! LinkHub signaling server (Stage 5 / M2).
//!
//! A thin WebSocket service that lets two already-paired devices find each other
//! across networks and exchange WebRTC signaling (SDP/ICE) before they establish
//! a direct, end-to-end-encrypted (Noise KK) connection. The server:
//!   * authenticates each device by an Ed25519 challenge ([`auth`]),
//!   * keeps an in-memory presence table keyed by the *proven* identity public
//!     key, and
//!   * store-and-forwards [`protocol::ClientMsg::Forward`] envelopes between
//!     online devices, treating `payload_hex` as opaque.
//!
//! It deliberately holds **no** key material and sees **no** plaintext: the
//! strongest thing it learns is metadata (who is online, who wants to reach
//! whom, when). See `docs/spec/设计-跨网络传输-webrtc.md` §5/§7.

pub mod auth;
pub mod limits;
pub mod protocol;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use limits::{Limits, RateLimiter};
use protocol::{ClientMsg, ServerMsg};

/// Presence table: identity public key (hex) -> channel into that device's
/// connection. A device is "online and routable" exactly while it has an entry.
type Registry = Arc<Mutex<HashMap<String, UnboundedSender<ServerMsg>>>>;

/// Accept connections forever, handling each in its own task. Returns only on
/// listener error.
pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    serve_with_limits(listener, Limits::default()).await
}

/// Like [`serve`] but with explicit abuse [`Limits`] (used by tests to make the
/// caps tight enough to trip deterministically).
pub async fn serve_with_limits(listener: TcpListener, limits: Limits) -> std::io::Result<()> {
    let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
    loop {
        let (stream, _peer) = listener.accept().await?;
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, registry, limits).await {
                // Per-connection errors are expected (clients drop, send junk);
                // log and move on, never take down the server.
                eprintln!("connection ended: {err}");
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    registry: Registry,
    limits: Limits,
) -> Result<(), String> {
    // Cap inbound frame/message size at the protocol layer so a single peer can't
    // exhaust memory with a giant frame (design §7 抗滥用).
    let ws_config = WebSocketConfig {
        max_message_size: Some(limits.max_message_bytes),
        max_frame_size: Some(limits.max_message_bytes),
        ..Default::default()
    };
    let ws = tokio_tungstenite::accept_async_with_config(stream, Some(ws_config))
        .await
        .map_err(|e| format!("ws handshake failed: {e}"))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    // 1. Challenge the device.
    let nonce = auth::new_nonce();
    send_ws(
        &mut ws_tx,
        &ServerMsg::Challenge {
            nonce: nonce.clone(),
        },
    )
    .await?;

    // 2. Expect Auth, verify the signature proves ownership of the public key.
    let first = ws_rx
        .next()
        .await
        .ok_or("connection closed before auth")?
        .map_err(|e| format!("ws error before auth: {e}"))?;
    let (device_id, public_key_hex) = match parse_client(&first)? {
        ClientMsg::Auth {
            device_id,
            public_key_hex,
            signature_hex,
        } => {
            if let Err(reason) = auth::verify_login(&public_key_hex, &nonce, &signature_hex) {
                let _ = send_ws(&mut ws_tx, &ServerMsg::Error { reason }).await;
                return Err("auth failed".to_string());
            }
            (device_id, public_key_hex)
        }
        other => {
            let _ = send_ws(
                &mut ws_tx,
                &ServerMsg::Error {
                    reason: "expected auth".to_string(),
                },
            )
            .await;
            return Err(format!("expected Auth, got {other:?}"));
        }
    };

    // 3. Register presence. A second login for the same key replaces the old
    //    connection's sender (last-writer-wins); the displaced task will exit
    //    when its send fails.
    let (self_tx, mut self_rx) = mpsc::unbounded_channel::<ServerMsg>();
    register(&registry, &public_key_hex, self_tx.clone());
    send_ws(
        &mut ws_tx,
        &ServerMsg::Welcome {
            device_id: device_id.clone(),
        },
    )
    .await?;

    // 4. Pump: outbound queue -> socket, and inbound socket -> routing.
    let mut rate_limiter = RateLimiter::from_limits(&limits, Instant::now());
    let result = loop {
        tokio::select! {
            outbound = self_rx.recv() => match outbound {
                Some(msg) => {
                    if let Err(e) = send_ws(&mut ws_tx, &msg).await {
                        break Err(e);
                    }
                }
                None => break Ok(()),
            },
            inbound = ws_rx.next() => match inbound {
                None => break Ok(()),
                Some(Err(e)) => break Err(format!("ws read error: {e}")),
                Some(Ok(Message::Close(_))) => break Ok(()),
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                Some(Ok(msg)) => {
                    // Flood protection: a connection over the message-rate limit is
                    // dropped (design §7 抗滥用). Tell it once, then close.
                    if !rate_limiter.allow(Instant::now()) {
                        let _ = send_ws(
                            &mut ws_tx,
                            &ServerMsg::Error {
                                reason: "rate limit exceeded".to_string(),
                            },
                        )
                        .await;
                        break Err("rate limit exceeded".to_string());
                    }
                    if let Err(e) = handle_client_msg(
                        msg,
                        &public_key_hex,
                        &device_id,
                        &registry,
                        &self_tx,
                        &limits,
                    ) {
                        // Bad frame from this client: tell it, keep the session.
                        let _ = self_tx.send(ServerMsg::Error { reason: e });
                    }
                }
            },
        }
    };

    unregister(&registry, &public_key_hex, &self_tx);
    result
}

/// Route a single decoded client frame. Replies/errors are enqueued on
/// `self_tx` so the connection keeps a single writer (the pump loop).
fn handle_client_msg(
    msg: Message,
    from_public_key_hex: &str,
    from_device_id: &str,
    registry: &Registry,
    self_tx: &UnboundedSender<ServerMsg>,
    limits: &Limits,
) -> Result<(), String> {
    match parse_client(&msg)? {
        ClientMsg::Ping => {
            let _ = self_tx.send(ServerMsg::Pong);
            Ok(())
        }
        ClientMsg::Auth { .. } => Err("already authenticated".to_string()),
        ClientMsg::Forward {
            to_public_key_hex,
            session_id,
            kind,
            payload_hex,
        } => {
            // Reject oversized payloads with a clean error (the protocol-layer
            // frame cap is the hard backstop; this is the graceful one).
            if payload_hex.len() > limits.max_payload_hex_len {
                return Err("signaling payload too large".to_string());
            }
            let target = registry.lock().unwrap().get(&to_public_key_hex).cloned();
            match target {
                Some(peer) => {
                    let deliver = ServerMsg::Deliver {
                        from_public_key_hex: from_public_key_hex.to_string(),
                        from_device_id: from_device_id.to_string(),
                        session_id,
                        kind,
                        payload_hex,
                    };
                    // If the peer just dropped, surface it to the sender.
                    peer.send(deliver).map_err(|_| "peer offline".to_string())
                }
                None => Err("peer offline".to_string()),
            }
        }
    }
}

fn parse_client(msg: &Message) -> Result<ClientMsg, String> {
    let text = match msg {
        Message::Text(t) => t.as_str(),
        Message::Binary(b) => std::str::from_utf8(b).map_err(|_| "binary frame not utf8")?,
        _ => return Err("expected a text frame".to_string()),
    };
    ClientMsg::from_json(text).map_err(|e| format!("bad client message: {e}"))
}

async fn send_ws<S>(sink: &mut S, msg: &ServerMsg) -> Result<(), String>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::fmt::Display,
{
    sink.send(Message::Text(msg.to_json()))
        .await
        .map_err(|e| format!("ws send failed: {e}"))
}

fn register(registry: &Registry, public_key_hex: &str, tx: UnboundedSender<ServerMsg>) {
    registry
        .lock()
        .unwrap()
        .insert(public_key_hex.to_string(), tx);
}

/// Remove our entry only if it is still ours — a later login for the same key
/// may have replaced it, and we must not evict the newer connection.
fn unregister(registry: &Registry, public_key_hex: &str, self_tx: &UnboundedSender<ServerMsg>) {
    let mut guard = registry.lock().unwrap();
    if let Some(current) = guard.get(public_key_hex) {
        if current.same_channel(self_tx) {
            guard.remove(public_key_hex);
        }
    }
}
