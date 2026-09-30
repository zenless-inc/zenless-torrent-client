//! BitTorrent engine: owns the librqbit [`Session`] on a private tokio runtime.
//!
//! The UI talks to it only through [`Command`]s and reads immutable
//! [`Snapshot`]s (republished about once per second), so nothing on the UI
//! thread ever waits for the network or the disk.

use crate::model::{
    AddSource, EngineEvent, EngineState, EngineStatus, FileEntry, PeerView, ResolvedTorrent,
    Snapshot, Status, TorrentView, classify, clean_error,
};
use crate::shared::kit::unix_now;
use crate::store::{PendingMagnet, Store};
use anyhow::{Context as _, bail};
use eframe::egui;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode,
    ListenerOptions, ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
    TorrentStatsState, api::TorrentIdOrHash, dht::DhtPersistenceConfig, dht::Id20,
    limits::LimitsConfig,
};
use std::{
    collections::{HashMap, HashSet},
    net::{Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::AbortHandle};

/// Maximum size of a downloaded .torrent file.
const MAX_TORRENT_FILE: usize = 32 * 1024 * 1024;
/// How long a magnet added with "start immediately" may take to find metadata.
const BACKGROUND_RESOLVE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Everything needed to (re)start the librqbit session.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    pub download_dir: PathBuf,
    pub session_dir: PathBuf,
    pub listen_port: u16,
    pub dht: bool,
    pub upnp: bool,
    pub utp: bool,
    pub download_limit_bps: Option<u32>,
    pub upload_limit_bps: Option<u32>,
}

/// A fully resolved torrent to add to the session.
#[derive(Clone, Debug)]
pub struct AddParams {
    pub torrent_bytes: Arc<Vec<u8>>,
    pub name: String,
    /// Exact folder the torrent's files go into (see [`output_folder_for`]).
    pub output_folder: PathBuf,
    pub only_files: Option<Vec<usize>>,
    pub paused: bool,
    pub initial_peers: Vec<SocketAddr>,
}

pub enum Command {
    /// Fetch metadata for the "Add torrent" dialog (never adds anything).
    Resolve { req: u64, source: AddSource },
    CancelResolve(u64),
    Add(AddParams),
    /// Resolve in the background and add with all files (API `mode: start`).
    AddDirect {
        source: AddSource,
        download_dir: PathBuf,
        paused: bool,
    },
    Pause(Vec<String>),
    Resume(Vec<String>),
    Remove { keys: Vec<String>, delete_files: bool },
    Recheck(Vec<String>),
    SetFiles { key: String, files: Vec<usize> },
    SetLimits {
        download_bps: Option<u32>,
        upload_bps: Option<u32>,
    },
    Restart(EngineConfig),
    /// Torrent whose peer list should be included in snapshots.
    Focus(Option<String>),
    Shutdown,
}

/// Where a torrent's files go: multi-file torrents get their own sub-folder
/// named after the torrent (like librqbit's default), single files don't.
pub fn output_folder_for(base: &Path, name: &str, file_count: usize) -> PathBuf {
    if file_count > 1 {
        base.join(sanitize_name(name))
    } else {
        base.to_path_buf()
    }
}

/// Makes a torrent name safe to use as a Windows folder name.
pub fn sanitize_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    let s = s.trim_start().to_owned();
    if s.is_empty() {
        return "torrent".to_owned();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        format!("_{s}")
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Handle used by the UI
// ---------------------------------------------------------------------------

pub type SharedSnapshot = Arc<Mutex<Arc<Snapshot>>>;

pub struct EngineHandle {
    tx: mpsc::UnboundedSender<Command>,
    shared: SharedSnapshot,
    events: std::sync::mpsc::Receiver<EngineEvent>,
    done: Option<std::sync::mpsc::Receiver<()>>,
}

impl EngineHandle {
    /// Starts the engine thread (and its tokio runtime).
    pub fn start(cfg: EngineConfig, ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared: SharedSnapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let (ev_tx, ev_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread_shared = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("zenless-engine".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .max_blocking_threads(16)
                    .enable_all()
                    .thread_name("zenless-rt")
                    .build();
                match runtime {
                    Ok(rt) => {
                        let engine = Engine::new(cfg, thread_shared, ev_tx, ctx);
                        rt.block_on(engine.run(rx));
                        rt.shutdown_timeout(Duration::from_secs(2));
                    }
                    Err(e) => set_failed(&thread_shared, &ctx, format!("could not start runtime: {e}")),
                }
                let _ = done_tx.send(());
            });
        if let Err(e) = spawned {
            set_failed(&shared, &egui::Context::default(), format!("could not start engine: {e}"));
        }
        Self {
            tx,
            shared,
            events: ev_rx,
            done: Some(done_rx),
        }
    }

    /// A handle without a session (demo mode); the snapshot is set by the caller.
    pub fn offline(snapshot: Snapshot) -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_ev_tx, ev_rx) = std::sync::mpsc::channel();
        Self {
            tx,
            shared: Arc::new(Mutex::new(Arc::new(snapshot))),
            events: ev_rx,
            done: None,
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn shared(&self) -> SharedSnapshot {
        self.shared.clone()
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_snapshot(&self, snapshot: Snapshot) {
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(snapshot);
    }

    pub fn try_event(&self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }

    /// Stops the session (pausing torrents, flushing state) and waits up to `timeout`.
    pub fn shutdown(&mut self, timeout: Duration) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(timeout);
        }
    }
}

