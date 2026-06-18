//! Synchronous WebSocket client for the LinkHub signaling server (M2-step2).
//!
//! Speaks the JSON envelope defined by `signaling-server` (device ↔ server):
//! the server challenges, this client proves ownership of its Ed25519 identity
//! key (domain-separated from the p2p handshake — see
//! [`LocalIdentity::sign_signaling_login`]), then the two relay opaque
//! `payload_hex` signaling blobs to each other addressed by identity public key.
//!
//! Sync `tungstenite` (not async tokio) is used deliberately: it matches core's
//! existing blocking `std::net` networking and keeps this off the Android `.so`'s
//! async surface. The async runtime only appears later for the WebRTC transport
//! (behind the `webrtc` feature). See `docs/spec/设计-跨网络传输-webrtc.md` §4.5.

use std::io;
use std::net::TcpStream;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::LocalIdentity;

/// Client → server frames. Must match `signaling-server` `protocol::ClientMsg`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg<'a> {
    Auth {
        device_id: &'a str,
        public_key_hex: &'a str,
        signature_hex: &'a str,
    },
    Forward {
        to_public_key_hex: &'a str,
        session_id: &'a str,
        kind: &'a str,
        payload_hex: &'a str,
    },
    #[allow(dead_code)]
    Ping,
}

/// Server → client frames. Must match `signaling-server` `protocol::ServerMsg`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg {
    Challenge {
        nonce: String,
    },
    Welcome {
        #[allow(dead_code)]
        device_id: String,
    },
    Deliver {
        from_public_key_hex: String,
        from_device_id: String,
        session_id: String,
        kind: String,
        payload_hex: String,
    },
    Pong,
    Error {
        reason: String,
    },
}

/// A signaling payload relayed from a peer through the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalingDelivery {
    pub from_public_key_hex: String,
    pub from_device_id: String,
    pub session_id: String,
    pub kind: String,
    pub payload_hex: String,
}

/// What a [`SignalingClient::recv`] call yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingEvent {
    /// A peer's signaling payload arrived.
    Delivery(SignalingDelivery),
    /// The server reported a non-fatal error for us (e.g. "peer offline").
    ServerError(String),
}

/// An authenticated, live connection to the signaling server.
pub struct SignalingClient {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    public_key_hex: String,
    device_id: String,
}

impl SignalingClient {
    /// Connect to `ws://host:port`, complete the Ed25519 login challenge, and
    /// return once the server has acknowledged us as present.
    pub fn connect(url: &str, identity: &LocalIdentity) -> io::Result<Self> {
        let (mut ws, _response) = tungstenite::connect(url).map_err(ws_err)?;

        let nonce = match read_server_msg(&mut ws)? {
            ServerMsg::Challenge { nonce } => nonce,
            other => return Err(unexpected("challenge", &other)),
        };

        let signature_hex = identity
            .sign_signaling_login(&nonce)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        send_json(
            &mut ws,
            &ClientMsg::Auth {
                device_id: identity.device_id(),
                public_key_hex: identity.public_key(),
                signature_hex: &signature_hex,
            },
        )?;

        match read_server_msg(&mut ws)? {
            ServerMsg::Welcome { .. } => {}
            ServerMsg::Error { reason } => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("signaling login rejected: {reason}"),
                ))
            }
            other => return Err(unexpected("welcome", &other)),
        }

        Ok(Self {
            ws,
            public_key_hex: identity.public_key().to_string(),
            device_id: identity.device_id().to_string(),
        })
    }

    /// Our own identity public key (how peers address us).
    pub fn public_key_hex(&self) -> &str {
        &self.public_key_hex
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Bound the time a blocking [`Self::recv`] will wait. `None` blocks forever.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self.ws.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            _ => Ok(()),
        }
    }

    /// Relay a signaling payload to `to_public_key_hex` via the server.
    pub fn send_signaling(
        &mut self,
        to_public_key_hex: &str,
        session_id: &str,
        kind: &str,
        payload_hex: &str,
    ) -> io::Result<()> {
        send_json(
            &mut self.ws,
            &ClientMsg::Forward {
                to_public_key_hex,
                session_id,
                kind,
                payload_hex,
            },
        )
    }

    /// Block until the next server frame for us (a peer delivery or a server
    /// error). Pong frames are consumed transparently.
    pub fn recv(&mut self) -> io::Result<SignalingEvent> {
        loop {
            match read_server_msg(&mut self.ws)? {
                ServerMsg::Deliver {
                    from_public_key_hex,
                    from_device_id,
                    session_id,
                    kind,
                    payload_hex,
                } => {
                    return Ok(SignalingEvent::Delivery(SignalingDelivery {
                        from_public_key_hex,
                        from_device_id,
                        session_id,
                        kind,
                        payload_hex,
                    }))
                }
                ServerMsg::Error { reason } => return Ok(SignalingEvent::ServerError(reason)),
                ServerMsg::Pong => continue,
                // Challenge/Welcome are only valid during login; ignore if echoed.
                ServerMsg::Challenge { .. } | ServerMsg::Welcome { .. } => continue,
            }
        }
    }

    /// Close the connection cleanly.
    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

fn send_json<T: Serialize>(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    msg: &T,
) -> io::Result<()> {
    let text = serde_json::to_string(msg)?;
    ws.send(Message::Text(text)).map_err(ws_err)?;
    ws.flush().map_err(ws_err)
}

fn read_server_msg(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> io::Result<ServerMsg> {
    loop {
        match ws.read().map_err(ws_err)? {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            Message::Binary(bytes) => {
                let text = String::from_utf8(bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                return serde_json::from_str(&text)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
            }
            Message::Close(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "signaling server closed the connection",
                ))
            }
            // Control frames are handled by tungstenite; skip and keep reading.
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
}

fn ws_err(err: tungstenite::Error) -> io::Error {
    match err {
        tungstenite::Error::Io(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

fn unexpected(expected: &str, got: &ServerMsg) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("expected {expected}, received {got:?}"),
    )
}
