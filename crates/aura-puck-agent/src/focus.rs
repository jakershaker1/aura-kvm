//! Android XR window focus watcher using `logcat -b events` (`input_focus`).

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub fn line_indicates_mirror_focus(line: &str, pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return false;
    }
    let low = line.to_ascii_lowercase();
    let pat = trimmed.to_ascii_lowercase();
    low.contains("focus entering") && low.contains(&pat)
}

pub struct FocusWatcher;

impl FocusWatcher {
    pub fn spawn(pattern: String, verbose: bool) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut child = match Command::new("logcat")
                .args([
                    "-b",
                    "events",
                    "-T",
                    "1",
                    "-v",
                    "brief",
                    "input_focus:I",
                    "*:S",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    if verbose {
                        eprintln!("[focus] logcat unavailable ({e}); focus auto-return disabled");
                    }
                    return;
                }
            };

            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                return;
            };

            let reader = BufReader::new(stdout);
            let mut last_sent: Option<Instant> = None;
            for line_res in reader.lines() {
                let Ok(line) = line_res else {
                    break;
                };
                if line_indicates_mirror_focus(&line, &pattern) {
                    let now = Instant::now();
                    if last_sent
                        .map(|t| now.duration_since(t) >= Duration::from_millis(250))
                        .unwrap_or(true)
                    {
                        last_sent = Some(now);
                        if verbose {
                            eprintln!("[focus] mirror window focused: {line}");
                        }
                        if tx.send(()).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_entering_wiredmirror_focus_only() {
        let enter_mirror =
            "I/input_focus( 1234): [Focus entering 9a1b2c com.google.android.apps.vr.wiredmirror/com.google.android.apps.vr.wiredmirror.MainActivity (server),reason=WindowManager]";
        let leave_mirror =
            "I/input_focus( 1234): [Focus leaving 9a1b2c com.google.android.apps.vr.wiredmirror/MainActivity (server),reason=WindowManager]";
        let enter_chrome =
            "I/input_focus( 1234): [Focus entering 8811 com.android.chrome/com.google.android.apps.chrome.Main (server)]";

        assert!(line_indicates_mirror_focus(enter_mirror, "wiredmirror"));
        assert!(!line_indicates_mirror_focus(leave_mirror, "wiredmirror"));
        assert!(!line_indicates_mirror_focus(enter_chrome, "wiredmirror"));
        assert!(!line_indicates_mirror_focus(enter_mirror, ""));
    }
}
