//! Non-blocking TCP client, heartbeat watchdog, and connection hub for `aura-puck-agent`.

use crate::logbuf::push_log;
use aura_protocol::{read_frame, write_frame, Message, SpatialPose, PROTOCOL_VERSION};
use std::io::BufWriter;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    UsbAdb,
    WirelessAdb,
    DirectTcp,
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportKind::UsbAdb => write!(f, "USB ADB"),
            TransportKind::WirelessAdb => write!(f, "Wireless ADB"),
            TransportKind::DirectTcp => write!(f, "Direct TCP"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Searching,
    Connecting {
        target: String,
    },
    Connected {
        target: String,
        transport: TransportKind,
    },
    Error {
        message: String,
    },
}

type StateObserver = Arc<dyn Fn(&ConnectionState) + Send + Sync>;
type FocusObserver = Arc<dyn Fn() + Send + Sync>;
type SpatialObserver = Arc<dyn Fn(SpatialPose) + Send + Sync>;

pub struct ConnectionHub {
    connected: AtomicBool,
    active_client: RwLock<Option<Arc<PuckClient>>>,
    state: RwLock<ConnectionState>,
    latest_spatial: RwLock<Option<(SpatialPose, Instant)>>,
    on_state_change: RwLock<Option<StateObserver>>,
    on_focus_return: RwLock<Option<FocusObserver>>,
}

impl ConnectionHub {
    pub fn new() -> Self {
        Self {
            connected: AtomicBool::new(false),
            active_client: RwLock::new(None),
            state: RwLock::new(ConnectionState::Searching),
            latest_spatial: RwLock::new(None),
            on_state_change: RwLock::new(None),
            on_focus_return: RwLock::new(None),
        }
    }

    pub fn update_spatial_pose(&self, pose: SpatialPose) {
        let mut guard = self
            .latest_spatial
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some((pose, Instant::now()));
    }

    pub fn latest_spatial_pose(&self) -> Option<(SpatialPose, Duration)> {
        if !self.is_connected() {
            return None;
        }
        let guard = self
            .latest_spatial
            .read()
            .unwrap_or_else(|e| e.into_inner());
        guard.map(|(pose, at)| (pose, at.elapsed()))
    }

    pub fn set_state_observer(&self, cb: StateObserver) {
        if let Ok(mut guard) = self.on_state_change.write() {
            *guard = Some(cb);
        }
    }

    pub fn set_focus_return_observer(&self, cb: FocusObserver) {
        if let Ok(mut guard) = self.on_focus_return.write() {
            *guard = Some(cb);
        }
    }

    pub fn notify_focus_return(&self) {
        if let Ok(guard) = self.on_focus_return.read() {
            if let Some(ref cb) = *guard {
                cb();
            }
        }
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn state(&self) -> ConnectionState {
        self.state
            .read()
            .map(|g| g.clone())
            .unwrap_or(ConnectionState::Searching)
    }

    pub fn last_rtt_ms(&self) -> Option<f32> {
        if !self.is_connected() {
            return None;
        }
        let guard = self.active_client.read().ok()?;
        guard.as_ref().and_then(|c| c.last_rtt_ms())
    }

    pub fn capabilities(&self) -> u16 {
        if !self.is_connected() {
            return 0;
        }
        self.active_client
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|c| c.capabilities()))
            .unwrap_or(0)
    }

    pub fn set_state(&self, next: ConnectionState) {
        let changed = {
            let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
            if *guard == next {
                false
            } else {
                *guard = next.clone();
                true
            }
        };
        if changed {
            match &next {
                ConnectionState::Searching => {
                    push_log("[aura-kvm] state: searching for XREAL Aura puck...");
                }
                ConnectionState::Connecting { target } => {
                    push_log(format!("[aura-kvm] state: connecting to {target}..."));
                }
                ConnectionState::Connected { target, transport } => {
                    push_log(format!(
                        "[aura-kvm] state: connected to {target} ({transport})"
                    ));
                }
                ConnectionState::Error { message } => {
                    push_log(format!("[aura-kvm] state: error: {message}"));
                }
            }
            if let Ok(obs) = self.on_state_change.read() {
                if let Some(ref cb) = *obs {
                    cb(&next);
                }
            }
        }
    }

    pub fn attach(&self, client: Arc<PuckClient>, target: String, transport: TransportKind) {
        let old_client = {
            let mut guard = self
                .active_client
                .write()
                .unwrap_or_else(|e| e.into_inner());
            guard.replace(client)
        };
        drop(old_client);
        self.connected.store(true, Ordering::Release);
        self.set_state(ConnectionState::Connected { target, transport });
    }

    pub fn detach(&self, next_state: ConnectionState) {
        self.connected.store(false, Ordering::Release);
        {
            let mut spatial = self
                .latest_spatial
                .write()
                .unwrap_or_else(|e| e.into_inner());
            *spatial = None;
        }
        let old_client = {
            let mut guard = self
                .active_client
                .write()
                .unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        drop(old_client);
        self.set_state(next_state);
    }

    #[inline]
    pub fn send(&self, msg: Message) {
        if !self.is_connected() {
            return;
        }
        if let Ok(guard) = self.active_client.read() {
            if let Some(ref client) = *guard {
                client.send(msg);
            }
        }
    }
}

