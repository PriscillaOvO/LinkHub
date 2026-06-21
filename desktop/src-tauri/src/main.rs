// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use linkhub_core::{
    plan_connection, AcceptPeerCallback, ConnectionPath, DeviceIdentity, DiscoveryEndpoint,
    IncomingPeer, LocalIdentity, MdnsAdvertisement, MdnsRegistration, MdnsRuntime,
    PairingInvitation, PairingSession, PeerReachability, TrustStore, TrustedDevice,
};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::net::{TcpListener, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// Global listener state shared across commands.
struct ListenerState {
    running: AtomicBool,
    bind_addr: Mutex<String>,
    last_error: Mutex<String>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn listener_state() -> &'static ListenerState {
    static STATE: OnceLock<ListenerState> = OnceLock::new();
    STATE.get_or_init(|| ListenerState {
        running: AtomicBool::new(false),
        bind_addr: Mutex::new("127.0.0.1:8787".to_string()),
        last_error: Mutex::new(String::new()),
        handle: Mutex::new(None),
    })
}

struct WebRtcReceiverState {
    running: AtomicBool,
    stopping: AtomicBool,
    completed_sessions: Mutex<u64>,
    last_error: Mutex<String>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn webrtc_receiver_state() -> &'static WebRtcReceiverState {
    static STATE: OnceLock<WebRtcReceiverState> = OnceLock::new();
    STATE.get_or_init(|| WebRtcReceiverState {
        running: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        completed_sessions: Mutex::new(0),
        last_error: Mutex::new(String::new()),
        stop: Mutex::new(None),
        handle: Mutex::new(None),
    })
}

fn reap_webrtc_receiver_if_finished(state: &WebRtcReceiverState) {
    let mut handle_guard = state.handle.lock().unwrap();
    let finished = handle_guard
        .as_ref()
        .map(|handle| handle.is_finished())
        .unwrap_or(false);
    if finished {
        if let Some(handle) = handle_guard.take() {
            let _ = handle.join();
        }
        *state.stop.lock().unwrap() = None;
    }
}

struct MdnsAdvertiseHandle {
    runtime: MdnsRuntime,
    registration: MdnsRegistration,
}

fn mdns_advertise_state() -> &'static Mutex<Option<MdnsAdvertiseHandle>> {
    static STATE: OnceLock<Mutex<Option<MdnsAdvertiseHandle>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

struct IncomingPeerPending {
    payload: IncomingPeerPromptPayload,
    responder: mpsc::Sender<bool>,
}

fn incoming_peer_pending() -> &'static Mutex<Option<IncomingPeerPending>> {
    static STATE: OnceLock<Mutex<Option<IncomingPeerPending>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn next_incoming_peer_request_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

// ── Response types ─────────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct QrPayload {
    qr_svg: String,
    payload: String,
    device_name: String,
    device_id: String,
    fingerprint: String,
    ttl_seconds: u64,
}

#[derive(Clone, Serialize)]
struct PeerInfo {
    device_name: String,
    device_id: String,
    fingerprint: String,
    confirmation_code: String,
}

#[derive(Clone, Serialize)]
struct TrustedPeer {
    device_id: String,
    device_name: String,
    fingerprint: String,
}

#[derive(Clone, Serialize)]
struct DiscoveredPeer {
    device_id: String,
    device_name: String,
    address: String,
    fingerprint: String,
    public_key: String,
    dh_public_key: String,
    binding_sig: String,
    trusted: bool,
}

#[derive(Clone, Serialize)]
struct IncomingPeerPromptPayload {
    request_id: u64,
    device_id: String,
    device_name: String,
    public_key: String,
    dh_public_key: String,
    fingerprint: String,
    // The peer's advertised .onion (if any), carried through so it is persisted
    // with the device on accept, enabling later Tor reconnect with no signaling.
    onion_address: Option<String>,
}

#[derive(Clone, Serialize)]
struct StatusSnapshot {
    local_device_id: String,
    local_device_name: String,
    local_fingerprint: String,
    trusted_devices: Vec<TrustedPeer>,
}

#[derive(Clone, Serialize)]
struct SendResult {
    success: bool,
    message_id: String,
    detail: String,
}

#[derive(Clone, Serialize)]
struct WebRtcReceiverStatus {
    running: bool,
    stopping: bool,
    completed_sessions: u64,
    error: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct HistoryEntry {
    timestamp: String,
    direction: String,
    peer_device_id: String,
    peer_device_name: String,
    kind: String,
    content_preview: String,
    status: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct TransmissionHistory {
    entries: Vec<HistoryEntry>,
}

/// Best-effort cross-platform data dir, used only when no path is supplied by
/// the frontend. Mirrors Tauri's `app_data_dir` layout (`<data>/com.linkhub.desktop`)
/// without needing an `AppHandle`.
fn fallback_data_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("com.linkhub.desktop")
}

fn history_fallback() -> &'static str {
    static FALLBACK: OnceLock<String> = OnceLock::new();
    FALLBACK
        .get_or_init(|| {
            fallback_data_dir()
                .join("history.json")
                .display()
                .to_string()
        })
        .as_str()
}

fn resolved_history_path(history_path: &str) -> &str {
    if history_path.trim().is_empty() {
        history_fallback()
    } else {
        history_path
    }
}

fn append_history(history_path: &str, entry: HistoryEntry) -> Result<(), String> {
    let path = resolved_history_path(history_path);
    let mut history = TransmissionHistory { entries: vec![] };
    if std::path::Path::new(path).exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        history = serde_json::from_str(&raw).unwrap_or(TransmissionHistory { entries: vec![] });
    }
    history.entries.push(entry);
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&history).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn append_send_history(
    history_path: &str,
    peer_device_id: &str,
    peer_device_name: &str,
    kind: &str,
    content_preview: String,
    status: &str,
) {
    let _ = append_history(
        history_path,
        HistoryEntry {
            timestamp: now_iso(),
            direction: "sent".into(),
            peer_device_id: peer_device_id.to_string(),
            peer_device_name: peer_device_name.to_string(),
            kind: kind.to_string(),
            content_preview,
            status: status.to_string(),
        },
    );
}

fn now_iso() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ── Helper ─────────────────────────────────────────────────────────

fn load_identity(path: &str) -> Result<LocalIdentity, String> {
    match path.strip_prefix("secure:") {
        Some(secure_path) => LocalIdentity::load_from_secure_path(secure_path)
            .map_err(|err| format!("failed to load secure identity: {err}")),
        None => LocalIdentity::load_from_path(path)
            .map_err(|err| format!("failed to load identity: {err}")),
    }
}

