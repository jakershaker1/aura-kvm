//! On-puck closed-loop 3D cursor servo and live HID-to-radians self-calibration.

use crate::spatial::query_wmshell_spatial;
use crate::uhid::HidSink;
use aura_protocol::{
    SpatialPose, WARP_EDGE_BOTTOM, WARP_EDGE_LEFT, WARP_EDGE_RIGHT, WARP_EDGE_TOP,
};
use std::thread;
use std::time::Duration;

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

/// Computes the 3D world-space point just outside the specified edge & offset of the
/// `Wired Mirroring` window.
pub fn target_exit_point(
    pose: &SpatialPose,
    edge: u8,
    norm_x: f32,
    norm_y: f32,
) -> Option<[f64; 3]> {
    if !pose.is_usable() {
        return None;
    }
    let width_m = f64::from(pose.panel_size_m[0]);
    let height_m = f64::from(pose.panel_size_m[1]);
    let panel_pos = to_f64_3(pose.panel_pos);
    let panel_quat = to_f64_4(pose.panel_quat);
    let r_axis = quat_rotate(panel_quat, [1.0, 0.0, 0.0]);
    let u_axis = quat_rotate(panel_quat, [0.0, 1.0, 0.0]);
    let margin_m = (0.025 * width_m).clamp(0.015, 0.045);

    let nx = f64::from(norm_x).clamp(-1.0, 1.0);
    let ny = f64::from(norm_y).clamp(-1.0, 1.0);
    let (off_x, off_y) = match edge {
        WARP_EDGE_RIGHT => (0.5 * width_m + margin_m, -ny * 0.5 * height_m),
        WARP_EDGE_LEFT => (-(0.5 * width_m + margin_m), -ny * 0.5 * height_m),
        WARP_EDGE_TOP => (nx * 0.5 * width_m, 0.5 * height_m + margin_m),
        WARP_EDGE_BOTTOM => (nx * 0.5 * width_m, -(0.5 * height_m + margin_m)),
        _ => return None,
    };

    Some([
        panel_pos[0] + off_x * r_axis[0] + off_y * u_axis[0],
        panel_pos[1] + off_x * r_axis[1] + off_y * u_axis[1],
        panel_pos[2] + off_x * r_axis[2] + off_y * u_axis[2],
    ])
}

/// Computes `(yaw_rad, pitch_rad)` of a 3D world point relative to the head camera (+right, +up).
pub fn ray_angles_in_head(pose: &SpatialPose, world_pt: [f64; 3]) -> Option<(f64, f64)> {
    if !pose.has_head() {
        return None;
    }
    let h = to_f64_3(pose.head_pos);
    let head_inv = quat_conjugate(to_f64_4(pose.head_quat));
    let v = [world_pt[0] - h[0], world_pt[1] - h[1], world_pt[2] - h[2]];
    let vc = quat_rotate(head_inv, v);
    if -vc[2] <= 0.05 {
        return None;
    }
    let yaw = vc[0].atan2(-vc[2]);
    let pitch = vc[1].atan2(vc[0].hypot(-vc[2]));
    Some((yaw, pitch))
}

pub fn split_steps(total_dx: i32, total_dy: i32, steps: usize) -> Vec<(i16, i16)> {
    let count = steps.max(1);
    let mut out = Vec::with_capacity(count);
    let mut acc_x = 0i32;
    let mut acc_y = 0i32;
    for i in 1..=count {
        let target_x = ((i64::from(total_dx) * i as i64) as f64 / count as f64).round() as i32;
        let target_y = ((i64::from(total_dy) * i as i64) as f64 / count as f64).round() as i32;
        let sx = (target_x - acc_x).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let sy = (target_y - acc_y).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        acc_x += i32::from(sx);
        acc_y += i32::from(sy);
        out.push((sx, sy));
    }
    out
}

pub struct SpatialServo {
    pub counts_per_rad_x: f64,
    pub counts_per_rad_y: f64,
    calibrated_x: bool,
    calibrated_y: bool,
}

impl Default for SpatialServo {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialServo {
    pub fn new() -> Self {
        Self {
            counts_per_rad_x: 700.0,
            counts_per_rad_y: 700.0,
            calibrated_x: false,
            calibrated_y: false,
        }
    }