fn set_failed(shared: &SharedSnapshot, ctx: &egui::Context, msg: String) {
    let snap = Snapshot {
        engine: EngineStatus::Failed(msg),
        ..Default::default()
    };
    *shared.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(snap);
    ctx.request_repaint();
}

// ---------------------------------------------------------------------------
// Engine internals (run on the tokio runtime)
// ---------------------------------------------------------------------------

/// Static per-torrent info, computed once.
struct StaticInfo {
    name: String,
    files: Arc<Vec<FileEntry>>,
    trackers: Arc<Vec<String>>,
    pieces: u32,
    piece_length: u32,
    private: bool,
    full_size: u64,
}

impl StaticInfo {
    fn of(h: &ManagedTorrent, hash: &str) -> Self {
        let name = h.name().unwrap_or_else(|| hash.to_owned());
        let mut trackers: Vec<String> = h.shared().trackers.iter().map(|u| u.to_string()).collect();
        trackers.sort();
        let meta = h
            .with_metadata(|m| {
                let files: Vec<FileEntry> = m
                    .file_infos
                    .iter()
                    .map(|fi| FileEntry {
                        path: fi
                            .relative_filename
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect(),
                        len: fi.len,
                        padding: fi.attrs.padding,
                    })
                    .collect();
                let lengths = m.info.lengths();
                (
                    files,
                    lengths.total_pieces(),
                    lengths.default_piece_length(),
                    m.info.info().private,
                    lengths.total_length(),
                )
            })
            .unwrap_or_default();
        Self {
            name,
            files: Arc::new(meta.0),
            trackers: Arc::new(trackers),
            pieces: meta.1,
            piece_length: meta.2,
            private: meta.3,
            full_size: meta.4,
        }
    }
}

/// librqbit's upload counter restarts whenever a torrent goes live again; this
/// keeps a lifetime total.
#[derive(Clone, Copy, Default)]
struct UploadTrack {
    base: u64,
    last: u64,
}

/// A magnet / URL being resolved in the background before it is added.
struct Pending {
    id: u64,
    source: AddSource,
    download_dir: PathBuf,
    paused: bool,
    name: String,
    info_hash: String,
    added_at: u64,
    error: Option<String>,
    task: Option<AbortHandle>,
}

impl Pending {
    fn key(&self) -> String {
        format!("pending-{}", self.id)
    }

    fn view(&self) -> TorrentView {
        let status = if self.error.is_some() {
            Status::Error
        } else {
            Status::Metadata
        };
        let mut v = TorrentView::placeholder(self.key(), self.name.clone(), status);
        v.info_hash = self.info_hash.clone();
        v.error = self.error.clone();
        v.added_at = self.added_at;
        v.save_path = self.download_dir.clone();
        v.pending = true;
        v
    }
}

/// Messages from spawned tasks back to the engine loop.
enum Internal {
    Added { pending: Option<u64> },
    PendingFailed { id: u64, error: String },
    ResolveDone(u64),
    Removed { key: String },
}

struct Engine {
    cfg: EngineConfig,
    session: Option<Arc<Session>>,
    status: EngineStatus,
    notice: Option<String>,
    shared: SharedSnapshot,
    events: std::sync::mpsc::Sender<EngineEvent>,
    ctx: egui::Context,
    internal_tx: mpsc::UnboundedSender<Internal>,
    internal_rx: Option<mpsc::UnboundedReceiver<Internal>>,
    store: Store,
    statics: HashMap<String, Arc<StaticInfo>>,
    prev: HashMap<String, (EngineState, bool)>,
    uploads: HashMap<String, UploadTrack>,
    pending: Vec<Pending>,
    next_pending: u64,
    resolves: HashMap<u64, AbortHandle>,
    focus: Option<String>,
    peer_prev: HashMap<String, (u64, u64)>,
    peer_prev_at: Instant,
    generation: u64,
    http: reqwest::Client,
}

