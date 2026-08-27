//! Windows taskbar text rendering.
//!
//! Strategy (mirrors TrafficMonitor's Win11 "overlay" approach):
//!   * Each instance is its own top-level `WS_EX_LAYERED` window, made
//!     transparent to mouse input (`WS_EX_TRANSPARENT`) so it never steals
//!     clicks from the real taskbar beneath it.
//!   * Instances are pinned to the **left** edge (just right of the Start
//!     button) or the **right** edge (just left of the notification/tray area)
//!     of the Windows taskbar, on both Windows 10 and 11.
//!   * A dedicated UI thread owns every window and runs a message pump; all
//!     public calls are marshalled to it through an `mpsc` channel + a
//!     `PostThreadMessageW` wake-up. This keeps Win32 object creation on a
//!     single thread (required by the API) while the Tauri command handlers
//!     run on arbitrary async threads.
//!   * A `WinEventHook` on the taskbar's `EVENT_OBJECT_LOCATIONCHANGE`
//!     re-lays-out every instance when the taskbar moves/resizes.
//!
//! NOTE: this module can only be compiled for `cfg(target_os = "windows")`.
//! It is written against `windows-sys` 0.52 and has **not** been run on a
//! Windows machine from this repo yet — see README.md for the verification
//! checklist before shipping.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use tauri::{Emitter, Wry};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::System::WindowsProgramming::MulDiv;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::models::{ColorStyle, Rect, Side};

// ---------------------------------------------------------------------------
// Small local constants (avoid depending on glob re-exports that may differ
// across windows-sys versions).
// ---------------------------------------------------------------------------

#[inline]
fn rgb_val(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

fn rect_zero() -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    }
}

// ---------------------------------------------------------------------------
// Cross-thread plumbing
// ---------------------------------------------------------------------------

const WM_APP_COMMAND: u32 = WM_APP + 1;

enum UiCommand {
    Create { id: String, side: Side },
    Remove { id: String },
    SetText { id: String, top: String, bottom: String },
    SetFontSizes { id: String, top: f64, bottom: f64 },
    SetLayout { id: String, layout: i32 },
    SetColors { id: String, top: ColorStyle, bottom: ColorStyle },
    SetBold { id: String, top: bool, bottom: bool },
    SetAlignment { id: String, top: i32, bottom: i32 },
    SetVisible { id: String, visible: bool },
    Relayout,
}

/// Sender half + UI thread id, used by `post` to marshal work.
static UI_TX: OnceLock<Sender<UiCommand>> = OnceLock::new();
static UI_THREAD: OnceLock<u32> = OnceLock::new();
/// App handle, so the UI thread can emit `ready` events.
static APP: OnceLock<tauri::AppHandle<Wry>> = OnceLock::new();
/// Taskbar HWND, so the WinEvent hook can recognise it.
static TASKBAR_HWND: Mutex<HWND> = Mutex::new(0);

/// Instances, owned exclusively by the UI thread.
static INSTANCES: OnceLock<Mutex<HashMap<String, Inst>>> = OnceLock::new();
/// Monotonic creation counter, used to order instances within a side.
static NEXT_ORDER: Mutex<u64> = Mutex::new(0);

/// Ensure the UI thread is running. Idempotent.
pub fn start(app: tauri::AppHandle<Wry>) {
    let _ = APP.set(app);
    if UI_TX.get().is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel::<UiCommand>();
    let _ = UI_TX.set(tx);
    std::thread::spawn(move || ui_thread(rx));
}

fn post(cmd: UiCommand) {
    if let Some(tx) = UI_TX.get() {
        let _ = tx.send(cmd);
        if let Some(tid) = UI_THREAD.get() {
            unsafe {
                PostThreadMessageW(*tid, WM_APP_COMMAND, 0, 0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Instance state
// ---------------------------------------------------------------------------

struct Inst {
    hwnd: HWND,
    side: Side,
    order: u64,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    top: String,
    bottom: String,
    /// Font sizes in points.
    top_size: f64,
    bottom_size: f64,
    layout: i32,
    top_color: ColorStyle,
    bottom_color: ColorStyle,
    top_bold: bool,
    bottom_bold: bool,
    top_align: i32,
    bottom_align: i32,
    visible: bool,
}

impl Default for Inst {
    fn default() -> Self {
        // layout 0 = emphasis-bottom (small label on top, large value below)
        Inst {
            hwnd: 0,
            side: Side::Right,
            order: 0,
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            top: String::new(),
            bottom: String::new(),
            top_size: 9.0,
            bottom_size: 12.0,
            layout: 0,
            top_color: ColorStyle::Default,
            bottom_color: ColorStyle::Default,
            top_bold: false,
            bottom_bold: false,
            top_align: 0,
            bottom_align: 0,
            visible: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API (called from desktop.rs on the command-handler thread)
// ---------------------------------------------------------------------------

pub fn create(id: String, side: Side) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::Create { id, side });
    Ok(())
}

pub fn remove(id: String) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::Remove { id });
    Ok(())
}

pub fn set_text(id: String, top: String, bottom: String) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetText { id, top, bottom });
    Ok(())
}

pub fn set_font_sizes(id: String, top: f64, bottom: f64) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetFontSizes { id, top, bottom });
    Ok(())
}

