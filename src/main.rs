//! Zenless Torrent — a calm, fast BitTorrent client (eframe/egui + librqbit).
//!
//! Single instance: if another Zenless Torrent answers `GET /ping` on the
//! local API port, magnet links / .torrent files are forwarded to it and this
//! process exits. After a self-update the previous version starts the new one
//! with `--updated-from <old version>`: it first waits until the old instance
//! has exited, then says "Updated to vX". Torrents that were running keep
//! running (librqbit's session persistence).
//!
//! Environment (debug aids, all harmless):
//! * `ZENLESS_DEMO=1` / `ZENLESS_DEMO_VIEW=…` / `ZENLESS_DEMO_THEME=…` – sample
//!   data and screens, see `demo.rs` (includes `update-available`,
//!   `update-downloading`, `update-ready`, `update-failed`, `settings-updates`).
//! * `ZENLESS_SCREENSHOT=<file.png>` – save a screenshot after a few frames and exit.
//! * `ZENLESS_UPDATE_API=<url>` / `ZENLESS_UPDATE_DELAY_SECS=<n>` – update
//!   server (a mock of the GitHub API) and delay of the first automatic check.
//! * `ZENLESS_UPDATE_TEST_RESTART=1` – "Restart now" as soon as an update is ready.
//! * `ZENLESS_TORRENT_DATA_DIR=<dir>` / `ZENLESS_TORRENT_PORT=<port>` – separate
//!   data folder and API port (tests next to a real instance).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod cli;
mod demo;
mod engine;
mod magnet;
mod model;
mod platform;
mod settings;
// Shared verbatim by all Zenless apps; not every helper is used here.
#[allow(dead_code)]
mod shared;
mod store;
mod ui;

use eframe::egui;

/// Logs go to `%APPDATA%\Zenless\Torrent\zenless-torrent.log` (truncated on
/// start). Set `ZENLESS_LOG=debug` (or any tracing filter) for more detail.
fn init_logging() {
    let dir = settings::data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Ok(file) = std::fs::File::create(dir.join("zenless-torrent.log")) else { return };
    let filter = std::env::var("ZENLESS_LOG").unwrap_or_else(|_| "warn,zenless_torrent=info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::sync::Mutex::new(file))
        .with_target(true)
        .try_init();
}

fn main() -> eframe::Result {
    let args = cli::parse(std::env::args().skip(1));
    let demo = demo::enabled();

    if !demo {
        // Started by the previous version after an update: let it finish
        // exiting first, or the single-instance check would hand everything
        // back to it.
        if args.updated_from.is_some() {
            shared::updater::wait_for_previous_instance(api::port());
        }
        // Single instance: hand our arguments to the running app and quit.
        if api::client::ping() {
            api::client::forward(&args.items);
            return Ok(());
        }
        init_logging();
        tracing::info!("Zenless Torrent {} starting", env!("CARGO_PKG_VERSION"));
        if let Some(from) = &args.updated_from {
            tracing::info!("updated from {from}");
        }
    }

    let icon = egui::IconData {
        rgba: include_bytes!("../assets/icon-128.rgba").to_vec(),
        width: 128,
        height: 128,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Zenless Torrent")
            .with_app_id("zenless-torrent")
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([920.0, 540.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "zenless-torrent",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, args, demo)))),
    )
}
