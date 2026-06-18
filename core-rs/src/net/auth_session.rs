//! Authenticated peer sessions: the responder-side [`run_authenticated_session`]
//! receive loop and the initiator-side [`open_authenticated_stream`] handshake,
//! plus the Noise-encrypted frame send/recv helpers shared by the authenticated
//! senders in the parent module.
//!
//! Flow: plaintext HELLO → ed25519 challenge/signature against the trust store →
//! Noise KK handshake → length-prefixed ChaCha20-Poly1305 frames carrying the
//! same [`WireMessage`] protocol used in the clear by [`super::session`].

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

use crate::crypto::{NoiseHandshake, NoiseTransport};
use crate::{new_handshake_nonce, LocalIdentity, TrustStore};

use super::ack::{parse_file_start_ack_status, write_message, ACK_TIMEOUT};
use super::file_transfer::{
    file_chunk_ack_id, file_sha256_hex, file_start_ack_status, partial_file_path,
    receive_metadata_path, received_bytes_after_chunk, received_file_path,
    reusable_receive_progress_metadata, FileReceiveState,
};
use super::protocol::{decode_hex, encode_hex, parse_message, serialize_message, WireMessage};
use super::{FileReceivedCallback, LocalDevice, ReceivedFileEvent};

pub(super) fn run_authenticated_session(
    stream: TcpStream,
    local_identity: LocalIdentity,
    trust_store: Arc<TrustStore>,
    receive_dir: PathBuf,
    on_file_received: Option<FileReceivedCallback>,
) -> io::Result<()> {
    let writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    run_authenticated_session_over(
        writer,
        reader,
        local_identity,
        trust_store,
        receive_dir,
        on_file_received,
    )
}

