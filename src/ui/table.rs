//! The main torrent table.

use super::{App, widgets};
use crate::model::{Column, Snapshot, Status, TorrentView, format_peers, format_percent, format_ratio};
use crate::shared::{kit, theme::Palette};
use eframe::egui::{self, Align, Layout, RichText, Sense};
use egui_extras::{Column as Col, TableBuilder};
use egui_phosphor::regular as icon;
use std::cmp::Ordering;

const COLUMNS: [(Column, &str); 10] = [
    (Column::Name, "Name"),
    (Column::Size, "Size"),
    (Column::Progress, "Progress"),
    (Column::Status, "Status"),
    (Column::Down, "Down"),
    (Column::Up, "Up"),
    (Column::Peers, "Peers"),
    (Column::Eta, "ETA"),
    (Column::Ratio, "Ratio"),
    (Column::Added, "Added"),
];

/// Actions picked from the row context menu.
enum RowAction {
    Resume,
    Pause,
    OpenFolder,
    CopyMagnet,
    CopyHash,
    Recheck,
    Remove,
    RemoveWithData,
}

fn compare(a: &TorrentView, b: &TorrentView, col: Column) -> Ordering {
    match col {
        Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        Column::Size => a.total_bytes.cmp(&b.total_bytes),
        Column::Progress => a.progress().total_cmp(&b.progress()),
        Column::Status => a.status.order().cmp(&b.status.order()),
        Column::Down => a.down_bps.total_cmp(&b.down_bps),
        Column::Up => a.up_bps.total_cmp(&b.up_bps),
        Column::Peers => a.peers_live.cmp(&b.peers_live),
        Column::Eta => a
            .eta_secs
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.eta_secs.unwrap_or(f64::INFINITY)),
        Column::Ratio => a.ratio().total_cmp(&b.ratio()),
        Column::Added => a.added_at.cmp(&b.added_at),
    }
}

/// Columns that sort descending on first click.
fn default_descending(col: Column) -> bool {
    !matches!(col, Column::Name | Column::Status | Column::Eta)
}

fn added_text(t: &TorrentView, now: u64) -> String {
    if t.added_at == 0 {
        "—".into()
    } else if now.saturating_sub(t.added_at) < 86_400 {
        widgets::ago(t.added_at, now)
    } else {
        kit::human_date(t.added_at)[..10].to_owned()
    }
}

