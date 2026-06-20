//! Tor onion transport (Phase 2), behind the `tor` feature.
//!
//! Bridges Arti's **async** onion [`DataStream`] to the **sync, byte-stream**
//! `Read + Write` that core's Noise KK authenticated session already speaks —
//! the same seam the `webrtc` path uses ([`crate::net::webrtc_transport`]). So an
//! onion connection is just another pipe: the existing
//! [`crate::net::run_authenticated_file_sender_over`] /
//! [`crate::net::run_authenticated_responder_over_with_accept`] run unchanged over
//! it, with Noise KK providing E2E on top of Tor's own onion encryption.
//!
//! Rendezvous needs no server: a peer's v3 `.onion` address is derived from its
//! identity (see [`crate::identity`]'s onion derivation) and was exchanged at
//! pairing, so [`TorContext::connect_onion`] just dials it. The host side serves
//! at exactly that address via [`TorContext::host_onion`], which builds the
//! hidden-service key from the same identity seed.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, Once};

use arti_client::{DataStream, TorClient, TorClientConfig};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Mutex as AsyncMutex;
use tor_cell::relaycell::msg::Connected;
use tor_hscrypto::pk::HsIdKeypair;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::handle_rend_requests;
use tor_llcrypto::pk::ed25519::{ExpandedKeypair, Keypair};
use tor_rtcompat::PreferredRuntime;

/// Max bytes pulled from the onion stream per async read into the inbound buffer.
const READ_CHUNK: usize = 16 * 1024;

/// Virtual port used for the onion connection (onion services multiplex by port;
/// both sides agree on this single port for LinkHub's stream).
const ONION_PORT: u16 = 9_735;

/// Pluggable-transport + bridge settings for reaching the Tor network from a
/// censored network (e.g. obfs4 via lyrebird). `None` = direct (works only where
/// Tor isn't blocked).
#[derive(Clone, Debug)]
pub struct BridgeSettings {
    /// Bridge lines, each like `"Bridge obfs4 <ip:port> <fpr> cert=.. iat-mode=.."`.
    pub bridge_lines: Vec<String>,
    /// Transport protocol name(s) the PT provides, e.g. `["obfs4"]`.
    pub protocols: Vec<String>,
    /// Absolute path to the pluggable-transport client binary (e.g. lyrebird).
    pub pt_binary: String,
}

/// Owns the tokio runtime + bootstrapped Arti client. Kept alive for the duration
/// of any onion connection it creates (its multi-thread runtime drives the
/// background read pumps and the onion-service accept loop).
pub struct TorContext {
    // Field order matters for drop: client before runtime. The builder's
    // `create_bootstrapped` hands back an `Arc<TorClient>`.
    client: Arc<TorClient<PreferredRuntime>>,
    runtime: Runtime,
}

impl TorContext {
    /// Bootstrap a Tor client (optionally through bridges/pluggable transports).
    /// Blocking — bootstrap can take seconds (longer over bridges).
    pub fn bootstrap(bridges: Option<BridgeSettings>) -> io::Result<Self> {
        install_crypto_provider();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let config = build_config(bridges).map_err(other)?;
        let client = runtime
            .block_on(async {
                TorClient::builder()
                    .config(config)
                    .create_bootstrapped()
                    .await
            })
            .map_err(other)?;
        Ok(Self { client, runtime })
    }

    fn handle(&self) -> Handle {
        self.runtime.handle().clone()
    }

    /// Dial a peer's `.onion` address and return a blocking duplex byte stream.
    pub fn connect_onion(&self, onion_address: &str) -> io::Result<OnionStreamDuplex> {
        let target = format!("{}:{}", onion_address.trim(), ONION_PORT);
        let stream = self
            .runtime
            .block_on(async { self.client.connect(target).await })
            .map_err(other)?;
        Ok(OnionStreamDuplex::wrap(stream, self.handle()))
    }

    /// Launch an onion service whose address is the one derived from `hs_seed`
    /// (so it equals the peer-known identity onion address). Returns a listener
    /// that yields each accepted incoming connection as a blocking duplex stream.
    pub fn host_onion(&self, hs_seed: &[u8; 32], nickname: &str) -> io::Result<OnionListener> {
        let id_keypair = hsid_keypair_from_seed(hs_seed);
        let svc_config = OnionServiceConfigBuilder::default()
            .nickname(nickname.to_owned().try_into().map_err(other)?)
            .build()
            .map_err(other)?;
        let (service, rend_requests) = self
            .client
            .launch_onion_service_with_hsid(svc_config, id_keypair)
            .map_err(other)?
            .ok_or_else(|| other("onion service disabled in config"))?;
        let onion_address = service
            .onion_address()
            .ok_or_else(|| other("onion service has no address"))?
            .display_unredacted()
            .to_string();

        let (tx, rx) = flume::unbounded();
        let handle = self.handle();
        let accept_handle = handle.clone();
        handle.spawn(async move {
            let mut requests = handle_rend_requests(rend_requests);
            while let Some(request) = requests.next().await {
                match request.accept(Connected::new_empty()).await {
                    Ok(stream) => {
                        let duplex = OnionStreamDuplex::wrap(stream, accept_handle.clone());
                        if tx.send(duplex).is_err() {
                            break; // listener dropped
                        }
                    }
                    Err(err) => eprintln!("onion accept failed: {err}"),
                }
            }
        });

        Ok(OnionListener {
            onion_address,
            incoming: rx,
            _service: service,
        })
    }
}

