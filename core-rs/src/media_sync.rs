//! Media playback coordination (Stage 8).
//!
//! Protocol for one device to control playback on another,
//! sync play queues, and transfer playback between devices.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaItem {
    pub uri: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: u64,
    pub album_art_uri: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayQueue {
    pub items: Vec<MediaItem>,
    pub current_index: usize,
    pub position_ms: u64,
    pub state: PlayState,
    pub volume: u8, // 0-100
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

impl Default for PlayQueue {
    fn default() -> Self {
        Self {
            items: vec![],
            current_index: 0,
            position_ms: 0,
            state: PlayState::Stopped,
            volume: 80,
            shuffle: false,
            repeat: RepeatMode::Off,
        }
    }
}

/// Commands that can be sent to control remote playback.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaCommand {
    Play { uri: Option<String> },
    Pause,
    Resume,
    Seek { position_ms: u64 },
    SetVolume { level: u8 },
    Next,
    Previous,
    SwitchDevice { target_device_id: String },
    SyncState { queue: PlayQueue },
    SetShuffle { enabled: bool },
    SetRepeat { mode: RepeatMode },
}

/// Response to a media command.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaStatus {
    pub success: bool,
    pub current_state: PlayQueue,
    pub error: Option<String>,
}

/// Session for coordinated playback between devices.
#[derive(Clone, Debug)]
pub struct MediaSession {
    pub session_id: String,
    pub controller_device: String, // which device is in control
    pub player_device: String,     // which device is playing
    pub queue: PlayQueue,
}

impl MediaSession {
    pub fn new(session_id: &str, controller: &str, player: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            controller_device: controller.to_string(),
            player_device: player.to_string(),
            queue: PlayQueue::default(),
        }
    }

    /// Transfer playback to another device.
    pub fn switch_player(&mut self, new_player: &str) {
        self.player_device = new_player.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_queue_defaults() {
        let q = PlayQueue::default();
        assert_eq!(q.state, PlayState::Stopped);
        assert_eq!(q.volume, 80);
        assert!(q.items.is_empty());
    }

    #[test]
    fn media_command_serialization() {
        let cmd = MediaCommand::Play {
            uri: Some("file:///music/song.mp3".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("song.mp3"));
    }

    #[test]
    fn media_session_switch_player() {
        let mut session = MediaSession::new("s1", "controller", "player-a");
        assert_eq!(session.player_device, "player-a");
        session.switch_player("player-b");
        assert_eq!(session.player_device, "player-b");
    }
}