fn prompt_payload_from_peer(request_id: u64, peer: &IncomingPeer) -> IncomingPeerPromptPayload {
    IncomingPeerPromptPayload {
        request_id,
        device_id: peer.device_id.clone(),
        device_name: peer.device_name.clone(),
        public_key: peer.public_key.clone(),
        dh_public_key: peer.dh_public_key.clone(),
        fingerprint: peer.fingerprint.clone(),
        onion_address: peer.onion_address.clone(),
    }
}

fn identity_from_prompt_payload(payload: &IncomingPeerPromptPayload) -> DeviceIdentity {
    DeviceIdentity::new(
        payload.device_id.clone(),
        payload.device_name.clone(),
        payload.public_key.clone(),
        payload.dh_public_key.clone(),
    )
    .with_onion_address(payload.onion_address.clone())
}

fn trust_incoming_peer(
    payload: &IncomingPeerPromptPayload,
    trust_store_path: &str,
) -> Result<(), String> {
    let identity = identity_from_prompt_payload(payload);
    if !identity.has_consistent_device_id() {
        return Err("incoming peer identity has inconsistent device_id".to_string());
    }

    let mut store = TrustStore::load_from_path(trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    store.trust(TrustedDevice::new(identity, SystemTime::now()));
    store
        .save_to_path(trust_store_path)
        .map_err(|e| format!("failed to save trust store: {e}"))
}

fn make_desktop_accept_callback(
    app: tauri::AppHandle,
    trust_store_path: String,
) -> AcceptPeerCallback {
    Arc::new(move |peer: IncomingPeer| {
        let request_id = next_incoming_peer_request_id();
        let payload = prompt_payload_from_peer(request_id, &peer);
        let (tx, rx) = mpsc::channel();

        {
            let mut pending = incoming_peer_pending().lock().unwrap();
            if pending.is_some() {
                return false;
            }
            *pending = Some(IncomingPeerPending {
                payload: payload.clone(),
                responder: tx,
            });
        }

        focus_main_window(&app);
        let _ = app.emit("incoming-peer", payload.clone());
        let accepted = rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false);

        let mut pending = incoming_peer_pending().lock().unwrap();
        let should_clear = pending
            .as_ref()
            .map(|pending| pending.payload.request_id == request_id)
            .unwrap_or(false);
        if should_clear {
            *pending = None;
        }

        if accepted {
            TrustStore::load_from_path(&trust_store_path)
                .map(|store| store.is_trusted(&payload.device_id))
                .unwrap_or(false)
        } else {
            false
        }
    })
}

#[tauri::command]
fn pending_incoming_peer() -> Option<IncomingPeerPromptPayload> {
    incoming_peer_pending()
        .lock()
        .unwrap()
        .as_ref()
        .map(|pending| pending.payload.clone())
}

#[tauri::command]
fn respond_incoming_peer(
    request_id: u64,
    accept: bool,
    trust_store_path: String,
) -> Result<(), String> {
    let pending = {
        let mut guard = incoming_peer_pending().lock().unwrap();
        match guard.as_ref() {
            Some(pending) if pending.payload.request_id == request_id => guard.take(),
            Some(_) => return Err("incoming peer request is no longer current".to_string()),
            None => return Ok(()),
        }
    };

    let Some(pending) = pending else {
        return Ok(());
    };

    if accept {
        if let Err(err) = trust_incoming_peer(&pending.payload, &trust_store_path) {
            let _ = pending.responder.send(false);
            return Err(err);
        }
    }

    let _ = pending.responder.send(accept);
    Ok(())
}

// ── Default paths (cross-platform) ─────────────────────────────────

#[derive(Clone, Serialize)]
struct DefaultConfig {
    identity_path: String,
    trust_store_path: String,
    receive_dir: String,
    history_path: String,
    listener_addr: String,
}

/// OS-appropriate default paths under the app data dir, so the desktop client
/// is not pinned to `C:\LinkHub` and can run on macOS/Linux. The frontend seeds
/// these only when a setting is not already stored (existing setups untouched).
#[tauri::command]
fn default_config(app: tauri::AppHandle) -> DefaultConfig {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| fallback_data_dir());
    let join = |name: &str| base.join(name).display().to_string();
    // `secure:` (DPAPI) identity is only wired up on Windows; until the macOS
    // Keychain / Linux Secret Service backends land, default to a plaintext
    // identity file off-Windows.
    let identity_path = if cfg!(windows) {
        format!(
            "secure:{}",
            base.join("local-identity.secure.txt").display()
        )
    } else {
        base.join("local-identity.txt").display().to_string()
    };
    DefaultConfig {
        identity_path,
        trust_store_path: join("trust-store.txt"),
        receive_dir: join("inbox"),
        history_path: join("history.json"),
        listener_addr: "127.0.0.1:8787".to_string(),
    }
}

// ── Pairing commands ───────────────────────────────────────────────

#[tauri::command]
fn pairing_generate_qr(identity_path: String, ttl_seconds: u64) -> Result<QrPayload, String> {
    if ttl_seconds == 0 {
        return Err("ttl_seconds must be greater than zero".to_string());
    }
    let identity = load_identity(&identity_path)?;
    let invitation = PairingInvitation::new(
        identity.identity().clone(),
        SystemTime::now(),
        std::time::Duration::from_secs(ttl_seconds),
    );
    let payload = invitation.to_payload();
    let qr = QrCode::new(&payload).map_err(|e| format!("qr error: {e}"))?;
    let qr_svg = qr
        .render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#1a1a2e"))
        .light_color(svg::Color("#ffffff"))
        .build();

    Ok(QrPayload {
        qr_svg,
        payload,
        device_name: identity.device_name().to_string(),
        device_id: identity.device_id().to_string(),
        fingerprint: identity.identity().fingerprint(),
        ttl_seconds,
    })
}

#[tauri::command]
fn pairing_inspect(identity_path: String, payload: String) -> Result<PeerInfo, String> {
    let identity = load_identity(&identity_path)?;
    let invitation =
        PairingInvitation::from_payload(&payload, SystemTime::now()).map_err(|e| e.to_string())?;
    let session = PairingSession::new(identity.identity().clone(), invitation);
    Ok(PeerInfo {
        device_name: session.peer_identity().device_name().to_string(),
        device_id: session.peer_identity().device_id().to_string(),
        fingerprint: session.peer_identity().fingerprint(),
        confirmation_code: session.confirmation_code(),
    })
}

