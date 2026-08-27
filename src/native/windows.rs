//! Windows taskbar text rendering.
//!
//! Strategy (mirrors TrafficMonitor's taskbar embedding approach):
//!   * Each instance is a `WS_EX_LAYERED` window that is embedded as a
//!     **child of the taskbar** via `SetParent(hwnd, Shell_TrayWnd)` (Win11)
//!     — exactly what TrafficMonitor does (`TaskBarDlg::OnInitDialog`:
//!     `SetParent(this->m_hWnd, GetParentHwnd())`). A child window is always
//!     painted above its parent, so clicking the taskbar (which raises the
//!     taskbar's z-order) can never push the text below it — no flash.
//!   * Instances are pinned to the **left** edge (just right of the Start
//!     button) or the **right** edge (just left of the notification/tray area)
//!     of the Windows taskbar, on both Windows 10 and 11. Position is
//!     expressed in taskbar client coordinates once embedded.
//!   * Windows are **clickable** (no `WS_EX_TRANSPARENT`): a left click on an
//!     instance toggles its settings popup (a Tauri webview window, see
//!     `set_popup_window`), a right click emits a `click` event the host can
//!     use for its own context menu. The overlay only covers its own small
//!     label area, so the rest of the taskbar is unaffected.
//!   * A dedicated UI thread owns every window and runs a message pump; all
//!     public calls are marshalled to it through an `mpsc` channel + a
//!     `PostThreadMessageW` wake-up. This keeps Win32 object creation on a
//!     single thread (required by the API) while the Tauri command handlers
//!     run on arbitrary async threads.
//!   * A `WinEventHook` on the taskbar's `EVENT_OBJECT_LOCATIONCHANGE`
//!     re-lays-out every instance when the taskbar moves/resizes.
//!   * If embedding fails (e.g. explorer is in an odd state), instances fall
//!     back to top-level `WS_EX_TOPMOST` windows; the keep-on-top timer then
//!     re-asserts their z-order above the taskbar (pre-Win11-embedding
//!     behaviour).
//!
//! NOTE: this module can only be compiled for `cfg(target_os = "windows")`.
//! It is written against `windows-sys` 0.52 and has **not** been run on a
//! Windows machine from this repo yet — see README.md for the verification
//! checklist before shipping.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tauri::menu::{ContextMenu as TauriContextMenu, Menu as TauriMenu, MenuItem as TauriMenuItem, MenuEvent, PredefinedMenuItem as TauriPredefined};
use tauri::{Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent, Wry};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::System::WindowsProgramming::MulDiv;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::models::{ColorStyle, MenuItemDescriptor, Rect, Side};

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

// ---------------------------------------------------------------------------
// Popup window state (mirrors tauri-plugin-multiline-menubar)
// ---------------------------------------------------------------------------

/// Label of the Tauri window used as the popup (default "popup").
static POPUP_WINDOW: RwLock<Option<Arc<str>>> = RwLock::new(None);

/// Whether a left click automatically toggles the popup window.
static AUTO_POPUP: Mutex<bool> = Mutex::new(true);

/// While set, the popup's blur handler ignores focus loss (prevents the
/// popup from immediately closing when it opens and steals focus).
static POPUP_IGNORE_BLUR_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Ensures the popup auto-hide handler is attached only once.
static POPUP_HANDLER_ATTACHED: Mutex<bool> = Mutex::new(false);

/// Instance id whose popup is currently open, so the blur handler can emit
/// `popup-close` on the right instance's channel.
static ACTIVE_POPUP_ID: Mutex<Option<String>> = Mutex::new(None);

/// Event name used to tell the popup window which instance opened it and what
/// that instance's current state is. Delivered with `emit_to` so only the
/// popup window receives it.
const POPUP_OPEN_TARGET_EVENT: &str = "multiline-taskband://popup//open";

// ---------------------------------------------------------------------------
// Right-click context menu state
// ---------------------------------------------------------------------------

/// Per-instance right-click context menus (built with `tauri::menu` so menu
/// events flow through tauri's own `MenuEvent` bridge — muda's global
/// `set_event_handler` is already owned by tauri).
static MENUS: Mutex<Option<HashMap<String, Arc<TauriMenu<Wry>>>>> = Mutex::new(None);

