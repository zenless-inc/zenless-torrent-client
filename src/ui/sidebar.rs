//! Left sidebar: status filters with counts and the global speed card.

use super::{App, widgets};
use crate::model::{Filter, Snapshot};
use crate::shared::{
    kit,
    theme::{Palette, mix},
};
use eframe::egui::{self, Align2, CornerRadius, FontId, RichText, Sense, Vec2};
use egui_phosphor::regular as icon;

fn filter_icon(f: Filter) -> &'static str {
    match f {
        Filter::All => icon::SQUARES_FOUR,
        Filter::Downloading => icon::ARROW_DOWN,
        Filter::Seeding => icon::ARROW_UP,
        Filter::Completed => icon::CHECK_CIRCLE,
        Filter::Paused => icon::PAUSE,
        Filter::Error => icon::WARNING_CIRCLE,
    }
}

impl App {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui, p: &Palette, snap: &Snapshot) {
        egui::Panel::left("sidebar")
            .exact_size(212.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(10, 12))
                    .stroke(egui::Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                egui::Panel::bottom("sidebar-speed")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .resizable(false)
                    .show(ui, |ui| self.speed_card(ui, p, snap));
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    kit::section_label(ui, p, "Torrents");
                });
                ui.add_space(4.0);
                for f in Filter::ALL {
                    let count = snap.count_filter(f);
                    if self.filter_row(ui, p, f, count) {
                        self.filter = f;
                    }
                }
            });
    }

    fn filter_row(&self, ui: &mut egui::Ui, p: &Palette, f: Filter, count: usize) -> bool {
        let selected = self.filter == f;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(8), mix(p.surface, p.accent, 0.14));
            let bar = egui::Rect::from_min_size(rect.left_top() + Vec2::new(0.0, 8.0), Vec2::new(3.0, 18.0));
            painter.rect_filled(bar, CornerRadius::same(2), p.accent);
        } else if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(8), mix(p.surface, p.surface2, 0.7));
        }
        let fg = if selected { p.text } else { mix(p.text, p.text_dim, 0.35) };
        let icon_color = if selected {
            p.accent
        } else if f == Filter::Error && count > 0 {
            p.danger
        } else {
            p.text_dim
        };
        painter.text(
            rect.left_center() + Vec2::new(14.0, 0.0),
            Align2::LEFT_CENTER,
            filter_icon(f),
            FontId::proportional(16.0),
            icon_color,
        );
        painter.text(
            rect.left_center() + Vec2::new(40.0, 0.0),
            Align2::LEFT_CENTER,
            f.label(),
            FontId::proportional(14.0),
            fg,
        );
        // count badge
        let text = count.to_string();
        let badge_fg = if selected { p.accent_fg } else { p.text_dim };
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), badge_fg);
        let bw = (galley.size().x + 12.0).max(24.0);
        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 8.0 - bw / 2.0, rect.center().y),
            Vec2::new(bw, 18.0),
        );
        let badge_fill = if selected { p.accent } else { mix(p.surface2, p.bg, 0.25) };
        painter.rect_filled(badge, CornerRadius::same(9), badge_fill);
        painter.galley(badge.center() - galley.size() / 2.0, galley, badge_fg);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    fn speed_card(&self, ui: &mut egui::Ui, p: &Palette, snap: &Snapshot) {
        let samples: Vec<(f32, f32)> = self.global_history.iter().copied().collect();
        kit::card(p).fill(mix(p.surface, p.bg, 0.45)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.speed_card_body(ui, p, snap, &samples);
        });
    }

    fn speed_card_body(&self, ui: &mut egui::Ui, p: &Palette, snap: &Snapshot, samples: &[(f32, f32)]) {
        {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::ARROW_DOWN).color(p.accent));
                ui.label(RichText::new(kit::human_speed(snap.down_bps)).size(15.0).strong().color(p.text));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::ARROW_UP).color(p.accent2));
                ui.label(RichText::new(kit::human_speed(snap.up_bps)).size(15.0).strong().color(p.text));
            });
            ui.add_space(2.0);
            let w = ui.available_width();
            widgets::speed_chart(ui, p, Vec2::new(w, 46.0), samples, false);
            let limits = match (self.settings.download_limit_kib, self.settings.upload_limit_kib) {
                (0, 0) => "No speed limits".to_owned(),
                (d, u) => {
                    let f = |v: u32| if v == 0 { icon::INFINITY.to_owned() } else { kit::human_speed(v as f64 * 1024.0) };
                    format!("Limits {} / {}", f(d), f(u))
                }
            };
            ui.label(RichText::new(limits).small().color(p.text_dim));
        }
    }
}
