# tauri-plugin-multiline-taskband

A Tauri v2 plugin that renders **two-line text labels on the left and right edges of the Windows taskbar** — the Windows counterpart of `tauri-plugin-multiline-menubar` (which does the same on the macOS menu bar).

It is designed for apps like **fund01** that want to show several groups of holding P&L on the taskbar: create one instance per group, pin some to the `left` edge (next to Start) and some to the `right` edge (next to the tray), each rendering two lines of text (e.g. a group name + the live value).

> **Status:** scaffold complete, Win32 implementation written against `windows-sys` 0.52, **compiled and verified on Windows 11 (build 26200, ARM64)** via `examples/demo`. Windows 10 verification pending — see [Verification checklist](#verification-checklist) below.

## How it works

The approach is borrowed from [TrafficMonitor](https://github.com/zhongyang219/TrafficMonitor)'s **Win11** path (see `Win11TaskbarDlg.cpp`): instead of hacking the taskbar's window tree (which is fragile and effectively impossible on Win11), each instance is its **own top-level layered window** overlaid on the taskbar:

1. `FindWindowExW(0, 0, "Shell_TrayWnd", 0)` locates the taskbar.
2. The **right** edge is anchored just left of `TrayNotifyWnd` (the notification area); the **left** edge just right of the `Start` button.
3. The window is created with `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`:
   - *Layered* + a 32-bit ARGB bitmap drawn via `UpdateLayeredWindow` → true per-pixel alpha (text visible, background fully transparent).
   - **Clickable**: a left click toggles the instance's **settings popup** (a Tauri webview window, see [Per-instance popup](#per-instance-popup)); a right click emits a `click` event the host can use for its own context menu. Only the label's own small rectangle is covered, so the rest of the taskbar keeps receiving clicks normally. **(important)** the bitmap's *background* is rendered as alpha `1`, not `0`: `UpdateLayeredWindow` hit-tests layered windows per-pixel, so any alpha-0 pixel would (click) fall through to the taskbar beneath. alpha 1 is visually indistinguishable from the taskbar but makes the entire label area hit-testable.
4. Text is rendered with GDI `DrawTextW` into a **white-on-black** DIB; because white text on black makes every channel equal the coverage, the red channel doubles as the alpha mask — giving clean anti-aliased edges we can recolour to any target colour (system colour for `default`, or a `#rrggbb` for `solid`).
5. A dedicated **UI thread** owns every window and runs a message pump. All public calls are marshalled to it through an `mpsc` channel + `PostThreadMessageW`, which keeps Win32 object creation on a single thread (required by the API) while Tauri command handlers run on arbitrary async threads.
6. A `SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, …)` on the taskbar re-lays-out every instance when the taskbar moves/resizes (e.g. explorer restart, DPI change, monitor change).

This works identically on **Windows 10 and Windows 11** (both still expose `Shell_TrayWnd`, `Start` and `TrayNotifyWnd`).

## Project layout

```
tauri-plugin-multiline-taskband/
├── Cargo.toml
├── build.rs                      # generates the permission files
├── src/
│   ├── lib.rs                    # plugin entry, command registration
│   ├── models.rs                 # request/response types (Side, ColorStyle, …)
│   ├── commands.rs               # Tauri command handlers
│   ├── error.rs
│   ├── desktop.rs                # shared state + MultilineTaskband struct
│   └── native/
│       └── windows.rs           # ← the Win32 implementation (cfg(windows) only)
├── guest-js/                     # JS/TS API (index.ts, package.json, rollup)
└── permissions/                  # default permission set
```

## Integration

### 1. Add the plugin to your Tauri app

In `src-tauri/Cargo.toml`:

```toml
[dependencies]
tauri-plugin-multiline-taskband = { path = "../tauri-plugin-multiline-taskband" }
```

In `src-tauri/src/lib.rs` (or `main.rs`):

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_multiline_taskband::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### 2. Add the JS/TS API

```sh
pnpm add ../tauri-plugin-multiline-taskband
# or copy dist-js/index.js + index.d.ts into your frontend
```

### 3. Use it (fund01 example)

```ts
import {
  create, setText, setLayout, setColors, setFontSizes, onReady,
} from 'tauri-plugin-multiline-taskband-api'

// A "holding group" on the RIGHT edge, value emphasised below the label.
await create({ id: 'group-a', side: 'right', top: 'A股', bottom: '+1.23%' })
await setLayout({ id: 'group-a', layout: 0 })
await setFontSizes({ id: 'group-a', top: 9, bottom: 13 })
await setColors({ id: 'group-a', top: { type: 'default' }, bottom: { type: 'solid', value: '#FF4F44' } })

// Another group, deeper on the RIGHT edge (stacks leftward automatically).
await create({ id: 'group-b', side: 'right', top: 'QDII', bottom: '-0.40%' })

// A group on the LEFT edge (next to Start).
await create({ id: 'group-c', side: 'left', top: '总收益', bottom: '+5.67%' })

// On each refresh, just push the new numbers; layout/position are handled.
await setText({ id: 'group-a', top: 'A股', bottom: liveA })
```

`rect(id)` returns the on-screen rectangle (physical pixels) if you need to anchor a popup. `isVisible(id)` / `setVisible(id, bool)` toggle an instance. `onReady(id, cb)` fires once the overlay window exists on the taskbar.

### 4. Per-instance popup

Mirroring the macOS plugin, a left click on an instance toggles a **settings popup** window in which the host edits that single instance's text and appearance. The popup is a plain Tauri webview window the host declares in `tauri.conf.json`:

```json
{
  "label": "popup",
  "url": "popup.html",
  "width": 400,
  "height": 640,
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "visible": false,
  "skipTaskbar": true,
  "resizable": false
}
```

The host registers it once at startup and keeps auto-popup on left click enabled (both are the defaults except for the window registration itself):

```ts
import { setPopupWindow, setAutoPopup, onClick } from 'tauri-plugin-multiline-taskband-api'

await setPopupWindow({ label: 'popup' })
await setAutoPopup({ enabled: true })

onClick('group-a', (e) => console.log('clicked', e.button, e.position))
```

When an instance is clicked the plugin positions the popup next to the label (above it on a bottom taskbar, below on a top taskbar, to the side on a vertical one — multi-monitor aware), shows it, and emits `multiline-taskband://popup//open` **to the popup window only**, prefilled with that instance's current state (`top`/`bottom` text, sizes, layout, colors, bold, alignment). The popup's own JS then calls the regular `set_*` commands against that instance id, and the plugin hides the window again on focus loss. Listen per instance with `onPopupOpen(id, cb)` / `onPopupClose(id, cb)`.

See `examples/demo/src/popup.html` + `popup.js` for a full reference implementation.

### 5. Permissions

The plugin ships `permissions/default.toml` granting all commands. Reference it from your app's capability file (`src-tauri/capabilities/default.json`):

```json
{
  "permissions": [
    "multiline-taskband:default"
  ]
}
```

## Verification checklist

Verified on **Windows 11 Pro 22H2+ (build 26200, ARM64)** via `examples/demo`:

- [x] `cargo build` compiles `src/native/windows.rs` (windows-sys 0.52, ARM64 msvc) with no warnings; `cargo clippy` clean.
- [x] Overlay windows appear on the taskbar, left and right, with crisp two-line text.
- [x] Multiple instances on the same side stack without overlapping; order follows creation order.
- [x] Text updates (`set_text`) repaint without flicker; window width tracks the new text.
- [x] **Clicking the taskbar does not hide the overlays** — on Win11 (build 26200) clicking the taskbar raises its z-order above non-activating layered windows, visually hiding them until re-composited. Fixed with a keep-alive timer (`keep_on_top`) that re-asserts `HWND_TOPMOST` only when an overlay ends up stacked below the taskbar (`GetWindow(GW_HWNDPREV)`).
- [x] `left` instances pin to the **far left edge** of the taskbar (Win11's centered layout puts the Start button mid-taskbar; anchoring to the button would push labels toward the centre).
- [x] **Clicking an instance's label opens its settings popup** next to the item (auto-popup on left click; position adapts to bottom/top/vertical taskbar). Popup edits (text/sizes/layout/colors/bold/alignment) apply live to that instance; the popup hides on focus loss. *Verified on Win11 26200 / ARM64 @ 200% DPI — see `C:\Users\lingsmbp\tmp-demo-cli\verify_popup.py` and the screenshots `popup_top.png` / `popup_right3.png`.*
- [x] Dark/light mode: `default` colour tracks the taskbar text colour (`COLOR_BTNTEXT`).
- [ ] Taskbar relocate (bottom↔top, or monitor change) re-lays-out instances via the WinEvent hook — implemented, pending manual test.
- [ ] Explorer restart (`taskkill /f /im explorer.exe && start explorer`) does not leave orphaned windows or crash the app — implemented, pending manual test.
- [ ] Windows 10 verification — pending (needs a Win10 machine/VM).

## Roadmap

- **Per-monitor DPI** (`WM_DPICHANGED`) instead of the system DPI used today.
- **Right-click context menu** on an instance (the `click` event already carries `button: 'right'`; wiring it to a native menu is next).
- **Vertical taskbar** polish (currently a best-effort downward stack).

## License

MIT (or whatever your project uses).
