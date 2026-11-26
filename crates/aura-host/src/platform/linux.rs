//! Linux input capture backend using `/dev/input/event*` (`evdev`), working on both Wayland & X11.

use super::{InputBackend, Modifiers, PermissionStatus, SessionController};
use crate::keymap::linux_evdev_to_hid;
use crate::logbuf::push_log;
use crate::switch::{DisplayBounds, DisplayInfo};
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;

#[cfg(target_env = "musl")]
type IoctlReq = i32;
#[cfg(not(target_env = "musl"))]
type IoctlReq = std::ffi::c_ulong;

const O_NONBLOCK_LINUX: i32 = 0o4000;
const O_CLOEXEC_LINUX: i32 = 0o2000000;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;

const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_RIGHTALT: u16 = 100;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;

const POLLIN: i16 = 0x0001;

// _IOW('E', 0x90, int)
const EVIOCGRAB: IoctlReq = 0x4004_4590u32 as IoctlReq;
// _IOC(_IOC_READ, 'E', 0x06, 128)
const EVIOCGNAME_128: IoctlReq = 0x8080_4506u32 as IoctlReq;
// _IOC(_IOC_READ, 'E', 0x20, 8) -> EV_MAX bits
const EVIOCGBIT_0_8: IoctlReq = 0x8008_4520u32 as IoctlReq;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    tv_sec: isize,
    tv_usec: isize,
    type_: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

extern "C" {
    fn ioctl(fd: i32, request: IoctlReq, ...) -> i32;
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
}

struct EvdevDevice {
    file: File,
    name: String,
    grabbed: bool,
}

impl EvdevDevice {
    fn set_grab(&mut self, grab: bool) {
        if self.grabbed == grab {
            return;
        }
        let arg: i32 = if grab { 1 } else { 0 };
        let rc = unsafe { ioctl(self.file.as_raw_fd(), EVIOCGRAB, arg) };
        if rc == 0 {
            self.grabbed = grab;
        }
    }
}

impl Drop for EvdevDevice {
    fn drop(&mut self) {
        self.set_grab(false);
    }
}

fn discover_devices() -> Result<Vec<EvdevDevice>, String> {
    let entries = fs::read_dir("/dev/input")
        .map_err(|e| format!("failed to read /dev/input: {e}"))?;
    let mut devices = Vec::new();
    let mut denied_count = 0usize;

    for entry in entries.flatten() {
        let fname = entry.file_name();
        let name_str = fname.to_string_lossy();
        if !name_str.starts_with("event") {
            continue;
        }
        let path = entry.path();
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK_LINUX | O_CLOEXEC_LINUX)
            .open(&path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                denied_count += 1;
                continue;
            }
            Err(_) => continue,
        };

        let mut ev_bits = [0u8; 8];
        if unsafe { ioctl(file.as_raw_fd(), EVIOCGBIT_0_8, ev_bits.as_mut_ptr()) } < 0 {
            continue;
        }
        let has_key = (ev_bits[0] & (1 << EV_KEY)) != 0;
        let has_rel = (ev_bits[0] & (1 << EV_REL)) != 0;
        if !has_key && !has_rel {
            continue;
        }

        let mut name_buf = [0u8; 128];
        let dev_name = if unsafe { ioctl(file.as_raw_fd(), EVIOCGNAME_128, name_buf.as_mut_ptr()) } >= 0 {
            CStr::from_bytes_until_nul(&name_buf)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|_| name_str.into_owned())
        } else {
            name_str.into_owned()
        };

        if dev_name.starts_with("Aura KVM") {
            continue;
        }

        devices.push(EvdevDevice {
            file,
            name: dev_name,
            grabbed: false,
        });
    }

    if devices.is_empty() {
        if denied_count > 0 {
            return Err(
                "permission denied opening /dev/input/event*. Add your user to the `input` group:\n\
                 sudo usermod -aG input $USER  (then log out and back in)"
                    .into(),
            );
        }
        return Err("no keyboard or mouse evdev devices found in /dev/input/".into());
    }

    Ok(devices)
}

pub fn check_permissions() -> PermissionStatus {
    match discover_devices() {
        Ok(_) => PermissionStatus {
            ready: true,
            accessibility: true,
            input_monitoring: true,
            summary: "evdev (/dev/input/event*) readable".to_string(),
        },
        Err(msg) => PermissionStatus {
            ready: false,
            accessibility: false,
            input_monitoring: false,
            summary: msg,
        },
    }
}

pub struct LinuxEvdevBackend;

