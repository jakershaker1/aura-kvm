//! Background auto-connect and reconnect supervisor for `aura-host`.

use crate::adb::{select_puck_device, AdbSession};
use crate::client::{ConnectionHub, ConnectionState, PuckClient, TransportKind};
use crate::logbuf::push_log;
use crate::platform::SessionController;
use aura_protocol::SpatialPose;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    pub adb_path: Option<PathBuf>,
    pub serial: Option<String>,
    pub wireless: Option<String>,
    pub direct_tcp: Option<String>,
    pub agent_path: Option<PathBuf>,
    pub port: u16,
    pub no_push: bool,
    pub force_push: bool,
    pub once: bool,
    pub verbose: bool,
}

pub struct ConnectorHandle {
    config: Arc<RwLock<ConnectorConfig>>,
    generation: Arc<AtomicU64>,
    hub: Arc<ConnectionHub>,
    controller: Arc<SessionController>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl ConnectorHandle {
    pub fn update_and_reconnect(&self, new_config: ConnectorConfig) {
        {
            let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
            *guard = new_config;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.controller.force_host_mode();
        self.hub.detach(ConnectionState::Searching);
    }

    pub fn trigger_reconnect(&self, force_push: bool) {
        if force_push {
            let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
            guard.force_push = true;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.controller.force_host_mode();
        self.hub.detach(ConnectionState::Searching);
    }

    pub fn join(mut self) {
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

pub struct AutoConnector;

impl AutoConnector {
    pub fn spawn(
        config: ConnectorConfig,
        hub: Arc<ConnectionHub>,
        controller: Arc<SessionController>,
    ) -> thread::JoinHandle<()> {
        let mut managed = Self::spawn_managed(config, hub, controller);
        managed.join_handle.take().unwrap()
    }

    pub fn spawn_managed(
        config: ConnectorConfig,
        hub: Arc<ConnectionHub>,
        controller: Arc<SessionController>,
    ) -> ConnectorHandle {
        let shared_cfg = Arc::new(RwLock::new(config));
        let generation = Arc::new(AtomicU64::new(0));
        let cfg_c = Arc::clone(&shared_cfg);
        let gen_c = Arc::clone(&generation);
        let hub_c = Arc::clone(&hub);
        let ctrl_c = Arc::clone(&controller);

        let join_handle = thread::spawn(move || {
            Self::run_loop(cfg_c, gen_c, hub_c, ctrl_c);
        });

        ConnectorHandle {
            config: shared_cfg,
            generation,
            hub,
            controller,
            join_handle: Some(join_handle),
        }
    }

    fn sleep_interruptible(
        controller: &SessionController,
        generation: &AtomicU64,
        observed_gen: u64,
        total: Duration,
    ) {
        let mut elapsed = Duration::ZERO;
        let step = Duration::from_millis(100);
        while elapsed < total
            && !controller.should_exit()
            && generation.load(Ordering::Acquire) == observed_gen
        {
            let chunk = step.min(total - elapsed);
            thread::sleep(chunk);
            elapsed += chunk;
        }
    }

    fn run_loop(
        shared_cfg: Arc<RwLock<ConnectorConfig>>,
        generation: Arc<AtomicU64>,
        hub: Arc<ConnectionHub>,
        controller: Arc<SessionController>,
    ) {
        let mut backoff_ms = 500u64;
        let mut last_error: Option<String> = None;

        while !controller.should_exit() {
            let gen_now = generation.load(Ordering::Acquire);
            let cfg = shared_cfg.read().unwrap_or_else(|e| e.into_inner()).clone();

            if let Some(ref addr) = cfg.direct_tcp {
                Self::step_direct_tcp(
                    addr,
                    &cfg,
                    &generation,
                    gen_now,
                    &hub,
                    &controller,
                    &mut backoff_ms,
                );
            } else {
                Self::step_adb(
                    &shared_cfg,
                    &cfg,
                    &generation,
                    gen_now,
                    &hub,
                    &controller,
                    &mut last_error,
                );
            }
        }
    }

    fn make_disconnect_handler(
        hub: &Arc<ConnectionHub>,
        controller: &Arc<SessionController>,
        once: bool,
    ) -> Arc<dyn Fn() + Send + Sync> {
        let hub_c = Arc::clone(hub);
        let ctrl_c = Arc::clone(controller);
        Arc::new(move || {
            ctrl_c.force_host_mode();
            hub_c.detach(ConnectionState::Searching);
            if once {
                ctrl_c.request_exit();
            }
        })
    }

    fn make_focus_handler(
        hub: &Arc<ConnectionHub>,
        controller: &Arc<SessionController>,
    ) -> Arc<dyn Fn() + Send + Sync> {
        let hub_c = Arc::clone(hub);
        let ctrl_c = Arc::clone(controller);
        Arc::new(move || {
            ctrl_c.handle_remote_focus_return();
            hub_c.notify_focus_return();
        })
    }

    fn make_spatial_handler(hub: &Arc<ConnectionHub>) -> Arc<dyn Fn(SpatialPose) + Send + Sync> {
        let hub_c = Arc::clone(hub);
        Arc::new(move |pose| {
            hub_c.update_spatial_pose(pose);
        })
    }

    fn step_direct_tcp(
        addr: &str,
        config: &ConnectorConfig,
        generation: &AtomicU64,
        gen_now: u64,
        hub: &Arc<ConnectionHub>,
        controller: &Arc<SessionController>,
        backoff_ms: &mut u64,
    ) {
        hub.set_state(ConnectionState::Connecting {
            target: addr.to_string(),
        });
        let on_disc = Self::make_disconnect_handler(hub, controller, config.once);
        let on_focus = Self::make_focus_handler(hub, controller);
        let on_spatial = Self::make_spatial_handler(hub);
        match PuckClient::connect_with_callbacks(addr, on_disc, Some(on_focus), Some(on_spatial)) {
            Ok(client) => {
                *backoff_ms = 500;
                hub.attach(Arc::new(client), addr.to_string(), TransportKind::DirectTcp);
                while hub.is_connected()
                    && !controller.should_exit()
                    && generation.load(Ordering::Acquire) == gen_now
                {
                    thread::sleep(Duration::from_millis(150));
                }
                controller.force_host_mode();
                hub.detach(ConnectionState::Searching);
                if config.once {
                    controller.request_exit();
                }
            }
            Err(e) => {
                if config.once {
                    hub.set_state(ConnectionState::Error { message: e });
                    controller.request_exit();
                    return;
                }
                if config.verbose {
                    push_log(format!("[connector] direct TCP attempt failed: {e}"));
                }
                hub.set_state(ConnectionState::Searching);
                Self::sleep_interruptible(
                    controller,
                    generation,
                    gen_now,
                    Duration::from_millis(*backoff_ms),
                );
                *backoff_ms = (*backoff_ms * 2).min(2000);
            }
        }
    }

    fn step_adb(
        shared_cfg: &Arc<RwLock<ConnectorConfig>>,
        config: &ConnectorConfig,
        generation: &AtomicU64,
        gen_now: u64,
        hub: &Arc<ConnectionHub>,
        controller: &Arc<SessionController>,
        last_error: &mut Option<String>,
    ) {
        let adb_bin = match AdbSession::find_adb(config.adb_path.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                if last_error.as_deref() != Some(&e) {
                    push_log(format!("[connector] {e}"));
                    *last_error = Some(e.clone());
                }
                if config.once {
                    hub.set_state(ConnectionState::Error { message: e });
                    controller.request_exit();
                    return;
                }
                hub.set_state(ConnectionState::Error { message: e });
                Self::sleep_interruptible(controller, generation, gen_now, Duration::from_secs(2));
                return;
            }
        };

        let mut devices = match AdbSession::query_devices(&adb_bin) {
            Ok(d) => d,
            Err(e) => {
                if last_error.as_deref() != Some(&e) {
                    push_log(format!("[connector] {e}"));
                    *last_error = Some(e.clone());
                }
                if config.once {
                    hub.set_state(ConnectionState::Error { message: e });
                    controller.request_exit();
                    return;
                }
                hub.set_state(ConnectionState::Searching);
                Self::sleep_interruptible(
                    controller,
                    generation,
                    gen_now,
                    Duration::from_millis(1000),
                );
                return;
            }
        };

        let mut selected = select_puck_device(
            &devices,
            config.serial.as_deref(),
            config.wireless.as_deref(),
        );

        if matches!(selected, Ok(None)) {
            if let Some(ref waddr) = config.wireless {
                let _ = AdbSession::connect_wireless(&adb_bin, waddr);
                if let Ok(refreshed) = AdbSession::query_devices(&adb_bin) {
                    devices = refreshed;
                    selected = select_puck_device(
                        &devices,
                        config.serial.as_deref(),
                        config.wireless.as_deref(),
                    );
                }
            }
        }

        let device = match selected {
            Ok(Some(dev)) => {
                *last_error = None;
                dev
            }
            Ok(None) => {
                if config.once {
                    hub.set_state(ConnectionState::Error {
                        message: "no authorized ADB device found".into(),
                    });
                    controller.request_exit();
                    return;
                }
                hub.set_state(ConnectionState::Searching);
                Self::sleep_interruptible(
                    controller,
                    generation,
                    gen_now,
                    Duration::from_millis(800),
                );
                return;
            }
            Err(e) => {
                if last_error.as_deref() != Some(&e) {
                    push_log(format!("[connector] {e}"));
                    *last_error = Some(e.clone());
                }
                if config.once {
                    hub.set_state(ConnectionState::Error { message: e });
                    controller.request_exit();
                    return;
                }
                hub.set_state(ConnectionState::Searching);
                Self::sleep_interruptible(
                    controller,
                    generation,
                    gen_now,
                    Duration::from_millis(1000),
                );
                return;
            }
        };

        hub.set_state(ConnectionState::Connecting {
            target: device.serial.clone(),
        });

        let adb_session = match AdbSession::setup_for_device(
            adb_bin,
            &device,
            config.agent_path.as_deref(),
            config.no_push,
            config.force_push,
            config.port,
            config.verbose,
        ) {
            Ok(s) => {
                if config.force_push {
                    if let Ok(mut g) = shared_cfg.write() {
                        g.force_push = false;
                    }
                }
                s
            }
            Err(e) => {
                push_log(format!(
                    "[connector] ADB setup failed for {}: {e}",
                    device.serial
                ));
                if config.once {
                    hub.set_state(ConnectionState::Error { message: e });
                    controller.request_exit();
                    return;
                }
                hub.set_state(ConnectionState::Searching);
                Self::sleep_interruptible(
                    controller,
                    generation,
                    gen_now,
                    Duration::from_millis(1200),
                );
                return;
            }
        };

        let target_addr = format!("127.0.0.1:{}", config.port);
        let mut connected_client = None;
        for attempt in 0..3 {
            if controller.should_exit() || generation.load(Ordering::Acquire) != gen_now {
                break;
            }
            let on_disc = Self::make_disconnect_handler(hub, controller, config.once);
            let on_focus = Self::make_focus_handler(hub, controller);
            let on_spatial = Self::make_spatial_handler(hub);
            match PuckClient::connect_with_callbacks(
                &target_addr,
                on_disc,
                Some(on_focus),
                Some(on_spatial),
            ) {
                Ok(c) => {
                    connected_client = Some(Arc::new(c));
                    break;
                }
                Err(e) => {
                    if attempt == 2 {
                        push_log(format!("[connector] TCP handshake failed: {e}"));
                    } else {
                        thread::sleep(Duration::from_millis(150));
                    }
                }
            }
        }

        if let Some(client) = connected_client {
            hub.attach(
                client,
                adb_session.serial().to_string(),
                adb_session.transport_kind(),
            );
            while hub.is_connected()
                && !controller.should_exit()
                && generation.load(Ordering::Acquire) == gen_now
            {
                thread::sleep(Duration::from_millis(150));
            }
            controller.force_host_mode();
            hub.detach(ConnectionState::Searching);
            drop(adb_session);
            if config.once {
                controller.request_exit();
            }
        } else {
            drop(adb_session);
            if config.once {
                hub.set_state(ConnectionState::Error {
                    message: "could not establish TCP session with aura-puck-agent".into(),
                });
                controller.request_exit();
                return;
            }
            hub.set_state(ConnectionState::Searching);
            Self::sleep_interruptible(
                controller,
                generation,
                gen_now,
                Duration::from_millis(1000),
            );
        }
    }
}
