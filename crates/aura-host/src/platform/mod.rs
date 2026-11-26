//! Shared session controller & portable input backend trait.

use crate::client::{ConnectionHub, ConnectionState};
use crate::keymap::remap_mac_shortcuts;
use crate::logbuf::push_log;
use crate::switch::{
    compute_spawn_warp, compute_spatial_spawn_warp, project_cursor_to_panel_uv,
    spatial_panel_distance_m, split_warp_steps, ActiveEdge, DisplayBounds, DisplayInfo, EdgeEntry,
    EdgeMoveOutcome, EdgeSwitchConfig, EdgeSwitchTracker,
};
use aura_protocol::hid::{CONSUMER_AC_BACK, CONSUMER_AC_HOME, KEY_B, KEY_ESC, KEY_H, KEY_X};
use aura_protocol::{
    HidState, Message, SpatialPose, CAP_SPATIAL_WARP, WARP_EDGE_BOTTOM, WARP_EDGE_LEFT,
    WARP_EDGE_RIGHT, WARP_EDGE_TOP,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod mock;

static SIGNAL_EXIT: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_unix_signal(_sig: i32) {
    SIGNAL_EXIT.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
pub fn install_signal_handlers(_controller: Arc<SessionController>) {
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    unsafe {
        signal(2, handle_unix_signal); // SIGINT
        signal(15, handle_unix_signal); // SIGTERM
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_win_ctrl(_ctrl_type: u32) -> i32 {
    SIGNAL_EXIT.store(true, Ordering::SeqCst);
    1
}

#[cfg(windows)]
pub fn install_signal_handlers(_controller: Arc<SessionController>) {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: unsafe extern "system" fn(u32) -> i32,
            add: i32,
        ) -> i32;
    }
    unsafe {
        SetConsoleCtrlHandler(handle_win_ctrl, 1);
    }
}

#[cfg(not(any(unix, windows)))]
pub fn install_signal_handlers(_controller: Arc<SessionController>) {}

#[derive(Debug, Clone)]
pub struct PermissionStatus {
    pub ready: bool,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub summary: String,
}

pub fn check_permissions() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        macos::check_permissions()
    }
    #[cfg(target_os = "linux")]
    {
        linux::check_permissions()
    }
    #[cfg(target_os = "windows")]
    {
        windows::check_permissions()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        PermissionStatus {
            ready: true,
            accessibility: true,
            input_monitoring: true,
            summary: "OK".into(),
        }
    }
}

pub fn request_permissions() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        macos::request_permissions()
    }
    #[cfg(not(target_os = "macos"))]
    {
        check_permissions()
    }
}

pub fn open_permission_settings(pane: &str) {
    #[cfg(target_os = "macos")]
    {
        macos::open_permission_settings(pane);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
    }
}

pub fn list_displays() -> Vec<DisplayInfo> {
    #[cfg(target_os = "macos")]
    {
        macos::list_displays()
    }
    #[cfg(target_os = "linux")]
    {
        linux::list_displays()
    }
    #[cfg(target_os = "windows")]
    {
        windows::list_displays()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        vec![DisplayInfo {
            id: "1".into(),
            label: "Primary Monitor".into(),
            is_primary: true,
            bounds: DisplayBounds::default(),
        }]
    }
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub sensitivity: f64,
    pub scroll_sensitivity: f64,
    pub natural_scroll: bool,
    pub mac_shortcuts: bool,
    pub verbose: bool,
    pub edge_switch: EdgeSwitchConfig,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            scroll_sensitivity: 1.0,
            natural_scroll: cfg!(target_os = "macos"),
            mac_shortcuts: cfg!(target_os = "macos"),
            verbose: false,
            edge_switch: EdgeSwitchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub gui: bool,
}

pub trait InputBackend {
    fn run(&mut self, controller: Arc<SessionController>) -> Result<(), String>;
}

type ModeObserver = Arc<dyn Fn(bool) + Send + Sync>;