/// Transport-agnostic responder side of the authenticated session.
///
/// Runs over any duplex byte stream — LAN `TcpStream` today; WebRTC DataChannel
/// or relay tunnel in stage 5 (see `docs/spec/设计-跨网络传输-webrtc.md`). The
/// `writer` and `reader` must be independent handles to the *same* connection
/// (e.g. a cloned socket); the security model (Noise KK bound to the trust
/// store) is identical regardless of the underlying transport.
pub(super) fn run_authenticated_session_over<W: Write, R: BufRead>(
    mut writer: W,
    mut reader: R,
    local_identity: LocalIdentity,
    trust_store: Arc<TrustStore>,
    receive_dir: PathBuf,
    on_file_received: Option<FileReceivedCallback>,
) -> io::Result<()> {
    let mut line = String::new();

    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed before HELLO",
        ));
    }

    let (peer_device_id, peer_name) = match parse_message(line.trim_end()) {
        Ok(WireMessage::Hello { device_id, name }) => (device_id, name),
        Ok(message) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected HELLO, received {message:?}"),
            ));
        }
        Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    };

    let Some(trusted) = trust_store.trusted_device(&peer_device_id) else {
        write_message(
            &mut writer,
            &WireMessage::ack(&peer_device_id, "AUTH_UNTRUSTED"),
        )?;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("untrusted authenticated peer: {peer_device_id}"),
        ));
    };

    let nonce = new_handshake_nonce();
    write_message(&mut writer, &WireMessage::auth_challenge(&nonce))?;

    line.clear();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed before AUTH_SIGNATURE",
        ));
    }

    match parse_message(line.trim_end()) {
        Ok(WireMessage::AuthSignature {
            device_id,
            signature_hex,
        }) if device_id == peer_device_id => {
            let verified = trusted
                .identity()
                .verify_handshake_signature(local_identity.device_id(), &nonce, &signature_hex)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

            if !verified {
                write_message(
                    &mut writer,
                    &WireMessage::ack(&peer_device_id, "AUTH_FAILED"),
                )?;
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("invalid authenticated peer signature: {peer_device_id}"),
                ));
            }
        }
        Ok(message) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected AUTH_SIGNATURE, received {message:?}"),
            ));
        }
        Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }

    // --- Noise KK handshake (responder side) ---
    let peer_db = trusted.identity().dh_public_key().to_string();
    let peer_db_bytes =
        decode_hex(&peer_db).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let peer_db_bytes: [u8; 32] = peer_db_bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid peer dh key length"))?;
    let local_db_bytes = local_identity
        .static_dh_key_bytes()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    let mut noise = NoiseHandshake::new_responder(&local_db_bytes, &peer_db_bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    println!("Authenticated peer '{}' ({})", peer_name, peer_device_id);
    write_message(&mut writer, &WireMessage::ack(&peer_device_id, "AUTH_OK"))?;

    // Step 1: Receive NOISE_HS from initiator
    let mut noise_line = String::new();
    if reader.read_line(&mut noise_line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed before NOISE_HS",
        ));
    }
    match parse_message(noise_line.trim_end()) {
        Ok(WireMessage::NoiseHs { payload_hex }) => {
            let payload = decode_hex(&payload_hex)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            let decrypted = noise
                .read_message(&payload)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if !decrypted.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected noise handshake payload",
                ));
            }
        }
        Ok(message) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected NOISE_HS, received {message:?}"),
            ));
        }
        Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }

    // Step 2: Send NOISE_HS response
    let response_payload = noise
        .write_message(&[])
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write_message(
        &mut writer,
        &WireMessage::noise_hs(&encode_hex(&response_payload)),
    )?;

    let mut transport = noise
        .into_transport()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    println!("Noise KK handshake complete — encrypted session established");

    // --- Encrypted message loop ---
    let mut received_text_ids = HashSet::new();
    let mut received_file_start_ids = HashSet::new();
    let mut received_file_end_ids = HashSet::new();
    let mut file_receivers = HashMap::new();
    loop {
        match recv_encrypted_frame(&mut transport, &mut reader) {
            Ok(WireMessage::Text {
                message_id,
                content,
            }) => {
                if received_text_ids.insert(message_id.clone()) {
                    println!("Authenticated text from {peer_device_id} [{message_id}]: {content}");
                } else {
                    println!(
                        "Duplicate authenticated text from {peer_device_id} [{message_id}] acknowledged again"
                    );
                }

                send_encrypted_frame(
                    &mut transport,
                    &mut writer,
                    &WireMessage::ack(&message_id, "TEXT_RECEIVED"),
                )?;
            }
            Ok(WireMessage::FileStart {
                transfer_id,
                filename,
                size_bytes,
                sha256_hex,
            }) => {
                if received_file_start_ids.insert(transfer_id.clone()) {
                    let final_path = received_file_path(&receive_dir, &transfer_id, &filename);
                    let temp_path = partial_file_path(&final_path);
                    let metadata_path = receive_metadata_path(&temp_path);
                    if let Some(parent) = final_path.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    let resume_metadata = reusable_receive_progress_metadata(
                        &metadata_path,
                        &transfer_id,
                        &filename,
                        size_bytes,
                        sha256_hex.as_deref(),
                        &temp_path,
                        &final_path,
                    );
                    let (file, received_bytes, next_chunk_index) = match resume_metadata {
                        Some((received_bytes, next_chunk_index)) => {
                            println!(
                                "Resuming authenticated partial file from {peer_device_id} [{transfer_id}]: received {} bytes, next chunk {}",
                                received_bytes, next_chunk_index
                            );
                            (
                                OpenOptions::new().append(true).open(&temp_path)?,
                                received_bytes,
                                next_chunk_index,
                            )
                        }
                        None => (File::create(&temp_path)?, 0, 0),
                    };
                    let receiver = FileReceiveState {
                        transfer_id: transfer_id.clone(),
                        filename: filename.clone(),
                        size_bytes,
                        expected_sha256_hex: sha256_hex.clone(),
                        received_bytes,
                        next_chunk_index,
                        final_path: final_path.clone(),
                        temp_path: temp_path.clone(),
                        metadata_path: metadata_path.clone(),
                        file,
                    };
                    receiver.write_progress_metadata()?;
                    file_receivers.insert(transfer_id.clone(), receiver);

                    println!(
                        "Authenticated file start from {peer_device_id} [{transfer_id}]: {filename} ({size_bytes} bytes, sha256={}) -> {} (metadata: {})",
                        sha256_hex.as_deref().unwrap_or("not-provided"),
                        temp_path.display(),
                        metadata_path.display()
                    );
                } else {
                    println!(
                        "Duplicate authenticated file start from {peer_device_id} [{transfer_id}] acknowledged again"
                    );
                }

                send_encrypted_frame(
                    &mut transport,
                    &mut writer,
                    &WireMessage::ack(
                        &transfer_id,
                        &file_start_ack_status(
                            sha256_hex.is_some(),
                            file_receivers
                                .get(&transfer_id)
                                .map(|receiver| receiver.next_chunk_index)
                                .unwrap_or(0),
                        ),
                    ),
                )?;
            }
            Ok(WireMessage::FileChunk {
                transfer_id,
                chunk_index,
                data_hex,
            }) => {
                let chunk_ack_id = file_chunk_ack_id(&transfer_id, chunk_index);
                let Some(receiver) = file_receivers.get_mut(&transfer_id) else {
                    eprintln!(
                        "Ignored authenticated file chunk for unknown transfer: {transfer_id}"
                    );
                    continue;
                };

                if chunk_index < receiver.next_chunk_index {
                    println!(
                        "Duplicate authenticated file chunk from {peer_device_id} [{transfer_id}#{chunk_index}] acknowledged again"
                    );
                    send_encrypted_frame(
                        &mut transport,
                        &mut writer,
                        &WireMessage::ack(&chunk_ack_id, "FILE_CHUNK_RECEIVED"),
                    )?;
                    continue;
                }

                if chunk_index != receiver.next_chunk_index {
                    eprintln!(
                        "Ignored out-of-order authenticated file chunk from {peer_device_id} [{transfer_id}#{chunk_index}], expected {}",
                        receiver.next_chunk_index
                    );
                    continue;
                }

                let bytes = decode_hex(&data_hex).map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid authenticated file chunk hex for {transfer_id}#{chunk_index}: {err}"),
                    )
                })?;
                let Some(next_received_bytes) = received_bytes_after_chunk(
                    receiver.received_bytes,
                    bytes.len(),
                    receiver.size_bytes,
                ) else {
                    eprintln!(
                        "Ignored oversized authenticated file chunk from {peer_device_id} [{transfer_id}#{chunk_index}]: would exceed declared size {} bytes",
                        receiver.size_bytes
                    );
                    continue;
                };

                receiver.file.write_all(&bytes)?;
                receiver.received_bytes = next_received_bytes;
                receiver.next_chunk_index += 1;
                receiver.file.flush()?;
                receiver.write_progress_metadata()?;

                println!(
                    "Authenticated file chunk from {peer_device_id} [{transfer_id}#{chunk_index}]: {} bytes",
                    bytes.len()
                );
                send_encrypted_frame(
                    &mut transport,
                    &mut writer,
                    &WireMessage::ack(&chunk_ack_id, "FILE_CHUNK_RECEIVED"),
                )?;
            }
            Ok(WireMessage::FileEnd { transfer_id }) => {
                if received_file_end_ids.insert(transfer_id.clone()) {
                    let Some(mut receiver) = file_receivers.remove(&transfer_id) else {
                        eprintln!(
                            "Authenticated file end for unknown transfer from {peer_device_id}: {transfer_id}"
                        );
                        continue;
                    };
                    receiver.file.flush()?;
                    drop(receiver.file);

                    let size_matches = receiver.received_bytes == receiver.size_bytes;
                    let hash_matches = match receiver.expected_sha256_hex.as_deref() {
                        Some(expected_hash) => {
                            let actual_hash = file_sha256_hex(&receiver.temp_path)?;
                            if actual_hash == expected_hash {
                                true
                            } else {
                                eprintln!(
                                    "Authenticated file SHA-256 mismatch from {peer_device_id} [{transfer_id}]: expected {expected_hash}, received {actual_hash}"
                                );
                                false
                            }
                        }
                        None => true,
                    };

                    if size_matches && hash_matches {
                        fs::rename(&receiver.temp_path, &receiver.final_path)?;
                        fs::remove_file(&receiver.metadata_path)?;
                        println!(
                            "Authenticated file end from {peer_device_id} [{transfer_id}]: saved {} ({} bytes)",
                            receiver.final_path.display(),
                            receiver.received_bytes
                        );
                        if let Some(callback) = on_file_received.as_ref() {
                            callback(ReceivedFileEvent {
                                peer_device_id: peer_device_id.clone(),
                                peer_device_name: peer_name.clone(),
                                filename: receiver.filename.clone(),
                                final_path: receiver.final_path.display().to_string(),
                                size_bytes: receiver.received_bytes,
                            });
                        }
                    } else if !size_matches {
                        eprintln!(
                            "Authenticated file size mismatch from {peer_device_id} [{transfer_id}]: expected {} bytes, received {} bytes",
                            receiver.size_bytes, receiver.received_bytes
                        );
                        eprintln!(
                            "Incomplete authenticated file from {peer_device_id} [{transfer_id}] kept at {} with metadata {}",
                            receiver.temp_path.display(),
                            receiver.metadata_path.display()
                        );
                    } else if !hash_matches {
                        eprintln!(
                            "Unverified authenticated file from {peer_device_id} [{transfer_id}] kept at {} with metadata {}",
                            receiver.temp_path.display(),
                            receiver.metadata_path.display()
                        );
                    }
                } else {
                    println!(
                        "Duplicate authenticated file end from {peer_device_id} [{transfer_id}] acknowledged again"
                    );
                }

                send_encrypted_frame(
                    &mut transport,
                    &mut writer,
                    &WireMessage::ack(&transfer_id, "FILE_END_RECEIVED"),
                )?;
            }
            Ok(WireMessage::Heartbeat(_)) => continue,
            Ok(WireMessage::AuthChallenge { .. }) => {
                eprintln!("Ignored replayed AUTH_CHALLENGE in encrypted session");
            }
            Ok(WireMessage::AuthSignature { .. }) => {
                eprintln!("Ignored replayed AUTH_SIGNATURE in encrypted session");
            }
            Ok(WireMessage::NoiseHs { .. }) => {
                eprintln!("Ignored replayed NOISE_HS in encrypted session");
            }
            Ok(message) => eprintln!("Ignored authenticated peer message: {message:?}"),
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof {
                    println!("Encrypted session closed by peer");
                    return Ok(());
                }
                eprintln!("Ignored invalid encrypted peer message: {err}");
            }
        }
    }
}