    pub fn update_calibration(
        &mut self,
        sent_dx: i32,
        sent_dy: i32,
        actual_dyaw: f64,
        actual_dpitch: f64,
    ) {
        if sent_dx.abs() >= 25
            && actual_dyaw.abs() >= 0.015
            && f64::from(sent_dx).signum() == actual_dyaw.signum()
        {
            let measured = (f64::from(sent_dx) / actual_dyaw).clamp(150.0, 4000.0);
            if self.calibrated_x {
                self.counts_per_rad_x = 0.7 * measured + 0.3 * self.counts_per_rad_x;
            } else {
                self.counts_per_rad_x = measured;
                self.calibrated_x = true;
            }
        }
        // Positive HID dy moves the cursor downward (negative head-space pitch).
        let neg_dpitch = -actual_dpitch;
        if sent_dy.abs() >= 25
            && neg_dpitch.abs() >= 0.015
            && f64::from(sent_dy).signum() == neg_dpitch.signum()
        {
            let measured = (f64::from(sent_dy) / neg_dpitch).clamp(150.0, 4000.0);
            if self.calibrated_y {
                self.counts_per_rad_y = 0.7 * measured + 0.3 * self.counts_per_rad_y;
            } else {
                self.counts_per_rad_y = measured;
                self.calibrated_y = true;
            }
        }
    }

    pub fn plan_delta(&self, dyaw: f64, dpitch: f64, gain: f64) -> (i32, i32) {
        let g = if gain.is_finite() {
            gain.clamp(0.25, 2.5)
        } else {
            1.0
        };
        let dx = (dyaw * self.counts_per_rad_x * g).clamp(-1600.0, 1600.0);
        let dy = (-dpitch * self.counts_per_rad_y * g).clamp(-1200.0, 1200.0);
        (dx.round() as i32, dy.round() as i32)
    }

