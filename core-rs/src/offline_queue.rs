//! Offline queue — persists pending messages when target is unreachable
//! and retries delivery when the device comes back online.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OfflineKind {
    Text,
    File,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OfflineMessage {
    pub id: String,
    pub target_device: String,
    pub kind: OfflineKind,
    pub payload_json: String, // serialized text or file metadata
    pub created_at_unix: u64,
    pub retry_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct OfflineQueue {
    pub messages: Vec<OfflineMessage>,
}

impl OfflineQueue {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).map_err(|e| format!("{e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("{e}"))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
        }
        fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(|e| format!("{e}"))?,
        )
        .map_err(|e| format!("{e}"))
    }

    pub fn enqueue(&mut self, msg: OfflineMessage, path: impl AsRef<Path>) -> Result<(), String> {
        self.messages.push(msg);
        self.save(path)
    }

    pub fn dequeue(&mut self, id: &str) -> Option<OfflineMessage> {
        if let Some(pos) = self.messages.iter().position(|m| m.id == id) {
            Some(self.messages.remove(pos))
        } else {
            None
        }
    }

    /// Retry all queued messages for a specific device. Returns successful IDs.
    pub fn pending_for(&self, device_id: &str) -> Vec<&OfflineMessage> {
        self.messages
            .iter()
            .filter(|m| m.target_device == device_id)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

pub fn new_offline_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("offline-{ts}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn queue_enqueue_and_persist() {
        let path = env::temp_dir().join("linkhub-offline-queue-test.json");
        let _ = fs::remove_file(&path);

        let mut queue = OfflineQueue::default();
        queue
            .enqueue(
                OfflineMessage {
                    id: "msg-1".into(),
                    target_device: "peer-a".into(),
                    kind: OfflineKind::Text,
                    payload_json: r#""hello""#.into(),
                    created_at_unix: 0,
                    retry_count: 0,
                },
                &path,
            )
            .unwrap();

        let loaded = OfflineQueue::load(&path).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].target_device, "peer-a");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn queue_pending_filters_by_device() {
        let mut queue = OfflineQueue::default();
        queue.messages.push(OfflineMessage {
            id: "1".into(),
            target_device: "a".into(),
            kind: OfflineKind::Text,
            payload_json: "".into(),
            created_at_unix: 0,
            retry_count: 0,
        });
        queue.messages.push(OfflineMessage {
            id: "2".into(),
            target_device: "b".into(),
            kind: OfflineKind::File,
            payload_json: "".into(),
            created_at_unix: 0,
            retry_count: 0,
        });
        assert_eq!(queue.pending_for("a").len(), 1);
        assert_eq!(queue.pending_for("b").len(), 1);
        assert_eq!(queue.pending_for("c").len(), 0);
    }
}