pub struct SessionController {
    xr_active: Arc<AtomicBool>,
    should_exit: AtomicBool,
    state: Mutex<HidState>,
    scroll_accum: Mutex<(f64, f64)>,
    edge_tracker: Mutex<EdgeSwitchTracker>,
    last_spatial_query: Mutex<Option<Instant>>,
    pending_host_cursor_uv: Mutex<Option<(f64, f64)>>,
    hub: Arc<ConnectionHub>,
    config: RwLock<CaptureConfig>,
    on_mode_change: RwLock<Option<ModeObserver>>,
}

impl SessionController {
    pub fn new(
        hub: Arc<ConnectionHub>,
        config: CaptureConfig,
        on_mode_change: Option<ModeObserver>,
    ) -> Self {
        Self {
            xr_active: Arc::new(AtomicBool::new(false)),
            should_exit: AtomicBool::new(false),
            state: Mutex::new(HidState::new()),
            scroll_accum: Mutex::new((0.0, 0.0)),
            edge_tracker: Mutex::new(EdgeSwitchTracker::new()),
            last_spatial_query: Mutex::new(None),
            pending_host_cursor_uv: Mutex::new(None),
            hub,
            config: RwLock::new(config),
            on_mode_change: RwLock::new(on_mode_change),
        }
    }

    pub fn set_mode_observer(&self, cb: ModeObserver) {
        if let Ok(mut guard) = self.on_mode_change.write() {
            *guard = Some(cb);
        }
    }

