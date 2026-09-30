//! Top toolbar: add / control buttons and the search box.

use super::{App, MagnetInput, PickPurpose, SettingsTab, widgets};
use crate::shared::{kit, theme::Palette};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

impl App {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .stroke(egui::Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(4.0);
                    widgets::logo(ui, &self.logo, 30.0);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Zenless").strong().size(14.0).color(p.text));
                        ui.label(RichText::new("TORRENT").size(9.5).color(p.accent2));
                    });
                    ui.add_space(14.0);
                    separator(ui, p);

                    let keys = self.selected_keys();
                    let sel: Vec<_> = self.snap.torrents.iter().filter(|t| self.selected.contains(&t.key)).collect();
                    let can_resume = sel.iter().any(|t| t.status.can_resume());
                    let can_pause = sel.iter().any(|t| t.status.can_pause());
                    let any_resume = self.snap.torrents.iter().any(|t| t.status.can_resume());
                    let any_pause = self.snap.torrents.iter().any(|t| t.status.can_pause());
                    let has_sel = !sel.is_empty();

                    if kit::toolbar_button(ui, p, icon::MAGNET, "Add link", true)
                        .on_hover_text("Add a magnet link (Ctrl+U)")
                        .clicked()
                    {
                        self.magnet_input = Some(MagnetInput {
                            text: String::new(),
                            focus: true,
                        });
                    }
                    if kit::toolbar_button(ui, p, icon::FILE_PLUS, "Add file", true)
                        .on_hover_text("Open .torrent files (Ctrl+O)")
                        .clicked()
                    {
                        let dir = self.settings.download_dir.clone();
                        self.pick(ui.ctx(), PickPurpose::TorrentFiles, dir);
                    }
                    separator(ui, p);
                    if kit::toolbar_button(ui, p, icon::PLAY, "Resume", can_resume).clicked() {
                        self.resume(keys.clone());
                    }
                    if kit::toolbar_button(ui, p, icon::PAUSE, "Pause", can_pause).clicked() {
                        self.pause(keys.clone());
                    }
                    if kit::toolbar_button(ui, p, icon::PLAY_CIRCLE, "Resume all", any_resume).clicked() {
                        self.resume_all();
                    }
                    if kit::toolbar_button(ui, p, icon::PAUSE_CIRCLE, "Pause all", any_pause).clicked() {
                        self.pause_all();
                    }
                    separator(ui, p);
                    if kit::toolbar_button(ui, p, icon::TRASH, "Remove", has_sel)
                        .on_hover_text("Remove the selected torrents (Del)")
                        .clicked()
                    {
                        self.ask_remove(keys.clone(), false);
                    }
                    if kit::toolbar_button(ui, p, icon::FOLDER_OPEN, "Open folder", has_sel).clicked() {
                        self.open_folder_of(&keys);
                    }
                    separator(ui, p);
                    if kit::toolbar_button(ui, p, icon::GEAR_SIX, "Settings", true).clicked() {
                        self.open_settings(SettingsTab::General);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        search_box(ui, p, &mut self.search);
                    });
                });
            });
    }
}

fn separator(ui: &mut egui::Ui, p: &Palette) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 34.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border);
    ui.add_space(6.0);
}

fn search_box(ui: &mut egui::Ui, p: &Palette, text: &mut String) {
    egui::Frame::new()
        .fill(p.input)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if !text.is_empty() {
                    if ui
                        .add(egui::Button::new(RichText::new(icon::X).color(p.text_dim)).frame(false))
                        .on_hover_text("Clear")
                        .clicked()
                    {
                        text.clear();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text("Search torrents")
                        .frame(egui::Frame::NONE)
                        .desired_width(200.0),
                );
                ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(p.text_dim));
            });
        });
}
