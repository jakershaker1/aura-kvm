//! USB HID usage constants and report descriptors shared between host and puck agent.

// Modifier bitmask constants (USB HID Usage Page 0x07, usages 0xE0..=0xE7)
pub const MOD_LCTRL: u8 = 1 << 0;
pub const MOD_LSHIFT: u8 = 1 << 1;
pub const MOD_LALT: u8 = 1 << 2;
pub const MOD_LGUI: u8 = 1 << 3;
pub const MOD_RCTRL: u8 = 1 << 4;
pub const MOD_RSHIFT: u8 = 1 << 5;
pub const MOD_RALT: u8 = 1 << 6;
pub const MOD_RGUI: u8 = 1 << 7;

// Standard USB HID Usage Page 0x07 (Keyboard/Keypad) scan codes
pub const KEY_NONE: u8 = 0x00;
pub const KEY_A: u8 = 0x04;
pub const KEY_B: u8 = 0x05;
pub const KEY_C: u8 = 0x06;
pub const KEY_D: u8 = 0x07;
pub const KEY_E: u8 = 0x08;
pub const KEY_F: u8 = 0x09;
pub const KEY_G: u8 = 0x0A;
pub const KEY_H: u8 = 0x0B;
pub const KEY_I: u8 = 0x0C;
pub const KEY_J: u8 = 0x0D;
pub const KEY_K: u8 = 0x0E;
pub const KEY_L: u8 = 0x0F;
pub const KEY_M: u8 = 0x10;
pub const KEY_N: u8 = 0x11;
pub const KEY_O: u8 = 0x12;
pub const KEY_P: u8 = 0x13;
pub const KEY_Q: u8 = 0x14;
pub const KEY_R: u8 = 0x15;
pub const KEY_S: u8 = 0x16;
pub const KEY_T: u8 = 0x17;
pub const KEY_U: u8 = 0x18;
pub const KEY_V: u8 = 0x19;
pub const KEY_W: u8 = 0x1A;
pub const KEY_X: u8 = 0x1B;
pub const KEY_Y: u8 = 0x1C;
pub const KEY_Z: u8 = 0x1D;

pub const KEY_1: u8 = 0x1E;
pub const KEY_2: u8 = 0x1F;
pub const KEY_3: u8 = 0x20;
pub const KEY_4: u8 = 0x21;
pub const KEY_5: u8 = 0x22;
pub const KEY_6: u8 = 0x23;
pub const KEY_7: u8 = 0x24;
pub const KEY_8: u8 = 0x25;
pub const KEY_9: u8 = 0x26;
pub const KEY_0: u8 = 0x27;

pub const KEY_ENTER: u8 = 0x28;
pub const KEY_ESC: u8 = 0x29;
pub const KEY_BACKSPACE: u8 = 0x2A;
pub const KEY_TAB: u8 = 0x2B;
pub const KEY_SPACE: u8 = 0x2C;
pub const KEY_MINUS: u8 = 0x2D;
pub const KEY_EQUAL: u8 = 0x2E;
pub const KEY_LEFTBRACE: u8 = 0x2F;
pub const KEY_RIGHTBRACE: u8 = 0x30;
pub const KEY_BACKSLASH: u8 = 0x31;
pub const KEY_SEMICOLON: u8 = 0x33;
pub const KEY_APOSTROPHE: u8 = 0x34;
pub const KEY_GRAVE: u8 = 0x35;
pub const KEY_COMMA: u8 = 0x36;
pub const KEY_DOT: u8 = 0x37;
pub const KEY_SLASH: u8 = 0x38;
pub const KEY_CAPSLOCK: u8 = 0x39;

pub const KEY_F1: u8 = 0x3A;
pub const KEY_F2: u8 = 0x3B;
pub const KEY_F3: u8 = 0x3C;
pub const KEY_F4: u8 = 0x3D;
pub const KEY_F5: u8 = 0x3E;
pub const KEY_F6: u8 = 0x3F;
pub const KEY_F7: u8 = 0x40;
pub const KEY_F8: u8 = 0x41;
pub const KEY_F9: u8 = 0x42;
pub const KEY_F10: u8 = 0x43;
pub const KEY_F11: u8 = 0x44;
pub const KEY_F12: u8 = 0x45;

pub const KEY_SYSRQ: u8 = 0x46;
pub const KEY_SCROLLLOCK: u8 = 0x47;
pub const KEY_PAUSE: u8 = 0x48;
pub const KEY_INSERT: u8 = 0x49;
pub const KEY_HOME: u8 = 0x4A;
pub const KEY_PAGEUP: u8 = 0x4B;
pub const KEY_DELETE: u8 = 0x4C;
pub const KEY_END: u8 = 0x4D;
pub const KEY_PAGEDOWN: u8 = 0x4E;
pub const KEY_RIGHT: u8 = 0x4F;
pub const KEY_LEFT: u8 = 0x50;
pub const KEY_DOWN: u8 = 0x51;
pub const KEY_UP: u8 = 0x52;