pub fn set_layout(id: String, layout: i32) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetLayout { id, layout });
    Ok(())
}

pub fn set_colors(id: String, top: ColorStyle, bottom: ColorStyle) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetColors { id, top, bottom });
    Ok(())
}

pub fn set_bold(id: String, top: bool, bottom: bool) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetBold { id, top, bottom });
    Ok(())
}

pub fn set_alignment(id: String, top: i32, bottom: i32) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetAlignment { id, top, bottom });
    Ok(())
}

pub fn set_visible(id: String, visible: bool) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetVisible { id, visible });
    Ok(())
}

pub fn rect(id: String) -> crate::Result<Rect> {
    let map = INSTANCES
        .get()
        .ok_or(crate::Error::UnsupportedPlatform)?
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let inst = map.get(&id).ok_or(crate::Error::InstanceNotFound)?;
    Ok(Rect {
        x: inst.x as f64,
        y: inst.y as f64,
        width: inst.w as f64,
        height: inst.h as f64,
    })
}

pub fn is_visible(id: String) -> crate::Result<bool> {
    let map = INSTANCES
        .get()
        .ok_or(crate::Error::UnsupportedPlatform)?
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let inst = map.get(&id).ok_or(crate::Error::InstanceNotFound)?;
    Ok(inst.visible)
}

fn start_if_needed() {
    if UI_TX.get().is_none() {
        if let Some(app) = APP.get() {
            let app = app.clone();
            start(app);
        }
    }
}

// ---------------------------------------------------------------------------
// UI thread
// ---------------------------------------------------------------------------

fn ui_thread(rx: Receiver<UiCommand>) {
    unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }
    let _ = UI_THREAD.set(unsafe { GetCurrentThreadId() });
    register_class();
    install_taskbar_hook();
    // Keep-alive timer: clicking the taskbar raises its z-order above
    // non-activating overlays on Win11 (observed on build 26200), which
    // visually hides them until the layered window is re-composited.
    // Periodically re-asserting HWND_TOPMOST (only when actually needed,
    // see keep_on_top) is cheap and immune to that.
    const TIMER_KEEP_ON_TOP: usize = 1;
    unsafe {
        SetTimer(0, TIMER_KEEP_ON_TOP, 500, None);
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        drain(&rx);
        let r = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
        if r == 0 || r == -1 {
            break; // WM_QUIT, or error (-1 → do not dispatch a stale message)
        }
        if msg.message == WM_APP_COMMAND {
            drain(&rx);
            continue;
        }
        if msg.message == WM_TIMER && msg.wParam == TIMER_KEEP_ON_TOP {
            keep_on_top();
            continue;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Re-assert HWND_TOPMOST for overlay windows that ended up below the
/// taskbar. Only windows actually stacked under the taskbar are touched, so
/// this stays quiet when nothing happened.
fn keep_on_top() {
    let taskbar = *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner());
    if taskbar == 0 {
        return;
    }
    if let Some(map) = INSTANCES.get() {
        let guard = map.lock().unwrap_or_else(|e| e.into_inner());
        for inst in guard.values() {
            if !inst.visible {
                continue;
            }
            let mut prev = unsafe { GetWindow(inst.hwnd, GW_HWNDPREV) };
            while prev != 0 {
                if prev == taskbar {
                    unsafe {
                        SetWindowPos(
                            inst.hwnd,
                            HWND_TOPMOST,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                    break;
                }
                prev = unsafe { GetWindow(prev, GW_HWNDPREV) };
            }
        }
    }
}

fn drain(rx: &Receiver<UiCommand>) {
    while let Ok(cmd) = rx.try_recv() {
        handle_command(cmd);
    }
}

fn handle_command(cmd: UiCommand) {
    let map = INSTANCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    match cmd {
        UiCommand::Create { id, side } => {
            if guard.contains_key(&id) {
                return;
            }
            match create_window(&id) {
                Some(hwnd) => {
                    let order = {
                        let mut o = NEXT_ORDER.lock().unwrap_or_else(|e| e.into_inner());
                        let v = *o;
                        *o += 1;
                        v
                    };
                    let inst = Inst {
                        hwnd,
                        side,
                        order,
                        ..Default::default()
                    };
                    guard.insert(id.clone(), inst);
                    drop(guard);
                    relayout_all();
                    if let Some(app) = APP.get() {
                        let _ = app.emit(
                            &format!("multiline-taskband://{id}//ready"),
                            serde_json::json!({ "id": id }),
                        );
                    }
                }
                None => {
                    eprintln!("[multiline-taskband] failed to create window for {id}");
                }
            }
        }
        UiCommand::Remove { id } => {
            if let Some(inst) = guard.remove(&id) {
                unsafe { DestroyWindow(inst.hwnd) };
                drop(guard);
                relayout_all();
            }
        }
        UiCommand::SetText { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top = top;
                inst.bottom = bottom;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetFontSizes { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_size = top;
                inst.bottom_size = bottom;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetLayout { id, layout } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.layout = layout;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetColors { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_color = top;
                inst.bottom_color = bottom;
            }
            drop(guard);
            paint(&id);
        }
        UiCommand::SetBold { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_bold = top;
                inst.bottom_bold = bottom;
            }
            drop(guard);
            paint(&id);
        }
        UiCommand::SetAlignment { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_align = top;
                inst.bottom_align = bottom;
            }
            drop(guard);
            paint(&id);
        }
        UiCommand::SetVisible { id, visible } => {
            let hwnd = if let Some(inst) = guard.get_mut(&id) {
                inst.visible = visible;
                inst.hwnd
            } else {
                0
            };
            drop(guard);
            if hwnd != 0 {
                unsafe { ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE }) };
            }
        }
        UiCommand::Relayout => {
            drop(guard);
            relayout_all();
        }
    }
}

