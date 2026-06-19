use std::env;
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(feature = "webrtc")]
use std::io::Write as _;
#[cfg(feature = "webrtc")]
use std::sync::Arc;

use linkhub_core::{
    new_handshake_nonce, run_authenticated_file_sender,
    run_authenticated_listener_with_receive_dir, run_authenticated_text_sender,
    run_connector_with_receive_dir, run_file_control_sender, run_file_sender,
    run_listener_with_receive_dir, run_text_sender, DeviceAgent, DeviceIdentity, DeviceNode,
    DiscoveryEndpoint, HeartbeatUpdate, LocalDevice, LocalIdentity, MdnsAdvertisement, MdnsRuntime,
    PairingInvitation, PairingSession, SignalingClient, SignalingSupervisor,
    SignalingSupervisorConfig, SignalingSupervisorEvent, TransportKind, TrustStore, TrustedDevice,
};

#[cfg(feature = "webrtc")]
use linkhub_core::net::webrtc_transport::{IceConfig, IceServer};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();

    if args.is_empty() {
        run_demo();
        return Ok(());
    }

    match args.remove(0).as_str() {
        "demo" => {
            run_demo();
            Ok(())
        }
        "listen" => {
            let (addr, local, receive_dir) = parse_endpoint_receive_args(
                &args,
                "listen <bind_addr> <device_id> <device_name> [--receive-dir <dir>]",
            )?;
            run_listener_with_receive_dir(&addr, local, receive_dir)
                .map_err(|err| format!("failed to listen on {addr}: {err}"))
        }
        "connect" => {
            let (addr, local, receive_dir) = parse_endpoint_receive_args(
                &args,
                "connect <peer_addr> <device_id> <device_name> [--receive-dir <dir>]",
            )?;
            run_connector_with_receive_dir(&addr, local, receive_dir)
                .map_err(|err| format!("failed to connect to {addr}: {err}"))
        }
        "send-text" => {
            let (addr, local, text) = parse_send_text_args(&args)?;
            run_text_sender(&addr, local, &text)
                .map_err(|err| format!("failed to send text to {addr}: {err}"))
        }
        "listen-auth" => {
            let (addr, identity_path, trust_store_path, receive_dir) =
                parse_listen_auth_args(&args)?;
            let identity = load_local_identity_arg(&identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let trust_store = TrustStore::load_from_path(&trust_store_path)
                .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
            run_authenticated_listener_with_receive_dir(&addr, identity, trust_store, receive_dir)
                .map_err(|err| format!("failed to listen with auth on {addr}: {err}"))
        }
        "send-text-auth" => {
            let (addr, identity_path, peer_device_id, trust_store_path, text) =
                parse_send_text_auth_args(&args)?;
            let identity = load_local_identity_arg(&identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let trust_store = TrustStore::load_from_path(&trust_store_path)
                .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
            let peer_dh_key_bytes = lookup_peer_dh_key(&trust_store, &peer_device_id)?;
            run_authenticated_text_sender(
                &addr,
                identity,
                &peer_device_id,
                &peer_dh_key_bytes,
                &text,
            )
            .map_err(|err| format!("failed to send authenticated text to {addr}: {err}"))
        }
        "send-file-auth" => {
            let (addr, identity_path, peer_device_id, trust_store_path, path) =
                parse_send_file_auth_args(&args)?;
            let identity = load_local_identity_arg(&identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let trust_store = TrustStore::load_from_path(&trust_store_path)
                .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
            let peer_dh_key_bytes = lookup_peer_dh_key(&trust_store, &peer_device_id)?;
            run_authenticated_file_sender(
                &addr,
                identity,
                &peer_device_id,
                &peer_dh_key_bytes,
                &path,
            )
            .map_err(|err| format!("failed to send authenticated file to {addr}: {err}"))
        }
        "send-file-control" => {
            let (addr, local, path) = parse_send_file_control_args(&args)?;
            run_file_control_sender(&addr, local, &path)
                .map_err(|err| format!("failed to send file control to {addr}: {err}"))
        }
        "send-file" => {
            let (addr, local, path) = parse_send_file_control_args(&args)?;
            run_file_sender(&addr, local, &path)
                .map_err(|err| format!("failed to send file to {addr}: {err}"))
        }
        "advertise-mdns" => {
            let (identity, port, duration) = parse_mdns_advertise_args(&args)?;
            run_mdns_advertise(identity, port, duration)
        }
        "scan-mdns" => {
            let duration =
                parse_optional_seconds_arg(&args, "scan-mdns [seconds]", Duration::from_secs(5))?;
            run_mdns_scan(duration)
        }
        "scan-trusted-mdns" => {
            let (local_name, trust_store_path, duration) = parse_scan_trusted_mdns_args(&args)?;
            run_trusted_mdns_scan(&local_name, &trust_store_path, duration)
        }
        "status" => {
            let (identity_path, trust_store_path) = parse_status_args(&args, "status")?;
            run_status(&identity_path, &trust_store_path)
        }
        "status-html" => {
            let (identity_path, trust_store_path, output_path) = parse_status_html_args(&args)?;
            run_status_html(&identity_path, &trust_store_path, &output_path)
        }
        "signal-listen" => {
            let (ws_url, identity_path) = parse_signal_listen_args(&args)?;
            let identity = load_local_identity_arg(&identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            run_signal_listen(&ws_url, identity)
        }
        "signal-relay" => {
            let (ws_url, identity_path, to_public_key_hex, kind, payload_hex) =
                parse_signal_relay_args(&args)?;
            let identity = load_local_identity_arg(&identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            run_signal_relay(&ws_url, identity, &to_public_key_hex, &kind, &payload_hex)
        }
        "listen-webrtc" => run_listen_webrtc_command(&args),
        "connect-webrtc" => run_connect_webrtc_command(&args),
        "identity" => run_identity_command(&args),
        _ => Err(usage()),
    }
}

fn parse_endpoint_args(args: &[String], shape: &str) -> Result<(String, LocalDevice), String> {
    if args.len() < 3 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    let addr = args[0].clone();
    let device_id = args[1].clone();
    let device_name = args[2..].join(" ");

    Ok((addr, LocalDevice::new(device_id, device_name)))
}

fn parse_endpoint_receive_args(
    args: &[String],
    shape: &str,
) -> Result<(String, LocalDevice, String), String> {
    let (positional, receive_dir) = split_receive_dir_arg(args, shape)?;
    let (addr, local) = parse_endpoint_args(&positional, shape)?;

    Ok((
        addr,
        local,
        receive_dir.unwrap_or_else(|| "received".to_string()),
    ))
}

fn split_receive_dir_arg(
    args: &[String],
    shape: &str,
) -> Result<(Vec<String>, Option<String>), String> {
    let mut positional = Vec::new();
    let mut receive_dir = None;
    let mut index = 0;

    while index < args.len() {
        if args[index] == "--receive-dir" {
            let Some(value) = args.get(index + 1) else {
                return Err(format!("usage: {}", command_usage(shape)));
            };

            if receive_dir.replace(value.clone()).is_some() {
                return Err("--receive-dir can only be provided once".to_string());
            }

            index += 2;
        } else {
            positional.push(args[index].clone());
            index += 1;
        }
    }

    Ok((positional, receive_dir))
}

fn parse_send_text_args(args: &[String]) -> Result<(String, LocalDevice, String), String> {
    if args.len() < 4 {
        return Err(format!(
            "usage: {}",
            command_usage("send-text <peer_addr> <device_id> <device_name> <text>")
        ));
    }

    let addr = args[0].clone();
    let device_id = args[1].clone();
    let device_name = args[2].clone();
    let text = args[3..].join(" ");

    if text.trim().is_empty() {
        return Err("text must not be empty".to_string());
    }

    Ok((addr, LocalDevice::new(device_id, device_name), text))
}

fn parse_listen_auth_args(args: &[String]) -> Result<(String, String, String, String), String> {
    let shape = "listen-auth <bind_addr> <identity_path> <trust_store_path> [--receive-dir <dir>]";
    let (positional, receive_dir) = split_receive_dir_arg(args, shape)?;

    if positional.len() != 3 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    Ok((
        positional[0].clone(),
        positional[1].clone(),
        positional[2].clone(),
        receive_dir.unwrap_or_else(|| "received".to_string()),
    ))
}

fn parse_send_text_auth_args(
    args: &[String],
) -> Result<(String, String, String, String, String), String> {
    if args.len() < 5 {
        return Err(format!(
            "usage: {}",
            command_usage(
                "send-text-auth <peer_addr> <identity_path> <peer_device_id> <trust_store_path> <text>"
            )
        ));
    }

    let text = args[4..].join(" ");

    if text.trim().is_empty() {
        return Err("text must not be empty".to_string());
    }

    Ok((
        args[0].clone(),
        args[1].clone(),
        args[2].clone(),
        args[3].clone(),
        text,
    ))
}

fn parse_send_file_auth_args(
    args: &[String],
) -> Result<(String, String, String, String, String), String> {
    if args.len() < 5 {
        return Err(format!(
            "usage: {}",
            command_usage(
                "send-file-auth <peer_addr> <identity_path> <peer_device_id> <trust_store_path> <file_path>"
            )
        ));
    }

    Ok((
        args[0].clone(),
        args[1].clone(),
        args[2].clone(),
        args[3].clone(),
        args[4..].join(" "),
    ))
}

fn parse_send_file_control_args(args: &[String]) -> Result<(String, LocalDevice, String), String> {
    if args.len() < 4 {
        return Err(format!(
            "usage: {}",
            command_usage("send-file-control <peer_addr> <device_id> <device_name> <file_path>")
        ));
    }

    Ok((
        args[0].clone(),
        LocalDevice::new(args[1].clone(), args[2].clone()),
        args[3..].join(" "),
    ))
}

fn parse_mdns_advertise_args(args: &[String]) -> Result<(DeviceIdentity, u16, Duration), String> {
    let shape = "advertise-mdns <device_id> <device_name> <public_key> <tcp_port> [seconds]";

    if args.len() < 4 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    let has_duration = args.len() >= 5
        && args
            .last()
            .is_some_and(|value| value.parse::<u64>().is_ok())
        && args
            .get(args.len() - 2)
            .is_some_and(|value| value.parse::<u16>().is_ok());
    let duration = if has_duration {
        args.last()
            .unwrap()
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| format!("usage: {}", command_usage(shape)))?
    } else {
        Duration::from_secs(30)
    };
    let port_index = if has_duration {
        args.len() - 2
    } else {
        args.len() - 1
    };
    let public_key_index = port_index
        .checked_sub(1)
        .ok_or_else(|| format!("usage: {}", command_usage(shape)))?;

    if public_key_index <= 1 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    let port = args[port_index]
        .parse::<u16>()
        .map_err(|_| "tcp_port must be a valid port number".to_string())?;

    Ok((
        DeviceIdentity::new(
            args[0].clone(),
            args[1..public_key_index].join(" "),
            args[public_key_index].clone(),
            "00".repeat(32),
        ),
        port,
        duration,
    ))
}

fn parse_optional_seconds_arg(
    args: &[String],
    shape: &str,
    default: Duration,
) -> Result<Duration, String> {
    match args {
        [] => Ok(default),
        [seconds] => seconds
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|_| format!("usage: {}", command_usage(shape))),
        _ => Err(format!("usage: {}", command_usage(shape))),
    }
}

fn parse_scan_trusted_mdns_args(args: &[String]) -> Result<(String, String, Duration), String> {
    if args.len() < 2 || args.len() > 3 {
        return Err(format!(
            "usage: {}",
            command_usage("scan-trusted-mdns <local_name> <trust_store_path> [seconds]")
        ));
    }

    let duration = parse_optional_seconds_arg(
        &args[2..],
        "scan-trusted-mdns <local_name> <trust_store_path> [seconds]",
        Duration::from_secs(5),
    )?;

    Ok((args[0].clone(), args[1].clone(), duration))
}

fn parse_status_args(args: &[String], command: &str) -> Result<(String, String), String> {
    if args.len() != 2 {
        return Err(format!(
            "usage: {}",
            command_usage(&format!("{command} <identity_path> <trust_store_path>"))
        ));
    }

    Ok((args[0].clone(), args[1].clone()))
}

fn parse_status_html_args(args: &[String]) -> Result<(String, String, String), String> {
    if args.len() != 3 {
        return Err(format!(
            "usage: {}",
            command_usage("status-html <identity_path> <trust_store_path> <output_html_path>")
        ));
    }

    Ok((args[0].clone(), args[1].clone(), args[2].clone()))
}

fn usage() -> String {
    [
        "usage:",
        &command_usage("demo"),
        &command_usage("identity init <identity_path> <device_name>"),
        &command_usage("identity secure-init <secure_identity_path> <device_name>"),
        &command_usage("identity show <identity_path>"),
        &command_usage("identity secure-show <secure_identity_path>"),
        &command_usage("identity pairing-payload <identity_path> [ttl_seconds]"),
        &command_usage("identity inspect-pairing <payload>"),
        &command_usage("identity pairing-code <identity_path> <payload>"),
        &command_usage(
            "identity trust-pairing <identity_path> <payload> <confirmation_code> <trust_store_path>",
        ),
        &command_usage("identity trust <identity_path> <trust_store_path>"),
        &command_usage("identity handshake-nonce"),
        &command_usage("identity sign-handshake <identity_path> <peer_device_id> <nonce>"),
        &command_usage(
            "identity verify-handshake <device_id> <device_name> <public_key> <peer_device_id> <nonce> <signature_hex>",
        ),
        &command_usage("listen <bind_addr> <device_id> <device_name> [--receive-dir <dir>]"),
        &command_usage("connect <peer_addr> <device_id> <device_name> [--receive-dir <dir>]"),
        &command_usage("send-text <peer_addr> <device_id> <device_name> <text>"),
        &command_usage(
            "listen-auth <bind_addr> <identity_path> <trust_store_path> [--receive-dir <dir>]",
        ),
        &command_usage(
            "send-text-auth <peer_addr> <identity_path> <peer_device_id> <trust_store_path> <text>",
        ),
        &command_usage(
            "send-file-auth <peer_addr> <identity_path> <peer_device_id> <trust_store_path> <file_path>",
        ),
        &command_usage("send-file-control <peer_addr> <device_id> <device_name> <file_path>"),
        &command_usage("send-file <peer_addr> <device_id> <device_name> <file_path>"),
        &command_usage(
            "advertise-mdns <device_id> <device_name> <public_key> <tcp_port> [seconds]",
        ),
        &command_usage("scan-mdns [seconds]"),
        &command_usage("scan-trusted-mdns <local_name> <trust_store_path> [seconds]"),
        &command_usage("status <identity_path> <trust_store_path>"),
        &command_usage("status-html <identity_path> <trust_store_path> <output_html_path>"),
        &command_usage("signal-listen <ws_url> <identity_path>"),
        &command_usage(
            "signal-relay <ws_url> <identity_path> <to_public_key_hex> <kind> <payload_hex>",
        ),
        &command_usage(
            "listen-webrtc <ws_url> <identity_path> <trust_store_path> [--receive-dir <dir>] [--ice <url>...]",
        ),
        &command_usage(
            "connect-webrtc <ws_url> <identity_path> <peer_device_id> <trust_store_path> <file_path> [--ice <url>...]",
        ),
    ]
    .join("\n")
}

fn command_usage(shape: &str) -> String {
    format!("  linkhub-core-prototype {shape}")
}

fn lookup_peer_dh_key(trust_store: &TrustStore, peer_device_id: &str) -> Result<[u8; 32], String> {
    let trusted = trust_store
        .trusted_device(peer_device_id)
        .ok_or_else(|| format!("peer device not found in trust store: {peer_device_id}"))?;
    let dh_hex = trusted.identity().dh_public_key();
    let dh_bytes =
        linkhub_core::decode_hex(dh_hex).map_err(|err| format!("invalid peer dh key: {err}"))?;
    dh_bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("peer dh key must be 32 bytes, got {}", bytes.len()))
}

fn load_local_identity_arg(path: &str) -> Result<LocalIdentity, std::io::Error> {
    match path.strip_prefix("secure:") {
        Some(secure_path) => LocalIdentity::load_from_secure_path(secure_path),
        None => LocalIdentity::load_from_path(path),
    }
}

fn run_identity_command(args: &[String]) -> Result<(), String> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err(format!(
            "usage:\n{}\n{}",
            command_usage("identity init <identity_path> <device_name>"),
            command_usage("identity show <identity_path>")
        ));
    };

    match subcommand {
        "init" => {
            if args.len() < 3 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity init <identity_path> <device_name>")
                ));
            }

            let path = &args[1];
            let device_name = args[2..].join(" ");
            let identity = LocalIdentity::load_or_generate(path, device_name, SystemTime::now())
                .map_err(|err| format!("failed to initialize identity at {path}: {err}"))?;
            print_local_identity(&identity);
            Ok(())
        }
        "secure-init" => {
            if args.len() < 3 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity secure-init <secure_identity_path> <device_name>")
                ));
            }

            let path = &args[1];
            let device_name = args[2..].join(" ");
            let identity =
                LocalIdentity::load_or_generate_secure(path, device_name, SystemTime::now())
                    .map_err(|err| {
                        format!("failed to initialize secure identity at {path}: {err}")
                    })?;
            print_local_identity(&identity);
            Ok(())
        }
        "show" => {
            if args.len() != 2 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity show <identity_path>")
                ));
            }

            let path = &args[1];
            let identity = load_local_identity_arg(path)
                .map_err(|err| format!("failed to load identity at {path}: {err}"))?;
            print_local_identity(&identity);
            Ok(())
        }
        "secure-show" => {
            if args.len() != 2 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity secure-show <secure_identity_path>")
                ));
            }

            let path = &args[1];
            let identity = LocalIdentity::load_from_secure_path(path)
                .map_err(|err| format!("failed to load secure identity at {path}: {err}"))?;
            print_local_identity(&identity);
            Ok(())
        }
        "pairing-payload" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity pairing-payload <identity_path> [ttl_seconds]")
                ));
            }

            let path = &args[1];
            let ttl = parse_optional_seconds_arg(
                &args[2..],
                "identity pairing-payload <identity_path> [ttl_seconds]",
                Duration::from_secs(120),
            )?;
            if ttl.is_zero() {
                return Err("ttl_seconds must be greater than zero".to_string());
            }

            let identity = load_local_identity_arg(path)
                .map_err(|err| format!("failed to load identity at {path}: {err}"))?;
            let invitation =
                PairingInvitation::new(identity.identity().clone(), SystemTime::now(), ttl);
            println!("{}", invitation.to_payload());
            println!("fingerprint={}", invitation.identity().fingerprint());
            println!("confirmation_ttl_seconds={}", invitation.ttl().as_secs());
            Ok(())
        }
        "inspect-pairing" => {
            if args.len() != 2 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity inspect-pairing <payload>")
                ));
            }

            let invitation = PairingInvitation::from_payload(&args[1], SystemTime::now())
                .map_err(|err| format!("failed to parse pairing payload: {err}"))?;
            println!("device_id={}", invitation.identity().device_id());
            println!("device_name={}", invitation.identity().device_name());
            println!("fingerprint={}", invitation.identity().fingerprint());
            println!("public_key={}", invitation.identity().public_key());
            println!("ttl_seconds={}", invitation.ttl().as_secs());
            Ok(())
        }
        "pairing-code" => {
            if args.len() != 3 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity pairing-code <identity_path> <payload>")
                ));
            }

            let identity_path = &args[1];
            let identity = load_local_identity_arg(identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let invitation = PairingInvitation::from_payload(&args[2], SystemTime::now())
                .map_err(|err| format!("failed to parse pairing payload: {err}"))?;
            let session = PairingSession::new(identity.identity().clone(), invitation);

            println!("peer_device_id={}", session.peer_identity().device_id());
            println!("peer_device_name={}", session.peer_identity().device_name());
            println!("peer_fingerprint={}", session.peer_identity().fingerprint());
            println!("confirmation_code={}", session.confirmation_code());
            Ok(())
        }
        "trust-pairing" => {
            if args.len() != 5 {
                return Err(format!(
                    "usage: {}",
                    command_usage(
                        "identity trust-pairing <identity_path> <payload> <confirmation_code> <trust_store_path>",
                    )
                ));
            }

            let identity_path = &args[1];
            let payload = &args[2];
            let confirmation_code = &args[3];
            let trust_store_path = &args[4];
            let identity = load_local_identity_arg(identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let invitation = PairingInvitation::from_payload(payload, SystemTime::now())
                .map_err(|err| format!("failed to parse pairing payload: {err}"))?;
            let session = PairingSession::new(identity.identity().clone(), invitation);
            let trusted = session
                .confirm(confirmation_code, SystemTime::now(), SystemTime::now())
                .map_err(|err| format!("failed to confirm pairing: {err}"))?;
            let trusted_device_id = trusted.device_id().to_string();
            let trusted_fingerprint = trusted.fingerprint().to_string();
            let mut trust_store = TrustStore::load_from_path(trust_store_path)
                .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
            trust_store.trust(trusted);
            trust_store
                .save_to_path(trust_store_path)
                .map_err(|err| format!("failed to save trust store {trust_store_path}: {err}"))?;

            println!("trusted_device={trusted_device_id} fingerprint={trusted_fingerprint}");
            Ok(())
        }
        "trust" => {
            if args.len() != 3 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity trust <identity_path> <trust_store_path>")
                ));
            }

            let identity_path = &args[1];
            let trust_store_path = &args[2];
            let identity = load_local_identity_arg(identity_path)
                .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
            let mut trust_store = TrustStore::load_from_path(trust_store_path)
                .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
            trust_store.trust(TrustedDevice::new(
                identity.identity().clone(),
                SystemTime::now(),
            ));
            trust_store
                .save_to_path(trust_store_path)
                .map_err(|err| format!("failed to save trust store {trust_store_path}: {err}"))?;

            println!(
                "trusted_device={} fingerprint={}",
                identity.device_id(),
                identity.identity().fingerprint()
            );
            Ok(())
        }
        "handshake-nonce" => {
            if args.len() != 1 {
                return Err(format!(
                    "usage: {}",
                    command_usage("identity handshake-nonce")
                ));
            }

            println!("{}", new_handshake_nonce());
            Ok(())
        }
        "sign-handshake" => {
            if args.len() != 4 {
                return Err(format!(
                    "usage: {}",
                    command_usage(
                        "identity sign-handshake <identity_path> <peer_device_id> <nonce>"
                    )
                ));
            }

            let path = &args[1];
            let identity = load_local_identity_arg(path)
                .map_err(|err| format!("failed to load identity at {path}: {err}"))?;
            let signature = identity
                .sign_handshake_challenge(&args[2], &args[3])
                .map_err(|err| format!("failed to sign handshake challenge: {err}"))?;

            println!("{signature}");
            Ok(())
        }
        "verify-handshake" => {
            if args.len() < 7 {
                return Err(format!(
                    "usage: {}",
                    command_usage(
                        "identity verify-handshake <device_id> <device_name> <public_key> <peer_device_id> <nonce> <signature_hex>",
                    )
                ));
            }

            let identity = DeviceIdentity::new(
                args[1].clone(),
                args[2].clone(),
                args[3].clone(),
                "00".repeat(32),
            );
            let verified = identity
                .verify_handshake_signature(&args[4], &args[5], &args[6])
                .map_err(|err| format!("failed to verify handshake signature: {err}"))?;

            println!("verified={verified}");
            Ok(())
        }
        _ => Err(format!(
            "usage:\n{}\n{}\n{}\n{}",
            command_usage("identity init <identity_path> <device_name>"),
            command_usage("identity secure-init <secure_identity_path> <device_name>"),
            command_usage("identity show <identity_path>"),
            command_usage("identity secure-show <secure_identity_path>")
        )),
    }
}

