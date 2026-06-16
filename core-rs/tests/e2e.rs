//! In-process end-to-end transfer tests over real TCP loopback.
//!
//! These mirror the cross-process coverage of `scripts/verify-local-e2e.ps1`
//! (plain text/file, authenticated text/file, and resume from a pre-seeded
//! partial) but drive the public crate API directly so they run under
//! `cargo test`.
//!
//! Key invariant exploited for synchronization: every sender returns `Ok` only
//! after the receiver's final ACK, and the receiver finishes writing/renaming
//! the file (and fires its callback) *before* sending that ACK. So once a
//! sender call returns `Ok`, the received artifacts are already on disk.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

use linkhub_core::{
    decode_hex, run_authenticated_file_sender, run_authenticated_listener_on_with_callback,
    run_authenticated_text_sender, run_file_sender, run_listener_with_receive_dir,
    FileReceivedCallback, LocalDevice, LocalIdentity, ReceivedFileEvent, TrustStore, TrustedDevice,
};
use sha2::{Digest, Sha256};

const FILE_CHUNK_SIZE: usize = 4096;

// ── helpers ──────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("linkhub-e2e-{tag}-{nanos}-{:?}", thread::current().id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 17) % 251) as u8).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn file_sha256_hex(path: impl AsRef<Path>) -> String {
    sha256_hex(&fs::read(path).unwrap())
}

/// Replicates `net::file_transfer::file_transfer_id` for clean (already-safe)
/// device ids and filenames, so tests can predict on-disk paths.
fn transfer_id(device_id: &str, filename: &str, size: u64, sha256_hex: &str) -> String {
    format!("{device_id}-{filename}-{size}-{}", &sha256_hex[..16])
}

/// Replicates `net::file_transfer::received_file_path` / `partial_file_path` /
/// `receive_metadata_path`.
fn received_paths(receive_dir: &Path, transfer_id: &str, filename: &str) -> (PathBuf, PathBuf, PathBuf) {
    let final_path = receive_dir.join(format!("{transfer_id}_{filename}"));
    let part_path = receive_dir.join(format!("{transfer_id}_{filename}.part"));
    let meta_path = receive_dir.join(format!("{transfer_id}_{filename}.part.meta"));
    (final_path, part_path, meta_path)
}

fn dh_public_key_bytes(identity: &LocalIdentity) -> [u8; 32] {
    decode_hex(identity.dh_public_key())
        .unwrap()
        .try_into()
        .unwrap()
}

struct AuthListener {
    addr: String,
    stop: Arc<AtomicBool>,
    received: Arc<Mutex<Vec<ReceivedFileEvent>>>,
    handle: Option<JoinHandle<()>>,
}

