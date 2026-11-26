//! Extracts live 3D headset camera pose and `Wired Mirroring` window size/pose
//! from Android XR's `WMShell` Impress scene graph without touching camera HALs.

use aura_protocol::{
    SpatialPose, SPATIAL_FLAG_CURSOR_ACTIVE, SPATIAL_FLAG_HAS_CURSOR, SPATIAL_FLAG_HAS_HEAD,
    SPATIAL_FLAG_HAS_PANEL, SPATIAL_FLAG_PANEL_ACTIVE,
};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
struct TaskCandidate {
    attached: bool,
    visible: bool,
    focused: bool,
    surface_match: bool,
}

impl TaskCandidate {
    fn merge(&mut self, other: &TaskCandidate) {
        self.attached |= other.attached;
        self.visible |= other.visible;
        self.focused |= other.focused;
        self.surface_match |= other.surface_match;
    }
}

#[derive(Debug, Clone)]
struct TaskNode {
    task_id: u32,
    active: bool,
    enabled: bool,
    attached_tree: bool,
    indent: usize,
    pos: [f32; 3],
    quat: [f32; 4],
    has_pose: bool,
    size_m: Option<[f32; 2]>,
    size_from_resize_visual: bool,
}

#[derive(Debug, Clone)]
struct CameraCandidate {
    is_primary_camera: bool,
    active: bool,
    enabled: bool,
    pos: [f32; 3],
    quat: [f32; 4],
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

fn parse_angle_floats(after_key: &str) -> Option<Vec<f32>> {
    let lt = after_key.find('<')?;
    let rest = &after_key[lt + 1..];
    let gt = rest.find('>')?;
    let inside = &rest[..gt];
    let mut vals = Vec::with_capacity(4);
    for raw in inside.split(',') {
        let cleaned = raw.trim().trim_end_matches(['i', 'j', 'k']);
        if cleaned.is_empty() {
            continue;
        }
        let f = cleaned.parse::<f32>().ok()?;
        if !f.is_finite() {
            return None;
        }
        vals.push(f);
    }
    Some(vals)
}

fn extract_vec3(line: &str, key: &str) -> Option<[f32; 3]> {
    let idx = line.find(key)?;
    let vals = parse_angle_floats(&line[idx + key.len()..])?;
    if vals.len() >= 3 {
        Some([vals[0], vals[1], vals[2]])
    } else {
        None
    }
}

fn extract_quat(line: &str, key: &str) -> Option<[f32; 4]> {
    let idx = line.find(key)?;
    let vals = parse_angle_floats(&line[idx + key.len()..])?;
    if vals.len() >= 4 {
        Some([vals[0], vals[1], vals[2], vals[3]])
    } else {
        None
    }
}

fn has_flag_one(line: &str, flag: &str) -> bool {
    line.contains(flag)
}

fn parse_task_transform_id(trimmed: &str) -> Option<u32> {
    let rest = trimmed.strip_prefix("Node<space-transform-task-")?;
    let end = rest.find('>')?;
    rest[..end].parse::<u32>().ok()
}

fn parse_u32_after(line: &str, prefix: &str) -> Option<u32> {
    let idx = line.find(prefix)?;
    let rest = &line[idx + prefix.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

fn package_matches_mirror(mirror_package_lower: &str) -> bool {
    mirror_package_lower.contains("wiredmirror") || mirror_package_lower.contains("mirroring")
}

/// Pure parser for Android XR `WMShell` `dumpsys` output.
pub fn parse_wmshell_spatial(dump: &str, mirror_package: &str) -> SpatialPose {
    let mirror_lower = mirror_package.trim().to_ascii_lowercase();
    let mut pose = SpatialPose::default();

    let mut cameras: Vec<CameraCandidate> = Vec::new();
    let mut task_nodes: Vec<TaskNode> = Vec::new();
    let mut candidates: HashMap<u32, TaskCandidate> = HashMap::new();

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum NodeKind {
        Other,
        Camera { idx: usize },
        Task(usize),
        ResizeVisual(usize),
        InterceptorBox(usize),
        MouseCursor,
    }

    let mut current_node = NodeKind::Other;
    let mut current_task_stack: Vec<usize> = Vec::new();
    let mut detached_indent: Option<usize> = None;
    let mut last_surface_mirror_indent: Option<usize> = None;
    let mut mouse_pointer_indent: Option<usize> = None;
    let mut mouse_cursor_active = false;
    let mut best_cursor: Option<(bool, [f32; 3])> = None;

    for raw_line in dump.lines() {
        let line = raw_line.trim_end_matches('\r');
        let indent = leading_spaces(line);
        let trimmed = &line[indent..];

        let is_node_header = trimmed.starts_with("Node<") || trimmed.starts_with("Node#");
        if is_node_header {
            if let Some(mp_ind) = mouse_pointer_indent {
                if indent <= mp_ind {
                    mouse_pointer_indent = None;
                }
            }
            if let Some(d_ind) = detached_indent {
                if indent <= d_ind {
                    detached_indent = None;
                }
            }
            while let Some(&idx) = current_task_stack.last() {
                if indent <= task_nodes[idx].indent {
                    current_task_stack.pop();
                } else {
                    break;
                }
            }
            if let Some(s_ind) = last_surface_mirror_indent {
                if indent < s_ind {
                    last_surface_mirror_indent = None;
                }
            }

            if let Some(mp_ind) = mouse_pointer_indent {
                if indent > mp_ind {
                    mouse_cursor_active =
                        has_flag_one(trimmed, "active=1") && has_flag_one(trimmed, "enabled=1");
                    current_node = NodeKind::MouseCursor;
                    continue;
                }
            }

            if trimmed.starts_with("Node<Detached Nodes>#") {
                detached_indent = Some(indent);
                current_node = NodeKind::Other;
                continue;
            }

            if trimmed.starts_with("Node<Mouse Pointer>#") {
                mouse_pointer_indent = Some(indent);
                current_node = NodeKind::Other;
                continue;
            }

            if trimmed.starts_with("Node<camera>#") {
                cameras.push(CameraCandidate {
                    is_primary_camera: true,
                    active: has_flag_one(trimmed, "active=1"),
                    enabled: has_flag_one(trimmed, "enabled=1"),
                    pos: [0.0; 3],
                    quat: [1.0, 0.0, 0.0, 0.0],
                });
                current_node = NodeKind::Camera {
                    idx: cameras.len() - 1,
                };
                continue;
            }

            if trimmed.starts_with("Node<PerceivedResolutionCamera>#") {
                cameras.push(CameraCandidate {
                    is_primary_camera: false,
                    active: has_flag_one(trimmed, "active=1"),
                    enabled: has_flag_one(trimmed, "enabled=1"),
                    pos: [0.0; 3],
                    quat: [1.0, 0.0, 0.0, 0.0],
                });
                current_node = NodeKind::Camera {
                    idx: cameras.len() - 1,
                };
                continue;
            }

            if trimmed.contains("SurfaceTracking-VRI[MirroringActivity]")
                && !mirror_lower.is_empty()
                && package_matches_mirror(&mirror_lower)
            {
                last_surface_mirror_indent = Some(indent);
                current_node = NodeKind::Other;
                continue;
            }

            if let Some(task_id) = parse_task_transform_id(trimmed) {
                if last_surface_mirror_indent == Some(indent) {
                    candidates
                        .entry(task_id)
                        .or_default()
                        .surface_match = true;
                    last_surface_mirror_indent = None;
                }
                let node_idx = task_nodes.len();
                task_nodes.push(TaskNode {
                    task_id,
                    active: has_flag_one(trimmed, "active=1"),
                    enabled: has_flag_one(trimmed, "enabled=1"),
                    attached_tree: detached_indent.is_none(),
                    indent,
                    pos: [0.0; 3],
                    quat: [1.0, 0.0, 0.0, 0.0],
                    has_pose: false,
                    size_m: None,
                    size_from_resize_visual: false,
                });
                current_task_stack.push(node_idx);
                current_node = NodeKind::Task(node_idx);
                continue;
            }

            if let Some(&task_idx) = current_task_stack.last() {
                if trimmed.starts_with("Node<Resize Placeholder Visual>#") {
                    current_node = NodeKind::ResizeVisual(task_idx);
                    continue;
                }
                if trimmed.starts_with("Node<Inside Interceptor Box>#") {
                    current_node = NodeKind::InterceptorBox(task_idx);
                    continue;
                }
            }

            current_node = NodeKind::Other;
            continue;
        }

        // Parse world_* properties belonging to the active Impress node
        if trimmed.contains("world_position=")
            || trimmed.contains("world_rotation=")
            || trimmed.contains("world_scale=")
        {
            match current_node {
                NodeKind::Camera { idx } => {
                    if let Some(p) = extract_vec3(trimmed, "world_position=") {
                        cameras[idx].pos = p;
                    }
                    if let Some(q) = extract_quat(trimmed, "world_rotation=") {
                        cameras[idx].quat = q;
                    }
                }
                NodeKind::Task(idx) => {
                    if let Some(p) = extract_vec3(trimmed, "world_position=") {
                        task_nodes[idx].pos = p;
                        task_nodes[idx].has_pose = true;
                    }
                    if let Some(q) = extract_quat(trimmed, "world_rotation=") {
                        task_nodes[idx].quat = q;
                    }
                }
                NodeKind::ResizeVisual(idx) => {
                    if let Some(scale) = extract_vec3(trimmed, "world_scale=") {
                        if scale[0] > 0.05 && scale[1] > 0.05 {
                            task_nodes[idx].size_m = Some([scale[0], scale[1]]);
                            task_nodes[idx].size_from_resize_visual = true;
                        }
                    }
                }
                NodeKind::InterceptorBox(idx) => {
                    if !task_nodes[idx].size_from_resize_visual {
                        if let Some(scale) = extract_vec3(trimmed, "world_scale=") {
                            if scale[0] > 0.05 && scale[1] > 0.05 {
                                task_nodes[idx].size_m = Some([scale[0], scale[1]]);
                            }
                        }
                    }
                }
                NodeKind::MouseCursor => {
                    if let Some(p) = extract_vec3(trimmed, "world_position=") {
                        let replace = match best_cursor {
                            None => true,
                            Some((was_active, _)) => mouse_cursor_active && !was_active,
                        };
                        if replace {
                            best_cursor = Some((mouse_cursor_active, p));
                        }
                    }
                }
                NodeKind::Other => {}
            }
        }

        // Pass 2 information (TaskSubspaceRepository & TaskInfo lines)
        if !mirror_lower.is_empty() {
            let line_lower = trimmed.to_ascii_lowercase();
            if line_lower.contains(&mirror_lower) {
                if let Some(tid) = parse_u32_after(trimmed, "Task #") {
                    let cand = TaskCandidate {
                        attached: trimmed.contains("isAttachedToScene=true"),
                        visible: false,
                        focused: false,
                        surface_match: false,
                    };
                    candidates.entry(tid).or_default().merge(&cand);
                } else if let Some(tid) = parse_u32_after(trimmed, "taskId=") {
                    let cand = TaskCandidate {
                        attached: false,
                        visible: trimmed.contains("isVisible=true"),
                        focused: trimmed.contains("isFocused=true"),
                        surface_match: false,
                    };
                    candidates.entry(tid).or_default().merge(&cand);
                }
            }
        }
    }

    // Select best head camera pose
    if let Some(cam) = cameras
        .into_iter()
        .max_by_key(|c| (c.is_primary_camera, c.active, c.enabled))
    {
        pose.head_pos = cam.pos;
        pose.head_quat = cam.quat;
        pose.flags |= SPATIAL_FLAG_HAS_HEAD;
    }

    // Select best wiredmirror spatial task panel
    let mut best_panel: Option<(TaskNode, (bool, bool, bool, bool, bool, bool, u32))> = None;
    for node in task_nodes {
        if !node.has_pose {
            continue;
        }
        let Some([w, h]) = node.size_m else {
            continue;
        };
        if w <= 0.05 || h <= 0.05 {
            continue;
        }
        let Some(cand) = candidates.get(&node.task_id) else {
            continue;
        };
        let rank = (
            node.active,
            node.attached_tree,
            cand.attached,
            cand.visible,
            cand.focused,
            node.enabled || cand.surface_match,
            node.task_id,
        );
        if best_panel
            .as_ref()
            .map(|(_, r)| rank > *r)
            .unwrap_or(true)
        {
            best_panel = Some((node, rank));
        }
    }

    if let Some((node, _)) = best_panel {
        let [w, h] = node.size_m.unwrap_or([0.0, 0.0]);
        pose.task_id = node.task_id;
        pose.panel_pos = node.pos;
        pose.panel_quat = node.quat;
        pose.panel_size_m = [w, h];
        pose.flags |= SPATIAL_FLAG_HAS_PANEL;
        if node.active || node.attached_tree {
            pose.flags |= SPATIAL_FLAG_PANEL_ACTIVE;
        }
    }

    if let Some((active, pos)) = best_cursor {
        pose.cursor_pos = pos;
        pose.flags |= SPATIAL_FLAG_HAS_CURSOR;
        if active {
            pose.flags |= SPATIAL_FLAG_CURSOR_ACTIVE;
        }
    }

    pose
}

pub fn query_wmshell_spatial(mirror_package: &str) -> Result<SpatialPose, String> {
    let output = Command::new("dumpsys")
        .args([
            "activity",
            "service",
            "com.android.systemui/.SystemUIService",
            "WMShell",
        ])
        .output()
        .or_else(|_| {
            Command::new("/system/bin/dumpsys")
                .args([
                    "activity",
                    "service",
                    "com.android.systemui/.SystemUIService",
                    "WMShell",
                ])
                .output()
        })
        .map_err(|e| format!("failed to execute dumpsys WMShell: {e}"))?;

    if !output.status.success() {
        return Err(format!("dumpsys WMShell exited with status {}", output.status));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_wmshell_spatial(&stdout, mirror_package))
}

pub struct SpatialWatcher;

impl SpatialWatcher {
    pub fn spawn(
        mirror_package: String,
        active_session: Arc<AtomicBool>,
        verbose: bool,
    ) -> (mpsc::Sender<()>, mpsc::Receiver<SpatialPose>) {
        let (trigger_tx, trigger_rx) = mpsc::channel::<()>();
        let (pose_tx, pose_rx) = mpsc::channel::<SpatialPose>();

        thread::spawn(move || {
            let mut last_query: Option<Instant> = None;
            loop {
                let triggered = match trigger_rx.recv_timeout(Duration::from_millis(750)) {
                    Ok(()) => {
                        while trigger_rx.try_recv().is_ok() {}
                        true
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => false,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };

                if !active_session.load(Ordering::Acquire) {
                    continue;
                }

                if triggered {
                    if let Some(prev) = last_query {
                        let since = prev.elapsed();
                        if since < Duration::from_millis(80) {
                            thread::sleep(Duration::from_millis(80) - since);
                            while trigger_rx.try_recv().is_ok() {}
                        }
                    }
                } else if let Some(prev) = last_query {
                    if prev.elapsed() < Duration::from_millis(650) {
                        continue;
                    }
                }

                last_query = Some(Instant::now());
                match query_wmshell_spatial(&mirror_package) {
                    Ok(pose) => {
                        if pose.has_head() || pose.has_panel() {
                            if verbose {
                                eprintln!(
                                    "[spatial] task={} active={} size={:.2}x{:.2}m head={:?}",
                                    pose.task_id,
                                    pose.is_panel_active(),
                                    pose.panel_size_m[0],
                                    pose.panel_size_m[1],
                                    pose.head_pos
                                );
                            }
                            if pose_tx.send(pose).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        if verbose {
                            eprintln!("[spatial] {e}");
                        }
                    }
                }
            }
        });

        (trigger_tx, pose_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DUMP: &str = r#"
      Node<PerceivedResolutionCamera>#658 enabled=0 active=0 valid=1
      world_position=< -0.188557, -0.078493, 5.97098 > world_rotation=< 0.888, -0.039i,  0.458j,  0.020k> world_scale=< 1, 1, 1 >
      Node<Detached Nodes>#18 enabled=0 active=0 valid=1
      world_position=< 0, 0, 0 > world_rotation=< 1.000,  0.000i,  0.000j,  0.000k>
        Node<space-transform-task-81>#131799 enabled=0 active=0 valid=1
        world_position=< -0.285343, -0.271891, -1.77553 > world_rotation=< 0.997,  0.000i,  0.079j,  0.000k> world_scale=< 1.76139, 1.76139, 1.76139 >
          Node<Resize Placeholder Visual>#131817 enabled=1 active=0 valid=1
          world_position=< -0.285065, -0.271891, -1.77379 > world_rotation=< 0.997,  0.000i,  0.079j,  0.000k> world_scale=< 1.1791, 0.663112, 1.76139 >
      Node<Root>#1 enabled=1 active=1 valid=1
      world_position=< 0, 0, 0 > world_rotation=< 1.000, 0.000i, 0.000j, 0.000k >
        Node<SurfaceTracking-VRI[MirroringActivity]>#131975 enabled=1 active=1 valid=1
        world_position=< 0, 0, 0 > world_rotation=< 1.000, 0.000i, 0.000j, 0.000k >
        Node<space-transform-task-82>#131921 enabled=1 active=1 valid=1
        local_position=< 4.7952, 0.283442, -1.79189 > local_rotation=< 0.960, -0.000i, -0.279j, -0.000k>
        world_position=< 4.7952, 0.283442, -1.79189 > world_rotation=< 0.960, -0.000i, -0.279j, -0.000k> world_scale=< 1.83476, 1.83476, 1.83476 > world_forward=< 0.536301, -1.79434e-09, -0.844027 >
          Node<Resize Placeholder Visual>#131945 enabled=1 active=1 valid=1
          world_position=< 4.79422, 0.283442, -1.79034 > world_rotation=< 0.960, -0.000i, -0.279j, -0.000k> world_scale=< 1.09006, 0.613024, 1.83476 >
          Node<Inside Interceptor Box>#131946 enabled=1 active=1 valid=1
          world_scale=< 1.20000, 0.70000, 1.20000 >
      Node<camera>#3 enabled=1 active=1 valid=1
      world_position=< -0.0680735, -0.0465074, -0.149775 > world_rotation=< 0.712, -0.698i,  0.003j, -0.079k> world_scale=< 1, 1, 1 >

      mTaskInfo=TaskInfo{userId=0 taskId=81 baseIntent=Intent { cmp=com.google.android.apps.vr.wiredmirror/.mirroring.MirroringActivity } isFocused=false isVisible=false}
      mTaskInfo=TaskInfo{userId=0 taskId=82 baseIntent=Intent { cmp=com.google.android.apps.vr.wiredmirror/.mirroring.MirroringActivity } isFocused=true isVisible=true}
    "#;

    #[test]
    fn parses_active_head_camera_and_wired_mirror_panel() {
        let pose = parse_wmshell_spatial(SAMPLE_DUMP, "wiredmirror");
        assert!(pose.is_usable());
        assert!(pose.is_panel_active());
        assert_eq!(pose.task_id, 82);
        assert!((pose.head_pos[0] - (-0.0680735)).abs() < 1e-5);
        assert!((pose.head_quat[1] - (-0.698)).abs() < 1e-4);
        assert!((pose.panel_pos[0] - 4.7952).abs() < 1e-4);
        assert!((pose.panel_quat[2] - (-0.279)).abs() < 1e-4);
        assert!((pose.panel_size_m[0] - 1.09006).abs() < 1e-4);
        assert!((pose.panel_size_m[1] - 0.613024).abs() < 1e-4);
    }

    #[test]
    fn supports_surface_tracking_and_interceptor_box_fallback() {
        let dump = r#"
      Node<PerceivedResolutionCamera>#658 enabled=1 active=1 valid=1
      world_position=< 0, 0, 0 > world_rotation=< 1.0, 0i, 0j, 0k >
        Node<SurfaceTracking-VRI[MirroringActivity]>#99 enabled=0 active=0 valid=1
        Node<space-transform-task-95>#100 enabled=1 active=0 valid=1
        world_position=< 0.1, 0.2, -2.0 > world_rotation=< 1.0, 0i, 0j, 0k >
          Node<Inside Interceptor Box>#101 enabled=1 active=0 valid=1
          world_position=< 0.1, 0.2, -2.0 > world_scale=< 1.35, 0.85, 1.35 >
        "#;

        let pose = parse_wmshell_spatial(dump, "wiredmirror");
        assert!(pose.is_usable());
        assert_eq!(pose.task_id, 95);
        assert!((pose.panel_size_m[0] - 1.35).abs() < 1e-4);
        assert!((pose.panel_size_m[1] - 0.85).abs() < 1e-4);
    }

    #[test]
    fn subspace_repository_attached_task_wins_even_without_taskinfo() {
        let dump = r#"
      Node<camera>#3 enabled=1 active=1 valid=1
      world_position=< 0, 0, 0 > world_rotation=< 1.0, 0i, 0j, 0k >
        Node<space-transform-task-50>#1 enabled=0 active=0 valid=1
        world_position=< 0, 0, -2.0 > world_rotation=< 1.0, 0i, 0j, 0k >
          Node<Resize Placeholder Visual>#2 enabled=1 active=0 valid=1
          world_scale=< 1.0, 0.5, 1.0 >
      TaskSubspaceRepository:
        Task #50 {com.google.android.apps.vr.wiredmirror, isAttachedToScene=true}
        "#;

        let pose = parse_wmshell_spatial(dump, "wiredmirror");
        assert!(pose.is_usable());
        assert_eq!(pose.task_id, 50);
    }

    #[test]
    fn parses_mouse_pointer_child_world_position_and_active_flag() {
        let dump = r#"
      Node<camera>#3 enabled=1 active=1 valid=1
      world_position=< 0, 0, 0 > world_rotation=< 1.0, 0i, 0j, 0k >
        Node<Mouse Pointer>#66 enabled=1 active=1 valid=1
        world_position=< 0, 0, 0 > world_rotation=< 1.0, 0i, 0j, 0k >
          Node#91 enabled=1 active=1 valid=1
          world_position=< 0.242797, 0.466858, -1.6072 > world_rotation=< 0.989, 0.133i, -0.058j, 0.008k >
        "#;

        let pose = parse_wmshell_spatial(dump, "wiredmirror");
        assert!(pose.has_cursor());
        assert!(pose.is_cursor_active());
        assert!((pose.cursor_pos[0] - 0.242797).abs() < 1e-5);
        assert!((pose.cursor_pos[1] - 0.466858).abs() < 1e-5);
        assert!((pose.cursor_pos[2] - (-1.6072)).abs() < 1e-5);
    }
}
