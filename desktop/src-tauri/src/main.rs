// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use linkhub_core::{
    DiscoveryEndpoint, LocalIdentity, MdnsAdvertisement, MdnsRegistration, MdnsRuntime,
    PairingInvitation, PairingSession, TrustStore,
};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::net::{TcpListener, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
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

struct MdnsAdvertiseHandle {
    runtime: MdnsRuntime,
    registration: MdnsRegistration,
}

fn mdns_advertise_state() -> &'static Mutex<Option<MdnsAdvertiseHandle>> {
    static STATE: OnceLock<Mutex<Option<MdnsAdvertiseHandle>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
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
        let raw = std::fs::read_to_string(path).map_err(|e| format!("{e}"))?;
        history = serde_json::from_str(&raw).unwrap_or(TransmissionHistory { entries: vec![] });
    }
    history.entries.push(entry);
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
        }
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&history).map_err(|e| format!("{e}"))?,
    )
    .map_err(|e| format!("{e}"))?;
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
        PairingInvitation::from_payload(&payload, SystemTime::now()).map_err(|e| format!("{e}"))?;
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
        PairingInvitation::from_payload(&payload, SystemTime::now()).map_err(|e| format!("{e}"))?;
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
    let mut peers = endpoints
        .into_iter()
        .filter_map(|endpoint| {
            store.trusted_device(endpoint.device_id())?;
            Some(DiscoveredPeer {
                device_id: endpoint.device_id().to_string(),
                device_name: endpoint.device_name().to_string(),
                address: endpoint.addr().to_string(),
            })
        })
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
    let store = TrustStore::load_from_path(&trust_store_path).map_err(|e| format!("{e}"))?;
    let peer = store
        .trusted_device(&peer_device_id)
        .ok_or_else(|| format!("peer device '{peer_device_id}' not in trust store"))?;
    let dh_hex = peer.identity().dh_public_key();
    let dh_bytes = linkhub_core::decode_hex(dh_hex).map_err(|e| format!("{e}"))?;
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
    let store = TrustStore::load_from_path(&trust_store_path).map_err(|e| format!("{e}"))?;
    let peer = store
        .trusted_device(&peer_device_id)
        .ok_or_else(|| format!("peer device '{peer_device_id}' not in trust store"))?;
    let dh_hex = peer.identity().dh_public_key();
    let dh_bytes = linkhub_core::decode_hex(dh_hex).map_err(|e| format!("{e}"))?;
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

// ── History commands ───────────────────────────────────────────────

#[tauri::command]
fn get_history(history_path: String) -> Result<TransmissionHistory, String> {
    let path = resolved_history_path(&history_path);
    if !std::path::Path::new(path).exists() {
        return Ok(TransmissionHistory { entries: vec![] });
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("{e}"))
}

#[tauri::command]
fn clear_history(history_path: String) -> Result<(), String> {
    let path = resolved_history_path(&history_path);
    if std::path::Path::new(path).exists() {
        std::fs::write(path, "{\"entries\":[]}").map_err(|e| format!("{e}"))?;
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

    let handle = std::thread::spawn(move || {
        if let Err(err) = linkhub_core::run_authenticated_listener_on(
            listener,
            &addr,
            identity,
            trust_store,
            &receive_dir,
            || !listener_state().running.load(Ordering::Relaxed),
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
    let advertisement = MdnsAdvertisement::from_identity(identity.identity(), port);
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
                        focus_main_window(&tray.app_handle());
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
            choose_file_path,
            choose_folder_path,
            send_encrypted_text,
            send_encrypted_file,
            get_history,
            clear_history,
            start_listener,
            stop_listener,
            listener_status,
            local_network_hints,
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
}
