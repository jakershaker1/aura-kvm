//! Main control & settings window for Aura KVM.

use crate::hud::{
    self, failsafe_shortcut_label, toggle_shortcut_label, HudState,
};
use crate::theme::{
    self, ACCENT_AMBER, ACCENT_EMERALD, ACCENT_ROSE, BG_INPUT, TEXT_MUTED, TEXT_PRIMARY,
};
use aura_host::client::{ConnectionHub, ConnectionState};
use aura_host::config_store::AppSettings;
use aura_host::connector::{ConnectorConfig, ConnectorHandle};
use aura_host::logbuf;
use aura_host::platform::{self, CaptureConfig, PermissionStatus, SessionController};
use aura_host::switch::{
    spatial_panel_distance_m, spatial_panel_yaw_deg, DisplayInfo, SwitchEdge,
};
use aura_protocol::hid::{
    CONSUMER_AC_BACK, CONSUMER_AC_HOME, CONSUMER_MUTE, CONSUMER_PLAY_PAUSE, CONSUMER_VOL_DOWN,
    CONSUMER_VOL_UP,
};
use eframe::egui::{self, Color32, CornerRadius, RichText};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub struct AuraUiApp {
    controller: Arc<SessionController>,
    hub: Arc<ConnectionHub>,
    connector: Option<ConnectorHandle>,
    backend_handle: Option<thread::JoinHandle<()>>,
    adb_path: Option<PathBuf>,
    agent_path: Option<PathBuf>,
    settings: AppSettings,
    hud: HudState,
    permissions: PermissionStatus,
    last_perm_check: Instant,
    displays: Vec<DisplayInfo>,
    last_display_refresh: Instant,
    show_logs: bool,
    settings_dirty: bool,
    last_save: Instant,
    last_xr_state: bool,
    last_conn_state: ConnectionState,
    last_beta_edge: SwitchEdge,
    themed: bool,
}

impl AuraUiApp {
    pub fn new(
        controller: Arc<SessionController>,
        hub: Arc<ConnectionHub>,
        connector: ConnectorHandle,
        backend_handle: thread::JoinHandle<()>,
        settings: AppSettings,
        adb_path: Option<PathBuf>,
        agent_path: Option<PathBuf>,
    ) -> Self {
        let initial_conn = hub.state();
        let initial_xr = controller.is_xr_active();
        let last_beta_edge = if settings.switch_edge != SwitchEdge::Disabled {
            settings.switch_edge
        } else {
            SwitchEdge::Right
        };
        Self {
            controller,
            hub,
            connector: Some(connector),
            backend_handle: Some(backend_handle),
            adb_path,
            agent_path,
            settings,
            hud: HudState::default(),
            permissions: platform::check_permissions(),
            last_perm_check: Instant::now(),
            displays: platform::list_displays(),
            last_display_refresh: Instant::now(),
            show_logs: false,
            settings_dirty: false,
            last_save: Instant::now(),
            last_xr_state: initial_xr,
            last_conn_state: initial_conn,
            last_beta_edge,
            themed: false,
        }
    }

    pub fn build_connector_config(&self, force_push: bool) -> ConnectorConfig {
        let trimmed_wireless = self.settings.wireless.trim();
        let trimmed_serial = self.settings.serial.trim();
        let trimmed_tcp = self.settings.direct_tcp.trim();
        ConnectorConfig {
            adb_path: self.adb_path.clone(),
            serial: if trimmed_serial.is_empty() {
                None
            } else {
                Some(trimmed_serial.to_string())
            },
            wireless: if trimmed_wireless.is_empty() {
                None
            } else {
                Some(trimmed_wireless.to_string())
            },
            direct_tcp: if self.settings.use_direct_tcp && !trimmed_tcp.is_empty() {
                Some(trimmed_tcp.to_string())
            } else {
                None
            },
            agent_path: self.agent_path.clone(),
            port: self.settings.port.max(1),
            no_push: false,
            force_push,
            once: false,
            verbose: self.settings.verbose,
        }
    }

    fn apply_live_capture_settings(&mut self) {
        self.controller
            .update_config(self.settings.to_capture_config());
        self.settings_dirty = true;
    }