// ---------------------------------------------------------------------------
// Window creation
// ---------------------------------------------------------------------------

const CLASS_NAME: &[u16] = &wide_const("MultilineTaskbandOverlay");

/// Build a fixed-size wide string at compile time. Only ASCII class names are
/// supported (they are by construction).
const fn wide_const(s: &str) -> [u16; 32] {
    let bytes = s.as_bytes();
    let mut out = [0u16; 32];
    let mut i = 0;
    while i < bytes.len() && i < 31 {
        out[i] = bytes[i] as u16;
        i += 1;
    }
    out
}

fn register_class() {
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: 0,
        hCursor: unsafe { LoadCursorW(0, IDC_ARROW) },
        hbrBackground: 0,
        lpszMenuName: std::ptr::null(),
        lpszClassName: CLASS_NAME.as_ptr(),
        hIconSm: 0,
    };
    unsafe { RegisterClassExW(&wc) };
}

extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn create_window(id: &str) -> Option<HWND> {
    let name = to_wide(id);
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | WS_EX_TOPMOST,
            CLASS_NAME.as_ptr(),
            name.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            0,
            0,
            hinst,
            std::ptr::null(),
        )
    };
    if hwnd == 0 {
        None
    } else {
        Some(hwnd)
    }
}

// ---------------------------------------------------------------------------
// Layout (left/right edge positioning, Win10 + Win11)
// ---------------------------------------------------------------------------

const GAP: i32 = 4;
const PAD: i32 = 4;

fn dpi() -> i32 {
    let d = unsafe { GetDpiForSystem() };
    if d == 0 {
        96
    } else {
        d as i32
    }
}

