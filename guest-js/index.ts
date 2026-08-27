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

export interface LayoutOptions {
  id: string
  /**
   * Vertical layout:
   * - `0` (emphasis-bottom, default): small label on top (light weight), large value below (regular weight).
   * - `1` (emphasis-top): the vertical mirror — large value on top, small label below.
   * - `2` (equal): both lines share one size, vertically centered & symmetric.
   */
  layout: number
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
 * Choose the vertical layout for an instance. See {@link LayoutOptions}.
 */
export async function setLayout(options: LayoutOptions): Promise<void> {
  return await invoke('plugin:multiline-taskband|set_layout', { payload: options })
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
 * Force the top and/or bottom line bold, independently of `layout`.
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