pub(super) fn send_encrypted_with_ack_retries<W: Write, R: BufRead>(
    transport: &mut NoiseTransport,
    writer: &mut W,
    reader: &mut R,
    message_id: &str,
    expected_ack_status: &str,
    make_message: impl Fn() -> WireMessage,
    label: &str,
) -> io::Result<()> {
    let mut last_error = None;

    for attempt in 1..=3 {
        if attempt > 1 {
            println!("Retrying encrypted {label} attempt {attempt}/3: {message_id}");
        }
        send_encrypted_frame(transport, writer, &make_message())?;

        match recv_encrypted_frame(transport, reader) {
            Ok(WireMessage::Ack {
                message_id: ack_id,
                status,
            }) => {
                let ok = ack_id == message_id
                    && (status == expected_ack_status
                        || (expected_ack_status == "FILE_START_RECEIVED"
                            && status.starts_with("FILE_START_RECEIVED")));
                if ok {
                    return Ok(());
                }
                last_error = Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected ACK: {ack_id} {status}"),
                ));
            }
            Ok(message) => {
                last_error = Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected ACK, received {message:?}"),
                ));
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof {
                    return Err(err);
                }
                eprintln!("No matching ACK for {message_id} on attempt {attempt}: {err}");
                last_error = Some(err);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "ACK retry attempts exhausted")))
}

