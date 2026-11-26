//! Stateful 6KRO keyboard and mouse button state tracker.

use crate::hid::{modifier_bit_for_hid, KEY_NONE};

/// Tracks pressed modifiers, up to 6 active non-modifier keys, and 5 mouse buttons.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HidState {
    modifiers: u8,
    keys: [u8; 6],
    mouse_buttons: u8,
}

impl HidState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update keyboard state for a key press or release.
    /// Returns `Some([u8; 8])` when the report changed and should be transmitted.
    pub fn set_key(&mut self, hid_usage: u8, pressed: bool) -> Option<[u8; 8]> {
        if hid_usage == KEY_NONE {
            return None;
        }
        let prev = self.keyboard_report();

        if let Some(bit) = modifier_bit_for_hid(hid_usage) {
            if pressed {
                self.modifiers |= bit;
            } else {
                self.modifiers &= !bit;
            }
        } else if pressed {
            if !self.keys.contains(&hid_usage) {
                if let Some(slot) = self.keys.iter_mut().find(|k| **k == KEY_NONE) {
                    *slot = hid_usage;
                } else {
                    // 6-key rollover: replace oldest slot (index 0) and shift left
                    self.keys.rotate_left(1);
                    self.keys[5] = hid_usage;
                }
            }
        } else {
            for slot in &mut self.keys {
                if *slot == hid_usage {
                    *slot = KEY_NONE;
                }
            }
            // Compact non-zero keys to the front for clean boot-protocol reports
            let mut compacted = [KEY_NONE; 6];
            let mut idx = 0;
            for k in self.keys {
                if k != KEY_NONE {
                    compacted[idx] = k;
                    idx += 1;
                }
            }
            self.keys = compacted;
        }

        let next = self.keyboard_report();
        if next != prev {
            Some(next)
        } else {
            None
        }
    }

    /// Update mouse button state (`button_index` in 0..5: Left=0, Right=1, Middle=2, Back=3, Forward=4).
    /// Returns `true` if the button mask changed.
    pub fn set_mouse_button(&mut self, button_index: u8, pressed: bool) -> bool {
        if button_index >= 5 {
            return false;
        }
        let mask = 1u8 << button_index;
        let prev = self.mouse_buttons;
        if pressed {
            self.mouse_buttons |= mask;
        } else {
            self.mouse_buttons &= !mask;
        }
        self.mouse_buttons != prev
    }

    #[inline]
    pub fn modifiers(&self) -> u8 {
        self.modifiers
    }

    #[inline]
    pub fn mouse_buttons(&self) -> u8 {
        self.mouse_buttons
    }

    #[inline]
    pub fn keyboard_report(&self) -> [u8; 8] {
        [
            self.modifiers,
            0,
            self.keys[0],
            self.keys[1],
            self.keys[2],
            self.keys[3],
            self.keys[4],
            self.keys[5],
        ]
    }

    /// Clears all pressed modifiers, keys, and mouse buttons, returning the zeroed keyboard report.
    pub fn release_all(&mut self) -> [u8; 8] {
        self.modifiers = 0;
        self.keys = [KEY_NONE; 6];
        self.mouse_buttons = 0;
        [0u8; 8]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::*;

    #[test]
    fn key_press_dedup_and_compaction() {
        let mut state = HidState::new();
        assert_eq!(
            state.set_key(KEY_LCTRL, true),
            Some([MOD_LCTRL, 0, 0, 0, 0, 0, 0, 0])
        );
        // Duplicate press emits nothing
        assert_eq!(state.set_key(KEY_LCTRL, true), None);

        assert_eq!(
            state.set_key(KEY_A, true),
            Some([MOD_LCTRL, 0, KEY_A, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            state.set_key(KEY_B, true),
            Some([MOD_LCTRL, 0, KEY_A, KEY_B, 0, 0, 0, 0])
        );
        // Release A -> B compacts to slot 0
        assert_eq!(
            state.set_key(KEY_A, false),
            Some([MOD_LCTRL, 0, KEY_B, 0, 0, 0, 0, 0])
        );
    }

    #[test]
    fn six_key_rollover_and_release_all() {
        let mut state = HidState::new();
        for k in [KEY_A, KEY_B, KEY_C, KEY_D, KEY_E, KEY_F] {
            assert!(state.set_key(k, true).is_some());
        }
        // 7th key rolls over cleanly without dropping new input
        let report = state.set_key(KEY_G, true).unwrap();
        assert_eq!(&report[2..], &[KEY_B, KEY_C, KEY_D, KEY_E, KEY_F, KEY_G]);

        assert!(state.set_mouse_button(0, true));
        assert!(!state.set_mouse_button(0, true));
        assert_eq!(state.mouse_buttons(), 0b0000_0001);

        assert_eq!(state.release_all(), [0u8; 8]);
        assert_eq!(state.mouse_buttons(), 0);
        assert_eq!(state.modifiers(), 0);
    }
}
