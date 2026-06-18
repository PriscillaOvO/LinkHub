//! WebRTC DataChannel transport (Stage 5 / M3), behind the `webrtc` feature.
//!
//! Bridges webrtc-rs's **async, message-oriented** DataChannel to the **sync,
//! byte-stream** `Read + Write` that core's Noise KK authenticated session
//! ([`crate::net::run_authenticated_file_sender_over`] /
//! [`crate::net::run_authenticated_responder_over`]) already speaks — see the
//! runtime-architecture decision in `docs/spec/设计-跨网络传输-webrtc.md` §4.5
//! (chosen: keep core sync, pump the DataChannel from a tokio runtime and bridge
//! it to a blocking `Read`/`Write` via a buffer + condvar — the same seam the
//! in-memory `auth_session` test already proves).
//!
//! Establishment is non-trickle: each side gathers ICE to completion and sends
//! one self-contained SDP (offer/answer) through the provided signaling channel
//! ([`SdpSignal`]). The orchestrator wires those channels to a
//! [`crate::net::SignalingClient`]; tests wire them directly.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

/// Max bytes per DataChannel `send` — keeps each SCTP message comfortably within
/// limits. The Noise frame layer above us reassembles, so this is transparent.
const MAX_DC_CHUNK: usize = 16 * 1024;

const ESTABLISH_TIMEOUT: Duration = Duration::from_secs(30);

/// One self-contained SDP exchanged during establishment (ICE candidates are
/// embedded — non-trickle).
#[derive(Debug, Clone)]
pub struct SdpSignal {
    pub is_offer: bool,
    pub sdp: String,
}

struct Inbound {
    buf: VecDeque<u8>,
    closed: bool,
}

struct Shared {
    inbound: Mutex<Inbound>,
    cond: Condvar,
}

/// A WebRTC DataChannel presented as a blocking duplex byte stream.
///
/// Clone to get independent writer/reader handles to the *same* channel (as
/// core's sessions do: `writer` is only written, a `BufReader`-wrapped clone is
/// only read) — both share one inbound buffer and the one underlying channel.
#[derive(Clone)]
pub struct DataChannelDuplex {
    // Kept alive so dropping the duplex doesn't tear down the connection.
    _pc: Arc<RTCPeerConnection>,
    dc: Arc<RTCDataChannel>,
    shared: Arc<Shared>,
    handle: Handle,
}

impl DataChannelDuplex {
    /// Close the channel and signal EOF to any blocked reader.
    pub fn close(&self) {
        let dc = self.dc.clone();
        let _ = self.handle.block_on(async move { dc.close().await });
        let mut guard = self.shared.inbound.lock().unwrap();
        guard.closed = true;
        drop(guard);
        self.shared.cond.notify_all();
    }
}

impl Read for DataChannelDuplex {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut guard = self.shared.inbound.lock().unwrap();
        loop {
            if !guard.buf.is_empty() {
                let n = guard.buf.len().min(out.len());
                for slot in out.iter_mut().take(n) {
                    *slot = guard.buf.pop_front().unwrap();
                }
                return Ok(n);
            }
            if guard.closed {
                return Ok(0);
            }
            guard = self.shared.cond.wait(guard).unwrap();
        }
    }
}

