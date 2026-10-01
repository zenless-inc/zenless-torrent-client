//! App settings stored in `%APPDATA%\Zenless\Torrent\settings.json`.

use crate::model::Column;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `%APPDATA%\Zenless\Torrent` (`ZENLESS_TORRENT_DATA_DIR` overrides it, for
/// tests that must not touch the real data).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZENLESS_TORRENT_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    crate::shared::theme::zenless_config_dir().join("Torrent")
}

/// librqbit session persistence folder.
pub fn session_dir() -> PathBuf {
    data_dir().join("session")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// `Downloads\Torrents`.
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Torrents")
}

pub const DEFAULT_PORT: u16 = 6881;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Default save folder for new torrents.
    pub download_dir: PathBuf,
    pub start_with_windows: bool,
    /// Start hidden in the taskbar (also used for the autostart entry).
    pub start_minimized: bool,
    /// Incoming peer connections (TCP, and uTP when enabled).
    pub listen_port: u16,
    pub enable_dht: bool,
    pub enable_upnp: bool,
    pub enable_utp: bool,
    /// KiB/s, 0 = unlimited.
    pub download_limit_kib: u32,
    /// KiB/s, 0 = unlimited.
    pub upload_limit_kib: u32,
    /// Default of the "Start immediately" checkbox in the Add dialog.
    pub start_immediately: bool,
    pub notify_on_complete: bool,
    pub show_details: bool,
    pub sort_column: Column,
    pub sort_descending: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            start_with_windows: false,
            start_minimized: true,
            listen_port: DEFAULT_PORT,
            enable_dht: true,
            enable_upnp: true,
            enable_utp: false,
            download_limit_kib: 0,
            upload_limit_kib: 0,
            start_immediately: true,
            notify_on_complete: true,
            show_details: true,
            sort_column: Column::Added,
            sort_descending: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(&settings_path())
    }

    /// Missing or broken files fall back to defaults (unknown fields are ignored).
    pub fn load_from(path: &Path) -> Self {
        let mut s: Settings = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if s.download_dir.as_os_str().is_empty() {
            s.download_dir = default_download_dir();
        }
        if s.listen_port == 0 {
            s.listen_port = DEFAULT_PORT;
        }
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&settings_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, &json)
    }

    /// Download limit in bytes/s (`None` = unlimited).
    pub fn download_limit_bps(&self) -> Option<u32> {
        kib_to_bps(self.download_limit_kib)
    }

    pub fn upload_limit_bps(&self) -> Option<u32> {
        kib_to_bps(self.upload_limit_kib)
    }

    /// Settings that require restarting the BitTorrent session when changed.
    pub fn network_key(&self) -> (u16, bool, bool, bool, PathBuf) {
        (
            self.listen_port,
            self.enable_dht,
            self.enable_upnp,
            self.enable_utp,
            self.download_dir.clone(),
        )
    }
}

fn kib_to_bps(kib: u32) -> Option<u32> {
    if kib == 0 {
        None
    } else {
        Some(kib.saturating_mul(1024))
    }
}

/// Writes via a temp file + rename so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("zenless-torrent-test-{}-{name}", std::process::id()))
            .join("settings.json")
    }

    #[test]
    fn roundtrip() {
        let path = temp_file("roundtrip");
        let s = Settings {
            download_dir: PathBuf::from(r"D:\Torrents"),
            listen_port: 51413,
            enable_dht: false,
            download_limit_kib: 512,
            sort_column: Column::Ratio,
            sort_descending: false,
            ..Default::default()
        };
        s.save_to(&path).unwrap();
        let back = Settings::load_from(&path);
        assert_eq!(back, s);
        assert_eq!(back.download_limit_bps(), Some(512 * 1024));
        assert_eq!(back.upload_limit_bps(), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn partial_and_broken_files_use_defaults() {
        let path = temp_file("partial");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"listen_port": 7000, "future_option": true}"#).unwrap();
        let s = Settings::load_from(&path);
        assert_eq!(s.listen_port, 7000);
        assert!(s.enable_dht);
        assert_eq!(s.download_dir, default_download_dir());

        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn default_folder_is_torrents_subfolder() {
        assert!(default_download_dir().ends_with("Torrents"));
        assert!(session_dir().ends_with("session"));
    }
}
