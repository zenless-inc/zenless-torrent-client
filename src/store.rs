//! Small per-torrent bookkeeping that librqbit does not persist itself:
//! when a torrent was added / completed, lifetime upload, and magnets that
//! were still resolving when the app closed. Stored in
//! `%APPDATA%\Zenless\Torrent\torrents.json`.

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentMeta {
    pub added_at: u64,
    pub completed_at: Option<u64>,
    /// Bytes uploaded in previous sessions.
    pub uploaded: u64,
}

/// A magnet added with "start immediately" whose metadata was not resolved yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingMagnet {
    pub magnet: String,
    pub output_dir: PathBuf,
    pub paused: bool,
    pub added_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreData {
    pub torrents: HashMap<String, TorrentMeta>,
    pub pending: Vec<PendingMagnet>,
}

pub struct Store {
    path: PathBuf,
    pub data: StoreData,
    dirty: bool,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            data,
            dirty: false,
        }
    }

    pub fn meta(&mut self, info_hash: &str, now: u64) -> &mut TorrentMeta {
        let dirty = &mut self.dirty;
        self.data
            .torrents
            .entry(info_hash.to_owned())
            .or_insert_with(|| {
                *dirty = true;
                TorrentMeta {
                    added_at: now,
                    ..Default::default()
                }
            })
    }

    pub fn get(&self, info_hash: &str) -> Option<&TorrentMeta> {
        self.data.torrents.get(info_hash)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn remove(&mut self, info_hash: &str) {
        if self.data.torrents.remove(info_hash).is_some() {
            self.dirty = true;
        }
    }

    pub fn save_if_dirty(&mut self) {
        if self.dirty {
            self.save();
        }
    }

    pub fn save(&mut self) {
        match serde_json::to_vec_pretty(&self.data) {
            Ok(json) => match crate::settings::write_atomic(&self.path, &json) {
                Ok(()) => self.dirty = false,
                Err(e) => tracing::warn!("could not save {}: {e}", self.path.display()),
            },
            Err(e) => tracing::warn!("could not serialize torrent store: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("zenless-store-test-{}", std::process::id()));
        let path = dir.join("torrents.json");
        let mut s = Store::load(path.clone());
        s.meta("abc", 100).uploaded = 42;
        s.data.pending.push(PendingMagnet {
            magnet: "magnet:?xt=urn:btih:abc".into(),
            output_dir: PathBuf::from("C:/x"),
            paused: false,
            added_at: 5,
        });
        s.save();
        let back = Store::load(path);
        assert_eq!(back.data, s.data);
        assert_eq!(back.get("abc").unwrap().added_at, 100);
        let _ = std::fs::remove_dir_all(dir);
    }
}
