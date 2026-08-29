# multiline-taskband — API Reference

A Tauri v2 plugin that renders **two-line text labels on the left and right edges of the Windows taskbar** — the Windows counterpart of [`tauri-plugin-multiline-menubar`](https://github.com/lingyired/tauri-plugin-multiline-menubar) (which does the same on the macOS menu bar).

- **Windows 10/11**: fully supported. Each instance is its own top-level layered window overlaid on the taskbar (`Shell_TrayWnd` / `Start` / `TrayNotifyWnd` all exist on both).
- **macOS / Linux**: every command resolves with the `UnsupportedPlatform` error (`"This feature is only supported on Windows"`). Pair this plugin with the menubar plugin to cover both platforms with the same frontend code — the APIs deliberately mirror each other (same function names, same `MenuItemDescriptor`, same font/colour semantics).

Naming conventions: the guest-js functions are `camelCase` and map one-to-one to snake_case raw commands (e.g. `setFontFamily` → `plugin:multiline-taskband|set_font_family`). See [Raw invoke commands](#raw-invoke-commands).

## Quick start

### Rust

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_multiline_taskband::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### Capabilities

```json
{
  "identifier": "default",
  "windows": ["main", "popup"],
  "permissions": [
    "core:default",
    "multiline-taskband:default",
    "multiline-taskband:allow-set-popup-window",
    "multiline-taskband:allow-open-popup",
    "multiline-taskband:allow-close-popup",
    "multiline-taskband:allow-remove",
    "multiline-taskband:allow-set-menu"
  ]
}
```

`multiline-taskband:default` covers the core rendering + read-only queries (see [Permissions](#permissions)); the popup-window control, `remove` and `set_menu` must be granted explicitly.

### JavaScript

```ts
import { create, setText, onReady } from 'tauri-plugin-multiline-taskband-api'

await onReady('group-a', () => console.log('on the taskbar'))
await create({ id: 'group-a', side: 'right', top: 'A股', bottom: '+1.23%' })
await setText({ id: 'group-a', top: 'A股', bottom: liveValue })
```

Without the npm package, raw invoke works too:

```ts
import { invoke } from '@tauri-apps/api/core'

await invoke('plugin:multiline-taskband|set_text', {
  payload: { id: 'group-a', top: 'A股', bottom: '+1.23%' },
})
```

## JavaScript API (guest-js)

All functions are async and return a `Promise`.

### Lifecycle

| Function | Signature | Description |
|---|---|---|
| `create` | `create(options: CreateOptions): Promise<void>` | Create a taskbar instance pinned to `side` (defaults to `'right'`, i.e. left of the tray area; `'left'` pins next to the Start button). Optional `top`/`bottom` set the initial two lines of text. Multiple instances on the same side stack in creation order, growing inward from the edge. |
| `remove` | `remove(options: IdOptions): Promise<void>` | Destroy an instance and its overlay window. |
| `onReady` | `onReady(id: string, handler: (event: ReadyEvent) => void): Promise<UnlistenFn>` | Fired once the instance's overlay window exists on the taskbar. Returns the usual Tauri unlisten function. |

### Content

| Function | Signature | Description |
|---|---|---|
| `setText` | `setText(options: SetTextOptions): Promise<void>` | Replace both lines of text. The window width re-measures automatically. |
| `setFontSizes` | `setFontSizes(options: FontSizesOptions): Promise<void>` | Per-line font size in points (`top` / `bottom`, independent). Both lines default to `9` (TrafficMonitor's taskbar default). |
| `setFontFamily` | `setFontFamily(options: SetFontFamilyOptions): Promise<void>` | Per-line font family. Pass `null` (or `''`) for a line to reset it to the system default font. Unknown family names fall back silently — same semantics as the menubar plugin. |
| `setColors` | `setColors(options: SetColorsOptions): Promise<void>` | Per-line text paint. Each line takes a [ColorStyle](#colorstyle): `default` follows the system colour (tracks light/dark mode), `solid` is a `#rrggbb` value. |
| `setBold` | `setBold(options: SetBoldOptions): Promise<void>` | Force the top and/or bottom line bold (`true` = bold, `false` = normal weight; each line independent). |
| `setAlignment` | `setAlignment(options: SetAlignmentOptions): Promise<void>` | Per-line horizontal alignment: `0` left (default), `1` center, `2` right. See [SetAlignmentOptions](#setalignmentoptions--alignment). |

### Visibility & geometry

| Function | Signature | Description |
|---|---|---|
| `setVisible` | `setVisible(options: SetVisibleOptions): Promise<void>` | Show/hide an instance without destroying it. |
| `isVisible` | `isVisible(options: IdOptions): Promise<boolean>` | Current visibility of an instance. |
| `rect` | `rect(options: IdOptions): Promise<Rect>` | On-screen rectangle of an instance in **physical pixels** (origin top-left, y down). Useful for anchoring your own windows. |
| `setPadding` | `setPadding(options: SetPaddingOptions): Promise<void>` | Per-instance horizontal padding between the window edges and the text, in physical pixels. `left` and `right` can differ; defaults to `4` / `4`. |
| `setMargin` | `setMargin(options: SetMarginOptions): Promise<void>` | **Global** margin between adjacent instances, in physical pixels (no `id`). Defaults to `4`. The gap between the two text lines *inside* an instance is a fixed internal style and is not affected. |
| `setEdgeMargins` | `setEdgeMargins(options: SetEdgeMarginsOptions): Promise<void>` | **Global** extra edge margins, in physical pixels (no `id`): `left` shifts the whole left-side group away from the taskbar's left edge, `right` shifts the whole right-side group away from the notification area — e.g. to dodge other embedded tools like TrafficMonitor. Both default to `0`; omitted fields keep their current value. Horizontal taskbars only. |
| `setSide` | `setSide(options: SetSideOptions): Promise<void>` | Move an existing instance to the other side of the taskbar (`'left'`/`'right'`) without recreating it. Creation order is preserved, so it keeps its relative position within the new side. |
| `setOrder` | `setOrder(options: SetOrderOptions): Promise<void>` | Re-order an instance within its side. Same-side instances are laid out by ascending `order` (creation order by default); use the neighbours' current values (e.g. swap with an adjacent instance) to move it up/down. |

### Context menu

| Function | Signature | Description |
|---|---|---|
| `setMenu` | `setMenu(options: SetMenuOptions): Promise<void>` | Attach a right-click context menu to an instance, shown at the mouse position. Pass `items: null` (or omit it) to detach, mirroring Tauri's `setMenu(null)` semantics. The menu is a real Tauri/muda menu built on the Rust side. |
| `onMenuSelection` | `onMenuSelection(id: string, handler: (event: MenuSelectionEvent) => void): Promise<UnlistenFn>` | Subscribe to selections in the instance's right-click menu. |

Note: with a menu attached, right-clicking an instance shows the native menu; the `click` event (see [Events](#events)) still fires on mouse-down for **both** buttons. Without a menu, the host can use the right-click `click` event to drive its own menu. See [MenuItemDescriptor](#menuitemdescriptor) for the menu tree format — it is identical to the menubar plugin's, so the same menu tree works on both platforms.

Note: `onMenuSelection` fires when the user selects an `item` or `check` entry. For `check` items, `checked` reports the state *after* the native layer toggled it.

### Popup window

| Function | Signature | Description |
|---|---|---|
| `setPopupWindow` | `setPopupWindow(options: PopupWindowOptions): Promise<void>` | Select which Tauri webview window is used as the settings popup (by window label, as registered in `tauri.conf.json`). Call before the first open. The window must already exist (e.g. declared with `visible: false`); the plugin positions it next to the clicked instance and hides it on focus loss. |
| `setAutoPopup` | `setAutoPopup(options: SetAutoPopupOptions): Promise<void>` | Enable/disable automatically toggling the popup on left click. Defaults to `true`. |
| `openPopup` | `openPopup(options: IdOptions): Promise<void>` | Show the popup anchored next to the given instance. |
| `closePopup` | `closePopup(options: IdOptions): Promise<void>` | Hide the popup. |
| `togglePopup` | `togglePopup(options: IdOptions): Promise<void>` | Toggle the popup's visibility, anchored next to the given instance. |
| `onPopupOpen` | `onPopupOpen(id: string, handler: (event: PopupEvent) => void): Promise<UnlistenFn>` | Subscribe to the popup opening for one instance. |
| `onPopupClose` | `onPopupClose(id: string, handler: (event: PopupEvent) => void): Promise<UnlistenFn>` | Subscribe to the popup closing for one instance (also fires when the popup auto-hides on focus loss). |

Note: when the popup opens, the plugin emits `multiline-taskband://{id}//popup-open` app-wide **and** a second event `multiline-taskband://popup//open` **to the popup window only**, carrying a snapshot of that instance's full state so the popup's form can pre-fill (see [InstanceState](#instancestate)). The popup's own JS then calls the regular `set_*` commands against that instance id.

Note: on a bottom taskbar the popup appears above the label; on a top taskbar below it; on a vertical taskbar to the side — multi-monitor aware, clamped to the monitor's work area.

### Events

Event names are namespaced per instance:

```ts
import { eventName, onClick, onReady } from 'tauri-plugin-multiline-taskband-api'
// eventName(id, name) === `multiline-taskband://${id}//${name}`
```

| Event name | Constant | Payload |
|---|---|---|
| `multiline-taskband://{id}//ready` | `EVENT_READY(id)` | [`ReadyEvent`](#readyevent) |
| `multiline-taskband://{id}//click` | `EVENT_CLICK(id)` | [`ClickEvent`](#clickevent) |
| `multiline-taskband://{id}//popup-open` | `EVENT_POPUP_OPEN(id)` | [`PopupEvent`](#popupevent) |
| `multiline-taskband://{id}//popup-close` | `EVENT_POPUP_CLOSE(id)` | [`PopupEvent`](#popupevent) |
| `multiline-taskband://{id}//menu` | `EVENT_MENU(id)` | [`MenuSelectionEvent`](#menuselectionevent) |

The `click` payload mirrors Tauri's own `TrayIconEvent::Click` (`button` is `'left'` or `'right'`; `buttonState` is always `'down'` — the overlay fires on mouse-down). Prefer the typed helpers (`onClick`, `onReady`, …) over raw `listen` calls; they subscribe to the same events.

## Types

```ts
export type Side = 'left' | 'right'

export interface CreateOptions {
  id: string
  side?: Side
  top?: string
  bottom?: string
}

export interface IdOptions { id: string }

export interface SetTextOptions { id: string; top: string; bottom: string }

export interface FontSizesOptions { id: string; top: number; bottom: number }

export interface SetFontFamilyOptions {
  id: string
  top: string | null
  bottom: string | null
}

export interface SetPaddingOptions { id: string; left: number; right: number }

export interface SetSideOptions { id: string; side: Side }

export interface SetOrderOptions { id: string; order: number }

export interface SetMarginOptions { margin: number }

export interface SetEdgeMarginsOptions { left?: number; right?: number }

export type ColorStyle = { type: 'default' } | { type: 'solid'; value: string }

export interface SetColorsOptions { id: string; top: ColorStyle; bottom: ColorStyle }

export interface SetBoldOptions { id: string; top: boolean; bottom: boolean }

export interface SetAlignmentOptions { id: string; top: number; bottom: number }

export interface SetVisibleOptions { id: string; visible: boolean }

export interface PopupWindowOptions { label: string }

export interface SetAutoPopupOptions { enabled: boolean }

export interface SetMenuOptions { id: string; items?: MenuItemDescriptor[] | null }

export interface Rect { x: number; y: number; width: number; height: number }

export interface ReadyEvent { id: string }

export interface PopupEvent { id: string; window: string }

export interface ClickEvent {
  id: string
  position: { x: number; y: number }
  rect: Rect
  button: 'left' | 'right'
  buttonState: 'up' | 'down'
}

export interface MenuSelectionEvent {
  id: string
  itemId: string
  checked?: boolean
}
```

### ColorStyle

How a taskbar line's text is painted:

```ts
type ColorStyle = { type: 'default' } | { type: 'solid'; value: string }
```

- `{ type: 'default' }` — the system taskbar text colour (`COLOR_BTNTEXT`); follows light/dark mode automatically.
- `{ type: 'solid', value: '#FF4F44' }` — a fixed hex colour.

```js
// Red value, system-coloured name:
await setColors({
  id: 'group-a',
  top: { type: 'default' },
  bottom: { type: 'solid', value: '#FF4F44' },
})
```

### SetFontFamilyOptions

Each line is independent; `null` (or `''`) resets that line to the system default font. Unknown family names fall back silently, matching the menubar plugin's `set_font_family` semantics.

```js
// Monospaced digits for the value, system font for the name:
await setFontFamily({ id: 'group-a', top: null, bottom: 'Consolas' })
```

### SetAlignmentOptions & Alignment

`top` / `bottom` are integers: `0` = left (default), `1` = center, `2` = right. Alignment is within the instance window; the window itself is only as wide as the wider line plus padding, so alignment is mostly visible when the two lines differ in width.

```js
await setAlignment({ id: 'group-a', top: 0, bottom: 2 }) // name left, value right
```

### MenuItemDescriptor

A right-click context-menu item descriptor. Mirrors the menubar plugin's type one-for-one, so the same menu tree works on both platforms:

```ts
type MenuItemDescriptor =
  | { type: 'item'; id: string; text: string; accelerator?: string; enabled?: boolean }
  | { type: 'check'; id: string; text: string; checked?: boolean; accelerator?: string }
  | { type: 'separator' }
  | { type: 'submenu'; text: string; items: MenuItemDescriptor[] }
```

- `item` — a plain action; `enabled` defaults to `true`.
- `check` — a toggle; `checked` defaults to `false`.
- `separator` — a visual divider.
- `submenu` — a nested tree (recurse freely).

```js
await setMenu({
  id: 'group-a',
  items: [
    { type: 'item', id: 'edit', text: 'Edit…' },
    { type: 'check', id: 'bold', text: 'Bold', checked: true },
    { type: 'separator' },
    { type: 'submenu', text: 'More', items: [{ type: 'item', id: 'quit', text: 'Quit' }] },
  ],
})
```

### InstanceState

The snapshot delivered to the popup window as `multiline-taskband://popup//open` whenever the popup opens, so its form can pre-fill with the clicked instance's current state:

```ts
interface InstanceState {
  id: string
  top: string
  bottom: string
  topSize: number
  bottomSize: number
  topFontFamily: string | null
  bottomFontFamily: string | null
  leftPadding: number
  rightPadding: number
  side: Side
  order: number
  topColor: ColorStyle
  bottomColor: ColorStyle
  topBold: boolean
  bottomBold: boolean
  topAlign: number
  bottomAlign: number
}
```

See `examples/demo/src/popup.js` for a reference popup that consumes it.

## Raw invoke commands

All guest-js functions are thin wrappers over these. Payloads are wrapped in a single `payload` argument and use `camelCase` keys.

| Command | Payload |
|---|---|
| `plugin:multiline-taskband\|create` | `{ id, side?, top?, bottom? }` |
| `plugin:multiline-taskband\|remove` | `{ id }` |
| `plugin:multiline-taskband\|set_text` | `{ id, top, bottom }` |
| `plugin:multiline-taskband\|set_font_sizes` | `{ id, top, bottom }` |
| `plugin:multiline-taskband\|set_font_family` | `{ id, top, bottom }` (`top`/`bottom`: `string \| null`) |
| `plugin:multiline-taskband\|set_padding` | `{ id, left, right }` |
| `plugin:multiline-taskband\|set_side` | `{ id, side }` |
| `plugin:multiline-taskband\|set_order` | `{ id, order }` |
| `plugin:multiline-taskband\|set_margin` | `{ margin }` |
| `plugin:multiline-taskband\|set_edge_margins` | `{ left?, right? }` |
| `plugin:multiline-taskband\|set_colors` | `{ id, top: ColorStyle, bottom: ColorStyle }` |
| `plugin:multiline-taskband\|set_bold` | `{ id, top, bottom }` |
| `plugin:multiline-taskband\|set_alignment` | `{ id, top, bottom }` |
| `plugin:multiline-taskband\|set_visible` | `{ id, visible }` |
| `plugin:multiline-taskband\|rect` | `{ id }` → `Rect` |
| `plugin:multiline-taskband\|is_visible` | `{ id }` → `{ visible }` |
| `plugin:multiline-taskband\|set_popup_window` | `{ label }` |
| `plugin:multiline-taskband\|set_auto_popup` | `{ enabled }` |
| `plugin:multiline-taskband\|open_popup` | `{ id }` |
| `plugin:multiline-taskband\|close_popup` | `{ id }` |
| `plugin:multiline-taskband\|toggle_popup` | `{ id }` |
| `plugin:multiline-taskband\|set_menu` | `{ id, items?: MenuItemDescriptor[] \| null }` |

`ColorStyle` serialises as a tagged enum: `{ "type": "default" }` or `{ "type": "solid", "value": "#FF4F44" }`.

## Rust API

Everything above is also reachable from Rust through the `MultilineTaskbandExt` trait (implemented for `tauri::App`, `tauri::AppHandle` and `tauri::Window`), which returns the `MultilineTaskband<R>` handle:

```rust
use tauri_plugin_multiline_taskband::{MultilineTaskbandExt, Side, ColorStyle};

app.multiline_taskband().create("group-a".into(), Side::Right)?;
app.multiline_taskband().set_text("group-a".into(), "A股".into(), "+1.23%".into())?;
```

| Trait method | Maps to command |
|---|---|
| `create(id, side)` | `create` |
| `remove(id)` | `remove` |
| `set_text(id, top, bottom)` | `set_text` |
| `set_font_sizes(id, top, bottom)` | `set_font_sizes` |
| `set_font_family(id, top: Option<String>, bottom: Option<String>)` | `set_font_family` |
| `set_padding(id, left, right)` | `set_padding` |
| `set_side(id, side)` | `set_side` |
| `set_order(id, order)` | `set_order` |
| `set_margin(margin)` | `set_margin` |
| `set_edge_margins(left: Option<i32>, right: Option<i32>)` | `set_edge_margins` |
| `set_colors(id, top, bottom)` | `set_colors` |
| `set_bold(id, top, bottom)` | `set_bold` |
| `set_alignment(id, top, bottom)` | `set_alignment` |
| `set_visible(id, visible)` | `set_visible` |
| `rect(id)` | `rect` |
| `is_visible(id)` | `is_visible` |
| `set_popup_window(label)` | `set_popup_window` |
| `set_auto_popup(enabled)` | `set_auto_popup` |
| `open_popup(id)` | `open_popup` |
| `close_popup(id)` | `close_popup` |
| `toggle_popup(id)` | `toggle_popup` |
| `set_menu(id, items: Option<Vec<MenuItemDescriptor>>)` | `set_menu` |

All methods return `crate::Result<T>` (`tauri_plugin_multiline_taskband::Result`). Request/response types (`Side`, `Rect`, `ColorStyle`, `MenuItemDescriptor`, …) are re-exported from the crate root via `pub use models::*`.

## Permissions

The default permission set (`multiline-taskband:default`) covers core rendering + read-only queries:

`allow-create`, `allow-set-text`, `allow-set-font-sizes`, `allow-set-font-family`, `allow-set-padding`, `allow-set-side`, `allow-set-order`, `allow-set-margin`, `allow-set-edge-margins`, `allow-set-colors`, `allow-set-bold`, `allow-set-alignment`, `allow-set-visible`, `allow-rect`, `allow-is-visible`, `allow-set-auto-popup`

High-impact commands are intentionally **not** in the default set — grant them explicitly:

| Permission | Command |
|---|---|
| `multiline-taskband:allow-remove` | `remove` |
| `multiline-taskband:allow-set-menu` | `set_menu` |
| `multiline-taskband:allow-set-popup-window` | `set_popup_window` |
| `multiline-taskband:allow-open-popup` | `open_popup` |
| `multiline-taskband:allow-close-popup` | `close_popup` |
| `multiline-taskband:allow-toggle-popup` | `toggle_popup` |

Each has a matching `multiline-taskband:deny-*` counterpart.

## Errors

All commands serialise errors as strings; the possible variants are:

| Error | Message | Cause |
|---|---|---|
| `UnsupportedPlatform` | `This feature is only supported on Windows` | Any command on macOS/Linux |
| `InstanceNotFound` | `Taskbar instance not found` | `rect` / `is_visible` with an unknown `id` |
| `InvalidArgument` | `Invalid argument: {0}` | e.g. a font family containing an embedded NUL in `set_font_family` |
| `Windows` | `Windows error: {0}` | A Win32 call failed |
| `Tauri` | underlying message | A Tauri-internal operation failed |

## Notes

- Error behaviour is asymmetric by design: the `set_*` commands silently no-op when given an unknown `id` (they are fire-and-forget), while the queries `rect` / `is_visible` reject an unknown `id` with `InstanceNotFound`.
- All pixel values (padding, margin, `rect`) are **physical** pixels, not logical/DIP.
- Font sizes are in **points**; both lines default to `9` (matching TrafficMonitor's taskbar font). The window is sized automatically: width from the wider line plus padding, height from the sum of both lines' full cell heights plus a fixed internal line gap — the `tmInternalLeading` is deliberately not trimmed and each line is vertically centred with `DT_VCENTER`, so glyphs are never clipped (see `docs/debug-text-rendering-clip.md`).
- Instances survive taskbar moves and explorer restarts: a `WinEventHook` on the taskbar re-lays-out every instance when the taskbar moves/resizes (DPI change, monitor change) and recreates overlays if the taskbar window is rebuilt. Clicking the taskbar does not hide the overlays (a keep-alive timer re-asserts topmost z-order when needed).
- `set_margin` is global — it affects every instance, including ones created later. `set_padding` is per-instance.
- `setEdgeMargins` is also global, and is clamped to `>= 0`: `left` offsets the left-side group's stacking start from `taskbar left edge + 2` and `right` offsets the right-side group's stacking start from the tray edge, so larger values push each whole group further into the free taskbar space. Both default to `0`; on vertical taskbars they are ignored.
- The right-edge anchor is the tray/notification area, not the last system icon; the left-edge anchor on Win11 is the far left edge of the taskbar (Win11's centered layout puts the Start button mid-taskbar).
- The plugin registers the popup's auto-hide (focus loss → hide + `popup-close`) the first time it shows the window; it ignores the initial blur that the `show()` itself can cause.
- The popup window is a plain Tauri webview window owned by your app (recommended flags: `decorations: false`, `transparent: true`, `alwaysOnTop: true`, `visible: false`, `skipTaskbar: true`, `resizable: false`). The plugin only positions, shows and hides it.
