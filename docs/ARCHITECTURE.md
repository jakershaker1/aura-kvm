# Aura KVM Architecture

Aura KVM splits input capture, transport, and HID injection across four zero-bloat Rust crates (`aura-protocol`, `aura-puck-agent`, `aura-host`) plus a standalone `eframe`/`egui` UI (`aura-ui`).

## 1. Wire Protocol (`crates/aura-protocol`)

All host ↔ puck traffic flows over a single TCP connection (`127.0.0.1:27190` via `adb forward` or direct TCP) using little-endian length-prefixed frames:

```text
+-----------+---------------+------------------------+
| tag (1 B) | len (u16 LE)  | payload (0..1024 B)    |
+-----------+---------------+------------------------+
```

| Tag | Message | Direction | Payload |
| --- | --- | --- | --- |
| `0x00` | `Hello` | Host → Agent | `PROTOCOL_MAGIC` (`AKVM`), `version: u16` (`1`) |
| `0x80` | `HelloAck` | Agent → Host | `AKVM`, `version: u16`, `capabilities: u16` |
| `0x01` | `KeyReport` | Host → Agent | 8-byte boot-compatible HID keyboard report |
| `0x02` | `MouseReport` | Host → Agent | `buttons: u8`, `dx: i16`, `dy: i16`, `wheel_v: i8`, `wheel_h: i8` |
| `0x03` | `ConsumerReport` | Host → Agent | 16-bit Consumer Page (`0x0C`) usage (`AC_HOME`, `AC_BACK`, volume, media) |
| `0x04` | `ReleaseAll` | Host → Agent | Empty (clears pressed keys & mouse buttons) |
| `0x05` / `0x06` | `Ping` / `Pong` | Bidirectional | `seq: u64` (500 ms heartbeat, 3 s agent watchdog) |
| `0x07` | `FocusReturn` | Agent → Host | Empty (`Wired Mirroring` focused in Android XR) |
| `0x08` | `QuerySpatialPose` | Host → Agent | Empty (triggers immediate `WMShell` sample) |
| `0x09` | `SpatialPose` | Agent → Host | 81 bytes: `flags`, `task_id`, head pos/quat, panel pos/quat/size, cursor pos |
| `0x0A` | `WarpToEdge` | Host → Agent | `edge: u8`, `norm_x: f32`, `norm_y: f32`, `gain: f32` |

Negotiated capability flags in `HelloAck`: `CAP_KEYBOARD`, `CAP_MOUSE`, `CAP_CONSUMER`, `CAP_FOCUS_WATCH`, `CAP_SPATIAL_POSE`, `CAP_SPATIAL_WARP`.

## 2. Unprivileged Virtual HID Injection (`crates/aura-puck-agent/src/uhid.rs`)

- Runs as standard ADB `shell` (`UID 2000`) compiled statically against `aarch64-unknown-linux-musl` (`rust-lld`).
- Opens `/dev/uhid` directly and issues `UHID_CREATE2` for:
  - **Keyboard + Consumer Control** composite report descriptor (Report ID 1: modifiers + 6KRO; Report ID 2: 16-bit consumer usages)
  - **5-Button Relative Mouse** with 16-bit `X`/`Y` deltas and vertical/horizontal scrolling (`VWheel` / `HWheel`)
- Always invokes `release_all()` on disconnect, timeout, or stream error so modifier keys or mouse buttons never get stuck on Android XR.

## 3. Window Switching, Scene Graph Extraction & Closed-Loop Servo

### Why Wired Mirroring Stays on Android XR's Built-In App
On the XREAL Aura Compute Puck, DP-in video capture is exposed on Camera ID `5` with `SYSTEM_CAMERA` restrictions (`0660` device nodes), so unrooted third-party companion apps cannot read the hardware DP-in stream directly. Aura KVM therefore keeps hardware **Wired Mirroring** for zero-latency video and pulls real-time spatial geometry out of Android XR's compositor.

### Live `WMShell` 3D Scene Graph (`spatial.rs`)
Without touching camera HALs (calling `dumpsys media.camera` causes a display blanking side effect and is avoided completely), `aura-puck-agent` parses `dumpsys activity service com.android.systemui/.SystemUIService WMShell`:
- **Head Pose:** `Node<camera>#` (fallback `Node<PerceivedResolutionCamera>#`) → `world_position`, `world_rotation`.
- **Panel Pose & Size:** Matches the `wiredmirror` package via `TaskInfo`, `TaskSubspaceRepository`, and `SurfaceTracking-VRI[MirroringActivity]` to `Node<space-transform-task-<id>>#` and reads world-space width/height in meters from `Node<Resize Placeholder Visual>#` (fallback `Node<Inside Interceptor Box>#`).
- **Live XR Pointer:** Child node under `Node<Mouse Pointer>#` → `world_position` and `active=1 enabled=1` flags.

### Default Focus Return + Beta Single-Monitor Edge Servo (`servo.rs`, `switch.rs`)
- **Focus Auto-Return (Default):** `FocusWatcher` tails `logcat -b events -T 1 -v brief input_focus:I *:S`. When `wiredmirror` receives focus in Android XR, the agent sends a fresh `SpatialPose` followed by `FocusReturn`, and the host projects `cursor_pos` into panel `(u, v)` coordinates to warp the host cursor to the clicked spot.
- **Closed-Loop 3D Edge Warp (Beta Opt-In):** On `WarpToEdge`, `SpatialServo` nudges `/dev/uhid` to wake the XR cursor, samples `pose_0`, sends Move #1 toward the 3D window edge, samples `pose_1`, updates per-axis `counts_per_rad_x/y` calibration from measured angular displacement (initial `700.0` counts/rad), and sends a precision corrective burst if residual angular error exceeds `0.012 rad`.
- **Single-Monitor Edge Selection (`switch.rs`):** Host edge detection validates `(pos_x, pos_y)` strictly within the chosen monitor (`DisplayInfo` / `select_target_display`) so cursors on adjacent displays never trigger switching.

## 4. Cross-Platform Host Capture (`crates/aura-host/src/platform/`)

- **macOS (`macos.rs`):** Installs a `CGEventTap` prioritizing `kCGHIDEventTap` (fallback `kCGSessionEventTap`), locks and hides the host cursor via `CGAssociateMouseAndMouseCursorPosition(0)` + `CGDisplayHideCursor` + drift-pinning `CGWarpMouseCursorPosition`, enumerates monitors with `CGGetActiveDisplayList` + `CGDisplayIsBuiltin`, and exposes `request_permissions()` / `open_permission_settings()` for Accessibility and Input Monitoring.
- **Linux (`linux.rs`):** Reads `/dev/input/event*` devices directly via `evdev` and exclusively grabs them with `EVIOCGRAB` during XR mode, supporting both Wayland and X11 compositors cleanly.
- **Windows (`windows.rs`):** Installs `WH_KEYBOARD_LL` and `WH_MOUSE_LL` (`user32.dll`), clips/centers the cursor during XR mode, and enumerates individual monitors via `EnumDisplayMonitors` + `GetMonitorInfoW`.
