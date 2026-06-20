use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::{LocalIdentity, TrustStore};

use super::auth_session::run_authenticated_session_with_accept;
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

/// A not-yet-trusted peer requesting to connect at first contact (no prior
/// pairing). The identity is **already cryptographically verified** before this
/// is surfaced: `device_id` derives from `public_key`, and `public_key` has
/// signed `dh_public_key` (so the wire DH key can't be MITM-swapped). The shell
/// only has to decide whether the *user* trusts this device (AirDrop-style
/// accept prompt showing `fingerprint`); the crypto is settled.
#[derive(Clone, Debug)]
pub struct IncomingPeer {
    pub device_id: String,
    pub device_name: String,
    pub public_key: String,
    pub dh_public_key: String,
    pub fingerprint: String,
}

/// Decides whether to accept a first-contact peer. Runs on the per-session
/// worker thread and **blocks the handshake** until it returns, so a UI shell
/// can show an accept/reject prompt. Returning `true` should also persist the
/// device to the trust store (build it from the `public_key`/`dh_public_key` in
/// [`IncomingPeer`]) so subsequent connections from it are silent.
pub type AcceptPeerCallback = Arc<dyn Fn(IncomingPeer) -> bool + Send + Sync>;

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
    should_stop: impl FnMut() -> bool,
    on_file_received: Option<FileReceivedCallback>,
) -> io::Result<()> {
    run_authenticated_listener_on_with_callbacks(
        listener,
        bind_label,
        local_identity,
        trust_store,
        receive_dir,
        should_stop,
        on_file_received,
        None,
    )
}

/// Same as [`run_authenticated_listener_on_with_callback`] with an optional
/// first-contact accept callback for AirDrop-style trust establishment.
#[allow(clippy::too_many_arguments)]
pub fn run_authenticated_listener_on_with_callbacks(
    listener: TcpListener,
    bind_label: &str,
    local_identity: LocalIdentity,
    trust_store: TrustStore,
    receive_dir: impl AsRef<Path>,
    mut should_stop: impl FnMut() -> bool,
    on_file_received: Option<FileReceivedCallback>,
    on_accept: Option<AcceptPeerCallback>,
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
                let on_accept = on_accept.clone();
                println!("Accepted authenticated peer connection from {peer_addr}");

                thread::spawn(move || {
                    if let Err(err) = run_authenticated_session_with_accept(
                        stream,
                        local_identity,
                        trust_store,
                        receive_dir,
                        on_file_received,
                        on_accept,
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
