//! Bottom details panel: General, Files, Peers, Trackers and Speed tabs.

use super::{App, DetailsTab, widgets};
use crate::engine::Command;
use crate::model::{Snapshot, Status, TorrentView, format_percent, format_ratio};
use crate::shared::{kit, theme::Palette};
use eframe::egui::{self, Align, Layout, RichText, Vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;

const TABS: [(DetailsTab, &str, &str); 5] = [
    (DetailsTab::General, icon::INFO, "General"),
    (DetailsTab::Files, icon::FILES, "Files"),
    (DetailsTab::Peers, icon::USERS_THREE, "Peers"),
    (DetailsTab::Trackers, icon::BROADCAST, "Trackers"),
    (DetailsTab::Speed, icon::CHART_LINE, "Speed"),
];

impl App {
    pub(super) fn details(&mut self, ui: &mut egui::Ui, p: &Palette, snap: &Snapshot) {
        ui.style_mut().interaction.selectable_labels = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, ic, label) in TABS {
                let sel = self.details_tab == tab;
                let text = RichText::new(format!("{ic}  {label}")).color(if sel { p.text } else { p.text_dim });
                let r = ui.add(widgets::tab_button(p, text, sel));
                if r.clicked() {
                    self.details_tab = tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new(icon::CARET_DOWN).color(p.text_dim)).frame(false))
                    .on_hover_text("Hide details")
                    .clicked()
                {
                    self.settings.show_details = false;
                    self.save_settings();
                }
            });
        });
        ui.add_space(4.0);

        let selected = self.selected_views(snap);
        match selected.as_slice() {
            [] => {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Select a torrent to see its details").color(p.text_dim));
                });
            }
            [t] => {
                let t = (*t).clone();
                match self.details_tab {
                    DetailsTab::General => self.general_tab(ui, p, &t),
                    DetailsTab::Files => self.files_tab(ui, p, &t),
                    DetailsTab::Peers => self.peers_tab(ui, p, &t, snap),
                    DetailsTab::Trackers => self.trackers_tab(ui, p, &t),
                    DetailsTab::Speed => self.speed_tab(ui, p, &t),
                }
            }
            many => {
                let total: u64 = many.iter().map(|t| t.total_bytes).sum();
                let have: u64 = many.iter().map(|t| t.have_bytes).sum();
                let down: f64 = many.iter().map(|t| t.down_bps).sum();
                let up: f64 = many.iter().map(|t| t.up_bps).sum();
                ui.add_space(10.0);
                ui.label(RichText::new(format!("{} torrents selected", many.len())).size(16.0).strong());
                ui.label(
                    RichText::new(format!(
                        "{} of {} · {} {} · {} {}",
                        kit::human_bytes(have),
                        kit::human_bytes(total),
                        icon::ARROW_DOWN,
                        kit::human_speed(down),
                        icon::ARROW_UP,
                        kit::human_speed(up)
                    ))
                    .color(p.text_dim),
                );
            }
        }
    }

    fn general_tab(&mut self, ui: &mut egui::Ui, p: &Palette, t: &TorrentView) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Title row: name, status and progress
            let frac = t.progress();
            let status_color = widgets::status_color(p, t.status);
            let bar_color = match t.status {
                Status::Seeding | Status::Completed => p.success,
                _ => status_color,
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(widgets::status_icon(t.status)).size(17.0).color(status_color));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format_percent(frac)).size(15.0).strong().color(p.text));
                    kit::pill(ui, t.status.label(), status_color);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&t.name).size(15.0).strong()).truncate());
                    });
                });
            });
            let w = ui.available_width();
            kit::progress_bar(ui, p, frac, bar_color, w, 6.0);
            if let Some(e) = &t.error {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(crate::shared::theme::alpha(p.danger, 0.1))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(RichText::new(format!("{}  {e}", icon::WARNING_CIRCLE)).color(p.danger))
                                .wrap(),
                        );
                    });
            }
            ui.add_space(8.0);
            ui.spacing_mut().interact_size.y = 18.0;
            ui.columns(2, |cols| {
                kit::card(p).show(&mut cols[0], |ui| {
                    ui.set_width(ui.available_width());
                    kit::section_label(ui, p, "Transfer");
                    egui::Grid::new("gen-transfer").striped(false).num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
                        widgets::kv(
                            ui,
                            p,
                            "Downloaded",
                            format!("{} of {}", kit::human_bytes(t.have_bytes), kit::human_bytes(t.total_bytes)),
                        );
                        widgets::kv(ui, p, "Uploaded", kit::human_bytes(t.uploaded));
                        widgets::kv(ui, p, "Ratio", format_ratio(t.ratio()));
                        widgets::kv(
                            ui,
                            p,
                            "Speed",
                            format!(
                                "{} {}   {} {}",
                                icon::ARROW_DOWN,
                                kit::human_speed(t.down_bps),
                                icon::ARROW_UP,
                                kit::human_speed(t.up_bps)
                            ),
                        );
                        let eta = match (t.status, t.eta_secs) {
                            (Status::Downloading, Some(s)) => kit::human_eta(s),
                            (Status::Seeding | Status::Completed, _) => "Done".into(),
                            _ => "—".into(),
                        };
                        widgets::kv(ui, p, "Time left", eta);
                        widgets::kv(
                            ui,
                            p,
                            "Peers",
                            format!(
                                "{} connected · {} connecting · {} known",
                                t.peers_live, t.peers_connecting, t.peers_seen
                            ),
                        );
                    });
                });
                kit::card(p).show(&mut cols[1], |ui| {
                    ui.set_width(ui.available_width());
                    kit::section_label(ui, p, "Torrent");
                    egui::Grid::new("gen-info").striped(false).num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
                        ui.label(RichText::new("Info hash").color(p.text_dim));
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&t.info_hash).monospace().size(12.0)).truncate());
                            if !t.info_hash.is_empty()
                                && ui
                                    .add(egui::Button::new(RichText::new(icon::COPY).color(p.text_dim)).frame(false))
                                    .on_hover_text("Copy info hash")
                                    .clicked()
                            {
                                ui.ctx().copy_text(t.info_hash.clone());
                            }
                        });
                        ui.end_row();
                        ui.label(RichText::new("Save path").color(p.text_dim));
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(RichText::new(icon::FOLDER_OPEN).color(p.text_dim)).frame(false))
                                .on_hover_text("Open folder")
                                .clicked()
                            {
                                crate::platform::open_folder(&t.save_path);
                            }
                            ui.add(egui::Label::new(t.save_path.display().to_string()).truncate());
                        });
                        ui.end_row();
                        let pieces = if t.pieces > 0 {
                            format!("{} × {}", t.pieces, kit::human_bytes(t.piece_length as u64))
                        } else {
                            "—".into()
                        };
                        widgets::kv(ui, p, "Pieces", pieces);
                        let size = if t.full_size != t.total_bytes && t.full_size > 0 {
                            format!("{} (selected {})", kit::human_bytes(t.full_size), kit::human_bytes(t.total_bytes))
                        } else if t.full_size > 0 {
                            kit::human_bytes(t.full_size)
                        } else {
                            "—".into()
                        };
                        widgets::kv(ui, p, "Total size", size);
                        let added = if t.added_at > 0 { format!("{} UTC", kit::human_date(t.added_at)) } else { "—".into() };
                        widgets::kv(ui, p, "Added", added);
                        let done = t.completed_at.map(|c| format!("{} UTC", kit::human_date(c))).unwrap_or_else(|| "—".into());
                        widgets::kv(ui, p, "Completed", done);
                        if t.private {
                            widgets::kv(ui, p, "Privacy", "Private torrent (trackers only)");
                        }
                    });
                });
            });
        });
    }

    fn files_tab(&mut self, ui: &mut egui::Ui, p: &Palette, t: &TorrentView) {
        let files: Vec<usize> = (0..t.files.len()).filter(|&i| !t.files[i].padding).collect();
        if files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("The file list appears once the metadata has been received.").color(p.text_dim));
            });
            return;
        }
        let mut toggle: Option<(usize, bool)> = None;
        let mut set_all: Option<bool> = None;
        let editable = !t.pending && t.status != Status::Checking;
        ui.horizontal(|ui| {
            let selected = files.iter().filter(|&&i| t.included.get(i).copied().unwrap_or(true)).count();
            ui.label(
                RichText::new(format!("{selected} of {} files selected", files.len()))
                    .small()
                    .color(p.text_dim),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(editable, egui::Button::new("None").small()).clicked() {
                    set_all = Some(false);
                }
                if ui.add_enabled(editable, egui::Button::new("All").small()).clicked() {
                    set_all = Some(true);
                }
            });
        });
        TableBuilder::new(ui)
            .id_salt(("files", &t.key))
            .min_scrolled_height(40.0)
            .striped(true)
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(28.0))
            .column(Column::remainder().at_least(180.0).clip(true))
            .column(Column::exact(84.0))
            .column(Column::exact(170.0))
            .header(22.0, |mut h| {
                h.col(|_| {});
                for label in ["Name", "Size", "Progress"] {
                    h.col(|ui| {
                        ui.label(RichText::new(label).small().strong().color(p.text_dim));
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, files.len(), |mut row| {
                    let i = files[row.index()];
                    let f = &t.files[i];
                    let mut inc = t.included.get(i).copied().unwrap_or(true);
                    row.col(|ui| {
                        if ui.add_enabled(editable, egui::Checkbox::without_text(&mut inc)).changed() {
                            toggle = Some((i, inc));
                        }
                    });
                    row.col(|ui| {
                        let name = f.display();
                        ui.label(RichText::new(super::file_tree::file_icon(&name)).color(p.text_dim));
                        let color = if inc { p.text } else { p.text_dim };
                        ui.add(egui::Label::new(RichText::new(name).color(color)).truncate());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(kit::human_bytes(f.len)).color(p.text_dim));
                    });
                    row.col(|ui| {
                        let have = t.file_progress.get(i).copied().unwrap_or(0);
                        let frac = if f.len == 0 { 1.0 } else { (have as f64 / f.len as f64) as f32 };
                        let color = if frac >= 1.0 { p.success } else if inc { p.accent } else { p.text_dim };
                        widgets::progress_cell(ui, p, frac, color, &format_percent(frac));
                    });
                });
            });
        let new_selection = match (toggle, set_all) {
            (_, Some(all)) => Some(if all { files.clone() } else { Vec::new() }),
            (Some((i, v)), None) => Some(
                files
                    .iter()
                    .copied()
                    .filter(|&j| if j == i { v } else { t.included.get(j).copied().unwrap_or(true) })
                    .collect(),
            ),
            _ => None,
        };
        if let Some(sel) = new_selection {
            if sel.is_empty() {
                self.toast("Keep at least one file selected — or pause the torrent.", super::ToastKind::Info);
            } else {
                self.engine.send(Command::SetFiles {
                    key: t.key.clone(),
                    files: sel,
                });
            }
        }
    }

    fn peers_tab(&mut self, ui: &mut egui::Ui, p: &Palette, t: &TorrentView, snap: &Snapshot) {
        let peers = snap.peers.as_ref().filter(|(k, _)| *k == t.key).map(|(_, v)| v.as_slice());
        ui.label(
            RichText::new(format!(
                "{} connected · {} connecting · {} queued · {} known",
                t.peers_live, t.peers_connecting, t.peers_queued, t.peers_seen
            ))
            .small()
            .color(p.text_dim),
        );
        let Some(peers) = peers.filter(|p| !p.is_empty()) else {
            ui.centered_and_justified(|ui| {
                let msg = if matches!(t.status, Status::Downloading | Status::Seeding) {
                    "Looking for peers…"
                } else {
                    "Peers are shown while the torrent is active."
                };
                ui.label(RichText::new(msg).color(p.text_dim));
            });
            return;
        };
        TableBuilder::new(ui)
            .id_salt("peers")
            .min_scrolled_height(40.0)
            .striped(true)
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(210.0).at_least(120.0).clip(true))
            .column(Column::initial(200.0).at_least(100.0).clip(true))
            .column(Column::exact(54.0))
            .column(Column::initial(90.0))
            .column(Column::initial(90.0))
            .column(Column::initial(96.0))
            .column(Column::remainder().at_least(80.0))
            .header(22.0, |mut h| {
                for label in ["Address", "Client", "Type", "Down", "Up", "Downloaded", "Uploaded"] {
                    h.col(|ui| {
                        ui.label(RichText::new(label).small().strong().color(p.text_dim));
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, peers.len(), |mut row| {
                    let peer = &peers[row.index()];
                    row.col(|ui| {
                        ui.label(RichText::new(&peer.addr).monospace().size(12.0));
                    });
                    row.col(|ui| {
                        let c = if peer.client.is_empty() { "—" } else { &peer.client };
                        ui.add(egui::Label::new(c).truncate());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(&peer.kind).small().color(p.text_dim));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(kit::human_speed(peer.down_bps)).color(p.accent));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(kit::human_speed(peer.up_bps)).color(p.accent2));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(kit::human_bytes(peer.downloaded)).color(p.text_dim));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(kit::human_bytes(peer.uploaded)).color(p.text_dim));
                    });
                });
            });
    }

    fn trackers_tab(&mut self, ui: &mut egui::Ui, p: &Palette, t: &TorrentView) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let source_row = |ui: &mut egui::Ui, ic: &str, name: &str, state: String, on: bool| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic).color(if on { p.success } else { p.text_dim }));
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(state).color(p.text_dim));
                });
            };
            let dht_on = self.settings.enable_dht && !t.private;
            let dht_state = match (dht_on, self.snap.dht_nodes) {
                (false, _) if t.private => "disabled for private torrents".to_owned(),
                (false, _) => "turned off in settings".to_owned(),
                (true, Some(n)) => format!("working · {n} nodes"),
                (true, None) => "starting…".to_owned(),
            };
            source_row(ui, icon::GLOBE, "DHT", dht_state, dht_on);
            source_row(
                ui,
                icon::WIFI_HIGH,
                "Local peer discovery",
                if t.private { "disabled for private torrents".into() } else { "on".into() },
                !t.private,
            );
            ui.add_space(6.0);
            kit::section_label(ui, p, &format!("Trackers ({})", t.trackers.len()));
            if t.trackers.is_empty() {
                ui.label(RichText::new("This torrent has no trackers; peers come from DHT and other peers.").color(p.text_dim));
            }
            for tr in t.trackers.iter() {
                ui.horizontal(|ui| {
                    let ic = if tr.starts_with("udp") { icon::BROADCAST } else { icon::GLOBE_SIMPLE };
                    ui.label(RichText::new(ic).color(p.accent2));
                    ui.add(egui::Label::new(RichText::new(tr).monospace().size(12.5)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(RichText::new(icon::COPY).color(p.text_dim)).frame(false))
                            .on_hover_text("Copy URL")
                            .clicked()
                        {
                            ui.ctx().copy_text(tr.clone());
                        }
                    });
                });
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Per-tracker announce status is not reported by the BitTorrent engine yet.")
                    .small()
                    .color(p.text_dim),
            );
        });
    }

    fn speed_tab(&mut self, ui: &mut egui::Ui, p: &Palette, t: &TorrentView) {
        let samples: Vec<(f32, f32)> = self
            .histories
            .get(&t.key)
            .map(|h| h.iter().copied().collect())
            .unwrap_or_default();
        let avg = |f: fn(&(f32, f32)) -> f32| {
            if samples.is_empty() {
                0.0
            } else {
                samples.iter().map(f).sum::<f32>() as f64 / samples.len() as f64
            }
        };
        let (avg_d, avg_u) = (avg(|s| s.0), avg(|s| s.1));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            widgets::dot_label(ui, p.accent, RichText::new(format!("Download {}", kit::human_speed(t.down_bps))));
            widgets::dot_label(ui, p.accent2, RichText::new(format!("Upload {}", kit::human_speed(t.up_bps))));
            ui.label(
                RichText::new(format!(
                    "Average {} / {} over the last {} s",
                    kit::human_speed(avg_d),
                    kit::human_speed(avg_u),
                    samples.len()
                ))
                .small()
                .color(p.text_dim),
            );
        });
        ui.add_space(4.0);
        let size = Vec2::new(ui.available_width(), (ui.available_height() - 4.0).max(60.0));
        widgets::speed_chart(ui, p, size, &samples, true);
    }
}