pub(super) fn send_encrypted_file_start_with_retries<W: Write, R: BufRead>(
    transport: &mut NoiseTransport,
    writer: &mut W,
    reader: &mut R,
    transfer_id: &str,
    make_message: impl Fn() -> WireMessage,
) -> io::Result<u64> {
    let mut last_error = None;

    for attempt in 1..=3 {
        if attempt > 1 {
            println!("Retrying encrypted FILE_START attempt {attempt}/3: {transfer_id}");
        }
        send_encrypted_frame(transport, writer, &make_message())?;

        match recv_encrypted_frame(transport, reader) {
            Ok(WireMessage::Ack { message_id, status }) if message_id == transfer_id => {
                let Some(resume_from_chunk) = parse_file_start_ack_status(&status) else {
                    last_error = Some(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unexpected FILE_START ACK status: {status}"),
                    ));
                    continue;
                };
                println!("Delivery acknowledged: {message_id} {status}");
                return Ok(resume_from_chunk);
            }
            Ok(WireMessage::Ack { message_id, status }) => {
                last_error = Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected ACK: {message_id} {status}"),
                ));
            }
            Ok(message) => {
                last_error = Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected ACK, received {message:?}"),
                ));
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::UnexpectedEof {
                    return Err(err);
                }
                eprintln!("No matching ACK for {transfer_id} on attempt {attempt}: {err}");
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "encrypted FILE_START ACK retry attempts exhausted",
        )
    }))
}

