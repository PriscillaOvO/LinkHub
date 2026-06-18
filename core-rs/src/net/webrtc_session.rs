//! High-level cross-network WebRTC file transfer (Stage 5 / T6), feature `webrtc`.
//!
//! One place that ties together the pieces the CLI, the desktop (Tauri) and
//! Android (JNI) all need: a [`SignalingClient`] login, the **signed** SDP
//! offer/answer exchange (T3 — [`seal_sdp`]/[`open_sdp`]), DataChannel
//! establishment ([`connect_initiator`]/[`accept_responder`]), and the existing
//! authenticated Noise file transfer running on top. Exposed as two blocking
//! calls so each frontend only has to load identities/trust and call in.
//!
//! The signaling bridge runs on its own thread (a sync [`SignalingClient`]) while
//! a tokio runtime drives the async WebRTC establishment; the two talk over
//! `SdpSignal` channels. Compiled only with `--features webrtc`.

use std::io::{self, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::identity::decode_hex;
use crate::net::webrtc_transport::{
    accept_responder, connect_initiator, DataChannelDuplex, IceConfig, SdpSignal,
};
use crate::net::{
    open_sdp, run_authenticated_file_sender_over, run_authenticated_responder_over, seal_sdp,
    FileReceivedCallback, SignalingClient, SignalingDelivery, SignalingEvent,
};
use crate::{DeviceIdentity, LocalIdentity, TrustStore};

/// How long the bridge waits for the signaling connection to come up.
const SIGNALING_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll cadence for the signaling read loop (so it can observe the stop flag).
const SIGNALING_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Send `file_path` to a trusted peer over a cross-network WebRTC DataChannel,
/// end-to-end encrypted with the existing Noise KK session. Blocks until the
/// transfer completes (or fails).
pub fn send_file_over_webrtc(
    ws_url: &str,
    identity: &LocalIdentity,
    peer_identity: &DeviceIdentity,
    ice: IceConfig,
    file_path: impl AsRef<Path>,
) -> io::Result<()> {
    let peer_dh = dh_key_bytes(peer_identity)?;
    let runtime = new_runtime()?;
    let handle = runtime.handle().clone();
    let session_id = new_session_id(identity.device_id());
    let (local_tx, local_rx) = unbounded_channel::<SdpSignal>();
    let (remote_tx, remote_rx) = unbounded_channel::<SdpSignal>();

    let bridge = start_signaling_bridge(
        ws_url.to_string(),
        identity.clone(),
        SignalingRole::Initiator {
            peer_public_key_hex: peer_identity.public_key().to_string(),
            session_id,
        },
        local_rx,
        remote_tx,
    )?;

    let established = runtime.block_on(connect_initiator(ice, local_tx, remote_rx, handle));
    let bridge_result = bridge.stop();
    let duplex = finish_establishment(established, bridge_result)?;

    let writer = duplex.clone();
    let reader = BufReader::new(duplex.clone());
    let result = run_authenticated_file_sender_over(
        writer,
        reader,
        identity,
        peer_identity.device_id(),
        &peer_dh,
        file_path,
    );
    duplex.close();
    result
}

/// Accept one cross-network WebRTC offer from a *trusted* peer, receive into
/// `receive_dir`, and return when the session ends. Blocks. `on_file` (if set)
/// fires for each completed file, exactly as the LAN listener does.
pub fn receive_file_over_webrtc(
    ws_url: &str,
    identity: LocalIdentity,
    trust_store: Arc<TrustStore>,
    receive_dir: impl AsRef<Path>,
    ice: IceConfig,
    on_file: Option<FileReceivedCallback>,
) -> io::Result<()> {
    let runtime = new_runtime()?;
    let handle = runtime.handle().clone();
    let (local_tx, local_rx) = unbounded_channel::<SdpSignal>();
    let (remote_tx, remote_rx) = unbounded_channel::<SdpSignal>();

    let bridge = start_signaling_bridge(
        ws_url.to_string(),
        identity.clone(),
        SignalingRole::Responder {
            trust_store: Arc::clone(&trust_store),
        },
        local_rx,
        remote_tx,
    )?;

    let established = runtime.block_on(accept_responder(ice, local_tx, remote_rx, handle));
    let bridge_result = bridge.stop();
    let duplex = finish_establishment(established, bridge_result)?;

    let writer = duplex.clone();
    let reader = BufReader::new(duplex.clone());
    let result = run_authenticated_responder_over(
        writer,
        reader,
        identity,
        trust_store,
        receive_dir,
        on_file,
    );
    duplex.close();
    result
}

enum SignalingRole {
    Initiator {
        peer_public_key_hex: String,
        session_id: String,
    },
    Responder {
        trust_store: Arc<TrustStore>,
    },
}

struct RunningSignalingBridge {
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<Result<(), String>>,
}

impl RunningSignalingBridge {
    fn stop(self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .join()
            .map_err(|_| "signaling bridge thread panicked".to_string())?
    }
}

/// Spawn the signaling thread: log in, then shuttle signed SDP between the local
/// WebRTC engine (`outbound_sdp`/`inbound_sdp`) and the peer via the server.
fn start_signaling_bridge(
    ws_url: String,
    identity: LocalIdentity,
    role: SignalingRole,
    mut outbound_sdp: UnboundedReceiver<SdpSignal>,
    inbound_sdp: UnboundedSender<SdpSignal>,
) -> io::Result<RunningSignalingBridge> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let handle = thread::spawn(move || {
        let mut client = match SignalingClient::connect(&ws_url, &identity) {
            Ok(client) => client,
            Err(err) => {
                let message = format!("failed to connect to signaling server {ws_url}: {err}");
                let _ = ready_tx.send(Err(message.clone()));
                return Err(message);
            }
        };
        if let Err(err) = client.set_read_timeout(Some(SIGNALING_POLL_INTERVAL)) {
            let message = format!("failed to configure signaling read timeout: {err}");
            let _ = ready_tx.send(Err(message.clone()));
            return Err(message);
        }
        let _ = ready_tx.send(Ok(()));

        let mut active_session_id = match &role {
            SignalingRole::Initiator { session_id, .. } => Some(session_id.clone()),
            SignalingRole::Responder { .. } => None,
        };
        let mut target_public_key_hex = match &role {
            SignalingRole::Initiator {
                peer_public_key_hex,
                ..
            } => Some(peer_public_key_hex.clone()),
            SignalingRole::Responder { .. } => None,
        };

        loop {
            drain_outbound_sdp(
                &mut client,
                &identity,
                &mut outbound_sdp,
                active_session_id.as_deref(),
                target_public_key_hex.as_deref(),
            )?;

            if stop_for_thread.load(Ordering::Relaxed) {
                client.close();
                return Ok(());
            }

            match client.recv() {
                Ok(SignalingEvent::Delivery(delivery)) => {
                    if !accept_signaling_delivery(
                        &role,
                        &delivery,
                        &mut active_session_id,
                        &mut target_public_key_hex,
                    ) {
                        continue;
                    }
                    let signal = delivery_to_sdp_signal(&delivery)?;
                    inbound_sdp
                        .send(signal)
                        .map_err(|_| "WebRTC SDP receiver closed".to_string())?;
                }
                Ok(SignalingEvent::ServerError(reason)) => {
                    return Err(format!("signaling server error: {reason}"));
                }
                Err(err) if is_poll_timeout(&err) => continue,
                Err(err) => return Err(format!("signaling connection ended: {err}")),
            }
        }
    });

    match ready_rx.recv_timeout(SIGNALING_CONNECT_TIMEOUT) {
        Ok(Ok(())) => Ok(RunningSignalingBridge { stop, handle }),
        Ok(Err(message)) => {
            let _ = handle.join();
            Err(io::Error::other(message))
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out connecting to signaling server",
            ))
        }
    }
}

