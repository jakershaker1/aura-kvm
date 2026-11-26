//! Portable keycode translation tables (macOS, Linux evdev, Windows scancodes -> USB HID).

use aura_protocol::hid::*;

/// Translate macOS ANSI `CGKeyCode` (`u16`) to USB HID Usage Page 0x07 code.
pub fn macos_keycode_to_hid(code: u16) -> Option<u8> {
    Some(match code {
        0x00 => KEY_A,
        0x01 => KEY_S,
        0x02 => KEY_D,
        0x03 => KEY_F,
        0x04 => KEY_H,
        0x05 => KEY_G,
        0x06 => KEY_Z,
        0x07 => KEY_X,
        0x08 => KEY_C,
        0x09 => KEY_V,
        0x0B => KEY_B,
        0x0C => KEY_Q,
        0x0D => KEY_W,
        0x0E => KEY_E,
        0x0F => KEY_R,
        0x10 => KEY_Y,
        0x11 => KEY_T,
        0x12 => KEY_1,
        0x13 => KEY_2,
        0x14 => KEY_3,
        0x15 => KEY_4,
        0x16 => KEY_6,
        0x17 => KEY_5,
        0x18 => KEY_EQUAL,
        0x19 => KEY_9,
        0x1A => KEY_7,
        0x1B => KEY_MINUS,
        0x1C => KEY_8,
        0x1D => KEY_0,
        0x1E => KEY_RIGHTBRACE,
        0x1F => KEY_O,
        0x20 => KEY_U,
        0x21 => KEY_LEFTBRACE,
        0x22 => KEY_I,
        0x23 => KEY_P,
        0x24 => KEY_ENTER,
        0x25 => KEY_L,
        0x26 => KEY_J,
        0x27 => KEY_APOSTROPHE,
        0x28 => KEY_K,
        0x29 => KEY_SEMICOLON,
        0x2A => KEY_BACKSLASH,
        0x2B => KEY_COMMA,
        0x2C => KEY_SLASH,
        0x2D => KEY_N,
        0x2E => KEY_M,
        0x2F => KEY_DOT,
        0x30 => KEY_TAB,
        0x31 => KEY_SPACE,
        0x32 => KEY_GRAVE,
        0x33 => KEY_BACKSPACE,
        0x35 => KEY_ESC,
        0x36 => KEY_RGUI,
        0x37 => KEY_LGUI,
        0x38 => KEY_LSHIFT,
        0x39 => KEY_CAPSLOCK,
        0x3A => KEY_LALT,
        0x3B => KEY_LCTRL,
        0x3C => KEY_RSHIFT,
        0x3D => KEY_RALT,
        0x3E => KEY_RCTRL,
        0x41 => KEY_KPDOT,
        0x43 => KEY_KPASTERISK,
        0x45 => KEY_KPPLUS,
        0x47 => KEY_NUMLOCK,
        0x4B => KEY_KPSLASH,
        0x4C => KEY_KPENTER,
        0x4E => KEY_KPMINUS,
        0x51 => KEY_KPEQUAL,
        0x52 => KEY_KP0,
        0x53 => KEY_KP1,
        0x54 => KEY_KP2,
        0x55 => KEY_KP3,
        0x56 => KEY_KP4,
        0x57 => KEY_KP5,
        0x58 => KEY_KP6,
        0x59 => KEY_KP7,
        0x5B => KEY_KP8,
        0x5C => KEY_KP9,
        0x60 => KEY_F5,
        0x61 => KEY_F6,
        0x62 => KEY_F7,
        0x63 => KEY_F3,
        0x64 => KEY_F8,
        0x65 => KEY_F9,
        0x67 => KEY_F11,
        0x69 => KEY_F13,
        0x6B => KEY_F14,
        0x6D => KEY_F10,
        0x6F => KEY_F12,
        0x71 => KEY_F15,
        0x72 => KEY_INSERT,
        0x73 => KEY_HOME,
        0x74 => KEY_PAGEUP,
        0x75 => KEY_DELETE,
        0x76 => KEY_F4,
        0x77 => KEY_END,
        0x78 => KEY_F2,
        0x79 => KEY_PAGEDOWN,
        0x7A => KEY_F1,
        0x7B => KEY_LEFT,
        0x7C => KEY_RIGHT,
        0x7D => KEY_DOWN,
        0x7E => KEY_UP,
        _ => return None,
    })
}

