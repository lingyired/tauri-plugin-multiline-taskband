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
//!   * A second hook on `EVENT_OBJECT_CREATE..EVENT_OBJECT_DESTROY` recovers
//!     from explorer.exe restarts: the taskbar window tree is rebuilt, so the
//!     hook triggers a relayout which re-finds `Shell_TrayWnd` and re-embeds
//!     every instance (`ensure_embedded`). This mirrors TrafficMonitor's
//!     `TaskbarCreated` handling (destroy + recreate the embedded window).
//!   * If embedding fails (e.g. explorer is in an odd state), instances fall
//!     back to top-level `WS_EX_TOPMOST` windows; the keep-on-top timer then
//!     re-asserts their z-order above the taskbar (pre-Win11-embedding
//!     behaviour).
//!
//! Text rendering follows **TrafficMonitor's approach** exactly (see
//! `docs/debug-text-rendering-clip.md` for the history):
//!   * Every line band is the font's **full cell height** (tmHeight) — the
//!     `tmInternalLeading` is never trimmed.
//!   * Text is drawn with **`DT_VCENTER`** (TrafficMonitor's GDI path uses
//!     `DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX`; its DirectWrite path uses
//!     paragraph centre), so GDI centres the line and nothing is clipped.
//!   * White-on-black rendering keeps the per-subpixel ClearType coverage and
//!     the final colour is premultiplied by it — the same premultiplied
//!     pixels TrafficMonitor's D2D path hands to `UpdateLayeredWindow`.
//!   * Default font size is 9pt, TrafficMonitor's taskbar default, which fits
//!     the full two-line cell stack inside the taskbar at 96dpi.
//!   * A 3 s paint retry inside the keep-on-top timer re-issues every
//!     window's `UpdateLayeredWindow` — a compositor safety net for display
//!     drivers (e.g. Parallels) that can drop a layered surface even though
//!     ULW reported success. Each attempt succeeds independently, so retries
//!     converge to all overlays visible within a few seconds.
//!
//! NOTE: this module can only be compiled for `cfg(target_os = "windows")`.
//! It is written against `windows-sys` 0.52 and has **not** been run on a
//! Windows machine from this repo yet — see README.md for the verification
//! checklist before shipping.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tauri::menu::{
    CheckMenuItem as TauriCheckMenuItem, ContextMenu as TauriContextMenu, Menu as TauriMenu,
    MenuItem as TauriMenuItem, MenuEvent, MenuItemKind as TauriMenuItemKind,
    PredefinedMenuItem as TauriPredefined, SubmenuBuilder as TauriSubmenuBuilder,
};
use tauri::{Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent, Wry};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
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

/// `DT_VCENTER` — TrafficMonitor draws every label with
/// `DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX` (`CDrawCommon::DrawWindowText`),
/// letting GDI vertically centre the line inside its band. We mirror that
/// exactly (see `render_line`); manual `DT_TOP` + baseline arithmetic is what
/// clipped glyphs in earlier experiments.
const DT_VCENTER: u32 = 0x00000004;

// ---------------------------------------------------------------------------
// Cross-thread plumbing
// ---------------------------------------------------------------------------

const WM_APP_COMMAND: u32 = WM_APP + 1;

enum UiCommand {
    Create { id: String, side: Side },
    Remove { id: String },
    SetText { id: String, top: String, bottom: String },
    SetFontSizes { id: String, top: f64, bottom: f64 },
    SetFontFamily { id: String, top: Option<String>, bottom: Option<String> },
    SetPadding { id: String, left: i32, right: i32 },
    SetSide { id: String, side: Side },
    SetOrder { id: String, order: u64 },
    SetMargin { margin: i32 },
    SetEdgeMargins { left: Option<i32>, right: Option<i32> },
    SetColors { id: String, top: ColorStyle, bottom: ColorStyle },
    SetBold { id: String, top: bool, bottom: bool },
    SetAlignment { id: String, top: i32, bottom: i32 },
    SetVisible { id: String, visible: bool },
    SetLineVisible { id: String, top: bool, bottom: bool },
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
    /// Font family per line; `None` = system default font.
    top_face: Option<String>,
    bottom_face: Option<String>,
    /// Horizontal padding in physical pixels (gap between window edge and text).
    pad_left: i32,
    pad_right: i32,
    top_color: ColorStyle,
    bottom_color: ColorStyle,
    top_bold: bool,
    bottom_bold: bool,
    top_align: i32,
    bottom_align: i32,
    visible: bool,
    /// Per-line visibility. Hiding one line shrinks the window to the other
    /// (recentred in the taskbar); hiding both is equivalent to `visible ==
    /// false` for rendering/layout, but leaves the `visible` flag untouched.
    top_visible: bool,
    bottom_visible: bool,
    /// True when the window has been embedded as a child of the taskbar
    /// (`SetParent`). Embedded children are painted in taskbar client
    /// coordinates and never need z-order maintenance.
    embedded: bool,
}