fn send_encrypted_frame<W: Write>(
    transport: &mut NoiseTransport,
    writer: &mut W,
    message: &WireMessage,
) -> io::Result<()> {
    let plaintext = serialize_message(message);
    let ciphertext = transport
        .encrypt(plaintext.as_bytes())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if ciphertext.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "encrypted frame too large",
        ));
    }
    let len = ciphertext.len() as u16;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&ciphertext)?;
    writer.flush()
}

fn recv_encrypted_frame<R: BufRead>(
    transport: &mut NoiseTransport,
    reader: &mut R,
) -> io::Result<WireMessage> {
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf)?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut ciphertext = vec![0u8; len];
    reader.read_exact(&mut ciphertext)?;
    let plaintext = transport
        .decrypt(&ciphertext)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let line = String::from_utf8(plaintext)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    parse_message(&line).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(super) fn open_authenticated_stream(
    peer_addr: &str,
    local_identity: &LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
) -> io::Result<(TcpStream, BufReader<TcpStream>, NoiseTransport)> {
    let mut stream = TcpStream::connect(peer_addr)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    stream.set_read_timeout(Some(ACK_TIMEOUT))?;

    let transport = perform_initiator_handshake(
        &mut stream,
        &mut reader,
        local_identity,
        peer_device_id,
        peer_dh_public_key,
    )?;

    Ok((stream, reader, transport))
}

/// Transport-agnostic initiator handshake: plaintext HELLO → ed25519
/// challenge/signature against the trust store → Noise KK, returning the
/// established encrypted transport. Mirrors [`run_authenticated_session_over`]
/// for the connecting side and works over any duplex stream (LAN socket today,
/// WebRTC DataChannel / relay tunnel in stage 5).
pub(super) fn perform_initiator_handshake<W: Write, R: BufRead>(
    writer: &mut W,
    reader: &mut R,
    local_identity: &LocalIdentity,
    peer_device_id: &str,
    peer_dh_public_key: &[u8; 32],
) -> io::Result<NoiseTransport> {
    let local = LocalDevice::new(local_identity.device_id(), local_identity.device_name());

    write_message(writer, &WireMessage::hello(&local))?;
    let nonce = wait_for_auth_challenge(reader)?;
    let signature = local_identity
        .sign_handshake_challenge(peer_device_id, &nonce)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    write_message(
        writer,
        &WireMessage::auth_signature(local_identity.device_id(), &signature),
    )?;
    wait_for_ack(reader, local_identity.device_id(), "AUTH_OK")?;

    // --- Noise KK handshake (initiator side) ---
    let local_db_bytes = local_identity
        .static_dh_key_bytes()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut noise = NoiseHandshake::new_initiator(&local_db_bytes, peer_dh_public_key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    // Step 1: Send NOISE_HS to responder
    let init_payload = noise
        .write_message(&[])
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write_message(writer, &WireMessage::noise_hs(&encode_hex(&init_payload)))?;

    // Step 2: Receive NOISE_HS from responder
    let mut noise_line = String::new();
    loop {
        let bytes_read = reader.read_line(&mut noise_line)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before noise handshake response",
            ));
        }
        match parse_message(noise_line.trim_end()) {
            Ok(WireMessage::NoiseHs { payload_hex }) => {
                let payload = decode_hex(&payload_hex)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                let decrypted = noise
                    .read_message(&payload)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                if !decrypted.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected noise handshake payload",
                    ));
                }
                break;
            }
            Ok(WireMessage::Ack { message_id, status }) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("noise handshake rejected: {message_id} {status}"),
                ));
            }
            Ok(WireMessage::Hello { .. } | WireMessage::Heartbeat(_)) => {
                noise_line.clear();
                continue;
            }
            Ok(message) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected NOISE_HS response, received {message:?}"),
                ));
            }
            Err(err) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, err));
            }
        }
    }

    let transport = noise
        .into_transport()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    println!("Noise KK handshake complete — encrypted session established");

    Ok(transport)
}