    fn send_burst<S: HidSink + ?Sized>(sink: &mut S, dx: i32, dy: i32, steps: usize) {
        for (sx, sy) in split_steps(dx, dy, steps) {
            if sx != 0 || sy != 0 {
                let _ = sink.send_mouse(0, sx, sy, 0, 0);
                sink.poll_drain();
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Performs a two-stage closed-loop warp to place the Android XR pointer directly
    /// at the 3D window border matching `(edge, norm_x, norm_y)`.
    pub fn warp_to_edge<S: HidSink + ?Sized>(
        &mut self,
        sink: &mut S,
        mirror_package: &str,
        edge: u8,
        norm_x: f32,
        norm_y: f32,
        gain: f32,
        verbose: bool,
    ) -> Option<SpatialPose> {
        let wake_dx: i16 = if edge == WARP_EDGE_LEFT { -2 } else { 2 };
        let _ = sink.send_mouse(0, wake_dx, 0, 0, 0);
        sink.poll_drain();
        thread::sleep(Duration::from_millis(30));

        let pose_0 = query_wmshell_spatial(mirror_package).ok()?;
        let Some(target_0) = target_exit_point(&pose_0, edge, norm_x, norm_y) else {
            return Some(pose_0);
        };
        let Some((target_yaw_0, target_pitch_0)) = ray_angles_in_head(&pose_0, target_0) else {
            return Some(pose_0);
        };

        let cur0_valid = pose_0.is_cursor_active();
        let (cur0_yaw, cur0_pitch) = if cur0_valid {
            ray_angles_in_head(&pose_0, to_f64_3(pose_0.cursor_pos)).unwrap_or((0.0, 0.0))
        } else {
            (0.0, 0.0)
        };

        let (dx_1, dy_1) = self.plan_delta(
            target_yaw_0 - cur0_yaw,
            target_pitch_0 - cur0_pitch,
            f64::from(gain),
        );
        Self::send_burst(sink, dx_1, dy_1, 8);
        thread::sleep(Duration::from_millis(30));

        let pose_1 = match query_wmshell_spatial(mirror_package) {
            Ok(p) => p,
            Err(_) => return Some(pose_0),
        };

        if pose_1.is_usable() && pose_1.has_cursor() {
            if let Some((cur1_yaw, cur1_pitch)) =
                ray_angles_in_head(&pose_1, to_f64_3(pose_1.cursor_pos))
            {
                let (ref0_yaw, ref0_pitch) = if cur0_valid {
                    ray_angles_in_head(&pose_1, to_f64_3(pose_0.cursor_pos)).unwrap_or((0.0, 0.0))
                } else {
                    (0.0, 0.0)
                };
                self.update_calibration(dx_1, dy_1, cur1_yaw - ref0_yaw, cur1_pitch - ref0_pitch);

                if let Some(target_1) = target_exit_point(&pose_1, edge, norm_x, norm_y) {
                    if let Some((target_yaw_1, target_pitch_1)) =
                        ray_angles_in_head(&pose_1, target_1)
                    {
                        let err_yaw = target_yaw_1 - cur1_yaw;
                        let err_pitch = target_pitch_1 - cur1_pitch;
                        if verbose {
                            eprintln!(
                                "[servo] move1=({dx_1},{dy_1}) err=({:.3},{:.3})rad cal=({:.0},{:.0})",
                                err_yaw,
                                err_pitch,
                                self.counts_per_rad_x,
                                self.counts_per_rad_y
                            );
                        }
                        if err_yaw.abs() > 0.012 || err_pitch.abs() > 0.012 {
                            let (dx_2, dy_2) = self.plan_delta(err_yaw, err_pitch, f64::from(gain));
                            Self::send_burst(sink, dx_2, dy_2, 6);
                        }
                    }
                }
            }
        }

        Some(pose_1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::{SPATIAL_FLAG_HAS_HEAD, SPATIAL_FLAG_HAS_PANEL, SPATIAL_FLAG_PANEL_ACTIVE};

    #[test]
    fn target_exit_tracks_moved_and_rotated_panel() {
        // Real moved window sample: panel_pos=[0.403526, 0.113731, -2.05352], yawed slightly
        let pose = SpatialPose {
            flags: SPATIAL_FLAG_HAS_HEAD | SPATIAL_FLAG_HAS_PANEL | SPATIAL_FLAG_PANEL_ACTIVE,
            task_id: 85,
            head_pos: [0.055, -0.080, 0.045],
            head_quat: [0.962, -0.267, -0.019, -0.048],
            panel_pos: [0.4035, 0.1137, -2.0535],
            panel_quat: [0.996, 0.0, -0.085, 0.0],
            panel_size_m: [1.1837, 0.6657],
            cursor_pos: [0.0; 3],
        };

        let right = target_exit_point(&pose, WARP_EDGE_RIGHT, 1.0, 0.0).unwrap();
        let left = target_exit_point(&pose, WARP_EDGE_LEFT, -1.0, 0.0).unwrap();
        assert!(right[0] > 0.95 && left[0] < -0.15);
        assert!((right[1] - 0.1137).abs() < 1e-3);

        // Symmetric about panel center along X/Z
        let mid_x = 0.5 * (right[0] + left[0]);
        let mid_z = 0.5 * (right[2] + left[2]);
        assert!((mid_x - 0.4035).abs() < 1e-3);
        assert!((mid_z - (-2.0535)).abs() < 1e-3);
    }

    #[test]
    fn calibration_converges_and_closes_loop_error() {
        let mut servo = SpatialServo::new();
        // Simulate hardware with true scale = 420 counts/rad in X and 380 in Y
        let true_x = 420.0;
        let true_y = 380.0;
        let target_yaw = 0.30_f64;
        let target_pitch = -0.15_f64;

        // Pass 1 uses initial 700 counts/rad guess -> overshoots!
        let (dx1, dy1) = servo.plan_delta(target_yaw, target_pitch, 1.0);
        let landed1_yaw = f64::from(dx1) / true_x;
        let landed1_pitch = -f64::from(dy1) / true_y;
        assert!((landed1_yaw - target_yaw).abs() > 0.15);

        // Servo measures actual displacement and updates calibration
        servo.update_calibration(dx1, dy1, landed1_yaw, landed1_pitch);
        assert!((servo.counts_per_rad_x - true_x).abs() < 3.0);
        assert!((servo.counts_per_rad_y - true_y).abs() < 3.0);

        // Pass 2 corrects remaining error to < 0.005 rad
        let (dx2, dy2) = servo.plan_delta(target_yaw - landed1_yaw, target_pitch - landed1_pitch, 1.0);
        let final_yaw = landed1_yaw + f64::from(dx2) / true_x;
        let final_pitch = landed1_pitch - f64::from(dy2) / true_y;
        assert!((final_yaw - target_yaw).abs() < 0.005);
        assert!((final_pitch - target_pitch).abs() < 0.005);
    }
}