fn print_local_identity(identity: &LocalIdentity) {
    println!("device_id={}", identity.device_id());
    println!("device_name={}", identity.device_name());
    println!("fingerprint={}", identity.identity().fingerprint());
    println!("public_key={}", identity.public_key());
}

fn run_mdns_advertise(
    identity: DeviceIdentity,
    port: u16,
    duration: Duration,
) -> Result<(), String> {
    let advertisement = MdnsAdvertisement::from_identity(&identity, port);
    let runtime = MdnsRuntime::new()?;
    let registration = runtime.register(&advertisement)?;

    println!(
        "Advertising {} as {} for {} seconds",
        advertisement.service_name(),
        registration.fullname(),
        duration.as_secs()
    );
    println!("TXT {:?}", advertisement.txt_records());

    thread::sleep(duration);

    runtime.unregister(&registration)?;
    runtime.shutdown()?;
    println!("Stopped advertising {}", registration.fullname());

    Ok(())
}

fn run_mdns_scan(duration: Duration) -> Result<(), String> {
    let runtime = MdnsRuntime::new()?;

    println!(
        "Scanning {} for {} seconds",
        linkhub_core::LINKHUB_MDNS_SERVICE,
        duration.as_secs()
    );
    let endpoints = runtime.browse_for(duration)?;
    runtime.shutdown()?;

    if endpoints.is_empty() {
        println!("No LinkHub devices discovered");
    } else {
        for endpoint in endpoints {
            println!(
                "- {} ({}) addr={} transport={}",
                endpoint.device_name(),
                endpoint.device_id(),
                endpoint.addr(),
                endpoint.transport()
            );
        }
    }

    Ok(())
}

