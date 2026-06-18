//! M3 acceptance (feature `webrtc`): run the existing Noise KK authenticated
//! session over a **real webrtc-rs DataChannel** and transfer a file, asserting
//! the receiver's SHA-256 matches the source.
//!
//! Two in-process PeerConnections are wired through direct channels (standing in
//! for the signaling server — the SDP exchange is the same shape the
//! `SignalingClient` carries). The point under test is the async-DataChannel →
//! sync-`Read`/`Write` bridge plus the unchanged Noise/file layer running on top.
//!
//! The whole file is compiled out unless `--features webrtc` is set.
#![cfg(feature = "webrtc")]

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use linkhub_core::net::webrtc_transport::{
    accept_responder, connect_initiator, IceConfig, SdpSignal,
};
use linkhub_core::{
    decode_hex, run_authenticated_file_sender_over, run_authenticated_responder_over,
    FileReceivedCallback, LocalIdentity, ReceivedFileEvent, TrustStore, TrustedDevice,
};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::unbounded_channel;

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("linkhub-webrtc-{tag}-{nanos}"));
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
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn dh_bytes(identity: &LocalIdentity) -> [u8; 32] {
    decode_hex(identity.dh_public_key())
        .unwrap()
        .try_into()
        .unwrap()
}

#[test]
fn noise_file_transfer_over_webrtc_datachannel() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();

    // Direct signaling channels in both directions (loopback stand-in).
    let (i2r_tx, i2r_rx) = unbounded_channel::<SdpSignal>();
    let (r2i_tx, r2i_rx) = unbounded_channel::<SdpSignal>();

    // Establish both ends concurrently (no STUN needed on loopback).
    let init_fut = connect_initiator(IceConfig::default(), i2r_tx, r2i_rx, handle.clone());
    let resp_fut = accept_responder(IceConfig::default(), r2i_tx, i2r_rx, handle.clone());
    let (init_duplex, resp_duplex) = rt.block_on(async move { tokio::join!(init_fut, resp_fut) });
    let init_duplex = init_duplex.expect("initiator establishes DataChannel");
    let resp_duplex = resp_duplex.expect("responder establishes DataChannel");

    // Identities + trust (responder trusts the sender it authenticates).
    let now = SystemTime::now();
    let sender = LocalIdentity::generate("Sender", now);
    let receiver = LocalIdentity::generate("Receiver", now);
    let receiver_device_id = receiver.device_id().to_string();
    let receiver_dh = dh_bytes(&receiver);

    let mut trust = TrustStore::new();
    trust.trust(TrustedDevice::new(sender.identity().clone(), now));
    let trust = Arc::new(trust);

    let receive_dir = unique_dir("recv");
    let send_dir = unique_dir("send");
    let payload = deterministic_bytes(40_000); // spans many DataChannel chunks
    let source = send_dir.join("webrtc-sample.bin");
    fs::write(&source, &payload).unwrap();
    let expected_hash = sha256_hex(&payload);

    // Responder runs the authenticated receive loop on its own thread.
    let received: Arc<Mutex<Vec<ReceivedFileEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_for_cb = Arc::clone(&received);
    let callback: FileReceivedCallback = Arc::new(move |event: ReceivedFileEvent| {
        received_for_cb.lock().unwrap().push(event);
    });
    let resp_writer = resp_duplex.clone();
    let resp_reader = BufReader::new(resp_duplex.clone());
    let receive_dir_for_thread = receive_dir.clone();
    let responder_thread = thread::spawn(move || {
        run_authenticated_responder_over(
            resp_writer,
            resp_reader,
            receiver,
            trust,
            receive_dir_for_thread,
            Some(callback),
        )
    });

    // Initiator sends the file over the DataChannel; returns after final ACK.
    let init_writer = init_duplex.clone();
    let init_reader = BufReader::new(init_duplex.clone());
    run_authenticated_file_sender_over(
        init_writer,
        init_reader,
        &sender,
        &receiver_device_id,
        &receiver_dh,
        &source,
    )
    .expect("authenticated file send over DataChannel");

    // Close so the responder's receive loop sees EOF and returns.
    init_duplex.close();
    let responder_result = responder_thread.join().expect("responder thread panicked");
    assert!(
        responder_result.is_ok(),
        "responder session should end Ok, got {responder_result:?}"
    );

    let events = received.lock().unwrap().clone();
    assert_eq!(events.len(), 1, "expected one received-file callback");
    let event = &events[0];
    assert_eq!(event.peer_device_id, sender.device_id());
    assert_eq!(event.filename, "webrtc-sample.bin");
    assert_eq!(event.size_bytes, payload.len() as u64);

    let final_path = Path::new(&event.final_path);
    assert!(final_path.exists(), "received file should exist");
    assert_eq!(
        sha256_hex(&fs::read(final_path).unwrap()),
        expected_hash,
        "received file SHA-256 must match source"
    );

    let _ = fs::remove_dir_all(&receive_dir);
    let _ = fs::remove_dir_all(&send_dir);
    drop(rt);
}
