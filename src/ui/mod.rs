//! The Zenless Torrent window.

mod add_dialog;
mod details;
mod file_tree;
mod settings_view;
mod sidebar;
mod table;
mod toolbar;
mod widgets;

use crate::api::{self, AddMode, ApiEvent, SharedApiStatus};
use crate::cli::{CliArgs, CliItem};
use crate::engine::{AddParams, Command, EngineConfig, EngineHandle};
use crate::model::{AddSource, EngineEvent, Filter, Snapshot, TorrentView};
use crate::settings::Settings;
use crate::shared::{kit, theme::ThemeManager};
use add_dialog::{AddDialog, AddOutcome};
use eframe::egui::{self, RichText, ViewportCommand};
use egui_phosphor::regular as icon;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, mpsc::Receiver},
    time::{Duration, Instant},
};
use widgets::{Toast, ToastKind};

const HISTORY_LEN: usize = 180;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DetailsTab {
    General,
    Files,
    Peers,
    Trackers,
    Speed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsTab {
    General,
    Network,
    Browser,
    Appearance,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickPurpose {
    TorrentFiles,
    AddFolder,
    DefaultFolder,
}

/// Multi-line magnet input dialog.
struct MagnetInput {
    text: String,
    focus: bool,
}

struct ConfirmRemove {
    keys: Vec<String>,
    names: Vec<String>,
    delete_files: bool,
}

pub struct App {
    themes: ThemeManager,
    shot: kit::AutoScreenshot,
    settings: Settings,
    /// Network settings the running session was started with.
    applied_network: (u16, bool, bool, bool, PathBuf),
    engine: EngineHandle,
    api_rx: Option<Receiver<ApiEvent>>,
    api_status: Option<SharedApiStatus>,
    demo: bool,
    snap: Arc<Snapshot>,
    logo: egui::TextureHandle,

    filter: Filter,
    search: String,
    selected: HashSet<String>,
    anchor: Option<String>,
    details_tab: DetailsTab,
    histories: HashMap<String, VecDeque<(f32, f32)>>,
    global_history: VecDeque<(f32, f32)>,
    last_sample: Instant,
    sampled_generation: u64,
    focus_sent: Option<String>,

    add_queue: VecDeque<(AddSource, String)>,
    add: Option<AddDialog>,
    magnet_input: Option<MagnetInput>,
    confirm_remove: Option<ConfirmRemove>,
    settings_open: bool,
    settings_tab: SettingsTab,
    toasts: Vec<Toast>,
    picker: Option<(PickPurpose, Receiver<Option<Vec<PathBuf>>>)>,
    next_req: u64,

    /// Demo only (`ZENLESS_DEMO_THEME`): a theme applied in memory, so
    /// screenshots never touch the shared appearance.json.
    theme_override: Option<crate::shared::theme::Theme>,
    start_minimized: bool,
    frames: u64,
    raise_frames: u8,
    autostart_on: bool,
    handlers_on: bool,
}

pub fn engine_config(s: &Settings) -> EngineConfig {
    EngineConfig {
        download_dir: s.download_dir.clone(),
        session_dir: crate::settings::session_dir(),
        listen_port: s.listen_port,
        dht: s.enable_dht,
        upnp: s.enable_upnp,
        utp: s.enable_utp,
        download_limit_bps: s.download_limit_bps(),
        upload_limit_bps: s.upload_limit_bps(),
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: CliArgs, demo: bool) -> Self {
        let ctx = cc.egui_ctx.clone();
        let themes = ThemeManager::new(&ctx);
        let settings = if demo {
            Settings::default()
        } else {
            Settings::load()
        };
        let logo = ctx.load_texture(
            "zenless-logo",
            egui::ColorImage::from_rgba_unmultiplied([128, 128], include_bytes!("../../assets/icon-128.rgba")),
            egui::TextureOptions::LINEAR,
        );

        let (engine, api_rx, api_status) = if demo {
            (EngineHandle::offline(crate::demo::snapshot()), None, None)
        } else {
            let engine = EngineHandle::start(engine_config(&settings), ctx.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            let status = api::start(engine.shared(), tx, ctx.clone());
            (engine, Some(rx), Some(status))
        };

        let mut app = Self {
            applied_network: settings.network_key(),
            snap: engine.snapshot(),
            themes,
            shot: kit::AutoScreenshot::from_env(),
            settings,
            engine,
            api_rx,
            api_status,
            demo,
            logo,
            filter: Filter::All,
            search: String::new(),
            selected: HashSet::new(),
            anchor: None,
            details_tab: DetailsTab::General,
            histories: HashMap::new(),
            global_history: VecDeque::new(),
            last_sample: Instant::now() - Duration::from_secs(5),
            sampled_generation: 0,
            focus_sent: None,
            add_queue: VecDeque::new(),
            add: None,
            magnet_input: None,
            confirm_remove: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
            toasts: Vec::new(),
            picker: None,
            next_req: 1,
            theme_override: None,
            start_minimized: args.minimized && std::env::var_os("ZENLESS_SCREENSHOT").is_none(),
            frames: 0,
            raise_frames: 0,
            autostart_on: false,
            handlers_on: false,
        };
        for item in args.items {
            let source = match item {
                CliItem::Magnet(m) => AddSource::Magnet(m),
                CliItem::File(p) => AddSource::Path(p),
            };
            app.add_queue.push_back((source, "command line".into()));
        }
        if demo {
            app.setup_demo(&ctx);
        } else {
            app.refresh_registry_state();
        }
        app
    }

    fn palette(&self) -> crate::shared::theme::Palette {
        match &self.theme_override {
            Some(t) => t.palette.clone(),
            None => self.themes.palette().clone(),
        }
    }

    fn setup_demo(&mut self, ctx: &egui::Context) {
        if let Ok(name) = std::env::var("ZENLESS_DEMO_THEME") {
            use crate::shared::theme;
            if let Some(t) = theme::builtin_themes()
                .into_iter()
                .find(|t| t.name.eq_ignore_ascii_case(name.trim()))
            {
                theme::apply(ctx, &t, &theme::Appearance::default());
                self.theme_override = Some(t);
            }
        }
        let snap = self.snap.clone();
        for t in &snap.torrents {
            self.histories.insert(
                t.key.clone(),
                crate::demo::history(HISTORY_LEN, t.down_bps.max(t.up_bps * 3.0)).into(),
            );
        }
        self.global_history = crate::demo::history(HISTORY_LEN, snap.down_bps).into();
        if let Some(first) = snap.torrents.first() {
            self.selected.insert(first.key.clone());
            self.anchor = Some(first.key.clone());
        }
        match crate::demo::view().as_str() {
            "add" => {
                let mut d = AddDialog::new(0, AddSource::Magnet(String::new()), "chrome-extension/0.1.0".into(), &self.settings);
                d.set_resolved(Ok(crate::demo::resolved()));
                self.add = Some(d);
            }
            "adding" => {
                self.add = Some(AddDialog::new(
                    0,
                    AddSource::Magnet("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny".into()),
                    "firefox-extension/0.1.0".into(),
                    &self.settings,
                ));
            }
            "magnet" => {
                self.magnet_input = Some(MagnetInput {
                    text: "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny".into(),
                    focus: false,
                })
            }
            "settings" => self.settings_open = true,
            "settings-network" => self.open_settings(SettingsTab::Network),
            "settings-browser" => self.open_settings(SettingsTab::Browser),
            "settings-appearance" => self.open_settings(SettingsTab::Appearance),
            "about" => self.open_settings(SettingsTab::About),
            "files" => self.details_tab = DetailsTab::Files,
            "peers" => self.details_tab = DetailsTab::Peers,
            "trackers" => self.details_tab = DetailsTab::Trackers,
            "speed" => self.details_tab = DetailsTab::Speed,
            "remove" => {
                self.confirm_remove = Some(ConfirmRemove {
                    keys: vec![],
                    names: vec![snap.torrents[0].name.clone()],
                    delete_files: true,
                })
            }
            "empty" => {
                self.engine.set_snapshot(Snapshot {
                    engine: crate::model::EngineStatus::Demo,
                    ..Default::default()
                });
                self.snap = self.engine.snapshot();
                self.selected.clear();
            }
            "toast" => self.toast("Download complete: debian-12.7.0-amd64-netinst.iso", ToastKind::Success),
            _ => {}
        }
    }

    fn open_settings(&mut self, tab: SettingsTab) {
        self.settings_tab = tab;
        self.settings_open = true;
        self.refresh_registry_state();
    }

    fn refresh_registry_state(&mut self) {
        if !self.demo {
            self.autostart_on = crate::platform::autostart_enabled();
            self.handlers_on = crate::platform::handlers_registered();
        }
    }

    fn save_settings(&self) {
        if self.demo {
            return;
        }
        if let Err(e) = self.settings.save() {
            tracing::warn!("could not save settings: {e}");
        }
    }

    fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
    }

    /// Restores and raises the window (used for API `mode: ask` and `/focus`).
    fn raise_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.raise_frames = 6;
    }

    // -- adding torrents ------------------------------------------------------

    fn queue_add(&mut self, source: AddSource, client: &str, mode: AddMode) {
        match mode {
            AddMode::Ask => self.add_queue.push_back((source, client.to_owned())),
            AddMode::Start => {
                self.toast(format!("Adding {}…", source.describe()), ToastKind::Info);
                self.engine.send(Command::AddDirect {
                    source,
                    download_dir: self.settings.download_dir.clone(),
                    paused: false,
                });
            }
        }
    }

    fn open_next_add(&mut self) {
        if self.add.is_some() {
            return;
        }
        let Some((source, client)) = self.add_queue.pop_front() else { return };
        let req = self.next_req;
        self.next_req += 1;
        self.engine.send(Command::Resolve {
            req,
            source: source.clone(),
        });
        self.add = Some(AddDialog::new(req, source, client, &self.settings));
    }

    fn finish_add(&mut self, dialog: &AddDialog) {
        let Some(r) = dialog.resolved() else { return };
        let real: Vec<usize> = (0..r.files.len()).filter(|&i| !r.files[i].padding).collect();
        let chosen: Vec<usize> = real.iter().copied().filter(|&i| dialog.selected[i]).collect();
        let only_files = (chosen.len() != real.len()).then_some(chosen);
        let folder = PathBuf::from(dialog.folder.trim());
        if self.settings.start_immediately != dialog.start {
            self.settings.start_immediately = dialog.start;
            self.save_settings();
        }
        self.engine.send(Command::Add(AddParams {
            torrent_bytes: r.torrent_bytes.clone(),
            name: r.name.clone(),
            output_folder: crate::engine::output_folder_for(&folder, &r.name, r.files.len()),
            only_files,
            paused: !dialog.start,
            initial_peers: r.seen_peers.clone(),
        }));
    }

    fn pick(&mut self, ctx: &egui::Context, purpose: PickPurpose, start_dir: PathBuf) {
        if self.picker.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("zenless-picker".into())
            .spawn(move || {
                let result = match purpose {
                    PickPurpose::TorrentFiles => rfd::FileDialog::new()
                        .set_title("Open torrent files")
                        .add_filter("Torrent files", &["torrent"])
                        .add_filter("All files", &["*"])
                        .pick_files(),
                    PickPurpose::AddFolder | PickPurpose::DefaultFolder => rfd::FileDialog::new()
                        .set_title("Choose a folder")
                        .set_directory(&start_dir)
                        .pick_folder()
                        .map(|p| vec![p]),
                };
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.picker = Some((purpose, rx));
        }
    }

    fn poll_picker(&mut self) {
        let Some((purpose, rx)) = &self.picker else { return };
        let purpose = *purpose;
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.picker = None;
        let Some(paths) = result else { return };
        match purpose {
            PickPurpose::TorrentFiles => {
                for p in paths {
                    self.queue_add(AddSource::Path(p), "file picker", AddMode::Ask);
                }
            }
            PickPurpose::AddFolder => {
                if let (Some(d), Some(p)) = (self.add.as_mut(), paths.into_iter().next()) {
                    d.folder = p.display().to_string();
                }
            }
            PickPurpose::DefaultFolder => {
                if let Some(p) = paths.into_iter().next() {
                    self.settings.download_dir = p;
                    self.save_settings();
                }
            }
        }
    }

    // -- actions on torrents --------------------------------------------------

    fn selected_views<'a>(&self, snap: &'a Snapshot) -> Vec<&'a TorrentView> {
        snap.torrents.iter().filter(|t| self.selected.contains(&t.key)).collect()
    }

    fn selected_keys(&self) -> Vec<String> {
        self.snap
            .torrents
            .iter()
            .filter(|t| self.selected.contains(&t.key))
            .map(|t| t.key.clone())
            .collect()
    }

    fn resume(&mut self, keys: Vec<String>) {
        if !keys.is_empty() {
            self.engine.send(Command::Resume(keys));
        }
    }

    fn pause(&mut self, keys: Vec<String>) {
        if !keys.is_empty() {
            self.engine.send(Command::Pause(keys));
        }
    }

    fn resume_all(&mut self) {
        let keys = self
            .snap
            .torrents
            .iter()
            .filter(|t| t.status.can_resume())
            .map(|t| t.key.clone())
            .collect();
        self.resume(keys);
    }

    fn pause_all(&mut self) {
        let keys = self
            .snap
            .torrents
            .iter()
            .filter(|t| t.status.can_pause())
            .map(|t| t.key.clone())
            .collect();
        self.pause(keys);
    }

    fn ask_remove(&mut self, keys: Vec<String>, delete_files: bool) {
        if keys.is_empty() {
            return;
        }
        let names = self
            .snap
            .torrents
            .iter()
            .filter(|t| keys.contains(&t.key))
            .map(|t| t.name.clone())
            .collect();
        self.confirm_remove = Some(ConfirmRemove {
            keys,
            names,
            delete_files,
        });
    }

    fn open_folder_of(&self, keys: &[String]) {
        for t in self.snap.torrents.iter().filter(|t| keys.contains(&t.key)).take(3) {
            let single = t.files.iter().filter(|f| !f.padding).count() == 1;
            match t.files.iter().find(|f| !f.padding) {
                Some(f) if single => {
                    let mut path = t.save_path.clone();
                    path.extend(&f.path);
                    crate::platform::reveal(&path);
                }
                _ => crate::platform::open_folder(&t.save_path),
            }
        }
    }

    fn copy_magnets(&mut self, ctx: &egui::Context, keys: &[String]) {
        let links: Vec<String> = self
            .snap
            .torrents
            .iter()
            .filter(|t| keys.contains(&t.key) && !t.info_hash.is_empty())
            .map(|t| crate::magnet::build(&t.info_hash, Some(&t.name), t.trackers.as_slice()))
            .collect();
        if !links.is_empty() {
            let n = links.len();
            ctx.copy_text(links.join("\n"));
            self.toast(
                if n == 1 { "Magnet link copied".to_owned() } else { format!("{n} magnet links copied") },
                ToastKind::Info,
            );
        }
    }

    fn copy_hashes(&mut self, ctx: &egui::Context, keys: &[String]) {
        let hashes: Vec<String> = self
            .snap
            .torrents
            .iter()
            .filter(|t| keys.contains(&t.key) && !t.info_hash.is_empty())
            .map(|t| t.info_hash.clone())
            .collect();
        if !hashes.is_empty() {
            ctx.copy_text(hashes.join("\n"));
            self.toast("Info hash copied", ToastKind::Info);
        }
    }

    fn recheck(&mut self, keys: Vec<String>) {
        let keys: Vec<String> = keys.into_iter().filter(|k| !k.starts_with("pending-")).collect();
        if !keys.is_empty() {
            let n = keys.len();
            self.engine.send(Command::Recheck(keys));
            self.toast(
                if n == 1 { "Rechecking data…".to_owned() } else { format!("Rechecking {n} torrents…") },
                ToastKind::Info,
            );
        }
    }

    // -- per-frame bookkeeping ----------------------------------------------------

    fn sample_history(&mut self) {
        if self.demo
            || self.last_sample.elapsed() < Duration::from_millis(950)
            || self.snap.generation == self.sampled_generation
        {
            return;
        }
        self.last_sample = Instant::now();
        let snap = self.snap.clone();
        self.sampled_generation = snap.generation;
        push_sample(&mut self.global_history, (snap.down_bps as f32, snap.up_bps as f32));
        let mut alive = HashSet::new();
        for t in &snap.torrents {
            alive.insert(t.key.as_str());
            let h = self.histories.entry(t.key.clone()).or_default();
            push_sample(h, (t.down_bps as f32, t.up_bps as f32));
        }
        self.histories.retain(|k, _| alive.contains(k.as_str()));
        self.selected.retain(|k| alive.contains(k.as_str()));
    }

    fn handle_engine_events(&mut self, ctx: &egui::Context) {
        while let Some(ev) = self.engine.try_event() {
            match ev {
                EngineEvent::Resolved { req, result } => {
                    if let Some(d) = self.add.as_mut().filter(|d| d.req == req) {
                        d.set_resolved(result);
                    }
                }
                EngineEvent::Added { name } => self.toast(format!("Added \"{name}\""), ToastKind::Success),
                EngineEvent::Completed { name } => {
                    if self.settings.notify_on_complete {
                        self.toast(format!("Download complete: {name}"), ToastKind::Success);
                        if !ctx.input(|i| i.focused) {
                            ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
                                egui::UserAttentionType::Informational,
                            ));
                        }
                    }
                }
                EngineEvent::Info(msg) => self.toast(msg, ToastKind::Info),
                EngineEvent::Error(msg) => self.toast(msg, ToastKind::Error),
            }
        }
    }

    fn handle_api_events(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.api_rx else { return };
        let events: Vec<ApiEvent> = rx.try_iter().collect();
        for ev in events {
            match ev {
                ApiEvent::Add(req) => {
                    if req.mode == AddMode::Ask {
                        self.raise_window(ctx);
                    }
                    self.queue_add(req.source, &req.client, req.mode);
                }
                ApiEvent::Focus => self.raise_window(ctx),
                ApiEvent::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }

    /// Dropped .torrent files, pasted magnet links and keyboard shortcuts.
    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            let path = f.path().to_path_buf();
            let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() < 256 * 1024);
            if ext == "torrent" {
                self.queue_add(AddSource::Path(path), "drag & drop", AddMode::Ask);
            } else if !small {
                self.toast(format!("{} is not a .torrent file", path.display()), ToastKind::Error);
            } else if let Ok(text) = f.bytes().map(|b| String::from_utf8_lossy(&b).into_owned()) {
                // .txt / .url / .magnet files containing magnet links
                let magnets = extract_magnets(&text);
                if magnets.is_empty() {
                    self.toast(format!("{} is not a .torrent file", path.display()), ToastKind::Error);
                }
                for m in magnets {
                    self.queue_add(AddSource::Magnet(m), "drag & drop", AddMode::Ask);
                }
            }
        }

        let modal_open = self.add.is_some()
            || self.magnet_input.is_some()
            || self.confirm_remove.is_some()
            || self.settings_open;
        if modal_open || ctx.egui_wants_keyboard_input() {
            return;
        }
        let pasted: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Paste(s) => Some(s.clone()),
                    _ => None,
                })
                .collect()
        });
        for text in pasted {
            let magnets = extract_magnets(&text);
            for m in magnets {
                self.queue_add(AddSource::Magnet(m), "clipboard", AddMode::Ask);
            }
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        let (del, sel_all, open, link) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Delete),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::A)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::U)),
            )
        });
        if del {
            let keys = self.selected_keys();
            self.ask_remove(keys, false);
        }
        if sel_all {
            let rows = self.visible_rows(&self.snap.clone());
            self.selected = rows.iter().map(|&i| self.snap.torrents[i].key.clone()).collect();
        }
        if open {
            self.pick(ctx, PickPurpose::TorrentFiles, self.settings.download_dir.clone());
        }
        if link {
            self.magnet_input = Some(MagnetInput {
                text: String::new(),
                focus: true,
            });
        }
    }

    /// Asks the engine for the peer list of the torrent shown in the Peers tab.
    fn sync_focus(&mut self) {
        let want = if self.settings.show_details && self.details_tab == DetailsTab::Peers && self.selected.len() == 1 {
            self.selected.iter().next().cloned()
        } else {
            None
        };
        if want != self.focus_sent {
            self.focus_sent = want.clone();
            self.engine.send(Command::Focus(want));
        }
    }

    // -- dialogs --------------------------------------------------------------

    fn magnet_modal(&mut self, ctx: &egui::Context, p: &crate::shared::theme::Palette) {
        let Some(input) = &mut self.magnet_input else { return };
        let mut submit = false;
        let mut cancel = false;
        let resp = egui::Modal::new(egui::Id::new("magnet-input")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::MAGNET).size(22.0).color(p.accent));
                ui.heading("Add magnet link");
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Paste one or more magnet links (one per line).").color(p.text_dim));
            ui.add_space(6.0);
            let edit = ui.add(
                egui::TextEdit::multiline(&mut input.text)
                    .hint_text("magnet:?xt=urn:btih:…")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if input.focus {
                edit.request_focus();
                input.focus = false;
            }
            let lines: Vec<&str> = input.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            let valid = lines.iter().filter(|l| crate::magnet::parse(l).is_ok()).count();
            let invalid = lines.len() - valid;
            ui.add_space(4.0);
            if invalid > 0 {
                ui.label(
                    RichText::new(format!("{} {invalid} line(s) are not valid magnet links", icon::WARNING))
                        .color(p.warning)
                        .small(),
                );
            } else if valid > 0 {
                ui.label(
                    RichText::new(format!("{} {valid} magnet link(s) ready", icon::CHECK))
                        .color(p.success)
                        .small(),
                );
            } else {
                ui.label(RichText::new(" ").small());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = ui.add_enabled(
                        valid > 0,
                        egui::Button::new(RichText::new("Continue").color(p.accent_fg).strong()).fill(p.accent),
                    );
                    if ok.clicked() {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if valid > 0 && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
        });
        if submit {
            let links: Vec<String> = input
                .text
                .lines()
                .map(str::trim)
                .filter(|l| crate::magnet::parse(l).is_ok())
                .map(str::to_owned)
                .collect();
            self.magnet_input = None;
            for l in links {
                self.add_queue.push_back((AddSource::Magnet(l), "magnet dialog".into()));
            }
        } else if cancel || resp.should_close() {
            self.magnet_input = None;
        }
    }

    fn remove_modal(&mut self, ctx: &egui::Context, p: &crate::shared::theme::Palette) {
        let Some(c) = &mut self.confirm_remove else { return };
        let mut confirm = false;
        let mut cancel = false;
        let resp = egui::Modal::new(egui::Id::new("confirm-remove")).show(ctx, |ui| {
            ui.set_width(460.0);
            let n = c.names.len().max(c.keys.len());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::TRASH).size(22.0).color(p.danger));
                ui.heading(if n == 1 { "Remove torrent?".to_owned() } else { format!("Remove {n} torrents?") });
            });
            ui.add_space(6.0);
            for name in c.names.iter().take(5) {
                ui.add(egui::Label::new(RichText::new(format!("·  {name}")).color(p.text)).truncate());
            }
            if c.names.len() > 5 {
                ui.label(RichText::new(format!("…and {} more", c.names.len() - 5)).color(p.text_dim));
            }
            ui.add_space(10.0);
            let label_color = if c.delete_files { p.danger } else { p.text };
            ui.checkbox(
                &mut c.delete_files,
                RichText::new("Also delete the downloaded files from disk").color(label_color),
            );
            ui.label(
                RichText::new(if c.delete_files {
                    "Files are deleted permanently (not moved to the Recycle Bin)."
                } else {
                    "Downloaded files stay where they are."
                })
                .small()
                .color(p.text_dim),
            );
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, fill, fg) = if c.delete_files {
                    ("Remove and delete files", p.danger, egui::Color32::WHITE)
                } else {
                    ("Remove", p.accent, p.accent_fg)
                };
                if ui.add(egui::Button::new(RichText::new(label).color(fg).strong()).fill(fill)).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if confirm {
            let c = self.confirm_remove.take();
            if let Some(c) = c {
                for k in &c.keys {
                    self.selected.remove(k);
                }
                if !c.keys.is_empty() {
                    self.engine.send(Command::Remove {
                        keys: c.keys,
                        delete_files: c.delete_files,
                    });
                }
            }
        } else if cancel || resp.should_close() {
            self.confirm_remove = None;
        }
    }

    fn add_modal(&mut self, ctx: &egui::Context, p: &crate::shared::theme::Palette) {
        let Some(dialog) = &mut self.add else { return };
        let outcome = dialog.show(ctx, p, self.add_queue.len());
        match outcome {
            AddOutcome::None => {}
            AddOutcome::Browse => {
                let start = PathBuf::from(dialog.folder.trim());
                self.pick(ctx, PickPurpose::AddFolder, start);
            }
            AddOutcome::Cancel => {
                if let Some(d) = self.add.take()
                    && d.is_resolving()
                {
                    self.engine.send(Command::CancelResolve(d.req));
                }
            }
            AddOutcome::Add => {
                if let Some(d) = self.add.take() {
                    self.finish_add(&d);
                }
            }
            AddOutcome::ShowExisting(hash) => {
                self.add = None;
                self.filter = Filter::All;
                self.search.clear();
                self.selected = [hash.clone()].into();
                self.anchor = Some(hash);
            }
        }
    }

    // -- layout -------------------------------------------------------------------

    fn status_bar(&mut self, ui: &mut egui::Ui, p: &crate::shared::theme::Palette, snap: &Snapshot) {
        use crate::model::EngineStatus;
        egui::Panel::bottom("statusbar")
            .exact_size(28.0)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(12, 0))
                    .stroke(egui::Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let (dot, text) = match &snap.engine {
                        EngineStatus::Running => (p.success, "Connected".to_owned()),
                        EngineStatus::Starting => (p.warning, "Starting…".to_owned()),
                        EngineStatus::Restarting => (p.warning, "Restarting…".to_owned()),
                        EngineStatus::Demo => (p.info, "Demo mode".to_owned()),
                        EngineStatus::Failed(_) => (p.danger, "Engine error".to_owned()),
                    };
                    let r = widgets::dot_label(ui, dot, RichText::new(text).small().color(p.text_dim));
                    if let EngineStatus::Failed(e) = &snap.engine {
                        r.on_hover_text(e);
                        if ui.small_button("Retry").clicked() {
                            self.applied_network = self.settings.network_key();
                            self.engine.send(Command::Restart(engine_config(&self.settings)));
                        }
                    }
                    ui.label(
                        RichText::new(format!("{} {}", icon::ARROW_DOWN, kit::human_speed(snap.down_bps)))
                            .small()
                            .color(p.accent),
                    );
                    ui.label(
                        RichText::new(format!("{} {}", icon::ARROW_UP, kit::human_speed(snap.up_bps)))
                            .small()
                            .color(p.accent2),
                    );
                    let dht = match (snap.dht_nodes, self.settings.enable_dht) {
                        (Some(n), _) => format!("DHT: {n} nodes"),
                        (None, false) => "DHT: off".to_owned(),
                        (None, true) => "DHT: …".to_owned(),
                    };
                    ui.label(RichText::new(dht).small().color(p.text_dim));
                    if let Some(port) = snap.listen_port {
                        ui.label(RichText::new(format!("Port {port}")).small().color(p.text_dim));
                    }
                    if let Some(n) = &snap.notice {
                        ui.label(RichText::new(format!("{} {n}", icon::WARNING)).small().color(p.warning));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let details = if self.settings.show_details { "Hide details" } else { "Show details" };
                        if ui
                            .add(egui::Button::new(RichText::new(icon::SIDEBAR_SIMPLE).size(15.0)).frame(false))
                            .on_hover_text(details)
                            .clicked()
                        {
                            self.settings.show_details = !self.settings.show_details;
                            self.save_settings();
                        }
                        let n = snap.torrents.len();
                        ui.label(
                            RichText::new(if n == 1 { "1 torrent".to_owned() } else { format!("{n} torrents") })
                                .small()
                                .color(p.text_dim),
                        );
                        if let Some(st) = &self.api_status {
                            let st = st.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let (c, t) = if st.listening {
                                (p.text_dim, format!("{} API {}", icon::PLUGS_CONNECTED, api::PORT))
                            } else {
                                (p.warning, format!("{} API off", icon::PLUGS))
                            };
                            ui.label(RichText::new(t).small().color(c));
                        }
                    });
                });
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, p: &crate::shared::theme::Palette, filtered: bool) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(20.0));
            if filtered {
                ui.label(RichText::new(icon::FUNNEL).size(40.0).color(p.text_dim));
                ui.add_space(6.0);
                ui.label(RichText::new("Nothing matches this view").size(18.0).color(p.text));
                ui.label(RichText::new("Try another filter or clear the search.").color(p.text_dim));
                ui.add_space(8.0);
                if ui.button("Show all torrents").clicked() {
                    self.filter = Filter::All;
                    self.search.clear();
                }
                return;
            }
            widgets::logo(ui, &self.logo, 72.0);
            ui.add_space(10.0);
            ui.label(RichText::new("No torrents yet").size(22.0).strong().color(p.text));
            ui.add_space(2.0);
            ui.label(
                RichText::new("Add a magnet link or a .torrent file to start downloading.").color(p.text_dim),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let w = 330.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                if kit::primary_button(ui, p, format!("{}  Add magnet link", icon::MAGNET)).clicked() {
                    self.magnet_input = Some(MagnetInput {
                        text: String::new(),
                        focus: true,
                    });
                }
                if ui.button(format!("{}  Open .torrent file", icon::FILE_PLUS)).clicked() {
                    let dir = self.settings.download_dir.clone();
                    self.pick(ui.ctx(), PickPurpose::TorrentFiles, dir);
                }
            });
            ui.add_space(18.0);
            let hint = |ui: &mut egui::Ui, ic: &str, text: &str| {
                ui.label(RichText::new(format!("{ic}  {text}")).color(p.text_dim));
            };
            hint(ui, icon::HAND_GRABBING, "Drag .torrent files onto this window");
            hint(ui, icon::CLIPBOARD_TEXT, "Paste a magnet link anywhere with Ctrl+V");
            hint(ui, icon::PUZZLE_PIECE, "Or send them from your browser with the Zenless extension");
        });
    }
}