impl Default for Inst {
    fn default() -> Self {
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
            // Both lines share one default size; per-line sizes are the only
            // way to tune the vertical emphasis (no layout presets).
            // 9pt matches TrafficMonitor's default taskbar font size — at 9pt
            // the full cell height (tmHeight ≈ 16-17px @96dpi) fits inside the
            // taskbar, which is what keeps the two-line layout clip-free.
            top_size: 9.0,
            bottom_size: 9.0,
            top_face: None,
            bottom_face: None,
            pad_left: PAD,
            pad_right: PAD,
            top_color: ColorStyle::Default,
            bottom_color: ColorStyle::Default,
            top_bold: false,
            bottom_bold: false,
            top_align: 0,
            bottom_align: 0,
            visible: true,
            top_visible: true,
            bottom_visible: true,
            embedded: false,
        }
    }
}

/// Whether the instance should be on screen and reserve layout space at all:
/// the instance-level switch AND at least one visible line. With both lines
/// hidden the instance behaves exactly like a hidden one (window hidden, no
/// slot in the layout) while `visible` itself keeps its value, so re-showing
/// either line restores it without another `set_visible` call.
fn effective_visible(inst: &Inst) -> bool {
    inst.visible && (inst.top_visible || inst.bottom_visible)
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

pub fn set_font_family(
    id: String,
    top: Option<String>,
    bottom: Option<String>,
) -> crate::Result<()> {
    start_if_needed();
    // Defensive: reject embedded NULs (they would truncate `lfFaceName`),
    // mirroring the menubar plugin's defensive style.
    for face in [&top, &bottom].into_iter().flatten() {
        if face.contains('\0') {
            return Err(crate::Error::InvalidArgument(
                "font family must not contain NUL".into(),
            ));
        }
    }
    post(UiCommand::SetFontFamily { id, top, bottom });
    Ok(())
}

/// `""` means the system font (reset), matching the menubar plugin's
/// `null`/`""` semantics. Stored as `None` so rendering can branch on
/// `Option::is_none` and `as_deref()` cleanly.
fn normalize_face(face: Option<String>) -> Option<String> {
    face.filter(|s| !s.is_empty())
}

pub fn set_padding(id: String, left: i32, right: i32) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetPadding { id, left, right });
    Ok(())
}

pub fn set_side(id: String, side: Side) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetSide { id, side });
    Ok(())
}

pub fn set_order(id: String, order: u64) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetOrder { id, order });
    Ok(())
}

pub fn set_margin(margin: i32) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetMargin { margin });
    Ok(())
}

pub fn set_edge_margins(left: Option<i32>, right: Option<i32>) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetEdgeMargins { left, right });
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