#[tauri::command]
fn pairing_confirm(
    identity_path: String,
    payload: String,
    confirmation_code: String,
    trust_store_path: String,
) -> Result<TrustedPeer, String> {
    let identity = load_identity(&identity_path)?;
    let invitation =
        PairingInvitation::from_payload(&payload, SystemTime::now()).map_err(|e| e.to_string())?;
    let session = PairingSession::new(identity.identity().clone(), invitation);
    let trusted = session
        .confirm(&confirmation_code, SystemTime::now(), SystemTime::now())
        .map_err(|e| format!("pairing failed: {e}"))?;
    let device_id = trusted.device_id().to_string();
    let device_name = trusted.device_name().to_string();
    let fingerprint = trusted.fingerprint().to_string();

    let mut store = TrustStore::load_from_path(&trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    store.trust(trusted);
    store
        .save_to_path(&trust_store_path)
        .map_err(|e| format!("failed to save trust store: {e}"))?;

    Ok(TrustedPeer {
        device_id,
        device_name,
        fingerprint,
    })
}

// ── Identity commands ──────────────────────────────────────────────

#[tauri::command]
fn identity_init(identity_path: String, device_name: String) -> Result<StatusSnapshot, String> {
    let identity = match identity_path.strip_prefix("secure:") {
        Some(secure_path) => {
            LocalIdentity::load_or_generate_secure(secure_path, &device_name, SystemTime::now())
        }
        None => LocalIdentity::load_or_generate(
            Path::new(&identity_path),
            &device_name,
            SystemTime::now(),
        ),
    }
    .map_err(|e| format!("failed to init identity: {e}"))?;
    Ok(StatusSnapshot {
        local_device_id: identity.device_id().to_string(),
        local_device_name: identity.device_name().to_string(),
        local_fingerprint: identity.identity().fingerprint(),
        trusted_devices: vec![],
    })
}

#[tauri::command]
fn identity_load(identity_path: String) -> Result<StatusSnapshot, String> {
    let identity = load_identity(&identity_path)?;
    Ok(StatusSnapshot {
        local_device_id: identity.device_id().to_string(),
        local_device_name: identity.device_name().to_string(),
        local_fingerprint: identity.identity().fingerprint(),
        trusted_devices: vec![],
    })
}

// ── Status commands ────────────────────────────────────────────────

#[tauri::command]
fn get_local_status(
    identity_path: String,
    trust_store_path: String,
) -> Result<StatusSnapshot, String> {
    let identity = load_identity(&identity_path)?;
    let store = TrustStore::load_from_path(&trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    let trusted_devices = store
        .trusted_devices()
        .into_iter()
        .map(|d| TrustedPeer {
            device_id: d.device_id().to_string(),
            device_name: d.device_name().to_string(),
            fingerprint: d.fingerprint().to_string(),
        })
        .collect();
    Ok(StatusSnapshot {
        local_device_id: identity.device_id().to_string(),
        local_device_name: identity.device_name().to_string(),
        local_fingerprint: identity.identity().fingerprint(),
        trusted_devices,
    })
}

fn trusted_discovered_peers(
    store: &TrustStore,
    endpoints: Vec<DiscoveryEndpoint>,
) -> Vec<DiscoveredPeer> {
    discovered_peers(store, endpoints)
        .into_iter()
        .filter(|peer| peer.trusted)
        .collect()
}

fn discovered_peers(store: &TrustStore, endpoints: Vec<DiscoveryEndpoint>) -> Vec<DiscoveredPeer> {
    let mut peers = endpoints
        .into_iter()
        .filter_map(|endpoint| discovered_peer_from_endpoint(store, endpoint))
        .collect::<Vec<_>>();
    peers.sort_by(|left, right| {
        left.device_name
            .cmp(&right.device_name)
            .then_with(|| left.device_id.cmp(&right.device_id))
            .then_with(|| left.address.cmp(&right.address))
    });
    peers
        .dedup_by(|left, right| left.device_id == right.device_id && left.address == right.address);
    peers
}

fn discovered_peer_from_endpoint(
    store: &TrustStore,
    endpoint: DiscoveryEndpoint,
) -> Option<DiscoveredPeer> {
    if let Some(trusted) = store.trusted_device(endpoint.device_id()) {
        let identity = trusted.identity();
        return Some(DiscoveredPeer {
            device_id: identity.device_id().to_string(),
            device_name: identity.device_name().to_string(),
            address: endpoint.addr().to_string(),
            fingerprint: identity.fingerprint(),
            public_key: identity.public_key().to_string(),
            dh_public_key: identity.dh_public_key().to_string(),
            binding_sig: endpoint.binding_sig().to_string(),
            trusted: true,
        });
    }

    let identity = verified_endpoint_identity(&endpoint)?;
    Some(DiscoveredPeer {
        device_id: identity.device_id().to_string(),
        device_name: identity.device_name().to_string(),
        address: endpoint.addr().to_string(),
        fingerprint: identity.fingerprint(),
        public_key: identity.public_key().to_string(),
        dh_public_key: identity.dh_public_key().to_string(),
        binding_sig: endpoint.binding_sig().to_string(),
        trusted: false,
    })
}

fn verified_endpoint_identity(endpoint: &DiscoveryEndpoint) -> Option<DeviceIdentity> {
    if endpoint.public_key().trim().is_empty()
        || endpoint.dh_public_key().trim().is_empty()
        || endpoint.binding_sig().trim().is_empty()
    {
        return None;
    }

    let identity = DeviceIdentity::new(
        endpoint.device_id().to_string(),
        endpoint.device_name().to_string(),
        endpoint.public_key().to_string(),
        endpoint.dh_public_key().to_string(),
    );
    if !identity.has_consistent_device_id() {
        return None;
    }
    if !identity
        .verify_identity_binding(endpoint.binding_sig())
        .ok()?
    {
        return None;
    }
    Some(identity)
}

fn verified_identity_from_fields(
    peer_device_id: &str,
    peer_device_name: &str,
    peer_public_key: &str,
    peer_dh_public_key: &str,
    binding_sig: &str,
) -> Result<DeviceIdentity, String> {
    let identity = DeviceIdentity::new(
        peer_device_id.to_string(),
        peer_device_name.to_string(),
        peer_public_key.to_string(),
        peer_dh_public_key.to_string(),
    );
    if !identity.has_consistent_device_id() {
        return Err("discovered peer identity has inconsistent device_id".to_string());
    }
    if binding_sig.trim().is_empty() {
        return Err("discovered peer is missing identity binding signature".to_string());
    }
    let verified = identity
        .verify_identity_binding(binding_sig)
        .map_err(|err| format!("invalid discovered peer identity binding: {err}"))?;
    if !verified {
        return Err("discovered peer identity binding did not verify".to_string());
    }
    Ok(identity)
}

#[tauri::command]
async fn scan_trusted_mdns(
    trust_store_path: String,
    timeout_seconds: u64,
) -> Result<Vec<DiscoveredPeer>, String> {
    let timeout = timeout_seconds.clamp(1, 15);
    let store = TrustStore::load_from_path(&trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    let runtime = MdnsRuntime::new().map_err(|e| format!("failed to start mDNS scanner: {e}"))?;
    let endpoints = runtime
        .browse_for(Duration::from_secs(timeout))
        .map_err(|e| format!("mDNS scan failed: {e}"))?;
    let _ = runtime.shutdown();
    Ok(trusted_discovered_peers(&store, endpoints))
}

#[tauri::command]
async fn scan_mdns(
    trust_store_path: String,
    timeout_seconds: u64,
) -> Result<Vec<DiscoveredPeer>, String> {
    let timeout = timeout_seconds.clamp(1, 15);
    let store = TrustStore::load_from_path(&trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    let runtime = MdnsRuntime::new().map_err(|e| format!("failed to start mDNS scanner: {e}"))?;
    let endpoints = runtime
        .browse_for(Duration::from_secs(timeout))
        .map_err(|e| format!("mDNS scan failed: {e}"))?;
    let _ = runtime.shutdown();
    Ok(discovered_peers(&store, endpoints))
}

// ── Send commands ──────────────────────────────────────────────────

#[tauri::command]
fn choose_file_path(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(path) = app.dialog().file().blocking_pick_file() else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|e| format!("failed to read selected path: {e}"))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
fn choose_folder_path(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(path) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|e| format!("failed to read selected path: {e}"))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
async fn send_encrypted_text(
    peer_addr: String,
    identity_path: String,
    peer_device_id: String,
    trust_store_path: String,
    history_path: String,
    text: String,
) -> Result<SendResult, String> {
    let identity = load_identity(&identity_path)?;
    let store = TrustStore::load_from_path(&trust_store_path).map_err(|e| e.to_string())?;
    let peer = store
        .trusted_device(&peer_device_id)
        .ok_or_else(|| format!("peer device '{peer_device_id}' not in trust store"))?;
    let dh_hex = peer.identity().dh_public_key();
    let dh_bytes = linkhub_core::decode_hex(dh_hex).map_err(|e| e.to_string())?;
    let dh_bytes: [u8; 32] = dh_bytes
        .try_into()
        .map_err(|_| "dh key must be 32 bytes".to_string())?;

    if let Err(err) = linkhub_core::run_authenticated_text_sender(
        &peer_addr,
        identity,
        &peer_device_id,
        &dh_bytes,
        &text,
    ) {
        let detail = format!("failed: {err}");
        append_send_history(
            &history_path,
            &peer_device_id,
            peer.device_name(),
            "text",
            text.chars().take(100).collect(),
            &detail,
        );
        return Err(format!("send failed: {err}"));
    }

    append_send_history(
        &history_path,
        &peer_device_id,
        peer.device_name(),
        "text",
        text.chars().take(100).collect(),
        "success",
    );

    Ok(SendResult {
        success: true,
        message_id: String::new(),
        detail: "文本已发送并确认".to_string(),
    })
}

#[tauri::command]
async fn send_encrypted_file(
    peer_addr: String,
    identity_path: String,
    peer_device_id: String,
    trust_store_path: String,
    history_path: String,
    file_path: String,
) -> Result<SendResult, String> {
    let identity = load_identity(&identity_path)?;
    let store = TrustStore::load_from_path(&trust_store_path).map_err(|e| e.to_string())?;
    let peer = store
        .trusted_device(&peer_device_id)
        .ok_or_else(|| format!("peer device '{peer_device_id}' not in trust store"))?;
    let dh_hex = peer.identity().dh_public_key();
    let dh_bytes = linkhub_core::decode_hex(dh_hex).map_err(|e| e.to_string())?;
    let dh_bytes: [u8; 32] = dh_bytes
        .try_into()
        .map_err(|_| "dh key must be 32 bytes".to_string())?;

    let fname = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file_path)
        .to_string();

    if let Err(err) = linkhub_core::run_authenticated_file_sender(
        &peer_addr,
        identity,
        &peer_device_id,
        &dh_bytes,
        &file_path,
    ) {
        let detail = format!("failed: {err}");
        append_send_history(
            &history_path,
            &peer_device_id,
            peer.device_name(),
            "file",
            fname,
            &detail,
        );
        return Err(format!("send failed: {err}"));
    }

    append_send_history(
        &history_path,
        &peer_device_id,
        peer.device_name(),
        "file",
        fname,
        "success",
    );

    Ok(SendResult {
        success: true,
        message_id: String::new(),
        detail: "文件已发送并确认".to_string(),
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn send_file_to_discovered(
    peer_addr: String,
    identity_path: String,
    peer_device_id: String,
    peer_device_name: String,
    peer_public_key: String,
    peer_dh_public_key: String,
    binding_sig: String,
    trust_store_path: String,
    history_path: String,
    file_path: String,
) -> Result<SendResult, String> {
    let identity = load_identity(&identity_path)?;
    let peer_identity = if binding_sig.trim().is_empty() {
        let store = TrustStore::load_from_path(&trust_store_path)
            .map_err(|e| format!("failed to load trust store: {e}"))?;
        store
            .trusted_device(&peer_device_id)
            .map(|device| device.identity().clone())
            .ok_or_else(|| {
                format!("discovered peer '{peer_device_id}' is not trusted and has no signature")
            })?
    } else {
        verified_identity_from_fields(
            &peer_device_id,
            &peer_device_name,
            &peer_public_key,
            &peer_dh_public_key,
            &binding_sig,
        )?
    };
    let dh_bytes = linkhub_core::decode_hex(peer_identity.dh_public_key())
        .map_err(|e| format!("invalid discovered peer dh key: {e}"))?;
    let dh_bytes: [u8; 32] = dh_bytes
        .try_into()
        .map_err(|_| "discovered peer dh key must be 32 bytes".to_string())?;
    let fname = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file_path)
        .to_string();

    if let Err(err) = linkhub_core::run_authenticated_file_sender(
        &peer_addr,
        identity,
        peer_identity.device_id(),
        &dh_bytes,
        &file_path,
    ) {
        let detail = format!("failed: {err}");
        append_send_history(
            &history_path,
            peer_identity.device_id(),
            peer_identity.device_name(),
            "file",
            fname,
            &detail,
        );
        return Err(format!("send failed: {err}"));
    }

    append_send_history(
        &history_path,
        peer_identity.device_id(),
        peer_identity.device_name(),
        "file",
        fname,
        "success",
    );

    Ok(SendResult {
        success: true,
        message_id: String::new(),
        detail: "文件已发送并确认".to_string(),
    })
}

// ── Cross-network (WebRTC) commands ────────────────────────────────

#[derive(Clone, Serialize)]
struct TransportPath {
    /// Stable machine id: "lan" | "webrtc" | "relay".
    kind: String,
    /// Human label for the UI (中文).
    label: String,
    /// Optional detail (e.g. the LAN address).
    detail: String,
}

/// Ordered transport plan for a peer (LAN 直连 → WebRTC 打洞 → 中继), driven by
/// core's `plan_connection`/`ConnectionPath`. The UI shows this so the user knows
/// which path a transfer will take. Pure — no network, no `webrtc` feature.
#[tauri::command]
fn connection_plan(
    lan_addr: Option<String>,
    signaling_available: bool,
    relay_available: bool,
) -> Vec<TransportPath> {
    let reachability = PeerReachability {
        lan_addr: lan_addr.filter(|addr| !addr.trim().is_empty()),
        signaling_available,
        // Onion / I2P paths are wired into the desktop UI in a later phase; not
        // surfaced yet.
        onion_addr: None,
        i2p_addr: None,
        relay_available,
    };
    plan_connection(&reachability)
        .paths
        .iter()
        .map(|path| match path {
            ConnectionPath::LanTcp { addr } => TransportPath {
                kind: "lan".into(),
                label: "局域网直连".into(),
                detail: addr.clone(),
            },
            ConnectionPath::WebRtc => TransportPath {
                kind: "webrtc".into(),
                label: "打洞直连 (WebRTC)".into(),
                detail: String::new(),
            },
            ConnectionPath::Onion { addr } => TransportPath {
                kind: "onion".into(),
                label: "Tor 匿名连接 (.onion)".into(),
                detail: addr.clone(),
            },
            ConnectionPath::I2p { addr } => TransportPath {
                kind: "i2p".into(),
                label: "I2P 匿名连接 (.b32.i2p)".into(),
                detail: addr.clone(),
            },
            ConnectionPath::CloudRelay => TransportPath {
                kind: "relay".into(),
                label: "中继转发 (TURN)".into(),
                detail: String::new(),
            },
        })
        .collect()
}

#[cfg(feature = "webrtc")]
fn build_ice_config(
    ice_urls: Vec<String>,
    turn_username: Option<String>,
    turn_credential: Option<String>,
    relay_only: bool,
) -> linkhub_core::net::webrtc_transport::IceConfig {
    use linkhub_core::net::webrtc_transport::{IceConfig, IceServer};
    let username = turn_username.unwrap_or_default();
    let credential = turn_credential.unwrap_or_default();
    let servers = ice_urls
        .into_iter()
        .filter(|url| !url.trim().is_empty())
        .map(|url| {
            if url.starts_with("turn:") || url.starts_with("turns:") {
                IceServer::turn(url, username.clone(), credential.clone())
            } else {
                IceServer::stun(url)
            }
        })
        .collect();
    IceConfig {
        servers,
        force_relay: relay_only,
    }
}

#[cfg(not(feature = "webrtc"))]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_send_file(
    _signaling_url: String,
    _identity_path: String,
    _peer_device_id: String,
    _trust_store_path: String,
    _history_path: String,
    _file_path: String,
    _ice_urls: Vec<String>,
    _turn_username: Option<String>,
    _turn_credential: Option<String>,
    _relay_only: bool,
) -> Result<SendResult, String> {
    Err("此构建未启用跨网络 (WebRTC) 功能，请用 `--features webrtc` 重新构建".into())
}

#[cfg(feature = "webrtc")]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_send_file(
    signaling_url: String,
    identity_path: String,
    peer_device_id: String,
    trust_store_path: String,
    history_path: String,
    file_path: String,
    ice_urls: Vec<String>,
    turn_username: Option<String>,
    turn_credential: Option<String>,
    relay_only: bool,
) -> Result<SendResult, String> {
    let identity = load_identity(&identity_path)?;
    let store = TrustStore::load_from_path(&trust_store_path).map_err(|e| e.to_string())?;
    let peer = store
        .trusted_device(&peer_device_id)
        .ok_or_else(|| format!("peer device '{peer_device_id}' not in trust store"))?;
    let peer_identity = peer.identity().clone();
    let peer_name = peer.device_name().to_string();
    let ice = build_ice_config(ice_urls, turn_username, turn_credential, relay_only);
    let fname = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file_path)
        .to_string();

    // The transfer is blocking (runs its own tokio runtime + signaling thread);
    // keep it off the async/UI thread.
    let send = tauri::async_runtime::spawn_blocking(move || {
        linkhub_core::net::webrtc_session::send_file_over_webrtc(
            &signaling_url,
            &identity,
            &peer_identity,
            ice,
            &file_path,
        )
    })
    .await
    .map_err(|e| format!("webrtc send task failed: {e}"))?;

    if let Err(err) = send {
        let detail = format!("failed: {err}");
        append_send_history(
            &history_path,
            &peer_device_id,
            &peer_name,
            "file-webrtc",
            fname,
            &detail,
        );
        return Err(format!("跨网络发送失败: {err}"));
    }

    append_send_history(
        &history_path,
        &peer_device_id,
        &peer_name,
        "file-webrtc",
        fname,
        "success",
    );
    Ok(SendResult {
        success: true,
        message_id: String::new(),
        detail: "文件已通过跨网络 (WebRTC) 发送并确认".to_string(),
    })
}

#[cfg(not(feature = "webrtc"))]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_receive_file(
    _app: tauri::AppHandle,
    _signaling_url: String,
    _identity_path: String,
    _trust_store_path: String,
    _receive_dir: String,
    _ice_urls: Vec<String>,
    _turn_username: Option<String>,
    _turn_credential: Option<String>,
    _relay_only: bool,
) -> Result<SendResult, String> {
    Err("此构建未启用跨网络 (WebRTC) 功能，请用 `--features webrtc` 重新构建".into())
}

#[cfg(feature = "webrtc")]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_receive_file(
    app: tauri::AppHandle,
    signaling_url: String,
    identity_path: String,
    trust_store_path: String,
    receive_dir: String,
    ice_urls: Vec<String>,
    turn_username: Option<String>,
    turn_credential: Option<String>,
    relay_only: bool,
) -> Result<SendResult, String> {
    let identity = load_identity(&identity_path)?;
    let trust_store = std::sync::Arc::new(
        TrustStore::load_from_path(&trust_store_path)
            .map_err(|e| format!("failed to load trust store: {e}"))?,
    );
    let ice = build_ice_config(ice_urls, turn_username, turn_credential, relay_only);
    let on_accept = make_desktop_accept_callback(app, trust_store_path);
    let stop = Arc::new(AtomicBool::new(false));

    let received = tauri::async_runtime::spawn_blocking(move || {
        linkhub_core::net::webrtc_session::receive_file_over_webrtc_until_with_accept(
            &signaling_url,
            identity,
            trust_store,
            &receive_dir,
            ice,
            None,
            Some(on_accept),
            stop,
        )
    })
    .await
    .map_err(|e| format!("webrtc receive task failed: {e}"))?;

    received.map_err(|err| format!("跨网络接收失败: {err}"))?;
    Ok(SendResult {
        success: true,
        message_id: String::new(),
        detail: "已通过跨网络 (WebRTC) 接收文件".to_string(),
    })
}