fn wait_for_auth_challenge<R: BufRead>(reader: &mut R) -> io::Result<String> {
    loop {
        let mut response = String::new();
        let bytes_read = reader.read_line(&mut response)?;

        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before AUTH_CHALLENGE",
            ));
        }

        match parse_message(response.trim_end()) {
            Ok(WireMessage::AuthChallenge { nonce }) => return Ok(nonce),
            Ok(WireMessage::Ack { message_id, status }) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("authentication rejected: {message_id} {status}"),
                ));
            }
            Ok(WireMessage::Hello { .. } | WireMessage::Heartbeat(_)) => continue,
            Ok(message) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected AUTH_CHALLENGE, received {message:?}"),
                ));
            }
            Err(err) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid AUTH_CHALLENGE response: {err}"),
                ));
            }
        }
    }
}

fn wait_for_ack<R: BufRead>(
    reader: &mut R,
    expected_message_id: &str,
    expected_status: &str,
) -> io::Result<()> {
    loop {
        let mut response = String::new();
        let bytes_read = reader.read_line(&mut response)?;

        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before ACK",
            ));
        }

        match parse_message(response.trim_end()) {
            Ok(WireMessage::Ack { message_id, status }) => {
                if message_id == expected_message_id && status == expected_status {
                    println!("Delivery acknowledged: {message_id} {status}");
                    return Ok(());
                }

                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("authentication failed: {message_id} {status}"),
                ));
            }
            Ok(WireMessage::Hello { .. } | WireMessage::Heartbeat(_)) => continue,
            Ok(message) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected ACK, received {message:?}"),
                ));
            }
            Err(err) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid ACK response: {err}"),
                ));
            }
        }
    }
}

#[cfg(test)]
mod transport_tests {
    //! Proves the authenticated session is transport-agnostic by running the
    //! full handshake + encrypted TEXT exchange over an in-memory duplex (no
    //! sockets). This is the seam that stage-5 WebRTC / relay transports plug
    //! into; the TCP path is exercised separately by `tests/e2e.rs`.

    use super::*;
    use crate::TrustedDevice;
    use std::collections::VecDeque;
    use std::io::Read;
    use std::sync::{Condvar, Mutex};
    use std::thread;
    use std::time::SystemTime;

    /// A blocking byte channel shared between a writer end and a reader end.
    type MemChannel = Arc<(Mutex<MemState>, Condvar)>;

    struct MemState {
        buf: VecDeque<u8>,
        closed: bool,
    }

    fn new_channel() -> MemChannel {
        Arc::new((
            Mutex::new(MemState {
                buf: VecDeque::new(),
                closed: false,
            }),
            Condvar::new(),
        ))
    }

