//! App-specific widgets: status colors/icons, graphs, toasts, small helpers.

use crate::model::Status;
use crate::shared::theme::{Palette, alpha, mix};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};
use egui_phosphor::regular as icon;
use std::time::{Duration, Instant};

pub fn status_color(p: &Palette, s: Status) -> Color32 {
    match s {
        Status::Downloading => p.accent,
        Status::Seeding => p.success,
        Status::Completed => p.accent2,
        Status::Paused => p.text_dim,
        Status::Checking => p.warning,
        Status::Metadata => p.info,
        Status::Error => p.danger,
    }
}

pub fn status_icon(s: Status) -> &'static str {
    match s {
        Status::Downloading => icon::ARROW_DOWN,
        Status::Seeding => icon::ARROW_UP,
        Status::Completed => icon::CHECK_CIRCLE,
        Status::Paused => icon::PAUSE,
        Status::Checking => icon::ARROWS_CLOCKWISE,
        Status::Metadata => icon::MAGNET,
        Status::Error => icon::WARNING_CIRCLE,
    }
}

/// Rounded progress bar with an optional label drawn on the right.
pub fn progress_cell(ui: &mut egui::Ui, p: &Palette, fraction: f32, color: Color32, text: &str) {
    let text_w = 46.0;
    let avail = ui.available_width();
    let bar_w = (avail - text_w - 6.0).max(24.0);
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        crate::shared::kit::progress_bar(ui, p, fraction, color, bar_w, 6.0);
        ui.label(RichText::new(text).small().color(p.text_dim));
    });
}

/// Filled line chart of `(down, up)` samples in bytes/s.
pub fn speed_chart(ui: &mut egui::Ui, p: &Palette, size: Vec2, samples: &[(f32, f32)], labels: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), mix(p.input, p.surface, 0.4));
    let max = samples
        .iter()
        .map(|(d, u)| d.max(*u))
        .fold(0.0f32, f32::max)
        .max(16.0 * 1024.0)
        * 1.15;
    let inner = rect.shrink2(Vec2::new(4.0, 4.0));
    if labels {
        for i in 1..4 {
            let y = inner.bottom() - inner.height() * i as f32 / 4.0;
            painter.line_segment(
                [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
                Stroke::new(1.0, alpha(p.border, 0.55)),
            );
        }
    }
    let n = samples.len();
    if n >= 2 {
        let x_at = |i: usize| inner.left() + inner.width() * i as f32 / (n - 1) as f32;
        let y_at = |v: f32| inner.bottom() - inner.height() * (v / max).clamp(0.0, 1.0);
        for (series, color) in [(0usize, p.accent), (1usize, p.accent2)] {
            let pts: Vec<Pos2> = samples
                .iter()
                .enumerate()
                .map(|(i, s)| Pos2::new(x_at(i), y_at(if series == 0 { s.0 } else { s.1 })))
                .collect();
            // area under the curve
            let mut mesh = egui::Mesh::default();
            let top = alpha(color, if series == 0 { 0.28 } else { 0.18 });
            let bottom = alpha(color, 0.02);
            for pt in &pts {
                mesh.colored_vertex(*pt, top);
                mesh.colored_vertex(Pos2::new(pt.x, inner.bottom()), bottom);
            }
            for i in 0..(pts.len() as u32 - 1) {
                let a = i * 2;
                mesh.add_triangle(a, a + 1, a + 2);
                mesh.add_triangle(a + 1, a + 3, a + 2);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::line(pts, Stroke::new(1.6, color)));
        }
    }
    if labels {
        painter.text(
            inner.left_top() + Vec2::new(4.0, 2.0),
            Align2::LEFT_TOP,
            crate::shared::kit::human_speed(max as f64 / 1.15),
            FontId::proportional(11.0),
            p.text_dim,
        );
    }
    response
}

/// Paints the app icon (from `assets/icon-128.rgba`).
pub fn logo(ui: &mut egui::Ui, texture: &egui::TextureHandle, size: f32) -> egui::Response {
    ui.add(egui::Image::new((texture.id(), Vec2::splat(size))).corner_radius(CornerRadius::same((size * 0.22) as u8)))
}