impl Default for ConnectionHub {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PuckClient {
    tx: Mutex<Option<Sender<Message>>>,
    shutdown_stream: Mutex<Option<TcpStream>>,
    connected: Arc<AtomicBool>,
    capabilities: AtomicU16,
    last_pong_seq: Arc<AtomicU64>,
    last_rtt_us: Arc<AtomicU64>,
    writer_handle: Option<thread::JoinHandle<()>>,
    reader_handle: Option<thread::JoinHandle<()>>,
    heartbeat_handle: Option<thread::JoinHandle<()>>,
}

impl PuckClient {
    pub fn connect(addr: &str, on_disconnect: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        Self::connect_with_callbacks(addr, on_disconnect, None, None)
    }

    pub fn connect_with_focus(
        addr: &str,
        on_disconnect: Arc<dyn Fn() + Send + Sync>,
        on_focus_return: Option<FocusObserver>,
    ) -> Result<Self, String> {
        Self::connect_with_callbacks(addr, on_disconnect, on_focus_return, None)
    }

    pub fn connect_with_callbacks(
        addr: &str,
        on_disconnect: Arc<dyn Fn() + Send + Sync>,
        on_focus_return: Option<FocusObserver>,
        on_spatial_pose: Option<SpatialObserver>,
    ) -> Result<Self, String> {
        let mut stream =
            TcpStream::connect(addr).map_err(|e| format!("failed to connect to {addr}: {e}"))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));

        write_frame(
            &mut stream,
            &Message::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .map_err(|e| format!("failed to send Hello: {e}"))?;

        let capabilities = match read_frame(&mut stream)
            .map_err(|e| format!("failed to read HelloAck: {e}"))?
        {
            Message::HelloAck {
                version,
                capabilities,
            } if version == PROTOCOL_VERSION => capabilities,
            other => return Err(format!("unexpected handshake response: {other:?}")),
        };

        let reader_stream = stream
            .try_clone()
            .map_err(|e| format!("failed to clone TCP stream: {e}"))?;
        let shutdown_clone = stream.try_clone().ok();

        let connected = Arc::new(AtomicBool::new(true));
        let last_pong_seq = Arc::new(AtomicU64::new(0));
        let last_rtt_us = Arc::new(AtomicU64::new(u64::MAX));
        let last_ping_sent: Arc<Mutex<Option<(u64, Instant)>>> = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<Message>();

        let writer_connected = Arc::clone(&connected);
        let writer_cb = Arc::clone(&on_disconnect);
        let writer_handle = thread::spawn(move || {
            let mut writer = BufWriter::new(stream);
            while let Ok(msg) = rx.recv() {
                if write_frame(&mut writer, &msg).is_err()
                    || std::io::Write::flush(&mut writer).is_err()
                {
                    if writer_connected.swap(false, Ordering::SeqCst) {
                        writer_cb();
                    }
                    break;
                }
            }
            if writer_connected.swap(false, Ordering::SeqCst) {
                writer_cb();
            }
        });

        let reader_connected = Arc::clone(&connected);
        let reader_pong = Arc::clone(&last_pong_seq);
        let reader_rtt = Arc::clone(&last_rtt_us);
        let reader_ping_sent = Arc::clone(&last_ping_sent);
        let reader_cb = Arc::clone(&on_disconnect);
        let reader_focus_cb = on_focus_return;
        let reader_spatial_cb = on_spatial_pose;
        let reader_handle = thread::spawn(move || {
            let mut r = reader_stream;
            while reader_connected.load(Ordering::Relaxed) {
                match read_frame(&mut r) {
                    Ok(Message::Pong(seq)) => {
                        reader_pong.store(seq, Ordering::Relaxed);
                        if let Ok(guard) = reader_ping_sent.lock() {
                            if let Some((sent_seq, sent_at)) = *guard {
                                if sent_seq == seq {
                                    reader_rtt.store(
                                        sent_at.elapsed().as_micros() as u64,
                                        Ordering::Relaxed,
                                    );
                                }
                            }
                        }
                    }
                    Ok(Message::FocusReturn) => {
                        if let Some(ref cb) = reader_focus_cb {
                            cb();
                        }
                    }
                    Ok(Message::SpatialPose(pose)) => {
                        if let Some(ref cb) = reader_spatial_cb {
                            cb(pose);
                        }
                    }
                    Ok(_) => {}
                    Err(aura_protocol::WireError::Io(ref e))
                        if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        continue;
                    }
                    Err(_) => {
                        if reader_connected.swap(false, Ordering::SeqCst) {
                            reader_cb();
                        }
                        break;
                    }
                }
            }
        });