impl Engine {
    fn new(
        cfg: EngineConfig,
        shared: SharedSnapshot,
        events: std::sync::mpsc::Sender<EngineEvent>,
        ctx: egui::Context,
    ) -> Self {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let store = Store::load(crate::settings::data_dir().join("torrents.json"));
        let http = reqwest::Client::builder()
            .user_agent(concat!("ZenlessTorrent/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.url().scheme() == "magnet" {
                    attempt.stop()
                } else if attempt.previous().len() > 10 {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            cfg,
            session: None,
            status: EngineStatus::Starting,
            notice: None,
            shared,
            events,
            ctx,
            internal_tx,
            internal_rx: Some(internal_rx),
            store,
            statics: HashMap::new(),
            prev: HashMap::new(),
            uploads: HashMap::new(),
            pending: Vec::new(),
            next_pending: 1,
            resolves: HashMap::new(),
            focus: None,
            peer_prev: HashMap::new(),
            peer_prev_at: Instant::now(),
            generation: 0,
            http,
        }
    }

    async fn run(mut self, mut cmd_rx: mpsc::UnboundedReceiver<Command>) {
        let Some(mut internal_rx) = self.internal_rx.take() else {
            return;
        };
        self.publish();
        self.start_session().await;
        // Magnets that were still resolving when the app closed.
        for p in self.store.data.pending.clone() {
            let PendingMagnet {
                magnet,
                output_dir,
                paused,
                added_at,
            } = p;
            self.queue_pending(AddSource::Magnet(magnet), output_dir, paused, added_at, false);
        }
        self.publish();

        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut save_tick = tokio::time::interval(Duration::from_secs(30));
        save_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => match cmd {
                    None | Some(Command::Shutdown) => break,
                    Some(cmd) => {
                        self.handle(cmd).await;
                        self.publish();
                    }
                },
                Some(msg) = internal_rx.recv() => {
                    self.handle_internal(msg);
                    self.publish();
                }
                _ = tick.tick() => self.publish(),
                _ = save_tick.tick() => {
                    self.sync_uploads_to_store();
                    self.store.save_if_dirty();
                }
            }
        }
        self.shutdown().await;
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
        self.ctx.request_repaint();
    }

    // -- session lifecycle -------------------------------------------------

    async fn start_session(&mut self) {
        self.status = EngineStatus::Starting;
        self.notice = None;
        self.publish();
        if let Err(e) = std::fs::create_dir_all(&self.cfg.download_dir) {
            tracing::warn!("could not create {}: {e}", self.cfg.download_dir.display());
        }
        let port = self.cfg.listen_port;
        let result = match create_session(&self.cfg, port).await {
            Ok(s) => Ok(s),
            Err(e) if port != 0 => {
                tracing::warn!("session start on port {port} failed: {e:#}; retrying on a random port");
                let first = clean_error(&format!("{e:#}"));
                create_session(&self.cfg, 0).await.map(|s| {
                    self.notice = Some(format!(
                        "Port {port} could not be used ({first}); using a random port instead."
                    ));
                    s
                })
            }
            Err(e) => Err(e),
        };
        match result {
            Ok(session) => {
                tracing::info!(
                    "session started, listening on {:?}",
                    session.listen_addr()
                );
                self.session = Some(session);
                self.status = EngineStatus::Running;
            }
            Err(e) => {
                let msg = clean_error(&format!("{e:#}"));
                tracing::error!("could not start session: {msg}");
                self.status = EngineStatus::Failed(msg.clone());
                self.emit(EngineEvent::Error(format!("The BitTorrent engine could not start: {msg}")));
            }
        }
    }

    async fn stop_session(&mut self) {
        for (_, h) in self.resolves.drain() {
            h.abort();
        }
        for p in &mut self.pending {
            if let Some(t) = p.task.take() {
                t.abort();
            }
        }
        self.sync_uploads_to_store();
        if let Some(session) = self.session.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), session.stop()).await;
        }
        self.prev.clear();
    }

    async fn shutdown(&mut self) {
        self.stop_session().await;
        self.store.save();
        tracing::info!("engine stopped");
    }

    // -- commands -----------------------------------------------------------

    async fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Resolve { req, source } => self.spawn_resolve(req, source),
            Command::CancelResolve(req) => {
                if let Some(h) = self.resolves.remove(&req) {
                    h.abort();
                }
            }
            Command::Add(params) => self.spawn_add(params, None),
            Command::AddDirect {
                source,
                download_dir,
                paused,
            } => self.queue_pending(source, download_dir, paused, unix_now(), true),
            Command::Pause(keys) => {
                let Some(session) = self.session.clone() else { return };
                for key in keys {
                    if let Some(h) = find(&session, &key)
                        && let Err(e) = session.pause(&h).await
                    {
                        tracing::debug!("pause {key}: {e:#}");
                    }
                }
            }
            Command::Resume(keys) => {
                let Some(session) = self.session.clone() else { return };
                for key in keys {
                    if let Some(idx) = self.pending.iter().position(|p| p.key() == key) {
                        if self.pending[idx].error.is_some() {
                            self.pending[idx].error = None;
                            self.spawn_pending(idx);
                        }
                        continue;
                    }
                    if let Some(h) = find(&session, &key)
                        && let Err(e) = session.unpause(&h).await
                    {
                        self.emit(EngineEvent::Error(format!(
                            "Could not resume {}: {}",
                            h.name().unwrap_or(key),
                            clean_error(&format!("{e:#}"))
                        )));
                    }
                }
            }
            Command::Remove { keys, delete_files } => self.remove(keys, delete_files),
            Command::Recheck(keys) => self.recheck(keys),
            Command::SetFiles { key, files } => {
                let Some(session) = self.session.clone() else { return };
                if let Some(h) = find(&session, &key) {
                    let set: HashSet<usize> = files.into_iter().collect();
                    if set.is_empty() {
                        self.emit(EngineEvent::Error("Select at least one file.".into()));
                    } else if let Err(e) = session.update_only_files(&h, &set).await {
                        self.emit(EngineEvent::Error(format!(
                            "Could not change the file selection: {}",
                            clean_error(&format!("{e:#}"))
                        )));
                    }
                }
            }
            Command::SetLimits {
                download_bps,
                upload_bps,
            } => {
                self.cfg.download_limit_bps = download_bps;
                self.cfg.upload_limit_bps = upload_bps;
                if let Some(s) = &self.session {
                    s.ratelimits.set_download_bps(download_bps.and_then(NonZeroU32::new));
                    s.ratelimits.set_upload_bps(upload_bps.and_then(NonZeroU32::new));
                }
            }
            Command::Restart(cfg) => {
                self.status = EngineStatus::Restarting;
                self.publish();
                self.stop_session().await;
                tokio::time::sleep(Duration::from_millis(500)).await;
                self.cfg = cfg;
                self.start_session().await;
                for idx in 0..self.pending.len() {
                    if self.pending[idx].error.is_none() {
                        self.spawn_pending(idx);
                    }
                }
            }
            Command::Focus(key) => {
                if self.focus != key {
                    self.focus = key;
                    self.peer_prev.clear();
                    self.peer_prev_at = Instant::now();
                }
            }
            Command::Shutdown => {}
        }
    }

    fn handle_internal(&mut self, msg: Internal) {
        match msg {
            Internal::ResolveDone(req) => {
                self.resolves.remove(&req);
            }
            Internal::Added { pending } => {
                if let Some(id) = pending {
                    self.drop_pending(id);
                }
            }
            Internal::PendingFailed { id, error } => {
                if let Some(p) = self.pending.iter_mut().find(|p| p.id == id) {
                    p.task = None;
                    p.error = Some(error);
                }
            }
            Internal::Removed { key } => {
                self.store.remove(&key);
                self.statics.remove(&key);
                self.uploads.remove(&key);
                self.prev.remove(&key);
            }
        }
    }

    fn spawn_resolve(&mut self, req: u64, source: AddSource) {
        let Some(session) = self.session.clone() else {
            self.emit(EngineEvent::Resolved {
                req,
                result: Err("The BitTorrent engine is not running.".into()),
            });
            return;
        };
        let http = self.http.clone();
        let events = self.events.clone();
        let internal = self.internal_tx.clone();
        let ctx = self.ctx.clone();
        let task = tokio::spawn(async move {
            let result = resolve(&session, &http, source)
                .await
                .map_err(|e| clean_error(&format!("{e:#}")));
            let _ = events.send(EngineEvent::Resolved { req, result });
            let _ = internal.send(Internal::ResolveDone(req));
            ctx.request_repaint();
        });
        self.resolves.insert(req, task.abort_handle());
    }

    fn spawn_add(&mut self, params: AddParams, pending: Option<u64>) {
        let Some(session) = self.session.clone() else {
            self.emit(EngineEvent::Error("The BitTorrent engine is not running.".into()));
            return;
        };
        let now = unix_now();
        if let Ok(meta) = librqbit::torrent_from_bytes(&params.torrent_bytes) {
            // Record the "added" time before the first snapshot shows the torrent.
            self.store.meta(&meta.info_hash.as_string(), now);
        }
        let events = self.events.clone();
        let internal = self.internal_tx.clone();
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            match add_resolved(&session, params).await {
                Ok((name, fresh)) => {
                    let _ = events.send(if fresh {
                        EngineEvent::Added { name }
                    } else {
                        EngineEvent::Info(format!("\"{name}\" is already in your list."))
                    });
                    let _ = internal.send(Internal::Added { pending });
                }
                Err(e) => {
                    let msg = clean_error(&format!("{e:#}"));
                    match pending {
                        Some(id) => {
                            let _ = internal.send(Internal::PendingFailed { id, error: msg });
                        }
                        None => {
                            let _ = events.send(EngineEvent::Error(format!("Could not add torrent: {msg}")));
                        }
                    }
                }
            }
            ctx.request_repaint();
        });
    }

    /// Adds a background-resolving entry (shown with status "Metadata").
    fn queue_pending(
        &mut self,
        source: AddSource,
        download_dir: PathBuf,
        paused: bool,
        added_at: u64,
        persist: bool,
    ) {
        let (name, info_hash) = match &source {
            AddSource::Magnet(m) => match crate::magnet::parse(m) {
                Ok(info) => (
                    info.name.clone().unwrap_or_else(|| info.info_hash.clone()),
                    info.info_hash,
                ),
                Err(e) => {
                    self.emit(EngineEvent::Error(format!("Invalid magnet link: {e}")));
                    return;
                }
            },
            other => (other.describe(), String::new()),
        };
        if !info_hash.is_empty()
            && (self.pending.iter().any(|p| p.info_hash == info_hash)
                || self.session.as_ref().and_then(|s| find(s, &info_hash)).is_some())
        {
            if persist {
                self.emit(EngineEvent::Info(format!("\"{name}\" is already in your list.")));
            } else if let AddSource::Magnet(m) = &source {
                // A restored entry that was added after all: forget it.
                self.store.data.pending.retain(|x| &x.magnet != m);
                self.store.mark_dirty();
            }
            return;
        }
        if persist && let AddSource::Magnet(m) = &source {
            self.store.data.pending.push(PendingMagnet {
                magnet: m.clone(),
                output_dir: download_dir.clone(),
                paused,
                added_at,
            });
            self.store.mark_dirty();
        }
        let id = self.next_pending;
        self.next_pending += 1;
        self.pending.push(Pending {
            id,
            source,
            download_dir,
            paused,
            name,
            info_hash,
            added_at,
            error: None,
            task: None,
        });
        self.spawn_pending(self.pending.len() - 1);
    }

    fn spawn_pending(&mut self, idx: usize) {
        let Some(session) = self.session.clone() else {
            self.pending[idx].error = Some("The BitTorrent engine is not running.".into());
            return;
        };
        let p = &self.pending[idx];
        let (id, source, download_dir, paused) = (p.id, p.source.clone(), p.download_dir.clone(), p.paused);
        let http = self.http.clone();
        let events = self.events.clone();
        let internal = self.internal_tx.clone();
        let ctx = self.ctx.clone();
        let task = tokio::spawn(async move {
            let outcome = match tokio::time::timeout(
                BACKGROUND_RESOLVE_TIMEOUT,
                resolve(&session, &http, source),
            )
            .await
            {
                Err(_) => Err("No peer sent the torrent's metadata in time. Resume to try again.".to_owned()),
                Ok(Err(e)) => Err(clean_error(&format!("{e:#}"))),
                Ok(Ok(r)) if r.already_added => {
                    let _ = events.send(EngineEvent::Info(format!("\"{}\" is already in your list.", r.name)));
                    Ok(())
                }
                Ok(Ok(r)) => {
                    let params = AddParams {
                        output_folder: output_folder_for(&download_dir, &r.name, r.files.len()),
                        name: r.name,
                        torrent_bytes: r.torrent_bytes,
                        only_files: None,
                        paused,
                        initial_peers: r.seen_peers,
                    };
                    match add_resolved(&session, params).await {
                        Ok((name, _)) => {
                            let _ = events.send(EngineEvent::Added { name });
                            Ok(())
                        }
                        Err(e) => Err(clean_error(&format!("{e:#}"))),
                    }
                }
            };
            let _ = internal.send(match outcome {
                Ok(()) => Internal::Added { pending: Some(id) },
                Err(error) => Internal::PendingFailed { id, error },
            });
            ctx.request_repaint();
        });
        self.pending[idx].task = Some(task.abort_handle());
    }

    fn drop_pending(&mut self, id: u64) {
        if let Some(idx) = self.pending.iter().position(|p| p.id == id) {
            let p = self.pending.remove(idx);
            if let Some(t) = p.task {
                t.abort();
            }
            if let AddSource::Magnet(m) = &p.source {
                let before = self.store.data.pending.len();
                self.store.data.pending.retain(|x| &x.magnet != m);
                if self.store.data.pending.len() != before {
                    self.store.mark_dirty();
                }
            }
        }
    }

    fn remove(&mut self, keys: Vec<String>, delete_files: bool) {
        for key in keys {
            if let Some(id) = self.pending.iter().find(|p| p.key() == key).map(|p| p.id) {
                self.drop_pending(id);
                continue;
            }
            let Some(session) = self.session.clone() else { continue };
            let Ok(id) = Id20::from_str(&key) else { continue };
            let events = self.events.clone();
            let internal = self.internal_tx.clone();
            let ctx = self.ctx.clone();
            tokio::spawn(async move {
                if let Err(e) = session.delete(TorrentIdOrHash::Hash(id), delete_files).await {
                    let _ = events.send(EngineEvent::Error(format!(
                        "Could not remove torrent: {}",
                        clean_error(&format!("{e:#}"))
                    )));
                }
                let _ = internal.send(Internal::Removed { key });
                ctx.request_repaint();
            });
        }
    }

    /// librqbit has no "recheck" call: forget the torrent (keeping its files)
    /// and add it again, which verifies every piece on disk.
    fn recheck(&mut self, keys: Vec<String>) {
        let Some(session) = self.session.clone() else { return };
        for key in keys {
            let Some(h) = find(&session, &key) else { continue };
            let Ok(bytes) = h.with_metadata(|m| m.torrent_bytes.to_vec()) else { continue };
            let params = AddParams {
                torrent_bytes: Arc::new(bytes),
                name: h.name().unwrap_or_else(|| key.clone()),
                output_folder: h.output_folder().to_path_buf(),
                only_files: h.only_files(),
                paused: h.is_paused(),
                initial_peers: Vec::new(),
            };
            self.prev.remove(&key);
            let events = self.events.clone();
            let ctx = self.ctx.clone();
            let info_hash = h.info_hash();
            drop(h);
            let session = session.clone();
            tokio::spawn(async move {
                let result = async {
                    session
                        .delete(TorrentIdOrHash::Hash(info_hash), false)
                        .await
                        .context("could not detach torrent")?;
                    add_resolved(&session, params).await
                }
                .await;
                if let Err(e) = result {
                    let _ = events.send(EngineEvent::Error(format!(
                        "Recheck failed: {}",
                        clean_error(&format!("{e:#}"))
                    )));
                }
                ctx.request_repaint();
            });
        }
    }

    // -- snapshots -------------------------------------------------------------

    fn sync_uploads_to_store(&mut self) {
        for (hash, track) in &self.uploads {
            let total = track.base + track.last;
            if let Some(m) = self.store.data.torrents.get_mut(hash)
                && m.uploaded != total
            {
                m.uploaded = total;
                self.store.mark_dirty();
            }
        }
    }

    fn publish(&mut self) {
        self.generation += 1;
        let now = unix_now();
        let mut snap = Snapshot {
            generation: self.generation,
            engine: self.status.clone(),
            notice: self.notice.clone(),
            ..Default::default()
        };
        if let Some(session) = self.session.clone() {
            let mut handles: Vec<(usize, Arc<ManagedTorrent>)> =
                session.with_torrents(|it| it.map(|(id, h)| (id, h.clone())).collect());
            handles.sort_by_key(|(id, _)| *id);
            for (_, h) in &handles {
                let (view, peers) = self.view(h, now);
                if let Some(peers) = peers {
                    snap.peers = Some((view.key.clone(), peers));
                }
                snap.torrents.push(view);
            }
            let stats = session.stats_snapshot();
            snap.down_bps = stats.download_speed.as_bytes() as f64;
            snap.up_bps = stats.upload_speed.as_bytes() as f64;
            snap.dht_nodes = session.get_dht().map(|d| {
                let s = d.stats();
                s.routing_table_size + s.routing_table_size_v6
            });
            snap.listen_port = session
                .announce_port()
                .or_else(|| session.listen_addr().map(|a| a.port()));
        }
        snap.torrents.extend(self.pending.iter().map(Pending::view));
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(snap);
        self.ctx.request_repaint();
    }

    fn view(&mut self, h: &Arc<ManagedTorrent>, now: u64) -> (TorrentView, Option<Vec<PeerView>>) {
        let hash = h.info_hash().as_string();
        let info = self
            .statics
            .entry(hash.clone())
            .or_insert_with(|| Arc::new(StaticInfo::of(h, &hash)))
            .clone();
        let st = h.stats();
        let (state, finished) = match st.state {
            TorrentStatsState::Initializing { .. } => (EngineState::Initializing, false),
            TorrentStatsState::Live => (EngineState::Live, st.finished),
            TorrentStatsState::Paused => (EngineState::Paused, st.finished),
            TorrentStatsState::Error => (EngineState::Error, false),
        };
        let status = classify(state, finished);
        let live = h.live();

        let (down_bps, up_bps, eta_secs) = match &live {
            Some(l) => (
                l.down_speed_estimator().bps() as f64,
                l.up_speed_estimator().bps() as f64,
                if finished {
                    None
                } else {
                    l.down_speed_estimator().time_remaining().map(|d| d.as_secs_f64())
                },
            ),
            None => (0.0, 0.0, None),
        };
        let (peers_live, peers_connecting, peers_seen, peers_queued) = st
            .live
            .as_ref()
            .map(|l| {
                let p = &l.snapshot.peer_stats;
                (p.live, p.connecting, p.seen, p.queued)
            })
            .unwrap_or_default();

        // Lifetime upload.
        let stored_up = self.store.get(&hash).map(|m| m.uploaded).unwrap_or(0);
        let track = self.uploads.entry(hash.clone()).or_insert(UploadTrack {
            base: stored_up,
            last: 0,
        });
        if state == EngineState::Live {
            if st.uploaded_bytes < track.last {
                track.base += track.last;
            }
            track.last = st.uploaded_bytes;
        }
        let uploaded = track.base + track.last;

        // Completion notification (only for torrents we watched finishing).
        let prev = self.prev.insert(hash.clone(), (state, finished));
        let meta = self.store.meta(&hash, now);
        if prev == Some((EngineState::Live, false)) && state == EngineState::Live && finished {
            meta.completed_at = Some(now);
            let name = info.name.clone();
            self.store.mark_dirty();
            self.emit(EngineEvent::Completed { name });
        }
        let meta = self.store.meta(&hash, now).clone();

        let n = info.files.len();
        let included = match h.only_files() {
            None => vec![true; n],
            Some(list) => {
                let mut inc = vec![false; n];
                for i in list {
                    if let Some(slot) = inc.get_mut(i) {
                        *slot = true;
                    }
                }
                inc
            }
        };

        let peers = match (&self.focus, &live) {
            (Some(f), Some(l)) if *f == hash => {
                let snapshot = l.per_peer_stats_snapshot(Default::default());
                let dt = self.peer_prev_at.elapsed().as_secs_f64().max(0.25);
                let mut next = HashMap::new();
                let mut list: Vec<PeerView> = snapshot
                    .peers
                    .into_iter()
                    .map(|(addr, p)| {
                        let (f, u) = (p.counters.fetched_bytes, p.counters.uploaded_bytes);
                        let (pf, pu) = self.peer_prev.get(&addr).copied().unwrap_or((f, u));
                        next.insert(addr.clone(), (f, u));
                        PeerView {
                            client: p.client_name.unwrap_or_default(),
                            kind: p
                                .conn_kind
                                .map(|k| format!("{k:?}").to_uppercase())
                                .unwrap_or_default(),
                            addr,
                            downloaded: f,
                            uploaded: u,
                            down_bps: f.saturating_sub(pf) as f64 / dt,
                            up_bps: u.saturating_sub(pu) as f64 / dt,
                        }
                    })
                    .collect();
                list.sort_by(|a, b| {
                    b.down_bps
                        .total_cmp(&a.down_bps)
                        .then(b.downloaded.cmp(&a.downloaded))
                });
                self.peer_prev = next;
                self.peer_prev_at = Instant::now();
                Some(list)
            }
            (Some(f), None) if *f == hash => Some(Vec::new()),
            _ => None,
        };

        let view = TorrentView {
            key: hash.clone(),
            info_hash: hash,
            name: info.name.clone(),
            status,
            error: st.error.as_deref().map(clean_error),
            total_bytes: st.total_bytes,
            full_size: info.full_size,
            have_bytes: st.progress_bytes,
            uploaded,
            down_bps,
            up_bps,
            eta_secs,
            peers_live,
            peers_connecting,
            peers_seen,
            peers_queued,
            added_at: meta.added_at,
            completed_at: meta.completed_at,
            save_path: h.output_folder().to_path_buf(),
            pieces: info.pieces,
            piece_length: info.piece_length,
            private: info.private,
            files: info.files.clone(),
            file_progress: st.file_progress,
            included,
            trackers: info.trackers.clone(),
            pending: false,
        };
        (view, peers)
    }
}