fn webrtc_receiver_status_snapshot() -> WebRtcReceiverStatus {
    let state = webrtc_receiver_state();
    reap_webrtc_receiver_if_finished(state);
    WebRtcReceiverStatus {
        running: state.running.load(Ordering::Relaxed),
        stopping: state.stopping.load(Ordering::Relaxed),
        completed_sessions: *state.completed_sessions.lock().unwrap(),
        error: state.last_error.lock().unwrap().clone(),
    }
}

#[tauri::command]
fn webrtc_receiver_status() -> WebRtcReceiverStatus {
    webrtc_receiver_status_snapshot()
}

#[cfg(not(feature = "webrtc"))]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_start_receiver(
    _app: tauri::AppHandle,
    _signaling_url: String,
    _identity_path: String,
    _trust_store_path: String,
    _receive_dir: String,
    _ice_urls: Vec<String>,
    _turn_username: Option<String>,
    _turn_credential: Option<String>,
    _relay_only: bool,
) -> Result<WebRtcReceiverStatus, String> {
    Err("此构建未启用跨网络 (WebRTC) 功能，请用 `--features webrtc` 重新构建".into())
}

#[cfg(feature = "webrtc")]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn webrtc_start_receiver(
    app: tauri::AppHandle,
    signaling_url: String,
    identity_path: String,
    trust_store_path: String,
    receive_dir: String,
    ice_urls: Vec<String>,
    turn_username: Option<String>,
    turn_credential: Option<String>,
    relay_only: bool,
) -> Result<WebRtcReceiverStatus, String> {
    let state = webrtc_receiver_state();
    reap_webrtc_receiver_if_finished(state);
    if state.running.load(Ordering::Relaxed) {
        return Ok(webrtc_receiver_status_snapshot());
    }

    let identity = load_identity(&identity_path)?;
    let trust_store = Arc::new(
        TrustStore::load_from_path(&trust_store_path)
            .map_err(|e| format!("failed to load trust store: {e}"))?,
    );
    let ice = build_ice_config(ice_urls, turn_username, turn_credential, relay_only);
    let on_accept = make_desktop_accept_callback(app, trust_store_path);
    let stop = Arc::new(AtomicBool::new(false));

    state.running.store(true, Ordering::Relaxed);
    state.stopping.store(false, Ordering::Relaxed);
    *state.completed_sessions.lock().unwrap() = 0;
    *state.last_error.lock().unwrap() = String::new();
    *state.stop.lock().unwrap() = Some(Arc::clone(&stop));

    let handle = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let result =
                linkhub_core::net::webrtc_session::receive_file_over_webrtc_until_with_accept(
                    &signaling_url,
                    identity.clone(),
                    Arc::clone(&trust_store),
                    &receive_dir,
                    ice.clone(),
                    None,
                    Some(on_accept.clone()),
                    Arc::clone(&stop),
                );

            if stop.load(Ordering::Relaxed) {
                break;
            }

            match result {
                Ok(()) => {
                    let state = webrtc_receiver_state();
                    let mut completed = state.completed_sessions.lock().unwrap();
                    *completed += 1;
                    *state.last_error.lock().unwrap() = String::new();
                }
                Err(err) => {
                    let state = webrtc_receiver_state();
                    let message = err.to_string();
                    *state.last_error.lock().unwrap() = message.clone();
                    eprintln!("WebRTC receiver loop error: {message}");
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }

        let state = webrtc_receiver_state();
        state.running.store(false, Ordering::Relaxed);
        state.stopping.store(false, Ordering::Relaxed);
    });

    *state.handle.lock().unwrap() = Some(handle);
    Ok(webrtc_receiver_status_snapshot())
}

