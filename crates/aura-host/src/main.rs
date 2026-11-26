use aura_host::client::ConnectionHub;
use aura_host::connector::{AutoConnector, ConnectorConfig};
use aura_host::platform::{install_signal_handlers, CaptureConfig, InputBackend, SessionController};
use aura_host::switch::SwitchEdge;
use aura_protocol::DEFAULT_PORT;
use std::env;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

struct CliArgs {
    adb_path: Option<PathBuf>,
    serial: Option<String>,
    wireless: Option<String>,
    direct_tcp: Option<String>,
    agent_path: Option<PathBuf>,
    port: u16,
    no_push: bool,
    force_push: bool,
    once: bool,
    mock_backend: bool,
    config: CaptureConfig,
}

fn parse_args() -> Result<CliArgs, String> {
    let mut args = CliArgs {
        adb_path: None,
        serial: None,
        wireless: None,
        direct_tcp: None,
        agent_path: None,
        port: DEFAULT_PORT,
        no_push: false,
        force_push: false,
        once: false,
        mock_backend: false,
        config: CaptureConfig::default(),
    };

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--adb-path" => {
                args.adb_path = Some(PathBuf::from(
                    it.next().ok_or("--adb-path requires a file path")?,
                ));
            }
            "--serial" | "-s" => {
                args.serial = Some(it.next().ok_or("--serial requires a device ID")?);
            }
            "--wireless" => {
                args.wireless = Some(it.next().ok_or("--wireless requires <ip:port>")?);
            }
            "--direct-tcp" => {
                args.direct_tcp = Some(it.next().ok_or("--direct-tcp requires <host:port>")?);
            }
            "--agent-path" => {
                args.agent_path = Some(PathBuf::from(
                    it.next().ok_or("--agent-path requires a file path")?,
                ));
            }
            "--port" => {
                let raw = it.next().ok_or("--port requires a port number")?;
                args.port = raw.parse().map_err(|_| format!("invalid port: {raw}"))?;
            }
            "--no-push" => args.no_push = true,
            "--force-push" => args.force_push = true,
            "--once" => args.once = true,
            "--mock-backend" => args.mock_backend = true,
            "--sensitivity" => {
                let raw = it.next().ok_or("--sensitivity requires a number")?;
                args.config.sensitivity =
                    raw.parse().map_err(|_| format!("invalid sensitivity: {raw}"))?;
            }
            "--scroll-sensitivity" => {
                let raw = it.next().ok_or("--scroll-sensitivity requires a number")?;
                args.config.scroll_sensitivity = raw
                    .parse()
                    .map_err(|_| format!("invalid scroll-sensitivity: {raw}"))?;
            }
            "--natural-scroll" => args.config.natural_scroll = true,
            "--no-natural-scroll" => args.config.natural_scroll = false,
            "--mac-shortcuts" => args.config.mac_shortcuts = true,
            "--no-mac-shortcuts" => args.config.mac_shortcuts = false,
            "--switch-edge" => {
                let raw = it.next().ok_or("--switch-edge requires a value")?;
                args.config.edge_switch.edge = SwitchEdge::parse(&raw)
                    .ok_or_else(|| format!("invalid --switch-edge '{raw}' (use off|left|right|top|bottom|left-right|any)"))?;
            }
            "--switch-monitor" => {
                args.config.edge_switch.monitor_id =
                    it.next().ok_or("--switch-monitor requires a monitor ID")?;
            }
            "--edge-dwell-ms" => {
                let raw = it.next().ok_or("--edge-dwell-ms requires milliseconds")?;
                args.config.edge_switch.dwell_ms = raw
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --edge-dwell-ms: {raw}"))?
                    .clamp(30, 1000);
            }
            "--no-mirror-focus-return" => {
                args.config.edge_switch.auto_return_on_mirror_focus = false;
            }
            "--edge-return" => {
                args.config.edge_switch.auto_return_on_edge = true;
            }
            "--no-edge-spawn-warp" => {
                args.config.edge_switch.edge_spawn_warp = false;
            }
            "--edge-spawn-span" => {
                let raw = it.next().ok_or("--edge-spawn-span requires pixels")?;
                args.config.edge_switch.edge_spawn_span_px = raw
                    .parse::<f64>()
                    .map_err(|_| format!("invalid --edge-spawn-span: {raw}"))?
                    .clamp(200.0, 2400.0);
            }
            "--verbose" | "-v" => args.config.verbose = true,
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(args)
}