pub fn set_line_visible(id: String, top: bool, bottom: bool) -> crate::Result<()> {
    start_if_needed();
    post(UiCommand::SetLineVisible { id, top, bottom });
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
/// with `{ id, itemId }`, plus `checked` for `check` items.
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
///
/// Also acts as a safety net for explorer restarts: if an instance ended up
/// off the taskbar's horizontal range (e.g. `TrayNotifyWnd` reported a bogus
/// rect while the tray was rebuilding), a relayout is triggered to recompute
/// positions — by then the taskbar is fully built and the tray edge is valid.
fn keep_on_top() {
    // Compositor safety net (observed with the Parallels display driver):
    // `UpdateLayeredWindow` can report success while the surface never
    // reaches the screen for a given window. Each attempt succeeds
    // independently, so periodically re-issuing the paint makes every overlay
    // converge to visible within a few seconds. Cheap: a few small DIBs every
    // 3 s; hidden instances stay hidden (paint_inst re-applies SW_HIDE).
    const PAINT_RETRY_EVERY_TICKS: u32 = 6; // 6 × 500 ms = 3 s
    static PAINT_RETRY_TICK: AtomicU32 = AtomicU32::new(0);
    if PAINT_RETRY_TICK.fetch_add(1, Ordering::Relaxed) % PAINT_RETRY_EVERY_TICKS == 0 {
        paint_all();
    }
    let taskbar = *TASKBAR_HWND.lock().unwrap_or_else(|e| e.into_inner());
    if taskbar == 0 {
        return;
    }
    // The 500 ms timer doubles as a cheap theme watcher: when the system
    // light/dark setting flips, repaint every instance so `default`-coloured
    // lines follow the taskbar (light → dark text, dark → white text).
    let light = taskbar_light_theme();
    let mut last = LAST_LIGHT_THEME.lock().unwrap_or_else(|e| e.into_inner());
    if *last != Some(light) {
        *last = Some(light);
        drop(last);
        paint_all();
    }
    if let Some(map) = INSTANCES.get() {
        let guard = map.lock().unwrap_or_else(|e| e.into_inner());
        let mut tb_rect = rect_zero();
        if unsafe { GetWindowRect(taskbar, &mut tb_rect) } == 0 {
            return;
        }
        // Right-side instances must sit left of the tray; if the tray is
        // present and any right instance overlaps it, its position was
        // computed before the tray was ready (fallback edge). Recompute now
        // that the tray is stable — this is the safety net that catches a
        // stale `TrayNotifyWnd` rect during explorer restarts.
        let notify = find_window_ex(taskbar, "TrayNotifyWnd");
        if notify != 0 {
            let mut nr = rect_zero();
            if unsafe { GetWindowRect(notify, &mut nr) } != 0
                && nr.left > tb_rect.left
                && nr.left < tb_rect.right
            {
                for inst in guard.values() {
                    if inst.side != Side::Right || !effective_visible(inst) {
                        continue;
                    }
                    let mut r = rect_zero();
                    if unsafe { GetWindowRect(inst.hwnd, &mut r) } != 0 && r.right > nr.left {
                        drop(guard);
                        relayout_all();
                        return;
                    }
                }
            }
        }
        for inst in guard.values() {
            if !effective_visible(inst) {
                continue;
            }
            // Off-taskbar check (horizontal only — left/right edges are the
            // only ones derived from an external window rect): if an instance
            // sits outside the taskbar's x-range, something computed a bad
            // position (stale tray rect during an explorer restart). Force a
            // relayout, which will recompute against the now-stable taskbar.
            let mut r = rect_zero();
            if unsafe { GetWindowRect(inst.hwnd, &mut r) } == 0 {
                continue;
            }
            if r.left < tb_rect.left - 2 || r.right > tb_rect.right + 2 {
                drop(guard);
                relayout_all();
                return;
            }
            if inst.embedded {
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
        UiCommand::SetFontFamily { id, top, bottom } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_face = normalize_face(top);
                inst.bottom_face = normalize_face(bottom);
            }
            drop(guard);
            // Changing the font changes the text width, so neighbours must be
            // re-measured and re-spaced — a plain repaint would leave the
            // window width stale.
            relayout_all();
        }
        UiCommand::SetPadding { id, left, right } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.pad_left = left;
                inst.pad_right = right;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetSide { id, side } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.side = side;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetOrder { id, order } => {
            if let Some(inst) = guard.get_mut(&id) {
                inst.order = order;
            }
            drop(guard);
            relayout_all();
        }
        UiCommand::SetMargin { margin } => {
            *GLOBAL_MARGIN.lock().unwrap_or_else(|e| e.into_inner()) = margin.max(0);
            drop(guard);
            relayout_all();
        }
        UiCommand::SetEdgeMargins { left, right } => {
            {
                let mut em = GLOBAL_EDGE_MARGINS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(l) = left {
                    em.left = l.max(0);
                }
                if let Some(r) = right {
                    em.right = r.max(0);
                }
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
            // Relayout so hidden instances stop reserving space (their
            // neighbours fill the gap) and re-shown ones take a slot again.
            relayout_all();
        }
        UiCommand::SetLineVisible { id, top, bottom } => {
            let mut hwnd = 0;
            let mut now_hidden = false;
            if let Some(inst) = guard.get_mut(&id) {
                inst.top_visible = top;
                inst.bottom_visible = bottom;
                hwnd = inst.hwnd;
                now_hidden = !effective_visible(inst);
            }
            drop(guard);
            // With both lines hidden the instance is fully hidden; relayout
            // skips it (no slot), so the window must be hidden here or it
            // would keep showing stale content at its old size.
            if hwnd != 0 && now_hidden {
                unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
            // The window size and layout slot both depend on how many lines
            // are visible, so always relayout (which repaints shown ones).
            relayout_all();
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

/// Runtime wide (UTF-16 + NUL) string for registry paths that don't fit the
/// fixed 32-char `wide_const`.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether the taskbar uses the light theme. Mirrors TrafficMonitor's
/// `CWindowsSettingHelper::CheckWindows10LightTheme`: reads
/// `SystemUsesLightTheme` under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
///
/// This is what the `default` text colour keys off. `GetSysColor(COLOR_BTNTEXT)`
/// cannot be trusted on Win11 — it stays black even when the taskbar is dark,
/// which makes `default` text invisible on a dark taskbar.
fn taskbar_light_theme() -> bool {
    let mut key: HKEY = 0;
    let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
    let ok = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_READ, &mut key)
    };
    if ok != 0 {
        return true; // default to light on any failure
    }
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegQueryValueExW(
            key,
            wide("SystemUsesLightTheme").as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut u8,
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    r == 0 && data != 0
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

/// Vertical gap between the two text lines *inside* an instance. This is a
/// fixed internal style and is not exposed; the configurable spacing lives in
/// `GLOBAL_MARGIN` below.
const LINE_GAP: i32 = 4;

/// Per-instance horizontal padding (window edge to text), the `set_padding`
/// default. Exposed per-instance via `set_padding`.
const PAD: i32 = 4;

/// Default spacing between adjacent instances; overridable at runtime with
/// `set_margin` (physical pixels).
const DEFAULT_MARGIN: i32 = 4;

/// Global spacing between adjacent instances (physical px). Read by the
/// layout pass; the UI thread owns it but it can be touched from the command
/// handler through the `SetMargin` UiCommand, so a plain static is fine.
static GLOBAL_MARGIN: Mutex<i32> = Mutex::new(DEFAULT_MARGIN);

/// Extra gap (physical px) between the taskbar's left edge and the first
/// left-side instance (`left`), and between the notification area and the
/// first right-side instance (`right`). Both default to `0`; set at runtime
/// with `set_edge_margins` to dodge other tools embedded in the taskbar
/// (e.g. TrafficMonitor). Horizontal taskbars only.
#[derive(Clone, Copy)]
struct EdgeMargins {
    left: i32,
    right: i32,
}

static GLOBAL_EDGE_MARGINS: Mutex<EdgeMargins> =
    Mutex::new(EdgeMargins { left: 0, right: 0 });

fn edge_margins() -> EdgeMargins {
    *GLOBAL_EDGE_MARGINS.lock().unwrap_or_else(|e| e.into_inner())
}

fn margin() -> i32 {
    *GLOBAL_MARGIN.lock().unwrap_or_else(|e| e.into_inner())
}

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

    // Explorer restart recovery: when the taskbar window tree is torn down
    // (`Shell_TrayWnd` destroyed), our SetParent-embedded overlay windows are
    // destroyed along with it — a parent destroys its children on `DestroyWindow`
    // regardless of their style. The WinEvent hook on `EVENT_OBJECT_CREATE`
    // triggers this relayout, which must *recreate* any window whose HWND is
    // gone before re-embedding it into the new taskbar — mirroring
    // TrafficMonitor's destroy+recreate on the `TaskbarCreated` broadcast.
    for (id, inst) in guard.iter_mut() {
        if !window_alive(inst) {
            if let Some(hwnd) = create_window(id) {
                inst.hwnd = hwnd;
                inst.embedded = false;
                if effective_visible(inst) {
                    unsafe { ShowWindow(hwnd, SW_SHOW) };
                }
            }
        }
    }

    // Self-heal visibility: while the taskbar rebuilds its child tree it can
    // hide our embedded windows; a surviving window whose effective visibility
    // says it should be shown gets re-shown on every relayout. Instances with
    // both lines hidden (or `visible == false`) stay hidden.
    for inst in guard.values() {
        if effective_visible(inst) && unsafe { IsWindowVisible(inst.hwnd) } == 0 {
            unsafe { ShowWindow(inst.hwnd, SW_SHOW) };
        }
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
        let m = margin();
        let em = edge_margins();
        // Right side: stack leftward from the notification area, leaving the
        // user's right edge margin before it.
        let mut cursor = taskbar.right_edge_for_right() - em.right;
        for id in &right_ids {
            if let Some(inst) = guard.get_mut(id) {
                if !effective_visible(inst) {
                    continue; // hidden instances do not reserve space
                }
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                cursor -= w;
                inst.x = cursor;
                inst.y = center_y(h);
                cursor -= m;
                paint_inst(inst);
            }
        }
        // Left side: stack rightward from the taskbar's left edge, leaving
        // the user's left edge margin after it.
        let mut cursor = taskbar.left_edge_for_left() + em.left;
        for id in &left_ids {
            if let Some(inst) = guard.get_mut(id) {
                if !effective_visible(inst) {
                    continue;
                }
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = cursor;
                inst.y = center_y(h);
                cursor += w + m;
                paint_inst(inst);
            }
        }
    } else {
        // Vertical taskbar (left/right of screen): stack downward, centred.
        let m = margin();
        let mut cursor_y = taskbar.rect.top + m;
        for id in &right_ids {
            if let Some(inst) = guard.get_mut(id) {
                if !effective_visible(inst) {
                    continue;
                }
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = taskbar.rect.left
                    + (taskbar.rect.right - taskbar.rect.left - w) / 2;
                inst.y = cursor_y;
                cursor_y += h + m;
                paint_inst(inst);
            }
        }
        let mut cursor_y = taskbar.rect.top + m;
        for id in &left_ids {
            if let Some(inst) = guard.get_mut(id) {
                if !effective_visible(inst) {
                    continue;
                }
                let (w, h) = measure(inst);
                inst.w = w;
                inst.h = h;
                inst.x = taskbar.rect.left
                    + (taskbar.rect.right - taskbar.rect.left - w) / 2;
                inst.y = cursor_y;
                cursor_y += h + m;
                paint_inst(inst);
            }
        }
    }
}

/// Whether `inst.hwnd` still refers to one of our overlay windows. `IsWindow`
/// alone is not enough: after the taskbar tears down and recreates windows,
/// the kernel may reuse an old HWND value for a *different* window. Checking
/// the class name pins it down.
fn window_alive(inst: &Inst) -> bool {
    if unsafe { IsWindow(inst.hwnd) } == 0 {
        return false;
    }
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(inst.hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return false;
    }
    String::from_utf16_lossy(&buf[..n as usize]) == "MultilineTaskbandOverlay"
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
                // Guard against a stale / not-yet-positioned tray window:
                // while explorer restarts, `TrayNotifyWnd` can briefly report
                // a (0,0,..) rect, which would push every right-side instance
                // off-screen. Only trust the tray edge when it actually sits
                // inside the taskbar.
                if r.left > self.rect.left && r.left < self.rect.right {
                    return r.left - 2;
                }
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

/// Measure an instance's window size from its **visible** lines (no drawing).
/// Hidden lines contribute nothing, so with one line hidden the window
/// shrinks to the other line and `relayout_all`'s `center_y` re-centres it
/// vertically in the taskbar.
fn measure(inst: &Inst) -> (i32, i32) {
    let d = dpi();
    let mut w = inst.pad_left + inst.pad_right;
    let mut h = 1;
    if inst.top_visible {
        let (tw, th) = measure_text(
            &inst.top,
            pt_to_px(inst.top_size, d),
            inst.top_bold,
            inst.top_face.as_deref(),
        );
        w = w.max(tw + inst.pad_left + inst.pad_right);
        h = th;
    }
    if inst.bottom_visible {
        let (bw, bh) = measure_text(
            &inst.bottom,
            pt_to_px(inst.bottom_size, d),
            inst.bottom_bold,
            inst.bottom_face.as_deref(),
        );
        w = w.max(bw + inst.pad_left + inst.pad_right);
        h = if inst.top_visible { h + LINE_GAP + bh } else { bh };
    }
    (w.max(1), h.max(1))
}

fn measure_text(text: &str, size_px: i32, bold: bool, face: Option<&str>) -> (i32, i32) {
    let hdc = unsafe { CreateCompatibleDC(0) };
    if hdc == 0 {
        return ((text.len() as i32 * size_px).max(1), size_px.max(1));
    }
    let effective_face = face.or_else(|| default_face());
    let font = make_font(size_px, bold, effective_face.as_deref(), DEFAULT_QUALITY);
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
    // Height = the font's full cell height (`r.bottom`, i.e. tmHeight).
    // TrafficMonitor never trims `tmInternalLeading` away: every line band is
    // the full cell and the text is vertically centred inside it with
    // `DT_VCENTER`, so the glyph ink is always fully contained. Trimming the
    // leading is exactly what used to clip descenders (the rect got shorter
    // than the cell while `DT_TOP` anchored the baseline near the top).
    let w = r.right.max(1);
    let h = r.bottom.max(1);
    unsafe {
        SelectObject(hdc, old);
        DeleteObject(font);
        DeleteDC(hdc);
    }
    (w, h)
}

/// Render a single line **the way TrafficMonitor does** (GDI path,
/// `CDrawCommon::DrawWindowText`):
///
/// 1. The DIB is the font's **full cell height** (`full_h`) and the DrawText
///    rect spans the whole cell (`[0, full_h)`). TrafficMonitor never trims
///    `tmInternalLeading` — the leading is part of the cell and keeps the
///    glyph ink away from the band edges.
/// 2. Text is drawn with **`DT_VCENTER`** (TrafficMonitor uses
///    `DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX`; its DirectWrite path uses
///    paragraph centre for the same reason). GDI positions the line inside
///    the band, so no manual baseline arithmetic — the `DT_TOP` + leading
///    trim experiments in `docs/debug-text-rendering-clip.md` are what
///    clipped glyph bottoms.
/// 3. Text is drawn white on black, so each RGB channel of the result IS the
///    per-subpixel ClearType coverage (the channels differ under ClearType).
///    The raw coverage is returned instead of averaging it to grayscale —
///    averaging is what made our text look softer than TrafficMonitor's.
///    `blit_line` premultiplies the final colour by this per-channel coverage
///    (the same premultiplied pixels TrafficMonitor's D2D path produces).
fn render_line(text: &str, size_px: i32, bold: bool, face: Option<&str>) -> (i32, i32, Vec<u32>) {
    let hdc = unsafe { CreateCompatibleDC(0) };
    let effective_face = face.or_else(|| default_face());
    let font = make_font(size_px, bold, effective_face.as_deref(), DEFAULT_QUALITY);
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
    let full_h = r.bottom.max(1);

    // Full cell height, full rect — nothing to clip (see function docs).
    let (hbmp, bits) = create_dib(hdc, w, full_h);
    let old_bmp = unsafe { SelectObject(hdc, hbmp) };
    // Fill opaque black, then draw white text.
    let bits_u32 = bits as *mut u32;
    for i in 0..(w * full_h) as usize {
        unsafe { bits_u32.add(i).write(0xFF00_0000) };
    }
    unsafe {
        SetTextColor(hdc, rgb_val(255, 255, 255));
        let mut dr = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: full_h,
        };
        // DT_VCENTER, not DT_TOP — with a full-height rect GDI centres the
        // line and nothing is clipped. Horizontal alignment is applied later,
        // when the line is blitted into the window.
        DrawTextW(
            hdc,
            wtext.as_ptr(),
            -1,
            &mut dr,
            DT_SINGLELINE | DT_NOPREFIX | DT_VCENTER,
        );
    }

    // Copy the raw white-on-black pixels: RGB = per-channel ClearType
    // coverage (GDI never touches the DIB's alpha channel, which stays at the
    // 0xFF we wrote above — the coverage lives in the colour channels).
    let wu = w as usize;
    let hu = full_h as usize;
    let mut coverage = vec![0u32; wu * hu];
    for i in 0..(wu * hu) {
        coverage[i] = unsafe { *bits_u32.add(i) };
    }

    unsafe {
        SelectObject(hdc, old_bmp);
        DeleteObject(hbmp);
        SelectObject(hdc, old);
        DeleteObject(font);
        DeleteDC(hdc);
    }
    (w, full_h, coverage)
}

/// Return a locale-aware default font family name, or `None` to fall back
/// to GDI's system default.  The candidate list is ordered by visual quality
/// for CJK text on Windows 10/11; each entry is probed with
/// `EnumFontFamiliesExW` so unknown names silently skip.
///
/// This mirrors TrafficMonitor's approach: its language-pack `.ini` files map
/// `DEFAULT_FONT` → `"微软雅黑"` (Simplified Chinese), `"Microsoft JhengHei"`
/// (Traditional), `"Segoe UI"` (everything else).  We go one step further by
/// checking actual font availability at runtime.
static FONT_ENUM_FOUND: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn font_enum_cb(
    _lfntm: *const LOGFONTW,
    _tm: *const TEXTMETRICW,
    _ftype: u32,
    _lparam: isize,
) -> i32 {
    FONT_ENUM_FOUND.store(true, Ordering::Relaxed);
    0 // stop enumerating
}

fn default_face() -> Option<&'static str> {
    // Ordered by preference: best CJK rendering first, then generic UI font.
    // Both English and localized names are tried because GDI's face-name
    // matching is locale-sensitive on some Windows builds.
    static CANDIDATES: &[&str] = &[
        "Microsoft YaHei",
        "Microsoft YaHei UI",
        "微软雅黑",
        "Microsoft JhengHei",
        "Microsoft JhengHei UI",
        "微軟正黑體",
        "Segoe UI",
    ];

    let hdc = unsafe { CreateCompatibleDC(0) };
    if hdc == 0 {
        return None;
    }

    for &name in CANDIDATES {
        let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
        lf.lfCharSet = DEFAULT_CHARSET;
        let wide = to_wide(name);
        let n = wide.len().min((LF_FACESIZE - 1) as usize);
        lf.lfFaceName[..n].copy_from_slice(&wide[..n]);
        lf.lfFaceName[n] = 0;

        FONT_ENUM_FOUND.store(false, Ordering::Relaxed);
        unsafe { EnumFontFamiliesExW(hdc, &lf, Some(font_enum_cb), 0, 0) };
        if FONT_ENUM_FOUND.load(Ordering::Relaxed) {
            unsafe { DeleteDC(hdc) };
            return Some(name);
        }
    }
    unsafe { DeleteDC(hdc) };
    None
}

fn make_font(size_px: i32, bold: bool, face: Option<&str>, quality: u8) -> HFONT {
    let weight: i32 = if bold { FW_BOLD as i32 } else { FW_NORMAL as i32 };
    // `LF_FACESIZE` is 32 u16s: at most 31 characters plus the NUL terminator.
    // All-zero (the default below) selects the system default font. Over-long
    // names are truncated to 31 chars without panicking, and unknown names
    // silently fall back to the system font via GDI — both matching the
    // menubar plugin's behaviour.
    let mut face_name = [0u16; 32];
    if let Some(face) = face {
        let wide = to_wide(face); // always NUL-terminated
        let n = wide.len().min(31);
        face_name[..n].copy_from_slice(&wide[..n]);
    }
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
        lfQuality: quality,
        // `DEFAULT_PITCH | FF_SWISS` matches TrafficMonitor's `FontInfo::Create`
        // (variable-width sans-serif; only a hint once a face name is given).
        lfPitchAndFamily: DEFAULT_PITCH | FF_SWISS,
        lfFaceName: face_name,
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
            // The taskbar's default text colour: light taskbar → dark text,
            // dark taskbar → white text. (GetSysColor(COLOR_BTNTEXT) stays
            // black on Win11 regardless of theme — see taskbar_light_theme.)
            if taskbar_light_theme() {
                (0, 0, 0)
            } else {
                (255, 255, 255)
            }
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

/// Last observed taskbar theme (light/dark), so the keep_on_top timer can
/// detect flips and repaint `default`-coloured instances.
static LAST_LIGHT_THEME: Mutex<Option<bool>> = Mutex::new(None);

/// Repaint every instance (used when the system theme flips so `default`
/// text colour follows the taskbar's appearance).
fn paint_all() {
    if let Some(map) = INSTANCES.get() {
        let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
        for inst in guard.values_mut() {
            paint_inst(inst);
        }
    }
}

fn paint_inst(inst: &mut Inst) {
    let d = dpi();
    let top_sz = pt_to_px(inst.top_size, d);
    let bot_sz = pt_to_px(inst.bottom_size, d);
    // Only visible lines are rendered — hidden ones contribute nothing to the
    // bitmap (and `measure` has already shrunk the window accordingly).
    let top = if inst.top_visible {
        Some(render_line(
            &inst.top,
            top_sz,
            inst.top_bold,
            inst.top_face.as_deref(),
        ))
    } else {
        None
    };
    let bot = if inst.bottom_visible {
        Some(render_line(
            &inst.bottom,
            bot_sz,
            inst.bottom_bold,
            inst.bottom_face.as_deref(),
        ))
    } else {
        None
    };

    let w = inst.w.max(1);
    let h = inst.h.max(1);

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
    // TrafficMonitor's D2D path fills the same alpha=1 background
    // (`FillRect(draw_rect, 0x00000000, 1)`).
    for i in 0..(w * h) as usize {
        unsafe { bits_u32.add(i).write(0x0100_0000) };
    }

    // Top band, anchored at y=0. When the bottom line is hidden too, this is
    // the only band and fills the single-line window.
    if let Some((tw, th, top_cov)) = &top {
        let top_x = align_x(inst.top_align, *tw, w, inst.pad_left, inst.pad_right);
        blit_line(
            bits_u32,
            w as usize,
            h as usize,
            0,
            top_x as usize,
            *tw as usize,
            *th as usize,
            top_cov,
            tr,
            tg,
            tb,
        );
    }
    // Bottom band: right below the top band (plus the fixed internal gap)
    // when both lines show, else at y=0 so the single remaining line sits in
    // the shrunken window that `center_y` keeps vertically centred.
    if let Some((bw, bh, bot_cov)) = &bot {
        let bot_y = match &top {
            Some((_, th, _)) => (*th + LINE_GAP) as usize,
            None => 0,
        };
        let bot_x = align_x(inst.bottom_align, *bw, w, inst.pad_left, inst.pad_right);
        blit_line(
            bits_u32,
            w as usize,
            h as usize,
            bot_y,
            bot_x as usize,
            *bw as usize,
            *bh as usize,
            bot_cov,
            br,
            bg,
            bb,
        );
    }

    let hwnd = inst.hwnd;
    let x = inst.x;
    let y = inst.y;
    let visible = effective_visible(inst);
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
fn align_x(align: i32, line_w: i32, win_w: i32, pad_left: i32, pad_right: i32) -> i32 {
    if align == 2 {
        (win_w - line_w - pad_right).max(0)
    } else if align == 1 {
        ((win_w - line_w) / 2).max(0)
    } else {
        pad_left
    }
}

/// Composite one line's white-on-black coverage into the final bitmap at
/// (off_x, off_y). Each pixel's RGB channels are the per-subpixel ClearType
/// coverage; the final colour is premultiplied by that per-channel coverage
/// and the pixel's alpha is the average coverage — the premultiplied format
/// `UpdateLayeredWindow` (AC_SRC_ALPHA) expects, and the same pixels
/// TrafficMonitor's D2D path produces.
#[allow(clippy::too_many_arguments)] // win32-style blit signature
fn blit_line(
    dst: *mut u32,
    win_w: usize,
    _win_h: usize,
    off_y: usize,
    off_x: usize,
    line_w: usize,
    line_h: usize,
    coverage: &[u32],
    r: u8,
    g: u8,
    b: u8,
) {
    for ly in 0..line_h {
        let dy = off_y + ly;
        for lx in 0..line_w {
            let px = coverage[ly * line_w + lx];
            let cr = ((px >> 16) & 0xFF) as u32;
            let cg = ((px >> 8) & 0xFF) as u32;
            let cb = (px & 0xFF) as u32;
            let a = ((cr + cg + cb) / 3) as u8;
            if a == 0 {
                continue;
            }
            let dx = off_x + lx;
            let idx = dy * win_w + dx;
            let ar = (r as u32 * cr) / 255;
            let ag = (g as u32 * cg) / 255;
            let ab = (b as u32 * cb) / 255;
            unsafe {
                dst.add(idx)
                    .write((a as u32) << 24 | ab | (ag << 8) | (ar << 16));
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
        "topFontFamily": inst.top_face,
        "bottomFontFamily": inst.bottom_face,
        "leftPadding": inst.pad_left,
        "rightPadding": inst.pad_right,
        "side": inst.side,
        "order": inst.order,
        "topColor": top_color,
        "bottomColor": bottom_color,
        "topBold": inst.top_bold,
        "bottomBold": inst.bottom_bold,
        "topAlign": inst.top_align,
        "bottomAlign": inst.bottom_align,
        "topVisible": inst.top_visible,
        "bottomVisible": inst.bottom_visible,
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
    append_menu_items(app, inst_id, items, &menu)?;
    Ok(menu)
}

/// Append a descriptor tree to `menu`, recursing into submenus.
fn append_menu_items(
    app: &tauri::AppHandle<Wry>,
    inst_id: &str,
    items: &[MenuItemDescriptor],
    menu: &TauriMenu<Wry>,
) -> crate::Result<()> {
    for item in items {
        match item {
            MenuItemDescriptor::Item {
                id,
                text,
                accelerator,
                enabled,
            } => {
                let full_id = format!("{inst_id}{MENU_ID_SEPARATOR}{id}");
                let mi = TauriMenuItem::with_id(
                    app,
                    &full_id,
                    text,
                    enabled.unwrap_or(true),
                    accelerator.as_deref(),
                )?;
                menu.append(&mi)?;
            }
            MenuItemDescriptor::Check {
                id,
                text,
                checked,
                accelerator,
            } => {
                let full_id = format!("{inst_id}{MENU_ID_SEPARATOR}{id}");
                let mi = TauriCheckMenuItem::with_id(
                    app,
                    &full_id,
                    text,
                    true,
                    checked.unwrap_or(false),
                    accelerator.as_deref(),
                )?;
                menu.append(&mi)?;
            }
            MenuItemDescriptor::Separator => {
                let sep = TauriPredefined::separator(app)?;
                menu.append(&sep)?;
            }
            MenuItemDescriptor::Submenu { text, items } => {
                let sub = build_submenu(app, inst_id, text, items)?;
                menu.append(&sub)?;
            }
        }
    }
    Ok(())
}

/// Build a nested submenu, recursing with the same `{instance}::{action}` id
/// scheme so selections inside it are routed like any other item.
fn build_submenu(
    app: &tauri::AppHandle<Wry>,
    inst_id: &str,
    text: &str,
    items: &[MenuItemDescriptor],
) -> crate::Result<tauri::menu::Submenu<Wry>> {
    let mut builder = TauriSubmenuBuilder::new(app, text);
    for item in items {
        match item {
            MenuItemDescriptor::Item {
                id,
                text,
                accelerator,
                enabled,
            } => {
                let full_id = format!("{inst_id}{MENU_ID_SEPARATOR}{id}");
                let mi = TauriMenuItem::with_id(
                    app,
                    &full_id,
                    text,
                    enabled.unwrap_or(true),
                    accelerator.as_deref(),
                )?;
                builder = builder.item(&mi);
            }
            MenuItemDescriptor::Check {
                id,
                text,
                checked,
                accelerator,
            } => {
                let full_id = format!("{inst_id}{MENU_ID_SEPARATOR}{id}");
                let mi = TauriCheckMenuItem::with_id(
                    app,
                    &full_id,
                    text,
                    true,
                    checked.unwrap_or(false),
                    accelerator.as_deref(),
                )?;
                builder = builder.item(&mi);
            }
            MenuItemDescriptor::Separator => {
                builder = builder.separator();
            }
            MenuItemDescriptor::Submenu { text, items } => {
                let sub = build_submenu(app, inst_id, text, items)?;
                builder = builder.item(&sub);
            }
        }
    }
    Ok(builder.build()?)
}

/// Current checked state of a check item, or `None` if the action id does not
/// belong to a check item (or the menu is gone).
fn menu_item_checked(inst: &str, action: &str) -> Option<bool> {
    fn walk(items: &[TauriMenuItemKind<Wry>], full_id: &str) -> Option<bool> {
        for item in items {
            match item {
                TauriMenuItemKind::Check(check) if check.id().0 == full_id => {
                    return check.is_checked().ok();
                }
                TauriMenuItemKind::Submenu(sub) => {
                    if let Ok(children) = sub.items() {
                        if let Some(found) = walk(&children, full_id) {
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    let menus = MENUS.lock().ok()?;
    let menu = menus.as_ref()?.get(inst)?;
    let items = menu.items().ok()?;
    walk(&items, &format!("{inst}{MENU_ID_SEPARATOR}{action}"))
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
        // `checked` is only present for `check` items; by the time the event
        // fires the native layer has already toggled the new state.
        let payload = match menu_item_checked(inst, action) {
            Some(checked) => {
                serde_json::json!({ "id": inst, "itemId": action, "checked": checked })
            }
            None => serde_json::json!({ "id": inst, "itemId": action }),
        };
        let _ = app.emit(
            format!("multiline-taskband://{inst}//menu").as_str(),
            payload,
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
        // Taskbar move/resize → re-layout every instance.
        SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            0,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        // Taskbar destroy/create → explorer.exe restart. The taskbar window
        // tree is torn down and rebuilt, so our embedded children must be
        // re-embedded into the new `Shell_TrayWnd` (same approach as
        // TrafficMonitor's `TaskbarCreated` handling, but via WinEvents since
        // this plugin has no top-level window to receive the broadcast).
        // Registered as a range so both `EVENT_OBJECT_DESTROY` (0x8001) and
        // `EVENT_OBJECT_CREATE` (0x8000) reach the same callback.
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_DESTROY,
            0,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
}

/// Whether `hwnd` is a (primary or secondary) taskbar window. Class-name
/// based so it stays correct after explorer restarts, when the cached
/// `TASKBAR_HWND` points at a destroyed window.
fn is_taskbar_class(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&buf[..n as usize]);
    name == "Shell_TrayWnd" || name == "Shell_SecondaryTrayWnd"
}

/// Whether `hwnd` is the taskbar's notification/tray area (`TrayNotifyWnd`).
/// Its creation is the signal that the right-side edge has stabilised: right
/// instances anchored to a bogus fallback edge (tray not yet created) must be
/// recomputed once the tray window actually exists.
fn is_tray_class(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return false;
    }
    String::from_utf16_lossy(&buf[..n as usize]) == "TrayNotifyWnd"
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
    if event == EVENT_OBJECT_CREATE || event == EVENT_OBJECT_DESTROY {
        // Explorer restart: relayout re-finds the taskbar (new window) and
        // re-embeds every instance via `ensure_embedded`. `hwnd==0` here
        // means a desktop-wide event, which never carries a taskbar class;
        // the class check below already excludes it. `TrayNotifyWnd`
        // creation is included: right-side instances anchored to the fallback
        // edge (tray not yet present) get recomputed the moment the tray
        // window exists, before the 500 ms keep_on_top safety net fires.
        if hwnd != 0 && (is_taskbar_class(hwnd) || is_tray_class(hwnd)) {
            post(UiCommand::Relayout);
        }
        return;
    }
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
