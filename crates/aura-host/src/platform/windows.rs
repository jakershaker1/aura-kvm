//! Windows input capture backend using Win32 `WH_KEYBOARD_LL` & `WH_MOUSE_LL`.

use super::{InputBackend, Modifiers, PermissionStatus, SessionController};
use crate::keymap::windows_scancode_to_hid;
use crate::logbuf::push_log;
use crate::switch::{select_target_display, DisplayBounds, DisplayInfo};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const WH_KEYBOARD_LL: i32 = 13;
const WH_MOUSE_LL: i32 = 14;

const WM_KEYDOWN: usize = 0x0100;
const WM_KEYUP: usize = 0x0101;
const WM_SYSKEYDOWN: usize = 0x0104;
const WM_SYSKEYUP: usize = 0x0105;

const WM_MOUSEMOVE: usize = 0x0200;
const WM_LBUTTONDOWN: usize = 0x0201;
const WM_LBUTTONUP: usize = 0x0202;
const WM_RBUTTONDOWN: usize = 0x0204;
const WM_RBUTTONUP: usize = 0x0205;
const WM_MBUTTONDOWN: usize = 0x0207;
const WM_MBUTTONUP: usize = 0x0208;
const WM_MOUSEWHEEL: usize = 0x020A;
const WM_XBUTTONDOWN: usize = 0x020B;
const WM_XBUTTONUP: usize = 0x020C;
const WM_MOUSEHWHEEL: usize = 0x020E;
const WM_TIMER: u32 = 0x0113;

const LLKHF_EXTENDED: u32 = 0x01;
const LLKHF_INJECTED: u32 = 0x10;
const LLMHF_INJECTED: u32 = 0x01;

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const MONITORINFOF_PRIMARY: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MONITORINFOEXW {
    cb_size: u32,
    rc_monitor: RECT,
    rc_work: RECT,
    dw_flags: u32,
    sz_device: [u16; 32],
}

#[repr(C)]
struct MSG {
    hwnd: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: POINT,
}

#[repr(C)]
struct KBDLLHOOKSTRUCT {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[repr(C)]
struct MSLLHOOKSTRUCT {
    pt: POINT,
    mouse_data: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

type HookProc = unsafe extern "system" fn(code: i32, w_param: usize, l_param: isize) -> isize;
type MonitorEnumProc = unsafe extern "system" fn(
    hmonitor: *mut c_void,
    hdc: *mut c_void,
    lprc_monitor: *mut RECT,
    dw_data: isize,
) -> i32;

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        lpfn: HookProc,
        hmod: *mut c_void,
        dw_thread_id: u32,
    ) -> *mut c_void;
    fn UnhookWindowsHookEx(hhk: *mut c_void) -> i32;
    fn CallNextHookEx(hhk: *mut c_void, n_code: i32, w_param: usize, l_param: isize) -> isize;
    fn GetMessageW(
        lp_msg: *mut MSG,
        hwnd: *mut c_void,
        msg_filter_min: u32,
        msg_filter_max: u32,
    ) -> i32;
    fn TranslateMessage(lp_msg: *const MSG) -> i32;
    fn DispatchMessageW(lp_msg: *const MSG) -> isize;
    fn PostQuitMessage(exit_code: i32);
    fn SetTimer(
        hwnd: *mut c_void,
        id_event: usize,
        elapse: u32,
        timer_func: *const c_void,
    ) -> usize;
    fn KillTimer(hwnd: *mut c_void, id_event: usize) -> i32;
    fn GetAsyncKeyState(v_key: i32) -> i16;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn ClipCursor(rect: *const RECT) -> i32;
    fn ShowCursor(show: i32) -> i32;
    fn EnumDisplayMonitors(
        hdc: *mut c_void,
        lprc_clip: *const RECT,
        lpfn_enum: MonitorEnumProc,
        dw_data: isize,
    ) -> i32;
    fn GetMonitorInfoW(hmonitor: *mut c_void, lpmi: *mut MONITORINFOEXW) -> i32;
}

unsafe extern "system" fn enum_monitor_cb(
    hmonitor: *mut c_void,
    _hdc: *mut c_void,
    _lprc_monitor: *mut RECT,
    dw_data: isize,
) -> i32 {
    let out = &mut *(dw_data as *mut Vec<DisplayInfo>);
    let mut info: MONITORINFOEXW = std::mem::zeroed();
    info.cb_size = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmonitor, &mut info) != 0 {
        let end = info
            .sz_device
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.sz_device.len());
        let dev_name = String::from_utf16_lossy(&info.sz_device[..end]);
        let idx = out.len() + 1;
        let is_primary = (info.dw_flags & MONITORINFOF_PRIMARY) != 0;
        let w = info.rc_monitor.right - info.rc_monitor.left;
        let h = info.rc_monitor.bottom - info.rc_monitor.top;
        if w > 0 && h > 0 {
            let id = if dev_name.is_empty() {
                format!("display-{idx}")
            } else {
                dev_name
            };
            let primary_tag = if is_primary { ", Primary" } else { "" };
            out.push(DisplayInfo {
                id,
                label: format!("Display {idx}{primary_tag} ({w}×{h})"),
                is_primary,
                bounds: DisplayBounds {
                    min_x: f64::from(info.rc_monitor.left),
                    min_y: f64::from(info.rc_monitor.top),
                    max_x: f64::from(info.rc_monitor.right),
                    max_y: f64::from(info.rc_monitor.bottom),
                },
            });
        }
    }
    1
}

