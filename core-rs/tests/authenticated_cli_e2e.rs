use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn authenticated_cli_text_and_file_cross_process() {
    let cli = env!("CARGO_BIN_EXE_linkhub-cli");
    let sandbox = TestSandbox::new("linkhub-auth-cli-e2e");
    let receiver_identity = sandbox.path("receiver-identity.txt");
    let sender_identity = sandbox.path("sender-identity.txt");
    let receiver_trust = sandbox.path("receiver-trust-store.txt");
    let sender_trust = sandbox.path("sender-trust-store.txt");
    let receive_dir = sandbox.path("received");
    let listener_out = sandbox.path("listener.out");
    let listener_err = sandbox.path("listener.err");
    let send_file = sandbox.path("sample.txt");
    let expected_file = b"authenticated cli file transfer\n";
    fs::create_dir_all(&receive_dir).unwrap();
    fs::write(&send_file, expected_file).unwrap();

    command_ok(
        cli,
        &[
            "identity",
            "init",
            path_str(&receiver_identity),
            "Receiver PC",
        ],
    );
    command_ok(
        cli,
        &["identity", "init", path_str(&sender_identity), "Sender PC"],
    );

    pair_devices(cli, &sender_identity, &receiver_identity, &receiver_trust);
    pair_devices(cli, &receiver_identity, &sender_identity, &sender_trust);

    let receiver_id = identity_field(cli, &receiver_identity, "device_id");
    let port = free_tcp_port();
    let addr = format!("127.0.0.1:{port}");
    let mut listener = ListenerChild::spawn(
        cli,
        &addr,
        &receiver_identity,
        &receiver_trust,
        &receive_dir,
        &listener_out,
        &listener_err,
    );

    wait_for_port("127.0.0.1", port, Duration::from_secs(5));

    command_ok(
        cli,
        &[
            "send-text-auth",
            &addr,
            path_str(&sender_identity),
            &receiver_id,
            path_str(&sender_trust),
            "authenticated cargo test text",
        ],
    );

    command_ok(
        cli,
        &[
            "send-file-auth",
            &addr,
            path_str(&sender_identity),
            &receiver_id,
            path_str(&sender_trust),
            path_str(&send_file),
        ],
    );

    let received = wait_for_received_file(&receive_dir, "sample.txt", Duration::from_secs(5));
    assert_eq!(fs::read(received).unwrap(), expected_file);

    listener.stop();

    let _ = listener_err;
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
    );
}

fn identity_field(cli: &str, identity: &Path, field: &str) -> String {
    let output = command_stdout(cli, &["identity", "show", path_str(identity)]);
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

fn command_ok(cli: &str, args: &[&str]) {
    let output = command_output(cli, args, Duration::from_secs(8));
    assert!(
        output.status.success(),
        "command {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn command_stdout(cli: &str, args: &[&str]) -> String {
    let output = command_output(cli, args, Duration::from_secs(8));
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

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind free port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn wait_for_port(host: &str, port: u16, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if TcpStream::connect((host, port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("{host}:{port} did not open within {timeout:?}");
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

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test path utf-8")
}

struct ListenerChild {
    child: Child,
}

impl ListenerChild {
    fn spawn(
        cli: &str,
        addr: &str,
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
                "listen-auth",
                addr,
                path_str(identity),
                path_str(trust_store),
                "--receive-dir",
                path_str(receive_dir),
            ])
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn authenticated listener");
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

impl Drop for ListenerChild {
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

fn unique_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}