impl AuthListener {
    /// Binds a loopback listener that trusts `sender_identity` and runs the
    /// authenticated receive loop on a background thread.
    fn start(receiver_identity: LocalIdentity, sender_identity: &LocalIdentity, receive_dir: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        let mut trust = TrustStore::new();
        trust.trust(TrustedDevice::new(
            sender_identity.identity().clone(),
            SystemTime::now(),
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let received = Arc::new(Mutex::new(Vec::new()));

        let stop_for_loop = Arc::clone(&stop);
        let received_for_cb = Arc::clone(&received);
        let callback: FileReceivedCallback = Arc::new(move |event: ReceivedFileEvent| {
            received_for_cb.lock().unwrap().push(event);
        });
        let bind_label = addr.clone();

        let handle = thread::spawn(move || {
            run_authenticated_listener_on_with_callback(
                listener,
                &bind_label,
                receiver_identity,
                trust,
                receive_dir,
                move || stop_for_loop.load(Ordering::Relaxed),
                Some(callback),
            )
            .unwrap();
        });

        Self {
            addr,
            stop,
            received,
            handle: Some(handle),
        }
    }

    fn received_events(&self) -> Vec<ReceivedFileEvent> {
        self.received.lock().unwrap().clone()
    }
}

impl Drop for AuthListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// ── tests ────────────────────────────────────────────────────────────

#[test]
fn authenticated_text_round_trips() {
    let receive_dir = unique_dir("auth-text");
    let sender = LocalIdentity::generate("Sender PC", SystemTime::now());
    let receiver = LocalIdentity::generate("Receiver PC", SystemTime::now());
    let receiver_dh = dh_public_key_bytes(&receiver);

    let listener = AuthListener::start(receiver.clone(), &sender, receive_dir.clone());

    // Returns Ok only after the receiver acknowledges the encrypted TEXT frame.
    run_authenticated_text_sender(
        &listener.addr,
        sender,
        receiver.device_id(),
        &receiver_dh,
        "authenticated in-process text",
    )
    .unwrap();

    let _ = fs::remove_dir_all(&receive_dir);
}

#[test]
fn authenticated_file_round_trips_with_matching_hash() {
    let receive_dir = unique_dir("auth-file");
    let send_dir = unique_dir("auth-file-send");
    let sender = LocalIdentity::generate("Sender PC", SystemTime::now());
    let receiver = LocalIdentity::generate("Receiver PC", SystemTime::now());
    let receiver_dh = dh_public_key_bytes(&receiver);

    let payload = deterministic_bytes(10_000);
    let source = send_dir.join("auth-sample.bin");
    fs::write(&source, &payload).unwrap();
    let expected_hash = sha256_hex(&payload);

    let listener = AuthListener::start(receiver.clone(), &sender, receive_dir.clone());

    run_authenticated_file_sender(
        &listener.addr,
        sender.clone(),
        receiver.device_id(),
        &receiver_dh,
        &source,
    )
    .unwrap();

    let events = listener.received_events();
    assert_eq!(events.len(), 1, "expected exactly one received-file callback");
    let event = &events[0];
    assert_eq!(event.peer_device_id, sender.device_id());
    assert_eq!(event.filename, "auth-sample.bin");
    assert_eq!(event.size_bytes, payload.len() as u64);

    let final_path = Path::new(&event.final_path);
    assert!(final_path.exists(), "received file should exist on disk");
    assert_eq!(file_sha256_hex(final_path), expected_hash);

    let _ = fs::remove_dir_all(&receive_dir);
    let _ = fs::remove_dir_all(&send_dir);
}

#[test]
fn authenticated_file_resumes_from_pre_seeded_partial() {
    let receive_dir = unique_dir("auth-resume");
    let send_dir = unique_dir("auth-resume-send");
    let sender = LocalIdentity::generate("Sender PC", SystemTime::now());
    let receiver = LocalIdentity::generate("Receiver PC", SystemTime::now());
    let receiver_dh = dh_public_key_bytes(&receiver);

    let size = 8292usize;
    let payload = deterministic_bytes(size);
    let filename = "auth-resume-sample.bin";
    let source = send_dir.join(filename);
    fs::write(&source, &payload).unwrap();
    let full_hash = sha256_hex(&payload);

    // Pre-seed the receiver with the first chunk already received.
    let tid = transfer_id(sender.device_id(), filename, size as u64, &full_hash);
    let (final_path, part_path, meta_path) = received_paths(&receive_dir, &tid, filename);
    let first_chunk = &payload[..FILE_CHUNK_SIZE];
    fs::write(&part_path, first_chunk).unwrap();
    let partial_hash = sha256_hex(first_chunk);
    let meta = [
        format!("transfer_id={tid}"),
        format!("filename={filename}"),
        format!("size_bytes={size}"),
        format!("expected_sha256_hex={full_hash}"),
        format!("received_bytes={FILE_CHUNK_SIZE}"),
        "next_chunk_index=1".to_string(),
        format!("partial_sha256_hex={partial_hash}"),
        format!("temp_path={}", part_path.display()),
        format!("final_path={}", final_path.display()),
    ]
    .join("\n");
    fs::write(&meta_path, meta).unwrap();

    let listener = AuthListener::start(receiver.clone(), &sender, receive_dir.clone());

    run_authenticated_file_sender(
        &listener.addr,
        sender.clone(),
        receiver.device_id(),
        &receiver_dh,
        &source,
    )
    .unwrap();

    let events = listener.received_events();
    assert_eq!(events.len(), 1);
    assert_eq!(Path::new(&events[0].final_path), final_path);
    assert!(final_path.exists());
    assert_eq!(file_sha256_hex(&final_path), full_hash);
    assert!(!part_path.exists(), "partial file should be cleaned up after resume");
    assert!(!meta_path.exists(), "resume metadata should be cleaned up after resume");

    let _ = fs::remove_dir_all(&receive_dir);
    let _ = fs::remove_dir_all(&send_dir);
}

#[test]
fn plain_file_round_trips_with_matching_hash() {
    let receive_dir = unique_dir("plain-file");
    let send_dir = unique_dir("plain-file-send");

    // Probe a free loopback port, then hand the address to the plain listener.
    let addr = {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().to_string()
    };

    let device_id = "sender-001";
    let payload = deterministic_bytes(10_000);
    let filename = "plain-sample.bin";
    let source = send_dir.join(filename);
    fs::write(&source, &payload).unwrap();
    let expected_hash = sha256_hex(&payload);

    let listener_dir = receive_dir.clone();
    let listener_addr = addr.clone();
    // The plain listener loops forever; let the detached thread die with the
    // test process once the transfer is acknowledged.
    thread::spawn(move || {
        let _ = run_listener_with_receive_dir(
            &listener_addr,
            LocalDevice::new("receiver-001", "Receiver PC"),
            listener_dir,
        );
    });

    // Returns Ok only after FILE_END is acknowledged; by then the receiver has
    // already renamed the verified file into place.
    run_file_sender(&addr, LocalDevice::new(device_id, "Sender PC"), &source).unwrap();

    let tid = transfer_id(device_id, filename, payload.len() as u64, &expected_hash);
    let (final_path, _, _) = received_paths(&receive_dir, &tid, filename);
    assert!(final_path.exists(), "received file should exist on disk: {}", final_path.display());
    assert_eq!(file_sha256_hex(&final_path), expected_hash);

    let _ = fs::remove_dir_all(&receive_dir);
    let _ = fs::remove_dir_all(&send_dir);
}
