import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type Side = 'left' | 'right'

export interface CreateOptions {
  id: string
  /** Edge to pin to. Defaults to `'right'` (left of the notification area). */
  side?: Side
  top?: string
  bottom?: string
}

export interface IdOptions {
  id: string
}

export interface SetTextOptions {
  id: string
  top: string
  bottom: string
}

export interface FontSizesOptions {
  id: string
  top: number
  bottom: number
}

/**
 * Set the font family of the top and/or bottom line of an instance. Each line
 * is independent: pass `null` (or `''`) for a line to reset it to the system
 * default font. Unknown family names fall back silently, matching the menubar
 * plugin's semantics.
 */
export interface SetFontFamilyOptions {
  id: string
  /** Font family for the top line, or `null`/`''` for the system font. */
  top: string | null
  /** Font family for the bottom line, or `null`/`''` for the system font. */
  bottom: string | null
}

/**
 * Per-instance horizontal padding, in physical pixels. The gap between the
 * left/right edge of the instance window and its text; `left` and `right`
 * can differ. Defaults to `4`.
 */
export interface SetPaddingOptions {
  id: string
  left: number
  right: number
}

/**
 * Move an existing instance to the other side of the taskbar (left/right)
 * without recreating it. Its creation order is preserved, so within the new
 * side it keeps its relative position.
 */
export interface SetSideOptions {
  id: string
  /** Edge to pin to: `'left'` or `'right'`. */
  side: Side
}

/**
 * Re-order an instance within its side. Instances on the same side are laid
 * out by ascending `order` (creation order by default); use the neighbours'
 * current values (e.g. swap with an adjacent instance) to move it up/down.
 */
export interface SetOrderOptions {
  id: string
  /** Sort key within the instance's side. */
  order: number
}

/**
 * Set the global margin between adjacent instances, in physical pixels.
 * The gap between the two text lines *inside* an instance is a fixed
 * internal style and is not affected. Defaults to `4`.
 */
export interface SetMarginOptions {
  /** Margin between instances, in physical pixels. */
  margin: number
}

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export interface VisibilityResult {
  visible: boolean
}

/** How a taskbar line should be painted. */
export type ColorStyle =
  | { type: 'default' }
  | { type: 'solid'; value: string }

export interface SetColorsOptions {
  id: string
  /** Paint for the top line. */
  top: ColorStyle
  /** Paint for the bottom line. */
  bottom: ColorStyle
}

/** Per-line bold toggle for the two taskbar lines. */
export interface SetBoldOptions {
  id: string
  top: boolean
  bottom: boolean
}

/**
 * Per-line horizontal alignment for the two taskbar lines.
 * Each field is `0` (left, default), `1` (center) or `2` (right).
 */
export interface SetAlignmentOptions {
  id: string
  top: number
  bottom: number
}

export interface SetVisibleOptions {
  id: string
  visible: boolean
}

/** Select which Tauri webview window is used as the settings popup. */
export interface PopupWindowOptions {
  /** Window label (as registered in `tauri.conf.json`). */
  label: string
}

/** Enable/disable automatically toggling the popup on left click. */
export interface SetAutoPopupOptions {
  enabled: boolean
}

/**
 * Emitted when the user clicks an instance's label on the taskbar. Payload
 * mirrors Tauri's own `TrayIconEvent::Click`: `button` is `'left'` or
 * `'right'`, `buttonState` is `'down'` (the overlay fires on mouse-down).
 */
export interface ClickEvent {
  id: string
  position: { x: number; y: number }
  rect: Rect
  button: 'left' | 'right'
  buttonState: 'up' | 'down'
}

/** Emitted when the settings popup opens/closes for an instance. */
export interface PopupEvent {
  id: string
  window: string
}

/**
 * A right-click context-menu item descriptor. Mirrors the menubar plugin's
 * type one-for-one, so the same menu tree works on both platforms: `item`
 * (plain action), `check` (toggle), `separator` and `submenu` (nested tree).
 * `item` and `check` selections are reported back through
 * {@link onMenuSelection}.
 */
export type MenuItemDescriptor =
  | {
      type: 'item'
      id: string
      text: string
      accelerator?: string
      /** Whether the item is clickable. Defaults to `true`. */
      enabled?: boolean
    }
  | {
      type: 'check'
      id: string
      text: string
      /** Initial checked state. Defaults to `false`. */
      checked?: boolean
      accelerator?: string
    }
  | { type: 'separator' }
  | { type: 'submenu'; text: string; items: MenuItemDescriptor[] }

/** Attach/detach the right-click context menu of an instance. */
export interface SetMenuOptions {
  id: string
  /** Menu items. Omit or pass `null` to detach the menu. */
  items?: MenuItemDescriptor[] | null
}

/** Emitted when an item in an instance's right-click menu is selected. */
export interface MenuSelectionEvent {
  /** The taskbar instance the menu belongs to. */
  id: string
  /** The `id` of the selected menu item. */
  itemId: string
  /** Present only for `check` items: the state after the toggle. */
  checked?: boolean
}

export interface ReadyEvent {
  id: string
}

// ---------------------------------------------------------------------------
// Event name helpers
// ---------------------------------------------------------------------------

export function eventName(id: string, name: string): string {
  return `multiline-taskband://${id}//${name}`
}

export const EVENT_READY = (id: string) => eventName(id, 'ready')
export const EVENT_CLICK = (id: string) => eventName(id, 'click')
export const EVENT_POPUP_OPEN = (id: string) => eventName(id, 'popup-open')
export const EVENT_POPUP_CLOSE = (id: string) => eventName(id, 'popup-close')
export const EVENT_MENU = (id: string) => eventName(id, 'menu')

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export async function create(options: CreateOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|create', { payload: options })
}