fn run_trusted_mdns_scan(
    local_name: &str,
    trust_store_path: &str,
    duration: Duration,
) -> Result<(), String> {
    let trust_store = TrustStore::load_from_path(trust_store_path)
        .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
    let runtime = MdnsRuntime::new()?;

    println!(
        "Scanning trusted LinkHub devices from {} for {} seconds",
        trust_store_path,
        duration.as_secs()
    );
    let endpoints = runtime.browse_for(duration)?;
    runtime.shutdown()?;

    let agent = agent_from_trusted_mdns(local_name, &trust_store, &endpoints, Instant::now());
    agent.print_status();

    Ok(())
}

fn run_status(identity_path: &str, trust_store_path: &str) -> Result<(), String> {
    let identity = load_local_identity_arg(identity_path)
        .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
    let trust_store = TrustStore::load_from_path(trust_store_path)
        .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;

    print!("{}", render_status_text(&identity, &trust_store));
    Ok(())
}

fn run_status_html(
    identity_path: &str,
    trust_store_path: &str,
    output_path: impl AsRef<Path>,
) -> Result<(), String> {
    let identity = load_local_identity_arg(identity_path)
        .map_err(|err| format!("failed to load identity {identity_path}: {err}"))?;
    let trust_store = TrustStore::load_from_path(trust_store_path)
        .map_err(|err| format!("failed to load trust store {trust_store_path}: {err}"))?;
    let output_path = output_path.as_ref();

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create status page directory: {err}"))?;
    }

    fs::write(output_path, render_status_html(&identity, &trust_store)).map_err(|err| {
        format!(
            "failed to write status page {}: {err}",
            output_path.display()
        )
    })?;
    println!("status_page={}", output_path.display());
    Ok(())
}

