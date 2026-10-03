//! Zenless Torrent in the system tray: the icon's menu and tooltip, what
//! closing the window does, and the one way to really quit.
//!
//! The generic tray code is `shared::tray`. Everything here runs from
//! `App::logic()`, because eframe only runs `logic()` (not `ui()`) while the
//! window is hidden or minimized.

use super::{App, MagnetInput, PickPurpose, widgets::ToastKind};
use crate::api;
use crate::shared::tray::{self, CloseAction, MenuEntry, Tray, TrayEvent};
use eframe::egui;
use std::time::{Duration, Instant};

/// The tray icon: 32×32 RGBA.
const ICON: &[u8] = include_bytes!("../../assets/tray-32.rgba");
const ICON_SIZE: u32 = 32;
/// How often the tooltip's speeds are refreshed.
const REFRESH: Duration = Duration::from_secs(2);

const MENU_WINDOW: &str = "window";
const MENU_ADD_MAGNET: &str = "add-magnet";
const MENU_ADD_FILE: &str = "add-file";
const MENU_PAUSE_ALL: &str = "pause-all";
const MENU_RESUME_ALL: &str = "resume-all";
const MENU_QUIT: &str = "quit";

const SHOW_LABEL: &str = "Show Zenless Torrent";
const HIDE_LABEL: &str = "Hide";

fn menu(window_label: &str) -> Vec<MenuEntry> {
    vec![
        MenuEntry::item(MENU_WINDOW, window_label),
        MenuEntry::Separator,
        MenuEntry::item(MENU_ADD_MAGNET, "Add magnet link…"),
        MenuEntry::item(MENU_ADD_FILE, "Add .torrent file…"),
        MenuEntry::item(MENU_PAUSE_ALL, "Pause all"),
        MenuEntry::item(MENU_RESUME_ALL, "Resume all"),
        MenuEntry::Separator,
        MenuEntry::item(MENU_QUIT, "Quit"),
    ]
}

/// `"Zenless Torrent · ↓ 3.1 MB/s ↑ 420 KB/s"`, or `"Zenless Torrent · idle"`.
pub fn tooltip(down_bps: f64, up_bps: f64) -> String {
    if down_bps < 1.0 && up_bps < 1.0 {
        format!("{} · idle", api::APP_NAME)
    } else {
        format!("{} · ↓ {} ↑ {}", api::APP_NAME, tray::speed(down_bps), tray::speed(up_bps))
    }
}

impl App {
    fn tray_tooltip(&self) -> String {
        tooltip(self.snap.down_bps, self.snap.up_bps)
    }

    /// Adds or removes the icon to match Settings › "Show an icon in the
    /// system tray". Never in screenshot mode, so no icon is left behind.
    pub(super) fn sync_tray(&mut self, ctx: &egui::Context) {
        let want = self.tray_allowed && self.settings.tray_icon;
        if !want {
            self.tray_error = None;
            if self.tray.take().is_some() && self.win.is_hidden() {
                // Nothing left to bring the window back with.
                self.raise_window(ctx);
            }
            return;
        }
        if self.tray.is_some() || self.tray_error.is_some() {
            return;
        }
        let label = if self.win.on_screen(ctx) { HIDE_LABEL } else { SHOW_LABEL };
        match Tray::new(ctx, ICON, ICON_SIZE, &self.tray_tooltip(), &menu(label)) {
            Ok(t) => {
                self.tray = Some(t);
                self.tray_refreshed = Instant::now();
            }
            Err(e) => {
                tracing::warn!("no tray icon: {e}");
                self.tray_error = Some(e);
            }
        }
    }

    /// Tray clicks and menu choices, then the tooltip and the Show/Hide label.
    pub(super) fn tray_tick(&mut self, ctx: &egui::Context) {
        self.sync_tray(ctx);
        let Some(tray) = self.tray.as_mut() else { return };
        for event in tray.poll() {
            match event {
                TrayEvent::Activate => self.toggle_window(ctx),
                TrayEvent::Menu(id) => self.tray_menu(ctx, &id),
            }
        }
        let tooltip = (self.tray_refreshed.elapsed() >= REFRESH).then(|| self.tray_tooltip());
        let label = if self.win.on_screen(ctx) { HIDE_LABEL } else { SHOW_LABEL };
        let Some(tray) = self.tray.as_mut() else { return };
        if let Some(tooltip) = tooltip {
            tray.set_tooltip(&tooltip);
            self.tray_refreshed = Instant::now();
        }
        tray.set_label(MENU_WINDOW, label);
        self.show_pending_tray_notice(ctx);
        ctx.request_repaint_after(REFRESH);
    }

