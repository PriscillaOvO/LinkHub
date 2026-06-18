use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{DeviceAgent, LocalIdentity, TrustStore};

mod ack;
mod auth_listener;
mod auth_session;
mod connection_plan;
mod file_transfer;
mod protocol;
mod session;
mod signaling_client;
mod signaling_signed;
#[cfg(feature = "webrtc")]
pub mod webrtc_session;
#[cfg(feature = "webrtc")]
pub mod webrtc_transport;

use ack::{
    send_file_start_with_retries, send_text_with_retries, send_with_ack_retries, write_message,
    ACK_TIMEOUT,
};
use auth_session::{
    open_authenticated_stream, perform_initiator_handshake, run_authenticated_session_over,
    send_encrypted_file_start_with_retries, send_encrypted_with_ack_retries,
};
use file_transfer::{file_chunk_ack_id, file_sha256_hex, file_transfer_id, FILE_CHUNK_SIZE};
use protocol::{encode_hex, sanitize_field, WireMessage};
use session::run_peer_session;

pub use auth_listener::{
    run_authenticated_listener_on, run_authenticated_listener_on_with_callback,
    run_authenticated_listener_until, run_authenticated_listener_with_receive_dir,
    run_authenticated_text_listener, FileReceivedCallback, ReceivedFileEvent,
};
pub use connection_plan::{
    attempt_with_fallback, plan_connection, preferred_established_route, ConnectionPath,
    ConnectionPlan, PeerReachability,
};
pub use signaling_client::{RetryPolicy, SignalingClient, SignalingDelivery, SignalingEvent};
pub use signaling_signed::{open_sdp, seal_sdp, verify_signaling_sdp};

const RECEIVED_DIR: &str = "received";

#[derive(Clone, Debug)]
pub struct LocalDevice {
    pub id: String,
    pub name: String,
}

impl LocalDevice {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

pub fn run_listener(bind_addr: &str, local: LocalDevice) -> io::Result<()> {
    run_listener_with_receive_dir(bind_addr, local, RECEIVED_DIR)
}

pub fn run_listener_with_receive_dir(
    bind_addr: &str,
    local: LocalDevice,
    receive_dir: impl AsRef<Path>,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind_addr)?;
    let agent = Arc::new(Mutex::new(DeviceAgent::new(local.name.clone())));
    let receive_dir = receive_dir.as_ref().to_path_buf();

    println!(
        "LinkHub agent '{}' ({}) listening on {} and saving files to {}",
        local.name,
        local.id,
        bind_addr,
        receive_dir.display()
    );

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let peer_addr = stream.peer_addr()?;
                let local = local.clone();
                let agent = Arc::clone(&agent);
                let receive_dir = receive_dir.clone();
                println!("Accepted peer connection from {peer_addr}");

