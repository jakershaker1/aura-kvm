//! macOS input capture backend using CoreGraphics `CGEventTap`.

use super::{InputBackend, Modifiers, PermissionStatus, SessionController};
use crate::keymap::macos_keycode_to_hid;
use crate::logbuf::push_log;
use crate::switch::{select_target_display, DisplayBounds, DisplayInfo};
use aura_protocol::hid::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type CGEventTapProxy = *mut c_void;
type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFStringRef = *const c_void;

const K_CG_HID_EVENT_TAP: u32 = 0;
const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_KEY_UP: u32 = 11;
const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
const K_CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
const K_CG_EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;
const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const K_CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;
const K_CG_MOUSE_EVENT_DELTA_X: u32 = 4;
const K_CG_MOUSE_EVENT_DELTA_Y: u32 = 5;
const K_CG_KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;
const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1: u32 = 11;
const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2: u32 = 12;
const K_CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS: u32 = 88;
const K_CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1: u32 = 96;
const K_CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2: u32 = 97;

const K_CG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
const K_CG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
const K_CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;
const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;

const NX_DEVICELCTLKEYMASK: u64 = 0x0000_0001;
const NX_DEVICELSHIFTKEYMASK: u64 = 0x0000_0002;
const NX_DEVICERSHIFTKEYMASK: u64 = 0x0000_0004;
const NX_DEVICELCMDKEYMASK: u64 = 0x0000_0008;
const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;
const NX_DEVICELALTKEYMASK: u64 = 0x0000_0020;
const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;
const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;
const K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT: u64 = 0x0001_0000;

type CGEventTapCallBack = extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: u8) -> i32;
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;
    static kCFRunLoopDefaultMode: CFStringRef;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(
        mode: CFStringRef,
        seconds: f64,
        return_after_source_handled: u8,
    ) -> i32;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
}

struct TapContext {
    controller: Arc<SessionController>,
    tap_port: CFMachPortRef,
    bounds: Mutex<Option<DisplayBounds>>,
    cursor_guard: Arc<CursorGuard>,
}

pub fn list_displays() -> Vec<DisplayInfo> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    let rc = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if rc != 0 || count == 0 {
        return Vec::new();
    }
    let main_id = unsafe { CGMainDisplayID() };
    let mut out = Vec::new();
    for (idx, &id) in ids[..(count as usize).min(ids.len())].iter().enumerate() {
        let rect = unsafe { CGDisplayBounds(id) };
        if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
            continue;
        }
        let is_primary = id == main_id;
        let is_builtin = unsafe { CGDisplayIsBuiltin(id) } != 0;
        let kind = if is_builtin { "Built-in" } else { "External" };
        let primary_tag = if is_primary { ", Primary" } else { "" };
        out.push(DisplayInfo {
            id: id.to_string(),
            label: format!(
                "Display {} — {}{} ({}×{})",
                idx + 1,
                kind,
                primary_tag,
                rect.size.width.round() as i32,
                rect.size.height.round() as i32,
            ),
            is_primary,
            bounds: DisplayBounds {
                min_x: rect.origin.x,
                min_y: rect.origin.y,
                max_x: rect.origin.x + rect.size.width,
                max_y: rect.origin.y + rect.size.height,
            },
        });
    }
    out
}

fn primary_display_bounds() -> DisplayBounds {
    let rect = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    if rect.size.width > 0.0 && rect.size.height > 0.0 {
        DisplayBounds {
            min_x: rect.origin.x,
            min_y: rect.origin.y,
            max_x: rect.origin.x + rect.size.width,
            max_y: rect.origin.y + rect.size.height,
        }
    } else {
        DisplayBounds::default()
    }
}

fn refresh_target_display_bounds(monitor_id: &str) -> Option<DisplayBounds> {
    let displays = list_displays();
    if displays.is_empty() && monitor_id.trim().is_empty() {
        return Some(primary_display_bounds());
    }
    select_target_display(&displays, monitor_id).map(|d| d.bounds)
}

struct CursorGuard {
    captured: AtomicBool,
    locked_pos: Mutex<Option<CGPoint>>,
}

impl CursorGuard {
    fn new() -> Self {
        Self {
            captured: AtomicBool::new(false),
            locked_pos: Mutex::new(None),
        }
    }

    fn sync(&self, want_captured: bool, anchor: Option<CGPoint>) {
        let was = self.captured.swap(want_captured, Ordering::SeqCst);
        if was == want_captured {
            return;
        }
        unsafe {
            let main = CGMainDisplayID();
            if want_captured {
                if let Ok(mut lp) = self.locked_pos.lock() {
                    *lp = anchor;
                }
                CGAssociateMouseAndMouseCursorPosition(0);
                CGDisplayHideCursor(main);
                if let Some(pt) = anchor {
                    CGWarpMouseCursorPosition(pt);
                }
            } else {
                if let Ok(mut lp) = self.locked_pos.lock() {
                    *lp = None;
                }
                CGDisplayShowCursor(main);
                CGAssociateMouseAndMouseCursorPosition(1);
            }
        }
    }

