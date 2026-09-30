//! `ZENLESS_DEMO=1`: fills the UI with sample torrents (no network, no disk
//! writes, no API server) so the app can be screenshotted populated.
//!
//! `ZENLESS_DEMO_VIEW` picks an extra state to show: `add` (Add dialog),
//! `magnet`, `settings`, `settings-network`, `settings-browser`,
//! `settings-appearance`, `about`, `files`, `peers`, `trackers`, `speed`,
//! `remove`, `empty` or `toast`. `ZENLESS_DEMO_THEME=<built-in theme name>`
//! applies a theme in memory only (the shared appearance.json is untouched).

use crate::model::{
    EngineStatus, FileEntry, PeerView, ResolvedTorrent, Snapshot, Status, TorrentView,
};
use crate::shared::kit::unix_now;
use std::{path::PathBuf, sync::Arc};

pub fn enabled() -> bool {
    std::env::var("ZENLESS_DEMO").is_ok_and(|v| !v.is_empty() && v != "0")
}

pub fn view() -> String {
    std::env::var("ZENLESS_DEMO_VIEW").unwrap_or_default().to_ascii_lowercase()
}

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

fn hash(seed: u8) -> String {
    (0..20u8)
        .map(|i| format!("{:02x}", i.wrapping_mul(37).wrapping_add(seed.wrapping_mul(91)) ^ seed))
        .collect()
}

fn files(list: &[(&str, u64)]) -> Arc<Vec<FileEntry>> {
    Arc::new(
        list.iter()
            .map(|(p, len)| FileEntry {
                path: p.split('/').map(str::to_owned).collect(),
                len: *len,
                padding: false,
            })
            .collect(),
    )
}

struct Row {
    name: &'static str,
    status: Status,
    size: u64,
    progress: f64,
    down: f64,
    up: f64,
    peers: (u32, u32),
    ratio: f64,
    age_h: u64,
    files: Arc<Vec<FileEntry>>,
    error: Option<&'static str>,
}