impl InputBackend for LinuxEvdevBackend {
    fn run(&mut self, controller: Arc<SessionController>) -> Result<(), String> {
        let mut devices = discover_devices()?;
        push_log(format!(
            "[linux-evdev] monitoring {} input device(s) (Wayland + X11 compatible)",
            devices.len()
        ));
        for d in &devices {
            eprintln!("  - {}", d.name);
        }
        push_log(
            "[linux-evdev] ready! Press Ctrl+Alt+X to toggle PC <-> XREAL Aura \
             (Ctrl+Alt+Shift+Esc for emergency release).",
        );

        let mut mods = Modifiers {
            ctrl: false,
            shift: false,
            alt: false,
            gui: false,
        };
        let mut was_xr = false;
        let bounds = DisplayBounds::default();
        let mut host_pos = (960.0_f64, 540.0_f64);
        let ev_size = std::mem::size_of::<InputEvent>();
        let mut raw_buf = vec![0u8; ev_size * 64];

        while !controller.should_exit() {
            let xr_now = controller.is_xr_active();
            if xr_now != was_xr {
                for d in &mut devices {
                    d.set_grab(xr_now);
                }
                was_xr = xr_now;
            }

            let mut pollfds: Vec<PollFd> = devices
                .iter()
                .map(|d| PollFd {
                    fd: d.file.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                })
                .collect();

            let rc = unsafe { poll(pollfds.as_mut_ptr(), pollfds.len(), 100) };
            if rc <= 0 {
                continue;
            }

            let mut dx_acc = 0i32;
            let mut dy_acc = 0i32;
            let mut wv_acc = 0i32;
            let mut wh_acc = 0i32;

            for (idx, pfd) in pollfds.iter().enumerate() {
                if (pfd.revents & POLLIN) == 0 {
                    continue;
                }
                loop {
                    match devices[idx].file.read(&mut raw_buf) {
                        Ok(n) if n >= ev_size => {
                            let count = n / ev_size;
                            for i in 0..count {
                                let ev: InputEvent = unsafe {
                                    std::ptr::read_unaligned(
                                        raw_buf.as_ptr().add(i * ev_size) as *const InputEvent
                                    )
                                };
                                match ev.type_ {
                                    EV_KEY => {
                                        if ev.value == 2 {
                                            // Key repeat is handled natively by Android HID.
                                            continue;
                                        }
                                        let pressed = ev.value != 0;
                                        match ev.code {
                                            KEY_LEFTCTRL | KEY_RIGHTCTRL => mods.ctrl = pressed,
                                            KEY_LEFTSHIFT | KEY_RIGHTSHIFT => mods.shift = pressed,
                                            KEY_LEFTALT | KEY_RIGHTALT => mods.alt = pressed,
                                            KEY_LEFTMETA | KEY_RIGHTMETA => mods.gui = pressed,
                                            _ => {}
                                        }
                                        match ev.code {
                                            BTN_LEFT => {
                                                controller.handle_mouse_button(0, pressed);
                                            }
                                            BTN_RIGHT => {
                                                controller.handle_mouse_button(1, pressed);
                                            }
                                            BTN_MIDDLE => {
                                                controller.handle_mouse_button(2, pressed);
                                            }
                                            BTN_SIDE => {
                                                controller.handle_mouse_button(3, pressed);
                                            }
                                            BTN_EXTRA => {
                                                controller.handle_mouse_button(4, pressed);
                                            }
                                            _ => {
                                                if let Some(hid) = linux_evdev_to_hid(ev.code) {
                                                    controller.handle_key(hid, pressed, mods);
                                                }
                                            }
                                        }
                                    }
                                    EV_REL => match ev.code {
                                        REL_X => dx_acc += ev.value,
                                        REL_Y => dy_acc += ev.value,
                                        REL_WHEEL => wv_acc += ev.value,
                                        REL_HWHEEL => wh_acc += ev.value,
                                        _ => {}
                                    },
                                    EV_SYN => {
                                        if dx_acc != 0 || dy_acc != 0 {
                                            if controller.is_xr_active() {
                                                controller.handle_mouse_move(
                                                    dx_acc as f64,
                                                    dy_acc as f64,
                                                );
                                            } else {
                                                host_pos.0 = (host_pos.0 + dx_acc as f64)
                                                    .clamp(bounds.min_x, bounds.max_x - 1.0);
                                                host_pos.1 = (host_pos.1 + dy_acc as f64)
                                                    .clamp(bounds.min_y, bounds.max_y - 1.0);
                                                controller.handle_host_cursor_sample(
                                                    host_pos.0,
                                                    host_pos.1,
                                                    dx_acc as f64,
                                                    dy_acc as f64,
                                                    bounds,
                                                );
                                            }
                                            dx_acc = 0;
                                            dy_acc = 0;
                                        }
                                        if wv_acc != 0 || wh_acc != 0 {
                                            controller.handle_scroll(
                                                wv_acc as f64,
                                                wh_acc as f64,
                                                true,
                                            );
                                            wv_acc = 0;
                                            wh_acc = 0;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Ok(_) => break,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
            }
        }

        for d in &mut devices {
            d.set_grab(false);
        }
        Ok(())
    }
}

pub fn list_displays() -> Vec<DisplayInfo> {
    vec![DisplayInfo {
        id: "primary".into(),
        label: "Primary Monitor".into(),
        is_primary: true,
        bounds: DisplayBounds::default(),
    }]
}