fn find(session: &Session, key: &str) -> Option<Arc<ManagedTorrent>> {
    let id = Id20::from_str(key).ok()?;
    session.get(TorrentIdOrHash::Hash(id))
}

async fn create_session(cfg: &EngineConfig, port: u16) -> anyhow::Result<Arc<Session>> {
    std::fs::create_dir_all(&cfg.session_dir)
        .with_context(|| format!("could not create {}", cfg.session_dir.display()))?;
    let listen = ListenerOptions {
        mode: if cfg.utp {
            ListenerMode::TcpAndUtp
        } else {
            ListenerMode::TcpOnly
        },
        listen_addr: SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
        enable_upnp_port_forwarding: cfg.upnp,
        ..Default::default()
    };
    let dht = cfg.dht.then(|| DhtSessionConfig {
        bootstrap_addrs: None,
        port: None,
        persistence: Some(DhtPersistenceConfig {
            config_filename: Some(cfg.session_dir.join("dht.json")),
            ..Default::default()
        }),
    });
    let opts = SessionOptions {
        dht,
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json {
            folder: Some(cfg.session_dir.clone()),
        }),
        listen: Some(listen),
        ratelimits: LimitsConfig {
            download_bps: cfg.download_limit_bps.and_then(NonZeroU32::new),
            upload_bps: cfg.upload_limit_bps.and_then(NonZeroU32::new),
        },
        concurrent_init_limit: Some(2),
        client_name_and_version: Some(concat!("Zenless Torrent ", env!("CARGO_PKG_VERSION")).to_owned()),
        ..Default::default()
    };
    Session::new_with_opts(cfg.download_dir.clone(), opts).await
}

