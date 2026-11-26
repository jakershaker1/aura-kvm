//! Floating click-through Status HUD viewport across macOS, Linux, and Windows.

use crate::theme::{ACCENT_AMBER, ACCENT_CYAN, ACCENT_EMERALD, ACCENT_ROSE, TEXT_MUTED, TEXT_PRIMARY};
use aura_host::client::ConnectionState;
use aura_host::config_store::AppSettings;
use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Margin, Stroke, ViewportBuilder, ViewportId,
    WindowLevel,
};
use std::time::{Duration, Instant};

const HUD_WIDTH: f32 = 340.0;
const HUD_HEIGHT: f32 = 54.0;
const TRANSIENT_DURATION: Duration = Duration::from_millis(1800);
const FADE_DURATION_SECS: f32 = 0.4;

pub struct HudState {
    last_trigger: Option<Instant>,
    last_title: String,
    last_subtitle: String,
    last_accent: Color32,
    preview_until: Option<Instant>,
}

impl Default for HudState {
    fn default() -> Self {
        Self {
            last_trigger: None,
            last_title: String::new(),
            last_subtitle: String::new(),
            last_accent: ACCENT_EMERALD,
            preview_until: None,
        }
    }
}

impl HudState {
    pub fn trigger(&mut self, title: impl Into<String>, subtitle: impl Into<String>, accent: Color32) {
        self.last_title = title.into();
        self.last_subtitle = subtitle.into();
        self.last_accent = accent;
        self.last_trigger = Some(Instant::now());
    }

    pub fn preview(&mut self, xr_active: bool) {
        let now = Instant::now();
        self.preview_until = Some(now + TRANSIENT_DURATION);
        if xr_active {
            self.trigger("XREAL AURA", toggle_hint_return(), ACCENT_CYAN);
        } else {
            self.trigger("HOST COMPUTER", toggle_hint_enter(), ACCENT_EMERALD);
        }
    }

    pub fn is_animating(&self, settings: &AppSettings, xr_active: bool) -> bool {
        if !settings.hud_enabled {
            return false;
        }
        if xr_active && settings.hud_pin_in_xr {
            return false;
        }
        if let Some(t) = self.last_trigger {
            return t.elapsed() < TRANSIENT_DURATION;
        }
        if let Some(until) = self.preview_until {
            return Instant::now() < until;
        }
        false
    }
}

pub fn toggle_shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Option + Cmd + X"
    } else {
        "Ctrl + Alt + X"
    }
}

pub fn failsafe_shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Ctrl + Option + Cmd + Esc"
    } else {
        "Ctrl + Alt + Shift + Esc"
    }
}

fn toggle_hint_return() -> String {
    format!("{} to return", toggle_shortcut_label())
}

fn toggle_hint_enter() -> String {
    format!("{} for XREAL Aura", toggle_shortcut_label())
}

pub fn on_mode_transition(hud: &mut HudState, xr_active: bool) {
    if xr_active {
        hud.trigger("XREAL AURA", toggle_hint_return(), ACCENT_CYAN);
    } else {
        hud.trigger("HOST COMPUTER", "Control returned to local screen", ACCENT_EMERALD);
    }
}

pub fn on_connection_transition(hud: &mut HudState, state: &ConnectionState) {
    match state {
        ConnectionState::Connected { target, transport } => {
            hud.trigger(
                "PUCK CONNECTED",
                format!("{target} ({transport})"),
                ACCENT_EMERALD,
            );
        }
        ConnectionState::Searching => {
            hud.trigger(
                "PUCK DISCONNECTED",
                "Searching for XREAL Aura...",
                ACCENT_AMBER,
            );
        }
        ConnectionState::Connecting { target } => {
            hud.trigger("CONNECTING", format!("Setting up {target}..."), ACCENT_AMBER);
        }
        ConnectionState::Error { message } => {
            hud.trigger("CONNECTION ERROR", message.clone(), ACCENT_ROSE);
        }
    }
}

pub fn render_hud_viewport(
    ctx: &egui::Context,
    settings: &AppSettings,
    xr_active: bool,
    hud: &mut HudState,
) {
    if !settings.hud_enabled {
        return;
    }

    let pinned = xr_active && settings.hud_pin_in_xr;
    let alpha_factor = if pinned {
        settings.hud_opacity
    } else if let Some(t) = hud.last_trigger {
        let elapsed = t.elapsed();
        if elapsed >= TRANSIENT_DURATION {
            return;
        }
        let remaining = (TRANSIENT_DURATION - elapsed).as_secs_f32();
        let fade = (remaining / FADE_DURATION_SECS).clamp(0.0, 1.0);
        settings.hud_opacity * fade
    } else {
        return;
    };

    let (title, subtitle, accent) = if pinned && hud.last_trigger.map(|t| t.elapsed() >= TRANSIENT_DURATION).unwrap_or(true) {
        (
            "XREAL AURA".to_string(),
            toggle_hint_return(),
            ACCENT_CYAN,
        )
    } else {
        (
            hud.last_title.clone(),
            hud.last_subtitle.clone(),
            hud.last_accent,
        )
    };

    let monitor = ctx.input(|i| i.viewport().monitor_size);
    let pos_x = monitor
        .map(|m| ((m.x - HUD_WIDTH) * 0.5).max(16.0))
        .unwrap_or(500.0);
    let pos = egui::pos2(pos_x, 42.0);

    let builder = ViewportBuilder::default()
        .with_title("Aura Status HUD")
        .with_inner_size([HUD_WIDTH, HUD_HEIGHT])
        .with_position(pos)
        .with_decorations(false)
        .with_transparent(true)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_mouse_passthrough(true)
        .with_active(false)
        .with_taskbar(false)
        .with_resizable(false);

    ctx.show_viewport_deferred(
        ViewportId::from_hash_of("aura_status_hud"),
        builder,
        move |hud_ctx, _class| {
            let bg_alpha = (232.0 * alpha_factor).round().clamp(0.0, 255.0) as u8;
            let fg_alpha = (255.0 * alpha_factor).round().clamp(0.0, 255.0) as u8;

            let bg = Color32::from_rgba_unmultiplied(15, 19, 28, bg_alpha);
            let border = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), fg_alpha);
            let title_col = Color32::from_rgba_unmultiplied(
                TEXT_PRIMARY.r(),
                TEXT_PRIMARY.g(),
                TEXT_PRIMARY.b(),
                fg_alpha,
            );
            let sub_col = Color32::from_rgba_unmultiplied(
                TEXT_MUTED.r(),
                TEXT_MUTED.g(),
                TEXT_MUTED.b(),
                fg_alpha,
            );

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                .show(hud_ctx, |ui| {
                    egui::Frame::new()
                        .fill(bg)
                        .stroke(Stroke::new(1.5_f32, border))
                        .corner_radius(CornerRadius::same(24))
                        .inner_margin(Margin::symmetric(16, 10))
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(HUD_WIDTH - 36.0, HUD_HEIGHT - 24.0));
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(10.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(dot_rect.center(), 5.0, border);
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(&title)
                                        .strong()
                                        .size(13.5)
                                        .color(title_col),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("•")
                                        .size(13.0)
                                        .color(sub_col),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&subtitle)
                                        .size(12.0)
                                        .color(sub_col),
                                );
                            });
                        });
                });
        },
    );
}