    /// One end of an in-memory full-duplex link. `rx` is what we read; `tx` is
    /// what we write. Cloning shares the same underlying channels, so a writer
    /// handle and a `BufReader`-wrapped reader handle stay wired together.
    struct MemoryDuplex {
        rx: MemChannel,
        tx: MemChannel,
    }

    impl MemoryDuplex {
        fn pair() -> (MemoryDuplex, MemoryDuplex) {
            let a = new_channel();
            let b = new_channel();
            (
                MemoryDuplex {
                    rx: b.clone(),
                    tx: a.clone(),
                },
                MemoryDuplex { rx: a, tx: b },
            )
        }

        fn handle(&self) -> MemoryDuplex {
            MemoryDuplex {
                rx: self.rx.clone(),
                tx: self.tx.clone(),
            }
        }

        /// Signal EOF to the peer reading our `tx`.
        fn close_tx(&self) {
            let (lock, cvar) = &*self.tx;
            lock.lock().unwrap().closed = true;
            cvar.notify_all();
        }
    }

    impl Read for MemoryDuplex {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let (lock, cvar) = &*self.rx;
            let mut state = lock.lock().unwrap();
            loop {
                if !state.buf.is_empty() {
                    let n = state.buf.len().min(out.len());
                    for slot in out.iter_mut().take(n) {
                        *slot = state.buf.pop_front().unwrap();
                    }
                    return Ok(n);
                }
                if state.closed {
                    return Ok(0);
                }
                state = cvar.wait(state).unwrap();
            }
        }
    }

    impl Write for MemoryDuplex {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            let (lock, cvar) = &*self.tx;
            let mut state = lock.lock().unwrap();
            state.buf.extend(data.iter().copied());
            cvar.notify_all();
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn dh_bytes(identity: &LocalIdentity) -> [u8; 32] {
        decode_hex(identity.dh_public_key())
            .unwrap()
            .try_into()
            .unwrap()
    }

    #[test]
    fn authenticated_text_round_trips_over_in_memory_transport() {
        let now = SystemTime::now();
        let initiator = LocalIdentity::generate("Initiator", now);
        let responder = LocalIdentity::generate("Responder", now);
        let responder_dh = dh_bytes(&responder);

        // Responder trusts the initiator (the side it authenticates by signature).
        let mut trust = TrustStore::new();
        trust.trust(TrustedDevice::new(initiator.identity().clone(), now));
        let trust = Arc::new(trust);

        let receive_dir = std::env::temp_dir().join(format!(
            "linkhub-mem-{}-{:?}",
            now.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            thread::current().id()
        ));

        let (initiator_end, responder_end) = MemoryDuplex::pair();
        let resp_writer = responder_end.handle();
        let resp_reader = BufReader::new(responder_end.handle());

        let responder_identity = responder.clone();
        let receive_dir_for_thread = receive_dir.clone();
        let responder_thread = thread::spawn(move || {
            run_authenticated_session_over(
                resp_writer,
                resp_reader,
                responder_identity,
                trust,
                receive_dir_for_thread,
                None,
            )
        });

        // Initiator side over the same in-memory link.
        let mut init_writer = initiator_end.handle();
        let mut init_reader = BufReader::new(initiator_end.handle());
        let mut transport = perform_initiator_handshake(
            &mut init_writer,
            &mut init_reader,
            &initiator,
            responder.device_id(),
            &responder_dh,
        )
        .expect("initiator handshake should complete over memory transport");

        let message_id = format!("{}-mem-1", initiator.device_id());
        send_encrypted_with_ack_retries(
            &mut transport,
            &mut init_writer,
            &mut init_reader,
            &message_id,
            "TEXT_RECEIVED",
            || WireMessage::text(&message_id, "hello over in-memory transport"),
            "TEXT",
        )
        .expect("encrypted TEXT should be acknowledged over memory transport");

        // Close the link so the responder's receive loop ends cleanly.
        init_writer.close_tx();
        let responder_result = responder_thread.join().expect("responder thread panicked");
        assert!(
            responder_result.is_ok(),
            "responder session should end Ok on EOF, got {responder_result:?}"
        );

        let _ = std::fs::remove_dir_all(&receive_dir);
    }
}