/// What a torrent URL turned out to be.
enum Fetched {
    Bytes(Vec<u8>),
    Magnet(String),
}

async fn download_torrent(
    http: &reqwest::Client,
    url: &str,
    cookies: Option<&str>,
    referrer: Option<&str>,
    user_agent: Option<&str>,
) -> anyhow::Result<Fetched> {
    use reqwest::header;
    let mut req = http.get(url).header(header::ACCEPT, "application/x-bittorrent, */*");
    if let Some(c) = cookies.filter(|c| !c.trim().is_empty()) {
        req = req.header(header::COOKIE, c);
    }
    if let Some(r) = referrer.filter(|r| !r.trim().is_empty()) {
        req = req.header(header::REFERER, r);
    }
    if let Some(ua) = user_agent.filter(|u| !u.trim().is_empty()) {
        req = req.header(header::USER_AGENT, ua);
    }
    let resp = req.send().await.context("could not download the .torrent file")?;
    if resp.status().is_redirection()
        && let Some(loc) = resp.headers().get(header::LOCATION).and_then(|v| v.to_str().ok())
        && loc.to_ascii_lowercase().starts_with("magnet:")
    {
        return Ok(Fetched::Magnet(loc.to_owned()));
    }
    if !resp.status().is_success() {
        bail!("the server answered {}", resp.status());
    }
    if resp.content_length().is_some_and(|l| l as usize > MAX_TORRENT_FILE) {
        bail!("the file is too large to be a .torrent");
    }
    let bytes = resp.bytes().await.context("download interrupted")?;
    if bytes.len() > MAX_TORRENT_FILE {
        bail!("the file is too large to be a .torrent");
    }
    if bytes.first() != Some(&b'd') {
        bail!("the link did not return a .torrent file");
    }
    Ok(Fetched::Bytes(bytes.to_vec()))
}

