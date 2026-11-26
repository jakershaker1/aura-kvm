//! Persistent zero-dependency `key=value` settings store for `aura-ui` and `aura-host`.

use crate::platform::CaptureConfig;
use crate::switch::{EdgeSwitchConfig, SwitchEdge};
use aura_protocol::DEFAULT_PORT;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    pub sensitivity: f64,
    pub scroll_sensitivity: f64,
    pub natural_scroll: bool,
    pub mac_shortcuts: bool,
    pub verbose: bool,
    pub wireless: String,
    pub direct_tcp: String,
    pub use_direct_tcp: bool,
    pub serial: String,
    pub port: u16,
    pub hud_enabled: bool,
    pub hud_pin_in_xr: bool,
    pub hud_opacity: f32,
    pub switch_edge: SwitchEdge,
    pub edge_monitor_id: String,
    pub edge_dwell_ms: u64,
    pub auto_return_on_mirror_focus: bool,
    pub auto_return_on_edge: bool,
    pub edge_spawn_warp: bool,
    pub edge_spawn_span_px: f64,
    pub use_spatial_warp: bool,
    pub spatial_warp_gain: f64,
}

impl Default for AppSettings {
    fn default() -> Self {
        let capture = CaptureConfig::default();
        Self {
            sensitivity: capture.sensitivity,
            scroll_sensitivity: capture.scroll_sensitivity,
            natural_scroll: capture.natural_scroll,
            mac_shortcuts: capture.mac_shortcuts,
            verbose: capture.verbose,
            wireless: String::new(),
            direct_tcp: format!("127.0.0.1:{DEFAULT_PORT}"),
            use_direct_tcp: false,
            serial: String::new(),
            port: DEFAULT_PORT,
            hud_enabled: true,
            hud_pin_in_xr: true,
            hud_opacity: 0.92,
            switch_edge: capture.edge_switch.edge,
            edge_monitor_id: capture.edge_switch.monitor_id,
            edge_dwell_ms: capture.edge_switch.dwell_ms,
            auto_return_on_mirror_focus: capture.edge_switch.auto_return_on_mirror_focus,
            auto_return_on_edge: capture.edge_switch.auto_return_on_edge,
            edge_spawn_warp: capture.edge_switch.edge_spawn_warp,
            edge_spawn_span_px: capture.edge_switch.edge_spawn_span_px,
            use_spatial_warp: capture.edge_switch.use_spatial_warp,
            spatial_warp_gain: capture.edge_switch.spatial_warp_gain,
        }
    }
}