pub fn snapshot() -> Snapshot {
    let now = unix_now();
    let base = dirs::download_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\you\Downloads")).join("Torrents");
    let open_movies = files(&[
        ("Big Buck Bunny/big_buck_bunny_1080p.mp4", 928 * MB),
        ("Big Buck Bunny/big_buck_bunny.srt", 42 * 1024),
        ("Sintel/sintel_1080p.mkv", 1210 * MB),
        ("Sintel/subtitles/sintel_en.srt", 38 * 1024),
        ("Sintel/subtitles/sintel_de.srt", 39 * 1024),
        ("Tears of Steel/tears_of_steel_1080p.webm", 740 * MB),
        ("Cosmos Laundromat/cosmos_laundromat_1080p.mp4", 1480 * MB),
        ("poster.jpg", 2 * MB),
        ("README.txt", 3 * 1024),
    ]);
    let rows = vec![
        Row { name: "Blender Open Movies Collection (1080p)", status: Status::Downloading, size: 4368 * MB, progress: 0.573, down: 9.4 * MB as f64, up: 1.1 * MB as f64, peers: (38, 212), ratio: 0.21, age_h: 1, files: open_movies, error: None },
        Row { name: "ubuntu-24.04.1-desktop-amd64.iso", status: Status::Downloading, size: 6_114 * MB, progress: 0.318, down: 21.7 * MB as f64, up: 2.4 * MB as f64, peers: (61, 540), ratio: 0.05, age_h: 2, files: files(&[("ubuntu-24.04.1-desktop-amd64.iso", 6_114 * MB)]), error: None },
        Row { name: "debian-12.7.0-amd64-netinst.iso", status: Status::Seeding, size: 631 * MB, progress: 1.0, down: 0.0, up: 640.0 * 1024.0, peers: (9, 57), ratio: 2.84, age_h: 30, files: files(&[("debian-12.7.0-amd64-netinst.iso", 631 * MB)]), error: None },
        Row { name: "Fedora-Workstation-Live-x86_64-40-1.14.iso", status: Status::Downloading, size: 2_190 * MB, progress: 0.914, down: 5.2 * MB as f64, up: 0.8 * MB as f64, peers: (22, 131), ratio: 0.12, age_h: 3, files: files(&[("Fedora-Workstation-Live-x86_64-40-1.14.iso", 2_190 * MB)]), error: None },
        Row { name: "archlinux-2024.09.01-x86_64.iso", status: Status::Checking, size: 1_180 * MB, progress: 0.44, down: 0.0, up: 0.0, peers: (0, 0), ratio: 0.0, age_h: 5, files: files(&[("archlinux-2024.09.01-x86_64.iso", 1_180 * MB)]), error: None },
        Row { name: "Tears of Steel 4K", status: Status::Metadata, size: 0, progress: 0.0, down: 0.0, up: 0.0, peers: (0, 0), ratio: 0.0, age_h: 0, files: Arc::new(Vec::new()), error: None },
        Row { name: "LibreOffice_24.8.1_Win_x86-64.msi", status: Status::Paused, size: 346 * MB, progress: 0.27, down: 0.0, up: 0.0, peers: (0, 0), ratio: 0.0, age_h: 26, files: files(&[("LibreOffice_24.8.1_Win_x86-64.msi", 346 * MB)]), error: None },
        Row { name: "Sintel (2010) 4K Director's Cut", status: Status::Completed, size: 3 * GB + 600 * MB, progress: 1.0, down: 0.0, up: 0.0, peers: (0, 0), ratio: 1.02, age_h: 76, files: files(&[("Sintel.2010.4K.mkv", 3 * GB + 600 * MB)]), error: None },
        Row { name: "Cosmos Laundromat (2015)", status: Status::Seeding, size: 1_480 * MB, progress: 1.0, down: 0.0, up: 1.9 * MB as f64, peers: (14, 88), ratio: 1.37, age_h: 50, files: files(&[("cosmos_laundromat.mp4", 1_480 * MB)]), error: None },
        Row { name: "Wikipedia offline dump (en, 2024-09)", status: Status::Error, size: 102 * GB, progress: 0.08, down: 0.0, up: 0.0, peers: (0, 0), ratio: 0.0, age_h: 120, files: files(&[("wikipedia_en_all_maxi_2024-09.zim", 102 * GB)]), error: Some("Not enough space on D:\\ to allocate 102 GB") },
    ];

    let torrents: Vec<TorrentView> = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let h = hash(i as u8 + 1);
            let mut v = TorrentView::placeholder(h.clone(), r.name, r.status);
            v.info_hash = h;
            v.total_bytes = r.size;
            v.full_size = r.size;
            v.have_bytes = (r.size as f64 * r.progress) as u64;
            v.uploaded = (v.have_bytes as f64 * r.ratio) as u64;
            v.down_bps = r.down;
            v.up_bps = r.up;
            v.eta_secs = (r.down > 0.0).then(|| (r.size - v.have_bytes) as f64 / r.down);
            v.peers_live = r.peers.0;
            v.peers_seen = r.peers.1;
            v.peers_connecting = r.peers.0 / 4;
            v.added_at = now - r.age_h * 3600 - 1800;
            v.completed_at = (r.progress >= 1.0).then(|| now - r.age_h * 1800);
            v.save_path = if r.files.len() > 1 { base.join(r.name) } else { base.clone() };
            v.piece_length = if r.size > 2 * GB { 4 * MB as u32 } else { 2 * MB as u32 };
            v.pieces = r.size.div_ceil(v.piece_length as u64) as u32;
            v.error = r.error.map(str::to_owned);
            v.pending = r.status == Status::Metadata;
            // Per-file progress: earlier files further along.
            let n = r.files.len().max(1) as f64;
            v.file_progress = r
                .files
                .iter()
                .enumerate()
                .map(|(fi, f)| {
                    let frac = (r.progress * 1.6 - fi as f64 / n * 0.9).clamp(0.0, 1.0);
                    (f.len as f64 * frac) as u64
                })
                .collect();
            v.included = r.files.iter().map(|f| !f.path.last().is_some_and(|n| n.ends_with(".txt"))).collect();
            v.files = r.files;
            v.trackers = Arc::new(vec![
                "udp://tracker.opentrackr.org:1337/announce".into(),
                "udp://open.stealth.si:80/announce".into(),
                "https://torrent.ubuntu.com/announce".into(),
                "wss://tracker.webtorrent.dev".into(),
            ]);
            v
        })
        .collect();

    let down: f64 = torrents.iter().map(|t| t.down_bps).sum();
    let up: f64 = torrents.iter().map(|t| t.up_bps).sum();
    let peers = vec![
        peer("185.21.216.143:51413", "Transmission 4.0.6", "TCP", 3.1, 0.2),
        peer("[2a01:4f8:c17:1f::2]:6881", "qBittorrent 5.0.0", "TCP", 2.4, 0.3),
        peer("94.23.57.12:49160", "Deluge 2.1.1", "TCP", 1.2, 0.1),
        peer("51.158.144.7:6881", "rqbit 9.0.1", "UTP", 0.9, 0.2),
        peer("176.9.41.200:55028", "libtorrent (Rasterbar) 2.0.10", "TCP", 0.8, 0.1),
        peer("62.210.95.118:51820", "BiglyBT 3.6.0.0", "TCP", 0.6, 0.0),
        peer("5.9.83.14:6889", "µTorrent 3.6", "UTP", 0.3, 0.2),
    ];
    let focus = torrents[0].key.clone();
    Snapshot {
        generation: 1,
        engine: EngineStatus::Demo,
        torrents,
        down_bps: down,
        up_bps: up,
        dht_nodes: Some(342),
        listen_port: Some(6881),
        peers: Some((focus, peers)),
        notice: None,
    }
}