fn parse_signal_listen_args(args: &[String]) -> Result<(String, String), String> {
    if args.len() != 2 {
        return Err(format!(
            "usage: {}",
            command_usage("signal-listen <ws_url> <identity_path>")
        ));
    }

    Ok((args[0].clone(), args[1].clone()))
}

fn parse_signal_relay_args(
    args: &[String],
) -> Result<(String, String, String, String, String), String> {
    if args.len() != 5 {
        return Err(format!(
            "usage: {}",
            command_usage(
                "signal-relay <ws_url> <identity_path> <to_public_key_hex> <kind> <payload_hex>"
            )
        ));
    }

    Ok((
        args[0].clone(),
        args[1].clone(),
        args[2].clone(),
        args[3].clone(),
        args[4].clone(),
    ))
}

#[cfg(feature = "webrtc")]
struct ListenWebRtcArgs {
    ws_url: String,
    identity_path: String,
    trust_store_path: String,
    receive_dir: String,
    ice: IceConfig,
}

#[cfg(feature = "webrtc")]
struct ConnectWebRtcArgs {
    ws_url: String,
    identity_path: String,
    peer_device_id: String,
    trust_store_path: String,
    path: String,
    ice: IceConfig,
}

#[cfg(feature = "webrtc")]
struct WebRtcOptions {
    positional: Vec<String>,
    receive_dir: Option<String>,
    ice_urls: Vec<String>,
    turn_username: Option<String>,
    turn_credential: Option<String>,
    relay_only: bool,
}

#[cfg(feature = "webrtc")]
impl WebRtcOptions {
    /// Build the [`IceConfig`] from the parsed flags: `--ice` URLs starting with
    /// `turn:`/`turns:` become TURN servers (using `--turn-username` /
    /// `--turn-credential`), the rest STUN servers; `--relay-only` forces relay.
    fn to_ice_config(&self) -> IceConfig {
        let username = self.turn_username.clone().unwrap_or_default();
        let credential = self.turn_credential.clone().unwrap_or_default();
        let servers = self
            .ice_urls
            .iter()
            .filter(|url| !url.is_empty())
            .map(|url| {
                if url.starts_with("turn:") || url.starts_with("turns:") {
                    IceServer::turn(url.clone(), username.clone(), credential.clone())
                } else {
                    IceServer::stun(url.clone())
                }
            })
            .collect();
        IceConfig {
            servers,
            force_relay: self.relay_only,
        }
    }
}

#[cfg(feature = "webrtc")]
fn parse_listen_webrtc_args(args: &[String]) -> Result<ListenWebRtcArgs, String> {
    let shape =
        "listen-webrtc <ws_url> <identity_path> <trust_store_path> [--receive-dir <dir>] [--ice <url>...] [--turn-username <u>] [--turn-credential <p>] [--relay-only]";
    let options = split_webrtc_options(args, shape, true)?;

    if options.positional.len() != 3 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    let ice = options.to_ice_config();
    Ok(ListenWebRtcArgs {
        ws_url: options.positional[0].clone(),
        identity_path: options.positional[1].clone(),
        trust_store_path: options.positional[2].clone(),
        receive_dir: options
            .receive_dir
            .clone()
            .unwrap_or_else(|| "received".to_string()),
        ice,
    })
}

#[cfg(feature = "webrtc")]
fn parse_connect_webrtc_args(args: &[String]) -> Result<ConnectWebRtcArgs, String> {
    let shape =
        "connect-webrtc <ws_url> <identity_path> <peer_device_id> <trust_store_path> <file_path> [--ice <url>...] [--turn-username <u>] [--turn-credential <p>] [--relay-only]";
    let options = split_webrtc_options(args, shape, false)?;

    if options.receive_dir.is_some() || options.positional.len() < 5 {
        return Err(format!("usage: {}", command_usage(shape)));
    }

    let ice = options.to_ice_config();
    Ok(ConnectWebRtcArgs {
        ws_url: options.positional[0].clone(),
        identity_path: options.positional[1].clone(),
        peer_device_id: options.positional[2].clone(),
        trust_store_path: options.positional[3].clone(),
        path: options.positional[4..].join(" "),
        ice,
    })
}

