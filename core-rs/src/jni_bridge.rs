//! JNI bridge for Android — exports C-compatible functions that
//! Kotlin calls via `System.loadLibrary("linkhub_core")`.
//!
//! All functions use JSON strings for params and return values to
//! keep the JNI surface simple and avoid complex type marshalling.

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::jstring;
use jni::{JNIEnv, JavaVM};

use crate::{
    new_pairing_nonce, FileReceivedCallback, LocalIdentity, PairingInvitation, PairingSession,
    ReceivedFileEvent, TrustStore,
};
use serde::{Deserialize, Serialize};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

// ── JSON interchange types ─────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct JniIdentity {
    device_id: String,
    device_name: String,
    fingerprint: String,
    public_key: String,
    dh_public_key: String,
    signing_key_hex: String,
    static_dh_key_hex: String,
    created_at_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct JniPeerInfo {
    device_id: String,
    device_name: String,
    fingerprint: String,
    confirmation_code: String,
}

#[derive(Serialize, Deserialize)]
struct JniPairResult {
    device_id: String,
    device_name: String,
    fingerprint: String,
    success: bool,
    error: String,
}

#[derive(Serialize, Deserialize)]
struct JniSendResult {
    success: bool,
    detail: String,
}

// ── Helpers ────────────────────────────────────────────────────────

fn get_string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(|s| s.into()).unwrap_or_default()
}

fn make_string(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s).unwrap().into_raw()
}