    fn maybe_flush_settings(&mut self) {
        if self.settings_dirty && self.last_save.elapsed() >= Duration::from_millis(500) {
            if let Err(e) = self.settings.save() {
                logbuf::push_log(format!("[ui] failed to save settings: {e}"));
            }
            self.settings_dirty = false;
            self.last_save = Instant::now();
        }
    }
}

impl eframe::App for AuraUiApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.themed {
            theme::apply_theme(ctx);
            self.themed = true;
        }

        let xr_active = self.controller.is_xr_active();
        let conn_state = self.hub.state();

        if xr_active != self.last_xr_state {
            self.last_xr_state = xr_active;
            hud::on_mode_transition(&mut self.hud, xr_active);
        }
        if conn_state != self.last_conn_state {
            self.last_conn_state = conn_state.clone();
            hud::on_connection_transition(&mut self.hud, &conn_state);
        }

        self.maybe_flush_settings();

        if !self.permissions.ready && self.last_perm_check.elapsed() >= Duration::from_secs(2) {
            self.permissions = platform::check_permissions();
            self.last_perm_check = Instant::now();
        }
        if self.settings.switch_edge != SwitchEdge::Disabled
            && self.last_display_refresh.elapsed() >= Duration::from_secs(3)
        {
            self.displays = platform::list_displays();
            self.last_display_refresh = Instant::now();
        }

        hud::render_hud_viewport(ctx, &self.settings, xr_active, &mut self.hud);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_CANVAS)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_header(ui, xr_active, &conn_state);
                    ui.add_space(10.0);

                    if !self.permissions.ready {
                        self.render_permission_banner(ui);
                        ui.add_space(10.0);
                    }

                    self.render_input_card(ui);
                    ui.add_space(10.0);

                    self.render_switching_card(ui);
                    ui.add_space(10.0);

                    self.render_hud_card(ui, xr_active);
                    ui.add_space(10.0);

                    self.render_connection_card(ui);
                    ui.add_space(10.0);

                    self.render_quick_actions_card(ui, &conn_state);
                    ui.add_space(10.0);

                    self.render_logs_card(ui);
                });
            });

        if self.hud.is_animating(&self.settings, xr_active) {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(e) = self.settings.save() {
            eprintln!("[ui] failed to save settings on exit: {e}");
        }
        self.controller.request_exit();
        if let Some(h) = self.backend_handle.take() {
            let _ = h.join();
        }
        if let Some(c) = self.connector.take() {
            c.join();
        }
    }
}

