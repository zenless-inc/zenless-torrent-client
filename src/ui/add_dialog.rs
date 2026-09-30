//! "Add torrent" dialog: metadata spinner → name, size, file tree, folder.

use super::file_tree::{self, Node};
use crate::model::{AddSource, ResolvedTorrent};
use crate::settings::Settings;
use crate::shared::{kit, theme::Palette};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use std::time::Instant;

pub enum AddState {
    Resolving,
    Failed(String),
    Ready(ResolvedTorrent),
}

pub enum AddOutcome {
    None,
    Cancel,
    Add,
    Browse,
    /// The torrent already exists: select it in the list.
    ShowExisting(String),
}

pub struct AddDialog {
    pub req: u64,
    pub source: AddSource,
    pub client: String,
    started: Instant,
    pub state: AddState,
    pub selected: Vec<bool>,
    tree: Option<Node>,
    pub folder: String,
    pub start: bool,
}

impl AddDialog {
    pub fn new(req: u64, source: AddSource, client: String, settings: &Settings) -> Self {
        Self {
            req,
            source,
            client,
            started: Instant::now(),
            state: AddState::Resolving,
            selected: Vec::new(),
            tree: None,
            folder: settings.download_dir.display().to_string(),
            start: settings.start_immediately,
        }
    }

    pub fn is_resolving(&self) -> bool {
        matches!(self.state, AddState::Resolving)
    }

    pub fn resolved(&self) -> Option<&ResolvedTorrent> {
        match &self.state {
            AddState::Ready(r) => Some(r),
            _ => None,
        }
    }

    pub fn set_resolved(&mut self, result: Result<ResolvedTorrent, String>) {
        match result {
            Ok(r) => {
                self.selected = r.files.iter().map(|f| !f.padding).collect();
                self.tree = Some(file_tree::build(&r.files));
                self.state = AddState::Ready(r);
            }
            Err(e) => self.state = AddState::Failed(e),
        }
    }

