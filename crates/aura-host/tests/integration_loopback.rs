use aura_host::client::{ConnectionHub, PuckClient, TransportKind};
use aura_host::connector::{AutoConnector, ConnectorConfig};
use aura_host::platform::mock::MockBackend;
use aura_host::platform::{CaptureConfig, InputBackend, Modifiers, SessionController};
use aura_host::switch::EdgeSwitchConfig;
use aura_protocol::hid::{KEY_A, KEY_B};
use aura_protocol::{
    SpatialPose, CAP_SPATIAL_WARP, SPATIAL_FLAG_CURSOR_ACTIVE, SPATIAL_FLAG_HAS_CURSOR,
    SPATIAL_FLAG_HAS_HEAD, SPATIAL_FLAG_HAS_PANEL, SPATIAL_FLAG_PANEL_ACTIVE, WARP_EDGE_RIGHT,
};
use aura_puck_agent::server::{
    serve_connection, serve_connection_with_focus, serve_connection_with_spatial,
};
use aura_puck_agent::uhid::{MockHidSink, RecordedEvent};
use std::net::{Shutdown, TcpListener};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn end_to_end_loopback_host_to_agent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let mut sink = MockHidSink::new(false);
    let events_handle = Arc::clone(&sink.events);

    let server_thread = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        serve_connection(stream, &mut sink).unwrap();
    });

    let hub = Arc::new(ConnectionHub::new());
    let client = Arc::new(PuckClient::connect(&addr.to_string(), Arc::new(|| {})).unwrap());
    hub.attach(client, addr.to_string(), TransportKind::DirectTcp);

    let config = CaptureConfig {
        sensitivity: 1.0,
        scroll_sensitivity: 1.0,
        natural_scroll: false,
        mac_shortcuts: true,
        verbose: false,
        edge_switch: EdgeSwitchConfig::default(),
    };
    let controller = Arc::new(SessionController::new(Arc::clone(&hub), config, None));

    let mut backend = MockBackend;
    backend.run(Arc::clone(&controller)).unwrap();

    drop(controller);
    drop(hub);
    thread::sleep(Duration::from_millis(50));
    server_thread.join().unwrap();

    let recorded = events_handle.lock().unwrap();
    assert!(
        recorded.contains(&RecordedEvent::Keyboard([0, 0, KEY_A, 0, 0, 0, 0, 0])),
        "expected KEY_A press in {recorded:?}"
    );
    assert!(
        recorded.contains(&RecordedEvent::Mouse {
            buttons: 0,
            dx: 12,
            dy: -8,
            wheel_v: 0,
            wheel_h: 0,
        }),
        "expected mouse move in {recorded:?}"
    );
    assert!(
        recorded.contains(&RecordedEvent::Mouse {
            buttons: 1,
            dx: 0,
            dy: 0,
            wheel_v: 0,
            wheel_h: 0,
        }),
        "expected left click down in {recorded:?}"
    );
    assert_eq!(recorded.last(), Some(&RecordedEvent::ReleaseAll));
}