impl AuraUiApp {
    fn render_header(&mut self, ui: &mut egui::Ui, xr_active: bool, conn_state: &ConnectionState) {
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Aura KVM")
                            .strong()
                            .size(20.0)
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new("XREAL Aura Android XR Input Bridge")
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (badge_text, badge_color) = match conn_state {
                        ConnectionState::Connected { target, transport } => {
                            let rtt = self
                                .hub
                                .last_rtt_ms()
                                .map(|ms| format!(" • {:.1} ms", ms))
                                .unwrap_or_default();
                            (format!("{transport} • {target}{rtt}"), ACCENT_EMERALD)
                        }
                        ConnectionState::Connecting { target } => {
                            (format!("Connecting to {target}..."), ACCENT_AMBER)
                        }
                        ConnectionState::Searching => {
                            ("Searching for puck...".to_string(), ACCENT_AMBER)
                        }
                        ConnectionState::Error { .. } => {
                            ("Connection error".to_string(), ACCENT_ROSE)
                        }
                    };

                    egui::Frame::new()
                        .fill(BG_INPUT)
                        .stroke(egui::Stroke::new(1.0_f32, badge_color))
                        .corner_radius(CornerRadius::same(16))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(8.0, 8.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(r.center(), 4.0, badge_color);
                                ui.label(RichText::new(badge_text).size(11.5).color(TEXT_PRIMARY));
                            });
                        });
                });
            });

            if let ConnectionState::Error { message } = conn_state {
                ui.add_space(6.0);
                ui.label(RichText::new(message).size(12.0).color(ACCENT_ROSE));
            }

            ui.add_space(10.0);
            let connected = matches!(conn_state, ConnectionState::Connected { .. });

            ui.horizontal(|ui| {
                let (btn_label, btn_fill) = if xr_active {
                    ("Return to Host Computer", Color32::from_rgb(18, 108, 128))
                } else {
                    ("Switch Input to XREAL Aura", Color32::from_rgb(30, 58, 95))
                };

                let btn = egui::Button::new(RichText::new(btn_label).strong().size(13.5))
                    .fill(btn_fill)
                    .min_size(egui::vec2(220.0, 34.0));
                if ui.add_enabled(connected, btn).clicked() {
                    self.controller.toggle_mode();
                }

                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("Toggle: {}", toggle_shortcut_label()))
                            .size(11.5)
                            .color(TEXT_MUTED),
                    );
                    ui.label(
                        RichText::new(format!("Failsafe: {}", failsafe_shortcut_label()))
                            .size(11.5)
                            .color(TEXT_MUTED),
                    );
                });
            });
        });
    }

    fn render_permission_banner(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(Color32::from_rgb(48, 24, 28))
            .stroke(egui::Stroke::new(1.0_f32, ACCENT_ROSE))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Input Permissions Required")
                        .strong()
                        .color(ACCENT_ROSE),
                );
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    let ax_color = if self.permissions.accessibility {
                        ACCENT_EMERALD
                    } else {
                        ACCENT_ROSE
                    };
                    let im_color = if self.permissions.input_monitoring {
                        ACCENT_EMERALD
                    } else {
                        ACCENT_ROSE
                    };
                    ui.label(
                        RichText::new(format!(
                            "Accessibility: {}",
                            if self.permissions.accessibility {
                                "Granted"
                            } else {
                                "Missing"
                            }
                        ))
                        .size(11.5)
                        .color(ax_color),
                    );
                    ui.label(RichText::new("•").size(11.5).color(TEXT_MUTED));
                    ui.label(
                        RichText::new(format!(
                            "Input Monitoring: {}",
                            if self.permissions.input_monitoring {
                                "Granted"
                            } else {
                                "Missing"
                            }
                        ))
                        .size(11.5)
                        .color(im_color),
                    );
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&self.permissions.summary)
                        .size(12.0)
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if cfg!(target_os = "macos") {
                        if ui.button("Request Permissions").clicked() {
                            self.permissions = platform::request_permissions();
                            self.last_perm_check = Instant::now();
                        }
                        if ui.button("Accessibility Settings").clicked() {
                            platform::open_permission_settings("accessibility");
                        }
                        if ui.button("Input Monitoring Settings").clicked() {
                            platform::open_permission_settings("input_monitoring");
                        }
                    }
                    if ui.button("Re-check").clicked() {
                        self.permissions = platform::check_permissions();
                        self.last_perm_check = Instant::now();
                    }
                });
            });
    }

    fn render_input_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Input & Tuning").strong().size(14.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset Defaults").clicked() {
                        let def = CaptureConfig::default();
                        self.settings.sensitivity = def.sensitivity;
                        self.settings.scroll_sensitivity = def.scroll_sensitivity;
                        self.settings.natural_scroll = def.natural_scroll;
                        self.settings.mac_shortcuts = def.mac_shortcuts;
                        self.apply_live_capture_settings();
                    }
                });
            });
            ui.add_space(6.0);

            let mut changed = false;
            egui::Grid::new("tuning_grid")
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Mouse sensitivity");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.settings.sensitivity, 0.20..=3.00)
                                .step_by(0.05)
                                .suffix("x"),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    ui.end_row();

                    ui.label("Scroll sensitivity");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.settings.scroll_sensitivity, 0.20..=3.00)
                                .step_by(0.05)
                                .suffix("x"),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    ui.end_row();
                });

            ui.add_space(4.0);
            if ui
                .checkbox(&mut self.settings.natural_scroll, "Natural scrolling")
                .changed()
            {
                changed = true;
            }
            if ui
                .checkbox(
                    &mut self.settings.mac_shortcuts,
                    "Mac-friendly shortcuts (Cmd+C/V/X/Z/A/F/T/W/R/L -> Ctrl)",
                )
                .changed()
            {
                changed = true;
            }
            if ui
                .checkbox(&mut self.settings.verbose, "Verbose HID report logging")
                .changed()
            {
                changed = true;
            }

            if changed {
                self.apply_live_capture_settings();
            }
        });
    }

    fn render_switching_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Window Switching").strong().size(14.5));
            ui.add_space(6.0);

            let mut changed = false;
            if ui
                .checkbox(
                    &mut self.settings.auto_return_on_mirror_focus,
                    "Auto-return to host when Wired Mirroring window is focused",
                )
                .changed()
            {
                changed = true;
            }
            ui.label(
                RichText::new(format!(
                    "Press {} to switch to XREAL Aura; clicking your Wired Mirroring window in XR automatically returns control to the host.",
                    toggle_shortcut_label()
                ))
                .size(11.5)
                .color(TEXT_MUTED),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Seamless Edge Switching").strong().size(13.0));
                egui::Frame::new()
                    .fill(BG_INPUT)
                    .stroke(egui::Stroke::new(1.0_f32, ACCENT_AMBER))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new("BETA").strong().size(10.0).color(ACCENT_AMBER));
                    });
            });
            ui.add_space(4.0);

            let mut beta_enabled = self.settings.switch_edge != SwitchEdge::Disabled;
            if ui
                .checkbox(
                    &mut beta_enabled,
                    "Enable seamless screen-edge switching (Beta)",
                )
                .changed()
            {
                if beta_enabled {
                    self.settings.switch_edge = if self.last_beta_edge != SwitchEdge::Disabled {
                        self.last_beta_edge
                    } else {
                        SwitchEdge::Right
                    };
                } else {
                    if self.settings.switch_edge != SwitchEdge::Disabled {
                        self.last_beta_edge = self.settings.switch_edge;
                    }
                    self.settings.switch_edge = SwitchEdge::Disabled;
                    self.settings.auto_return_on_edge = false;
                }
                changed = true;
            }

            if self.settings.switch_edge != SwitchEdge::Disabled {
                ui.add_space(6.0);
                let beta_edges = [
                    SwitchEdge::Right,
                    SwitchEdge::Left,
                    SwitchEdge::Top,
                    SwitchEdge::Bottom,
                    SwitchEdge::LeftRight,
                    SwitchEdge::Any,
                ];
                let selected_monitor_label = if self.settings.edge_monitor_id.trim().is_empty() {
                    "Primary Monitor (auto)".to_string()
                } else if let Some(d) = self
                    .displays
                    .iter()
                    .find(|d| d.id == self.settings.edge_monitor_id)
                {
                    d.label.clone()
                } else {
                    format!("Disconnected monitor ({})", self.settings.edge_monitor_id)
                };
                egui::Grid::new("switching_grid")
                    .num_columns(2)
                    .spacing([14.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Switch monitor");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("switch_monitor_combo")
                                .selected_text(selected_monitor_label)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.edge_monitor_id,
                                            String::new(),
                                            "Primary Monitor (auto)",
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    for d in &self.displays {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.edge_monitor_id,
                                                d.id.clone(),
                                                &d.label,
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                            if ui.button("Refresh").clicked() {
                                self.displays = platform::list_displays();
                                self.last_display_refresh = Instant::now();
                            }
                        });
                        ui.end_row();

                        ui.label("Switch to XR at edge");
                        egui::ComboBox::from_id_salt("switch_edge_combo")
                            .selected_text(self.settings.switch_edge.label())
                            .show_ui(ui, |ui| {
                                for edge in beta_edges {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.switch_edge,
                                            edge,
                                            edge.label(),
                                        )
                                        .changed()
                                    {
                                        self.last_beta_edge = edge;
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Edge dwell delay");
                        let mut dwell = self.settings.edge_dwell_ms as u32;
                        if ui
                            .add(egui::Slider::new(&mut dwell, 50..=400).suffix(" ms"))
                            .changed()
                        {
                            self.settings.edge_dwell_ms = dwell as u64;
                            changed = true;
                        }
                        ui.end_row();

                        if self.settings.use_spatial_warp {
                            ui.label("Spatial warp gain");
                            if ui
                                .add_enabled(
                                    self.settings.edge_spawn_warp,
                                    egui::Slider::new(
                                        &mut self.settings.spatial_warp_gain,
                                        0.25..=2.50,
                                    )
                                    .step_by(0.05)
                                    .suffix("x"),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        } else {
                            ui.label("Manual fallback span");
                            if ui
                                .add_enabled(
                                    self.settings.edge_spawn_warp,
                                    egui::Slider::new(
                                        &mut self.settings.edge_spawn_span_px,
                                        200.0..=2000.0,
                                    )
                                    .step_by(20.0)
                                    .suffix(" px"),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                        ui.end_row();
                    });

                ui.add_space(4.0);
                if self.hub.is_connected() {
                    ui.horizontal(|ui| {
                        if let Some((pose, _age)) = self.hub.latest_spatial_pose() {
                            if pose.is_usable() {
                                let active = pose.is_panel_active();
                                let dot_color = if active { ACCENT_EMERALD } else { ACCENT_AMBER };
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(8.0, 8.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(r.center(), 4.0, dot_color);
                                let dist = spatial_panel_distance_m(&pose)
                                    .map(|d| format!(" • {:.2} m away", d))
                                    .unwrap_or_default();
                                let yaw = spatial_panel_yaw_deg(&pose)
                                    .map(|y| format!(" • yaw {:+.0}°", y))
                                    .unwrap_or_default();
                                let state_str = if active { "active" } else { "cached" };
                                ui.label(
                                    RichText::new(format!(
                                        "3D Panel: {:.2} m × {:.2} m{dist}{yaw} • {state_str}",
                                        pose.panel_size_m[0], pose.panel_size_m[1]
                                    ))
                                    .size(11.5)
                                    .color(TEXT_MUTED),
                                );
                            } else {
                                ui.label(
                                    RichText::new(
                                        "3D Panel: waiting for Wired Mirroring window (using manual span fallback)",
                                    )
                                    .size(11.5)
                                    .color(TEXT_MUTED),
                                );
                            }
                        } else {
                            ui.label(
                                RichText::new(
                                    "3D Panel: waiting for Wired Mirroring window (using manual span fallback)",
                                )
                                .size(11.5)
                                .color(TEXT_MUTED),
                            );
                        }
                    });
                    ui.add_space(2.0);
                }

                if ui
                    .checkbox(
                        &mut self.settings.edge_spawn_warp,
                        "Spawn XR cursor at exit edge position",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add_enabled(
                        self.settings.edge_spawn_warp,
                        egui::Checkbox::new(
                            &mut self.settings.use_spatial_warp,
                            "Auto-track 3D window size & head pose from headset",
                        ),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(
                        &mut self.settings.auto_return_on_edge,
                        "Auto-return when pulling cursor back past entry edge",
                    )
                    .changed()
                {
                    changed = true;
                }
            }

            if changed {
                self.apply_live_capture_settings();
            }
        });
    }

    fn render_hud_card(&mut self, ui: &mut egui::Ui, xr_active: bool) {
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Status HUD").strong().size(14.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Preview HUD").clicked() {
                        self.hud.preview(xr_active);
                    }
                });
            });
            ui.add_space(4.0);

            let mut changed = false;
            if ui
                .checkbox(&mut self.settings.hud_enabled, "Enable floating status HUD")
                .changed()
            {
                changed = true;
            }
            if ui
                .add_enabled(
                    self.settings.hud_enabled,
                    egui::Checkbox::new(
                        &mut self.settings.hud_pin_in_xr,
                        "Keep compact pill pinned while controlling XREAL Aura",
                    ),
                )
                .changed()
            {
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label("HUD opacity");
                if ui
                    .add_enabled(
                        self.settings.hud_enabled,
                        egui::Slider::new(&mut self.settings.hud_opacity, 0.40..=1.00)
                            .step_by(0.02),
                    )
                    .changed()
                {
                    changed = true;
                }
            });

            if changed {
                self.settings_dirty = true;
            }
        });
    }

    fn render_connection_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Connection & Transport").strong().size(14.5));
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                if ui
                    .radio_value(&mut self.settings.use_direct_tcp, false, "ADB Auto (USB + Wireless)")
                    .changed()
                {
                    self.settings_dirty = true;
                }
                if ui
                    .radio_value(&mut self.settings.use_direct_tcp, true, "Direct TCP")
                    .changed()
                {
                    self.settings_dirty = true;
                }
            });

            ui.add_space(6.0);
            egui::Grid::new("conn_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    if self.settings.use_direct_tcp {
                        ui.label("Agent TCP address");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.direct_tcp)
                                    .hint_text("192.168.1.42:27190")
                                    .desired_width(220.0),
                            )
                            .changed()
                        {
                            self.settings_dirty = true;
                        }
                        ui.end_row();
                    } else {
                        ui.label("Wireless ADB fallback");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.wireless)
                                    .hint_text("192.168.1.42:5555 (optional)")
                                    .desired_width(220.0),
                            )
                            .changed()
                        {
                            self.settings_dirty = true;
                        }
                        ui.end_row();

                        ui.label("Device serial override");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.serial)
                                    .hint_text("auto-detect XREAL Aura")
                                    .desired_width(220.0),
                            )
                            .changed()
                        {
                            self.settings_dirty = true;
                        }
                        ui.end_row();

                        ui.label("Forward TCP port");
                        let mut port_val = self.settings.port as u32;
                        if ui
                            .add(egui::DragValue::new(&mut port_val).range(1024..=65535))
                            .changed()
                        {
                            self.settings.port = port_val as u16;
                            self.settings_dirty = true;
                        }
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Apply & Reconnect").clicked() {
                    let _ = self.settings.save();
                    self.settings_dirty = false;
                    let cfg = self.build_connector_config(false);
                    if let Some(ref conn) = self.connector {
                        conn.update_and_reconnect(cfg);
                    }
                }
                if !self.settings.use_direct_tcp && ui.button("Force Re-Push Agent").clicked() {
                    if let Some(ref conn) = self.connector {
                        conn.trigger_reconnect(true);
                    }
                }
            });
        });
    }

    fn render_quick_actions_card(&mut self, ui: &mut egui::Ui, conn_state: &ConnectionState) {
        let connected = matches!(conn_state, ConnectionState::Connected { .. });
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Quick XR Actions").strong().size(14.5));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(connected, egui::Button::new("Home")).clicked() {
                    self.controller.send_consumer_tap(CONSUMER_AC_HOME);
                }
                if ui.add_enabled(connected, egui::Button::new("Back")).clicked() {
                    self.controller.send_consumer_tap(CONSUMER_AC_BACK);
                }
                if ui.add_enabled(connected, egui::Button::new("Vol -")).clicked() {
                    self.controller.send_consumer_tap(CONSUMER_VOL_DOWN);
                }
                if ui.add_enabled(connected, egui::Button::new("Vol +")).clicked() {
                    self.controller.send_consumer_tap(CONSUMER_VOL_UP);
                }
                if ui.add_enabled(connected, egui::Button::new("Mute")).clicked() {
                    self.controller.send_consumer_tap(CONSUMER_MUTE);
                }
                if ui
                    .add_enabled(connected, egui::Button::new("Play/Pause"))
                    .clicked()
                {
                    self.controller.send_consumer_tap(CONSUMER_PLAY_PAUSE);
                }
                if ui
                    .add_enabled(connected, egui::Button::new("Release All Keys"))
                    .clicked()
                {
                    self.controller.send_release_all();
                }
            });
        });
    }

    fn render_logs_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let toggle_label = if self.show_logs {
                    "Hide Activity Log"
                } else {
                    "Show Activity Log"
                };
                if ui.button(toggle_label).clicked() {
                    self.show_logs = !self.show_logs;
                }
                if self.show_logs && ui.button("Clear").clicked() {
                    logbuf::clear();
                }
            });

            if self.show_logs {
                ui.add_space(6.0);
                let entries = logbuf::snapshot();
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &entries {
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                            );
                        }
                    });
            }
        });
    }
}