export async function remove(options: IdOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|remove', { payload: options })
}

export async function setText(options: SetTextOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_text', { payload: options })
}

export async function setFontSizes(options: FontSizesOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_font_sizes', {
    payload: options,
  })
}

/**
 * Set the font family of the top and/or bottom line of an instance. Pass
 * `null`/`''` for a line to reset it to the system font. See
 * {@link SetFontFamilyOptions}.
 */
export async function setFontFamily(options: SetFontFamilyOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_font_family', {
    payload: options,
  })
}

/**
 * Set the horizontal padding (physical px) of an instance. See
 * {@link SetPaddingOptions}.
 */
export async function setPadding(options: SetPaddingOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_padding', { payload: options })
}

/**
 * Move an existing instance to the other side of the taskbar (left/right)
 * without recreating it. See {@link SetSideOptions}.
 */
export async function setSide(options: SetSideOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_side', { payload: options })
}

/**
 * Re-order an instance within its side. See {@link SetOrderOptions}.
 */
export async function setOrder(options: SetOrderOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_order', { payload: options })
}

/**
 * Set the global margin (physical px) between adjacent instances. See
 * {@link SetMarginOptions}.
 */
export async function setMargin(options: SetMarginOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_margin', { payload: options })
}

/**
 * Set the text paint for the top and bottom lines of an instance. Each line
 * accepts a {@link ColorStyle}: `default` (system colour, follows light/dark
 * mode) or `solid` (`#rrggbb`).
 */
export async function setColors(options: SetColorsOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_colors', { payload: options })
}

/**
 * Force the top and/or bottom line bold.
 */
export async function setBold(options: SetBoldOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_bold', { payload: options })
}

/**
 * Set the horizontal alignment of the top and/or bottom line.
 */
export async function setAlignment(options: SetAlignmentOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_alignment', { payload: options })
}

export async function setVisible(options: SetVisibleOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_visible', { payload: options })
}

/** Returns the on-screen rectangle of an instance in physical pixels. */
export async function rect(options: IdOptions): Promise<Rect> {
  return await invoke<Rect>('plugin:multiline-taskband|rect', { payload: options })
}

export async function isVisible(options: IdOptions): Promise<boolean> {
  const result = await invoke<VisibilityResult>(
    'plugin:multiline-taskband|is_visible',
    { payload: options }
  )
  return result.visible
}

/**
 * Set which Tauri window is used as the settings popup. Call before the first
 * open. The window must already exist (e.g. declared in `tauri.conf.json`
 * with `visible: false`); the plugin positions it next to the clicked
 * instance, shows it and hides it on focus loss.
 */
export async function setPopupWindow(options: PopupWindowOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_popup_window', {
    payload: options,
  })
}

/**
 * Enable/disable automatically toggling the popup when an instance is
 * left-clicked. Defaults to `true`.
 */
export async function setAutoPopup(options: SetAutoPopupOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_auto_popup', {
    payload: options,
  })
}

/** Show the popup window anchored next to the given instance. */
export async function openPopup(options: IdOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|open_popup', { payload: options })
}

/** Hide the popup window. */
export async function closePopup(options: IdOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|close_popup', { payload: options })
}

/** Toggle the popup window's visibility, anchored next to the given instance. */
export async function togglePopup(options: IdOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|toggle_popup', { payload: options })
}

/**
 * Subscribe to clicks on one instance's taskbar label. Returns the usual Tauri
 * unlisten function.
 */
export async function onClick(
  id: string,
  handler: (event: ClickEvent) => void
): Promise<UnlistenFn> {
  return await listen<ClickEvent>(EVENT_CLICK(id), (e) => handler(e.payload))
}

/**
 * Subscribe to the settings popup opening for one instance. Returns the usual
 * Tauri unlisten function.
 */
export async function onPopupOpen(
  id: string,
  handler: (event: PopupEvent) => void
): Promise<UnlistenFn> {
  return await listen<PopupEvent>(EVENT_POPUP_OPEN(id), (e) => handler(e.payload))
}

/**
 * Subscribe to the settings popup closing for one instance. Returns the usual
 * Tauri unlisten function.
 */
export async function onPopupClose(
  id: string,
  handler: (event: PopupEvent) => void
): Promise<UnlistenFn> {
  return await listen<PopupEvent>(EVENT_POPUP_CLOSE(id), (e) => handler(e.payload))
}

/**
 * Attach a right-click context menu to an instance, shown at the mouse
 * position. Pass `items: null` (or omit it) to detach the menu, mirroring
 * Tauri's `setMenu(null)` semantics.
 *
 * The menu is a real Tauri/muda menu built on the Rust side. Listen for
 * selections with {@link onMenuSelection}.
 */
export async function setMenu(options: SetMenuOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_menu', { payload: options })
}

/**
 * Subscribe to right-click context-menu selections for one instance. Returns
 * the usual Tauri unlisten function.
 *
 * ```ts
 * await onMenuSelection('right-1', (e) => {
 *   if (e.itemId === 'quit') exit()
 * })
 * ```
 */
export async function onMenuSelection(
  id: string,
  handler: (event: MenuSelectionEvent) => void
): Promise<UnlistenFn> {
  return await listen<MenuSelectionEvent>(EVENT_MENU(id), (e) =>
    handler(e.payload)
  )
}

/**
 * Subscribe to the `ready` event for one instance (fired after its overlay
 * window has been created on the taskbar). Returns the usual Tauri unlisten
 * function.
 */
export async function onReady(
  id: string,
  handler: (event: ReadyEvent) => void
): Promise<UnlistenFn> {
  return await listen<ReadyEvent>(EVENT_READY(id), (e) => handler(e.payload))
}