fn peer(addr: &str, client: &str, kind: &str, down_mb: f64, up_mb: f64) -> PeerView {
    PeerView {
        addr: addr.into(),
        client: client.into(),
        kind: kind.into(),
        downloaded: (down_mb * 180.0 * MB as f64) as u64,
        uploaded: (up_mb * 120.0 * MB as f64) as u64,
        down_bps: down_mb * MB as f64,
        up_bps: up_mb * MB as f64,
    }
}

/// Smooth-ish fake speed history (bytes/s) for the graphs.
pub fn history(len: usize, level: f64) -> Vec<(f32, f32)> {
    (0..len)
        .map(|i| {
            let t = i as f64;
            let d = level * (0.72 + 0.18 * (t / 7.0).sin() + 0.08 * (t / 2.3).cos() + 0.02 * ((t * 1.7).sin()));
            let u = level * 0.14 * (0.8 + 0.2 * (t / 5.0).cos());
            (d.max(0.0) as f32, u.max(0.0) as f32)
        })
        .collect()
}

/// Sample for the Add dialog.
pub fn resolved() -> ResolvedTorrent {
    let files = [
        ("Blender Open Movies/Big Buck Bunny/big_buck_bunny_1080p.mp4", 928 * MB),
        ("Blender Open Movies/Big Buck Bunny/big_buck_bunny.srt", 42 * 1024),
        ("Blender Open Movies/Sintel/sintel_1080p.mkv", 1210 * MB),
        ("Blender Open Movies/Sintel/subtitles/sintel_en.srt", 38 * 1024),
        ("Blender Open Movies/Tears of Steel/tears_of_steel_1080p.webm", 740 * MB),
        ("Blender Open Movies/poster.jpg", 2 * MB),
        ("Blender Open Movies/README.txt", 3 * 1024),
    ];
    let files: Vec<FileEntry> = files
        .iter()
        .map(|(p, l)| FileEntry {
            path: p.split('/').skip(1).map(str::to_owned).collect(),
            len: *l,
            padding: false,
        })
        .collect();
    ResolvedTorrent {
        info_hash: hash(77),
        name: "Blender Open Movies".into(),
        total_size: files.iter().map(|f| f.len).sum(),
        files,
        piece_length: 2 * MB as u32,
        pieces: 1437,
        private: false,
        trackers: vec!["udp://tracker.opentrackr.org:1337/announce".into()],
        torrent_bytes: Arc::new(Vec::new()),
        seen_peers: Vec::new(),
        already_added: false,
    }
}
