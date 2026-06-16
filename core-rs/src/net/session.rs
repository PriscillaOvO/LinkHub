//! The unauthenticated, cleartext peer session ([`run_peer_session`]): a
//! background heartbeat writer plus a line-oriented [`WireMessage`] receive loop
//! that records trusted peers and writes incoming files (with resume support)
//! to the receive directory. The authenticated counterpart lives in
//! [`super::auth_session`].

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::{DeviceAgent, DeviceNode};

use super::ack::write_message;
use super::file_transfer::{
    file_chunk_ack_id, file_sha256_hex, file_start_ack_status, partial_file_path,
    receive_metadata_path, received_bytes_after_chunk, received_file_path,
    reusable_receive_progress_metadata, FileReceiveState,
};
use super::protocol::{decode_hex, parse_message, WireMessage};
use super::LocalDevice;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

pub(super) fn run_peer_session(
    stream: TcpStream,
    local: LocalDevice,
    agent: Arc<Mutex<DeviceAgent>>,
    receive_dir: PathBuf,
) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut ack_writer = stream.try_clone()?;
    let sender_local = local.clone();
    let running = Arc::new(AtomicBool::new(true));
    let writer_running = Arc::clone(&running);

    thread::spawn(move || {
        if let Err(err) = write_message(&mut writer, &WireMessage::hello(&sender_local)) {
            eprintln!("Failed to send hello: {err}");
            return;
        }

        while writer_running.load(Ordering::Relaxed) {
            thread::sleep(HEARTBEAT_INTERVAL);

            if !writer_running.load(Ordering::Relaxed) {
                break;
            }

            if let Err(err) = write_message(&mut writer, &WireMessage::heartbeat()) {
                eprintln!("Failed to send heartbeat: {err}");
                break;
            }
        }
    });

    let mut peer_id = None;
    let mut received_text_ids = HashSet::new();
    let mut received_file_start_ids = HashSet::new();
    let mut received_file_end_ids = HashSet::new();
    let mut file_receivers = HashMap::new();
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line?;
        match parse_message(&line) {
            Ok(WireMessage::Hello { device_id, name }) => {
                peer_id = Some(device_id.clone());

                let mut agent = agent.lock().expect("agent mutex poisoned");
                if agent.device(&device_id).is_none() {
                    agent.trust_device(DeviceNode::new(device_id.clone(), name.clone()));
                }

                println!("Trusted peer '{}' ({})", name, device_id);
                agent.print_status();
            }
            Ok(WireMessage::Heartbeat(update)) => {
                let Some(device_id) = peer_id.as_deref() else {
                    eprintln!("Ignored heartbeat before peer hello");
                    continue;
                };

                let mut agent = agent.lock().expect("agent mutex poisoned");
                agent.receive_heartbeat(device_id, update, Instant::now());
                agent.print_status();
            }
            Ok(WireMessage::Text {
                message_id,
                content,
            }) => {
                let Some(device_id) = peer_id.as_deref() else {
                    eprintln!("Ignored text before peer hello");
                    continue;
                };

                if received_text_ids.insert(message_id.clone()) {
                    println!("Text from {device_id} [{message_id}]: {content}");
                } else {
                    println!("Duplicate text from {device_id} [{message_id}] acknowledged again");
                }

                write_message(
                    &mut ack_writer,
                    &WireMessage::ack(&message_id, "TEXT_RECEIVED"),
                )?;
            }
            Ok(WireMessage::FileStart {
                transfer_id,
                filename,
                size_bytes,
                sha256_hex,
            }) => {
                let Some(device_id) = peer_id.as_deref() else {
                    eprintln!("Ignored file start before peer hello");
                    continue;
                };

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
                                "Resuming partial file from {device_id} [{transfer_id}]: received {} bytes, next chunk {}",
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
                        "File start from {device_id} [{transfer_id}]: {filename} ({size_bytes} bytes, sha256={}) -> {} (metadata: {})",
                        sha256_hex.as_deref().unwrap_or("not-provided"),
                        temp_path.display(),
                        metadata_path.display()
                    );
                } else {
                    println!(
                        "Duplicate file start from {device_id} [{transfer_id}] acknowledged again"
                    );
                }

                write_message(
                    &mut ack_writer,
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
                let Some(device_id) = peer_id.as_deref() else {
                    eprintln!("Ignored file chunk before peer hello");
                    continue;
                };
                let chunk_ack_id = file_chunk_ack_id(&transfer_id, chunk_index);
                let Some(receiver) = file_receivers.get_mut(&transfer_id) else {
                    eprintln!("Ignored file chunk for unknown transfer: {transfer_id}");
                    continue;
                };

                if chunk_index < receiver.next_chunk_index {
                    println!(
                        "Duplicate file chunk from {device_id} [{transfer_id}#{chunk_index}] acknowledged again"
                    );
                    write_message(
                        &mut ack_writer,
                        &WireMessage::ack(&chunk_ack_id, "FILE_CHUNK_RECEIVED"),
                    )?;
                    continue;
                }

                if chunk_index != receiver.next_chunk_index {
                    eprintln!(
                        "Ignored out-of-order file chunk from {device_id} [{transfer_id}#{chunk_index}], expected {}",
                        receiver.next_chunk_index
                    );
                    continue;
                }

                let bytes = decode_hex(&data_hex).map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid file chunk hex for {transfer_id}#{chunk_index}: {err}"),
                    )
                })?;
                let Some(next_received_bytes) = received_bytes_after_chunk(
                    receiver.received_bytes,
                    bytes.len(),
                    receiver.size_bytes,
                ) else {
                    eprintln!(
                        "Ignored oversized file chunk from {device_id} [{transfer_id}#{chunk_index}]: would exceed declared size {} bytes",
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
                    "File chunk from {device_id} [{transfer_id}#{chunk_index}]: {} bytes",
                    bytes.len()
                );
                write_message(
                    &mut ack_writer,
                    &WireMessage::ack(&chunk_ack_id, "FILE_CHUNK_RECEIVED"),
                )?;
            }
            Ok(WireMessage::FileEnd { transfer_id }) => {
                let Some(device_id) = peer_id.as_deref() else {
                    eprintln!("Ignored file end before peer hello");
                    continue;
                };

                if received_file_end_ids.insert(transfer_id.clone()) {
                    let Some(mut receiver) = file_receivers.remove(&transfer_id) else {
                        eprintln!("File end for unknown transfer from {device_id}: {transfer_id}");
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
                                    "File SHA-256 mismatch from {device_id} [{transfer_id}]: expected {expected_hash}, received {actual_hash}"
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
                            "File end from {device_id} [{transfer_id}]: saved {} ({} bytes)",
                            receiver.final_path.display(),
                            receiver.received_bytes
                        );
                    } else if !size_matches {
                        eprintln!(
                            "File size mismatch from {device_id} [{transfer_id}]: expected {} bytes, received {} bytes",
                            receiver.size_bytes, receiver.received_bytes
                        );
                        eprintln!(
                            "Incomplete file from {device_id} [{transfer_id}] kept at {} with metadata {}",
                            receiver.temp_path.display(),
                            receiver.metadata_path.display()
                        );
                    } else if !hash_matches {
                        eprintln!(
                            "Unverified file from {device_id} [{transfer_id}] kept at {} with metadata {}",
                            receiver.temp_path.display(),
                            receiver.metadata_path.display()
                        );
                    }
                } else {
                    println!(
                        "Duplicate file end from {device_id} [{transfer_id}] acknowledged again"
                    );
                }

                write_message(
                    &mut ack_writer,
                    &WireMessage::ack(&transfer_id, "FILE_END_RECEIVED"),
                )?;
            }
            Ok(WireMessage::Ack { message_id, status }) => {
                println!("ACK from peer: {message_id} {status}");
            }
            Ok(
                WireMessage::AuthChallenge { .. }
                | WireMessage::AuthSignature { .. }
                | WireMessage::NoiseHs { .. },
            ) => {
                eprintln!("Ignored auth/noise message on unauthenticated peer session");
            }
            Ok(message) => {
                eprintln!("Ignored message on unauthenticated session: {message:?}");
            }
            Err(err) => eprintln!("Ignored invalid peer message: {err}"),
        }
    }

    running.store(false, Ordering::Relaxed);

    if let Some(device_id) = peer_id {
        let mut agent = agent.lock().expect("agent mutex poisoned");
        agent.tick(Instant::now() + Duration::from_secs(9));
        println!("Peer connection closed: {device_id}");
        agent.print_status();
    }

    Ok(())
}