fn print_help() {
    println!(
        "aura-host — Share keyboard & mouse from macOS/Linux/Windows to XREAL Aura (Android XR)\n\
         \n\
         USAGE:\n\
           aura-host [OPTIONS]\n\
         \n\
         OPTIONS:\n\
           --adb-path <path>          Explicit path to `adb` binary\n\
           -s, --serial <id>          Target ADB device serial\n\
           --wireless <ip:port>       Connect via Wireless ADB (and auto-fallback when unplugged)\n\
           --direct-tcp <host:port>   Skip ADB and connect directly to agent TCP socket\n\
           --agent-path <path>        Path to aarch64-unknown-linux-musl aura-puck-agent\n\
           --port <port>              TCP / adb forward port (default {DEFAULT_PORT})\n\
           --no-push                  Assume /data/local/tmp/aura-hid-agent is already on puck\n\
           --force-push               Re-push aura-puck-agent even if remote file size matches\n\
           --once                     Disable auto-reconnect; exit on disconnect or missing puck\n\
           --sensitivity <float>      Mouse delta multiplier (default 1.0)\n\
           --scroll-sensitivity <f>   Scroll multiplier (default 1.0)\n\
           --natural-scroll           Invert scroll direction (default on macOS)\n\
           --no-natural-scroll        Standard scroll direction (default on Linux/Windows)\n\
           --mac-shortcuts            Translate Cmd+C/V/X/Z/A/F/T/W/R/L -> Ctrl+... (default on macOS)\n\
           --no-mac-shortcuts         Forward Cmd/Super as Meta key unchanged\n\
           --switch-edge <edge>       [Beta] Screen edge to enter XR (off|left|right|top|bottom|left-right|any, default off)\n\
           --switch-monitor <id>      [Beta] Target monitor ID for edge switching (default primary)\n\
           --edge-dwell-ms <ms>       [Beta] Edge dwell delay before switching (default 140)\n\
           --no-mirror-focus-return   Disable default auto-return when Wired Mirroring is focused\n\
           --edge-return              [Beta] Enable opposite-edge pull-back while in XR mode\n\
           --no-edge-spawn-warp       Leave XR cursor at view center on edge entry\n\
           --edge-spawn-span <px>     Mirrored window span in XR cursor units (default 900)\n\
           --mock-backend             Run scripted mock input sequence (for testing)\n\
           -v, --verbose              Verbose report logging\n\
           -h, --help                 Show this help"
    );
}

fn create_backend(mock_backend: bool) -> Result<Box<dyn InputBackend>, String> {
    if mock_backend {
        return Ok(Box::new(aura_host::platform::mock::MockBackend));
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(aura_host::platform::macos::MacOsBackend))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(aura_host::platform::linux::LinuxEvdevBackend))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(aura_host::platform::windows::WindowsBackend))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("unsupported host OS; pass --mock-backend".into())
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\nRun `aura-host --help` for usage.");
            process::exit(2);
        }
    };

    let hub = Arc::new(ConnectionHub::new());
    let controller = Arc::new(SessionController::new(Arc::clone(&hub), args.config.clone(), None));
    install_signal_handlers(Arc::clone(&controller));

    let connector_cfg = ConnectorConfig {
        adb_path: args.adb_path,
        serial: args.serial,
        wireless: args.wireless,
        direct_tcp: args.direct_tcp,
        agent_path: args.agent_path,
        port: args.port,
        no_push: args.no_push,
        force_push: args.force_push,
        once: args.once,
        verbose: args.config.verbose,
    };

    let connector_handle =
        AutoConnector::spawn(connector_cfg, Arc::clone(&hub), Arc::clone(&controller));

    if args.mock_backend {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !hub.is_connected() && !controller.should_exit() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
    }

    let mut backend = match create_backend(args.mock_backend) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {e}");
            controller.request_exit();
            let _ = connector_handle.join();
            process::exit(1);
        }
    };

    let run_result = backend.run(Arc::clone(&controller));
    controller.request_exit();
    let _ = connector_handle.join();

    if let Err(e) = run_result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