impl App {
    /// Indices into `snap.torrents` after filter, search and sort.
    pub(super) fn visible_rows(&self, snap: &Snapshot) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        let mut rows: Vec<usize> = snap
            .torrents
            .iter()
            .enumerate()
            .filter(|(_, t)| self.filter.matches(t.status))
            .filter(|(_, t)| {
                needle.is_empty()
                    || t.name.to_lowercase().contains(&needle)
                    || t.info_hash.starts_with(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        let col = self.settings.sort_column;
        let desc = self.settings.sort_descending;
        rows.sort_by(|&a, &b| {
            let (ta, tb) = (&snap.torrents[a], &snap.torrents[b]);
            let o = compare(ta, tb, col).then_with(|| ta.name.cmp(&tb.name));
            if desc { o.reverse() } else { o }
        });
        rows
    }

    fn header_cell(&mut self, ui: &mut egui::Ui, p: &Palette, col: Column, label: &str) {
        let active = self.settings.sort_column == col;
        let text = if active {
            let arrow = if self.settings.sort_descending { icon::CARET_DOWN } else { icon::CARET_UP };
            format!("{label} {arrow}")
        } else {
            label.to_owned()
        };
        let r = ui
            .add(
                egui::Label::new(RichText::new(text).small().strong().color(if active { p.text } else { p.text_dim }))
                    .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() {
            if active {
                self.settings.sort_descending = !self.settings.sort_descending;
            } else {
                self.settings.sort_column = col;
                self.settings.sort_descending = default_descending(col);
            }
            self.save_settings();
        }
    }

    pub(super) fn table(&mut self, ui: &mut egui::Ui, p: &Palette, snap: &Snapshot, rows: &[usize]) {
        ui.style_mut().interaction.selectable_labels = false;
        // A calmer selection highlight than the global text-selection color.
        ui.visuals_mut().selection.bg_fill = crate::shared::theme::mix(p.surface2, p.accent, 0.16);
        let now = kit::unix_now();
        let mut clicked: Option<(String, egui::Modifiers)> = None;
        let mut double: Option<String> = None;
        let mut right: Option<String> = None;
        let mut action: Option<RowAction> = None;

        let table = TableBuilder::new(ui)
            .id_salt("torrent-table")
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .auto_shrink([false, false])
            .column(Col::remainder().at_least(180.0).clip(true))
            .column(Col::initial(74.0).at_least(56.0).clip(true))
            .column(Col::initial(140.0).at_least(90.0))
            .column(Col::initial(100.0).at_least(92.0))
            .column(Col::initial(80.0).at_least(56.0).clip(true))
            .column(Col::initial(80.0).at_least(56.0).clip(true))
            .column(Col::initial(66.0).at_least(44.0).clip(true))
            .column(Col::initial(62.0).at_least(44.0).clip(true))
            .column(Col::initial(48.0).at_least(40.0).clip(true))
            .column(Col::initial(86.0).at_least(60.0).clip(true));

        table
            .header(28.0, |mut header| {
                for (col, label) in COLUMNS {
                    header.col(|ui| self.header_cell(ui, p, col, label));
                }
            })
            .body(|body| {
                body.rows(30.0, rows.len(), |mut row| {
                    let t = &snap.torrents[rows[row.index()]];
                    let selected = self.selected.contains(&t.key);
                    row.set_selected(selected);
                    let color = widgets::status_color(p, t.status);

                    // Name
                    row.col(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new(widgets::status_icon(t.status)).color(color));
                        let r = ui.add(egui::Label::new(RichText::new(&t.name).color(p.text)).truncate());
                        if let Some(e) = &t.error {
                            r.on_hover_text(format!("{}\n\n{e}", t.name));
                        }
                    });
                    // Size
                    row.col(|ui| {
                        let text = if t.total_bytes == 0 { "—".to_owned() } else { kit::human_bytes(t.total_bytes) };
                        ui.label(RichText::new(text).color(p.text_dim));
                    });
                    // Progress
                    row.col(|ui| {
                        let frac = t.progress();
                        let bar = match t.status {
                            Status::Seeding | Status::Completed => p.success,
                            _ => color,
                        };
                        let text = if t.status == Status::Metadata { "—".to_owned() } else { format_percent(frac) };
                        widgets::progress_cell(ui, p, frac, bar, &text);
                    });
                    // Status
                    row.col(|ui| {
                        kit::pill(ui, t.status.label(), color);
                    });
                    // Down / Up
                    row.col(|ui| {
                        let c = if t.down_bps >= 1.0 { p.accent } else { p.text_dim };
                        ui.label(RichText::new(kit::human_speed(t.down_bps)).color(c));
                    });
                    row.col(|ui| {
                        let c = if t.up_bps >= 1.0 { p.accent2 } else { p.text_dim };
                        ui.label(RichText::new(kit::human_speed(t.up_bps)).color(c));
                    });
                    // Peers
                    row.col(|ui| {
                        let text = if t.status.can_pause() && t.status != Status::Checking {
                            format_peers(t.peers_live, t.peers_seen)
                        } else {
                            "—".to_owned()
                        };
                        ui.label(RichText::new(text).color(p.text_dim));
                    });
                    // ETA
                    row.col(|ui| {
                        let text = match (t.status, t.eta_secs) {
                            (Status::Downloading, Some(s)) => kit::human_eta(s),
                            (Status::Downloading, None) if t.down_bps < 1.0 => icon::INFINITY.into(),
                            _ => "—".into(),
                        };
                        ui.label(RichText::new(text).color(p.text_dim));
                    });
                    // Ratio
                    row.col(|ui| {
                        let r = t.ratio();
                        let c = if r >= 1.0 { p.success } else { p.text_dim };
                        ui.label(RichText::new(format_ratio(r)).color(c));
                    });
                    // Added
                    row.col(|ui| {
                        let r = ui.label(RichText::new(added_text(t, now)).color(p.text_dim));
                        if t.added_at > 0 {
                            r.on_hover_text(format!("{} UTC", kit::human_date(t.added_at)));
                        }
                    });

                    let resp = row.response();
                    if resp.clicked() {
                        clicked = Some((t.key.clone(), resp.ctx.input(|i| i.modifiers)));
                    }
                    if resp.double_clicked() {
                        double = Some(t.key.clone());
                    }
                    if resp.secondary_clicked() {
                        right = Some(t.key.clone());
                    }
                    resp.context_menu(|ui| {
                        ui.set_min_width(210.0);
                        let item = |ui: &mut egui::Ui, ic: &str, text: &str, enabled: bool| {
                            ui.add_enabled(enabled, egui::Button::new(format!("{ic}  {text}")).frame(false))
                                .clicked()
                        };
                        let real = !t.pending;
                        if item(ui, icon::PLAY, "Resume", t.status.can_resume()) {
                            action = Some(RowAction::Resume);
                        }
                        if item(ui, icon::PAUSE, "Pause", t.status.can_pause()) {
                            action = Some(RowAction::Pause);
                        }
                        ui.separator();
                        if item(ui, icon::FOLDER_OPEN, "Open folder", true) {
                            action = Some(RowAction::OpenFolder);
                        }
                        if item(ui, icon::MAGNET, "Copy magnet link", !t.info_hash.is_empty()) {
                            action = Some(RowAction::CopyMagnet);
                        }
                        if item(ui, icon::HASH, "Copy info hash", !t.info_hash.is_empty()) {
                            action = Some(RowAction::CopyHash);
                        }
                        if item(ui, icon::ARROWS_CLOCKWISE, "Force recheck", real && t.status != Status::Checking) {
                            action = Some(RowAction::Recheck);
                        }
                        ui.separator();
                        if item(ui, icon::TRASH, "Remove…", true) {
                            action = Some(RowAction::Remove);
                        }
                        if item(ui, icon::X_CIRCLE, "Remove and delete files…", real) {
                            action = Some(RowAction::RemoveWithData);
                        }
                    });
                });
            });

        // Selection
        if let Some(key) = right
            && !self.selected.contains(&key)
        {
            self.selected = [key.clone()].into();
            self.anchor = Some(key);
        }
        if let Some((key, mods)) = clicked {
            if mods.command {
                if !self.selected.remove(&key) {
                    self.selected.insert(key.clone());
                }
                self.anchor = Some(key);
            } else if mods.shift && self.anchor.is_some() {
                let keys: Vec<&String> = rows.iter().map(|&i| &snap.torrents[i].key).collect();
                let a = self.anchor.as_ref().and_then(|a| keys.iter().position(|k| *k == a));
                let b = keys.iter().position(|k| **k == key);
                if let (Some(a), Some(b)) = (a, b) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    self.selected = keys[lo..=hi].iter().map(|k| (*k).clone()).collect();
                }
            } else {
                self.selected = [key.clone()].into();
                self.anchor = Some(key);
            }
        }
        if let Some(key) = double {
            self.open_folder_of(&[key]);
        }
        if let Some(action) = action {
            let keys = self.selected_keys();
            let ctx = ui.ctx().clone();
            match action {
                RowAction::Resume => self.resume(keys),
                RowAction::Pause => self.pause(keys),
                RowAction::OpenFolder => self.open_folder_of(&keys),
                RowAction::CopyMagnet => self.copy_magnets(&ctx, &keys),
                RowAction::CopyHash => self.copy_hashes(&ctx, &keys),
                RowAction::Recheck => self.recheck(keys),
                RowAction::Remove => self.ask_remove(keys, false),
                RowAction::RemoveWithData => self.ask_remove(keys, true),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TorrentView;

    #[test]
    fn sorting_helpers() {
        let mut a = TorrentView::placeholder("a", "Alpha", Status::Downloading);
        let mut b = TorrentView::placeholder("b", "beta", Status::Seeding);
        a.total_bytes = 10;
        b.total_bytes = 20;
        a.eta_secs = Some(5.0);
        assert_eq!(compare(&a, &b, Column::Name), Ordering::Less);
        assert_eq!(compare(&a, &b, Column::Size), Ordering::Less);
        assert_eq!(compare(&a, &b, Column::Status), Ordering::Less);
        assert_eq!(compare(&a, &b, Column::Eta), Ordering::Less);
        assert!(default_descending(Column::Added));
        assert!(!default_descending(Column::Name));
    }
}
