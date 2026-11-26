# Aura KVM

Share your **macOS, Linux (Wayland & X11), or Windows** keyboard and mouse seamlessly with **XREAL Aura glasses** running **Android XR** through the Compute Puck—**no root required**.

```text
+-----------------------------------+        USB / Wireless ADB         +----------------------------------+
| Host Computer (macOS/Linux/Win)   |  <====== TCP :27190 (AKVM) =====> | XREAL Aura Compute Puck (ARM64)  |
|                                   |                                   |                                  |
| • aura-ui (Desktop UI + Status    |   HID Key / Mouse / Consumer      | • aura-puck-agent (static musl)  |
|   HUD overlay)                    | --------------------------------> | • /dev/uhid Virtual HID Devices: |
| • aura-host (headless CLI)        |                                   |   - Aura KVM Keyboard + Consumer |
| • Capture: CGEventTap (macOS HID),|   FocusReturn + Live WMShell      |   - Aura KVM Mouse (5-btn + 2D)  |
|   evdev (Linux), WH_*_LL (Win)    | <-------------------------------- | • FocusWatcher (logcat) +        |
| • Single-monitor edge tracking    |   3D Pose (Head, Panel, Cursor)   |   Closed-Loop 3D SpatialServo    |
+-----------------------------------+                                   +----------------------------------+
```

## How It Works

1. **`aura-ui`** (or the headless **`aura-host`** CLI) discovers the XREAL Aura Compute Puck over USB ADB or Wireless ADB, pushes the bundled static ARM64 agent if needed, and streams keyboard/mouse HID reports whenever XR Mode is active.
2. **`aura-puck-agent`** runs as ADB `shell` (`UID 2000`) on the Compute Puck and opens Linux `/dev/uhid` to create native virtual HID Keyboard, Consumer Control, and 5-button Mouse devices—giving Android XR real spatial pointer input, hover/focus, clicks, 2D trackpad/wheel scrolling, and shortcuts.
3. **3D-Aware Switching & Sync:** Using Android XR's `WMShell` scene graph and `input_focus` event log, clicking **Wired Mirroring** inside XR automatically returns control to your computer (and warps your host cursor to the clicked spot), while optional **Beta** screen-edge switching uses an on-puck closed-loop 3D servo to spawn the XR cursor at your window's real 3D edge.

## Features

- **Window Switching (Default Workflow):**
  - **Hotkey Toggle:** Press `Option + Cmd + X` (macOS) or `Ctrl + Alt + X` (Linux/Windows) to switch keyboard & mouse capture into XREAL Aura.
  - **Focus Auto-Return (Enabled by Default):** Clicking or focusing the **Wired Mirroring** (`wiredmirror`) window inside Android XR automatically returns input control to your host computer and syncs the host cursor to the clicked position.
- **Seamless Screen-Edge Switching `[BETA]` (Opt-In):**
  - Disabled by default; enable in **Window Switching → Seamless Edge Switching (Beta)** or via `--switch-edge <edge>`.
  - **Single-Monitor Configurable:** Restricts edge crossing strictly to one chosen monitor (**Primary Monitor (auto)** or any attached display via UI selector or `--switch-monitor <id>`), so adjacent monitors never misfire.
  - **Closed-Loop 3D Spatial Warp:** Queries live 6DoF head pose, `Wired Mirroring` window 3D size/position, and XR pointer position from `WMShell`, self-calibrating Android XR's pointer scaling so the XR cursor lands on the matching 3D window border even after moving or resizing the window (with automatic fallback to a manual pixel span).
  - Configurable edge dwell delay (`50–400 ms`), 48 px corner deadzones, and optional opposite-edge pull-back.
- **Desktop UI + Floating Status HUD (`aura-ui`):**
  - Launches cleanly with **no Terminal window** (`Aura KVM.app` on macOS, `windows_subsystem = "windows"` on Windows, `Terminal=false` `.desktop` on Linux).
  - Shows live transport (`USB ADB`, `Wireless ADB`, `Direct TCP`), target serial, heartbeat RTT (`ms`), 3D Panel telemetry, and persistent settings (`settings.conf`).
  - Transparent, always-on-top, click-through floating **Status HUD** pill on mode changes (optionally pinned compactly while controlling XR).
  - One-click XR media/nav actions (`Home`, `Back`, `Vol -/+`, `Mute`, `Play/Pause`, `Release All Keys`), live sensitivity/scroll sliders, macOS `Cmd → Ctrl` shortcut translation, and collapsible Activity Log.
- **Auto-Connect & Safety:**
  - Automatically detects and prefers `XREAL` / `Aura` / `XR` USB ADB devices, supports Wireless ADB fallback, and skips redundant `adb push` when file sizes match.
  - 500 ms heartbeat watchdog + guaranteed `ReleaseAll` on unplug, disconnect, or emergency failsafe hotkey.

## Shortcuts

| Action | macOS | Linux / Windows |
| --- | --- | --- |
| **Toggle Host ↔ XREAL Aura** | `Option + Cmd + X` | `Ctrl + Alt + X` |
| **Emergency Failsafe Release** | `Ctrl + Option + Cmd + Esc` | `Ctrl + Alt + Shift + Esc` |
| **XR Home / Back** *(while in XR mode)* | `Option + Cmd + H` / `B` | `Ctrl + Alt + H` / `B` |

## Repository Layout