/// Ensures the global menu-event handler (emitting `{id}//menu`) is installed
/// only once.
static MENU_HANDLER_ATTACHED: Mutex<bool> = Mutex::new(false);

/// Separator between the instance id and the action id in a menu item's id:
/// menu item ids are `{instance_id}::{action_id}`, which lets the single
/// global `AppHandle::on_menu_event` route each selection back to the right
/// instance.
const MENU_ID_SEPARATOR: &str = "::";

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
    /// True when the window has been embedded as a child of the taskbar
    /// (`SetParent`). Embedded children are painted in taskbar client
    /// coordinates and never need z-order maintenance.
    embedded: bool,
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
            embedded: false,
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

// ---------------------------------------------------------------------------
// Popup API (mirrors tauri-plugin-multiline-menubar)
// ---------------------------------------------------------------------------

/// Set which Tauri window is used as the popup. Call before the first open.
pub fn set_popup_window(label: String) -> crate::Result<()> {
    *POPUP_WINDOW.write().unwrap_or_else(|e| e.into_inner()) = Some(label.into());
    Ok(())
}

/// Enable/disable automatically toggling the popup on left click.
pub fn set_auto_popup(enabled: bool) -> crate::Result<()> {
    *AUTO_POPUP.lock().unwrap_or_else(|e| e.into_inner()) = enabled;
    Ok(())
}

pub fn open_popup(id: String) -> crate::Result<()> {
    let app = APP.get().ok_or(crate::Error::UnsupportedPlatform)?;
    open_popup_window(app, &id)
}

pub fn close_popup(id: String) -> crate::Result<()> {
    let app = APP.get().ok_or(crate::Error::UnsupportedPlatform)?;
    close_popup_window(app, &id)
}

pub fn toggle_popup(id: String) -> crate::Result<()> {
    let app = APP.get().ok_or(crate::Error::UnsupportedPlatform)?;
    toggle_popup_window(app, &id)
}

