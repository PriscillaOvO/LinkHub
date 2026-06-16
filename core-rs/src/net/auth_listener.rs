use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::{LocalIdentity, TrustStore};

use super::auth_session::run_authenticated_session;
use super::RECEIVED_DIR;

pub fn run_authenticated_text_listener(
    bind_addr: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
) -> io::Result<()> {
    run_authenticated_listener_with_receive_dir(
        bind_addr,
        local_identity,
        trust_store,
        RECEIVED_DIR,
    )
}

pub fn run_authenticated_listener_with_receive_dir(
    bind_addr: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
    receive_dir: impl AsRef<Path>,
) -> io::Result<()> {
    run_authenticated_listener_until(bind_addr, local_identity, trust_store, receive_dir, || {
        false
    })
}

pub fn run_authenticated_listener_until(
    bind_addr: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
    receive_dir: impl AsRef<Path>,
    should_stop: impl FnMut() -> bool,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind_addr)?;
    run_authenticated_listener_on(
        listener,
        bind_addr,
        local_identity,
        trust_store,
        receive_dir,
        should_stop,
    )
}

/// Details about a file that an authenticated peer finished sending to this
/// listener. Surfaced to platform shells (e.g. the Android service) so they can
/// notify the user and record receive history.
#[derive(Clone, Debug)]
pub struct ReceivedFileEvent {
    pub peer_device_id: String,
    pub peer_device_name: String,
    pub filename: String,
    pub final_path: String,
    pub size_bytes: u64,
}

/// Callback invoked once a file is fully received and verified. It runs on the
/// per-session worker thread, so it must be `Send + Sync`.
pub type FileReceivedCallback = Arc<dyn Fn(ReceivedFileEvent) + Send + Sync>;

pub fn run_authenticated_listener_on(
    listener: TcpListener,
    bind_label: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
    receive_dir: impl AsRef<Path>,
    should_stop: impl FnMut() -> bool,
) -> io::Result<()> {
    run_authenticated_listener_on_with_callback(
        listener,
        bind_label,
        local_identity,
        trust_store,
        receive_dir,
        should_stop,
        None,
    )
}

/// Same as [`run_authenticated_listener_on`] but invokes `on_file_received`
/// after each file is fully received and integrity-verified.
pub fn run_authenticated_listener_on_with_callback(
    listener: TcpListener,
    bind_label: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
    receive_dir: impl AsRef<Path>,
    mut should_stop: impl FnMut() -> bool,
    on_file_received: Option<FileReceivedCallback>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let trust_store = Arc::new(trust_store);
    let receive_dir = receive_dir.as_ref().to_path_buf();

    println!(
        "LinkHub authenticated agent '{}' ({}) listening on {} and saving files to {}",
        local_identity.device_name(),
        local_identity.device_id(),
        bind_label,
        receive_dir.display()
    );

    while !should_stop() {
        match listener.accept() {
            Ok((stream, peer_addr)) => {
                stream.set_nonblocking(false)?;
                let local_identity = local_identity.clone();
                let trust_store = Arc::clone(&trust_store);
                let receive_dir = receive_dir.clone();
                let on_file_received = on_file_received.clone();
                println!("Accepted authenticated peer connection from {peer_addr}");

                thread::spawn(move || {
                    if let Err(err) = run_authenticated_session(
                        stream,
                        local_identity,
                        trust_store,
                        receive_dir,
                        on_file_received,
                    ) {
                        eprintln!("Authenticated peer session ended with error: {err}");
                    }
                });
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => eprintln!("Failed to accept authenticated peer connection: {err}"),
        }
    }

    Ok(())
}