fn relayout_all() {
    let map = match INSTANCES.get() {
        Some(m) => m,
        None => return,
    };
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_empty() {
        return;
    }

    let taskbar = find_taskbar();

    // Remember taskbar hwnd for the event hook.
    if taskbar.hwnd != 0 {
        *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner()) = taskbar.hwnd;
    }

    let horizontal = (taskbar.rect.right - taskbar.rect.left)
        >= (taskbar.rect.bottom - taskbar.rect.top);

    // Collect ids per side (avoid holding two mutable borrows of `guard` at
    // once), then sort by creation order.
    let mut left_ids: Vec<String> = guard
        .iter()
        .filter(|(_, i)| i.side == Side::Left)
        .map(|(k, _)| k.clone())
        .collect();
    let mut right_ids: Vec<String> = guard
        .iter()
        .filter(|(_, i)| i.side == Side::Right)
        .map(|(k, _)| k.clone())
        .collect();
    left_ids.sort_by_key(|id| guard.get(id).map(|i| i.order).unwrap_or(0));
    right_ids.sort_by_key(|id| guard.get(id).map(|i| i.order).unwrap_or(0));

    let center_y = |h: i32| -> i32 {
        let tb_h = taskbar.rect.bottom - taskbar.rect.top;
        taskbar.rect.top + (tb_h - h) / 2
    };

    if horizontal {
        // Right side: stack leftward from the notification area.
        let mut cursor = taskbar.right_edge_for_right();
        for id in &right_ids {
            if let Some(inst) = guard.get_mut(id) {
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                cursor -= w;
                inst.x = cursor;
                inst.y = center_y(h);
                cursor -= GAP;
                paint_inst(inst);
            }
        }
        // Left side: stack rightward from the taskbar's left edge.
        let mut cursor = taskbar.left_edge_for_left();
        for id in &left_ids {
            if let Some(inst) = guard.get_mut(id) {
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = cursor;
                inst.y = center_y(h);
                cursor += w + GAP;
                paint_inst(inst);
            }
        }
    } else {
        // Vertical taskbar (left/right of screen): stack downward, centred.
        let mut cursor_y = taskbar.rect.top + GAP;
        for id in &right_ids {
            if let Some(inst) = guard.get_mut(id) {
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = taskbar.rect.left
                    + (taskbar.rect.right - taskbar.rect.left - w) / 2;
                inst.y = cursor_y;
                cursor_y += h + GAP;
                paint_inst(inst);
            }
        }
        let mut cursor_y = taskbar.rect.top + GAP;
        for id in &left_ids {
            if let Some(inst) = guard.get_mut(id) {
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = taskbar.rect.left
                    + (taskbar.rect.right - taskbar.rect.left - w) / 2;
                inst.y = cursor_y;
                cursor_y += h + GAP;
                paint_inst(inst);
            }
        }
    }
}

/// Win10/Win11 taskbar discovery + edge helpers.
struct Taskbar {
    hwnd: HWND,
    rect: RECT,
}

impl Taskbar {
    /// x just left of the notification/tray area (where right-side instances
    /// start stacking from).
    fn right_edge_for_right(&self) -> i32 {
        let notify = find_window_ex(self.hwnd, "TrayNotifyWnd");
        if notify != 0 {
            let mut r = rect_zero();
            if unsafe { GetWindowRect(notify, &mut r) } != 0 {
                return r.left - 2;
            }
        }
        self.rect.right - unsafe { MulDiv(88, dpi(), 96) }
    }

    /// x where left-side instances start stacking from: the far left edge of
    /// the taskbar. (Win11's default centered layout puts the Start button in
    /// the middle of the taskbar, so anchoring to the button would push the
    /// labels toward the centre; the far left is stable in both centered and
    /// left-aligned layouts.)
    fn left_edge_for_left(&self) -> i32 {
        self.rect.left + 2
    }
}

fn find_taskbar() -> Taskbar {
    let hwnd = find_window_ex(0, "Shell_TrayWnd");
    let hwnd = if hwnd == 0 {
        find_window_ex(0, "Shell_SecondaryTrayWnd")
    } else {
        hwnd
    };
    let mut rect = rect_zero();
    if hwnd != 0 {
        unsafe { GetWindowRect(hwnd, &mut rect) };
    }
    Taskbar { hwnd, rect }
}

fn find_window_ex(parent: HWND, class: &str) -> HWND {
    let w = to_wide(class);
    unsafe { FindWindowExW(parent, 0, w.as_ptr(), std::ptr::null()) }
}

// ---------------------------------------------------------------------------
// Measurement & painting
// ---------------------------------------------------------------------------

fn pt_to_px(pt: f64, dpi: i32) -> i32 {
    (pt * dpi as f64 / 72.0).round() as i32
}

/// Measure an instance's window size from its two lines (no drawing).
fn measure(inst: &Inst) -> (i32, i32) {
    let d = dpi();
    let (tw, th) = measure_text(&inst.top, pt_to_px(inst.top_size, d), inst.top_bold);
    let (bw, bh) = measure_text(&inst.bottom, pt_to_px(inst.bottom_size, d), inst.bottom_bold);
    let w = tw.max(bw) + 2 * PAD;
    let h = th + GAP + bh;
    (w.max(1), h.max(1))
}