/// Attach (or detach, with `None`) the right-click context menu of an
/// instance. Menu selections are emitted as `multiline-taskband://{id}//menu`
/// with `{ id, itemId }`.
pub fn set_menu(id: String, items: Option<Vec<MenuItemDescriptor>>) -> crate::Result<()> {
    let app = APP.get().ok_or(crate::Error::UnsupportedPlatform)?;
    install_menu_event_handler(app);
    let mut menus = MENUS.lock().unwrap_or_else(|e| e.into_inner());
    let map = menus.get_or_insert_with(HashMap::new);
    match items {
        None => {
            map.remove(&id);
        }
        Some(items) => {
            let menu = build_menu(app, &id, &items)?;
            map.insert(id, Arc::new(menu));
        }
    }
    Ok(())
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
    // Keep-alive timer: for the (rare) fallback where a window could not be
    // embedded into the taskbar, clicking the taskbar raises its z-order
    // above non-activating top-level overlays on Win11 (observed on build
    // 26200), visually hiding them until the layered window is re-composited.
    // Periodically re-asserting HWND_TOPMOST (only when actually needed, see
    // keep_on_top) is cheap and immune to that. Embedded children are skipped
    // by keep_on_top, so this is a no-op for the normal path.
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
/// this stays quiet when nothing happened. Instances embedded as taskbar
/// children (`embedded == true`) never need this — a child window is always
/// painted above its parent, so the taskbar can never cover it.
fn keep_on_top() {
    let taskbar = *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner());
    if taskbar == 0 {
        return;
    }
    if let Some(map) = INSTANCES.get() {
        let guard = map.lock().unwrap_or_else(|e| e.into_inner());
        for inst in guard.values() {
            if !inst.visible || inst.embedded {
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

/// The window's class procedure.
///
/// The overlay is clickable: a left click toggles the instance's settings
/// popup (when auto-popup is on), a right click is emitted as a `click` event
/// for the host to handle. `WS_EX_NOACTIVATE` keeps the click from stealing
/// focus from whatever the user was doing.
extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
        handle_click(hwnd, msg, lparam);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Emit a `click` event for a mouse-down on one of our overlay windows and, on
/// left click, toggle the instance's popup (mirrors the menubar plugin's
/// left-click behaviour).
fn handle_click(hwnd: HWND, msg: u32, lparam: LPARAM) {
    let Some(id) = instance_id_for(hwnd) else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };

    // Client coordinates -> screen coordinates (payload mirrors Tauri's own
    // `TrayIconEvent::Click` shape).
    let x = (lparam & 0xFFFF) as i16 as i32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    let mut pt = POINT { x, y };
    unsafe { ClientToScreen(hwnd, &mut pt) };

    let (rx, ry, rw, rh) = instance_rect_screen(&id).unwrap_or((0, 0, 0, 0));
    let button = if msg == WM_LBUTTONDOWN { "left" } else { "right" };
    let _ = app.emit(
        format!("multiline-taskband://{id}//click").as_str(),
        serde_json::json!({
            "id": id,
            "position": { "x": pt.x, "y": pt.y },
            "rect": { "x": rx, "y": ry, "width": rw, "height": rh },
            "button": button,
            "buttonState": "down",
        }),
    );

    let auto = *AUTO_POPUP.lock().unwrap_or_else(|e| e.into_inner());
    if auto && button == "left" {
        let _ = toggle_popup_window(app, &id);
    } else if button == "right" {
        show_instance_menu(app, &id);
    }
}

/// Find the instance id that owns `hwnd`.
fn instance_id_for(hwnd: HWND) -> Option<String> {
    let map = INSTANCES.get()?.lock().ok()?;
    map.iter()
        .find(|(_, i)| i.hwnd == hwnd)
        .map(|(k, _)| k.clone())
}

/// Screen-coordinate rect of an instance (origin top-left).
fn instance_rect_screen(id: &str) -> Option<(i32, i32, i32, i32)> {
    let map = INSTANCES.get()?.lock().ok()?;
    let inst = map.get(id)?;
    Some((inst.x, inst.y, inst.w, inst.h))
}

fn create_window(id: &str) -> Option<HWND> {
    let name = to_wide(id);
    let hinst = unsafe { GetModuleHandleW(std::ptr::null()) };
    // NOTE: no `WS_EX_TRANSPARENT` — the label area is intentionally clickable
    // (left click opens the settings popup). Only the label's own rectangle is
    // covered, so the rest of the taskbar keeps receiving clicks normally.
    // WS_EX_TOPMOST is kept as a fallback: when SetParent embedding succeeds,
    // the system clears it automatically (a child window cannot be topmost).
    // When embedding fails, the window stays a top-level overlay and the
    // keep_on_top timer re-asserts its z-order above the taskbar.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED
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

    // Embed every instance as a child of the taskbar (mirrors TrafficMonitor:
    // `SetParent(this->m_hWnd, GetParentHwnd())`). A child is always painted
    // above its parent, so clicking the taskbar can never push the text below
    // it — this is what removes the "flash" that top-level overlays exhibit.
    // Re-runs re-embed windows after explorer restarts (old taskbar HWND is
    // gone, `GetParent` no longer matches).
    for inst in guard.values_mut() {
        ensure_embedded(inst, taskbar.hwnd);
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

/// Embed `inst`'s window as a child of the taskbar (TrafficMonitor's
/// approach). Idempotent: if the window is already a child of `taskbar`,
/// nothing happens. Returns via `inst.embedded` whether the window is now
/// embedded. On failure (or no taskbar), the window stays a top-level
/// `WS_EX_TOPMOST` overlay and `embedded` is cleared.
///
/// Note: `SetParent`'s return value cannot be used to detect success here —
/// when the previous parent is the desktop it returns 0 even on success — and
/// `GetParent` cannot either: it returns the *owner* for WS_POPUP windows
/// (which is NULL for us) even after `SetParent` succeeds. The result is
/// therefore verified with `GetAncestor(GA_PARENT)`, which always returns the
/// real parent regardless of window styles.
fn ensure_embedded(inst: &mut Inst, taskbar: HWND) {
    if taskbar == 0 {
        inst.embedded = false;
        return;
    }
    let parent = unsafe { GetAncestor(inst.hwnd, GA_PARENT) };
    if parent == taskbar {
        inst.embedded = true;
        return;
    }
    unsafe { SetParent(inst.hwnd, taskbar) };
    inst.embedded = unsafe { GetAncestor(inst.hwnd, GA_PARENT) } == taskbar;
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
    // Background alpha is 1, not 0: UpdateLayeredWindow hit-tests layered
    // windows per-pixel — a pixel with alpha 0 is transparent to the mouse
    // (clicks fall through to the taskbar). alpha=1 is invisible to the eye
    // but makes the whole label area clickable, so left-click can open the
    // settings popup anywhere on the item (not just exactly on a glyph).
    for i in 0..(w * h) as usize {
        unsafe { bits_u32.add(i).write(0x0100_0000) };
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
    let embedded = inst.embedded;
    // Embedded children are positioned in taskbar client coordinates;
    // top-level fallback windows use screen coordinates. ScreenToClient is
    // used instead of a raw offset subtraction to stay correct even if the
    // taskbar ever gains a border.
    let (mx, my) = if embedded {
        let tb = *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner());
        if tb != 0 {
            let mut pt = POINT { x, y };
            unsafe { ScreenToClient(tb, &mut pt) };
            (pt.x, pt.y)
        } else {
            (x, y)
        }
    } else {
        (x, y)
    };
    unsafe {
        MoveWindow(hwnd, mx, my, w, h, FALSE);
        // For embedded children, pass NULL so UpdateLayeredWindow keeps the
        // position set by MoveWindow (a child's "screen position" semantics
        // are ambiguous; the taskbar client coordinates are authoritative).
        let ppt_dst = if embedded {
            std::ptr::null()
        } else {
            &POINT { x, y }
        };
        let psize = SIZE { cx: w, cy: h };
        let ppt_src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ulw = UpdateLayeredWindow(hwnd, 0, ppt_dst, &psize, hdc, &ppt_src, 0, &blend, ULW_ALPHA);
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
// Popup window helpers (mirrors tauri-plugin-multiline-menubar, adapted to
// Windows screen coordinates: origin top-left, y grows downward)
// ---------------------------------------------------------------------------

/// Position a popup window next to the instance's label:
///   * bottom taskbar  -> popup above the item, horizontally centred
///   * top taskbar     -> popup below the item, horizontally centred
///   * left taskbar    -> popup to the right of the item, vertically centred
///   * right taskbar   -> popup to the left of the item, vertically centred
///
/// Clamped to the monitor that contains the item (multi-monitor aware).
fn position_popup_under_item(win: &WebviewWindow, rect: (i32, i32, i32, i32)) {
    let (rx, ry, rw, rh) = rect;
    let outer = win.outer_size().unwrap_or_default();
    let win_w = outer.width as i32;
    let win_h = outer.height as i32;
    if win_w <= 0 || win_h <= 0 {
        return;
    }

    // Monitor containing the item (physical pixels).
    let item_rect = RECT {
        left: rx,
        top: ry,
        right: rx + rw,
        bottom: ry + rh,
    };
    let monitor = unsafe { MonitorFromRect(&item_rect, MONITOR_DEFAULTTONEAREST) };
    let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let (m_left, m_top, m_right, m_bottom) =
        if monitor != 0 && unsafe { GetMonitorInfoW(monitor, &mut mi) } != 0 {
            (
                mi.rcMonitor.left,
                mi.rcMonitor.top,
                mi.rcMonitor.right,
                mi.rcMonitor.bottom,
            )
        } else {
            (
                0,
                0,
                unsafe { GetSystemMetrics(SM_CXSCREEN) },
                unsafe { GetSystemMetrics(SM_CYSCREEN) },
            )
        };
    let m_w = m_right - m_left;
    let m_h = m_bottom - m_top;

    // Which screen edge the taskbar hugs (compare the taskbar's rect against
    // the item's monitor).
    let tb = find_taskbar().rect;
    let tb_w = tb.right - tb.left;
    let tb_h = tb.bottom - tb.top;
    let at_bottom = tb_h > 0 && tb_h < m_h && (tb.bottom - m_bottom).abs() < 16;
    let at_top = tb_h > 0 && tb_h < m_h && (tb.top - m_top).abs() < 16;
    let at_left = tb_w > 0 && tb_w < m_w && (tb.left - m_left).abs() < 16;

    let mut x = rx + rw / 2 - win_w / 2;
    let mut y;
    if at_bottom {
        // Popup above the item; flip below when there is no room.
        y = ry - win_h;
        if y < m_top {
            y = ry + rh;
        }
    } else if at_top {
        y = ry + rh;
        if y + win_h > m_bottom {
            y = ry - win_h;
        }
    } else if at_left {
        x = rx + rw;
        y = ry + rh / 2 - win_h / 2;
    } else {
        // Right-side taskbar (or unknown): popup to the left of the item.
        x = rx - win_w;
        y = ry + rh / 2 - win_h / 2;
    }
    x = x.clamp(m_left, (m_right - win_w).max(m_left));
    y = y.clamp(m_top, (m_bottom - win_h).max(m_top));
    let _ = win.set_position(PhysicalPosition::new(x as f64, y as f64));
}

fn popup_label() -> Arc<str> {
    POPUP_WINDOW
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| Arc::from("popup"))
}

/// Show the popup window anchored next to the given instance and tell the
/// popup window which instance opened it and what its current state is.
fn open_popup_window(app: &tauri::AppHandle<Wry>, id: &str) -> crate::Result<()> {
    let label = popup_label();
    let Some(win) = app.get_webview_window(label.as_ref()) else {
        return Ok(());
    };
    let rect = instance_rect_screen(id).unwrap_or((0, 0, 0, 0));
    position_popup_under_item(&win, rect);
    attach_auto_hide(app, &win, label.as_ref());
    *POPUP_IGNORE_BLUR_UNTIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) =
        Some(Instant::now() + Duration::from_millis(200));
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit(
        format!("multiline-taskband://{id}//popup-open").as_str(),
        serde_json::json!({ "id": id, "window": label }),
    );

    // Send the instance's current state to the popup window so it can pre-fill
    // its form with the values of whichever instance opened it.
    if let Some(state) = instance_state_json(id) {
        let _ = app.emit_to(&label, POPUP_OPEN_TARGET_EVENT, state);
    }
    *ACTIVE_POPUP_ID.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
    Ok(())
}

/// Hide the popup window.
fn close_popup_window(app: &tauri::AppHandle<Wry>, id: &str) -> crate::Result<()> {
    let label = popup_label();
    if let Some(win) = app.get_webview_window(label.as_ref()) {
        let _ = win.hide();
        let _ = app.emit(
            format!("multiline-taskband://{id}//popup-close").as_str(),
            serde_json::json!({ "id": id, "window": label }),
        );
    }
    *ACTIVE_POPUP_ID.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

/// Toggle the popup window's visibility, anchored next to the given instance.
fn toggle_popup_window(app: &tauri::AppHandle<Wry>, id: &str) -> crate::Result<()> {
    let label = popup_label();
    let Some(win) = app.get_webview_window(label.as_ref()) else {
        return Ok(());
    };
    let active_is_this = ACTIVE_POPUP_ID
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_deref()
        == Some(id);
    if win.is_visible().unwrap_or(false) && active_is_this {
        return close_popup_window(app, id);
    }
    open_popup_window(app, id)
}

/// Close the popup window when it loses focus (popup-app behaviour).
/// Attached once to the popup window.
fn attach_auto_hide(app: &tauri::AppHandle<Wry>, win: &WebviewWindow, label: &str) {
    let mut attached = POPUP_HANDLER_ATTACHED
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if *attached {
        return;
    }
    *attached = true;

    let app = app.clone();
    let label = label.to_string();
    // NOTE: tauri's `on_window_event` returns `()` and offers no API to remove
    // the listener, but the listener lives in a window-scoped handler map that
    // tauri drops when the window is destroyed, so the `app` clone captured
    // here is released then too — nothing leaks for the app's lifetime.
    win.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            let now = Instant::now();
            let ignore = POPUP_IGNORE_BLUR_UNTIL
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(until) = *ignore {
                if now < until {
                    return;
                }
            }
            if let Some(w) = app.get_webview_window(&label) {
                if w.is_visible().unwrap_or(false) {
                    let _ = w.hide();
                    let id = ACTIVE_POPUP_ID
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                        .unwrap_or_else(|| label.clone());
                    let _ = app.emit(
                        format!("multiline-taskband://{id}//popup-close").as_str(),
                        serde_json::json!({ "id": id, "window": label }),
                    );
                    *ACTIVE_POPUP_ID.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
            }
        }
    });
}

/// Snapshot of an instance's full style state, delivered to the popup window
/// when it opens so the form can pre-fill (mirrors the menubar plugin's
/// remembered per-instance state).
fn instance_state_json(id: &str) -> Option<serde_json::Value> {
    let map = INSTANCES.get()?.lock().ok()?;
    let inst = map.get(id)?;
    let top_color = serde_json::to_value(&inst.top_color).ok();
    let bottom_color = serde_json::to_value(&inst.bottom_color).ok();
    Some(serde_json::json!({
        "id": id,
        "top": inst.top,
        "bottom": inst.bottom,
        "topSize": inst.top_size,
        "bottomSize": inst.bottom_size,
        "layout": inst.layout,
        "topColor": top_color,
        "bottomColor": bottom_color,
        "topBold": inst.top_bold,
        "bottomBold": inst.bottom_bold,
        "topAlign": inst.top_align,
        "bottomAlign": inst.bottom_align,
    }))
}

// ---------------------------------------------------------------------------
// Right-click context menu (tauri::menu, mirrored from menubar's set_menu)
// ---------------------------------------------------------------------------

/// Build a per-instance context menu. Menu-item ids are `{instance}::{action}`
/// so the single global menu-event handler can route selections back to the
/// right instance.
fn build_menu(
    app: &tauri::AppHandle<Wry>,
    inst_id: &str,
    items: &[MenuItemDescriptor],
) -> crate::Result<TauriMenu<Wry>> {
    let menu = TauriMenu::with_id(app, inst_id)?;
    for item in items {
        match item {
            MenuItemDescriptor::Item { id, text, enabled } => {
                let full_id = format!("{inst_id}{MENU_ID_SEPARATOR}{id}");
                let mi = TauriMenuItem::with_id(app, &full_id, text, enabled.unwrap_or(true), None::<&str>)?;
                menu.append(&mi)?;
            }
            MenuItemDescriptor::Separator => {
                let sep = TauriPredefined::separator(app)?;
                menu.append(&sep)?;
            }
        }
    }
    Ok(menu)
}

/// Install the global menu-event listener once. Selections from any instance's
/// menu arrive here (tauri bridges muda events) and are re-emitted as
/// `multiline-taskband://{instance}//menu` with `{ id, itemId }`.
fn install_menu_event_handler(app: &tauri::AppHandle<Wry>) {
    let mut attached = MENU_HANDLER_ATTACHED
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if *attached {
        return;
    }
    *attached = true;

    let app = app.clone();
    app.on_menu_event(move |app, event: MenuEvent| {
        let id = event.id().0.clone();
        let Some((inst, action)) = id.split_once(MENU_ID_SEPARATOR) else {
            return; // not one of ours
        };
        let _ = app.emit(
            format!("multiline-taskband://{inst}//menu").as_str(),
            serde_json::json!({ "id": inst, "itemId": action }),
        );
    });
}

/// Pop the instance's context menu at the current cursor position (a
/// right-click always happens with the cursor over the item). The owner window
/// is the popup window when registered, otherwise any available webview
/// window.
///
/// NOTE: we intentionally use `popup()` (cursor position) instead of
/// `popup_at()` with an explicit position: muda's Windows implementation
/// runs the given position through `ClientToScreen` (treating it as a client
/// coordinate), so a screen coordinate would be double-translated and the
/// menu would appear offset from the cursor.
fn show_instance_menu(app: &tauri::AppHandle<Wry>, id: &str) {
    let menus = MENUS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(menu) = menus.as_ref().and_then(|m| m.get(id)) else {
        return;
    };
    let label = popup_label();
    let win = app
        .get_webview_window(label.as_ref())
        .or_else(|| app.webview_windows().into_values().next());
    let Some(win) = win else {
        return;
    };
    // `popup` needs a `Window`; `WebviewWindow` derefs to `Webview`, which
    // exposes `window()`.
    let window = win.as_ref().window();
    let _ = menu.popup(window);
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