pub fn list_displays() -> Vec<DisplayInfo> {
    let mut out: Vec<DisplayInfo> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            enum_monitor_cb,
            &mut out as *mut Vec<DisplayInfo> as isize,
        );
    }
    if out.is_empty() {
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if w > 0 && h > 0 {
            out.push(DisplayInfo {
                id: "primary".into(),
                label: format!("Primary Monitor ({w}×{h})"),
                is_primary: true,
                bounds: DisplayBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: f64::from(w),
                    max_y: f64::from(h),
                },
            });
        }
    }
    out
}

struct WinHookState {
    controller: Arc<SessionController>,
    cursor_captured: AtomicBool,
    anchor_x: i32,
    anchor_y: i32,
    prev_pos: Mutex<Option<(i32, i32)>>,
    target_bounds: Mutex<Option<DisplayBounds>>,
    last_monitor_id: Mutex<String>,
    last_bounds_refresh: Mutex<Instant>,
}

static HOOK_STATE: OnceLock<WinHookState> = OnceLock::new();

impl WinHookState {
    fn sync_cursor(&self, want_captured: bool) {
        let was = self.cursor_captured.swap(want_captured, Ordering::SeqCst);
        if was == want_captured {
            return;
        }
        unsafe {
            if want_captured {
                SetCursorPos(self.anchor_x, self.anchor_y);
                let rect = RECT {
                    left: self.anchor_x - 200,
                    top: self.anchor_y - 200,
                    right: self.anchor_x + 200,
                    bottom: self.anchor_y + 200,
                };
                ClipCursor(&rect);
                ShowCursor(0);
            } else {
                ClipCursor(std::ptr::null());
                ShowCursor(1);
            }
        }
    }
}

