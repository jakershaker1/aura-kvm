//! Seamless screen-edge switching, edge-exit spawn warp, and remote focus-return policy.

use aura_protocol::SpatialPose;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeEntry {
    pub edge: ActiveEdge,
    pub norm_x: f64, // -1.0 (left) .. +1.0 (right)
    pub norm_y: f64, // -1.0 (top) .. +1.0 (bottom)
    pub aspect: f64, // display height / width (clamped 0.3..=2.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchEdge {
    Disabled,
    Left,
    Right,
    Top,
    Bottom,
    LeftRight,
    Any,
}

impl Default for SwitchEdge {
    fn default() -> Self {
        Self::Disabled
    }
}

impl SwitchEdge {
    pub const ALL: [SwitchEdge; 7] = [
        SwitchEdge::Disabled,
        SwitchEdge::Left,
        SwitchEdge::Right,
        SwitchEdge::Top,
        SwitchEdge::Bottom,
        SwitchEdge::LeftRight,
        SwitchEdge::Any,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            SwitchEdge::Disabled => "off",
            SwitchEdge::Left => "left",
            SwitchEdge::Right => "right",
            SwitchEdge::Top => "top",
            SwitchEdge::Bottom => "bottom",
            SwitchEdge::LeftRight => "left-right",
            SwitchEdge::Any => "any",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SwitchEdge::Disabled => "Off (Hotkey Only)",
            SwitchEdge::Left => "Left Edge",
            SwitchEdge::Right => "Right Edge",
            SwitchEdge::Top => "Top Edge",
            SwitchEdge::Bottom => "Bottom Edge",
            SwitchEdge::LeftRight => "Left & Right Edges",
            SwitchEdge::Any => "Any Screen Edge",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" => Some(SwitchEdge::Disabled),
            "left" => Some(SwitchEdge::Left),
            "right" => Some(SwitchEdge::Right),
            "top" => Some(SwitchEdge::Top),
            "bottom" => Some(SwitchEdge::Bottom),
            "left-right" | "leftright" | "horizontal" => Some(SwitchEdge::LeftRight),
            "any" | "all" => Some(SwitchEdge::Any),
            _ => None,
        }
    }

    pub fn allows(&self, edge: ActiveEdge) -> bool {
        match self {
            SwitchEdge::Disabled => false,
            SwitchEdge::Left => edge == ActiveEdge::Left,
            SwitchEdge::Right => edge == ActiveEdge::Right,
            SwitchEdge::Top => edge == ActiveEdge::Top,
            SwitchEdge::Bottom => edge == ActiveEdge::Bottom,
            SwitchEdge::LeftRight => matches!(edge, ActiveEdge::Left | ActiveEdge::Right),
            SwitchEdge::Any => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Default for DisplayBounds {
    fn default() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1920.0,
            max_y: 1080.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub id: String,
    pub label: String,
    pub is_primary: bool,
    pub bounds: DisplayBounds,
}

/// Selects the single target monitor for edge switching.
/// - Empty `monitor_id` selects the primary monitor (falling back to the first display).
/// - Non-empty `monitor_id` matches only that exact monitor ID (returns `None` if unplugged).
pub fn select_target_display(displays: &[DisplayInfo], monitor_id: &str) -> Option<DisplayInfo> {
    let trimmed = monitor_id.trim();
    if trimmed.is_empty() {
        displays
            .iter()
            .find(|d| d.is_primary)
            .or_else(|| displays.first())
            .cloned()
    } else {
        displays.iter().find(|d| d.id == trimmed).cloned()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSwitchConfig {
    pub edge: SwitchEdge,
    pub monitor_id: String,
    pub dwell_ms: u64,
    pub corner_deadzone_px: f64,
    pub auto_return_on_mirror_focus: bool,
    pub auto_return_on_edge: bool,
    pub return_edge_threshold_px: f64,
    pub edge_spawn_warp: bool,
    pub edge_spawn_span_px: f64,
    pub use_spatial_warp: bool,
    pub spatial_warp_gain: f64,
}

impl Default for EdgeSwitchConfig {
    fn default() -> Self {
        Self {
            edge: SwitchEdge::Disabled,
            monitor_id: String::new(),
            dwell_ms: 140,
            corner_deadzone_px: 48.0,
            auto_return_on_mirror_focus: true,
            auto_return_on_edge: false,
            return_edge_threshold_px: 120.0,
            edge_spawn_warp: true,
            edge_spawn_span_px: 900.0,
            use_spatial_warp: true,
            spatial_warp_gain: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeMoveOutcome {
    None,
    DwellStarted(ActiveEdge),
    Triggered(EdgeEntry),
}

// Optical frustum half-angles measured from Aura `dumpsys cpm` (1920x1200 viewport).
// Android XR applies `Pointer Acceleration: 3.000` (`dumpsys input`) to relative mouse
// deltas, so base half-FOV raw HID counts are 960/3 = 320 and 600/3 = 200.
const AURA_HALF_FOV_X_RAD: f64 = 0.4762; // atan(0.5159)
const AURA_HALF_FOV_Y_RAD: f64 = 0.3065; // atan(0.3165)
const AURA_BASE_HALF_PX_X: f64 = 320.0;
const AURA_BASE_HALF_PX_Y: f64 = 200.0;

fn quat_normalize(q: [f64; 4]) -> [f64; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n < 1e-9 || !n.is_finite() {
        [1.0, 0.0, 0.0, 0.0]
    } else {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    }
}

fn quat_conjugate(q: [f64; 4]) -> [f64; 4] {
    let n = quat_normalize(q);
    [n[0], -n[1], -n[2], -n[3]]
}

fn quat_rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [w, x, y, z] = quat_normalize(q);
    let [vx, vy, vz] = v;
    let tx = 2.0 * (y * vz - z * vy);
    let ty = 2.0 * (z * vx - x * vz);
    let tz = 2.0 * (x * vy - y * vx);
    [
        vx + w * tx + (y * tz - z * ty),
        vy + w * ty + (z * tx - x * tz),
        vz + w * tz + (x * ty - y * tx),
    ]
}

fn to_f64_3(v: [f32; 3]) -> [f64; 3] {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}

fn to_f64_4(q: [f32; 4]) -> [f64; 4] {
    [
        f64::from(q[0]),
        f64::from(q[1]),
        f64::from(q[2]),
        f64::from(q[3]),
    ]
}

/// Computes Euclidean distance in meters between head camera and the mirrored window center.
pub fn spatial_panel_distance_m(pose: &SpatialPose) -> Option<f32> {
    if !pose.is_usable() {
        return None;
    }
    let dx = pose.panel_pos[0] - pose.head_pos[0];
    let dy = pose.panel_pos[1] - pose.head_pos[1];
    let dz = pose.panel_pos[2] - pose.head_pos[2];
    Some((dx * dx + dy * dy + dz * dz).sqrt())
}

/// Computes horizontal yaw offset in degrees of the panel center relative to the user's gaze (+right).
pub fn spatial_panel_yaw_deg(pose: &SpatialPose) -> Option<f32> {
    if !pose.is_usable() {
        return None;
    }
    let h = to_f64_3(pose.head_pos);
    let p = to_f64_3(pose.panel_pos);
    let v = [p[0] - h[0], p[1] - h[1], p[2] - h[2]];
    let v_cam = quat_rotate(quat_conjugate(to_f64_4(pose.head_quat)), v);
    if v_cam[0].hypot(-v_cam[2]) < 1e-4 {
        return None;
    }
    Some(v_cam[0].atan2(-v_cam[2]).to_degrees() as f32)
}

/// Projects `pose.cursor_pos` onto the `Wired Mirroring` 3D panel plane to recover
/// normalized `(u, v)` coordinates in `[0.01, 0.99]²` (top-left `(0, 0)` to bottom-right `(1, 1)`).
pub fn project_cursor_to_panel_uv(pose: &SpatialPose) -> Option<(f64, f64)> {
    if !pose.is_usable() || !pose.has_cursor() {
        return None;
    }
    let width_m = f64::from(pose.panel_size_m[0]);
    let height_m = f64::from(pose.panel_size_m[1]);
    let panel_pos = to_f64_3(pose.panel_pos);
    let panel_quat = to_f64_4(pose.panel_quat);
    let cursor_pos = to_f64_3(pose.cursor_pos);

    let r_axis = quat_rotate(panel_quat, [1.0, 0.0, 0.0]);
    let u_axis = quat_rotate(panel_quat, [0.0, 1.0, 0.0]);
    let d = [
        cursor_pos[0] - panel_pos[0],
        cursor_pos[1] - panel_pos[1],
        cursor_pos[2] - panel_pos[2],
    ];
    let proj_x = d[0] * r_axis[0] + d[1] * r_axis[1] + d[2] * r_axis[2];
    let proj_y = d[0] * u_axis[0] + d[1] * u_axis[1] + d[2] * u_axis[2];

    let u = proj_x / width_m + 0.5;
    let v = 0.5 - proj_y / height_m;
    if (-0.08..=1.08).contains(&u) && (-0.08..=1.08).contains(&v) {
        Some((u.clamp(0.01, 0.99), v.clamp(0.01, 0.99)))
    } else {
        None
    }
}

/// Projects the 3D window exit point onto the wearer's current head view frustum
/// to produce the HID `(dx, dy)` delta from either the currently active XR cursor or gaze center.
pub fn compute_spatial_spawn_warp(
    pose: &SpatialPose,
    entry: EdgeEntry,
    gain: f64,
) -> Option<(i32, i32)> {
    if !pose.is_usable() || !gain.is_finite() {
        return None;
    }
    let width_m = f64::from(pose.panel_size_m[0]);
    let height_m = f64::from(pose.panel_size_m[1]);
    let panel_pos = to_f64_3(pose.panel_pos);
    let panel_quat = to_f64_4(pose.panel_quat);
    let head_pos = to_f64_3(pose.head_pos);
    let head_quat = to_f64_4(pose.head_quat);
    let head_inv = quat_conjugate(head_quat);

    let r_axis = quat_rotate(panel_quat, [1.0, 0.0, 0.0]);
    let u_axis = quat_rotate(panel_quat, [0.0, 1.0, 0.0]);
    let margin_m = (0.03 * width_m).clamp(0.02, 0.06);

    let (off_x, off_y) = match entry.edge {
        ActiveEdge::Right => (0.5 * width_m + margin_m, -entry.norm_y * 0.5 * height_m),
        ActiveEdge::Left => (-(0.5 * width_m + margin_m), -entry.norm_y * 0.5 * height_m),
        ActiveEdge::Top => (entry.norm_x * 0.5 * width_m, 0.5 * height_m + margin_m),
        ActiveEdge::Bottom => (entry.norm_x * 0.5 * width_m, -(0.5 * height_m + margin_m)),
    };

    let p_exit = [
        panel_pos[0] + off_x * r_axis[0] + off_y * u_axis[0],
        panel_pos[1] + off_x * r_axis[1] + off_y * u_axis[1],
        panel_pos[2] + off_x * r_axis[2] + off_y * u_axis[2],
    ];
    let v = [
        p_exit[0] - head_pos[0],
        p_exit[1] - head_pos[1],
        p_exit[2] - head_pos[2],
    ];
    let v_cam = quat_rotate(head_inv, v);

    // Panel exit point must be in front of the head camera (-Z in OpenXR camera space).
    if -v_cam[2] <= 0.05 {
        return None;
    }

    let exit_yaw = v_cam[0].atan2(-v_cam[2]);
    let exit_pitch = v_cam[1].atan2(v_cam[0].hypot(-v_cam[2]));

    let (start_yaw, start_pitch) = if pose.is_cursor_active() && pose.has_cursor() {
        let cur = to_f64_3(pose.cursor_pos);
        let vc = [
            cur[0] - head_pos[0],
            cur[1] - head_pos[1],
            cur[2] - head_pos[2],
        ];
        let vc_cam = quat_rotate(head_inv, vc);
        if -vc_cam[2] > 0.05 {
            (
                vc_cam[0].atan2(-vc_cam[2]),
                vc_cam[1].atan2(vc_cam[0].hypot(-vc_cam[2])),
            )
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };

    let dyaw = exit_yaw - start_yaw;
    let dpitch = exit_pitch - start_pitch;
    let dx = ((dyaw / AURA_HALF_FOV_X_RAD) * AURA_BASE_HALF_PX_X * gain).clamp(-2400.0, 2400.0);
    let dy = ((-dpitch / AURA_HALF_FOV_Y_RAD) * AURA_BASE_HALF_PX_Y * gain).clamp(-1600.0, 1600.0);

    Some((dx.round() as i32, dy.round() as i32))
}

/// Computes the relative delta `(dx, dy)` needed to move Android XR's center-spawned cursor
/// to the corresponding edge & offset where the host cursor left the mirrored display.
pub fn compute_spawn_warp(entry: EdgeEntry, span_px: f64) -> (i32, i32) {
    let half_w = (span_px * 0.5).max(40.0);
    let half_h = half_w * entry.aspect.clamp(0.3, 2.0);
    let outward_nudge = 28.0;

    let (dx, dy) = match entry.edge {
        ActiveEdge::Right => (half_w + outward_nudge, entry.norm_y * half_h),
        ActiveEdge::Left => (-(half_w + outward_nudge), entry.norm_y * half_h),
        ActiveEdge::Top => (entry.norm_x * half_w, -(half_h + outward_nudge)),
        ActiveEdge::Bottom => (entry.norm_x * half_w, half_h + outward_nudge),
    };
    (dx.round() as i32, dy.round() as i32)
}

/// Splits a total `(total_dx, total_dy)` warp into `steps` even integer steps whose sum
/// equals `(total_dx, total_dy)` exactly.
pub fn split_warp_steps(total_dx: i32, total_dy: i32, steps: usize) -> Vec<(i16, i16)> {
    let count = steps.max(1);
    let mut out = Vec::with_capacity(count);
    let mut acc_x = 0i32;
    let mut acc_y = 0i32;

    for i in 1..=count {
        let target_x = ((i64::from(total_dx) * i as i64) as f64 / count as f64).round() as i32;
        let target_y = ((i64::from(total_dy) * i as i64) as f64 / count as f64).round() as i32;
        let step_x = (target_x - acc_x).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let step_y = (target_y - acc_y).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        acc_x += i32::from(step_x);
        acc_y += i32::from(step_y);
        out.push((step_x, step_y));
    }
    out
}

#[derive(Debug, Default)]
pub struct EdgeSwitchTracker {
    edge_candidate: Option<ActiveEdge>,
    edge_hit_since: Option<Instant>,
    outward_push_px: f64,
    xr_entered_at: Option<Instant>,
    host_returned_at: Option<Instant>,
    entered_edge: Option<ActiveEdge>,
    xr_virtual_pos: (f64, f64),
}

impl EdgeSwitchTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_dwell(&mut self) {
        self.edge_candidate = None;
        self.edge_hit_since = None;
        self.outward_push_px = 0.0;
    }

    pub fn mark_entered_xr(&mut self, now: Instant, entry_edge: Option<ActiveEdge>) {
        self.xr_entered_at = Some(now);
        self.entered_edge = entry_edge;
        self.xr_virtual_pos = (0.0, 0.0);
        self.reset_dwell();
    }

    pub fn mark_returned_host(&mut self, now: Instant) {
        self.host_returned_at = Some(now);
        self.entered_edge = None;
        self.xr_virtual_pos = (0.0, 0.0);
        self.reset_dwell();
    }

    pub fn should_accept_mirror_focus_return(&self, now: Instant, cfg: &EdgeSwitchConfig) -> bool {
        if !cfg.auto_return_on_mirror_focus {
            return false;
        }
        match self.xr_entered_at {
            Some(entered) => now.duration_since(entered) >= Duration::from_millis(400),
            None => true,
        }
    }

    pub fn on_host_mouse_move(
        &mut self,
        now: Instant,
        pos_x: f64,
        pos_y: f64,
        dx: f64,
        dy: f64,
        bounds: DisplayBounds,
        cfg: &EdgeSwitchConfig,
    ) -> Option<EdgeEntry> {
        match self.on_host_mouse_move_with_dwell(now, pos_x, pos_y, dx, dy, bounds, cfg) {
            EdgeMoveOutcome::Triggered(entry) => Some(entry),
            EdgeMoveOutcome::None | EdgeMoveOutcome::DwellStarted(_) => None,
        }
    }

    pub fn on_host_mouse_move_with_dwell(
        &mut self,
        now: Instant,
        pos_x: f64,
        pos_y: f64,
        dx: f64,
        dy: f64,
        bounds: DisplayBounds,
        cfg: &EdgeSwitchConfig,
    ) -> EdgeMoveOutcome {
        if cfg.edge == SwitchEdge::Disabled {
            self.reset_dwell();
            return EdgeMoveOutcome::None;
        }
        if let Some(returned) = self.host_returned_at {
            if now.duration_since(returned) < Duration::from_millis(500) {
                self.reset_dwell();
                return EdgeMoveOutcome::None;
            }
        }

        let width = bounds.max_x - bounds.min_x;
        let height = bounds.max_y - bounds.min_y;
        if width <= 10.0 || height <= 10.0 {
            return EdgeMoveOutcome::None;
        }

        // Reject cursor positions that lie outside the selected monitor's rectangle so
        // adjacent monitors never trigger edge switching.
        if pos_x < bounds.min_x - 2.0
            || pos_x > bounds.max_x + 2.0
            || pos_y < bounds.min_y - 2.0
            || pos_y > bounds.max_y + 2.0
        {
            self.reset_dwell();
            return EdgeMoveOutcome::None;
        }

        let dz = cfg.corner_deadzone_px;
        let check_y_deadzone = height > dz * 2.5;
        let check_x_deadzone = width > dz * 2.5;

        let in_y_band = !check_y_deadzone
            || (pos_y > bounds.min_y + dz && pos_y < bounds.max_y - dz);
        let in_x_band = !check_x_deadzone
            || (pos_x > bounds.min_x + dz && pos_x < bounds.max_x - dz);

        let hit = if cfg.edge.allows(ActiveEdge::Right)
            && pos_x >= bounds.max_x - 3.0
            && pos_x <= bounds.max_x + 2.0
            && in_y_band
            && dx >= 0.0
        {
            Some((ActiveEdge::Right, dx.max(0.0)))
        } else if cfg.edge.allows(ActiveEdge::Left)
            && pos_x >= bounds.min_x - 2.0
            && pos_x <= bounds.min_x + 3.0
            && in_y_band
            && dx <= 0.0
        {
            Some((ActiveEdge::Left, (-dx).max(0.0)))
        } else if cfg.edge.allows(ActiveEdge::Top)
            && pos_y >= bounds.min_y - 2.0
            && pos_y <= bounds.min_y + 3.0
            && in_x_band
            && dy <= 0.0
        {
            Some((ActiveEdge::Top, (-dy).max(0.0)))
        } else if cfg.edge.allows(ActiveEdge::Bottom)
            && pos_y >= bounds.max_y - 3.0
            && pos_y <= bounds.max_y + 2.0
            && in_x_band
            && dy >= 0.0
        {
            Some((ActiveEdge::Bottom, dy.max(0.0)))
        } else {
            None
        };

        let Some((edge, push)) = hit else {
            self.reset_dwell();
            return EdgeMoveOutcome::None;
        };

        if self.edge_candidate != Some(edge) {
            self.edge_candidate = Some(edge);
            self.edge_hit_since = Some(now);
            self.outward_push_px = push;
            return EdgeMoveOutcome::DwellStarted(edge);
        }

        self.outward_push_px += push;
        let elapsed = self
            .edge_hit_since
            .map(|t| now.duration_since(t))
            .unwrap_or_default();

        if elapsed >= Duration::from_millis(cfg.dwell_ms)
            || (self.outward_push_px >= 45.0 && elapsed >= Duration::from_millis(40))
        {
            self.reset_dwell();
            let norm_x = (((pos_x - bounds.min_x) / width) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let norm_y = (((pos_y - bounds.min_y) / height) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let aspect = (height / width).clamp(0.3, 2.0);
            EdgeMoveOutcome::Triggered(EdgeEntry {
                edge,
                norm_x,
                norm_y,
                aspect,
            })
        } else {
            EdgeMoveOutcome::None
        }
    }

    pub fn on_xr_mouse_move(
        &mut self,
        now: Instant,
        dx: f64,
        dy: f64,
        cfg: &EdgeSwitchConfig,
    ) -> bool {
        if !cfg.auto_return_on_edge {
            return false;
        }
        let Some(entry) = self.entered_edge else {
            return false;
        };
        if let Some(entered) = self.xr_entered_at {
            if now.duration_since(entered) < Duration::from_millis(350) {
                return false;
            }
        }

        let thresh = cfg.return_edge_threshold_px.max(20.0);
        match entry {
            ActiveEdge::Right => {
                self.xr_virtual_pos.0 = (self.xr_virtual_pos.0 + dx).min(1200.0);
                self.xr_virtual_pos.0 <= -thresh
            }
            ActiveEdge::Left => {
                self.xr_virtual_pos.0 = (self.xr_virtual_pos.0 + dx).max(-1200.0);
                self.xr_virtual_pos.0 >= thresh
            }
            ActiveEdge::Bottom => {
                self.xr_virtual_pos.1 = (self.xr_virtual_pos.1 + dy).min(1200.0);
                self.xr_virtual_pos.1 <= -thresh
            }
            ActiveEdge::Top => {
                self.xr_virtual_pos.1 = (self.xr_virtual_pos.1 + dy).max(-1200.0);
                self.xr_virtual_pos.1 >= thresh
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_focus_return_on_and_edge_switching_beta_off() {
        let cfg = EdgeSwitchConfig::default();
        assert_eq!(cfg.edge, SwitchEdge::Disabled);
        assert!(cfg.auto_return_on_mirror_focus);
        assert!(!cfg.auto_return_on_edge);
    }

    #[test]
    fn disabled_edge_never_triggers() {
        let mut tracker = EdgeSwitchTracker::new();
        let cfg = EdgeSwitchConfig {
            edge: SwitchEdge::Disabled,
            ..Default::default()
        };
        let bounds = DisplayBounds::default();
        let t0 = Instant::now();
        assert_eq!(
            tracker.on_host_mouse_move(t0, 1919.5, 500.0, 10.0, 0.0, bounds, &cfg),
            None
        );
        assert_eq!(
            tracker.on_host_mouse_move(
                t0 + Duration::from_millis(300),
                1919.5,
                500.0,
                50.0,
                0.0,
                bounds,
                &cfg
            ),
            None
        );
    }

    #[test]
    fn corner_deadzone_blocks_trigger_and_middle_edge_triggers_after_dwell() {
        let mut tracker = EdgeSwitchTracker::new();
        let cfg = EdgeSwitchConfig {
            edge: SwitchEdge::Right,
            ..Default::default()
        };
        let bounds = DisplayBounds::default();
        let t0 = Instant::now();

        // Top-right corner is inside deadzone (y=10 < 48)
        assert_eq!(
            tracker.on_host_mouse_move(t0, 1919.0, 10.0, 20.0, 0.0, bounds, &cfg),
            None
        );
        assert_eq!(
            tracker.on_host_mouse_move(
                t0 + Duration::from_millis(200),
                1919.0,
                10.0,
                60.0,
                0.0,
                bounds,
                &cfg
            ),
            None
        );

        // Middle of right edge triggers after dwell_ms with normalized geometry
        assert_eq!(
            tracker.on_host_mouse_move(t0, 1919.0, 540.0, 5.0, 0.0, bounds, &cfg),
            None
        );
        let entry = tracker
            .on_host_mouse_move(
                t0 + Duration::from_millis(160),
                1919.0,
                540.0,
                2.0,
                0.0,
                bounds,
                &cfg,
            )
            .unwrap();
        assert_eq!(entry.edge, ActiveEdge::Right);
        assert!((entry.norm_y - 0.0).abs() < 0.01);
        assert!((entry.aspect - (1080.0 / 1920.0)).abs() < 0.001);
    }

    #[test]
    fn post_return_cooldown_and_opposite_edge_pullback() {
        let mut tracker = EdgeSwitchTracker::new();
        let cfg = EdgeSwitchConfig {
            edge: SwitchEdge::Right,
            auto_return_on_edge: true,
            return_edge_threshold_px: 100.0,
            ..Default::default()
        };
        let bounds = DisplayBounds::default();
        let t0 = Instant::now();

        tracker.mark_entered_xr(t0, Some(ActiveEdge::Right));
        // Inside 350ms grace window: ignore pull-back
        assert!(!tracker.on_xr_mouse_move(t0 + Duration::from_millis(100), -200.0, 0.0, &cfg));

        // After grace window: moving right goes deeper, pulling left past -100 returns
        assert!(!tracker.on_xr_mouse_move(t0 + Duration::from_millis(400), 150.0, 0.0, &cfg));
        assert!(!tracker.on_xr_mouse_move(t0 + Duration::from_millis(420), -180.0, 0.0, &cfg));
        assert!(tracker.on_xr_mouse_move(t0 + Duration::from_millis(440), -80.0, 0.0, &cfg));

        // Post-return cooldown blocks immediate re-entry on host
        tracker.mark_returned_host(t0 + Duration::from_millis(440));
        assert_eq!(
            tracker.on_host_mouse_move(
                t0 + Duration::from_millis(650),
                1919.5,
                540.0,
                80.0,
                0.0,
                bounds,
                &cfg
            ),
            None
        );
    }

    #[test]
    fn mirror_focus_respects_grace_window() {
        let mut tracker = EdgeSwitchTracker::new();
        let cfg = EdgeSwitchConfig::default();
        let t0 = Instant::now();
        tracker.mark_entered_xr(t0, Some(ActiveEdge::Right));

        assert!(!tracker.should_accept_mirror_focus_return(t0 + Duration::from_millis(150), &cfg));
        assert!(tracker.should_accept_mirror_focus_return(t0 + Duration::from_millis(450), &cfg));
    }

    #[test]
    fn spawn_warp_maps_all_edges_and_offsets() {
        let span = 1000.0;
        let aspect = 0.5; // half_w = 500, half_h = 250

        assert_eq!(
            compute_spawn_warp(
                EdgeEntry {
                    edge: ActiveEdge::Right,
                    norm_x: 1.0,
                    norm_y: -0.5,
                    aspect,
                },
                span
            ),
            (528, -125)
        );
        assert_eq!(
            compute_spawn_warp(
                EdgeEntry {
                    edge: ActiveEdge::Left,
                    norm_x: -1.0,
                    norm_y: 0.8,
                    aspect,
                },
                span
            ),
            (-528, 200)
        );
        assert_eq!(
            compute_spawn_warp(
                EdgeEntry {
                    edge: ActiveEdge::Top,
                    norm_x: 0.4,
                    norm_y: -1.0,
                    aspect,
                },
                span
            ),
            (200, -278)
        );
        assert_eq!(
            compute_spawn_warp(
                EdgeEntry {
                    edge: ActiveEdge::Bottom,
                    norm_x: -0.6,
                    norm_y: 1.0,
                    aspect,
                },
                span
            ),
            (-300, 278)
        );
    }

    #[test]
    fn split_warp_steps_preserves_exact_sum() {
        for (dx, dy) in [(478, -143), (-529, 251), (0, 0), (-17, 9)] {
            let steps = split_warp_steps(dx, dy, 10);
            assert_eq!(steps.len(), 10);
            let sum_x: i32 = steps.iter().map(|s| i32::from(s.0)).sum();
            let sum_y: i32 = steps.iter().map(|s| i32::from(s.1)).sum();
            assert_eq!((sum_x, sum_y), (dx, dy));
        }
    }

    #[test]
    fn spatial_warp_scales_with_panel_size_distance_and_head_yaw() {
        use aura_protocol::{
            SPATIAL_FLAG_CURSOR_ACTIVE, SPATIAL_FLAG_HAS_CURSOR, SPATIAL_FLAG_HAS_HEAD,
            SPATIAL_FLAG_HAS_PANEL, SPATIAL_FLAG_PANEL_ACTIVE,
        };

        let base_pose = SpatialPose {
            flags: SPATIAL_FLAG_HAS_HEAD | SPATIAL_FLAG_HAS_PANEL | SPATIAL_FLAG_PANEL_ACTIVE,
            task_id: 82,
            head_pos: [0.0, 0.0, 0.0],
            head_quat: [1.0, 0.0, 0.0, 0.0],
            panel_pos: [0.0, 0.0, -1.8],
            panel_quat: [1.0, 0.0, 0.0, 0.0],
            panel_size_m: [1.09, 0.613],
            cursor_pos: [0.0, 0.0, 0.0],
        };
        let right_mid = EdgeEntry {
            edge: ActiveEdge::Right,
            norm_x: 1.0,
            norm_y: 0.0,
            aspect: 0.5625,
        };
        let (dx_base, dy_base) = compute_spatial_spawn_warp(&base_pose, right_mid, 1.0).unwrap();
        assert!(dx_base > 150 && dx_base < 350, "dx_base={dx_base}");
        assert!(dy_base.abs() <= 1);

        // Upper-right exit should move upward (negative HID dy)
        let right_top = EdgeEntry {
            norm_y: -0.6,
            ..right_mid
        };
        let (dx_rt, dy_rt) = compute_spatial_spawn_warp(&base_pose, right_top, 1.0).unwrap();
        assert!(dx_rt > 150);
        assert!(dy_rt < -50, "dy_rt={dy_rt}");

        // Larger panel produces larger warp
        let big_pose = SpatialPose {
            panel_size_m: [2.0, 1.125],
            ..base_pose
        };
        let (dx_big, _) = compute_spatial_spawn_warp(&big_pose, right_mid, 1.0).unwrap();
        assert!(dx_big > dx_base);

        // Turning head slightly to the right (-Y rotation in OpenXR right-hand rule) brings right edge closer to gaze center
        let half_theta = (-10.0_f32.to_radians()) * 0.5;
        let head_turned_right = SpatialPose {
            head_quat: [half_theta.cos(), 0.0, half_theta.sin(), 0.0],
            ..base_pose
        };
        let (dx_right_gaze, _) =
            compute_spatial_spawn_warp(&head_turned_right, right_mid, 1.0).unwrap();
        assert!(
            dx_right_gaze < dx_base,
            "expected smaller +dx when looking right: {dx_right_gaze} vs {dx_base}"
        );

        // Active XR cursor already sitting at the right edge yields near-zero delta instead of double-warping
        let cursor_at_edge = SpatialPose {
            flags: base_pose.flags | SPATIAL_FLAG_HAS_CURSOR | SPATIAL_FLAG_CURSOR_ACTIVE,
            cursor_pos: [0.57, 0.0, -1.8],
            ..base_pose
        };
        let (dx_near, dy_near) =
            compute_spatial_spawn_warp(&cursor_at_edge, right_mid, 1.0).unwrap();
        assert!(dx_near.abs() < 20 && dy_near.abs() < 5, "dx_near={dx_near}");

        // Panel behind the head returns None so fallback heuristic takes over
        let behind = SpatialPose {
            panel_pos: [0.0, 0.0, 2.0],
            ..base_pose
        };
        assert_eq!(compute_spatial_spawn_warp(&behind, right_mid, 1.0), None);
    }

    #[test]
    fn panel_uv_projection_maps_bounds_cleanly() {
        use aura_protocol::{SPATIAL_FLAG_HAS_CURSOR, SPATIAL_FLAG_HAS_HEAD, SPATIAL_FLAG_HAS_PANEL};

        let mut pose = SpatialPose {
            flags: SPATIAL_FLAG_HAS_HEAD | SPATIAL_FLAG_HAS_PANEL | SPATIAL_FLAG_HAS_CURSOR,
            task_id: 82,
            head_pos: [0.0, 0.0, 0.0],
            head_quat: [1.0, 0.0, 0.0, 0.0],
            panel_pos: [0.0, 0.0, -2.0],
            panel_quat: [1.0, 0.0, 0.0, 0.0],
            panel_size_m: [1.0, 0.5],
            cursor_pos: [0.0, 0.0, -2.0],
        };

        let (u, v) = project_cursor_to_panel_uv(&pose).unwrap();
        assert!((u - 0.5).abs() < 1e-4 && (v - 0.5).abs() < 1e-4);

        // Top-left corner
        pose.cursor_pos = [-0.5, 0.25, -2.0];
        let (u, v) = project_cursor_to_panel_uv(&pose).unwrap();
        assert!((u - 0.01).abs() < 1e-4 && (v - 0.01).abs() < 1e-4);

        // Bottom-right corner
        pose.cursor_pos = [0.5, -0.25, -2.0];
        let (u, v) = project_cursor_to_panel_uv(&pose).unwrap();
        assert!((u - 0.99).abs() < 1e-4 && (v - 0.99).abs() < 1e-4);

        // Far outside panel -> None
        pose.cursor_pos = [1.5, 0.0, -2.0];
        assert_eq!(project_cursor_to_panel_uv(&pose), None);
    }

    #[test]
    fn cursor_on_other_monitor_never_triggers_edge_switch() {
        let mut tracker = EdgeSwitchTracker::new();
        let cfg = EdgeSwitchConfig {
            edge: SwitchEdge::Any,
            dwell_ms: 100,
            ..Default::default()
        };
        // Secondary external monitor to the right of primary (0..1728)
        let monitor2 = DisplayBounds {
            min_x: 1728.0,
            min_y: 0.0,
            max_x: 3648.0,
            max_y: 1080.0,
        };
        let t0 = Instant::now();

        // Cursor on primary monitor at x=500 or x=0 (primary left edge) never triggers Monitor 2!
        assert_eq!(
            tracker.on_host_mouse_move(t0, 500.0, 540.0, -20.0, 0.0, monitor2, &cfg),
            None
        );
        assert_eq!(
            tracker.on_host_mouse_move(
                t0 + Duration::from_millis(200),
                0.0,
                540.0,
                -40.0,
                0.0,
                monitor2,
                &cfg
            ),
            None
        );

        // Monitor 2's right edge (x=3647) triggers normally after dwell
        assert_eq!(
            tracker.on_host_mouse_move(t0, 3647.5, 540.0, 5.0, 0.0, monitor2, &cfg),
            None
        );
        let entry = tracker
            .on_host_mouse_move(
                t0 + Duration::from_millis(160),
                3647.5,
                540.0,
                5.0,
                0.0,
                monitor2,
                &cfg,
            )
            .unwrap();
        assert_eq!(entry.edge, ActiveEdge::Right);
    }

    #[test]
    fn select_target_display_respects_primary_and_specific_id() {
        let displays = vec![
            DisplayInfo {
                id: "1".into(),
                label: "Built-in".into(),
                is_primary: true,
                bounds: DisplayBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 1728.0,
                    max_y: 1117.0,
                },
            },
            DisplayInfo {
                id: "2".into(),
                label: "External".into(),
                is_primary: false,
                bounds: DisplayBounds {
                    min_x: 1728.0,
                    min_y: 0.0,
                    max_x: 3648.0,
                    max_y: 1080.0,
                },
            },
        ];

        assert_eq!(select_target_display(&displays, "").unwrap().id, "1");
        assert_eq!(select_target_display(&displays, "2").unwrap().id, "2");
        assert_eq!(select_target_display(&displays, "99"), None);
    }
}
