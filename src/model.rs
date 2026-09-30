//! Plain data types shared by the engine, the local API and the UI.
//!
//! Nothing in here depends on librqbit, so the UI (and the demo mode) can be
//! driven by any producer of [`Snapshot`]s.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

/// User-facing torrent status (drives the status pill and the sidebar filters).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Downloading,
    Seeding,
    Paused,
    Checking,
    Metadata,
    Error,
    Completed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Downloading => "Downloading",
            Status::Seeding => "Seeding",
            Status::Paused => "Paused",
            Status::Checking => "Checking",
            Status::Metadata => "Metadata",
            Status::Error => "Error",
            Status::Completed => "Completed",
        }
    }

    /// Lower-case name used by the local API.
    pub fn api_name(self) -> &'static str {
        match self {
            Status::Downloading => "downloading",
            Status::Seeding => "seeding",
            Status::Paused => "paused",
            Status::Checking => "checking",
            Status::Metadata => "metadata",
            Status::Error => "error",
            Status::Completed => "completed",
        }
    }

    /// Whether "Pause" makes sense for a torrent in this state.
    pub fn can_pause(self) -> bool {
        matches!(self, Status::Downloading | Status::Seeding | Status::Checking)
    }

    /// Whether "Resume" makes sense for a torrent in this state.
    pub fn can_resume(self) -> bool {
        matches!(self, Status::Paused | Status::Completed | Status::Error)
    }

    /// Sort key used when sorting the table by status.
    pub fn order(self) -> u8 {
        match self {
            Status::Downloading => 0,
            Status::Metadata => 1,
            Status::Checking => 2,
            Status::Seeding => 3,
            Status::Paused => 4,
            Status::Completed => 5,
            Status::Error => 6,
        }
    }
}

/// Engine-level state of a torrent, independent of librqbit's types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    Initializing,
    Live,
    Paused,
    Error,
}

/// Maps the engine state + completion flag to what the user sees.
pub fn classify(state: EngineState, finished: bool) -> Status {
    match (state, finished) {
        (EngineState::Error, _) => Status::Error,
        (EngineState::Initializing, _) => Status::Checking,
        (EngineState::Live, true) => Status::Seeding,
        (EngineState::Live, false) => Status::Downloading,
        (EngineState::Paused, true) => Status::Completed,
        (EngineState::Paused, false) => Status::Paused,
    }
}

/// Sidebar filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    All,
    Downloading,
    Seeding,
    Completed,
    Paused,
    Error,
}

impl Filter {
    pub const ALL: [Filter; 6] = [
        Filter::All,
        Filter::Downloading,
        Filter::Seeding,
        Filter::Completed,
        Filter::Paused,
        Filter::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Downloading => "Downloading",
            Filter::Seeding => "Seeding",
            Filter::Completed => "Completed",
            Filter::Paused => "Paused",
            Filter::Error => "Error",
        }
    }

    pub fn matches(self, status: Status) -> bool {
        match self {
            Filter::All => true,
            Filter::Downloading => matches!(
                status,
                Status::Downloading | Status::Metadata | Status::Checking
            ),
            Filter::Seeding => status == Status::Seeding,
            Filter::Completed => matches!(status, Status::Seeding | Status::Completed),
            Filter::Paused => status == Status::Paused,
            Filter::Error => status == Status::Error,
        }
    }
}

/// Table columns (also used for sorting).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Column {
    Name,
    Size,
    Progress,
    Status,
    Down,
    Up,
    Peers,
    Eta,
    Ratio,
    Added,
}

/// One file inside a torrent.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    /// Path components relative to the torrent root.
    pub path: Vec<String>,
    pub len: u64,
    /// BEP-47 padding file (never shown to the user).
    pub padding: bool,
}

impl FileEntry {
    pub fn display(&self) -> String {
        self.path.join("/")
    }
}

/// Everything the UI needs to know about one torrent.
#[derive(Clone, Debug)]
pub struct TorrentView {
    /// Stable key: the info hash, or `pending-<n>` while a magnet resolves.
    pub key: String,
    pub info_hash: String,
    pub name: String,
    pub status: Status,
    pub error: Option<String>,
    /// Size of the selected files.
    pub total_bytes: u64,
    /// Size of the whole torrent.
    pub full_size: u64,
    /// Verified bytes of the selected files (or checked bytes while checking).
    pub have_bytes: u64,
    /// Uploaded bytes, accumulated across sessions.
    pub uploaded: u64,
    pub down_bps: f64,
    pub up_bps: f64,
    pub eta_secs: Option<f64>,
    pub peers_live: u32,
    pub peers_connecting: u32,
    pub peers_seen: u32,
    pub peers_queued: u32,
    pub added_at: u64,
    pub completed_at: Option<u64>,
    pub save_path: PathBuf,
    pub pieces: u32,
    pub piece_length: u32,
    pub private: bool,
    pub files: Arc<Vec<FileEntry>>,
    /// Verified bytes per file (same order as `files`; may be empty).
    pub file_progress: Vec<u64>,
    /// Whether each file is selected for download (same order as `files`).
    pub included: Vec<bool>,
    pub trackers: Arc<Vec<String>>,
    /// True for magnets that are still resolving (not yet in the session).
    pub pending: bool,
}

