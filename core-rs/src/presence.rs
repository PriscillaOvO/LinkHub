use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Discovered,
    Connected,
    Degraded,
    Reconnecting,
    Offline,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            ConnectionState::Discovered => "DISCOVERED",
            ConnectionState::Connected => "CONNECTED",
            ConnectionState::Degraded => "DEGRADED",
            ConnectionState::Reconnecting => "RECONNECTING",
            ConnectionState::Offline => "OFFLINE",
        };
        write!(f, "{value}")
    }
}
