//! Cross-platform ADB discovery and lifecycle helper for `aura-puck-agent`.

use crate::client::TransportKind;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

const REMOTE_AGENT_PATH: &str = "/data/local/tmp/aura-hid-agent";

fn adb_command(adb_bin: &Path) -> Command {
    let mut cmd = Command::new(adb_bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = &mut cmd;
    cmd
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    pub model: Option<String>,
    pub product: Option<String>,
    pub is_wireless: bool,
}

impl AdbDevice {
    pub fn looks_like_aura(&self) -> bool {
        let matches = |s: &str| {
            let low = s.to_ascii_lowercase();
            low.contains("xreal") || low.contains("aura") || low.contains("xr")
        };
        if matches(&self.serial) {
            return true;
        }
        if let Some(ref m) = self.model {
            if matches(m) {
                return true;
            }
        }
        if let Some(ref p) = self.product {
            if matches(p) {
                return true;
            }
        }
        false
    }

    pub fn transport_kind(&self) -> TransportKind {
        if self.is_wireless {
            TransportKind::WirelessAdb
        } else {
            TransportKind::UsbAdb
        }
    }
}

pub fn parse_adb_devices_long(output: &str) -> Vec<AdbDevice> {
    let mut out = Vec::new();
    for line in output.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('*') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(serial) = parts.next() else {
            continue;
        };
        let Some(state) = parts.next() else {
            continue;
        };
        let mut model = None;
        let mut product = None;
        for token in parts {
            if let Some(v) = token.strip_prefix("model:") {
                model = Some(v.to_string());
            } else if let Some(v) = token.strip_prefix("product:") {
                product = Some(v.to_string());
            }
        }
        let is_wireless = serial.contains(':') || serial.contains("._adb-tls-connect._tcp");
        out.push(AdbDevice {
            serial: serial.to_string(),
            state: state.to_string(),
            model,
            product,
            is_wireless,
        });
    }
    out
}

pub fn select_puck_device(
    devices: &[AdbDevice],
    explicit_serial: Option<&str>,
    wireless_addr: Option<&str>,
) -> Result<Option<AdbDevice>, String> {
    let ready: Vec<&AdbDevice> = devices.iter().filter(|d| d.state == "device").collect();

    if let Some(wanted) = explicit_serial {
        return Ok(ready.into_iter().find(|d| d.serial == wanted).cloned());
    }

    if ready.is_empty() {
        if let Some(unauth) = devices.iter().find(|d| d.state == "unauthorized") {
            return Err(format!(
                "device {} is unauthorized — accept 'Allow USB debugging' inside the glasses",
                unauth.serial
            ));
        }
        return Ok(None);
    }

    if ready.len() == 1 {
        return Ok(Some(ready[0].clone()));
    }

    // Multiple ready devices: prefer XREAL/Aura matches first, then USB over wireless.
    let aura_matches: Vec<&AdbDevice> = ready.iter().copied().filter(|d| d.looks_like_aura()).collect();
    if aura_matches.len() == 1 {
        return Ok(Some(aura_matches[0].clone()));
    }
    if aura_matches.len() > 1 {
        let usb_aura: Vec<&AdbDevice> = aura_matches
            .iter()
            .copied()
            .filter(|d| !d.is_wireless)
            .collect();
        if usb_aura.len() == 1 {
            return Ok(Some(usb_aura[0].clone()));
        }
    }

    if let Some(w) = wireless_addr {
        let usb_ready: Vec<&AdbDevice> = ready.iter().copied().filter(|d| !d.is_wireless).collect();
        if usb_ready.len() == 1 && usb_ready[0].looks_like_aura() {
            return Ok(Some(usb_ready[0].clone()));
        }
        if let Some(w_dev) = ready.iter().copied().find(|d| d.serial == w) {
            return Ok(Some(w_dev.clone()));
        }
    }

    let serials: Vec<&str> = ready.iter().map(|d| d.serial.as_str()).collect();
    Err(format!(
        "multiple ADB devices connected ({}); pass --serial <id>",
        serials.join(", ")
    ))
}

pub struct AdbSession {
    adb_bin: PathBuf,
    serial: String,
    transport: TransportKind,
    port: u16,
    agent_child: Option<Child>,
}

impl AdbSession {
    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn transport_kind(&self) -> TransportKind {
        self.transport
    }