    fn enforce_lock(&self, current_loc: CGPoint) {
        if !self.captured.load(Ordering::Acquire) {
            return;
        }
        let target = {
            let mut lp = self.locked_pos.lock().unwrap_or_else(|e| e.into_inner());
            if lp.is_none() {
                *lp = Some(current_loc);
                return;
            }
            *lp
        };
        if let Some(pt) = target {
            if (current_loc.x - pt.x).hypot(current_loc.y - pt.y) > 1.0 {
                unsafe {
                    CGAssociateMouseAndMouseCursorPosition(0);
                    CGWarpMouseCursorPosition(pt);
                }
            }
        }
    }

    fn warp_to(&self, pt: CGPoint) {
        unsafe {
            CGWarpMouseCursorPosition(pt);
        }
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        self.sync(false, None);
    }
}

#[inline]
fn flags_to_modifiers(flags: u64) -> Modifiers {
    Modifiers {
        ctrl: (flags & K_CG_EVENT_FLAG_MASK_CONTROL) != 0,
        shift: (flags & K_CG_EVENT_FLAG_MASK_SHIFT) != 0,
        alt: (flags & K_CG_EVENT_FLAG_MASK_ALTERNATE) != 0,
        gui: (flags & K_CG_EVENT_FLAG_MASK_COMMAND) != 0,
    }
}

#[inline]
fn modifier_pressed_from_flags(hid_usage: u8, flags: u64) -> Option<bool> {
    Some(match hid_usage {
        KEY_LCTRL => (flags & NX_DEVICELCTLKEYMASK) != 0,
        KEY_RCTRL => (flags & NX_DEVICERCTLKEYMASK) != 0,
        KEY_LSHIFT => (flags & NX_DEVICELSHIFTKEYMASK) != 0,
        KEY_RSHIFT => (flags & NX_DEVICERSHIFTKEYMASK) != 0,
        KEY_LALT => (flags & NX_DEVICELALTKEYMASK) != 0,
        KEY_RALT => (flags & NX_DEVICERALTKEYMASK) != 0,
        KEY_LGUI => (flags & NX_DEVICELCMDKEYMASK) != 0,
        KEY_RGUI => (flags & NX_DEVICERCMDKEYMASK) != 0,
        KEY_CAPSLOCK => (flags & K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT) != 0,
        _ => return None,
    })
}

extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let ctx = unsafe { &*(user_info as *const TapContext) };

    if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        if !ctx.tap_port.is_null() {
            unsafe { CGEventTapEnable(ctx.tap_port, true) };
        }
        return event;
    }

    let consumed = match event_type {
        K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP => {
            if event_type == K_CG_EVENT_KEY_DOWN
                && unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_AUTOREPEAT) }
                    != 0
            {
                return if ctx.controller.is_xr_active() {
                    std::ptr::null_mut()
                } else {
                    event
                };
            }
            let keycode =
                unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) } as u16;
            let flags = unsafe { CGEventGetFlags(event) };
            let pressed = event_type == K_CG_EVENT_KEY_DOWN;
            let mods = flags_to_modifiers(flags);
            let hid = macos_keycode_to_hid(keycode);
            let consumed = if let Some(hid) = hid {
                ctx.controller.handle_key(hid, pressed, mods)
            } else {
                ctx.controller.is_xr_active()
            };
            if ctx.controller.is_verbose() {
                eprintln!(
                    "[macos-tap] type={event_type} keycode=0x{keycode:02x} hid={hid:?} \
                     mods={mods:?} xr_active={} consumed={consumed}",
                    ctx.controller.is_xr_active()
                );
            }
            if consumed {
                let loc = unsafe { CGEventGetLocation(event) };
                ctx.cursor_guard
                    .sync(ctx.controller.is_xr_active(), Some(loc));
            }
            consumed
        }
        K_CG_EVENT_FLAGS_CHANGED => {
            let keycode =
                unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) } as u16;
            let flags = unsafe { CGEventGetFlags(event) };
            if let Some(hid) = macos_keycode_to_hid(keycode) {
                if let Some(pressed) = modifier_pressed_from_flags(hid, flags) {
                    ctx.controller
                        .handle_key(hid, pressed, flags_to_modifiers(flags))
                } else {
                    ctx.controller.is_xr_active()
                }
            } else {
                ctx.controller.is_xr_active()
            }
        }
        K_CG_EVENT_MOUSE_MOVED
        | K_CG_EVENT_LEFT_MOUSE_DRAGGED
        | K_CG_EVENT_RIGHT_MOUSE_DRAGGED
        | K_CG_EVENT_OTHER_MOUSE_DRAGGED => {
            let dx = unsafe { CGEventGetDoubleValueField(event, K_CG_MOUSE_EVENT_DELTA_X) };
            let dy = unsafe { CGEventGetDoubleValueField(event, K_CG_MOUSE_EVENT_DELTA_Y) };
            let loc = unsafe { CGEventGetLocation(event) };
            if !ctx.controller.is_xr_active() && ctx.controller.is_connected() {
                let switched = if let Ok(bounds_opt) = ctx.bounds.try_lock() {
                    if let Some(bounds) = *bounds_opt {
                        ctx.controller
                            .handle_host_cursor_sample(loc.x, loc.y, dx, dy, bounds)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if switched {
                    ctx.cursor_guard.sync(true, Some(loc));
                    return std::ptr::null_mut();
                }
            }
            if ctx.controller.is_xr_active() {
                ctx.cursor_guard.sync(true, Some(loc));
                ctx.cursor_guard.enforce_lock(loc);
            } else {
                ctx.cursor_guard.sync(false, None);
            }
            ctx.controller.handle_mouse_move(dx, dy)
        }
        K_CG_EVENT_LEFT_MOUSE_DOWN => ctx.controller.handle_mouse_button(0, true),
        K_CG_EVENT_LEFT_MOUSE_UP => ctx.controller.handle_mouse_button(0, false),
        K_CG_EVENT_RIGHT_MOUSE_DOWN => ctx.controller.handle_mouse_button(1, true),
        K_CG_EVENT_RIGHT_MOUSE_UP => ctx.controller.handle_mouse_button(1, false),
        K_CG_EVENT_OTHER_MOUSE_DOWN | K_CG_EVENT_OTHER_MOUSE_UP => {
            let btn =
                unsafe { CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_BUTTON_NUMBER) };
            let pressed = event_type == K_CG_EVENT_OTHER_MOUSE_DOWN;
            if (2..=4).contains(&btn) {
                ctx.controller.handle_mouse_button(btn as u8, pressed)
            } else {
                ctx.controller.is_xr_active()
            }
        }
        K_CG_EVENT_SCROLL_WHEEL => {
            let continuous =
                unsafe { CGEventGetIntegerValueField(event, K_CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS) }
                    != 0;
            if continuous {
                let pv = unsafe {
                    CGEventGetDoubleValueField(event, K_CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1)
                };
                let ph = unsafe {
                    CGEventGetDoubleValueField(event, K_CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2)
                };
                ctx.controller.handle_scroll(pv, ph, false)
            } else {
                let dv = unsafe {
                    CGEventGetDoubleValueField(event, K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1)
                };
                let dh = unsafe {
                    CGEventGetDoubleValueField(event, K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2)
                };
                ctx.controller.handle_scroll(dv, dh, true)
            }
        }
        _ => false,
    };

    if consumed {
        std::ptr::null_mut()
    } else {
        event
    }
}

pub fn check_permissions() -> PermissionStatus {
    let accessibility = unsafe { AXIsProcessTrusted() } != 0;
    let input_monitoring = unsafe { CGPreflightListenEventAccess() };
    let ready = accessibility && input_monitoring;
    let summary = if ready {
        "Accessibility and Input Monitoring granted".to_string()
    } else {
        format!(
            "Missing macOS permissions (Accessibility={}, Input Monitoring={}). \
             If Aura KVM is already checked in Privacy & Security after an app update, select it, click '−' to remove the stale entry, re-add it, then quit & relaunch.",
            accessibility, input_monitoring
        )
    };
    PermissionStatus {
        ready,
        accessibility,
        input_monitoring,
        summary,
    }
}

pub fn request_permissions() -> PermissionStatus {
    unsafe {
        if AXIsProcessTrusted() == 0 {
            let keys = [kAXTrustedCheckOptionPrompt as *const c_void];
            let values = [kCFBooleanTrue];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
            );
            if !dict.is_null() {
                AXIsProcessTrustedWithOptions(dict);
                CFRelease(dict);
            }
        }
        if !CGPreflightListenEventAccess() {
            CGRequestListenEventAccess();
        }
    }
    check_permissions()
}

pub fn open_permission_settings(pane: &str) {
    let url = if pane == "accessibility" {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
    } else {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
    };
    let _ = std::process::Command::new("open").arg(url).spawn();
}

pub struct MacOsBackend;