impl TorrentView {
    /// An empty view (used for resolving magnets and as a base for demo rows).
    pub fn placeholder(key: impl Into<String>, name: impl Into<String>, status: Status) -> Self {
        Self {
            key: key.into(),
            info_hash: String::new(),
            name: name.into(),
            status,
            error: None,
            total_bytes: 0,
            full_size: 0,
            have_bytes: 0,
            uploaded: 0,
            down_bps: 0.0,
            up_bps: 0.0,
            eta_secs: None,
            peers_live: 0,
            peers_connecting: 0,
            peers_seen: 0,
            peers_queued: 0,
            added_at: 0,
            completed_at: None,
            save_path: PathBuf::new(),
            pieces: 0,
            piece_length: 0,
            private: false,
            files: Arc::new(Vec::new()),
            file_progress: Vec::new(),
            included: Vec::new(),
            trackers: Arc::new(Vec::new()),
            pending: false,
        }
    }

    /// Progress in 0..=1.
    pub fn progress(&self) -> f32 {
        if self.total_bytes == 0 {
            if matches!(self.status, Status::Seeding | Status::Completed) {
                1.0
            } else {
                0.0
            }
        } else {
            (self.have_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub fn ratio(&self) -> f64 {
        ratio(self.uploaded, self.have_bytes)
    }
}

/// A connected peer of the focused torrent.
#[derive(Clone, Debug)]
pub struct PeerView {
    pub addr: String,
    pub client: String,
    pub kind: String,
    pub downloaded: u64,
    pub uploaded: u64,
    pub down_bps: f64,
    pub up_bps: f64,
}

/// Engine lifecycle as shown in the status bar.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineStatus {
    Starting,
    Running,
    Restarting,
    Failed(String),
    Demo,
}

/// Immutable picture of the engine, republished about once per second.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub generation: u64,
    pub engine: EngineStatus,
    pub torrents: Vec<TorrentView>,
    pub down_bps: f64,
    pub up_bps: f64,
    pub dht_nodes: Option<usize>,
    pub listen_port: Option<u16>,
    /// Peers of the torrent the UI asked to focus: `(key, peers)`.
    pub peers: Option<(String, Vec<PeerView>)>,
    /// Non-fatal note from the engine (e.g. "port busy, using random port").
    pub notice: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            engine: EngineStatus::Starting,
            torrents: Vec::new(),
            down_bps: 0.0,
            up_bps: 0.0,
            dht_nodes: None,
            listen_port: None,
            peers: None,
            notice: None,
        }
    }
}

impl Snapshot {
    pub fn count(&self, status: Status) -> usize {
        self.torrents.iter().filter(|t| t.status == status).count()
    }

    pub fn count_filter(&self, filter: Filter) -> usize {
        self.torrents.iter().filter(|t| filter.matches(t.status)).count()
    }
}

/// Where a new torrent comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum AddSource {
    Magnet(String),
    TorrentBytes {
        bytes: Arc<Vec<u8>>,
        name: Option<String>,
    },
    TorrentUrl {
        url: String,
        cookies: Option<String>,
        referrer: Option<String>,
        user_agent: Option<String>,
    },
    Path(PathBuf),
}

