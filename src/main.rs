//! Zenless Torrent — a calm, fast BitTorrent client (eframe/egui + librqbit).
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
        // Single instance: hand our arguments to the running app and quit.
        if api::client::ping() {
            api::client::forward(&args.items);
            return Ok(());
        }
        init_logging();
        tracing::info!("Zenless Torrent {} starting", env!("CARGO_PKG_VERSION"));
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
