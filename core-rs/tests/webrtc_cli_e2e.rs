//! T1 acceptance (feature `webrtc`): drive the real `linkhub-cli`
//! `listen-webrtc` and `connect-webrtc` commands through a real
//! `linkhub-signaling-server`, then verify the received file bytes match.
#![cfg(feature = "webrtc")]

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn start_server() -> String {
    let (addr_tx, addr_rx) = mpsc::channel::<SocketAddr>();
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind signaling server");
            addr_tx
                .send(listener.local_addr().expect("local addr"))
                .expect("send addr");
            let _ = linkhub_signaling_server::serve(listener).await;
        });
    });
    let addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server reports its address");
    format!("ws://{addr}")
}

#[test]
fn cli_file_transfer_over_webrtc_signaling_server() {
    let cli = env!("CARGO_BIN_EXE_linkhub-cli");
    let url = start_server();
    let sandbox = TestSandbox::new("linkhub-webrtc-cli-e2e");
    let receiver_identity = sandbox.path("receiver-identity.txt");
    let sender_identity = sandbox.path("sender-identity.txt");
    let receiver_trust = sandbox.path("receiver-trust-store.txt");
    let sender_trust = sandbox.path("sender-trust-store.txt");
    let receive_dir = sandbox.path("received");
    let listener_out = sandbox.path("listener.out");
    let listener_err = sandbox.path("listener.err");
    let send_file = sandbox.path("webrtc sample.bin");
    let expected_file = deterministic_bytes(40_000);
    fs::create_dir_all(&receive_dir).unwrap();
    fs::write(&send_file, &expected_file).unwrap();

    command_ok(
        cli,
        &[
            "identity",
            "init",
            path_str(&receiver_identity),
            "Receiver PC",
        ],
        Duration::from_secs(8),
    );
    command_ok(
        cli,
        &["identity", "init", path_str(&sender_identity), "Sender PC"],
        Duration::from_secs(8),
    );

    pair_devices(cli, &sender_identity, &receiver_identity, &receiver_trust);
    pair_devices(cli, &receiver_identity, &sender_identity, &sender_trust);

    let receiver_id = identity_field(cli, &receiver_identity, "device_id");
    let mut listener = WebRtcListenerChild::spawn(
        cli,
        &url,
        &receiver_identity,
        &receiver_trust,
        &receive_dir,
        &listener_out,
        &listener_err,
    );
    wait_for_file_contains(
        &listener_out,
        "WebRTC listener present",
        Duration::from_secs(8),
    );

    command_ok(
        cli,
        &[
            "connect-webrtc",
            &url,
            path_str(&sender_identity),
            &receiver_id,
            path_str(&sender_trust),
            path_str(&send_file),
        ],
        Duration::from_secs(30),
    );

    let received =
        wait_for_received_file(&receive_dir, "webrtc sample.bin", Duration::from_secs(8));
    assert_eq!(fs::read(received).unwrap(), expected_file);
    listener.stop();
}

fn pair_devices(cli: &str, peer_identity: &Path, local_identity: &Path, local_trust_store: &Path) {
    let payload_output = command_stdout(
        cli,
        &[
            "identity",
            "pairing-payload",
            path_str(peer_identity),
            "120",
        ],
        Duration::from_secs(8),
    );
    let payload = payload_output.lines().next().expect("pairing payload");
    let code_output = command_stdout(
        cli,
        &[
            "identity",
            "pairing-code",
            path_str(local_identity),
            payload,
        ],
        Duration::from_secs(8),
    );
    let code = parse_key(&code_output, "confirmation_code");
    command_ok(
        cli,
        &[
            "identity",
            "trust-pairing",
            path_str(local_identity),
            payload,
            &code,
            path_str(local_trust_store),
        ],
        Duration::from_secs(8),
    );
}

fn identity_field(cli: &str, identity: &Path, field: &str) -> String {
    let output = command_stdout(
        cli,
        &["identity", "show", path_str(identity)],
        Duration::from_secs(8),
    );
    parse_key(&output, field)
}

fn parse_key(output: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing {key}= in output:\n{output}"))
        .to_string()
}

fn command_ok(cli: &str, args: &[&str], timeout: Duration) {
    let output = command_output(cli, args, timeout);
    assert!(
        output.status.success(),
        "command {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn command_stdout(cli: &str, args: &[&str], timeout: Duration) -> String {
    let output = command_output(cli, args, timeout);
    assert!(
        output.status.success(),
        "command {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout utf-8")
}

fn command_output(cli: &str, args: &[&str], timeout: Duration) -> std::process::Output {
    let mut child = Command::new(cli)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("failed to run {args:?}: {err}"));
    let started = Instant::now();
    while started.elapsed() < timeout {
        if child.try_wait().expect("poll child").is_some() {
            return child.wait_with_output().expect("collect child output");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let output = child.wait_with_output().expect("collect timed out child");
    panic!(
        "command {args:?} timed out after {timeout:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if fs::read_to_string(path)
            .map(|content| content.contains(needle))
            .unwrap_or(false)
        {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let content = fs::read_to_string(path).unwrap_or_default();
    panic!("{needle:?} did not appear in {}\n{content}", path.display());
}

fn wait_for_received_file(base_dir: &Path, filename: &str, timeout: Duration) -> PathBuf {
    let started = Instant::now();
    while started.elapsed() < timeout {
        for entry in fs::read_dir(base_dir).unwrap() {
            let path = entry.unwrap().path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == filename || name.ends_with(&format!("_{filename}")) {
                return path;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("{filename} did not appear in {}", base_dir.display());
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 17) % 251) as u8).collect()
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test path utf-8")
}

struct WebRtcListenerChild {
    child: Child,
}

impl WebRtcListenerChild {
    fn spawn(
        cli: &str,
        ws_url: &str,
        identity: &Path,
        trust_store: &Path,
        receive_dir: &Path,
        stdout_path: &Path,
        stderr_path: &Path,
    ) -> Self {
        let stdout = fs::File::create(stdout_path).unwrap();
        let stderr = fs::File::create(stderr_path).unwrap();
        let child = Command::new(cli)
            .args([
                "listen-webrtc",
                ws_url,
                path_str(identity),
                path_str(trust_store),
                "--receive-dir",
                path_str(receive_dir),
            ])
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn WebRTC listener");
        Self { child }
    }

    fn stop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WebRtcListenerChild {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TestSandbox {
    root: PathBuf,
}

impl TestSandbox {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("{name}-{}-{}", std::process::id(), unique_suffix()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TestSandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