fn push_sample(h: &mut VecDeque<(f32, f32)>, s: (f32, f32)) {
    h.push_back(s);
    while h.len() > HISTORY_LEN {
        h.pop_front();
    }
}

/// All magnet links in a blob of text (one per whitespace-separated token).
fn extract_magnets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>') {
        let token = token.trim().trim_start_matches("URL=");
        if crate::magnet::looks_like_magnet(token)
            && crate::magnet::parse(token).is_ok()
            && !out.iter().any(|o| o == token)
        {
            out.push(token.to_owned());
        }
    }
    out
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.theme_override.is_none() {
            self.themes.poll(ctx);
        }
        self.frames += 1;
        if self.frames == 2 && self.start_minimized {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if self.raise_frames > 0 {
            self.raise_frames -= 1;
            if self.raise_frames == 0 {
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.snap = self.engine.snapshot();
        self.sample_history();
        self.handle_engine_events(ctx);
        self.handle_api_events(ctx);
        self.handle_input(ctx);
        self.poll_picker();
        self.open_next_add();
        self.sync_focus();
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shot.tick(ui.ctx());
        let ctx = ui.ctx().clone();
        let p = self.palette();
        let snap = self.snap.clone();

        self.toolbar(ui, &p);
        self.status_bar(ui, &p, &snap);
        self.sidebar(ui, &p, &snap);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let rows = self.visible_rows(&snap);
                if self.settings.show_details && !snap.torrents.is_empty() {
                    egui::Panel::bottom("details")
                        .resizable(true)
                        .default_size(270.0)
                        .min_size(150.0)
                        .max_size(560.0)
                        .frame(
                            egui::Frame::new()
                                .fill(p.surface)
                                .inner_margin(egui::Margin::symmetric(14, 8))
                                .stroke(egui::Stroke::new(1.0, p.border)),
                        )
                        .show(ui, |ui| self.details(ui, &p, &snap));
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                        left: 8,
                        right: 8,
                        top: 6,
                        bottom: 0,
                    }))
                    .show(ui, |ui| {
                        if rows.is_empty() {
                            let filtered = !snap.torrents.is_empty();
                            self.empty_state(ui, &p, filtered);
                        } else {
                            self.table(ui, &p, &snap, &rows);
                        }
                    });
            });

        self.add_modal(&ctx, &p);
        self.magnet_modal(&ctx, &p);
        self.remove_modal(&ctx, &p);
        if self.settings_open {
            self.settings_modal(&ctx, &p);
        }
        widgets::show_toasts(&ctx, &p, &mut self.toasts, 30.0);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            widgets::drop_overlay(&ctx, &p);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
        self.engine.shutdown(Duration::from_secs(8));
    }
}

#[cfg(test)]
mod tests {
    use super::extract_magnets;

    #[test]
    fn finds_magnets_in_text() {
        let text = "[InternetShortcut]\nURL=magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=x\n\
                    junk magnet:?nope <magnet:?xt=urn:btih:DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1D>";
        let m = extract_magnets(text);
        assert_eq!(m.len(), 2);
        assert!(m[0].starts_with("magnet:?xt=urn:btih:dd82"));
        assert!(extract_magnets("hello world").is_empty());
    }

    #[test]
    fn status_filters_count() {
        use crate::model::{Filter, Snapshot, Status, TorrentView};
        let mut s = Snapshot::default();
        for (i, st) in [Status::Downloading, Status::Seeding, Status::Completed, Status::Metadata]
            .into_iter()
            .enumerate()
        {
            s.torrents.push(TorrentView::placeholder(i.to_string(), "t", st));
        }
        assert_eq!(s.count_filter(Filter::Downloading), 2);
        assert_eq!(s.count_filter(Filter::Completed), 2);
        assert_eq!(s.count(Status::Seeding), 1);
    }
}