fn resolved_from_handle(h: &ManagedTorrent) -> ResolvedTorrent {
    let hash = h.info_hash().as_string();
    let info = StaticInfo::of(h, &hash);
    ResolvedTorrent {
        info_hash: hash,
        name: info.name,
        files: info.files.as_ref().clone(),
        total_size: info.full_size,
        piece_length: info.piece_length,
        pieces: info.pieces,
        private: info.private,
        trackers: info.trackers.as_ref().clone(),
        torrent_bytes: Arc::new(Vec::new()),
        seen_peers: Vec::new(),
        already_added: true,
    }
}

/// Reads / downloads / resolves a source into metadata, without adding it.
async fn resolve(
    session: &Arc<Session>,
    http: &reqwest::Client,
    source: AddSource,
) -> anyhow::Result<ResolvedTorrent> {
    let mut magnet_name = None;
    let add = match source {
        AddSource::Magnet(m) => {
            let info = crate::magnet::parse(&m).map_err(anyhow::Error::msg)?;
            if let Some(h) = find(session, &info.info_hash) {
                return Ok(resolved_from_handle(&h));
            }
            magnet_name = info.name;
            AddTorrent::from_url(m)
        }
        AddSource::TorrentBytes { bytes, .. } => AddTorrent::from_bytes(bytes.as_ref().clone()),
        AddSource::Path(p) => AddTorrent::from_bytes(
            tokio::fs::read(&p)
                .await
                .with_context(|| format!("could not read {}", p.display()))?,
        ),
        AddSource::TorrentUrl {
            url,
            cookies,
            referrer,
            user_agent,
        } => match download_torrent(
            http,
            &url,
            cookies.as_deref(),
            referrer.as_deref(),
            user_agent.as_deref(),
        )
        .await?
        {
            Fetched::Bytes(b) => AddTorrent::from_bytes(b),
            Fetched::Magnet(m) => {
                return Box::pin(resolve(session, http, AddSource::Magnet(m))).await;
            }
        },
    };
    let opts = AddTorrentOptions {
        list_only: true,
        ..Default::default()
    };
    match session.add_torrent(add, Some(opts)).await? {
        AddTorrentResponse::ListOnly(l) => {
            let info_hash = l.info_hash.as_string();
            if let Some(h) = find(session, &info_hash) {
                return Ok(resolved_from_handle(&h));
            }
            let files: Vec<FileEntry> = l
                .info
                .iter_file_details()
                .map(|fd| FileEntry {
                    path: fd.filename.to_vec(),
                    len: fd.len,
                    padding: fd.attrs().padding,
                })
                .collect();
            let lengths = l.info.lengths();
            let trackers = librqbit::torrent_from_bytes(&l.torrent_bytes)
                .map(|t| {
                    let mut v: Vec<String> = t
                        .iter_announce()
                        .filter_map(|b| std::str::from_utf8(b.as_ref()).ok().map(str::to_owned))
                        .collect();
                    v.dedup();
                    v
                })
                .unwrap_or_default();
            Ok(ResolvedTorrent {
                name: l
                    .info
                    .name()
                    .map(|n| n.into_owned())
                    .or(magnet_name)
                    .unwrap_or_else(|| info_hash.clone()),
                info_hash,
                total_size: lengths.total_length(),
                piece_length: lengths.default_piece_length(),
                pieces: lengths.total_pieces(),
                private: l.info.info().private,
                files,
                trackers,
                torrent_bytes: Arc::new(l.torrent_bytes.to_vec()),
                seen_peers: l.seen_peers,
                already_added: false,
            })
        }
        AddTorrentResponse::AlreadyManaged(_, h) | AddTorrentResponse::Added(_, h) => {
            Ok(resolved_from_handle(&h))
        }
    }
}