- [crates/aura-protocol](crates/aura-protocol): Zero-dependency USB HID descriptors, key/mouse state tracker, and binary TCP protocol (`AKVM`).
- [crates/aura-puck-agent](crates/aura-puck-agent): Zero-dependency Android XR `/dev/uhid` agent, `WMShell` 3D spatial parser, closed-loop `SpatialServo`, and `logcat` focus watcher (`aarch64-unknown-linux-musl`).
- [crates/aura-host](crates/aura-host): Zero-dependency cross-platform host capture backends (macOS `CGEventTap`, Linux `evdev`, Windows `WH_*_LL`), auto-connector, edge/display logic, and persistent config store.
- [crates/aura-ui](crates/aura-ui): `eframe`/`egui` desktop control window + transparent Status HUD overlay.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): Deep dive into wire framing, `/dev/uhid` ABI, `WMShell` spatial extraction, and pointer locking.
- [scripts/](scripts) & [packaging/](packaging): Cross-build, macOS `.app` bundling, Phase 0 hardware test, and Linux desktop entry.

## Prerequisites & Building

### Toolchains
- Rust toolchain (`rustup`, `cargo`, `rustc`) with static puck target:
  ```bash
  rustup target add aarch64-unknown-linux-musl
  ```
- Android Platform Tools (`adb`) in `PATH` (for ADB auto-discovery and forwarding).

### Build All Targets (macOS Host)
```bash
./scripts/build-all.sh
```
This runs workspace unit tests, builds release `aura-host` and `aura-ui`, compiles the static ARM64 `aura-puck-agent` (`rust-lld`, no NDK required), packages `target/release/Aura KVM.app`, and cross-checks Linux & Windows targets (`x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`).

### Native Windows / Linux Build
```bash
rustup target add aarch64-unknown-linux-musl
cargo build --release -p aura-puck-agent --target aarch64-unknown-linux-musl
cargo build --release -p aura-ui -p aura-host
```

## Hardware Setup & Phase 0 Validation

1. On the XREAL Aura Compute Puck: `Settings → About device → tap Build number 7 times`.
2. Enable `Developer options → USB debugging` (and optionally `Wireless debugging`).
3. Connect a video-capable USB-C (DisplayPort Alt Mode / Thunderbolt) cable between your computer and the puck's **Charging / DP-In** port.
4. Run the Phase 0 verification script:
   ```bash
   ./scripts/test-puck-phase0.sh
   ```
   You can also inspect live 3D spatial extraction directly on the puck:
   ```bash
   adb shell "/data/local/tmp/aura-hid-agent --dump-spatial"
   ```

## Running

### Desktop UI + Status HUD
- **macOS:**
  ```bash
  # Run directly:
  open "target/release/Aura KVM.app"
  # Or install into /Applications:
  rm -rf "/Applications/Aura KVM.app" && cp -R "target/release/Aura KVM.app" "/Applications/Aura KVM.app"
  open "/Applications/Aura KVM.app"
  ```
- **Windows:** Launch `target\release\aura-ui.exe` (runs without a console window).
- **Linux:** Run `./target/release/aura-ui` or install [packaging/linux/aura-kvm.desktop](packaging/linux/aura-kvm.desktop).

### Headless CLI (`aura-host`)
```bash
./target/release/aura-host
./target/release/aura-host --wireless 192.168.1.42:5555
./target/release/aura-host --direct-tcp 192.168.1.42:27190
./target/release/aura-host --switch-edge right --switch-monitor "" --edge-dwell-ms 140
```

Common flags: `-s/--serial <id>`, `--wireless <ip:port>`, `--direct-tcp <host:port>`, `--port <port>`, `--sensitivity <f>`, `--scroll-sensitivity <f>`, `--[no-]natural-scroll`, `--[no-]mac-shortcuts`, `--switch-edge <off|left|right|top|bottom|left-right|any>`, `--switch-monitor <id>`, `--edge-dwell-ms <ms>`, `--no-mirror-focus-return`, `--edge-return`, `--no-edge-spawn-warp`, `--edge-spawn-span <px>`, `--force-push`, `-v/--verbose`.

## Platform Permissions & macOS Signing

- **macOS:** Requires both **Accessibility** and **Input Monitoring** permissions in `System Settings → Privacy & Security`.
  - Use the in-app **Request Permissions**, **Accessibility Settings**, and **Input Monitoring Settings** buttons in the top banner, then **Quit (`Cmd+Q`) and reopen** `Aura KVM.app` after enabling **Input Monitoring**.
  - **Rebuilding ad-hoc bundles:** `scripts/bundle-macos.sh` signs ad-hoc (`-`) by default, so macOS TCC invalidates cached CDHash entries when you replace `/Applications/Aura KVM.app`. Clear stale entries with:
    ```bash
    tccutil reset Accessibility com.aurakvm.app
    tccutil reset ListenEvent com.aurakvm.app
    ```
  - **Keep permissions across rebuilds:** Create a free local **Code Signing** certificate named **`Aura KVM Signing`** in **Keychain Access → Certificate Assistant → Create a Certificate** (or set `AURA_CODESIGN_IDENTITY`); `bundle-macos.sh` detects and uses it automatically.
- **Linux (Wayland & X11):** Reads `/dev/input/event*` via `evdev` + `EVIOCGRAB`, working identically on Wayland and X11. Add your user to the `input` group (`sudo usermod -aG input $USER` and re-login).
- **Windows:** Uses `user32.dll` `WH_KEYBOARD_LL` and `WH_MOUSE_LL` hooks (no kernel drivers required; run as Administrator only if capturing over elevated apps).

## Testing

```bash
# Workspace unit tests (protocol, spatial parser, servo, edge switching, settings, ADB selection)
cargo test --workspace --lib

# Live TCP loopback integration tests (including reconnect release & SpatialPose/FocusReturn)
cargo test -p aura-host --test integration_loopback
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