    fn selected_size(&self) -> (u64, usize, usize) {
        let Some(r) = self.resolved() else { return (0, 0, 0) };
        let mut size = 0;
        let mut n = 0;
        let mut total = 0;
        for (f, s) in r.files.iter().zip(&self.selected) {
            if f.padding {
                continue;
            }
            total += 1;
            if *s {
                size += f.len;
                n += 1;
            }
        }
        (size, n, total)
    }

    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, queued: usize) -> AddOutcome {
        let mut outcome = AddOutcome::None;
        let resp = egui::Modal::new(egui::Id::new("add-torrent")).show(ctx, |ui| {
            ui.set_width(640.0);
            // Header
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::MAGNET).size(24.0).color(p.accent));
                ui.vertical(|ui| {
                    ui.heading("Add torrent");
                    let mut sub = format!("from {}", self.client);
                    if queued > 0 {
                        sub.push_str(&format!(" · {queued} more waiting"));
                    }
                    ui.label(RichText::new(sub).small().color(p.text_dim));
                });
            });
            ui.add_space(8.0);

            match &self.state {
                AddState::Resolving => {
                    kit::card(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(22.0).color(p.accent));
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(self.source.describe()).strong()).truncate(),
                                );
                                let secs = self.started.elapsed().as_secs();
                                let text = if self.source.is_magnet() {
                                    format!("Fetching metadata from peers… {secs}s")
                                } else {
                                    format!("Reading torrent… {secs}s")
                                };
                                ui.label(RichText::new(text).color(p.text_dim));
                            });
                        });
                        if self.source.is_magnet() && self.started.elapsed().as_secs() > 20 {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "This can take a while for rare torrents. You can keep waiting or cancel.",
                                )
                                .small()
                                .color(p.text_dim),
                            );
                        }
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                }
                AddState::Failed(e) => {
                    egui::Frame::new()
                        .fill(crate::shared::theme::alpha(p.danger, 0.1))
                        .stroke(egui::Stroke::new(1.0, crate::shared::theme::alpha(p.danger, 0.5)))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon::WARNING_CIRCLE).size(20.0).color(p.danger));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Could not read this torrent").strong());
                                    ui.add(egui::Label::new(RichText::new(e).color(p.text_dim)).wrap());
                                });
                            });
                        });
                }
                AddState::Ready(r) => {
                    let r = r.clone();
                    self.ready_body(ui, p, &r, &mut outcome);
                }
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match &self.state {
                        AddState::Ready(r) if r.already_added => {
                            if kit::primary_button(ui, p, "Show in list").clicked() {
                                outcome = AddOutcome::ShowExisting(r.info_hash.clone());
                            }
                            if ui.button("Close").clicked() {
                                outcome = AddOutcome::Cancel;
                            }
                        }
                        AddState::Ready(_) => {
                            let (_, n, _) = self.selected_size();
                            let ok = n > 0 && !self.folder.trim().is_empty();
                            let label = if self.start { "Add and start" } else { "Add paused" };
                            let btn = ui.add_enabled(
                                ok,
                                egui::Button::new(
                                    RichText::new(format!("{}  {label}", icon::DOWNLOAD_SIMPLE))
                                        .color(p.accent_fg)
                                        .strong(),
                                )
                                .fill(p.accent),
                            );
                            if btn.clicked() || (ok && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                outcome = AddOutcome::Add;
                            }
                            if ui.button("Cancel").clicked() {
                                outcome = AddOutcome::Cancel;
                            }
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.checkbox(&mut self.start, "Start immediately");
                            });
                        }
                        _ => {
                            if ui.button("Cancel").clicked() {
                                outcome = AddOutcome::Cancel;
                            }
                        }
                    }
                });
            });
        });
        if resp.should_close() && matches!(outcome, AddOutcome::None) {
            outcome = AddOutcome::Cancel;
        }
        outcome
    }

    fn ready_body(&mut self, ui: &mut egui::Ui, p: &Palette, r: &ResolvedTorrent, outcome: &mut AddOutcome) {
        // Summary card
        kit::card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&r.name).size(16.0).strong()).truncate());
            ui.add_space(2.0);
            let (sel, n, total) = self.selected_size();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let dim = |t: String| RichText::new(t).small().color(p.text_dim);
                ui.label(dim(format!("{} {}", icon::HARD_DRIVES, kit::human_bytes(r.total_size))));
                ui.label(dim(format!(
                    "{} {} file{}",
                    icon::FILES,
                    total,
                    if total == 1 { "" } else { "s" }
                )));
                ui.label(dim(format!(
                    "{} {} × {}",
                    icon::SQUARES_FOUR,
                    r.pieces,
                    kit::human_bytes(r.piece_length as u64)
                )));
                if !r.trackers.is_empty() {
                    let n = r.trackers.len();
                    ui.label(dim(format!("{} {n} tracker{}", icon::BROADCAST, if n == 1 { "" } else { "s" })));
                }
                if r.private {
                    ui.label(RichText::new(format!("{} Private", icon::LOCK)).small().color(p.warning));
                }
                let short = r.info_hash.get(..16).unwrap_or(&r.info_hash);
                ui.label(dim(format!("{} {short}…", icon::HASH)));
            });
            if r.already_added {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{}  This torrent is already in your list.", icon::INFO)).color(p.info),
                );
            } else if n < total {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("Selected: {} in {n} of {total} files", kit::human_bytes(sel)))
                        .small()
                        .color(p.accent),
                );
            }
        });
        if r.already_added {
            return;
        }

        // Files
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            kit::section_label(ui, p, "Files");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("None").clicked() {
                    self.selected.iter_mut().for_each(|s| *s = false);
                }
                if ui.small_button("All").clicked() {
                    for (s, f) in self.selected.iter_mut().zip(&r.files) {
                        *s = !f.padding;
                    }
                }
            });
        });
        egui::Frame::new()
            .fill(p.input)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.style_mut().interaction.selectable_labels = false;
                        if let Some(tree) = &self.tree {
                            file_tree::show(ui, p, tree, &mut self.selected, egui::Id::new(("add-tree", self.req)), 0);
                        }
                    });
            });

        // Folder
        ui.add_space(10.0);
        kit::section_label(ui, p, "Save to");
        ui.horizontal(|ui| {
            let w = ui.available_width() - 96.0;
            ui.add(
                egui::TextEdit::singleline(&mut self.folder)
                    .desired_width(w)
                    .hint_text("Folder"),
            );
            if ui.button(format!("{}  Browse", icon::FOLDER_OPEN)).clicked() {
                *outcome = AddOutcome::Browse;
            }
        });
        if r.files.iter().filter(|f| !f.padding).count() > 1 {
            let target = crate::engine::output_folder_for(
                std::path::Path::new(self.folder.trim()),
                &r.name,
                r.files.len(),
            );
            ui.label(
                RichText::new(format!("Files go into {}", target.display()))
                    .small()
                    .color(p.text_dim),
            );
        }
    }
}