#[test]
fn auto_connect_and_reconnect_releases_xr_mode_safely() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    drop(listener);

    let hub = Arc::new(ConnectionHub::new());
    let config = CaptureConfig {
        sensitivity: 1.0,
        scroll_sensitivity: 1.0,
        natural_scroll: false,
        mac_shortcuts: true,
        verbose: false,
        edge_switch: EdgeSwitchConfig::default(),
    };
    let controller = Arc::new(SessionController::new(Arc::clone(&hub), config, None));

    controller.set_xr_active(true);
    assert!(!controller.is_xr_active());

    let connector_cfg = ConnectorConfig {
        adb_path: None,
        serial: None,
        wireless: None,
        direct_tcp: Some(addr.to_string()),
        agent_path: None,
        port,
        no_push: true,
        force_push: false,
        once: false,
        verbose: false,
    };
    let connector = AutoConnector::spawn(
        connector_cfg,
        Arc::clone(&hub),
        Arc::clone(&controller),
    );

    let persistent_listener = TcpListener::bind(addr).unwrap();
    let mut sink1 = MockHidSink::new(false);
    let events1 = Arc::clone(&sink1.events);
    let (stream1, _) = persistent_listener.accept().unwrap();
    let stream1_shutdown = stream1.try_clone().unwrap();
    let srv1 = thread::spawn(move || {
        let _ = serve_connection(stream1, &mut sink1);
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    while !hub.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(hub.is_connected(), "AutoConnector did not connect to server #1");

    let empty = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
        gui: false,
    };
    controller.set_xr_active(true);
    assert!(controller.is_xr_active());
    controller.handle_key(KEY_A, true, empty);
    controller.handle_key(KEY_A, false, empty);
    thread::sleep(Duration::from_millis(80));

    {
        let rec = events1.lock().unwrap();
        assert!(
            rec.contains(&RecordedEvent::Keyboard([0, 0, KEY_A, 0, 0, 0, 0, 0])),
            "server #1 missing KEY_A: {rec:?}"
        );
    }

    stream1_shutdown.shutdown(Shutdown::Both).unwrap();
    let _ = srv1.join();

    let disc_deadline = Instant::now() + Duration::from_secs(5);
    while (hub.is_connected() || controller.is_xr_active()) && Instant::now() < disc_deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!hub.is_connected(), "hub did not detach on disconnect");
    assert!(
        !controller.is_xr_active(),
        "controller did not exit XR mode on disconnect"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !hub.is_connected() && Instant::now() < deadline {
        if let Ok((stream2, _)) = persistent_listener.accept() {
            let mut sink2 = MockHidSink::new(false);
            let events2 = Arc::clone(&sink2.events);
            let srv2 = thread::spawn(move || {
                let _ = serve_connection(stream2, &mut sink2);
            });

            let c_deadline = Instant::now() + Duration::from_secs(3);
            while !hub.is_connected() && Instant::now() < c_deadline {
                thread::sleep(Duration::from_millis(20));
            }
            assert!(hub.is_connected(), "AutoConnector did not reconnect to server #2");

            controller.set_xr_active(true);
            controller.handle_key(KEY_B, true, empty);
            controller.handle_key(KEY_B, false, empty);
            thread::sleep(Duration::from_millis(80));

            controller.request_exit();
            let _ = connector.join();
            let _ = srv2.join();

            let rec2 = events2.lock().unwrap();
            assert!(
                rec2.contains(&RecordedEvent::Keyboard([0, 0, KEY_B, 0, 0, 0, 0, 0])),
                "server #2 missing KEY_B: {rec2:?}"
            );
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for auto-reconnect");
}

#[test]
fn remote_focus_return_exits_xr_mode_automatically() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let (focus_tx, focus_rx) = mpsc::channel::<()>();
    let mut sink = MockHidSink::new(false);

    let srv = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = serve_connection_with_focus(stream, &mut sink, Some(&focus_rx));
    });

    let hub = Arc::new(ConnectionHub::new());
    let controller = Arc::new(SessionController::new(
        Arc::clone(&hub),
        CaptureConfig::default(),
        None,
    ));

    let connector_cfg = ConnectorConfig {
        adb_path: None,
        serial: None,
        wireless: None,
        direct_tcp: Some(addr.to_string()),
        agent_path: None,
        port: addr.port(),
        no_push: true,
        force_push: false,
        once: true,
        verbose: false,
    };
    let connector = AutoConnector::spawn(
        connector_cfg,
        Arc::clone(&hub),
        Arc::clone(&controller),
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    while !hub.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(hub.is_connected());

    controller.set_xr_active(true);
    assert!(controller.is_xr_active());

    // Wait past the 400ms grace window so Wired Mirroring focus triggers auto-return.
    thread::sleep(Duration::from_millis(450));
    focus_tx.send(()).unwrap();

    let return_deadline = Instant::now() + Duration::from_secs(3);
    while controller.is_xr_active() && Instant::now() < return_deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !controller.is_xr_active(),
        "controller did not auto-return to host on FocusReturn"
    );

    controller.request_exit();
    let _ = connector.join();
    let _ = srv.join();
}

#[test]
fn spatial_pose_streams_from_agent_to_host_hub() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let (trig_tx, trig_rx) = mpsc::channel::<()>();
    let (pose_tx, pose_rx) = mpsc::channel::<SpatialPose>();
    let mut sink = MockHidSink::new(false);

    let srv = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = serve_connection_with_spatial(
            stream,
            &mut sink,
            None,
            Some(&trig_tx),
            Some(&pose_rx),
            Some("wiredmirror"),
            false,
        );
    });

    let hub = Arc::new(ConnectionHub::new());
    let controller = Arc::new(SessionController::new(
        Arc::clone(&hub),
        CaptureConfig::default(),
        None,
    ));

    let connector_cfg = ConnectorConfig {
        adb_path: None,
        serial: None,
        wireless: None,
        direct_tcp: Some(addr.to_string()),
        agent_path: None,
        port: addr.port(),
        no_push: true,
        force_push: false,
        once: true,
        verbose: false,
    };
    let connector = AutoConnector::spawn(
        connector_cfg,
        Arc::clone(&hub),
        Arc::clone(&controller),
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    while !hub.is_connected() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(hub.is_connected());
    assert_ne!(hub.capabilities() & CAP_SPATIAL_WARP, 0);

    hub.send(aura_protocol::Message::WarpToEdge {
        edge: WARP_EDGE_RIGHT,
        norm_x: 1.0,
        norm_y: -0.25,
        gain: 1.0,
    });
    hub.send(aura_protocol::Message::QuerySpatialPose);
    trig_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let expected = SpatialPose {
        flags: SPATIAL_FLAG_HAS_HEAD
            | SPATIAL_FLAG_HAS_PANEL
            | SPATIAL_FLAG_PANEL_ACTIVE
            | SPATIAL_FLAG_HAS_CURSOR
            | SPATIAL_FLAG_CURSOR_ACTIVE,
        task_id: 82,
        head_pos: [0.0, 0.0, 0.0],
        head_quat: [1.0, 0.0, 0.0, 0.0],
        panel_pos: [0.2, 0.0, -1.8],
        panel_quat: [1.0, 0.0, 0.0, 0.0],
        panel_size_m: [1.15, 0.65],
        cursor_pos: [0.2, 0.0, -1.8],
    };
    pose_tx.send(expected).unwrap();

    let pose_deadline = Instant::now() + Duration::from_secs(2);
    while hub.latest_spatial_pose().is_none() && Instant::now() < pose_deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let (got, _) = hub.latest_spatial_pose().expect("hub did not receive SpatialPose");
    assert_eq!(got, expected);

    controller.request_exit();
    let _ = connector.join();
    let _ = srv.join();
}