impl Write for DataChannelDuplex {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        for chunk in data.chunks(MAX_DC_CHUNK) {
            let dc = self.dc.clone();
            let bytes = Bytes::copy_from_slice(chunk);
            self.handle
                .block_on(async move { dc.send(&bytes).await })
                .map_err(webrtc_io)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Initiator side: create the DataChannel, send an offer, accept the answer,
/// return once the channel is open.
pub async fn connect_initiator(
    ice_urls: Vec<String>,
    sdp_out: UnboundedSender<SdpSignal>,
    mut sdp_in: UnboundedReceiver<SdpSignal>,
    handle: Handle,
) -> io::Result<DataChannelDuplex> {
    let pc = new_peer_connection(&ice_urls).await?;
    let dc = pc
        .create_data_channel("linkhub", None)
        .await
        .map_err(webrtc_io)?;

    let offer = pc.create_offer(None).await.map_err(webrtc_io)?;
    pc.set_local_description(offer).await.map_err(webrtc_io)?;
    wait_for_ice_gathering(&pc).await;
    let local = pc
        .local_description()
        .await
        .ok_or_else(|| webrtc_io("no local description after gathering"))?;
    sdp_out
        .send(SdpSignal {
            is_offer: true,
            sdp: local.sdp,
        })
        .map_err(|_| signaling_closed())?;

    let answer_signal = sdp_in.recv().await.ok_or_else(signaling_closed)?;
    let answer = RTCSessionDescription::answer(answer_signal.sdp).map_err(webrtc_io)?;
    pc.set_remote_description(answer).await.map_err(webrtc_io)?;

    attach_and_open(pc, dc, handle).await
}

/// Responder side: accept an offer, send back an answer, return once the
/// peer-created DataChannel is open.
pub async fn accept_responder(
    ice_urls: Vec<String>,
    sdp_out: UnboundedSender<SdpSignal>,
    mut sdp_in: UnboundedReceiver<SdpSignal>,
    handle: Handle,
) -> io::Result<DataChannelDuplex> {
    let pc = new_peer_connection(&ice_urls).await?;

    let (dc_tx, dc_rx) = oneshot::channel::<Arc<RTCDataChannel>>();
    let dc_tx = Arc::new(Mutex::new(Some(dc_tx)));
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_tx = dc_tx.clone();
        Box::pin(async move {
            if let Some(tx) = dc_tx.lock().unwrap().take() {
                let _ = tx.send(dc);
            }
        })
    }));

    let offer_signal = sdp_in.recv().await.ok_or_else(signaling_closed)?;
    let offer = RTCSessionDescription::offer(offer_signal.sdp).map_err(webrtc_io)?;
    pc.set_remote_description(offer).await.map_err(webrtc_io)?;
    let answer = pc.create_answer(None).await.map_err(webrtc_io)?;
    pc.set_local_description(answer).await.map_err(webrtc_io)?;
    wait_for_ice_gathering(&pc).await;
    let local = pc
        .local_description()
        .await
        .ok_or_else(|| webrtc_io("no local description after gathering"))?;
    sdp_out
        .send(SdpSignal {
            is_offer: false,
            sdp: local.sdp,
        })
        .map_err(|_| signaling_closed())?;

    let dc = tokio::time::timeout(ESTABLISH_TIMEOUT, dc_rx)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no inbound DataChannel"))?
        .map_err(|_| webrtc_io("DataChannel sender dropped"))?;

    attach_and_open(pc, dc, handle).await
}

async fn new_peer_connection(ice_urls: &[String]) -> io::Result<Arc<RTCPeerConnection>> {
    let api = APIBuilder::new().build();
    let ice_servers = if ice_urls.is_empty() {
        Vec::new()
    } else {
        vec![RTCIceServer {
            urls: ice_urls.to_vec(),
            ..Default::default()
        }]
    };
    let config = RTCConfiguration {
        ice_servers,
        ..Default::default()
    };
    let pc = api.new_peer_connection(config).await.map_err(webrtc_io)?;
    Ok(Arc::new(pc))
}

async fn wait_for_ice_gathering(pc: &RTCPeerConnection) {
    let mut gather_complete = pc.gathering_complete_promise().await;
    let _ = gather_complete.recv().await;
}

/// Register inbound/close handlers on `dc`, wait until it is open, and wrap it
/// in a [`DataChannelDuplex`].
async fn attach_and_open(
    pc: Arc<RTCPeerConnection>,
    dc: Arc<RTCDataChannel>,
    handle: Handle,
) -> io::Result<DataChannelDuplex> {
    let shared = Arc::new(Shared {
        inbound: Mutex::new(Inbound {
            buf: VecDeque::new(),
            closed: false,
        }),
        cond: Condvar::new(),
    });

    let shared_msg = shared.clone();
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let shared = shared_msg.clone();
        Box::pin(async move {
            {
                let mut guard = shared.inbound.lock().unwrap();
                guard.buf.extend(msg.data.iter().copied());
            }
            shared.cond.notify_all();
        })
    }));

    let shared_close = shared.clone();
    dc.on_close(Box::new(move || {
        let shared = shared_close.clone();
        Box::pin(async move {
            {
                let mut guard = shared.inbound.lock().unwrap();
                guard.closed = true;
            }
            shared.cond.notify_all();
        })
    }));

    if dc.ready_state() != RTCDataChannelState::Open {
        let (open_tx, open_rx) = oneshot::channel::<()>();
        let open_tx = Arc::new(Mutex::new(Some(open_tx)));
        dc.on_open(Box::new(move || {
            let open_tx = open_tx.clone();
            Box::pin(async move {
                if let Some(tx) = open_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            })
        }));
        // Re-check to close the race where it opened during handler registration.
        if dc.ready_state() != RTCDataChannelState::Open {
            tokio::time::timeout(ESTABLISH_TIMEOUT, open_rx)
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "DataChannel never opened"))?
                .map_err(|_| webrtc_io("DataChannel open signal dropped"))?;
        }
    }

    Ok(DataChannelDuplex {
        _pc: pc,
        dc,
        shared,
        handle,
    })
}

fn webrtc_io<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}

fn signaling_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "signaling channel closed")
}