/// Left-aligned tab button; the selected tab gets a soft accent fill.
pub fn tab_button<'a>(p: &Palette, text: RichText, selected: bool) -> egui::Button<'a> {
    let fill = if selected {
        mix(p.surface, p.accent, 0.16)
    } else {
        Color32::TRANSPARENT
    };
    egui::Button::new(text)
        .right_text("") // a growing atom after the label keeps it left-aligned
        .fill(fill)
        .stroke(Stroke::NONE)
        .frame_when_inactive(selected)
}

/// Small filled circle followed by a label (legend / status indicator).
pub fn dot_label(ui: &mut egui::Ui, color: Color32, text: RichText) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, color);
        ui.label(text)
    })
    .inner
}

/// Key/value row for the details grid.
pub fn kv(ui: &mut egui::Ui, p: &Palette, key: &str, value: impl Into<egui::WidgetText>) {
    ui.label(RichText::new(key).color(p.text_dim));
    ui.add(egui::Label::new(value).truncate());
    ui.end_row();
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
    /// Optional `(label, url)` link shown under the text (e.g. "What's new").
    pub link: Option<(String, String)>,
}

impl Toast {
    fn lifetime(&self) -> Duration {
        if self.link.is_some() {
            // Leave time to click the link.
            return Duration::from_secs(12);
        }
        match self.kind {
            ToastKind::Error => Duration::from_secs(9),
            _ => Duration::from_secs(5),
        }
    }
}

/// Draws the toast stack in the bottom-right corner and expires old ones.
pub fn show_toasts(ctx: &egui::Context, p: &Palette, toasts: &mut Vec<Toast>, bottom_offset: f32) {
    toasts.retain(|t| t.born.elapsed() < t.lifetime());
    if toasts.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    let mut y = screen.bottom() - bottom_offset - 12.0;
    let mut dismiss = None;
    for (i, t) in toasts.iter().enumerate().rev().take(4) {
        let left = t.lifetime().saturating_sub(t.born.elapsed()).as_secs_f32();
        let fade = left.min(0.4) / 0.4;
        let (ic, color) = match t.kind {
            ToastKind::Info => (icon::INFO, p.info),
            ToastKind::Success => (icon::CHECK_CIRCLE, p.success),
            ToastKind::Error => (icon::WARNING_CIRCLE, p.danger),
        };
        let area = egui::Area::new(egui::Id::new(("toast", t.born)))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(Pos2::new(screen.right() - 16.0, y))
            .interactable(true);
        let resp = area.show(ctx, |ui| {
            ui.set_opacity(fade);
            egui::Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0, mix(p.border, color, 0.35)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .shadow(ctx.global_style().visuals.popup_shadow)
                .show(ui, |ui| {
                    ui.set_max_width(360.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ic).size(18.0).color(color));
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(RichText::new(&t.text).color(p.text)).wrap());
                            if let Some((label, url)) = &t.link {
                                let link = ui.add(
                                    egui::Button::new(
                                        RichText::new(format!("{label}  {}", icon::ARROW_SQUARE_OUT)).color(p.accent),
                                    )
                                    .frame(false),
                                );
                                if link.clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(url));
                                }
                            }
                        });
                    });
                })
        });
        if resp.response.clicked() {
            dismiss = Some(i);
        }
        y -= resp.response.rect.height() + 8.0;
    }
    if let Some(i) = dismiss {
        toasts.remove(i);
    }
    ctx.request_repaint_after(Duration::from_millis(100));
}

/// Dim overlay with a hint while files are dragged over the window.
pub fn drop_overlay(ctx: &egui::Context, p: &Palette) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
    painter.rect_filled(screen, CornerRadius::ZERO, alpha(p.bg, 0.82));
    let card = Rect::from_center_size(screen.center(), Vec2::new(360.0, 150.0));
    painter.rect(
        card,
        CornerRadius::same(16),
        p.surface,
        Stroke::new(2.0, p.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        card.center() - Vec2::new(0.0, 22.0),
        Align2::CENTER_CENTER,
        icon::DOWNLOAD_SIMPLE,
        FontId::proportional(38.0),
        p.accent,
    );
    painter.text(
        card.center() + Vec2::new(0.0, 28.0),
        Align2::CENTER_CENTER,
        "Drop .torrent files to add them",
        FontId::proportional(16.0),
        p.text,
    );
}

/// "3 minutes ago"-style relative time.
pub fn ago(unix: u64, now: u64) -> String {
    let d = now.saturating_sub(unix);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => format!("{} days ago", d / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn relative_times() {
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 180), "3 min ago");
        assert_eq!(ago(0, 7200), "2 h ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
    }
}