    /// Hidden or minimized → shown and raised; on screen → hidden.
    fn toggle_window(&mut self, ctx: &egui::Context) {
        if self.win.on_screen(ctx) {
            self.win.hide(ctx);
        } else {
            self.raise_window(ctx);
        }
    }

    fn tray_menu(&mut self, ctx: &egui::Context, id: &str) {
        let modal_open =
            self.add.is_some() || self.magnet_input.is_some() || self.confirm_remove.is_some() || self.settings_open;
        match id {
            // The label says "Show …" exactly when the window isn't on screen.
            MENU_WINDOW => self.toggle_window(ctx),
            MENU_ADD_MAGNET => {
                if !modal_open {
                    self.magnet_input = Some(MagnetInput {
                        text: String::new(),
                        focus: true,
                    });
                }
                self.raise_window(ctx);
            }
            MENU_ADD_FILE => {
                self.raise_window(ctx);
                let dir = self.settings.download_dir.clone();
                self.pick(ctx, PickPurpose::TorrentFiles, dir);
            }
            MENU_PAUSE_ALL => self.pause_all(),
            MENU_RESUME_ALL => self.resume_all(),
            MENU_QUIT => self.quit(ctx),
            _ => {}
        }
    }

    /// The window's close button (title-bar X, Alt+F4): hide to the tray, or
    /// let eframe close the window and exit.
    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        match tray::close_action(self.quitting, self.tray.is_some(), self.settings.close_to_tray) {
            CloseAction::Exit => self.quitting = true,
            CloseAction::HideToTray => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.win.hide(ctx);
                if !self.settings.tray_hint_shown {
                    self.settings.tray_hint_shown = true;
                    self.save_settings();
                    self.tray_hint();
                }
            }
        }
    }

    /// One-time notice that closing the window didn't quit: a Windows
    /// notification now, and a toast when the window is opened again (in
    /// case notifications are turned off).
    fn tray_hint(&mut self) {
        if let Some(t) = &self.tray {
            t.notify(
                "Zenless Torrent is still running",
                "Downloading and seeding continue in the background. Click the Zenless icon in the system tray \
                 to open the window again, or right-click it and choose Quit to exit.",
            );
        }
        self.tray_notice_pending = true;
    }

    /// The in-app half of [`Self::tray_hint`], once the window is back.
    fn show_pending_tray_notice(&mut self, ctx: &egui::Context) {
        if self.tray_notice_pending && self.win.on_screen(ctx) {
            self.tray_notice_pending = false;
            self.toast(
                "Zenless Torrent kept running in the tray. To exit, right-click the tray icon and choose Quit, \
                 or change this in Settings > General.",
                ToastKind::Info,
            );
        }
    }

    /// "Download complete" as an OS notification while the window is hidden
    /// in the tray (the in-app toast can't be seen then).
    pub(super) fn tray_notify_completed(&self, name: &str) {
        if self.win.is_hidden()
            && let Some(t) = &self.tray
        {
            t.notify("Download complete", name);
        }
    }

    /// Exits for real: tray "Quit", `POST /quit`, "Restart now" for an
    /// update. The close handler always lets this close through.
    pub(super) fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips() {
        assert_eq!(tooltip(0.0, 0.0), "Zenless Torrent · idle");
        assert_eq!(
            tooltip(3.1 * 1024.0 * 1024.0, 420.0 * 1024.0),
            "Zenless Torrent · ↓ 3.1 MB/s ↑ 420 KB/s"
        );
        assert_eq!(tooltip(0.0, 2048.0), "Zenless Torrent · ↓ 0 B/s ↑ 2.0 KB/s");
        assert!(tooltip(999e12, 999e12).chars().count() < 64);
    }

    #[test]
    fn menu_layout() {
        let m = menu(SHOW_LABEL);
        assert_eq!(m.first(), Some(&MenuEntry::item(MENU_WINDOW, SHOW_LABEL)));
        assert_eq!(m.last(), Some(&MenuEntry::item(MENU_QUIT, "Quit")));
        assert_eq!(m[m.len() - 2], MenuEntry::Separator, "a separator before Quit");
        assert!(m.contains(&MenuEntry::item(MENU_ADD_MAGNET, "Add magnet link…")));
        assert!(m.contains(&MenuEntry::item(MENU_ADD_FILE, "Add .torrent file…")));
    }

    #[test]
    fn the_icon_is_32x32_rgba() {
        assert_eq!(ICON.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }
}