/// Translate Linux `evdev` `KEY_*` code (`u16`) to USB HID Usage Page 0x07 code.
pub fn linux_evdev_to_hid(code: u16) -> Option<u8> {
    Some(match code {
        1 => KEY_ESC,
        2 => KEY_1,
        3 => KEY_2,
        4 => KEY_3,
        5 => KEY_4,
        6 => KEY_5,
        7 => KEY_6,
        8 => KEY_7,
        9 => KEY_8,
        10 => KEY_9,
        11 => KEY_0,
        12 => KEY_MINUS,
        13 => KEY_EQUAL,
        14 => KEY_BACKSPACE,
        15 => KEY_TAB,
        16 => KEY_Q,
        17 => KEY_W,
        18 => KEY_E,
        19 => KEY_R,
        20 => KEY_T,
        21 => KEY_Y,
        22 => KEY_U,
        23 => KEY_I,
        24 => KEY_O,
        25 => KEY_P,
        26 => KEY_LEFTBRACE,
        27 => KEY_RIGHTBRACE,
        28 => KEY_ENTER,
        29 => KEY_LCTRL,
        30 => KEY_A,
        31 => KEY_S,
        32 => KEY_D,
        33 => KEY_F,
        34 => KEY_G,
        35 => KEY_H,
        36 => KEY_J,
        37 => KEY_K,
        38 => KEY_L,
        39 => KEY_SEMICOLON,
        40 => KEY_APOSTROPHE,
        41 => KEY_GRAVE,
        42 => KEY_LSHIFT,
        43 => KEY_BACKSLASH,
        44 => KEY_Z,
        45 => KEY_X,
        46 => KEY_C,
        47 => KEY_V,
        48 => KEY_B,
        49 => KEY_N,
        50 => KEY_M,
        51 => KEY_COMMA,
        52 => KEY_DOT,
        53 => KEY_SLASH,
        54 => KEY_RSHIFT,
        55 => KEY_KPASTERISK,
        56 => KEY_LALT,
        57 => KEY_SPACE,
        58 => KEY_CAPSLOCK,
        59 => KEY_F1,
        60 => KEY_F2,
        61 => KEY_F3,
        62 => KEY_F4,
        63 => KEY_F5,
        64 => KEY_F6,
        65 => KEY_F7,
        66 => KEY_F8,
        67 => KEY_F9,
        68 => KEY_F10,
        69 => KEY_NUMLOCK,
        70 => KEY_SCROLLLOCK,
        71 => KEY_KP7,
        72 => KEY_KP8,
        73 => KEY_KP9,
        74 => KEY_KPMINUS,
        75 => KEY_KP4,
        76 => KEY_KP5,
        77 => KEY_KP6,
        78 => KEY_KPPLUS,
        79 => KEY_KP1,
        80 => KEY_KP2,
        81 => KEY_KP3,
        82 => KEY_KP0,
        83 => KEY_KPDOT,
        87 => KEY_F11,
        88 => KEY_F12,
        96 => KEY_KPENTER,
        97 => KEY_RCTRL,
        98 => KEY_KPSLASH,
        99 => KEY_SYSRQ,
        100 => KEY_RALT,
        102 => KEY_HOME,
        103 => KEY_UP,
        104 => KEY_PAGEUP,
        105 => KEY_LEFT,
        106 => KEY_RIGHT,
        107 => KEY_END,
        108 => KEY_DOWN,
        109 => KEY_PAGEDOWN,
        110 => KEY_INSERT,
        111 => KEY_DELETE,
        119 => KEY_PAUSE,
        125 => KEY_LGUI,
        126 => KEY_RGUI,
        127 => KEY_COMPOSE,
        _ => return None,
    })
}

