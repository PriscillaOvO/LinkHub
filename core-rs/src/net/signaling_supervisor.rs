//! Long-running synchronous supervisor for signaling presence.
//!
//! `SignalingClient` is a single authenticated WebSocket. This module owns the
//! higher-level loop: connect, stay alive with pings, surface deliveries, and
//! reconnect/re-authenticate after the socket dies. It intentionally uses
//! `std::thread` + `flume`, not tokio, so the default core/JNI surface stays
//! synchronous.

use std::io;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::LocalIdentity;

use super::signaling_client::{RetryPolicy, SignalingClient, SignalingDelivery, SignalingEvent};

#[derive(Debug, Clone)]
pub struct SignalingSupervisorConfig {
    pub url: String,
    pub identity: LocalIdentity,
    pub retry_policy: RetryPolicy,
    pub heartbeat_interval: Duration,
    pub read_timeout: Duration,
}

impl SignalingSupervisorConfig {
    pub fn new(url: impl Into<String>, identity: LocalIdentity) -> Self {
        Self {
            url: url.into(),
            identity,
            retry_policy: RetryPolicy::default(),
            heartbeat_interval: Duration::from_secs(30),
            read_timeout: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingSupervisorEvent {
    Connected {
        device_id: String,
        public_key_hex: String,
    },
    Delivery(SignalingDelivery),
    ServerError(String),
    Disconnected(String),
    Stopped,
}

pub struct SignalingSupervisor {
    stop_tx: flume::Sender<()>,
    events_rx: flume::Receiver<SignalingSupervisorEvent>,
    handle: Option<JoinHandle<()>>,
}

impl SignalingSupervisor {
    pub fn start(config: SignalingSupervisorConfig) -> Self {
        let (stop_tx, stop_rx) = flume::bounded::<()>(1);
        let (events_tx, events_rx) = flume::unbounded::<SignalingSupervisorEvent>();
        let handle = thread::spawn(move || run_supervisor_loop(config, stop_rx, events_tx));

        Self {
            stop_tx,
            events_rx,
            handle: Some(handle),
        }
    }

    pub fn events(&self) -> flume::Receiver<SignalingSupervisorEvent> {
        self.events_rx.clone()
    }

    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SignalingSupervisor {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_supervisor_loop(
    config: SignalingSupervisorConfig,
    stop_rx: flume::Receiver<()>,
    events_tx: flume::Sender<SignalingSupervisorEvent>,
) {
    while stop_rx.is_empty() {
        let Some(mut client) = connect_with_policy(&config, &stop_rx, &events_tx) else {
            break;
        };

        let _ = client.set_read_timeout(Some(config.read_timeout));
        let _ = events_tx.send(SignalingSupervisorEvent::Connected {
            device_id: client.device_id().to_string(),
            public_key_hex: client.public_key_hex().to_string(),
        });

        if run_connected_loop(&mut client, &config, &stop_rx, &events_tx).is_none() {
            break;
        }
    }

    let _ = events_tx.send(SignalingSupervisorEvent::Stopped);
}

fn connect_with_policy(
    config: &SignalingSupervisorConfig,
    stop_rx: &flume::Receiver<()>,
    events_tx: &flume::Sender<SignalingSupervisorEvent>,
) -> Option<SignalingClient> {
    let attempts = config.retry_policy.max_attempts.max(1);

    loop {
        for attempt in 1..=attempts {
            if stop_rx.try_recv().is_ok() {
                return None;
            }

            match SignalingClient::connect(&config.url, &config.identity) {
                Ok(client) => return Some(client),
                Err(err) => {
                    if attempt == attempts {
                        let _ = events_tx.send(SignalingSupervisorEvent::Disconnected(format!(
                            "failed to connect to signaling server {} after {attempts} attempts: {err}",
                            config.url
                        )));
                    } else if wait_or_stop(
                        stop_rx,
                        config.retry_policy.delay_after_attempt(attempt),
                    ) {
                        return None;
                    }
                }
            }
        }

        if wait_or_stop(stop_rx, config.retry_policy.delay_after_attempt(attempts)) {
            return None;
        }
    }
}

fn run_connected_loop(
    client: &mut SignalingClient,
    config: &SignalingSupervisorConfig,
    stop_rx: &flume::Receiver<()>,
    events_tx: &flume::Sender<SignalingSupervisorEvent>,
) -> Option<()> {
    let mut last_ping = Instant::now();

    loop {
        if stop_rx.try_recv().is_ok() {
            return None;
        }

        if last_ping.elapsed() >= config.heartbeat_interval {
            if let Err(err) = client.ping() {
                let _ = events_tx.send(SignalingSupervisorEvent::Disconnected(format!(
                    "signaling heartbeat failed: {err}"
                )));
                return Some(());
            }
            last_ping = Instant::now();
        }

        match client.recv() {
            Ok(SignalingEvent::Delivery(delivery)) => {
                let _ = events_tx.send(SignalingSupervisorEvent::Delivery(delivery));
            }
            Ok(SignalingEvent::ServerError(reason)) => {
                let _ = events_tx.send(SignalingSupervisorEvent::ServerError(reason));
            }
            Err(err) if is_timeout(&err) => {}
            Err(err) => {
                let _ = events_tx.send(SignalingSupervisorEvent::Disconnected(format!(
                    "signaling connection ended: {err}"
                )));
                return Some(());
            }
        }
    }
}

fn wait_or_stop(stop_rx: &flume::Receiver<()>, delay: Duration) -> bool {
    if delay.is_zero() {
        return stop_rx.try_recv().is_ok();
    }

    stop_rx.recv_timeout(delay).is_ok()
}

fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_delay_stop_check_is_nonblocking() {
        let (_tx, rx) = flume::bounded::<()>(1);
        assert!(!wait_or_stop(&rx, Duration::ZERO));
    }
}