impl AddSource {
    /// Short human description (dialog subtitle while resolving).
    pub fn describe(&self) -> String {
        match self {
            AddSource::Magnet(m) => crate::magnet::parse(m)
                .ok()
                .and_then(|m| m.name)
                .unwrap_or_else(|| "Magnet link".to_owned()),
            AddSource::TorrentBytes { name, .. } => {
                name.clone().unwrap_or_else(|| "Torrent file".to_owned())
            }
            AddSource::TorrentUrl { url, .. } => url.clone(),
            AddSource::Path(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    pub fn is_magnet(&self) -> bool {
        matches!(self, AddSource::Magnet(_))
    }
}

/// Metadata of a torrent that is about to be added.
#[derive(Clone, Debug)]
pub struct ResolvedTorrent {
    pub info_hash: String,
    pub name: String,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub piece_length: u32,
    pub pieces: u32,
    pub private: bool,
    pub trackers: Vec<String>,
    pub torrent_bytes: Arc<Vec<u8>>,
    pub seen_peers: Vec<SocketAddr>,
    /// The torrent is already in the session.
    pub already_added: bool,
}

/// One-shot notifications from the engine to the UI.
#[derive(Clone, Debug)]
pub enum EngineEvent {
    Resolved {
        req: u64,
        result: Result<ResolvedTorrent, String>,
    },
    Added {
        name: String,
    },
    Completed {
        name: String,
    },
    Info(String),
    Error(String),
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Share ratio; 0 when nothing was downloaded yet.
pub fn ratio(uploaded: u64, downloaded: u64) -> f64 {
    if downloaded == 0 {
        0.0
    } else {
        uploaded as f64 / downloaded as f64
    }
}

pub fn format_ratio(r: f64) -> String {
    if !r.is_finite() {
        "∞".to_owned()
    } else {
        format!("{r:.2}")
    }
}

/// `0.4217` → `"42.2%"`, `1.0` → `"100%"`.
pub fn format_percent(fraction: f32) -> String {
    let pct = (fraction.clamp(0.0, 1.0) * 100.0) as f64;
    if pct >= 99.95 {
        if fraction >= 1.0 { "100%".to_owned() } else { "99.9%".to_owned() }
    } else {
        format!("{pct:.1}%")
    }
}

/// `"12 (48)"` = 12 connected out of 48 known peers.
pub fn format_peers(live: u32, seen: u32) -> String {
    if seen > live {
        format!("{live} ({seen})")
    } else {
        live.to_string()
    }
}

/// Short one-line version of an (anyhow) error message.
pub fn clean_error(e: &str) -> String {
    let mut out = String::with_capacity(e.len().min(400));
    for part in e.split('\n').map(str::trim).filter(|p| !p.is_empty()) {
        if part.eq_ignore_ascii_case("caused by:") || part.starts_with("Stack backtrace") {
            continue;
        }
        // anyhow's "Caused by" list is numbered: "0: message".
        let part = match part.split_once(':') {
            Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest.trim(),
            _ => part,
        };
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str(": ");
        }
        out.push_str(part);
        if out.len() > 300 {
            break;
        }
    }
    if out.len() > 300 {
        let mut cut = 300;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_states() {
        assert_eq!(classify(EngineState::Live, false), Status::Downloading);
        assert_eq!(classify(EngineState::Live, true), Status::Seeding);
        assert_eq!(classify(EngineState::Paused, false), Status::Paused);
        assert_eq!(classify(EngineState::Paused, true), Status::Completed);
        assert_eq!(classify(EngineState::Initializing, true), Status::Checking);
        assert_eq!(classify(EngineState::Error, true), Status::Error);
    }

    #[test]
    fn status_labels() {
        assert_eq!(Status::Downloading.label(), "Downloading");
        assert_eq!(Status::Metadata.api_name(), "metadata");
        assert!(Status::Seeding.can_pause());
        assert!(!Status::Seeding.can_resume());
        assert!(Status::Error.can_resume());
    }

    #[test]
    fn filters() {
        assert!(Filter::Downloading.matches(Status::Metadata));
        assert!(Filter::Downloading.matches(Status::Checking));
        assert!(Filter::Completed.matches(Status::Seeding));
        assert!(Filter::Completed.matches(Status::Completed));
        assert!(!Filter::Seeding.matches(Status::Completed));
        assert!(Filter::All.matches(Status::Error));
        assert!(!Filter::Paused.matches(Status::Completed));
    }

    #[test]
    fn formatting() {
        assert_eq!(format_ratio(ratio(150, 100)), "1.50");
        assert_eq!(format_ratio(ratio(10, 0)), "0.00");
        assert_eq!(format_percent(0.4217), "42.2%");
        assert_eq!(format_percent(1.0), "100%");
        assert_eq!(format_percent(0.99999), "99.9%");
        assert_eq!(format_percent(0.0), "0.0%");
        assert_eq!(format_peers(12, 48), "12 (48)");
        assert_eq!(format_peers(3, 0), "3");
    }

    #[test]
    fn clean_errors() {
        let e = "error adding torrent\n\nCaused by:\n    0: error decoding torrent\n    1: unexpected EOF";
        assert_eq!(
            clean_error(e),
            "error adding torrent: error decoding torrent: unexpected EOF"
        );
        let long = "x".repeat(1000);
        assert!(clean_error(&long).chars().count() <= 302);
    }
}