/// Translate Windows PS/2 Set 1 `KBDLLHOOKSTRUCT::scanCode` + `LLKHF_EXTENDED` to USB HID.
pub fn windows_scancode_to_hid(scancode: u16, extended: bool) -> Option<u8> {
    if extended {
        return Some(match scancode {
            0x1C => KEY_KPENTER,
            0x1D => KEY_RCTRL,
            0x35 => KEY_KPSLASH,
            0x37 => KEY_SYSRQ,
            0x38 => KEY_RALT,
            0x47 => KEY_HOME,
            0x48 => KEY_UP,
            0x49 => KEY_PAGEUP,
            0x4B => KEY_LEFT,
            0x4D => KEY_RIGHT,
            0x4F => KEY_END,
            0x50 => KEY_DOWN,
            0x51 => KEY_PAGEDOWN,
            0x52 => KEY_INSERT,
            0x53 => KEY_DELETE,
            0x5B => KEY_LGUI,
            0x5C => KEY_RGUI,
            0x5D => KEY_COMPOSE,
            _ => return None,
        });
    }

    // Standard non-extended Set 1 scancodes 0x01..=0x58 align directly with evdev 1..=88.
    if (0x01..=0x58).contains(&scancode) {
        linux_evdev_to_hid(scancode)
    } else {
        None
    }
}

/// Optional macOS-friendly shortcut remapping: maps `Cmd + {C,V,X,Z,A,F,T,W,R,L}` -> `Ctrl + ...`.
pub fn remap_mac_shortcuts(mut report: [u8; 8]) -> [u8; 8] {
    let gui_mask = MOD_LGUI | MOD_RGUI;
    let other_mods = MOD_LCTRL | MOD_RCTRL | MOD_LALT | MOD_RALT;
    if (report[0] & gui_mask) != 0 && (report[0] & other_mods) == 0 {
        let has_shortcut_key = report[2..].iter().any(|&k| {
            matches!(
                k,
                KEY_C | KEY_V | KEY_X | KEY_Z | KEY_A | KEY_F | KEY_T | KEY_W | KEY_R | KEY_L
            )
        });
        if has_shortcut_key {
            report[0] = (report[0] & !gui_mask) | MOD_LCTRL;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymaps_cover_core_keys() {
        assert_eq!(macos_keycode_to_hid(0x00), Some(KEY_A));
        assert_eq!(macos_keycode_to_hid(0x37), Some(KEY_LGUI));
        assert_eq!(macos_keycode_to_hid(0x7E), Some(KEY_UP));

        assert_eq!(linux_evdev_to_hid(30), Some(KEY_A));
        assert_eq!(linux_evdev_to_hid(125), Some(KEY_LGUI));
        assert_eq!(linux_evdev_to_hid(103), Some(KEY_UP));

        assert_eq!(windows_scancode_to_hid(0x1E, false), Some(KEY_A));
        assert_eq!(windows_scancode_to_hid(0x5B, true), Some(KEY_LGUI));
        assert_eq!(windows_scancode_to_hid(0x48, true), Some(KEY_UP));
    }

    #[test]
    fn mac_shortcut_translation_remaps_cmd_copy_paste() {
        let cmd_c = [MOD_LGUI, 0, KEY_C, 0, 0, 0, 0, 0];
        assert_eq!(
            remap_mac_shortcuts(cmd_c),
            [MOD_LCTRL, 0, KEY_C, 0, 0, 0, 0, 0]
        );

        // Leaves Option+Cmd+X alone (e.g., non-shortcut or multi-modifier)
        let opt_cmd_c = [MOD_LGUI | MOD_LALT, 0, KEY_C, 0, 0, 0, 0, 0];
        assert_eq!(remap_mac_shortcuts(opt_cmd_c), opt_cmd_c);
    }
}
