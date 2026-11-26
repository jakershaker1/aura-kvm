//! Thread-safe ring-buffer logger shared between `aura-host` and `aura-ui`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

const MAX_ENTRIES: usize = 200;

static START_TIME: OnceLock<Instant> = OnceLock::new();
static LOGS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static OBSERVER: OnceLock<RwLock<Option<Arc<dyn Fn() + Send + Sync>>>> = OnceLock::new();

fn start_time() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

fn logs_lock() -> &'static Mutex<VecDeque<String>> {
    LOGS.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)))
}

fn observer_lock() -> &'static RwLock<Option<Arc<dyn Fn() + Send + Sync>>> {
    OBSERVER.get_or_init(|| RwLock::new(None))
}

pub fn set_observer(cb: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut guard) = observer_lock().write() {
        *guard = Some(cb);
    }
}

pub fn push_log(msg: impl Into<String>) {
    let text = msg.into();
    eprintln!("{text}");

    let elapsed = start_time().elapsed();
    let secs = elapsed.as_secs();
    let mins = secs / 60;
    let rem_secs = secs % 60;
    let tenths = elapsed.subsec_millis() / 100;
    let formatted = format!("[{mins:02}:{rem_secs:02}.{tenths}] {text}");

    {
        let mut q = logs_lock().lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= MAX_ENTRIES {
            q.pop_front();
        }
        q.push_back(formatted);
    }

    if let Ok(obs) = observer_lock().read() {
        if let Some(ref cb) = *obs {
            cb();
        }
    }
}

pub fn snapshot() -> Vec<String> {
    logs_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

pub fn clear() {
    logs_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}