/// Adds resolved torrent bytes. Returns the name and whether it was new.
async fn add_resolved(session: &Arc<Session>, p: AddParams) -> anyhow::Result<(String, bool)> {
    let opts = AddTorrentOptions {
        paused: p.paused,
        only_files: p.only_files,
        overwrite: true,
        output_folder: Some(p.output_folder.to_string_lossy().into_owned()),
        initial_peers: (!p.initial_peers.is_empty()).then_some(p.initial_peers),
        ..Default::default()
    };
    let bytes = Arc::try_unwrap(p.torrent_bytes).unwrap_or_else(|a| a.as_ref().clone());
    match session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await? {
        AddTorrentResponse::Added(_, h) => Ok((h.name().unwrap_or(p.name), true)),
        AddTorrentResponse::AlreadyManaged(_, h) => Ok((h.name().unwrap_or(p.name), false)),
        AddTorrentResponse::ListOnly(_) => bail!("unexpected list-only response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_folder_names() {
        assert_eq!(sanitize_name("Big Buck Bunny"), "Big Buck Bunny");
        assert_eq!(sanitize_name("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_name("trailing dots... "), "trailing dots");
        assert_eq!(sanitize_name("   "), "torrent");
        assert_eq!(sanitize_name("CON"), "_CON");
        assert_eq!(sanitize_name("nul.txt"), "_nul.txt");
    }

    #[test]
    fn output_folder_rules() {
        let base = Path::new(r"C:\Downloads\Torrents");
        assert_eq!(output_folder_for(base, "single.iso", 1), base);
        assert_eq!(
            output_folder_for(base, "Album: Live", 3),
            base.join("Album_ Live")
        );
    }
}