fn measure_text(text: &str, size_px: i32, bold: bool) -> (i32, i32) {
    let hdc = unsafe { CreateCompatibleDC(0) };
    if hdc == 0 {
        return ((text.len() as i32 * size_px).max(1), size_px.max(1));
    }
    let font = make_font(size_px, bold);
    let old = unsafe { SelectObject(hdc, font) };
    unsafe { SetBkMode(hdc, TRANSPARENT as i32) };
    let wtext = to_wide(text);
    let mut r = rect_zero();
    unsafe {
        DrawTextW(
            hdc,
            wtext.as_ptr(),
            -1,
            &mut r,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        )
    };
    unsafe {
        SelectObject(hdc, old);
        DeleteObject(font);
        DeleteDC(hdc);
    }
    (r.right.max(1), r.bottom.max(1))
}

/// Render a single line into a white-on-black DIB and return a per-pixel
/// coverage (alpha) buffer. Because the text is white on black, every channel
/// equals the coverage, so the red channel *is* the alpha — giving clean
/// anti-aliased edges we can recolour to any target colour.
fn render_line_alpha(text: &str, size_px: i32, bold: bool) -> (i32, i32, Vec<u8>) {
    let hdc = unsafe { CreateCompatibleDC(0) };
    let font = make_font(size_px, bold);
    let old = unsafe { SelectObject(hdc, font) };
    unsafe { SetBkMode(hdc, TRANSPARENT as i32) };

    let wtext = to_wide(text);
    let mut r = rect_zero();
    unsafe {
        DrawTextW(
            hdc,
            wtext.as_ptr(),
            -1,
            &mut r,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        )
    };
    let w = r.right.max(1);
    let h = r.bottom.max(1);

    let (hbmp, bits) = create_dib(hdc, w, h);
    let old_bmp = unsafe { SelectObject(hdc, hbmp) };
    // Fill opaque black, then draw white text.
    let bits_u32 = bits as *mut u32;
    for i in 0..(w * h) as usize {
        unsafe { bits_u32.add(i).write(0xFF00_0000) };
    }
    unsafe {
        SetTextColor(hdc, rgb_val(255, 255, 255));
        let mut dr = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        DrawTextW(
            hdc,
            wtext.as_ptr(),
            -1,
            &mut dr,
            DT_SINGLELINE | DT_NOPREFIX | DT_VCENTER | DT_CENTER,
        );
    }

    let mut alpha = vec![0u8; (w * h) as usize];
    for (i, px) in unsafe { std::slice::from_raw_parts(bits_u32, (w * h) as usize) }
        .iter()
        .enumerate()
    {
        // white text => R == G == B == coverage
        alpha[i] = ((*px >> 16) & 0xFF) as u8;
    }

    unsafe {
        SelectObject(hdc, old_bmp);
        DeleteObject(hbmp);
        SelectObject(hdc, old);
        DeleteObject(font);
        DeleteDC(hdc);
    }
    (w, h, alpha)
}

fn make_font(size_px: i32, bold: bool) -> HFONT {
    let weight: i32 = if bold { FW_BOLD as i32 } else { FW_NORMAL as i32 };
    let lf = LOGFONTW {
        lfHeight: -(size_px.max(1)),
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: weight,
        lfItalic: 0,
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: DEFAULT_CHARSET,
        lfOutPrecision: OUT_DEFAULT_PRECIS,
        lfClipPrecision: CLIP_DEFAULT_PRECIS,
        lfQuality: ANTIALIASED_QUALITY,
        lfPitchAndFamily: DEFAULT_PITCH,
        lfFaceName: [0u16; 32],
    };
    unsafe { CreateFontIndirectW(&lf) }
}

fn create_dib(hdc: HDC, w: i32, h: i32) -> (HBITMAP, *mut c_void) {
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -(h); // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = unsafe {
        CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0)
    };
    (hbmp, bits)
}

fn resolve_color(c: &ColorStyle) -> (u8, u8, u8) {
    match c {
        ColorStyle::Default => {
            // COLOR_BTNTEXT (18) = the taskbar's default text colour, which
            // follows the system light/dark theme.
            let v = unsafe { GetSysColor(COLOR_BTNTEXT) };
            ((v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8)
        }
        ColorStyle::Solid { value } => parse_hex(value),
    }
}

fn parse_hex(s: &str) -> (u8, u8, u8) {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(255);
        (r, g, b)
    } else {
        (255, 255, 255)
    }
}