#[cfg(feature = "webrtc")]
fn split_webrtc_options(
    args: &[String],
    shape: &str,
    allow_receive_dir: bool,
) -> Result<WebRtcOptions, String> {
    let mut positional = Vec::new();
    let mut receive_dir = None;
    let mut ice_urls = Vec::new();
    let mut turn_username = None;
    let mut turn_credential = None;
    let mut relay_only = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--receive-dir" if allow_receive_dir => {
                let Some(value) = args.get(index + 1) else {
                    return Err(format!("usage: {}", command_usage(shape)));
                };

                if receive_dir.replace(value.clone()).is_some() {
                    return Err("--receive-dir can only be provided once".to_string());
                }

                index += 2;
            }
            "--receive-dir" => return Err(format!("usage: {}", command_usage(shape))),
            "--ice" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(format!("usage: {}", command_usage(shape)));
                };

                ice_urls.push(value.clone());
                index += 2;
            }
            "--turn-username" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(format!("usage: {}", command_usage(shape)));
                };
                if turn_username.replace(value.clone()).is_some() {
                    return Err("--turn-username can only be provided once".to_string());
                }
                index += 2;
            }
            "--turn-credential" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(format!("usage: {}", command_usage(shape)));
                };
                if turn_credential.replace(value.clone()).is_some() {
                    return Err("--turn-credential can only be provided once".to_string());
                }
                index += 2;
            }
            "--relay-only" => {
                relay_only = true;
                index += 1;
            }
            value => {
                positional.push(value.to_string());
                index += 1;
            }
        }
    }

    Ok(WebRtcOptions {
        positional,
        receive_dir,
        ice_urls,
        turn_username,
        turn_credential,
        relay_only,
    })
}

fn run_signal_listen(ws_url: &str, identity: LocalIdentity) -> Result<(), String> {
    let supervisor =
        SignalingSupervisor::start(SignalingSupervisorConfig::new(ws_url, identity.clone()));
    let events = supervisor.events();

    println!(
        "Signaling supervisor starting for device_id={} public_key={}",
        identity.device_id(),
        identity.public_key()
    );
    println!("Waiting for signaling deliveries with automatic reconnect (Ctrl-C to stop)...");

    while let Ok(event) = events.recv() {
        match event {
            SignalingSupervisorEvent::Connected {
                device_id,
                public_key_hex,
            } => {
                println!("Signaling: present as device_id={device_id} public_key={public_key_hex}");
            }
            SignalingSupervisorEvent::Delivery(delivery) => {
                println!(
                    "Signaling delivery from device_id={} public_key={} session={} kind={} payload_hex={}",
                    delivery.from_device_id,
                    delivery.from_public_key_hex,
                    delivery.session_id,
                    delivery.kind,
                    delivery.payload_hex
                );
            }
            SignalingSupervisorEvent::ServerError(reason) => {
                println!("Signaling server error: {reason}");
            }
            SignalingSupervisorEvent::Disconnected(reason) => {
                eprintln!("Signaling disconnected; will retry: {reason}");
            }
            SignalingSupervisorEvent::Stopped => {
                break;
            }
        }
    }

    Ok(())
}

