use aura_puck_agent::focus::FocusWatcher;
use aura_puck_agent::server::serve_connection_with_spatial;
use aura_puck_agent::spatial::{query_wmshell_spatial, SpatialWatcher};
use aura_puck_agent::uhid::{HidSink, MockHidSink};
use aura_protocol::DEFAULT_PORT;
use std::env;
use std::net::TcpListener;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Args {
    bind: String,
    mock_uhid: bool,
    self_test: bool,
    verbose: bool,
    no_focus_watch: bool,
    no_spatial_watch: bool,
    dump_spatial: bool,
    mirror_package: String,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        bind: format!("127.0.0.1:{DEFAULT_PORT}"),
        mock_uhid: false,
        self_test: false,
        verbose: false,
        no_focus_watch: false,
        no_spatial_watch: false,
        dump_spatial: false,
        mirror_package: "wiredmirror".to_string(),
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                args.bind = it.next().ok_or("--bind requires <addr:port>")?;
            }
            "--mock-uhid" => args.mock_uhid = true,
            "--self-test" => args.self_test = true,
            "--no-focus-watch" => args.no_focus_watch = true,
            "--no-spatial-watch" => args.no_spatial_watch = true,
            "--dump-spatial" => args.dump_spatial = true,
            "--mirror-package" => {
                args.mirror_package = it.next().ok_or("--mirror-package requires a substring")?;
            }
            "--verbose" | "-v" => args.verbose = true,
            "--help" | "-h" => {
                println!(
                    "aura-puck-agent\n\
                     Options:\n\
                       --bind <addr:port>       TCP listen address (default 127.0.0.1:{DEFAULT_PORT})\n\
                       --mock-uhid              Use in-memory mock sink instead of /dev/uhid\n\
                       --self-test              Verify virtual HID creation on /dev/uhid and exit\n\
                       --mirror-package <str>   Focus/panel substring for mirror app (default wiredmirror)\n\
                       --no-focus-watch         Disable logcat Wired Mirroring focus watcher\n\
                       --no-spatial-watch       Disable WMShell 3D spatial pose watcher\n\
                       --dump-spatial           Query WMShell once, print parsed SpatialPose, and exit\n\
                       --verbose, -v            Print incoming HID reports and spatial updates"
                );
                process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn create_sink(mock_uhid: bool, verbose: bool) -> Result<Box<dyn HidSink>, String> {
    if mock_uhid {
        return Ok(Box::new(MockHidSink::new(verbose)));
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(aura_puck_agent::uhid::linux::LinuxUhidSink::open(
            verbose,
        )?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("real /dev/uhid is only available on Linux/Android; pass --mock-uhid on this OS".into())
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    if args.dump_spatial {
        match query_wmshell_spatial(&args.mirror_package) {
            Ok(pose) => {
                println!(
                    "flags=0x{:02x} has_head={} has_panel={} active={} usable={} \
                     has_cursor={} cursor_active={} task_id={} \
                     head_pos={:?} head_quat={:?} panel_pos={:?} panel_quat={:?} \
                     panel_size_m={:?} cursor_pos={:?}",
                    pose.flags,
                    pose.has_head(),
                    pose.has_panel(),
                    pose.is_panel_active(),
                    pose.is_usable(),
                    pose.has_cursor(),
                    pose.is_cursor_active(),
                    pose.task_id,
                    pose.head_pos,
                    pose.head_quat,
                    pose.panel_pos,
                    pose.panel_quat,
                    pose.panel_size_m,
                    pose.cursor_pos
                );
                return;
            }
            Err(e) => {
                eprintln!("dump-spatial failed: {e}");
                process::exit(1);
            }
        }
    }

    let mut sink = match create_sink(args.mock_uhid, args.verbose) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if args.self_test {
        if let Err(e) = sink.release_all() {
            eprintln!("self-test failed: {e}");
            process::exit(1);
        }
        println!("aura-puck-agent self-test OK");
        return;
    }

    let focus_rx = if !args.mock_uhid && !args.no_focus_watch {
        Some(FocusWatcher::spawn(args.mirror_package.clone(), args.verbose))
    } else {
        None
    };

    let client_connected = Arc::new(AtomicBool::new(false));
    let (spatial_trigger_tx, spatial_rx) = if !args.mock_uhid && !args.no_spatial_watch {
        let (tx, rx) = SpatialWatcher::spawn(
            args.mirror_package.clone(),
            Arc::clone(&client_connected),
            args.verbose,
        );
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let listener = match TcpListener::bind(&args.bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind {}: {e}", args.bind);
            process::exit(1);
        }
    };
    eprintln!("[aura-puck-agent] listening on {}", args.bind);

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Some(ref rx) = focus_rx {
                    while rx.try_recv().is_ok() {}
                }
                if let Some(ref rx) = spatial_rx {
                    while rx.try_recv().is_ok() {}
                }
                client_connected.store(true, Ordering::Release);
                if let Some(ref tx) = spatial_trigger_tx {
                    let _ = tx.send(());
                }
                let peer = stream
                    .peer_addr()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|_| "unknown".into());
                eprintln!("[aura-puck-agent] host connected from {peer}");
                let mirror_pkg = if !args.mock_uhid && !args.no_spatial_watch {
                    Some(args.mirror_package.as_str())
                } else {
                    None
                };
                let serve_res = serve_connection_with_spatial(
                    stream,
                    sink.as_mut(),
                    focus_rx.as_ref(),
                    spatial_trigger_tx.as_ref(),
                    spatial_rx.as_ref(),
                    mirror_pkg,
                    args.verbose,
                );
                client_connected.store(false, Ordering::Release);
                match serve_res {
                    Ok(()) => eprintln!("[aura-puck-agent] host {peer} disconnected cleanly"),
                    Err(e) => eprintln!("[aura-puck-agent] session ended: {e}"),
                }
            }
            Err(e) => {
                eprintln!("[aura-puck-agent] accept error: {e}");
            }
        }
    }
}