    pub fn config(&self) -> CaptureConfig {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn update_config(&self, cfg: CaptureConfig) {
        let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
        *guard = cfg;
    }

    pub fn is_xr_active(&self) -> bool {
        self.xr_active.load(Ordering::Acquire)
    }

    pub fn is_verbose(&self) -> bool {
        self.config().verbose
    }

    pub fn is_connected(&self) -> bool {
        self.hub.is_connected()
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.hub.state()
    }

    pub fn latest_spatial_pose(&self) -> Option<(SpatialPose, Duration)> {
        self.hub.latest_spatial_pose()
    }

    pub fn take_pending_host_cursor_uv(&self) -> Option<(f64, f64)> {
        self.pending_host_cursor_uv
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit.load(Ordering::Acquire) || SIGNAL_EXIT.load(Ordering::Acquire)
    }

    pub fn request_exit(&self) {
        self.force_host_mode();
        self.should_exit.store(true, Ordering::Release);
    }

    pub fn toggle_mode(&self) -> bool {
        let next = !self.xr_active.load(Ordering::Acquire);
        self.set_xr_active(next);
        self.is_xr_active()
    }

    pub fn force_host_mode(&self) {
        self.set_xr_active(false);
    }

    pub fn set_xr_active(&self, active: bool) {
        self.set_xr_active_with_edge(active, None);
    }

    pub fn set_xr_active_with_edge(&self, active: bool, entry: Option<EdgeEntry>) {
        if active && !self.hub.is_connected() {
            push_log(
                "[aura-kvm] cannot enter XR mode: puck not connected (waiting to auto-connect...)",
            );
            return;
        }
        let prev = self.xr_active.swap(active, Ordering::SeqCst);
        if prev != active {
            if let Ok(mut st) = self.state.lock() {
                st.release_all();
            }
            if let Ok(mut acc) = self.scroll_accum.lock() {
                *acc = (0.0, 0.0);
            }
            if let Ok(mut tracker) = self.edge_tracker.lock() {
                let now = Instant::now();
                if active {
                    tracker.mark_entered_xr(now, entry.map(|e| e.edge));
                } else {
                    tracker.mark_returned_host(now);
                }
            }
            self.hub.send(Message::ReleaseAll);
            if let Ok(guard) = self.on_mode_change.read() {
                if let Some(ref cb) = *guard {
                    cb(active);
                }
            }
            if active {
                push_log("[aura-kvm] -> XREAL AURA (XR mode active)");
                if let Some(edge_entry) = entry {
                    let edge_cfg = self.config().edge_switch;
                    if edge_cfg.edge_spawn_warp {
                        if edge_cfg.use_spatial_warp
                            && (self.hub.capabilities() & CAP_SPATIAL_WARP) != 0
                        {
                            let edge_u8 = match edge_entry.edge {
                                ActiveEdge::Left => WARP_EDGE_LEFT,
                                ActiveEdge::Right => WARP_EDGE_RIGHT,
                                ActiveEdge::Top => WARP_EDGE_TOP,
                                ActiveEdge::Bottom => WARP_EDGE_BOTTOM,
                            };
                            push_log(format!(
                                "[aura-kvm] closed-loop 3D edge warp requested ({:?}, gain={:.2})",
                                edge_entry.edge, edge_cfg.spatial_warp_gain
                            ));
                            self.hub.send(Message::WarpToEdge {
                                edge: edge_u8,
                                norm_x: edge_entry.norm_x as f32,
                                norm_y: edge_entry.norm_y as f32,
                                gain: edge_cfg.spatial_warp_gain as f32,
                            });
                        } else {
                            let spatial_opt = if edge_cfg.use_spatial_warp {
                                self.hub.latest_spatial_pose().and_then(|(pose, age)| {
                                    if age <= Duration::from_secs(5) {
                                        compute_spatial_spawn_warp(
                                            &pose,
                                            edge_entry,
                                            edge_cfg.spatial_warp_gain,
                                        )
                                        .map(|dxy| (dxy, pose))
                                    } else {
                                        None
                                    }
                                })
                            } else {
                                None
                            };
                            let (warp_dx, warp_dy) = match spatial_opt {
                                Some(((dx, dy), pose)) => {
                                    let dist = spatial_panel_distance_m(&pose).unwrap_or(0.0);
                                    push_log(format!(
                                        "[aura-kvm] 3D spatial warp: ({dx}, {dy}) [panel {:.2}x{:.2}m @ {:.2}m]",
                                        pose.panel_size_m[0], pose.panel_size_m[1], dist
                                    ));
                                    (dx, dy)
                                }
                                None => compute_spawn_warp(edge_entry, edge_cfg.edge_spawn_span_px),
                            };
                            let steps = split_warp_steps(warp_dx, warp_dy, 14);
                            let hub_c = Arc::clone(&self.hub);
                            let xr_flag = Arc::clone(&self.xr_active);
                            thread::spawn(move || {
                                if !xr_flag.load(Ordering::Acquire) || !hub_c.is_connected() {
                                    return;
                                }
                                // Tiny wake pulse so Android XR reveals the cursor at center before warping.
                                hub_c.send(Message::MouseReport {
                                    buttons: 0,
                                    dx: 1,
                                    dy: 0,
                                    wheel_v: 0,
                                    wheel_h: 0,
                                });
                                hub_c.send(Message::MouseReport {
                                    buttons: 0,
                                    dx: -1,
                                    dy: 0,
                                    wheel_v: 0,
                                    wheel_h: 0,
                                });
                                thread::sleep(Duration::from_millis(12));

                                for (sdx, sdy) in steps {
                                    if !xr_flag.load(Ordering::Acquire) || !hub_c.is_connected() {
                                        break;
                                    }
                                    if sdx != 0 || sdy != 0 {
                                        hub_c.send(Message::MouseReport {
                                            buttons: 0,
                                            dx: sdx,
                                            dy: sdy,
                                            wheel_v: 0,
                                            wheel_h: 0,
                                        });
                                    }
                                    thread::sleep(Duration::from_millis(3));
                                }
                            });
                        }
                    }
                }
            } else {
                push_log("[aura-kvm] -> HOST COMPUTER");
            }
        }
    }

    pub fn handle_host_cursor_sample(
        &self,
        pos_x: f64,
        pos_y: f64,
        dx: f64,
        dy: f64,
        bounds: DisplayBounds,
    ) -> bool {
        if self.is_xr_active() || !self.hub.is_connected() {
            return false;
        }
        let edge_cfg = self.config().edge_switch;
        let now = Instant::now();
        let outcome = {
            let mut tracker = self.edge_tracker.lock().unwrap_or_else(|e| e.into_inner());
            tracker.on_host_mouse_move_with_dwell(now, pos_x, pos_y, dx, dy, bounds, &edge_cfg)
        };
        match outcome {
            EdgeMoveOutcome::DwellStarted(_) => {
                if edge_cfg.use_spatial_warp && edge_cfg.edge_spawn_warp {
                    let should_query = {
                        let mut last = self
                            .last_spatial_query
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if last
                            .map(|t| now.duration_since(t) >= Duration::from_millis(250))
                            .unwrap_or(true)
                        {
                            *last = Some(now);
                            true
                        } else {
                            false
                        }
                    };
                    if should_query {
                        self.hub.send(Message::QuerySpatialPose);
                    }
                }
                false
            }
            EdgeMoveOutcome::Triggered(entry) => {
                push_log(format!("[aura-kvm] edge switch ({:?}) -> XREAL AURA", entry.edge));
                self.set_xr_active_with_edge(true, Some(entry));
                self.is_xr_active()
            }
            EdgeMoveOutcome::None => false,
        }
    }

    pub fn handle_remote_focus_return(&self) {
        if !self.is_xr_active() {
            return;
        }
        let edge_cfg = self.config().edge_switch;
        let accept = {
            let tracker = self.edge_tracker.lock().unwrap_or_else(|e| e.into_inner());
            tracker.should_accept_mirror_focus_return(Instant::now(), &edge_cfg)
        };
        if accept {
            if let Some((pose, age)) = self.hub.latest_spatial_pose() {
                if age <= Duration::from_secs(3) {
                    if let Some((u, v)) = project_cursor_to_panel_uv(&pose) {
                        if let Ok(mut pending) = self.pending_host_cursor_uv.lock() {
                            *pending = Some((u, v));
                        }
                    }
                }
            }
            push_log("[aura-kvm] Wired Mirroring focused -> HOST COMPUTER");
            self.force_host_mode();
        }
    }

    pub fn send_consumer_tap(&self, usage: u16) {
        if !self.hub.is_connected() {
            return;
        }
        self.hub.send(Message::ConsumerReport(usage));
        self.hub.send(Message::ConsumerReport(0));
    }

    pub fn send_release_all(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.release_all();
        }
        self.hub.send(Message::ReleaseAll);
    }