fn run_signal_relay(
    ws_url: &str,
    identity: LocalIdentity,
    to_public_key_hex: &str,
    kind: &str,
    payload_hex: &str,
) -> Result<(), String> {
    let mut client = SignalingClient::connect(ws_url, &identity)
        .map_err(|err| format!("failed to connect to signaling server {ws_url}: {err}"))?;

    let session_id = format!(
        "{}-{}",
        identity.device_id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    client
        .send_signaling(to_public_key_hex, &session_id, kind, payload_hex)
        .map_err(|err| format!("failed to relay signaling: {err}"))?;

    println!("Signaling relayed to {to_public_key_hex} (session {session_id}, kind {kind})");
    Ok(())
}

#[cfg(not(feature = "webrtc"))]
fn run_listen_webrtc_command(_args: &[String]) -> Result<(), String> {
    Err("listen-webrtc requires building linkhub-cli with --features webrtc".to_string())
}

#[cfg(not(feature = "webrtc"))]
fn run_connect_webrtc_command(_args: &[String]) -> Result<(), String> {
    Err("connect-webrtc requires building linkhub-cli with --features webrtc".to_string())
}

#[cfg(feature = "webrtc")]
fn run_listen_webrtc_command(args: &[String]) -> Result<(), String> {
    let parsed = parse_listen_webrtc_args(args)?;
    let identity = load_local_identity_arg(&parsed.identity_path)
        .map_err(|err| format!("failed to load identity {}: {err}", parsed.identity_path))?;
    let trust_store = Arc::new(
        TrustStore::load_from_path(&parsed.trust_store_path).map_err(|err| {
            format!(
                "failed to load trust store {}: {err}",
                parsed.trust_store_path
            )
        })?,
    );

    run_listen_webrtc(
        &parsed.ws_url,
        identity,
        trust_store,
        parsed.receive_dir,
        parsed.ice,
    )
}

#[cfg(feature = "webrtc")]
fn run_connect_webrtc_command(args: &[String]) -> Result<(), String> {
    let parsed = parse_connect_webrtc_args(args)?;
    let identity = load_local_identity_arg(&parsed.identity_path)
        .map_err(|err| format!("failed to load identity {}: {err}", parsed.identity_path))?;
    let trust_store = TrustStore::load_from_path(&parsed.trust_store_path).map_err(|err| {
        format!(
            "failed to load trust store {}: {err}",
            parsed.trust_store_path
        )
    })?;
    let peer_identity = lookup_peer_identity(&trust_store, &parsed.peer_device_id)?;

    run_connect_webrtc(
        &parsed.ws_url,
        identity,
        peer_identity,
        parsed.path,
        parsed.ice,
    )
}

#[cfg(feature = "webrtc")]
fn run_connect_webrtc(
    ws_url: &str,
    identity: LocalIdentity,
    peer_identity: DeviceIdentity,
    path: String,
    ice: IceConfig,
) -> Result<(), String> {
    println!(
        "WebRTC initiator present as device_id={} target_device_id={}; establishing...",
        identity.device_id(),
        peer_identity.device_id()
    );
    let _ = std::io::stdout().flush();

    linkhub_core::net::webrtc_session::send_file_over_webrtc(
        ws_url,
        &identity,
        &peer_identity,
        ice,
        &path,
    )
    .map_err(|err| format!("failed to send authenticated file over WebRTC: {err}"))?;

    println!(
        "WebRTC file sent to {} and acknowledged",
        peer_identity.device_id()
    );
    Ok(())
}

#[cfg(feature = "webrtc")]
fn run_listen_webrtc(
    ws_url: &str,
    identity: LocalIdentity,
    trust_store: Arc<TrustStore>,
    receive_dir: String,
    ice: IceConfig,
) -> Result<(), String> {
    println!(
        "WebRTC listener present as device_id={} public_key={}",
        identity.device_id(),
        identity.public_key()
    );
    println!("Waiting for a trusted WebRTC offer...");
    let _ = std::io::stdout().flush();

    linkhub_core::net::webrtc_session::receive_file_over_webrtc(
        ws_url,
        identity,
        trust_store,
        &receive_dir,
        ice,
        None,
    )
    .map_err(|err| format!("authenticated WebRTC responder failed: {err}"))?;

    println!("WebRTC file received into {receive_dir}");
    Ok(())
}

#[cfg(feature = "webrtc")]
fn lookup_peer_identity(
    trust_store: &TrustStore,
    peer_device_id: &str,
) -> Result<DeviceIdentity, String> {
    trust_store
        .trusted_device(peer_device_id)
        .map(|trusted| trusted.identity().clone())
        .ok_or_else(|| format!("peer device not found in trust store: {peer_device_id}"))
}

fn render_status_text(identity: &LocalIdentity, trust_store: &TrustStore) -> String {
    let mut lines = vec![
        "LinkHub Status".to_string(),
        format!("local_device_id={}", identity.device_id()),
        format!("local_device_name={}", identity.device_name()),
        format!("local_fingerprint={}", identity.identity().fingerprint()),
        format!(
            "trusted_device_count={}",
            trust_store.trusted_devices().len()
        ),
    ];

    for trusted in trust_store.trusted_devices() {
        lines.push(format!(
            "trusted_device={} name={} fingerprint={} paired_at_unix={}",
            trusted.device_id(),
            trusted.device_name(),
            trusted.fingerprint(),
            unix_seconds(trusted.paired_at())
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn render_status_html(identity: &LocalIdentity, trust_store: &TrustStore) -> String {
    let trusted_rows = trust_store
        .trusted_devices()
        .into_iter()
        .map(|trusted| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(trusted.device_name()),
                html_escape(trusted.device_id()),
                html_escape(trusted.fingerprint()),
                unix_seconds(trusted.paired_at())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let trusted_rows = if trusted_rows.is_empty() {
        "<tr><td colspan=\"4\" class=\"empty\">No trusted devices yet</td></tr>".to_string()
    } else {
        trusted_rows
    };

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>LinkHub Status</title>
  <style>
    :root {{
      color-scheme: light;
      font-family: "Segoe UI", Arial, sans-serif;
      color: #1f2937;
      background: #f6f8fb;
    }}
    body {{
      margin: 0;
    }}
    main {{
      max-width: 960px;
      margin: 0 auto;
      padding: 32px 20px;
    }}
    h1 {{
      margin: 0 0 20px;
      font-size: 28px;
      font-weight: 650;
    }}
    h2 {{
      margin: 28px 0 12px;
      font-size: 18px;
    }}
    .summary {{
      display: grid;
      gap: 12px;
      grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
    }}
    .metric {{
      background: #ffffff;
      border: 1px solid #d8dee8;
      border-radius: 8px;
      padding: 14px 16px;
    }}
    .label {{
      color: #68758a;
      font-size: 12px;
      text-transform: uppercase;
    }}
    .value {{
      margin-top: 6px;
      font-size: 17px;
      overflow-wrap: anywhere;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      background: #ffffff;
      border: 1px solid #d8dee8;
      border-radius: 8px;
      overflow: hidden;
    }}
    th, td {{
      padding: 11px 12px;
      border-bottom: 1px solid #e5eaf2;
      text-align: left;
      vertical-align: top;
      overflow-wrap: anywhere;
    }}
    th {{
      color: #526074;
      font-size: 12px;
      background: #f0f4f9;
      text-transform: uppercase;
    }}
    tr:last-child td {{
      border-bottom: 0;
    }}
    .empty {{
      color: #68758a;
      text-align: center;
    }}
  </style>
</head>
<body>
  <main>
    <h1>LinkHub Status</h1>
    <section class="summary" aria-label="Local device status">
      <div class="metric"><div class="label">Device Name</div><div class="value">{}</div></div>
      <div class="metric"><div class="label">Device ID</div><div class="value">{}</div></div>
      <div class="metric"><div class="label">Fingerprint</div><div class="value">{}</div></div>
      <div class="metric"><div class="label">Trusted Devices</div><div class="value">{}</div></div>
    </section>
    <h2>Trusted Devices</h2>
    <table>
      <thead>
        <tr><th>Name</th><th>Device ID</th><th>Fingerprint</th><th>Paired At</th></tr>
      </thead>
      <tbody>
        {}
      </tbody>
    </table>
  </main>
</body>
</html>
"#,
        html_escape(identity.device_name()),
        html_escape(identity.device_id()),
        html_escape(&identity.identity().fingerprint()),
        trust_store.trusted_devices().len(),
        trusted_rows
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn agent_from_trusted_mdns(
    local_name: &str,
    trust_store: &TrustStore,
    endpoints: &[DiscoveryEndpoint],
    now: Instant,
) -> DeviceAgent {
    let mut agent = DeviceAgent::new(local_name);

    for trusted in trust_store.trusted_devices() {
        agent.trust_paired_device(trusted);
    }

    for endpoint in endpoints {
        agent.observe_discovery(endpoint, now);
    }

    agent
}

fn run_demo() {
    let start = Instant::now();
    let pairing_start = SystemTime::now();
    let mut agent = DeviceAgent::new("Windows-PC");
    let local_identity = DeviceIdentity::new(
        "windows-001",
        "Windows PC",
        "windows-public-key",
        "00".repeat(32),
    );
    let android_identity = DeviceIdentity::new(
        "android-001",
        "Android Phone",
        "android-public-key",
        "00".repeat(32),
    );
    let android_advertisement = MdnsAdvertisement::from_identity(&android_identity, 8787);
    let pairing_session = PairingSession::new(
        local_identity,
        PairingInvitation::new(android_identity, pairing_start, Duration::from_secs(120)),
    );
    let pairing_code = pairing_session.confirmation_code();
    let trusted_android = pairing_session
        .confirm(&pairing_code, pairing_start, SystemTime::now())
        .expect("demo pairing code should match");
    let mut trust_store = TrustStore::new();
    trust_store.trust(trusted_android.clone());
    let demo_store_path = env::temp_dir().join("linkhub-demo-trust-store.txt");
    trust_store
        .save_to_path(&demo_store_path)
        .expect("demo trust store should be writable");
    let trust_store =
        TrustStore::load_from_path(&demo_store_path).expect("demo trust store should be readable");
    let _ = fs::remove_file(&demo_store_path);

    agent.trust_paired_device(&trusted_android);
    agent.trust_device(DeviceNode::new("ipad-001", "iPad Pro"));
    agent.trust_device(DeviceNode::new("mac-001", "MacBook"));

    println!("== Pair Android through short code {} ==", pairing_code);
    println!(
        "Trusted {} fingerprint={}",
        trusted_android.device_name(),
        trusted_android.fingerprint()
    );
    println!(
        "Trust store now tracks {} paired device(s)",
        trust_store.trusted_devices().len()
    );
    println!();

    println!("== Initial discovery ==");
    agent.print_status();

    println!("== LAN discovery advertises Android TCP endpoint ==");
    println!(
        "mDNS service={} instance={} txt={:?}",
        android_advertisement.service_name(),
        android_advertisement.instance_name(),
        android_advertisement.txt_records()
    );
    let parsed_advertisement =
        MdnsAdvertisement::from_txt_records(&android_advertisement.txt_records())
            .expect("demo mDNS TXT should parse");
    let android_lan = parsed_advertisement.to_endpoint(IpAddr::from([127, 0, 0, 1]), start);
    agent.observe_discovery(&android_lan, start);
    agent.print_status();

    println!("== Heartbeats arrive over multiple transports ==");
    agent.receive_heartbeat(
        "android-001",
        HeartbeatUpdate {
            transport: TransportKind::LanQuic,
            latency_ms: 12,
            bandwidth_score: 450,
            battery_cost: 10,
            metered_cost: 0,
        },
        start,
    );
    agent.receive_heartbeat(
        "android-001",
        HeartbeatUpdate {
            transport: TransportKind::BleControl,
            latency_ms: 80,
            bandwidth_score: 20,
            battery_cost: 2,
            metered_cost: 0,
        },
        start,
    );
    agent.receive_heartbeat(
        "ipad-001",
        HeartbeatUpdate {
            transport: TransportKind::CloudRelay,
            latency_ms: 140,
            bandwidth_score: 80,
            battery_cost: 20,
            metered_cost: 35,
        },
        start,
    );
    agent.receive_heartbeat(
        "mac-001",
        HeartbeatUpdate {
            transport: TransportKind::WebRtc,
            latency_ms: 45,
            bandwidth_score: 240,
            battery_cost: 18,
            metered_cost: 0,
        },
        start,
    );
    agent.print_status();

    println!("== Wi-Fi path gets stale; Android falls back to BLE control ==");
    agent.receive_heartbeat(
        "android-001",
        HeartbeatUpdate {
            transport: TransportKind::BleControl,
            latency_ms: 85,
            bandwidth_score: 20,
            battery_cost: 2,
            metered_cost: 0,
        },
        start + Duration::from_secs(9),
    );
    agent.tick(start + Duration::from_secs(9));
    agent.print_status();

    println!("== Better LAN route returns; Android upgrades automatically ==");
    agent.receive_heartbeat(
        "android-001",
        HeartbeatUpdate {
            transport: TransportKind::LanTcp,
            latency_ms: 18,
            bandwidth_score: 350,
            battery_cost: 10,
            metered_cost: 0,
        },
        start + Duration::from_secs(11),
    );
    agent.tick(start + Duration::from_secs(11));
    agent.print_status();

    println!("== All routes to iPad go stale; agent enters reconnecting ==");
    agent.tick(start + Duration::from_secs(20));
    agent.print_status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn endpoint_receive_args_default_to_received_dir() {
        let (addr, local, receive_dir) = parse_endpoint_receive_args(
            &args(&["127.0.0.1:8787", "windows-001", "Windows PC"]),
            "listen <bind_addr> <device_id> <device_name> [--receive-dir <dir>]",
        )
        .unwrap();

        assert_eq!(addr, "127.0.0.1:8787");
        assert_eq!(local.id, "windows-001");
        assert_eq!(local.name, "Windows PC");
        assert_eq!(receive_dir, "received");
    }

    #[test]
    fn endpoint_receive_args_accept_custom_receive_dir() {
        let (addr, local, receive_dir) = parse_endpoint_receive_args(
            &args(&[
                "127.0.0.1:8787",
                "windows-001",
                "Windows PC",
                "--receive-dir",
                "C:\\LinkHub\\inbox",
            ]),
            "listen <bind_addr> <device_id> <device_name> [--receive-dir <dir>]",
        )
        .unwrap();

        assert_eq!(addr, "127.0.0.1:8787");
        assert_eq!(local.id, "windows-001");
        assert_eq!(local.name, "Windows PC");
        assert_eq!(receive_dir, "C:\\LinkHub\\inbox");
    }

    #[test]
    fn listen_auth_args_accept_custom_receive_dir() {
        let (addr, identity_path, trust_store_path, receive_dir) =
            parse_listen_auth_args(&args(&[
                "127.0.0.1:8787",
                "receiver-identity.txt",
                "receiver-trust-store.txt",
                "--receive-dir",
                "C:\\LinkHub\\auth-inbox",
            ]))
            .unwrap();

        assert_eq!(addr, "127.0.0.1:8787");
        assert_eq!(identity_path, "receiver-identity.txt");
        assert_eq!(trust_store_path, "receiver-trust-store.txt");
        assert_eq!(receive_dir, "C:\\LinkHub\\auth-inbox");
    }

    #[test]
    fn send_file_auth_args_allow_paths_with_spaces() {
        let (addr, identity_path, peer_device_id, trust_store_path, path) =
            parse_send_file_auth_args(&args(&[
                "127.0.0.1:8787",
                "sender-identity.txt",
                "receiver-001",
                "trust-store.txt",
                "C:\\LinkHub\\send",
                "sample file.txt",
            ]))
            .unwrap();

        assert_eq!(addr, "127.0.0.1:8787");
        assert_eq!(identity_path, "sender-identity.txt");
        assert_eq!(peer_device_id, "receiver-001");
        assert_eq!(trust_store_path, "trust-store.txt");
        assert_eq!(path, "C:\\LinkHub\\send sample file.txt");
    }

    #[cfg(feature = "webrtc")]
    #[test]
    fn listen_webrtc_args_accept_receive_dir_and_ice_urls() {
        let parsed = parse_listen_webrtc_args(&args(&[
            "ws://127.0.0.1:9000",
            "receiver-identity.txt",
            "receiver-trust-store.txt",
            "--receive-dir",
            "C:\\LinkHub\\webrtc-inbox",
            "--ice",
            "stun:stun.l.google.com:19302",
            "--ice",
            "turn:turn.example.com:3478",
        ]))
        .unwrap();

        assert_eq!(parsed.ws_url, "ws://127.0.0.1:9000");
        assert_eq!(parsed.identity_path, "receiver-identity.txt");
        assert_eq!(parsed.trust_store_path, "receiver-trust-store.txt");
        assert_eq!(parsed.receive_dir, "C:\\LinkHub\\webrtc-inbox");
        assert_eq!(
            parsed.ice.servers,
            vec![
                IceServer::stun("stun:stun.l.google.com:19302"),
                IceServer::stun("turn:turn.example.com:3478"),
            ]
        );
        assert!(!parsed.ice.force_relay);
    }

    #[cfg(feature = "webrtc")]
    #[test]
    fn listen_webrtc_args_build_turn_servers_and_relay_only() {
        let parsed = parse_listen_webrtc_args(&args(&[
            "ws://127.0.0.1:9000",
            "receiver-identity.txt",
            "receiver-trust-store.txt",
            "--ice",
            "stun:stun.l.google.com:19302",
            "--ice",
            "turn:turn.example.com:3478",
            "--turn-username",
            "linkhub",
            "--turn-credential",
            "s3cret",
            "--relay-only",
        ]))
        .unwrap();

        // STUN URLs stay credential-free; turn:/turns: URLs pick up the creds.
        assert_eq!(
            parsed.ice.servers,
            vec![
                IceServer::stun("stun:stun.l.google.com:19302"),
                IceServer::turn("turn:turn.example.com:3478", "linkhub", "s3cret"),
            ]
        );
        assert!(parsed.ice.force_relay);
    }

    #[cfg(feature = "webrtc")]
    #[test]
    fn connect_webrtc_args_allow_paths_with_spaces() {
        let parsed = parse_connect_webrtc_args(&args(&[
            "ws://127.0.0.1:9000",
            "sender-identity.txt",
            "receiver-001",
            "sender-trust-store.txt",
            "C:\\LinkHub\\send",
            "sample file.txt",
            "--ice",
            "stun:stun.l.google.com:19302",
        ]))
        .unwrap();

        assert_eq!(parsed.ws_url, "ws://127.0.0.1:9000");
        assert_eq!(parsed.identity_path, "sender-identity.txt");
        assert_eq!(parsed.peer_device_id, "receiver-001");
        assert_eq!(parsed.trust_store_path, "sender-trust-store.txt");
        assert_eq!(parsed.path, "C:\\LinkHub\\send sample file.txt");
        assert_eq!(
            parsed.ice.servers,
            vec![IceServer::stun("stun:stun.l.google.com:19302")]
        );
        assert!(!parsed.ice.force_relay);
    }

    #[test]
    fn mdns_advertise_args_accept_duration_and_identity() {
        let (identity, port, duration) = parse_mdns_advertise_args(&args(&[
            "phone-001",
            "Android Phone",
            "phone-public-key",
            "8787",
            "3",
        ]))
        .unwrap();

        assert_eq!(identity.device_id(), "phone-001");
        assert_eq!(identity.device_name(), "Android Phone");
        assert_eq!(identity.public_key(), "phone-public-key");
        assert_eq!(port, 8787);
        assert_eq!(duration, Duration::from_secs(3));
    }

    #[test]
    fn mdns_advertise_args_allow_split_device_name_without_duration() {
        let (identity, port, duration) = parse_mdns_advertise_args(&args(&[
            "windows-001",
            "Windows",
            "PC",
            "windows-public-key",
            "8787",
        ]))
        .unwrap();

        assert_eq!(identity.device_id(), "windows-001");
        assert_eq!(identity.device_name(), "Windows PC");
        assert_eq!(identity.public_key(), "windows-public-key");
        assert_eq!(port, 8787);
        assert_eq!(duration, Duration::from_secs(30));
    }

    #[test]
    fn mdns_advertise_args_allow_split_device_name_with_duration() {
        let (identity, port, duration) = parse_mdns_advertise_args(&args(&[
            "windows-001",
            "Windows",
            "PC",
            "windows-public-key",
            "8787",
            "4",
        ]))
        .unwrap();

        assert_eq!(identity.device_name(), "Windows PC");
        assert_eq!(port, 8787);
        assert_eq!(duration, Duration::from_secs(4));
    }

    #[test]
    fn optional_seconds_args_use_default_when_missing() {
        let duration =
            parse_optional_seconds_arg(&args(&[]), "scan-mdns [seconds]", Duration::from_secs(5))
                .unwrap();

        assert_eq!(duration, Duration::from_secs(5));
    }

    #[test]
    fn trusted_mdns_args_accept_store_path_and_duration() {
        let (local_name, trust_store_path, duration) =
            parse_scan_trusted_mdns_args(&args(&["Windows PC", "trust-store.txt", "4"])).unwrap();

        assert_eq!(local_name, "Windows PC");
        assert_eq!(trust_store_path, "trust-store.txt");
        assert_eq!(duration, Duration::from_secs(4));
    }

    #[test]
    fn trusted_mdns_scan_updates_only_paired_devices() {
        let now = Instant::now();
        let mut trust_store = TrustStore::new();
        let trusted = linkhub_core::TrustedDevice::new(
            DeviceIdentity::new(
                "phone-001",
                "Android Phone",
                "phone-public-key",
                "00".repeat(32),
            ),
            SystemTime::UNIX_EPOCH,
        );
        trust_store.trust(trusted);
        let endpoints = vec![
            DiscoveryEndpoint::lan_tcp(
                "phone-001",
                "Android Phone",
                ([127, 0, 0, 1], 8787).into(),
                now,
            ),
            DiscoveryEndpoint::lan_tcp(
                "stranger-001",
                "Stranger",
                ([127, 0, 0, 1], 8788).into(),
                now,
            ),
        ];

        let agent = agent_from_trusted_mdns("Windows PC", &trust_store, &endpoints, now);

        let phone = agent.device("phone-001").unwrap();
        assert_eq!(phone.active_route(), Some(TransportKind::LanTcp));
        assert!(agent.device("stranger-001").is_none());
    }

    #[test]
    fn status_renderers_show_local_and_trusted_devices() {
        let identity = LocalIdentity::from_keys(
            "Windows <PC>",
            [29; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        let mut trust_store = TrustStore::new();
        trust_store.trust(TrustedDevice::new(
            DeviceIdentity::new(
                "phone-001",
                "Phone & Tablet",
                "phone-public-key",
                "00".repeat(32),
            ),
            SystemTime::UNIX_EPOCH + Duration::from_secs(42),
        ));

        let text = render_status_text(&identity, &trust_store);
        let html = render_status_html(&identity, &trust_store);

        assert!(text.contains("local_device_name=Windows <PC>"));
        assert!(text.contains("trusted_device_count=1"));
        assert!(text.contains("trusted_device=phone-001"));
        assert!(html.contains("Windows &lt;PC&gt;"));
        assert!(html.contains("Phone &amp; Tablet"));
        assert!(html.contains("<td>42</td>"));
    }

    #[test]
    fn identity_init_reuses_existing_identity_and_show_loads_it() {
        let path = env::temp_dir().join(format!(
            "linkhub-main-local-identity-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_arg = path.display().to_string();

        run_identity_command(&args(&["init", &path_arg, "Windows PC"])).unwrap();
        let first = LocalIdentity::load_from_path(&path).unwrap();
        run_identity_command(&args(&["init", &path_arg, "Renamed PC"])).unwrap();
        let second = LocalIdentity::load_from_path(&path).unwrap();
        run_identity_command(&args(&["show", &path_arg])).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(second.device_id(), first.device_id());
        assert_eq!(second.device_name(), "Windows PC");
    }

    #[cfg(windows)]
    #[test]
    fn identity_secure_init_reuses_secure_identity_and_supports_secure_prefix() {
        let path = env::temp_dir().join(format!(
            "linkhub-main-secure-local-identity-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_arg = path.display().to_string();
        let secure_arg = format!("secure:{path_arg}");

        run_identity_command(&args(&["secure-init", &path_arg, "Windows PC"])).unwrap();
        let first = load_local_identity_arg(&secure_arg).unwrap();
        run_identity_command(&args(&["secure-init", &path_arg, "Renamed PC"])).unwrap();
        let second = load_local_identity_arg(&secure_arg).unwrap();
        run_identity_command(&args(&["secure-show", &path_arg])).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(second.device_id(), first.device_id());
        assert_eq!(second.device_name(), "Windows PC");
        assert!(!content.contains(first.signing_key_hex()));
    }

    #[test]
    fn identity_pairing_payload_and_inspect_commands_accept_valid_payload() {
        let path = env::temp_dir().join(format!(
            "linkhub-main-pairing-identity-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_arg = path.display().to_string();
        let identity = LocalIdentity::from_keys(
            "Windows PC",
            [23; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        identity.save_to_path(&path).unwrap();
        let invitation = PairingInvitation::new(
            identity.identity().clone(),
            SystemTime::now(),
            Duration::from_secs(90),
        );
        let payload = invitation.to_payload();

        run_identity_command(&args(&["pairing-payload", &path_arg, "90"])).unwrap();
        run_identity_command(&args(&["inspect-pairing", &payload])).unwrap();
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn identity_pairing_payload_can_be_confirmed_into_trust_store() {
        let suffix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let local_path = env::temp_dir().join(format!("linkhub-main-local-pair-{suffix}.txt"));
        let trust_store_path =
            env::temp_dir().join(format!("linkhub-main-pairing-store-{suffix}.txt"));
        let local_path_arg = local_path.display().to_string();
        let trust_store_path_arg = trust_store_path.display().to_string();
        let local_identity = LocalIdentity::from_keys(
            "Receiver PC",
            [31; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        let peer_identity = LocalIdentity::from_keys(
            "Sender PC",
            [32; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        local_identity.save_to_path(&local_path).unwrap();
        let invitation = PairingInvitation::new(
            peer_identity.identity().clone(),
            SystemTime::now(),
            Duration::from_secs(120),
        );
        let payload = invitation.to_payload();
        let confirmation_code =
            PairingSession::new(local_identity.identity().clone(), invitation).confirmation_code();

        run_identity_command(&args(&["pairing-code", &local_path_arg, &payload])).unwrap();
        run_identity_command(&args(&[
            "trust-pairing",
            &local_path_arg,
            &payload,
            &confirmation_code,
            &trust_store_path_arg,
        ]))
        .unwrap();
        let trust_store = TrustStore::load_from_path(&trust_store_path).unwrap();
        let _ = fs::remove_file(&local_path);
        let _ = fs::remove_file(&trust_store_path);

        assert!(trust_store.is_trusted(peer_identity.device_id()));
    }

    #[test]
    fn identity_handshake_commands_sign_and_verify() {
        let path = env::temp_dir().join(format!(
            "linkhub-main-handshake-identity-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_arg = path.display().to_string();
        let identity = LocalIdentity::from_keys(
            "Windows PC",
            [41; 32],
            [0u8; 32],
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        identity.save_to_path(&path).unwrap();
        let signature = identity
            .sign_handshake_challenge("phone-001", "nonce-001")
            .unwrap();

        run_identity_command(&args(&["handshake-nonce"])).unwrap();
        run_identity_command(&args(&[
            "sign-handshake",
            &path_arg,
            "phone-001",
            "nonce-001",
        ]))
        .unwrap();
        run_identity_command(&args(&[
            "verify-handshake",
            identity.device_id(),
            identity.device_name(),
            identity.public_key(),
            "phone-001",
            "nonce-001",
            &signature,
        ]))
        .unwrap();
        let _ = fs::remove_file(&path);
    }
}
