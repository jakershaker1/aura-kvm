//! Custom dark spatial UI theme for Aura KVM.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

pub const BG_CANVAS: Color32 = Color32::from_rgb(15, 17, 23);
pub const BG_CARD: Color32 = Color32::from_rgb(23, 27, 36);
pub const BG_INPUT: Color32 = Color32::from_rgb(31, 36, 48);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(42, 49, 66);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(238, 242, 248);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(148, 163, 184);

pub const ACCENT_CYAN: Color32 = Color32::from_rgb(34, 211, 238);
pub const ACCENT_EMERALD: Color32 = Color32::from_rgb(52, 211, 153);
pub const ACCENT_AMBER: Color32 = Color32::from_rgb(251, 191, 36);
pub const ACCENT_ROSE: Color32 = Color32::from_rgb(248, 113, 113);

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.slider_width = 180.0;

    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG_CANVAS;
    visuals.panel_fill = BG_CANVAS;
    visuals.faint_bg_color = BG_CARD;
    visuals.extreme_bg_color = BG_INPUT;

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = BG_INPUT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 51, 68);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_CYAN);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);

    visuals.widgets.active.bg_fill = Color32::from_rgb(24, 92, 110);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, ACCENT_CYAN);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(7);

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(34, 211, 238, 70);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT_CYAN);

    style.visuals = visuals;
    ctx.set_style(style);
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 12))
}