impl InputBackend for MacOsBackend {
    fn run(&mut self, controller: Arc<SessionController>) -> Result<(), String> {
        let perms = check_permissions();
        if !perms.ready {
            push_log(format!("[macos] WARNING: {}", perms.summary));
        }

        let mask: u64 = (1 << K_CG_EVENT_LEFT_MOUSE_DOWN)
            | (1 << K_CG_EVENT_LEFT_MOUSE_UP)
            | (1 << K_CG_EVENT_RIGHT_MOUSE_DOWN)
            | (1 << K_CG_EVENT_RIGHT_MOUSE_UP)
            | (1 << K_CG_EVENT_MOUSE_MOVED)
            | (1 << K_CG_EVENT_LEFT_MOUSE_DRAGGED)
            | (1 << K_CG_EVENT_RIGHT_MOUSE_DRAGGED)
            | (1 << K_CG_EVENT_KEY_DOWN)
            | (1 << K_CG_EVENT_KEY_UP)
            | (1 << K_CG_EVENT_FLAGS_CHANGED)
            | (1 << K_CG_EVENT_SCROLL_WHEEL)
            | (1 << K_CG_EVENT_OTHER_MOUSE_DOWN)
            | (1 << K_CG_EVENT_OTHER_MOUSE_UP)
            | (1 << K_CG_EVENT_OTHER_MOUSE_DRAGGED);

        let cursor_guard = Arc::new(CursorGuard::new());
        let mut current_monitor_id = controller.config().edge_switch.monitor_id;
        let mut ctx = Box::new(TapContext {
            controller: Arc::clone(&controller),
            tap_port: std::ptr::null_mut(),
            bounds: Mutex::new(refresh_target_display_bounds(&current_monitor_id)),
            cursor_guard: Arc::clone(&cursor_guard),
        });
        let ctx_ptr = &mut *ctx as *mut TapContext as *mut c_void;

        let mut warned_waiting = false;
        let tap = loop {
            if controller.should_exit() {
                return Ok(());
            }
            let mut candidate = unsafe {
                CGEventTapCreate(
                    K_CG_HID_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_DEFAULT,
                    mask,
                    event_tap_callback,
                    ctx_ptr,
                )
            };
            if candidate.is_null() {
                candidate = unsafe {
                    CGEventTapCreate(
                        K_CG_SESSION_EVENT_TAP,
                        K_CG_HEAD_INSERT_EVENT_TAP,
                        K_CG_EVENT_TAP_OPTION_DEFAULT,
                        mask,
                        event_tap_callback,
                        ctx_ptr,
                    )
                };
            }
            if !candidate.is_null() {
                if warned_waiting {
                    push_log("[macos] permissions granted; CGEventTap active");
                }
                break candidate;
            }
            if !warned_waiting {
                warned_waiting = true;
                push_log(
                    "[macos] waiting for Accessibility & Input Monitoring permissions to start input capture...",
                );
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        };
        ctx.tap_port = tap;

        let source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0) };
        if source.is_null() {
            unsafe { CFRelease(tap) };
            return Err("CFMachPortCreateRunLoopSource failed".into());
        }

        let run_loop = unsafe { CFRunLoopGetCurrent() };
        unsafe {
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
        }

        eprintln!(
            "[macos] ready! Press Option+Cmd+X to toggle MacBook <-> XREAL Aura \
             (Ctrl+Option+Cmd+Esc for emergency release)."
        );

        let mut tick = 0u32;
        while !controller.should_exit() {
            let desired_monitor_id = controller.config().edge_switch.monitor_id;
            if desired_monitor_id != current_monitor_id || tick.is_multiple_of(30) {
                current_monitor_id = desired_monitor_id;
                if let Ok(mut b) = ctx.bounds.lock() {
                    *b = refresh_target_display_bounds(&current_monitor_id);
                }
            }
            tick = tick.wrapping_add(1);
            let active = controller.is_xr_active();
            let warp_pt = if !active {
                controller.take_pending_host_cursor_uv().map(|(u, v)| {
                    let b = ctx
                        .bounds
                        .lock()
                        .ok()
                        .and_then(|g| *g)
                        .unwrap_or_else(primary_display_bounds);
                    CGPoint {
                        x: b.min_x + u * (b.max_x - b.min_x),
                        y: b.min_y + v * (b.max_y - b.min_y),
                    }
                })
            } else {
                None
            };
            cursor_guard.sync(active, None);
            if let Some(pt) = warp_pt {
                cursor_guard.warp_to(pt);
            }
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 1);
            }
        }

        cursor_guard.sync(false, None);
        unsafe {
            CGEventTapEnable(tap, false);
            CFRelease(source);
            CFRelease(tap);
        }
        Ok(())
    }
}
