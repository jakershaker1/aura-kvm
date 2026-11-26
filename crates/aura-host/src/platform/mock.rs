//! Headless mock input backend for automated tests and `--mock-backend`.

use super::{InputBackend, Modifiers, SessionController};
use aura_protocol::hid::{KEY_A, KEY_X};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct MockBackend;

impl InputBackend for MockBackend {
    fn run(&mut self, controller: Arc<SessionController>) -> Result<(), String> {
        let toggle_mods = if cfg!(target_os = "macos") {
            Modifiers {
                ctrl: false,
                shift: false,
                alt: true,
                gui: true,
            }
        } else {
            Modifiers {
                ctrl: true,
                shift: false,
                alt: true,
                gui: false,
            }
        };
        let empty = Modifiers {
            ctrl: false,
            shift: false,
            alt: false,
            gui: false,
        };

        controller.handle_key(KEY_X, true, toggle_mods);
        controller.handle_key(KEY_A, true, empty);
        controller.handle_key(KEY_A, false, empty);
        controller.handle_mouse_move(12.0, -8.0);
        controller.handle_mouse_button(0, true);
        controller.handle_mouse_button(0, false);
        controller.handle_scroll(1.0, 0.0, true);
        thread::sleep(Duration::from_millis(100));
        controller.handle_key(KEY_X, true, toggle_mods);
        Ok(())
    }
}