impl AppSettings {
    pub fn config_path() -> Option<PathBuf> {
        if cfg!(target_os = "macos") {
            env::var_os("HOME").map(|h| {
                PathBuf::from(h).join("Library/Application Support/aura-kvm/settings.conf")
            })
        } else if cfg!(target_os = "windows") {
            env::var_os("APPDATA")
                .map(|a| PathBuf::from(a).join("aura-kvm").join("settings.conf"))
        } else if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            Some(PathBuf::from(xdg).join("aura-kvm/settings.conf"))
        } else {
            env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/aura-kvm/settings.conf"))
        }
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match fs::read_to_string(path) {
            Ok(text) => Self::parse_str(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = Self::config_path() else {
            return Err("could not determine user config directory".into());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create config dir {}: {e}", parent.display()))?;
        }
        fs::write(&path, self.serialize())
            .map_err(|e| format!("failed to write {}: {e}", path.display()))
    }

    pub fn to_capture_config(&self) -> CaptureConfig {
        CaptureConfig {
            sensitivity: self.sensitivity.clamp(0.1, 5.0),
            scroll_sensitivity: self.scroll_sensitivity.clamp(0.1, 5.0),
            natural_scroll: self.natural_scroll,
            mac_shortcuts: self.mac_shortcuts,
            verbose: self.verbose,
            edge_switch: EdgeSwitchConfig {
                edge: self.switch_edge,
                monitor_id: self.edge_monitor_id.clone(),
                dwell_ms: self.edge_dwell_ms.clamp(30, 1000),
                auto_return_on_mirror_focus: self.auto_return_on_mirror_focus,
                auto_return_on_edge: self.auto_return_on_edge,
                edge_spawn_warp: self.edge_spawn_warp,
                edge_spawn_span_px: self.edge_spawn_span_px.clamp(200.0, 2400.0),
                use_spatial_warp: self.use_spatial_warp,
                spatial_warp_gain: self.spatial_warp_gain.clamp(0.25, 2.5),
                ..EdgeSwitchConfig::default()
            },
        }
    }

    pub fn parse_str(input: &str) -> Self {
        let mut s = Self::default();
        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let k = key.trim();
            let v = val.trim();
            match k {
                "sensitivity" => {
                    if let Ok(n) = v.parse::<f64>() {
                        if n.is_finite() {
                            s.sensitivity = n.clamp(0.1, 5.0);
                        }
                    }
                }
                "scroll_sensitivity" => {
                    if let Ok(n) = v.parse::<f64>() {
                        if n.is_finite() {
                            s.scroll_sensitivity = n.clamp(0.1, 5.0);
                        }
                    }
                }
                "natural_scroll" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.natural_scroll = b;
                    }
                }
                "mac_shortcuts" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.mac_shortcuts = b;
                    }
                }
                "verbose" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.verbose = b;
                    }
                }
                "wireless" => s.wireless = v.to_string(),
                "direct_tcp" => {
                    if !v.is_empty() {
                        s.direct_tcp = v.to_string();
                    }
                }
                "use_direct_tcp" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.use_direct_tcp = b;
                    }
                }
                "serial" => s.serial = v.to_string(),
                "port" => {
                    if let Ok(p) = v.parse::<u16>() {
                        if p > 0 {
                            s.port = p;
                        }
                    }
                }
                "hud_enabled" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.hud_enabled = b;
                    }
                }
                "hud_pin_in_xr" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.hud_pin_in_xr = b;
                    }
                }
                "hud_opacity" => {
                    if let Ok(n) = v.parse::<f32>() {
                        if n.is_finite() {
                            s.hud_opacity = n.clamp(0.3, 1.0);
                        }
                    }
                }
                "switch_edge" => {
                    if let Some(edge) = SwitchEdge::parse(v) {
                        s.switch_edge = edge;
                    }
                }
                "edge_monitor_id" => s.edge_monitor_id = v.to_string(),
                "edge_dwell_ms" => {
                    if let Ok(ms) = v.parse::<u64>() {
                        s.edge_dwell_ms = ms.clamp(30, 1000);
                    }
                }
                "auto_return_on_mirror_focus" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.auto_return_on_mirror_focus = b;
                    }
                }
                "auto_return_on_edge" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.auto_return_on_edge = b;
                    }
                }
                "edge_spawn_warp" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.edge_spawn_warp = b;
                    }
                }
                "edge_spawn_span_px" => {
                    if let Ok(n) = v.parse::<f64>() {
                        if n.is_finite() {
                            s.edge_spawn_span_px = n.clamp(200.0, 2400.0);
                        }
                    }
                }
                "use_spatial_warp" => {
                    if let Ok(b) = v.parse::<bool>() {
                        s.use_spatial_warp = b;
                    }
                }
                "spatial_warp_gain" => {
                    if let Ok(n) = v.parse::<f64>() {
                        if n.is_finite() {
                            s.spatial_warp_gain = n.clamp(0.25, 2.5);
                        }
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        format!(
            "# Aura KVM Settings\n\
             sensitivity={:.3}\n\
             scroll_sensitivity={:.3}\n\
             natural_scroll={}\n\
             mac_shortcuts={}\n\
             verbose={}\n\
             wireless={}\n\
             direct_tcp={}\n\
             use_direct_tcp={}\n\
             serial={}\n\
             port={}\n\
             hud_enabled={}\n\
             hud_pin_in_xr={}\n\
             hud_opacity={:.3}\n\
             switch_edge={}\n\
             edge_monitor_id={}\n\
             edge_dwell_ms={}\n\
             auto_return_on_mirror_focus={}\n\
             auto_return_on_edge={}\n\
             edge_spawn_warp={}\n\
             edge_spawn_span_px={:.1}\n\
             use_spatial_warp={}\n\
             spatial_warp_gain={:.3}\n",
            self.sensitivity,
            self.scroll_sensitivity,
            self.natural_scroll,
            self.mac_shortcuts,
            self.verbose,
            self.wireless.replace(['\r', '\n'], ""),
            self.direct_tcp.replace(['\r', '\n'], ""),
            self.use_direct_tcp,
            self.serial.replace(['\r', '\n'], ""),
            self.port,
            self.hud_enabled,
            self.hud_pin_in_xr,
            self.hud_opacity,
            self.switch_edge.as_str(),
            self.edge_monitor_id.replace(['\r', '\n'], ""),
            self.edge_dwell_ms,
            self.auto_return_on_mirror_focus,
            self.auto_return_on_edge,
            self.edge_spawn_warp,
            self.edge_spawn_span_px,
            self.use_spatial_warp,
            self.spatial_warp_gain,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_malformed_resilience() {
        let orig = AppSettings {
            sensitivity: 1.75,
            scroll_sensitivity: 0.65,
            natural_scroll: false,
            mac_shortcuts: true,
            verbose: true,
            wireless: "192.168.1.99:5555".into(),
            direct_tcp: "127.0.0.1:28000".into(),
            use_direct_tcp: true,
            serial: "AURA123".into(),
            port: 28000,
            hud_enabled: false,
            hud_pin_in_xr: false,
            hud_opacity: 0.75,
            switch_edge: SwitchEdge::LeftRight,
            edge_monitor_id: "2".into(),
            edge_dwell_ms: 210,
            auto_return_on_mirror_focus: false,
            auto_return_on_edge: true,
            edge_spawn_warp: false,
            edge_spawn_span_px: 1120.0,
            use_spatial_warp: false,
            spatial_warp_gain: 1.15,
        };

        let text = orig.serialize();
        let parsed = AppSettings::parse_str(&text);
        assert_eq!(parsed, orig);

        let noisy = format!(
            "# comment\ngarbage_no_equals\nunknown=xyz\nsensitivity=not_a_number\nhud_opacity=NaN\n{text}"
        );
        let parsed_noisy = AppSettings::parse_str(&noisy);
        assert_eq!(parsed_noisy, orig);
    }
}
