//! Settings dialog: General, Network, Browser integration, Appearance, About.

use super::{App, PickPurpose, SettingsTab, engine_config, widgets, widgets::ToastKind};
use crate::engine::Command;
use crate::platform;
use crate::shared::{
    kit,
    theme::{Palette, mix},
    tray,
    updater::UpdateAction,
};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

const TABS: [(SettingsTab, &str, &str); 5] = [
    (SettingsTab::General, icon::SLIDERS, "General"),
    (SettingsTab::Network, icon::WIFI_HIGH, "Network"),
    (SettingsTab::Browser, icon::PUZZLE_PIECE, "Browser integration"),
    (SettingsTab::Appearance, icon::PALETTE, "Appearance"),
    (SettingsTab::About, icon::INFO, "About"),
];

fn hint(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(p.text_dim)).wrap());
}

impl App {
    pub(super) fn settings_modal(&mut self, ctx: &egui::Context, p: &Palette) {
        let frame = egui::Frame::new()
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(0))
            .shadow(ctx.global_style().visuals.window_shadow);
        let screen = ctx.content_rect();
        let width = (screen.width() - 80.0).clamp(640.0, 900.0);
        let height = (screen.height() - 90.0).clamp(380.0, 600.0);
        let mut close = false;
        let resp = egui::Modal::new(egui::Id::new("settings")).frame(frame).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.horizontal_top(|ui| {
                let default_spacing = ui.spacing().item_spacing;
                ui.spacing_mut().item_spacing.x = 0.0;
                // Tab column
                egui::Frame::new()
                    .fill(mix(p.surface, p.bg, 0.5))
                    .corner_radius(egui::CornerRadius {
                        nw: 14,
                        sw: 14,
                        ne: 0,
                        se: 0,
                    })
                    .inner_margin(egui::Margin::symmetric(10, 16))
                    .show(ui, |ui| {
                        ui.set_width(190.0);
                        ui.set_height(height - 32.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = default_spacing;
                            ui.horizontal(|ui| {
                                ui.add_space(6.0);
                                ui.heading("Settings");
                            });
                            ui.add_space(10.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for (tab, ic, label) in TABS {
                                let sel = self.settings_tab == tab;
                                let text =
                                    RichText::new(format!("{ic}   {label}")).color(if sel { p.text } else { p.text_dim });
                                let r = ui.add(super::widgets::tab_button(p, text, sel).min_size(egui::vec2(
                                    ui.available_width(),
                                    32.0,
                                )));
                                if r.clicked() {
                                    self.settings_tab = tab;
                                }
                            }
                        });
                    });
                // Content
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = default_spacing;
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(22, 16)).show(ui, |ui| {
                        ui.set_width(width - 190.0 - 20.0 - 44.0);
                        ui.horizontal(|ui| {
                            let title = TABS.iter().find(|t| t.0 == self.settings_tab).map(|t| t.2).unwrap_or("");
                            ui.label(RichText::new(title).size(18.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(egui::Button::new(RichText::new(icon::X).size(16.0)).frame(false))
                                    .on_hover_text("Close (Esc)")
                                    .clicked()
                                {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .max_height(height - 80.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width() - 8.0);
                                match self.settings_tab {
                                    SettingsTab::General => self.settings_general(ui, p),
                                    SettingsTab::Network => self.settings_network(ui, p),
                                    SettingsTab::Browser => self.settings_browser(ui, p),
                                    SettingsTab::Appearance => self.themes.appearance_ui(ui),
                                    SettingsTab::About => self.settings_about(ui, p),
                                }
                            });
                    });
                });
            });
        });
        if close || resp.should_close() {
            self.settings_open = false;
        }
    }

    fn settings_general(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let mut changed = false;
        kit::section_label(ui, p, "Downloads");
        ui.label("Save new torrents to");
        ui.horizontal(|ui| {
            let mut text = self.settings.download_dir.display().to_string();
            let w = ui.available_width() - 190.0;
            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(w)).changed() {
                self.settings.download_dir = text.into();
                changed = true;
            }
            if ui.button(format!("{}  Browse", icon::FOLDER_OPEN)).clicked() {
                let dir = self.settings.download_dir.clone();
                self.pick(ui.ctx(), PickPurpose::DefaultFolder, dir);
            }
            if ui.button("Open").clicked() {
                platform::open_folder(&self.settings.download_dir);
            }
        });
        ui.add_space(4.0);
        changed |= ui
            .checkbox(&mut self.settings.start_immediately, "Start torrents immediately after adding them")
            .changed();
        changed |= ui
            .checkbox(&mut self.settings.notify_on_complete, "Notify me when a download completes")
            .changed();

        ui.add_space(14.0);
        kit::section_label(ui, p, "System tray");
        changed |= ui
            .add_enabled(
                tray::supported(),
                egui::Checkbox::new(&mut self.settings.tray_icon, "Show an icon in the system tray"),
            )
            .on_hover_text(
                "Click the icon to show or hide the window. Right-click it to add torrents, pause or resume \
                 everything, or quit.",
            )
            .changed();
        ui.add_enabled_ui(tray::supported() && self.settings.tray_icon, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                changed |= ui
                    .checkbox(&mut self.settings.close_to_tray, "Keep running in the tray when the window is closed")
                    .on_hover_text("Downloading and seeding continue in the background. Quit from the tray icon's menu.")
                    .changed();
            });
        });
        if let Some(e) = &self.tray_error {
            let text = format!("{}  The tray icon couldn't be created: {e}", icon::WARNING_CIRCLE);
            ui.label(RichText::new(text).color(p.danger));
        }

        ui.add_space(14.0);
        kit::section_label(ui, p, "Startup");
        let mut autostart = self.autostart_on;
        if ui.checkbox(&mut autostart, "Start Zenless Torrent when I sign in to Windows").changed() {
            match (!self.demo)
                .then(|| platform::set_autostart(autostart, self.settings.start_minimized))
                .unwrap_or(Ok(()))
            {
                Ok(()) => {
                    self.autostart_on = autostart;
                    self.settings.start_with_windows = autostart;
                    changed = true;
                }
                Err(e) => self.toast(format!("Could not change autostart: {e}"), ToastKind::Error),
            }
        }
        let minimized_label = if self.settings.tray_icon && tray::supported() {
            "Start in the tray when launched at sign-in"
        } else {
            "Start minimized when launched at sign-in"
        };
        if ui
            .checkbox(&mut self.settings.start_minimized, minimized_label)
            .changed()
        {
            changed = true;
            if self.autostart_on
                && !self.demo
                && let Err(e) = platform::set_autostart(true, self.settings.start_minimized)
            {
                self.toast(format!("Could not update autostart: {e}"), ToastKind::Error);
            }
        }

        ui.add_space(14.0);
        kit::section_label(ui, p, "Default app");
        kit::card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (ic, c, text) = if self.handlers_on {
                    (icon::CHECK_CIRCLE, p.success, "Magnet links and .torrent files open in Zenless Torrent.")
                } else {
                    (icon::INFO, p.info, "Zenless Torrent is not registered for magnet links and .torrent files.")
                };
                ui.label(RichText::new(ic).size(18.0).color(c));
                ui.add(egui::Label::new(text).wrap());
            });
            ui.add_space(6.0);
            if kit::primary_button(ui, p, "Make Zenless Torrent the default for magnet links and .torrent files").clicked() {
                let result = if self.demo { Ok(()) } else { platform::register_handlers() };
                match result {
                    Ok(()) => {
                        self.handlers_on = true;
                        self.toast(
                            "Registered. If Windows still uses another app, pick Zenless Torrent in Default apps.",
                            ToastKind::Success,
                        );
                    }
                    Err(e) => self.toast(format!("Could not register: {e}"), ToastKind::Error),
                }
            }
            ui.add_space(2.0);
            if ui.button(format!("{}  Open Windows Default apps", icon::ARROW_SQUARE_OUT)).clicked() {
                platform::open_url("ms-settings:defaultapps");
            }
            hint(
                ui,
                p,
                "Windows 10 and 11 may ask you to confirm the choice once in Settings > Apps > Default apps.",
            );
        });
        if changed {
            self.save_settings();
        }
    }

    fn settings_network(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let mut changed = false;
        kit::section_label(ui, p, "Connections");
        egui::Grid::new("net-grid").striped(false).num_columns(2).spacing([18.0, 10.0]).show(ui, |ui| {
            ui.label("Listening port");
            ui.horizontal(|ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut self.settings.listen_port).range(1024..=65535).speed(1))
                    .changed();
                if ui.button("Random").clicked() {
                    let r = (crate::shared::kit::unix_now() as u32).wrapping_mul(2_654_435_761) % 40_000;
                    self.settings.listen_port = 20_000 + r as u16;
                    changed = true;
                }
                if let Some(port) = self.snap.listen_port {
                    ui.label(RichText::new(format!("currently {port}")).small().color(p.text_dim));
                }
            });
            ui.end_row();
        });
        changed |= ui
            .checkbox(&mut self.settings.enable_dht, "Find peers with DHT (recommended)")
            .changed();
        changed |= ui
            .checkbox(&mut self.settings.enable_upnp, "Forward the port on my router with UPnP")
            .changed();
        changed |= ui
            .checkbox(&mut self.settings.enable_utp, "Enable µTP connections (experimental)")
            .changed();

        if self.settings.network_key() != self.applied_network {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(crate::shared::theme::alpha(p.warning, 0.12))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::ARROWS_CLOCKWISE).color(p.warning));
                        ui.label("These changes take effect after the engine restarts.");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::primary_button(ui, p, "Apply now").clicked() {
                                self.applied_network = self.settings.network_key();
                                self.engine.send(Command::Restart(engine_config(&self.settings)));
                                self.toast("Restarting the BitTorrent engine…", ToastKind::Info);
                            }
                        });
                    });
                });
        }

        ui.add_space(14.0);
        kit::section_label(ui, p, "Speed limits");
        let mut limits_changed = false;
        egui::Grid::new("limit-grid").striped(false).num_columns(2).spacing([18.0, 10.0]).show(ui, |ui| {
            for (label, value) in [
                ("Download", &mut self.settings.download_limit_kib),
                ("Upload", &mut self.settings.upload_limit_kib),
            ] {
                let mut on = *value > 0;
                if ui.checkbox(&mut on, format!("Limit {}", label.to_lowercase())).changed() {
                    *value = if on { 1024 } else { 0 };
                    limits_changed = true;
                }
                ui.add_enabled_ui(on, |ui| {
                    ui.horizontal(|ui| {
                        let mut v = (*value).max(1);
                        if ui
                            .add(egui::DragValue::new(&mut v).range(1..=1_000_000).speed(8).suffix(" KiB/s"))
                            .changed()
                            && on
                        {
                            *value = v;
                            limits_changed = true;
                        }
                        if on {
                            ui.label(RichText::new(kit::human_speed(*value as f64 * 1024.0)).small().color(p.text_dim));
                        } else {
                            ui.label(RichText::new("unlimited").small().color(p.text_dim));
                        }
                    });
                });
                ui.end_row();
            }
        });
        hint(ui, p, "Speed limits apply immediately to all torrents.");
        if limits_changed {
            changed = true;
            self.engine.send(Command::SetLimits {
                download_bps: self.settings.download_limit_bps(),
                upload_bps: self.settings.upload_limit_bps(),
            });
        }
        if changed {
            self.save_settings();
        }
    }

    fn settings_browser(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let status = self
            .api_status
            .as_ref()
            .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).clone());
        kit::card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (ic, c, text) = match &status {
                    Some(s) if s.listening => (
                        icon::PLUGS_CONNECTED,
                        p.success,
                        format!("Local API is listening on 127.0.0.1:{}", crate::api::port()),
                    ),
                    Some(s) => (
                        icon::PLUGS,
                        p.danger,
                        s.error.clone().unwrap_or_else(|| "Local API is starting…".into()),
                    ),
                    None => (icon::PLUGS, p.text_dim, "Local API is off in demo mode".into()),
                };
                ui.label(RichText::new(ic).size(20.0).color(c));
                ui.add(egui::Label::new(RichText::new(text).strong()).wrap());
            });
            ui.add_space(4.0);
            let last = status.as_ref().and_then(|s| s.last_extension.clone());
            let text = match last {
                Some((client, when)) => format!(
                    "Last contact from a browser extension: {client}, {}",
                    widgets::ago(when, kit::unix_now())
                ),
                None if self.demo => "Last contact from a browser extension: chrome-extension/0.1.0, 3 min ago".into(),
                None => "No browser extension has connected yet.".into(),
            };
            ui.label(RichText::new(text).color(p.text_dim));
        });
        ui.add_space(12.0);
        kit::section_label(ui, p, "How it works");
        hint(
            ui,
            p,
            "The Zenless browser extension sends magnet links and .torrent downloads straight to this app. \
             Only browser extensions and programs on this PC can talk to it — websites cannot.",
        );
        ui.add_space(12.0);
        kit::section_label(ui, p, "Install the extension");
        let step = |ui: &mut egui::Ui, n: usize, text: &str| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{n}")).strong().color(p.accent));
                ui.add(egui::Label::new(text).wrap());
            });
        };
        ui.label(RichText::new(format!("{}  Chrome, Edge, Brave, Opera", icon::BROWSER)).strong());
        step(ui, 1, "Run Zenless Setup and tick 'Browser extensions', or download zenless-chrome-extension.zip.");
        step(ui, 2, "Open chrome://extensions, switch on Developer mode and choose 'Load unpacked'.");
        step(ui, 3, "Select the folder …\\Programs\\Zenless\\Browser Extensions\\Chrome.");
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{}  Firefox", icon::BROWSER)).strong());
        step(ui, 1, "Download zenless-firefox-extension.xpi (the installer also places it on your PC).");
        step(ui, 2, "Open about:addons, open the gear menu, choose 'Install Add-on From File' and pick the .xpi.");
        ui.add_space(10.0);
        if kit::primary_button(ui, p, format!("{}  Get the extensions", icon::DOWNLOAD_SIMPLE)).clicked() {
            platform::open_url(platform::DOWNLOAD_PAGE);
        }
    }

    fn settings_about(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            widgets::logo(ui, &self.logo, 64.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("Zenless Torrent").size(22.0).strong());
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(p.text_dim));
            });
        });
        ui.add_space(10.0);
        ui.label("A calm, fast BitTorrent client — part of the Zenless suite.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(format!("{}  Website", icon::GLOBE)).clicked() {
                platform::open_url(platform::WEBSITE);
            }
            if ui.button(format!("{}  Source code", icon::GITHUB_LOGO)).clicked() {
                platform::open_url(platform::REPO);
            }
            if ui.button(format!("{}  Data folder", icon::FOLDER_OPEN)).clicked() {
                platform::open_folder(&crate::settings::data_dir());
            }
        });
        ui.add_space(14.0);
        kit::section_label(ui, p, "Updates");
        if self.updater.settings_ui(ui, p) == Some(UpdateAction::RestartNow) {
            let ctx = ui.ctx().clone();
            self.restart_for_update(&ctx);
        }
        ui.add_space(14.0);
        kit::section_label(ui, p, "Credits");
        hint(
            ui,
            p,
            &format!(
                "BitTorrent engine: librqbit {} (Apache-2.0) · UI: egui / eframe (MIT/Apache-2.0) · \
                 Icons: Phosphor (MIT).",
                librqbit::version()
            ),
        );
        hint(ui, p, "Zenless Torrent is released under the MIT license. Copyright (c) 2026 Zenless.");
        ui.add_space(8.0);
        hint(
            ui,
            p,
            &format!("Settings and session data: {}", crate::settings::data_dir().display()),
        );
    }
}