                thread::spawn(move || {
                    if let Err(err) = run_peer_session(stream, local, agent, receive_dir) {
                        eprintln!("Peer session ended with error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("Failed to accept peer connection: {err}"),
        }
    }

    Ok(())
}

pub fn run_connector(peer_addr: &str, local: LocalDevice) -> io::Result<()> {
    run_connector_with_receive_dir(peer_addr, local, RECEIVED_DIR)
}

pub fn run_connector_with_receive_dir(
    peer_addr: &str,
    local: LocalDevice,
    receive_dir: impl AsRef<Path>,
) -> io::Result<()> {
    let stream = TcpStream::connect(peer_addr)?;
    let agent = Arc::new(Mutex::new(DeviceAgent::new(local.name.clone())));
    let receive_dir = receive_dir.as_ref().to_path_buf();

    println!(
        "LinkHub agent '{}' ({}) connected to {} and saving files to {}",
        local.name,
        local.id,
        peer_addr,
        receive_dir.display()
    );

    run_peer_session(stream, local, agent, receive_dir)
}

pub fn run_text_sender(peer_addr: &str, local: LocalDevice, text: &str) -> io::Result<()> {
    let mut stream = TcpStream::connect(peer_addr)?;
    let message_id = new_message_id(&local.id);

    println!(
        "LinkHub agent '{}' ({}) sending text {} to {}",
        local.name, local.id, message_id, peer_addr
    );

    write_message(&mut stream, &WireMessage::hello(&local))?;
    stream.set_read_timeout(Some(ACK_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    send_text_with_retries(&mut stream, &mut reader, &message_id, text)?;
    let _ = stream.shutdown(Shutdown::Write);

    Ok(())
}

pub fn run_authenticated_text_sender(
    peer_addr: &str,
    local_identity: LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
    text: &str,
) -> io::Result<()> {
    println!(
        "LinkHub authenticated agent '{}' ({}) sending text to {}",
        local_identity.device_name(),
        local_identity.device_id(),
        peer_addr
    );

    let (mut stream, mut reader, mut transport) = open_authenticated_stream(
        peer_addr,
        &local_identity,
        peer_device_id,
        peer_dh_public_key,
    )?;
    let message_id = new_message_id(local_identity.device_id());
    send_encrypted_with_ack_retries(
        &mut transport,
        &mut stream,
        &mut reader,
        &message_id,
        "TEXT_RECEIVED",
        || WireMessage::text(&message_id, text),
        "TEXT",
    )?;
    let _ = stream.shutdown(Shutdown::Write);

    Ok(())
}

/// Transport-agnostic authenticated TEXT send (Stage 5): runs the initiator
/// handshake + Noise KK + one encrypted TEXT over any duplex byte stream
/// (`writer`/`reader` are independent handles to the same connection). LAN uses
/// [`run_authenticated_text_sender`] over TCP; the WebRTC path drives this over a
/// [`crate::net::webrtc_transport::DataChannelDuplex`].
pub fn run_authenticated_text_sender_over<W: Write, R: BufRead>(
    mut writer: W,
    mut reader: R,
    local_identity: &LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
    text: &str,
) -> io::Result<()> {
    let mut transport = perform_initiator_handshake(
        &mut writer,
        &mut reader,
        local_identity,
        peer_device_id,
        peer_dh_public_key,
    )?;
    let message_id = new_message_id(local_identity.device_id());
    send_encrypted_with_ack_retries(
        &mut transport,
        &mut writer,
        &mut reader,
        &message_id,
        "TEXT_RECEIVED",
        || WireMessage::text(&message_id, text),
        "TEXT",
    )
}

/// Transport-agnostic responder side: trust-store-authenticated Noise KK receive
/// loop over any duplex stream. Thin public wrapper over the internal
/// session runner so non-TCP transports (WebRTC) can reuse it.
pub fn run_authenticated_responder_over<W: Write, R: BufRead>(
    writer: W,
    reader: R,
    local_identity: LocalIdentity,
    trust_store: Arc<TrustStore>,
    receive_dir: impl AsRef<Path>,
    on_file_received: Option<FileReceivedCallback>,
) -> io::Result<()> {
    run_authenticated_session_over(
        writer,
        reader,
        local_identity,
        trust_store,
        receive_dir.as_ref().to_path_buf(),
        on_file_received,
    )
}

pub fn run_authenticated_file_sender(
    peer_addr: &str,
    local_identity: LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
    path: impl AsRef<Path>,
) -> io::Result<()> {
    println!(
        "LinkHub authenticated agent '{}' ({}) sending file to {}",
        local_identity.device_name(),
        local_identity.device_id(),
        peer_addr
    );

    let stream = TcpStream::connect(peer_addr)?;
    stream.set_read_timeout(Some(ACK_TIMEOUT))?;
    let reader = BufReader::new(stream.try_clone()?);

    run_authenticated_file_sender_over(
        &stream,
        reader,
        &local_identity,
        peer_device_id,
        peer_dh_public_key,
        path,
    )?;
    let _ = stream.shutdown(Shutdown::Write);

    Ok(())
}

/// Transport-agnostic authenticated FILE send (Stage 5): initiator handshake +
/// Noise KK + chunked FILE_START/FILE_CHUNK/FILE_END with per-chunk ACKs and
/// resume, over any duplex byte stream. LAN uses [`run_authenticated_file_sender`]
/// over TCP; the WebRTC path drives this over a
/// [`crate::net::webrtc_transport::DataChannelDuplex`].
pub fn run_authenticated_file_sender_over<W: Write, R: BufRead>(
    mut writer: W,
    mut reader: R,
    local_identity: &LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
    path: impl AsRef<Path>,
) -> io::Result<()> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;

    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not a file: {}", path.display()),
        ));
    }

    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("file has no valid UTF-8 filename: {}", path.display()),
            )
        })?;
    let sha256_hex = file_sha256_hex(path)?;
    let transfer_id = file_transfer_id(
        local_identity.device_id(),
        filename,
        metadata.len(),
        &sha256_hex,
    );

    let mut transport = perform_initiator_handshake(
        &mut writer,
        &mut reader,
        local_identity,
        peer_device_id,
        peer_dh_public_key,
    )?;

    let resume_from_chunk = send_encrypted_file_start_with_retries(
        &mut transport,
        &mut writer,
        &mut reader,
        &transfer_id,
        || WireMessage::file_start_with_hash(&transfer_id, filename, metadata.len(), &sha256_hex),
    )?;

    let mut file = File::open(path)?;
    let mut buffer = vec![0; FILE_CHUNK_SIZE];
    let mut chunk_index = 0;

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        if chunk_index < resume_from_chunk {
            println!("Skipping already received FILE_CHUNK {transfer_id}:{chunk_index}");
            chunk_index += 1;
            continue;
        }

        let data_hex = encode_hex(&buffer[..bytes_read]);
        let chunk_ack_id = file_chunk_ack_id(&transfer_id, chunk_index);
        send_encrypted_with_ack_retries(
            &mut transport,
            &mut writer,
            &mut reader,
            &chunk_ack_id,
            "FILE_CHUNK_RECEIVED",
            || WireMessage::file_chunk(&transfer_id, chunk_index, &data_hex),
            "FILE_CHUNK",
        )?;

        chunk_index += 1;
    }

    send_encrypted_with_ack_retries(
        &mut transport,
        &mut writer,
        &mut reader,
        &transfer_id,
        "FILE_END_RECEIVED",
        || WireMessage::file_end(&transfer_id),
        "FILE_END",
    )
}

