#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cross-platform desktop UI + floating Status HUD entrypoint for Aura KVM.

mod app;
mod hud;
mod theme;

use app::AuraUiApp;
use aura_host::client::ConnectionHub;
use aura_host::config_store::AppSettings;
use aura_host::connector::{AutoConnector, ConnectorConfig};
use aura_host::logbuf;
use aura_host::platform::{install_signal_handlers, InputBackend, SessionController};
use eframe::egui;
use std::env;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;

fn parse_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    let mut adb_path = None;
    let mut agent_path = None;
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--adb-path" => adb_path = it.next().map(PathBuf::from),
            "--agent-path" => agent_path = it.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!(
                    "aura-ui — Desktop UI & Status HUD for Aura KVM\n\
                     \n\
                     OPTIONS:\n\
                       --adb-path <path>    Explicit path to `adb` binary\n\
                       --agent-path <path>  Path to aarch64-unknown-linux-musl aura-puck-agent"
                );
                process::exit(0);
            }
            _ => {}
        }
    }
    (adb_path, agent_path)
}

fn spawn_input_backend(controller: Arc<SessionController>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut backend: Box<dyn InputBackend> = {
            #[cfg(target_os = "macos")]
            {
                Box::new(aura_host::platform::macos::MacOsBackend)
            }
            #[cfg(target_os = "linux")]
            {
                Box::new(aura_host::platform::linux::LinuxEvdevBackend)
            }
            #[cfg(target_os = "windows")]
            {
                Box::new(aura_host::platform::windows::WindowsBackend)
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            {
                Box::new(aura_host::platform::mock::MockBackend)
            }
        };

        if let Err(e) = backend.run(Arc::clone(&controller)) {
            logbuf::push_log(format!("[input] backend stopped: {e}"));
        }
    })
}

fn main() -> Result<(), eframe::Error> {
    let (adb_path, agent_path) = parse_paths();
    let settings = AppSettings::load();

    let hub = Arc::new(ConnectionHub::new());
    let controller = Arc::new(SessionController::new(
        Arc::clone(&hub),
        settings.to_capture_config(),
        None,
    ));
    install_signal_handlers(Arc::clone(&controller));

    let connector_cfg = ConnectorConfig {
        adb_path: adb_path.clone(),
        serial: if settings.serial.trim().is_empty() {
            None
        } else {
            Some(settings.serial.trim().to_string())
        },
        wireless: if settings.wireless.trim().is_empty() {
            None
        } else {
            Some(settings.wireless.trim().to_string())
        },
        direct_tcp: if settings.use_direct_tcp && !settings.direct_tcp.trim().is_empty() {
            Some(settings.direct_tcp.trim().to_string())
        } else {
            None
        },
        agent_path: agent_path.clone(),
        port: settings.port.max(1),
        no_push: false,
        force_push: false,
        once: false,
        verbose: settings.verbose,
    };

    let connector =
        AutoConnector::spawn_managed(connector_cfg, Arc::clone(&hub), Arc::clone(&controller));
    let backend_handle = spawn_input_backend(Arc::clone(&controller));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Aura KVM")
            .with_inner_size([560.0, 740.0])
            .with_min_inner_size([460.0, 540.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Aura KVM",
        options,
        Box::new(move |cc| {
            let ctx_conn = cc.egui_ctx.clone();
            hub.set_state_observer(Arc::new(move |_| ctx_conn.request_repaint()));

            let ctx_mode = cc.egui_ctx.clone();
            controller.set_mode_observer(Arc::new(move |_| ctx_mode.request_repaint()));

            let ctx_log = cc.egui_ctx.clone();
            logbuf::set_observer(Arc::new(move || ctx_log.request_repaint()));

            Ok(Box::new(AuraUiApp::new(
                controller,
                hub,
                connector,
                backend_handle,
                settings,
                adb_path,
                agent_path,
            )))
        }),
    )
}