/// A running onion service. Dropping it tears the service down. `accept` blocks
/// for the next incoming connection (mirrors a `TcpListener`).
pub struct OnionListener {
    onion_address: String,
    incoming: flume::Receiver<OnionStreamDuplex>,
    _service: Arc<tor_hsservice::RunningOnionService>,
}

impl OnionListener {
    pub fn onion_address(&self) -> &str {
        &self.onion_address
    }

    /// Block until the next peer connects, returning its duplex stream.
    pub fn accept(&self) -> io::Result<OnionStreamDuplex> {
        self.incoming
            .recv()
            .map_err(|_| other("onion listener closed"))
    }
}

struct Inbound {
    buf: VecDeque<u8>,
    closed: bool,
}

struct Shared {
    inbound: Mutex<Inbound>,
    cond: Condvar,
}

/// An onion [`DataStream`] presented as a blocking duplex byte stream. Clone for
/// independent writer/reader handles to the *same* connection (core's sessions
/// write `self` and read a `BufReader`-wrapped clone), as with the WebRTC
/// [`crate::net::webrtc_transport::DataChannelDuplex`].
#[derive(Clone)]
pub struct OnionStreamDuplex {
    shared: Arc<Shared>,
    writer: Arc<AsyncMutex<WriteHalf<DataStream>>>,
    handle: Handle,
}

impl OnionStreamDuplex {
    fn wrap(stream: DataStream, handle: Handle) -> Self {
        let (read_half, write_half) = tokio::io::split(stream);
        let shared = Arc::new(Shared {
            inbound: Mutex::new(Inbound {
                buf: VecDeque::new(),
                closed: false,
            }),
            cond: Condvar::new(),
        });
        spawn_read_pump(&handle, read_half, shared.clone());
        Self {
            shared,
            writer: Arc::new(AsyncMutex::new(write_half)),
            handle,
        }
    }
}

fn spawn_read_pump(handle: &Handle, mut read_half: ReadHalf<DataStream>, shared: Arc<Shared>) {
    handle.spawn(async move {
        let mut buf = [0u8; READ_CHUNK];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let mut guard = shared.inbound.lock().unwrap();
                    guard.closed = true;
                    drop(guard);
                    shared.cond.notify_all();
                    break;
                }
                Ok(n) => {
                    let mut guard = shared.inbound.lock().unwrap();
                    guard.buf.extend(&buf[..n]);
                    drop(guard);
                    shared.cond.notify_all();
                }
            }
        }
    });
}

impl Read for OnionStreamDuplex {
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

impl Write for OnionStreamDuplex {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let writer = self.writer.clone();
        let owned = data.to_vec();
        self.handle.block_on(async move {
            let mut guard = writer.lock().await;
            guard.write_all(&owned).await
        })?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let writer = self.writer.clone();
        self.handle
            .block_on(async move { writer.lock().await.flush().await })
    }
}

/// Build the hidden-service identity keypair from a 32-byte seed (the one core
/// derives from the device's Ed25519 signing key). Its onion address equals the
/// pubkey-only derivation used by `LocalIdentity::onion_address()`.
fn hsid_keypair_from_seed(hs_seed: &[u8; 32]) -> HsIdKeypair {
    let keypair = Keypair::from_bytes(hs_seed);
    HsIdKeypair::from(ExpandedKeypair::from(&keypair))
}

fn build_config(bridges: Option<BridgeSettings>) -> Result<TorClientConfig, String> {
    let Some(bridges) = bridges else {
        return Ok(TorClientConfig::default());
    };

    use arti_client::config::pt::TransportConfigBuilder;
    use arti_client::config::{BridgeConfigBuilder, CfgPath};

    let mut builder = TorClientConfig::builder();
    for line in &bridges.bridge_lines {
        let bridge: BridgeConfigBuilder = line.parse().map_err(|e: _| format!("{e}"))?;
        builder.bridges().bridges().push(bridge);
    }
    let mut transport = TransportConfigBuilder::default();
    let protocols = bridges
        .protocols
        .iter()
        .map(|p| p.parse())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e: _| format!("{e}"))?;
    transport
        .protocols(protocols)
        .path(CfgPath::new(bridges.pt_binary.clone()))
        .run_on_startup(true);
    builder.bridges().transports().push(transport);
    builder.build().map_err(|e| format!("{e}"))
}

fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn other<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}