pub fn run_file_control_sender(
    peer_addr: &str,
    local: LocalDevice,
    path: impl AsRef<Path>,
) -> io::Result<()> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;

    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not a file: {}", path.display()),
        ));
    }

    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("file has no valid UTF-8 filename: {}", path.display()),
            )
        })?;
    let transfer_id = new_message_id(&local.id);
    let mut stream = TcpStream::connect(peer_addr)?;

    println!(
        "LinkHub agent '{}' ({}) sending file control {} to {}",
        local.name, local.id, transfer_id, peer_addr
    );

    write_message(&mut stream, &WireMessage::hello(&local))?;
    stream.set_read_timeout(Some(ACK_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    send_with_ack_retries(
        &mut stream,
        &mut reader,
        &transfer_id,
        "FILE_START_RECEIVED",
        || WireMessage::file_start(&transfer_id, filename, metadata.len()),
        "FILE_START",
    )?;
    send_with_ack_retries(
        &mut stream,
        &mut reader,
        &transfer_id,
        "FILE_END_RECEIVED",
        || WireMessage::file_end(&transfer_id),
        "FILE_END",
    )?;
    let _ = stream.shutdown(Shutdown::Write);

    Ok(())
}

pub fn run_file_sender(
    peer_addr: &str,
    local: LocalDevice,
    path: impl AsRef<Path>,
) -> io::Result<()> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;

    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not a file: {}", path.display()),
        ));
    }

    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("file has no valid UTF-8 filename: {}", path.display()),
            )
        })?;
    let sha256_hex = file_sha256_hex(path)?;
    let transfer_id = file_transfer_id(&local.id, filename, metadata.len(), &sha256_hex);
    let mut stream = TcpStream::connect(peer_addr)?;

    println!(
        "LinkHub agent '{}' ({}) sending file {} to {}",
        local.name, local.id, transfer_id, peer_addr
    );

    write_message(&mut stream, &WireMessage::hello(&local))?;
    stream.set_read_timeout(Some(ACK_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let resume_from_chunk =
        send_file_start_with_retries(&mut stream, &mut reader, &transfer_id, || {
            WireMessage::file_start_with_hash(&transfer_id, filename, metadata.len(), &sha256_hex)
        })?;

    let mut file = File::open(path)?;
    let mut buffer = vec![0; FILE_CHUNK_SIZE];
    let mut chunk_index = 0;

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        if chunk_index < resume_from_chunk {
            println!("Skipping already received FILE_CHUNK {transfer_id}:{chunk_index}");
            chunk_index += 1;
            continue;
        }

        let data_hex = encode_hex(&buffer[..bytes_read]);
        let chunk_ack_id = file_chunk_ack_id(&transfer_id, chunk_index);
        send_with_ack_retries(
            &mut stream,
            &mut reader,
            &chunk_ack_id,
            "FILE_CHUNK_RECEIVED",
            || WireMessage::file_chunk(&transfer_id, chunk_index, &data_hex),
            "FILE_CHUNK",
        )?;

        chunk_index += 1;
    }

    send_with_ack_retries(
        &mut stream,
        &mut reader,
        &transfer_id,
        "FILE_END_RECEIVED",
        || WireMessage::file_end(&transfer_id),
        "FILE_END",
    )?;
    let _ = stream.shutdown(Shutdown::Write);

    Ok(())
}

fn new_message_id(device_id: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    format!("{}-{millis}", sanitize_field(device_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_message_id_contains_sanitized_device_id() {
        let message_id = new_message_id("phone\t001");

        assert!(message_id.starts_with("phone 001-"));
    }
}