    pub fn find_adb(explicit: Option<&Path>) -> Result<PathBuf, String> {
        if let Some(p) = explicit {
            if p.exists() {
                return Ok(p.to_path_buf());
            }
            return Err(format!("specified --adb-path does not exist: {}", p.display()));
        }

        let bin_name = if cfg!(windows) { "adb.exe" } else { "adb" };

        if let Some(path_var) = env::var_os("PATH") {
            for dir in env::split_paths(&path_var) {
                let candidate = dir.join(bin_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }

        let mut fallbacks: Vec<PathBuf> = Vec::new();
        if cfg!(target_os = "macos") {
            fallbacks.push(PathBuf::from("/opt/homebrew/bin/adb"));
            fallbacks.push(PathBuf::from("/usr/local/bin/adb"));
            if let Some(home) = env::var_os("HOME") {
                fallbacks.push(
                    PathBuf::from(home).join("Library/Android/sdk/platform-tools/adb"),
                );
            }
        } else if cfg!(target_os = "linux") {
            fallbacks.push(PathBuf::from("/usr/bin/adb"));
            fallbacks.push(PathBuf::from("/usr/local/bin/adb"));
            if let Some(home) = env::var_os("HOME") {
                fallbacks.push(PathBuf::from(home).join("Android/Sdk/platform-tools/adb"));
            }
        } else if cfg!(windows) {
            if let Some(local) = env::var_os("LOCALAPPDATA") {
                fallbacks.push(
                    PathBuf::from(local).join("Android\\Sdk\\platform-tools\\adb.exe"),
                );
            }
        }

        for candidate in fallbacks {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        Err("could not locate `adb`. Install Android Platform Tools or pass --adb-path.".into())
    }

    pub fn resolve_agent_binary(explicit: Option<&Path>) -> Result<PathBuf, String> {
        if let Some(p) = explicit {
            if p.is_file() {
                return Ok(p.to_path_buf());
            }
            return Err(format!(
                "specified --agent-path does not exist: {}",
                p.display()
            ));
        }

        let mut candidates = Vec::new();
        if let Ok(exe) = env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("aura-puck-agent"));
                if let Some(contents_dir) = dir.parent() {
                    // macOS .app bundle: Contents/MacOS/aura-ui -> Contents/Resources/aura-puck-agent
                    candidates.push(contents_dir.join("Resources/aura-puck-agent"));
                    candidates.push(
                        contents_dir.join("aarch64-unknown-linux-musl/release/aura-puck-agent"),
                    );
                    candidates.push(
                        contents_dir.join("aarch64-unknown-linux-musl/debug/aura-puck-agent"),
                    );
                }
            }
        }
        candidates.push(PathBuf::from(
            "target/aarch64-unknown-linux-musl/release/aura-puck-agent",
        ));
        candidates.push(PathBuf::from(
            "target/aarch64-unknown-linux-musl/debug/aura-puck-agent",
        ));

        for c in candidates {
            if c.is_file() {
                return Ok(c);
            }
        }

        Err(
            "could not find `aura-puck-agent` binary for aarch64-unknown-linux-musl. \
             Build it with `cargo build --release -p aura-puck-agent --target aarch64-unknown-linux-musl` \
             or pass --agent-path."
                .into(),
        )
    }

