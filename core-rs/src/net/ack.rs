use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::protocol::{parse_message, serialize_message, WireMessage};

pub(super) const ACK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ACK_ATTEMPTS: u32 = 3;

pub(super) fn write_message(writer: &mut TcpStream, message: &WireMessage) -> io::Result<()> {
    writeln!(writer, "{}", serialize_message(message))?;
    writer.flush()
}

pub(super) fn send_text_with_retries(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    message_id: &str,
    text: &str,
) -> io::Result<()> {
    send_with_ack_retries(
        stream,
        reader,
        message_id,
        "TEXT_RECEIVED",
        || WireMessage::text(message_id, text),
        "TEXT",
    )
}

pub(super) fn send_file_start_with_retries(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    transfer_id: &str,
    make_message: impl Fn() -> WireMessage,
) -> io::Result<u64> {
    let mut last_error = None;

    for attempt in 1..=MAX_ACK_ATTEMPTS {
        println!("Sending FILE_START attempt {attempt}/{MAX_ACK_ATTEMPTS}: {transfer_id}");
        write_message(stream, &make_message())?;

        match wait_for_file_start_ack(reader, transfer_id) {
            Ok(resume_from_chunk) => return Ok(resume_from_chunk),
            Err(err) if is_retryable_ack_error(&err) => {
                eprintln!("No matching ACK for {transfer_id} on attempt {attempt}: {err}");
                last_error = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("FILE_START delivery timed out after {MAX_ACK_ATTEMPTS} attempts"),
        )
    }))
}

pub(super) fn send_with_ack_retries(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    message_id: &str,
    expected_status: &str,
    make_message: impl Fn() -> WireMessage,
    label: &str,
) -> io::Result<()> {
    let mut last_error = None;

    for attempt in 1..=MAX_ACK_ATTEMPTS {
        println!("Sending {label} attempt {attempt}/{MAX_ACK_ATTEMPTS}: {message_id}");
        write_message(stream, &make_message())?;

        match wait_for_ack(reader, message_id, expected_status) {
            Ok(()) => return Ok(()),
            Err(err) if is_retryable_ack_error(&err) => {
                eprintln!("No matching ACK for {message_id} on attempt {attempt}: {err}");
                last_error = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{label} delivery timed out after {MAX_ACK_ATTEMPTS} attempts"),
        )
    }))
}

fn wait_for_file_start_ack(
    reader: &mut BufReader<TcpStream>,
    expected_transfer_id: &str,
) -> io::Result<u64> {
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
                if message_id == expected_transfer_id {
                    let Some(resume_from_chunk) = parse_file_start_ack_status(&status) else {
                        eprintln!("Ignored ACK with unexpected FILE_START status: {status}");
                        continue;
                    };

                    println!("Delivery acknowledged: {message_id} {status}");
                    return Ok(resume_from_chunk);
                }

                eprintln!("Ignored ACK for unmatched message: {message_id} {status}");
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

fn wait_for_ack(
    reader: &mut BufReader<TcpStream>,
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

                eprintln!("Ignored ACK for unmatched message: {message_id} {status}");
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

pub(super) fn parse_file_start_ack_status(status: &str) -> Option<u64> {
    if status == "FILE_START_RECEIVED" {
        return Some(0);
    }

    status
        .strip_prefix("FILE_START_RECEIVED:")
        .and_then(|value| value.parse().ok())
}

fn is_retryable_ack_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_start_ack_status() {
        assert_eq!(parse_file_start_ack_status("FILE_START_RECEIVED"), Some(0));
        assert_eq!(
            parse_file_start_ack_status("FILE_START_RECEIVED:3"),
            Some(3)
        );
        assert_eq!(parse_file_start_ack_status("TEXT_RECEIVED"), None);
    }

    #[test]
    fn ack_timeout_errors_are_retryable() {
        let err = io::Error::new(io::ErrorKind::TimedOut, "timeout");

        assert!(is_retryable_ack_error(&err));
    }

    #[test]
    fn invalid_ack_errors_are_not_retryable() {
        let err = io::Error::new(io::ErrorKind::InvalidData, "bad ack");

        assert!(!is_retryable_ack_error(&err));
    }
}