pub const KEY_NUMLOCK: u8 = 0x53;
pub const KEY_KPSLASH: u8 = 0x54;
pub const KEY_KPASTERISK: u8 = 0x55;
pub const KEY_KPMINUS: u8 = 0x56;
pub const KEY_KPPLUS: u8 = 0x57;
pub const KEY_KPENTER: u8 = 0x58;
pub const KEY_KP1: u8 = 0x59;
pub const KEY_KP2: u8 = 0x5A;
pub const KEY_KP3: u8 = 0x5B;
pub const KEY_KP4: u8 = 0x5C;
pub const KEY_KP5: u8 = 0x5D;
pub const KEY_KP6: u8 = 0x5E;
pub const KEY_KP7: u8 = 0x5F;
pub const KEY_KP8: u8 = 0x60;
pub const KEY_KP9: u8 = 0x61;
pub const KEY_KP0: u8 = 0x62;
pub const KEY_KPDOT: u8 = 0x63;
pub const KEY_KPEQUAL: u8 = 0x67;
pub const KEY_COMPOSE: u8 = 0x65;

pub const KEY_F13: u8 = 0x68;
pub const KEY_F14: u8 = 0x69;
pub const KEY_F15: u8 = 0x6A;
pub const KEY_F16: u8 = 0x6B;
pub const KEY_F17: u8 = 0x6C;
pub const KEY_F18: u8 = 0x6D;
pub const KEY_F19: u8 = 0x6E;
pub const KEY_F20: u8 = 0x6F;

pub const KEY_LCTRL: u8 = 0xE0;
pub const KEY_LSHIFT: u8 = 0xE1;
pub const KEY_LALT: u8 = 0xE2;
pub const KEY_LGUI: u8 = 0xE3;
pub const KEY_RCTRL: u8 = 0xE4;
pub const KEY_RSHIFT: u8 = 0xE5;
pub const KEY_RALT: u8 = 0xE6;
pub const KEY_RGUI: u8 = 0xE7;

// Consumer Control Usage Page (0x0C) usages
pub const CONSUMER_NONE: u16 = 0x0000;
pub const CONSUMER_PLAY_PAUSE: u16 = 0x00CD;
pub const CONSUMER_MUTE: u16 = 0x00E2;
pub const CONSUMER_VOL_UP: u16 = 0x00E9;
pub const CONSUMER_VOL_DOWN: u16 = 0x00EA;
pub const CONSUMER_AC_HOME: u16 = 0x0223;
pub const CONSUMER_AC_BACK: u16 = 0x0224;

/// Map a modifier key usage (0xE0..=0xE7) to its modifier bitmask bit.
#[inline]
pub fn modifier_bit_for_hid(usage: u8) -> Option<u8> {
    match usage {
        KEY_LCTRL => Some(MOD_LCTRL),
        KEY_LSHIFT => Some(MOD_LSHIFT),
        KEY_LALT => Some(MOD_LALT),
        KEY_LGUI => Some(MOD_LGUI),
        KEY_RCTRL => Some(MOD_RCTRL),
        KEY_RSHIFT => Some(MOD_RSHIFT),
        KEY_RALT => Some(MOD_RALT),
        KEY_RGUI => Some(MOD_RGUI),
        _ => None,
    }
}

/// Composite HID Report Descriptor for Keyboard (Report ID 1) + Consumer Control (Report ID 2).
///
/// Report ID 1 payload (8 bytes): `[modifiers, reserved, k0, k1, k2, k3, k4, k5]`
/// Report ID 2 payload (2 bytes): `[usage_lo, usage_hi]`
pub const KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    // Keyboard Application Collection (Report ID 1)
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x06,       // Usage (Keyboard)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x01,       //   Report ID (1)
    // Modifiers (8 bits)
    0x05, 0x07,       //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0,       //   Usage Minimum (224 - Left Control)
    0x29, 0xE7,       //   Usage Maximum (231 - Right GUI)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x08,       //   Report Count (8)
    0x81, 0x02,       //   Input (Data, Variable, Absolute)
    // Reserved byte
    0x75, 0x08,       //   Report Size (8)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x01,       //   Input (Constant)
    // LEDs output report
    0x05, 0x08,       //   Usage Page (LEDs)
    0x19, 0x01,       //   Usage Minimum (1)
    0x29, 0x05,       //   Usage Maximum (5)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x05,       //   Report Count (5)
    0x91, 0x02,       //   Output (Data, Variable, Absolute)
    0x75, 0x03,       //   Report Size (3)
    0x95, 0x01,       //   Report Count (1)
    0x91, 0x01,       //   Output (Constant)
    // 6-key rollover array
    0x05, 0x07,       //   Usage Page (Keyboard/Keypad)
    0x19, 0x00,       //   Usage Minimum (0)
    0x29, 0xE7,       //   Usage Maximum (231)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xE7, 0x00, //   Logical Maximum (231)
    0x75, 0x08,       //   Report Size (8)
    0x95, 0x06,       //   Report Count (6)
    0x81, 0x00,       //   Input (Data, Array, Absolute)
    0xC0,             // End Collection

    // Consumer Control Application Collection (Report ID 2)
    0x05, 0x0C,       // Usage Page (Consumer)
    0x09, 0x01,       // Usage (Consumer Control)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x02,       //   Report ID (2)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (0x03FF)
    0x19, 0x00,       //   Usage Minimum (0)
    0x2A, 0xFF, 0x03, //   Usage Maximum (0x03FF)
    0x75, 0x10,       //   Report Size (16)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x00,       //   Input (Data, Array, Absolute)
    0xC0,             // End Collection
];