    /// Returns `true` if the event was consumed (XR mode active or toggle/failsafe hotkey).
    pub fn handle_key(&self, hid_usage: u8, pressed: bool, mods: Modifiers) -> bool {
        if pressed && Self::is_failsafe_hotkey(hid_usage, mods) {
            push_log("[aura-kvm] emergency failsafe triggered -> returning to host");
            self.force_host_mode();
            return true;
        }

        if pressed && Self::is_toggle_hotkey(hid_usage, mods) {
            self.toggle_mode();
            return true;
        }

        if !self.is_xr_active() {
            return false;
        }

        // XR convenience shortcuts while active: Option+Cmd+H (mac) / Ctrl+Alt+H (win/linux) -> Home
        // and ...+B -> Back.
        if pressed && Self::is_xr_nav_chord(mods) {
            if hid_usage == KEY_H {
                self.hub.send(Message::ConsumerReport(CONSUMER_AC_HOME));
                self.hub.send(Message::ConsumerReport(0));
                return true;
            }
            if hid_usage == KEY_B {
                self.hub.send(Message::ConsumerReport(CONSUMER_AC_BACK));
                self.hub.send(Message::ConsumerReport(0));
                return true;
            }
        }

        let cfg = self.config();
        let report_opt = {
            let mut st = self.state.lock().unwrap();
            st.set_key(hid_usage, pressed)
        };
        if let Some(mut report) = report_opt {
            if cfg.mac_shortcuts {
                report = remap_mac_shortcuts(report);
            }
            if cfg.verbose {
                push_log(format!("[host] key report={report:02x?}"));
            }
            self.hub.send(Message::KeyReport(report));
        }
        true
    }

    pub fn handle_mouse_move(&self, dx: f64, dy: f64) -> bool {
        if !self.is_xr_active() {
            return false;
        }
        let cfg = self.config();
        let sdx = (dx * cfg.sensitivity).round().clamp(-32767.0, 32767.0) as i16;
        let sdy = (dy * cfg.sensitivity).round().clamp(-32767.0, 32767.0) as i16;
        if sdx != 0 || sdy != 0 {
            let buttons = self.state.lock().unwrap().mouse_buttons();
            self.hub.send(Message::MouseReport {
                buttons,
                dx: sdx,
                dy: sdy,
                wheel_v: 0,
                wheel_h: 0,
            });
        }
        if cfg.edge_switch.auto_return_on_edge {
            let should_return = {
                let mut tracker = self.edge_tracker.lock().unwrap_or_else(|e| e.into_inner());
                tracker.on_xr_mouse_move(Instant::now(), dx, dy, &cfg.edge_switch)
            };
            if should_return {
                push_log("[aura-kvm] opposite-edge pull-back -> HOST COMPUTER");
                self.force_host_mode();
            }
        }
        true
    }