        let hb_connected = Arc::clone(&connected);
        let hb_pong = Arc::clone(&last_pong_seq);
        let hb_ping_sent = Arc::clone(&last_ping_sent);
        let hb_tx = tx.clone();
        let hb_cb = Arc::clone(&on_disconnect);
        let heartbeat_handle = thread::spawn(move || {
            let mut seq = 1u64;
            while hb_connected.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(500));
                if !hb_connected.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(mut guard) = hb_ping_sent.lock() {
                    *guard = Some((seq, Instant::now()));
                }
                if hb_tx.send(Message::Ping(seq)).is_err() {
                    if hb_connected.swap(false, Ordering::SeqCst) {
                        hb_cb();
                    }
                    break;
                }
                let acked = hb_pong.load(Ordering::Relaxed);
                if seq > 4 && seq.saturating_sub(acked) >= 4 {
                    push_log("[client] heartbeat lost (>2s); releasing XR capture");
                    if hb_connected.swap(false, Ordering::SeqCst) {
                        hb_cb();
                    }
                    break;
                }
                seq += 1;
            }
        });

        Ok(Self {
            tx: Mutex::new(Some(tx)),
            shutdown_stream: Mutex::new(shutdown_clone),
            connected,
            capabilities: AtomicU16::new(capabilities),
            last_pong_seq,
            last_rtt_us,
            writer_handle: Some(writer_handle),
            reader_handle: Some(reader_handle),
            heartbeat_handle: Some(heartbeat_handle),
        })
    }

    pub fn capabilities(&self) -> u16 {
        self.capabilities.load(Ordering::Relaxed)
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn send(&self, msg: Message) {
        if !self.is_connected() {
            return;
        }
        if let Ok(guard) = self.tx.lock() {
            if let Some(ref sender) = *guard {
                let _ = sender.send(msg);
            }
        }
    }

    pub fn last_pong_seq(&self) -> u64 {
        self.last_pong_seq.load(Ordering::Relaxed)
    }

    pub fn last_rtt_ms(&self) -> Option<f32> {
        let us = self.last_rtt_us.load(Ordering::Relaxed);
        if us == u64::MAX {
            None
        } else {
            Some(us as f32 / 1000.0)
        }
    }
}

impl Drop for PuckClient {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.tx.lock() {
            if let Some(sender) = guard.take() {
                let _ = sender.send(Message::ReleaseAll);
            }
        }
        self.connected.store(false, Ordering::SeqCst);
        if let Ok(mut sock) = self.shutdown_stream.lock() {
            if let Some(s) = sock.take() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
        let cur = thread::current().id();
        if let Some(h) = self.writer_handle.take() {
            if h.thread().id() != cur {
                let _ = h.join();
            }
        }
        if let Some(h) = self.reader_handle.take() {
            if h.thread().id() != cur {
                let _ = h.join();
            }
        }
        if let Some(h) = self.heartbeat_handle.take() {
            drop(h);
        }
    }
}