/// HID Report Descriptor for 5-button relative mouse with 16-bit X/Y deltas and 2D scroll.
///
/// Report payload (7 bytes, no Report ID):
/// `[buttons, dx_lo, dx_hi, dy_lo, dy_hi, wheel_v, wheel_h]`
pub const MOUSE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x02,       // Usage (Mouse)
    0xA1, 0x01,       // Collection (Application)
    0x09, 0x01,       //   Usage (Pointer)
    0xA1, 0x00,       //   Collection (Physical)
    // 5 Buttons
    0x05, 0x09,       //     Usage Page (Buttons)
    0x19, 0x01,       //     Usage Minimum (1)
    0x29, 0x05,       //     Usage Maximum (5)
    0x15, 0x00,       //     Logical Minimum (0)
    0x25, 0x01,       //     Logical Maximum (1)
    0x75, 0x01,       //     Report Size (1)
    0x95, 0x05,       //     Report Count (5)
    0x81, 0x02,       //     Input (Data, Variable, Absolute)
    // 3 bits padding
    0x75, 0x03,       //     Report Size (3)
    0x95, 0x01,       //     Report Count (1)
    0x81, 0x01,       //     Input (Constant)
    // X, Y (16-bit signed relative)
    0x05, 0x01,       //     Usage Page (Generic Desktop)
    0x09, 0x30,       //     Usage (X)
    0x09, 0x31,       //     Usage (Y)
    0x16, 0x01, 0x80, //     Logical Minimum (-32767)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x81, 0x06,       //     Input (Data, Variable, Relative)
    // Vertical Wheel (8-bit signed relative)
    0x09, 0x38,       //     Usage (Wheel)
    0x15, 0x81,       //     Logical Minimum (-127)
    0x25, 0x7F,       //     Logical Maximum (127)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x81, 0x06,       //     Input (Data, Variable, Relative)
    // Horizontal Wheel / AC Pan (8-bit signed relative)
    0x05, 0x0C,       //     Usage Page (Consumer)
    0x0A, 0x38, 0x02, //     Usage (AC Pan)
    0x15, 0x81,       //     Logical Minimum (-127)
    0x25, 0x7F,       //     Logical Maximum (127)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x81, 0x06,       //     Input (Data, Variable, Relative)
    0xC0,             //   End Collection (Physical)
    0xC0,             // End Collection (Application)
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk a HID descriptor item stream to verify tag/length integrity and balanced collections.
    fn validate_hid_descriptor(desc: &[u8]) -> Result<i32, String> {
        let mut i = 0;
        let mut depth = 0i32;
        while i < desc.len() {
            let prefix = desc[i];
            if prefix == 0xFE {
                return Err("unexpected long item".into());
            }
            let size_code = prefix & 0x03;
            let payload_len = match size_code {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 4,
                _ => unreachable!(),
            };
            if i + 1 + payload_len > desc.len() {
                return Err(format!("truncated item at offset {i}"));
            }
            let item_type = (prefix >> 2) & 0x03;
            let tag = (prefix >> 4) & 0x0F;
            if item_type == 0 {
                if tag == 0x0A {
                    depth += 1;
                } else if tag == 0x0C {
                    depth -= 1;
                    if depth < 0 {
                        return Err("unmatched End Collection".into());
                    }
                }
            }
            i += 1 + payload_len;
        }
        if depth != 0 {
            return Err(format!("unclosed collection depth {depth}"));
        }
        Ok(depth)
    }

    #[test]
    fn descriptors_are_well_formed() {
        assert_eq!(validate_hid_descriptor(KEYBOARD_REPORT_DESCRIPTOR), Ok(0));
        assert_eq!(validate_hid_descriptor(MOUSE_REPORT_DESCRIPTOR), Ok(0));
    }

    #[test]
    fn modifier_bits_match_hid_spec() {
        assert_eq!(modifier_bit_for_hid(KEY_LCTRL), Some(MOD_LCTRL));
        assert_eq!(modifier_bit_for_hid(KEY_RGUI), Some(MOD_RGUI));
        assert_eq!(modifier_bit_for_hid(KEY_A), None);
    }
}