    pub fn handle_mouse_button(&self, button_index: u8, pressed: bool) -> bool {
        if !self.is_xr_active() {
            return false;
        }
        let changed = {
            let mut st = self.state.lock().unwrap();
            st.set_mouse_button(button_index, pressed)
        };
        if changed {
            let buttons = self.state.lock().unwrap().mouse_buttons();
            self.hub.send(Message::MouseReport {
                buttons,
                dx: 0,
                dy: 0,
                wheel_v: 0,
                wheel_h: 0,
            });
        }
        true
    }

    /// `delta_v` / `delta_h` are continuous pixel deltas (trackpad) or wheel notches (`discrete = true`).
    pub fn handle_scroll(&self, delta_v: f64, delta_h: f64, discrete: bool) -> bool {
        if !self.is_xr_active() {
            return false;
        }
        let cfg = self.config();
        let sign = if cfg.natural_scroll { -1.0 } else { 1.0 };

        let (wv, wh) = if discrete {
            let v = (delta_v * cfg.scroll_sensitivity * sign)
                .round()
                .clamp(-127.0, 127.0) as i8;
            let h = (delta_h * cfg.scroll_sensitivity * sign)
                .round()
                .clamp(-127.0, 127.0) as i8;
            (v, h)
        } else {
            let mut acc = self.scroll_accum.lock().unwrap();
            acc.0 += delta_v * 0.12 * cfg.scroll_sensitivity * sign;
            acc.1 += delta_h * 0.12 * cfg.scroll_sensitivity * sign;
            let v = acc.0.trunc().clamp(-127.0, 127.0) as i8;
            let h = acc.1.trunc().clamp(-127.0, 127.0) as i8;
            acc.0 -= f64::from(v);
            acc.1 -= f64::from(h);
            (v, h)
        };

        if wv != 0 || wh != 0 {
            let buttons = self.state.lock().unwrap().mouse_buttons();
            self.hub.send(Message::MouseReport {
                buttons,
                dx: 0,
                dy: 0,
                wheel_v: wv,
                wheel_h: wh,
            });
        }
        true
    }

    fn is_toggle_hotkey(hid_usage: u8, mods: Modifiers) -> bool {
        if hid_usage != KEY_X {
            return false;
        }
        if cfg!(target_os = "macos") {
            mods.alt && mods.gui && !mods.ctrl && !mods.shift
        } else {
            mods.ctrl && mods.alt && !mods.shift && !mods.gui
        }
    }

    fn is_failsafe_hotkey(hid_usage: u8, mods: Modifiers) -> bool {
        if hid_usage != KEY_ESC {
            return false;
        }
        if cfg!(target_os = "macos") {
            mods.ctrl && mods.alt && mods.gui
        } else {
            mods.ctrl && mods.alt && mods.shift
        }
    }

    fn is_xr_nav_chord(mods: Modifiers) -> bool {
        if cfg!(target_os = "macos") {
            mods.alt && mods.gui && !mods.ctrl
        } else {
            mods.ctrl && mods.alt && !mods.gui
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_config_updates_immediately() {
        let hub = Arc::new(ConnectionHub::new());
        let ctrl = SessionController::new(hub, CaptureConfig::default(), None);
        assert_eq!(ctrl.config().sensitivity, 1.0);

        ctrl.update_config(CaptureConfig {
            sensitivity: 2.25,
            scroll_sensitivity: 0.5,
            natural_scroll: false,
            mac_shortcuts: false,
            verbose: true,
            edge_switch: EdgeSwitchConfig::default(),
        });
        let updated = ctrl.config();
        assert_eq!(updated.sensitivity, 2.25);
        assert_eq!(updated.scroll_sensitivity, 0.5);
        assert!(!updated.natural_scroll);
        assert!(!updated.mac_shortcuts);
        assert!(updated.verbose);
    }
}