/// Sign and forward any locally-produced SDP to the peer.
fn drain_outbound_sdp(
    client: &mut SignalingClient,
    identity: &LocalIdentity,
    outbound_sdp: &mut UnboundedReceiver<SdpSignal>,
    session_id: Option<&str>,
    target_public_key_hex: Option<&str>,
) -> Result<(), String> {
    loop {
        match outbound_sdp.try_recv() {
            Ok(signal) => {
                let session_id =
                    session_id.ok_or_else(|| "no active signaling session".to_string())?;
                let target = target_public_key_hex
                    .ok_or_else(|| "no signaling target public key".to_string())?;
                let kind = if signal.is_offer { "offer" } else { "answer" };
                // T3: sign the SDP so the peer can detect server tampering.
                let payload_hex = seal_sdp(identity, session_id, kind, &signal.sdp)
                    .map_err(|err| format!("failed to sign WebRTC {kind}: {err}"))?;
                client
                    .send_signaling(target, session_id, kind, &payload_hex)
                    .map_err(|err| format!("failed to relay WebRTC {kind}: {err}"))?;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(()),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Decide whether a delivery is the peer message we are waiting for (right
/// session, right role, trusted sender), latching the responder's session/target
/// on the first accepted offer.
fn accept_signaling_delivery(
    role: &SignalingRole,
    delivery: &SignalingDelivery,
    active_session_id: &mut Option<String>,
    target_public_key_hex: &mut Option<String>,
) -> bool {
    if let Some(session_id) = active_session_id.as_deref() {
        if delivery.session_id != session_id {
            return false;
        }
    }

    match role {
        SignalingRole::Initiator {
            peer_public_key_hex,
            ..
        } => delivery.kind == "answer" && delivery.from_public_key_hex == *peer_public_key_hex,
        SignalingRole::Responder { trust_store } => {
            if delivery.kind != "offer" {
                return false;
            }
            let Some(trusted) = trust_store.trusted_device(&delivery.from_device_id) else {
                return false;
            };
            if trusted.identity().public_key() != delivery.from_public_key_hex {
                return false;
            }
            if active_session_id.is_none() {
                *active_session_id = Some(delivery.session_id.clone());
                *target_public_key_hex = Some(delivery.from_public_key_hex.clone());
            }
            true
        }
    }
}

/// Verify the signed SDP (T3) against the already-vetted peer key, then unwrap it.
fn delivery_to_sdp_signal(delivery: &SignalingDelivery) -> Result<SdpSignal, String> {
    let sdp = open_sdp(
        &delivery.from_public_key_hex,
        &delivery.session_id,
        &delivery.kind,
        &delivery.payload_hex,
    )
    .map_err(|err| format!("rejected unsigned/tampered WebRTC {}: {err}", delivery.kind))?;
    Ok(SdpSignal {
        is_offer: delivery.kind == "offer",
        sdp,
    })
}

fn finish_establishment(
    established: io::Result<DataChannelDuplex>,
    bridge_result: Result<(), String>,
) -> io::Result<DataChannelDuplex> {
    match (established, bridge_result) {
        (Ok(duplex), Ok(())) => Ok(duplex),
        (Err(err), Ok(())) => Err(io::Error::other(format!(
            "failed to establish WebRTC DataChannel: {err}"
        ))),
        (_, Err(err)) => Err(io::Error::other(err)),
    }
}

fn new_runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
}

fn is_poll_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn new_session_id(device_id: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("webrtc-{}-{millis}", sanitize_token(device_id))
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn dh_key_bytes(identity: &DeviceIdentity) -> io::Result<[u8; 32]> {
    let bytes = decode_hex(identity.dh_public_key()).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad peer dh key: {err}"),
        )
    })?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("peer dh key must be 32 bytes, got {}", bytes.len()),
        )
    })
}