#[cfg(not(feature = "webrtc"))]
#[tauri::command]
fn webrtc_stop_receiver() -> Result<WebRtcReceiverStatus, String> {
    Err("此构建未启用跨网络 (WebRTC) 功能，请用 `--features webrtc` 重新构建".into())
}

#[cfg(feature = "webrtc")]
#[tauri::command]
fn webrtc_stop_receiver() -> Result<WebRtcReceiverStatus, String> {
    let state = webrtc_receiver_state();
    reap_webrtc_receiver_if_finished(state);
    if !state.running.load(Ordering::Relaxed) {
        return Ok(webrtc_receiver_status_snapshot());
    }

    state.stopping.store(true, Ordering::Relaxed);
    if let Some(stop) = state.stop.lock().unwrap().as_ref() {
        stop.store(true, Ordering::Relaxed);
    }
    Ok(webrtc_receiver_status_snapshot())
}

// ── History commands ───────────────────────────────────────────────

#[tauri::command]
fn get_history(history_path: String) -> Result<TransmissionHistory, String> {
    let path = resolved_history_path(&history_path);
    if !std::path::Path::new(path).exists() {
        return Ok(TransmissionHistory { entries: vec![] });
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_history(history_path: String) -> Result<(), String> {
    let path = resolved_history_path(&history_path);
    if std::path::Path::new(path).exists() {
        std::fs::write(path, "{\"entries\":[]}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Listener commands ──────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct ListenerStatus {
    running: bool,
    bind_addr: String,
    error: String,
}

#[derive(Clone, Serialize)]
struct MdnsAdvertiseStatus {
    running: bool,
    service_name: String,
}

#[derive(Clone, Serialize)]
struct NetworkHint {
    label: String,
    address: String,
}

fn default_ipv4_for(target: &str) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target).ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[tauri::command]
fn local_network_hints(port: u16) -> Vec<NetworkHint> {
    let mut hints = Vec::new();
    for (label, target) in [
        ("default route", "8.8.8.8:80"),
        ("cloudflare route", "1.1.1.1:80"),
    ] {
        if let Some(ip) = default_ipv4_for(target) {
            let address = format!("{ip}:{port}");
            if !hints
                .iter()
                .any(|hint: &NetworkHint| hint.address == address)
            {
                hints.push(NetworkHint {
                    label: label.to_string(),
                    address,
                });
            }
        }
    }
    hints
}

#[tauri::command]
fn start_listener(
    app: tauri::AppHandle,
    bind_addr: String,
    identity_path: String,
    trust_store_path: String,
    receive_dir: String,
) -> Result<ListenerStatus, String> {
    let state = listener_state();
    if state.running.load(Ordering::Relaxed) {
        return Err("Listener is already running".into());
    }

    let identity = load_identity(&identity_path)?;
    let trust_store = TrustStore::load_from_path(&trust_store_path)
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    let listener = TcpListener::bind(&bind_addr)
        .map_err(|e| format!("failed to bind listener on {bind_addr}: {e}"))?;

    *state.bind_addr.lock().unwrap() = bind_addr.clone();
    *state.last_error.lock().unwrap() = String::new();
    state.running.store(true, Ordering::Relaxed);

    let addr = bind_addr.clone();
    let receive_dir = receive_dir.clone();
    let on_accept = make_desktop_accept_callback(app, trust_store_path);

    let handle = std::thread::spawn(move || {
        if let Err(err) = linkhub_core::run_authenticated_listener_on_with_callbacks(
            listener,
            &addr,
            identity,
            trust_store,
            &receive_dir,
            || !listener_state().running.load(Ordering::Relaxed),
            None,
            Some(on_accept),
        ) {
            let err = err.to_string();
            *listener_state().last_error.lock().unwrap() = err.clone();
            eprintln!("Listener error: {err}");
        }
        listener_state().running.store(false, Ordering::Relaxed);
    });

    *state.handle.lock().unwrap() = Some(handle);

    Ok(ListenerStatus {
        running: true,
        bind_addr,
        error: String::new(),
    })
}

#[tauri::command]
fn start_mdns_advertise(identity_path: String, port: u16) -> Result<MdnsAdvertiseStatus, String> {
    let mut state = mdns_advertise_state().lock().unwrap();
    if let Some(handle) = state.as_ref() {
        return Ok(MdnsAdvertiseStatus {
            running: true,
            service_name: handle.registration.fullname().to_string(),
        });
    }

    let identity = load_identity(&identity_path)?;
    let advertisement = MdnsAdvertisement::from_local_identity(&identity, port);
    let runtime =
        MdnsRuntime::new().map_err(|e| format!("failed to start mDNS advertiser: {e}"))?;
    let registration = runtime
        .register(&advertisement)
        .map_err(|e| format!("failed to register mDNS service: {e}"))?;
    let service_name = registration.fullname().to_string();
    *state = Some(MdnsAdvertiseHandle {
        runtime,
        registration,
    });

    Ok(MdnsAdvertiseStatus {
        running: true,
        service_name,
    })
}

#[tauri::command]
fn stop_mdns_advertise() -> Result<MdnsAdvertiseStatus, String> {
    let handle = mdns_advertise_state().lock().unwrap().take();
    if let Some(handle) = handle {
        let _ = handle.runtime.unregister(&handle.registration);
        let _ = handle.runtime.shutdown();
    }
    Ok(MdnsAdvertiseStatus {
        running: false,
        service_name: String::new(),
    })
}

#[tauri::command]
fn mdns_advertise_status() -> MdnsAdvertiseStatus {
    let state = mdns_advertise_state().lock().unwrap();
    if let Some(handle) = state.as_ref() {
        return MdnsAdvertiseStatus {
            running: true,
            service_name: handle.registration.fullname().to_string(),
        };
    }
    MdnsAdvertiseStatus {
        running: false,
        service_name: String::new(),
    }
}

#[tauri::command]
fn stop_listener() -> Result<ListenerStatus, String> {
    let state = listener_state();
    if !state.running.load(Ordering::Relaxed) {
        return Err("Listener is not running".into());
    }

    state.running.store(false, Ordering::Relaxed);
    if let Some(handle) = state.handle.lock().unwrap().take() {
        let _ = handle.join();
    }

    Ok(ListenerStatus {
        running: false,
        bind_addr: String::new(),
        error: String::new(),
    })
}

#[tauri::command]
fn listener_status() -> ListenerStatus {
    let state = listener_state();
    ListenerStatus {
        running: state.running.load(Ordering::Relaxed),
        bind_addr: state.bind_addr.lock().unwrap().clone(),
        error: state.last_error.lock().unwrap().clone(),
    }
}

/// Bring the main window to the foreground, restoring it if hidden/minimized.
fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        // single-instance must be the first plugin so a second launch is
        // intercepted before any other state is touched.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "显示 LinkHub", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::with_id("linkhub-tray")
                .tooltip("LinkHub")
                .icon(
                    app.default_window_icon()
                        .expect("missing window icon")
                        .clone(),
                )
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => focus_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        focus_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            default_config,
            pairing_generate_qr,
            pairing_inspect,
            pairing_confirm,
            identity_init,
            identity_load,
            get_local_status,
            pending_incoming_peer,
            respond_incoming_peer,
            choose_file_path,
            choose_folder_path,
            send_encrypted_text,
            send_encrypted_file,
            send_file_to_discovered,
            connection_plan,
            webrtc_send_file,
            webrtc_receive_file,
            webrtc_start_receiver,
            webrtc_stop_receiver,
            webrtc_receiver_status,
            get_history,
            clear_history,
            start_listener,
            stop_listener,
            listener_status,
            local_network_hints,
            scan_mdns,
            scan_trusted_mdns,
            start_mdns_advertise,
            stop_mdns_advertise,
            mdns_advertise_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LinkHub");
}

// ── Smoke tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::Instant;

    fn temp_dir() -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!(
            "linkhub-smoke-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_identity(tmp: &std::path::Path, name: &str) -> String {
        let path = tmp.join(format!("{}.txt", name.replace(' ', "-")));
        let path_str = path.display().to_string();
        let result = identity_init(path_str.clone(), name.to_string()).unwrap();
        assert!(!result.local_device_id.is_empty());
        path_str
    }

    #[test]
    fn smoke_identity_init_and_load() {
        let tmp = temp_dir();
        let ip = init_identity(&tmp, "Smoke Tester");
        let loaded = identity_load(ip.clone()).unwrap();
        assert_eq!(loaded.local_device_name, "Smoke Tester");
        assert!(loaded.local_device_id.starts_with("lh-"));
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_get_local_status() {
        let tmp = temp_dir();
        let ip = init_identity(&tmp, "Status PC");
        let ts_path = tmp.join("trust.txt").display().to_string();
        let status = get_local_status(ip, ts_path).unwrap();
        assert_eq!(status.local_device_name, "Status PC");
        assert!(status.trusted_devices.is_empty());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_pairing_flow() {
        let tmp = temp_dir();
        let ip = init_identity(&tmp, "Pairer PC");
        let ts_path = tmp.join("trust.txt").display().to_string();

        // Generate QR payload
        let qr = pairing_generate_qr(ip.clone(), 120).unwrap();
        assert!(qr.payload.starts_with("linkhub-pair-v2|"));
        assert!(!qr.qr_svg.is_empty());

        // Inspect our own payload (as if peer scanned it)
        let info = pairing_inspect(ip.clone(), qr.payload.clone()).unwrap();
        assert_eq!(info.device_name, "Pairer PC");
        assert!(!info.confirmation_code.is_empty());

        // Confirm pairing (trust ourselves — for testing)
        let trusted = pairing_confirm(
            ip.clone(),
            qr.payload,
            info.confirmation_code,
            ts_path.clone(),
        )
        .unwrap();
        assert_eq!(trusted.device_name, "Pairer PC");

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_listener_status() {
        let status = listener_status();
        assert!(!status.running);
        assert!(!status.bind_addr.is_empty());
        assert!(status.error.is_empty());
    }

    #[test]
    fn smoke_mdns_advertise_status_starts_stopped() {
        let status = mdns_advertise_status();
        assert!(!status.running);
        assert!(status.service_name.is_empty());
    }

    #[test]
    fn smoke_history_read_empty() {
        let tmp = temp_dir();
        let hp = tmp.join("history.json").display().to_string();
        let history = get_history(hp.clone()).unwrap();
        assert!(history.entries.is_empty());

        // Clear on empty
        clear_history(hp).unwrap();
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_history_write_and_read() {
        let tmp = temp_dir();
        let hp = tmp.join("history.json").display().to_string();

        // Write a history file directly
        let entry = serde_json::json!({"entries": [{
            "timestamp": "12345", "direction": "sent",
            "peer_device_id": "peer-1", "peer_device_name": "Test Peer",
            "kind": "text", "content_preview": "hello", "status": "success"
        }]});
        fs::write(&hp, serde_json::to_string_pretty(&entry).unwrap()).unwrap();

        let history = get_history(hp.clone()).unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].kind, "text");

        clear_history(hp.clone()).unwrap();
        let empty = get_history(hp).unwrap();
        assert!(empty.entries.is_empty());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_append_history_uses_requested_path() {
        let tmp = temp_dir();
        let hp = tmp.join("custom-history.json").display().to_string();

        append_history(
            &hp,
            HistoryEntry {
                timestamp: "12345".into(),
                direction: "sent".into(),
                peer_device_id: "peer-1".into(),
                peer_device_name: "Test Peer".into(),
                kind: "text".into(),
                content_preview: "hello".into(),
                status: "success".into(),
            },
        )
        .unwrap();

        let history = get_history(hp.clone()).unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].content_preview, "hello");
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_append_send_history_records_failed_status() {
        let tmp = temp_dir();
        let hp = tmp.join("history.json").display().to_string();

        append_send_history(
            &hp,
            "peer-1",
            "Test Peer",
            "text",
            "hello".to_string(),
            "failed: connection refused",
        );

        let history = get_history(hp).unwrap();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].status, "failed: connection refused");
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn smoke_connection_plan_orders_lan_webrtc_relay() {
        let plan = connection_plan(Some("192.168.1.9:8787".into()), true, true);
        let kinds: Vec<String> = plan.iter().map(|p| p.kind.clone()).collect();
        assert_eq!(kinds, vec!["lan", "webrtc", "relay"]);
        assert_eq!(plan[0].detail, "192.168.1.9:8787");
        assert!(plan.iter().all(|p| !p.label.is_empty()));
    }

    #[test]
    fn smoke_connection_plan_skips_unavailable_paths() {
        // No LAN address, no relay: only the WebRTC hole-punch path remains.
        let plan = connection_plan(None, true, false);
        let kinds: Vec<String> = plan.iter().map(|p| p.kind.clone()).collect();
        assert_eq!(kinds, vec!["webrtc"]);

        // Blank LAN address is treated as absent.
        let plan_blank = connection_plan(Some("   ".into()), false, false);
        assert!(plan_blank.is_empty());
    }

    #[test]
    fn smoke_webrtc_receiver_status_starts_stopped() {
        let status = webrtc_receiver_status();
        assert!(!status.running);
        assert!(!status.stopping);
        assert_eq!(status.completed_sessions, 0);
    }

    #[test]
    fn smoke_trusted_mdns_results_filter_untrusted_devices() {
        let mut store = TrustStore::default();
        store.trust(linkhub_core::TrustedDevice::new(
            linkhub_core::DeviceIdentity::new(
                "trusted-001",
                "Trusted Phone",
                "trusted-public-key",
                "00".repeat(32),
            ),
            SystemTime::now(),
        ));

        let now = Instant::now();
        let peers = trusted_discovered_peers(
            &store,
            vec![
                DiscoveryEndpoint::lan_tcp(
                    "trusted-001",
                    "Trusted Phone",
                    ([192, 168, 1, 20], 8787).into(),
                    now,
                ),
                DiscoveryEndpoint::lan_tcp(
                    "unknown-001",
                    "Unknown Phone",
                    ([192, 168, 1, 30], 8787).into(),
                    now,
                ),
            ],
        );

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, "trusted-001");
        assert_eq!(peers[0].address, "192.168.1.20:8787");
    }

    #[test]
    fn smoke_discovered_peers_include_signed_first_contact_devices() {
        let identity = LocalIdentity::generate("Nearby PC", SystemTime::now());
        let now = Instant::now();
        let endpoint = MdnsAdvertisement::from_local_identity(&identity, 8787)
            .to_endpoint([192, 168, 1, 40].into(), now);

        let peers = discovered_peers(&TrustStore::default(), vec![endpoint]);

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, identity.device_id());
        assert_eq!(peers[0].device_name, "Nearby PC");
        assert_eq!(peers[0].address, "192.168.1.40:8787");
        assert!(!peers[0].trusted);
        assert!(!peers[0].binding_sig.is_empty());
    }
}