/// Paint one instance by id.
fn paint(id: &str) {
    let map = match INSTANCES.get() {
        Some(m) => m,
        None => return,
    };
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(inst) = guard.get_mut(id) {
        paint_inst(inst);
    }
}

fn paint_inst(inst: &mut Inst) {
    let d = dpi();
    let top_sz = pt_to_px(inst.top_size, d);
    let bot_sz = pt_to_px(inst.bottom_size, d);
    let (tw, th, top_alpha) = render_line_alpha(&inst.top, top_sz, inst.top_bold);
    let (bw, bh, bot_alpha) = render_line_alpha(&inst.bottom, bot_sz, inst.bottom_bold);

    let w = inst.w.max(1);
    let h = inst.h.max(1);

    let top_x = align_x(inst.top_align, tw, w);
    let bot_x = align_x(inst.bottom_align, bw, w);

    let (tr, tg, tb) = resolve_color(&inst.top_color);
    let (br, bg, bb) = resolve_color(&inst.bottom_color);

    let hdc = unsafe { CreateCompatibleDC(0) };
    let (hbmp, bits) = create_dib(hdc, w, h);
    let old = unsafe { SelectObject(hdc, hbmp) };
    let bits_u32 = bits as *mut u32;
    for i in 0..(w * h) as usize {
        unsafe { bits_u32.add(i).write(0) };
    }

    // Top band.
    blit_line(
        bits_u32,
        w as usize,
        h as usize,
        0,
        top_x as usize,
        tw as usize,
        th as usize,
        &top_alpha,
        tr,
        tg,
        tb,
    );
    // Bottom band.
    let bot_y = (th + GAP) as usize;
    blit_line(
        bits_u32,
        w as usize,
        h as usize,
        bot_y,
        bot_x as usize,
        bw as usize,
        bh as usize,
        &bot_alpha,
        br,
        bg,
        bb,
    );

    let hwnd = inst.hwnd;
    let x = inst.x;
    let y = inst.y;
    let visible = inst.visible;
    unsafe {
        MoveWindow(hwnd, x, y, w, h, FALSE);
        let ppt_dst = POINT { x, y };
        let psize = SIZE { cx: w, cy: h };
        let ppt_src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ulw = UpdateLayeredWindow(hwnd, 0, &ppt_dst, &psize, hdc, &ppt_src, 0, &blend, ULW_ALPHA);
        let _ = ulw;
        SelectObject(hdc, old);
        DeleteObject(hbmp);
        DeleteDC(hdc);
        if visible {
            ShowWindow(hwnd, SW_SHOW);
        } else {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[inline]
fn align_x(align: i32, line_w: i32, win_w: i32) -> i32 {
    if align == 2 {
        (win_w - line_w).max(0)
    } else if align == 1 {
        ((win_w - line_w) / 2).max(0)
    } else {
        PAD
    }
}

/// Composite one line's alpha buffer into the final bitmap at (off_x, off_y).
#[allow(clippy::too_many_arguments)] // win32-style blit signature
fn blit_line(
    dst: *mut u32,
    win_w: usize,
    _win_h: usize,
    off_y: usize,
    off_x: usize,
    line_w: usize,
    line_h: usize,
    alpha: &[u8],
    r: u8,
    g: u8,
    b: u8,
) {
    for ly in 0..line_h {
        let dy = off_y + ly;
        for lx in 0..line_w {
            let a = alpha[ly * line_w + lx];
            if a == 0 {
                continue;
            }
            let dx = off_x + lx;
            let idx = dy * win_w + dx;
            unsafe {
                dst.add(idx).write(
                    (a as u32) << 24 | (b as u32) | ((g as u32) << 8) | ((r as u32) << 16),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Taskbar move/resize hook
// ---------------------------------------------------------------------------

fn install_taskbar_hook() {
    unsafe {
        SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            0,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
}

extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if event != EVENT_OBJECT_LOCATIONCHANGE {
        return;
    }
    let known = *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner());
    if hwnd == 0 {
        return; // global/desktop-wide event, not taskbar-specific
    }
    if known != 0 && hwnd != known && unsafe { IsChild(known, hwnd) } == 0 {
        return; // not the taskbar or one of its descendants
    }
    // Throttle: taskbar hover/animations fire a burst of LOCATIONCHANGE
    // events; a full relayout+paint per event would be wasteful.
    {
        static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        if let Some(t) = *last {
            if now.duration_since(t) < std::time::Duration::from_millis(50) {
                return;
            }
        }
        *last = Some(now);
    }
    post(UiCommand::Relayout);
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