fn current_modifiers() -> Modifiers {
    unsafe {
        Modifiers {
            ctrl: (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0,
            shift: (GetAsyncKeyState(VK_SHIFT) as u16 & 0x8000) != 0,
            alt: (GetAsyncKeyState(VK_MENU) as u16 & 0x8000) != 0,
            gui: ((GetAsyncKeyState(VK_LWIN) | GetAsyncKeyState(VK_RWIN)) as u16 & 0x8000) != 0,
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, w_param: usize, l_param: isize) -> isize {
    if code >= 0 {
        if let Some(state) = HOOK_STATE.get() {
            let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
            if (kb.flags & LLKHF_INJECTED) == 0 {
                let pressed = w_param == WM_KEYDOWN || w_param == WM_SYSKEYDOWN;
                let up = w_param == WM_KEYUP || w_param == WM_SYSKEYUP;
                if pressed || up {
                    let extended = (kb.flags & LLKHF_EXTENDED) != 0;
                    if let Some(hid) = windows_scancode_to_hid(kb.scan_code as u16, extended) {
                        if state.controller.handle_key(hid, pressed, current_modifiers()) {
                            return 1;
                        }
                    } else if state.controller.is_xr_active() {
                        return 1;
                    }
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

unsafe extern "system" fn mouse_proc(code: i32, w_param: usize, l_param: isize) -> isize {
    if code >= 0 {
        if let Some(state) = HOOK_STATE.get() {
            let ms = &*(l_param as *const MSLLHOOKSTRUCT);
            if (ms.flags & LLMHF_INJECTED) == 0 {
                if !state.controller.is_xr_active() {
                    if w_param == WM_MOUSEMOVE && state.controller.is_connected() {
                        let (dx, dy) = {
                            let mut prev = state.prev_pos.lock().unwrap_or_else(|e| e.into_inner());
                            let delta = match *prev {
                                Some((px, py)) => ((ms.pt.x - px) as f64, (ms.pt.y - py) as f64),
                                None => (0.0, 0.0),
                            };
                            *prev = Some((ms.pt.x, ms.pt.y));
                            delta
                        };
                        let desired_id = state.controller.config().edge_switch.monitor_id;
                        let need_refresh = {
                            let last_t = state
                                .last_bounds_refresh
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            let last_id = state
                                .last_monitor_id
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            last_t.elapsed() >= Duration::from_secs(1) || *last_id != desired_id
                        };
                        if need_refresh {
                            let refreshed =
                                select_target_display(&list_displays(), &desired_id).map(|d| d.bounds);
                            if let Ok(mut b) = state.target_bounds.lock() {
                                *b = refreshed;
                            }
                            if let Ok(mut id) = state.last_monitor_id.lock() {
                                *id = desired_id;
                            }
                            if let Ok(mut t) = state.last_bounds_refresh.lock() {
                                *t = Instant::now();
                            }
                        }
                        let bounds_opt = state.target_bounds.lock().ok().and_then(|g| *g);
                        if let Some(bounds) = bounds_opt {
                            state.controller.handle_host_cursor_sample(
                                ms.pt.x as f64,
                                ms.pt.y as f64,
                                dx,
                                dy,
                                bounds,
                            );
                        }
                    }
                    return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
                }

                match w_param {
                    WM_MOUSEMOVE => {
                        let dx = ms.pt.x - state.anchor_x;
                        let dy = ms.pt.y - state.anchor_y;
                        if dx != 0 || dy != 0 {
                            state.controller.handle_mouse_move(dx as f64, dy as f64);
                            SetCursorPos(state.anchor_x, state.anchor_y);
                        }
                        return 1;
                    }
                    WM_LBUTTONDOWN => {
                        state.controller.handle_mouse_button(0, true);
                        return 1;
                    }
                    WM_LBUTTONUP => {
                        state.controller.handle_mouse_button(0, false);
                        return 1;
                    }
                    WM_RBUTTONDOWN => {
                        state.controller.handle_mouse_button(1, true);
                        return 1;
                    }
                    WM_RBUTTONUP => {
                        state.controller.handle_mouse_button(1, false);
                        return 1;
                    }
                    WM_MBUTTONDOWN => {
                        state.controller.handle_mouse_button(2, true);
                        return 1;
                    }
                    WM_MBUTTONUP => {
                        state.controller.handle_mouse_button(2, false);
                        return 1;
                    }
                    WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        let xbtn = ((ms.mouse_data >> 16) & 0xFFFF) as u16;
                        let idx = if xbtn == 1 { 3 } else { 4 };
                        state
                            .controller
                            .handle_mouse_button(idx, w_param == WM_XBUTTONDOWN);
                        return 1;
                    }
                    WM_MOUSEWHEEL => {
                        let delta = ((ms.mouse_data >> 16) as i16) as f64 / 120.0;
                        state.controller.handle_scroll(delta, 0.0, true);
                        return 1;
                    }
                    WM_MOUSEHWHEEL => {
                        let delta = ((ms.mouse_data >> 16) as i16) as f64 / 120.0;
                        state.controller.handle_scroll(0.0, delta, true);
                        return 1;
                    }
                    _ => {}
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

pub fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        ready: true,
        accessibility: true,
        input_monitoring: true,
        summary: "Win32 low-level input hooks ready".to_string(),
    }
}

pub struct WindowsBackend;

impl InputBackend for WindowsBackend {
    fn run(&mut self, controller: Arc<SessionController>) -> Result<(), String> {
        let anchor_x = unsafe { GetSystemMetrics(SM_CXSCREEN) } / 2;
        let anchor_y = unsafe { GetSystemMetrics(SM_CYSCREEN) } / 2;

        let init_monitor_id = controller.config().edge_switch.monitor_id;
        let init_bounds =
            select_target_display(&list_displays(), &init_monitor_id).map(|d| d.bounds);
        let _ = HOOK_STATE.set(WinHookState {
            controller: Arc::clone(&controller),
            cursor_captured: AtomicBool::new(false),
            anchor_x,
            anchor_y,
            prev_pos: Mutex::new(None),
            target_bounds: Mutex::new(init_bounds),
            last_monitor_id: Mutex::new(init_monitor_id),
            last_bounds_refresh: Mutex::new(Instant::now()),
        });

        let kb_hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, keyboard_proc, std::ptr::null_mut(), 0)
        };
        if kb_hook.is_null() {
            return Err("SetWindowsHookExW(WH_KEYBOARD_LL) failed".into());
        }

        let ms_hook =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, mouse_proc, std::ptr::null_mut(), 0) };
        if ms_hook.is_null() {
            unsafe { UnhookWindowsHookEx(kb_hook) };
            return Err("SetWindowsHookExW(WH_MOUSE_LL) failed".into());
        }

        let timer_id = unsafe { SetTimer(std::ptr::null_mut(), 1, 100, std::ptr::null()) };
        push_log(
            "[windows] ready! Press Ctrl+Alt+X to toggle PC <-> XREAL Aura \
             (Ctrl+Alt+Shift+Esc for emergency release).",
        );

        let mut msg = unsafe { std::mem::zeroed::<MSG>() };
        while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
            if msg.message == WM_TIMER {
                if let Some(state) = HOOK_STATE.get() {
                    state.sync_cursor(controller.is_xr_active());
                }
                if controller.should_exit() {
                    unsafe { PostQuitMessage(0) };
                }
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if let Some(state) = HOOK_STATE.get() {
            state.sync_cursor(false);
        }
        unsafe {
            if timer_id != 0 {
                KillTimer(std::ptr::null_mut(), timer_id);
            }
            UnhookWindowsHookEx(ms_hook);
            UnhookWindowsHookEx(kb_hook);
        }
        Ok(())
    }
}