    pub fn query_devices(adb_bin: &Path) -> Result<Vec<AdbDevice>, String> {
        let out = adb_command(adb_bin)
            .args(["devices", "-l"])
            .output()
            .map_err(|e| format!("failed to run `adb devices -l`: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "adb devices -l failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_adb_devices_long(&String::from_utf8_lossy(&out.stdout)))
    }

    pub fn connect_wireless(adb_bin: &Path, addr: &str) -> Result<(), String> {
        let out = adb_command(adb_bin)
            .args(["connect", addr])
            .output()
            .map_err(|e| format!("failed to run adb connect: {e}"))?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() || combined.to_ascii_lowercase().contains("cannot connect") {
            return Err(format!("adb connect {addr} failed: {}", combined.trim()));
        }
        Ok(())
    }

    pub fn setup_for_device(
        adb_bin: PathBuf,
        device: &AdbDevice,
        agent_path: Option<&Path>,
        no_push: bool,
        force_push: bool,
        port: u16,
        verbose: bool,
    ) -> Result<Self, String> {
        let mut session = Self {
            adb_bin,
            serial: device.serial.clone(),
            transport: device.transport_kind(),
            port,
            agent_child: None,
        };

        if !no_push {
            let local_agent = Self::resolve_agent_binary(agent_path)?;
            let local_len = fs::metadata(&local_agent).map(|m| m.len()).ok();
            let needs_push = if force_push {
                true
            } else {
                match (local_len, session.remote_file_size(REMOTE_AGENT_PATH)) {
                    (Some(l), Some(r)) if l == r => {
                        eprintln!(
                            "[adb] remote agent up-to-date ({l} bytes), skipping push"
                        );
                        false
                    }
                    _ => true,
                }
            };

            if needs_push {
                eprintln!(
                    "[adb] pushing {} -> {REMOTE_AGENT_PATH}",
                    local_agent.display()
                );
                session.run_adb(&[
                    "push",
                    &local_agent.to_string_lossy(),
                    REMOTE_AGENT_PATH,
                ])?;
            }
            session.run_adb(&["shell", "chmod", "755", REMOTE_AGENT_PATH])?;
        }

        // Stop any stale agent instance from an earlier run.
        let _ = session.run_adb(&["shell", "pkill", "-f", "aura-hid-agent"]);

        let bind_arg = format!("127.0.0.1:{port}");
        let mut cmd = adb_command(&session.adb_bin);
        cmd.args(["-s", &session.serial, "shell", REMOTE_AGENT_PATH, "--bind", &bind_arg]);
        if verbose {
            cmd.arg("--verbose");
            cmd.stderr(Stdio::inherit());
        } else {
            cmd.stderr(Stdio::null());
        }
        cmd.stdout(Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to start remote aura-hid-agent: {e}"))?;
        session.agent_child = Some(child);

        thread::sleep(Duration::from_millis(200));
        if let Some(ref mut c) = session.agent_child {
            if let Ok(Some(status)) = c.try_wait() {
                return Err(format!(
                    "remote aura-hid-agent exited early with status {status}. \
                     Run `./scripts/test-puck-phase0.sh` or pass --verbose to inspect /dev/uhid errors."
                ));
            }
        }

        let spec = format!("tcp:{port}");
        session.run_adb(&["forward", &spec, &spec])?;
        eprintln!(
            "[adb] forwarded localhost:{port} -> puck:{port} ({}, {})",
            session.serial, session.transport
        );

        Ok(session)
    }

    fn remote_file_size(&self, remote_path: &str) -> Option<u64> {
        let out = adb_command(&self.adb_bin)
            .args(["-s", &self.serial, "shell", "stat", "-c", "%s", remote_path])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().ok()
    }

    fn run_adb(&self, args: &[&str]) -> Result<(), String> {
        let out = adb_command(&self.adb_bin)
            .args(["-s", &self.serial])
            .args(args)
            .output()
            .map_err(|e| format!("failed to execute adb {:?}: {e}", args))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = if err.trim().is_empty() {
                String::from_utf8_lossy(&out.stdout).to_string()
            } else {
                err.to_string()
            };
            return Err(format!("adb {:?} failed: {}", args, msg.trim()));
        }
        Ok(())
    }
}

impl Drop for AdbSession {
    fn drop(&mut self) {
        if let Some(mut child) = self.agent_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let spec = format!("tcp:{}", self.port);
        let _ = self.run_adb(&["forward", "--remove", &spec]);
        let _ = self.run_adb(&["shell", "pkill", "-f", "aura-hid-agent"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices_long_output() {
        let sample = "\
List of devices attached
1234567890ABCDEF       device usb:1-1 product:aura_puck model:XREAL_Aura device:aura transport_id:1
emulator-5554          unauthorized usb:1-2
192.168.1.42:5555      device product:aura_puck model:XREAL_Aura transport_id:2
";
        let devs = parse_adb_devices_long(sample);
        assert_eq!(devs.len(), 3);
        assert!(!devs[0].is_wireless);
        assert_eq!(devs[0].model.as_deref(), Some("XREAL_Aura"));
        assert!(devs[0].looks_like_aura());
        assert_eq!(devs[1].state, "unauthorized");
        assert!(devs[2].is_wireless);
    }

    #[test]
    fn selects_xreal_usb_device_over_phone_or_wireless() {
        let phone = AdbDevice {
            serial: "R5CT12345".into(),
            state: "device".into(),
            model: Some("Pixel_9".into()),
            product: Some("tokay".into()),
            is_wireless: false,
        };
        let aura_usb = AdbDevice {
            serial: "AURA0001".into(),
            state: "device".into(),
            model: Some("XREAL_Aura".into()),
            product: Some("aura".into()),
            is_wireless: false,
        };
        let aura_wifi = AdbDevice {
            serial: "192.168.1.50:5555".into(),
            state: "device".into(),
            model: Some("XREAL_Aura".into()),
            product: Some("aura".into()),
            is_wireless: true,
        };

        // Prefers Aura USB over phone and wireless Aura
        let picked = select_puck_device(
            &[phone.clone(), aura_wifi.clone(), aura_usb.clone()],
            None,
            Some("192.168.1.50:5555"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(picked.serial, "AURA0001");

        // Unauthorized device reports actionable error when no ready device exists
        let unauth = AdbDevice {
            state: "unauthorized".into(),
            ..aura_usb.clone()
        };
        assert!(select_puck_device(&[unauth], None, None).is_err());

        // Empty list returns Ok(None) so auto-connect keeps searching
        assert_eq!(select_puck_device(&[], None, None).unwrap(), None);
    }
}