fn ok_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn err_json(msg: &str) -> String {
    format!(r#"{{"error":"{}"}}"#, msg.replace('"', "'"))
}

fn from_local_identity(id: &LocalIdentity) -> JniIdentity {
    JniIdentity {
        device_id: id.device_id().to_string(),
        device_name: id.device_name().to_string(),
        fingerprint: id.identity().fingerprint(),
        public_key: id.public_key().to_string(),
        dh_public_key: id.dh_public_key().to_string(),
        signing_key_hex: id.signing_key_hex().to_string(),
        static_dh_key_hex: id.static_dh_key_hex().to_string(),
        created_at_secs: id
            .created_at()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    }
}

fn to_local_identity(j: &JniIdentity) -> Result<LocalIdentity, String> {
    let sk: [u8; 32] = hex_to_array(&j.signing_key_hex, "signing_key")?;
    let dh: [u8; 32] = hex_to_array(&j.static_dh_key_hex, "static_dh_key")?;
    let created_at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(j.created_at_secs);
    Ok(LocalIdentity::from_keys(&j.device_name, sk, dh, created_at))
}

fn hex_to_array<const N: usize>(hex: &str, label: &str) -> Result<[u8; N], String> {
    let bytes = crate::decode_hex(hex).map_err(|e| format!("{label}: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{label} must be {N} bytes"))
}

// ── Identity ───────────────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_generateIdentity(
    mut env: JNIEnv,
    _class: JClass,
    device_name: JString,
) -> jstring {
    let name = get_string(&mut env, &device_name);
    let identity = LocalIdentity::generate(&name, SystemTime::now());
    make_string(&mut env, &ok_json(&from_local_identity(&identity)))
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_restoreIdentity(
    mut env: JNIEnv,
    _class: JClass,
    signing_key_hex: JString,
    static_dh_key_hex: JString,
    device_name: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let sk = get_string(&mut env, &signing_key_hex);
        let dh = get_string(&mut env, &static_dh_key_hex);
        let name = get_string(&mut env, &device_name);
        let sk_arr: [u8; 32] = hex_to_array(&sk, "signing_key")?;
        let dh_arr: [u8; 32] = hex_to_array(&dh, "static_dh_key")?;
        let identity = LocalIdentity::from_keys(&name, sk_arr, dh_arr, SystemTime::now());
        Ok(ok_json(&from_local_identity(&identity)))
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

// ── Pairing ────────────────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_generatePairingPayload(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    ttl_seconds: jni::sys::jlong,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let json = get_string(&mut env, &identity_json);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let invitation = PairingInvitation::new(
            local.identity().clone(),
            new_pairing_nonce(),
            Instant::now(),
            Duration::from_secs(ttl_seconds as u64),
        );
        Ok(invitation.to_payload())
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_parsePairingPayload(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    payload: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let json = get_string(&mut env, &identity_json);
        let payload = get_string(&mut env, &payload);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let invitation = PairingInvitation::from_payload(&payload, Instant::now())
            .map_err(|e| format!("{e}"))?;
        let session = PairingSession::new(local.identity().clone(), invitation);
        Ok(ok_json(&JniPeerInfo {
            device_id: session.peer_identity().device_id().to_string(),
            device_name: session.peer_identity().device_name().to_string(),
            fingerprint: session.peer_identity().fingerprint(),
            confirmation_code: session.confirmation_code(),
        }))
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_confirmPairing(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    payload: JString,
    confirmation_code: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let json = get_string(&mut env, &identity_json);
        let payload = get_string(&mut env, &payload);
        let code = get_string(&mut env, &confirmation_code);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let invitation = PairingInvitation::from_payload(&payload, Instant::now())
            .map_err(|e| format!("{e}"))?;
        let session = PairingSession::new(local.identity().clone(), invitation);
        let trusted = session
            .confirm(&code, Instant::now(), SystemTime::now())
            .map_err(|e| format!("{e}"))?;
        Ok(ok_json(&JniPairResult {
            device_id: trusted.device_id().to_string(),
            device_name: trusted.device_name().to_string(),
            fingerprint: trusted.fingerprint().to_string(),
            success: true,
            error: String::new(),
        }))
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

// ── Send ───────────────────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_sendText(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    peer_addr: JString,
    peer_device_id: JString,
    peer_dh_hex: JString,
    text: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let json = get_string(&mut env, &identity_json);
        let addr = get_string(&mut env, &peer_addr);
        let peer_id = get_string(&mut env, &peer_device_id);
        let dh_hex = get_string(&mut env, &peer_dh_hex);
        let text = get_string(&mut env, &text);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let dh_bytes: [u8; 32] = hex_to_array(&dh_hex, "dh_key")?;
        crate::run_authenticated_text_sender(&addr, local, &peer_id, &dh_bytes, &text)
            .map_err(|e| format!("send failed: {e}"))?;
        Ok(ok_json(&JniSendResult {
            success: true,
            detail: "text sent".into(),
        }))
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_sendFile(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    peer_addr: JString,
    peer_device_id: JString,
    peer_dh_hex: JString,
    file_path: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        let json = get_string(&mut env, &identity_json);
        let addr = get_string(&mut env, &peer_addr);
        let peer_id = get_string(&mut env, &peer_device_id);
        let dh_hex = get_string(&mut env, &peer_dh_hex);
        let path = get_string(&mut env, &file_path);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let dh_bytes: [u8; 32] = hex_to_array(&dh_hex, "dh_key")?;
        crate::run_authenticated_file_sender(&addr, local, &peer_id, &dh_bytes, &path)
            .map_err(|e| format!("send failed: {e}"))?;
        Ok(ok_json(&JniSendResult {
            success: true,
            detail: "file sent".into(),
        }))
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

// ── Listener ───────────────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
static LISTENER_RUNNING: AtomicBool = AtomicBool::new(false);
static LISTENER_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize)]
struct JniListenerStatus {
    running: bool,
    detail: String,
    error: String,
}

fn set_listener_last_error(error: Option<String>) {
    if let Ok(mut last_error) = LISTENER_LAST_ERROR.lock() {
        *last_error = error;
    }
}

fn listener_last_error() -> String {
    LISTENER_LAST_ERROR
        .lock()
        .ok()
        .and_then(|last_error| last_error.clone())
        .unwrap_or_default()
}

/// Builds a callback that forwards "file received" events to the static Kotlin
/// method `RustBridge.onFileReceived`. The JVM handle and a global ref to the
/// `RustBridge` class are captured on the JNI calling thread (where the app
/// class loader is reachable) so the worker thread can attach and call back
/// without hitting Android's `FindClass`-from-native-thread limitation.
fn make_file_received_callback(vm: JavaVM, class_ref: GlobalRef) -> FileReceivedCallback {
    Arc::new(move |event: ReceivedFileEvent| {
        let mut guard = match vm.attach_current_thread() {
            Ok(guard) => guard,
            Err(err) => {
                eprintln!("onFileReceived: failed to attach JVM thread: {err}");
                return;
            }
        };
        let env = &mut *guard;
        let class = unsafe { JClass::from_raw(class_ref.as_raw()) };

        let call = (|| -> Result<(), jni::errors::Error> {
            let peer_id: JObject = env.new_string(&event.peer_device_id)?.into();
            let peer_name: JObject = env.new_string(&event.peer_device_name)?.into();
            let filename: JObject = env.new_string(&event.filename)?.into();
            let final_path: JObject = env.new_string(&event.final_path)?.into();
            env.call_static_method(
                &class,
                "onFileReceived",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;J)V",
                &[
                    JValue::Object(&peer_id),
                    JValue::Object(&peer_name),
                    JValue::Object(&filename),
                    JValue::Object(&final_path),
                    JValue::Long(event.size_bytes as i64),
                ],
            )?;
            Ok(())
        })();

        if let Err(err) = call {
            eprintln!("onFileReceived: JNI call failed: {err}");
        }
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_startListener(
    mut env: JNIEnv,
    _class: JClass,
    identity_json: JString,
    bind_addr: JString,
    trust_store_path: JString,
    receive_dir: JString,
) -> jstring {
    let result = (|| -> Result<String, String> {
        if LISTENER_RUNNING.load(Ordering::Relaxed) {
            return Ok(r#"{"running":true,"detail":"listener already running"}"#.into());
        }
        let json = get_string(&mut env, &identity_json);
        let addr = get_string(&mut env, &bind_addr);
        let ts_path = get_string(&mut env, &trust_store_path);
        let dir = get_string(&mut env, &receive_dir);
        let jni: JniIdentity = serde_json::from_str(&json).map_err(|e| format!("{e}"))?;
        let local = to_local_identity(&jni)?;
        let trust_store = TrustStore::load_from_path(&ts_path).map_err(|e| format!("{e}"))?;
        let listener = TcpListener::bind(&addr)
            .map_err(|e| format!("failed to bind listener on {addr}: {e}"))?;

        // Capture the JVM and RustBridge class on this (JNI) thread so the
        // listener's worker threads can notify Kotlin when files arrive.
        let vm = env
            .get_java_vm()
            .map_err(|e| format!("failed to get JVM handle: {e}"))?;
        let bridge_class = env
            .find_class("com/linkhub/app/bridge/RustBridge")
            .map_err(|e| format!("failed to find RustBridge class: {e}"))?;
        let class_ref = env
            .new_global_ref(bridge_class)
            .map_err(|e| format!("failed to pin RustBridge class: {e}"))?;
        let on_file_received = make_file_received_callback(vm, class_ref);

        set_listener_last_error(None);
        LISTENER_RUNNING.store(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            let result = crate::run_authenticated_listener_on_with_callback(
                listener,
                &addr,
                local,
                trust_store,
                &dir,
                || !LISTENER_RUNNING.load(Ordering::Relaxed),
                Some(on_file_received),
            );
            if let Err(err) = result {
                set_listener_last_error(Some(format!("{err}")));
            }
            LISTENER_RUNNING.store(false, Ordering::Relaxed);
        });
        Ok(r#"{"running":true,"detail":"listener started"}"#.into())
    })();
    match result {
        Ok(s) => make_string(&mut env, &s),
        Err(e) => make_string(&mut env, &err_json(&e)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_stopListener(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    LISTENER_RUNNING.store(false, Ordering::Relaxed);
    make_string(&mut env, r#"{"running":false,"detail":"listener stopped"}"#)
}

#[no_mangle]
pub extern "system" fn Java_com_linkhub_app_bridge_RustBridge_listenerStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let running = LISTENER_RUNNING.load(Ordering::Relaxed);
    let error = listener_last_error();
    let detail = if running {
        "listener running"
    } else if error.is_empty() {
        "listener stopped"
    } else {
        "listener failed"
    };
    make_string(
        &mut env,
        &ok_json(&JniListenerStatus {
            running,
            detail: detail.to_string(),
            error,
        }),
    )
}
