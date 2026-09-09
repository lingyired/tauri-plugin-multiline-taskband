use serde::{Deserialize, Serialize};

/// Which edge of the taskbar an instance is pinned to.
///
/// `left` anchors just to the right of the Start button; `right` anchors just
/// to the left of the notification (tray) area. Multiple instances on the same
/// side are laid out in creation order, growing inward from the edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    #[default]
    Right,
}

/// On-screen rectangle of a taskbar item, in physical screen pixels
/// (origin top-left, y increasing downward). Mirrors Tauri's `Rect`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub id: String,
    /// Edge to pin to. Defaults to `right`.
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub top: Option<String>,
    #[serde(default)]
    pub bottom: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTextRequest {
    pub id: String,
    pub top: String,
    pub bottom: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontSizesRequest {
    pub id: String,
    /// Top-line font size in points.
    pub top: f64,
    /// Bottom-line font size in points.
    pub bottom: f64,
}

/// Per-line font family override. Each line is independent: `None`/`""`
/// means the system default font (reset), mirroring the menubar plugin's
/// `set_font_family` semantics so the same frontend code works on both.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFontFamilyRequest {
    pub id: String,
    /// Font family for the top line; `None`/`""` = system default.
    #[serde(default)]
    pub top: Option<String>,
    /// Font family for the bottom line; `None`/`""` = system default.
    #[serde(default)]
    pub bottom: Option<String>,
}

/// Per-instance horizontal padding, in physical pixels.
///
/// The gap between the left/right edge of the instance window and its text.
/// Defaults to the plugin's built-in `4` px; `left` and `right` can differ.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPaddingRequest {
    pub id: String,
    /// Left padding in physical pixels.
    pub left: i32,
    /// Right padding in physical pixels.
    pub right: i32,
}

/// Move an existing instance to the other side of the taskbar (left/right)
/// without recreating it. Its creation order is preserved, so within the
/// new side it keeps its relative position among same-side instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSideRequest {
    pub id: String,
    /// Edge to pin to: `"left"` or `"right"`.
    pub side: Side,
}

/// Re-order an existing instance relative to its side peers.
///
/// Instances on the same side are laid out by ascending `order` (creation
/// order by default). Setting `order` re-positions the instance within that
/// sequence; instances on different sides do not interleave.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetOrderRequest {
    pub id: String,
    /// Sort key within the instance's side. Any integer works; using the
    /// current neighbours' values (e.g. swap with an adjacent instance) is
    /// the usual way to move an instance up/down.
    pub order: u64,
}

/// Set the global margin between adjacent instances, in physical pixels.
///
/// This is the spacing between separate taskbar items (both horizontally on
/// a bottom/top taskbar and vertically on a side taskbar). The gap between
/// the two text lines *inside* an instance is a fixed internal style and is
/// not affected. Defaults to `4`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMarginRequest {
    /// Margin between instances, in physical pixels.
    pub margin: i32,
}

/// Set extra edge margins for the left/right instance groups, in physical
/// pixels.
///
/// `left` is the extra gap between the taskbar's left edge and the first
/// left-side instance (shifting the whole left group rightward); `right` is
/// the extra gap between the notification area and the first right-side
/// instance (shifting the whole right group leftward). Useful for dodging
/// other tools embedded in the taskbar, such as TrafficMonitor. Both default
/// to `0` and are clamped to `>= 0`. Horizontal taskbars only — ignored on
/// vertical taskbars. A field set to `None` (omitted) keeps the current
/// value.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetEdgeMarginsRequest {
    /// Extra gap (physical px) after the taskbar's left edge for the
    /// left-side group. `None` keeps the current value.
    #[serde(default)]
    pub left: Option<i32>,
    /// Extra gap (physical px) before the notification area for the
    /// right-side group. `None` keeps the current value.
    #[serde(default)]
    pub right: Option<i32>,
}

/// How the text of a taskbar line should be painted.
///
/// `default` keeps the system `textColor` (follows light/dark mode). `solid`
/// uses a single hex color (e.g. `"#FF4F44"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ColorStyle {
    Default,
    Solid { value: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetColorsRequest {
    pub id: String,
    pub top: ColorStyle,
    pub bottom: ColorStyle,
}

/// Per-line bold toggle for the two taskbar lines.
///
/// Each line is independent: `top`/`bottom` being `true` forces that line to
/// render bold. `false` renders it with the normal weight.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBoldRequest {
    pub id: String,
    pub top: bool,
    pub bottom: bool,
}

/// Per-line horizontal alignment for the two taskbar lines.
///
/// Each line is independent: `top`/`bottom` is `0` = left (default),
/// `1` = center, `2` = right.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAlignmentRequest {
    pub id: String,
    pub top: i32,
    pub bottom: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVisibleRequest {
    pub id: String,
    pub visible: bool,
}

/// Per-line visibility for the two taskbar lines.
///
/// Each line is independent: `top`/`bottom` being `false` hides that line
/// entirely. With one line hidden the other is centred vertically in the
/// taskbar (the instance window shrinks to a single line). With both hidden
/// the whole instance stops rendering and reserving space — like
/// [`SetVisibleRequest`] with `visible: false` — but the instance-level
/// `visible` flag is unchanged, so showing either line again restores it
/// without another `set_visible` call.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLineVisibleRequest {
    pub id: String,
    pub top: bool,
    pub bottom: bool,
}

/// A leading icon for one taskbar line, rendered from an image file or from
/// inline content.
///
/// The icon is drawn at the start of the line, before the text, and is scaled
/// to the line's full cell height — so it follows both the font size and the
/// system DPI. The width follows the source's aspect ratio.
///
/// Exactly one of `path` / `data` must be provided; see [`IconSpec::is_valid`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconSpec {
    /// Path to an image file. SVG is rasterised with `resvg`; PNG / ICO / BMP
    /// go through the `image` crate. Mutually exclusive with `data`.
    #[serde(default)]
    pub path: Option<String>,
    /// Inline content: SVG source text, a base64 string, or a
    /// `data:<mime>;base64,<payload>` URL. Mutually exclusive with `path`.
    #[serde(default)]
    pub data: Option<String>,
    /// Paint the icon with the line's own colour instead of its own colours.
    ///
    /// The icon's alpha becomes coverage — exactly what glyph coverage is for
    /// text — so a monochrome icon follows `set_colors` and the taskbar's
    /// light/dark theme instead of being stuck at whatever colour the asset
    /// was authored with.
    #[serde(default)]
    pub tint: bool,
    /// Display height in physical pixels. Only used by the instance-level
    /// column icon ([`SetLeadingIconRequest`]); per-line icons ignore it and
    /// always scale to their line's cell height. `None` = the full block
    /// height (the icon spans the whole two-line instance); an explicit value
    /// is clamped to `8 ..= block height`.
    #[serde(default)]
    pub size: Option<i32>,
}

impl IconSpec {
    /// `path` and `data` are mutually exclusive: exactly one must be `Some`.
    pub fn is_valid(&self) -> bool {
        matches!(
            (self.path.as_deref(), self.data.as_deref()),
            (Some(_), None) | (None, Some(_))
        )
    }
}

/// Per-line leading icon.
///
/// Each line is independent, mirroring `set_font_family` / `set_colors` /
/// `set_alignment`: `None` clears that line's icon and leaves the other one
/// untouched.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetIconRequest {
    pub id: String,
    /// Icon for the top line; `None` clears it.
    #[serde(default)]
    pub top: Option<IconSpec>,
    /// Icon for the bottom line; `None` clears it.
    #[serde(default)]
    pub bottom: Option<IconSpec>,
}

/// The instance-level leading (column) icon — mirrors the menubar plugin's
/// `setLeadingIcon` one-for-one.
///
/// Unlike [`SetIconRequest`], which hangs a small icon off each text line,
/// this renders one large icon in its own column on the left of the instance,
/// vertically centred across the whole block; both text lines start to its
/// right. Per-line icons keep working in the remaining text area. `None`
/// clears the column icon and returns the instance to the plain layout.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLeadingIconRequest {
    pub id: String,
    /// The column icon, or `None` to clear it. See [`IconSpec::size`] for the
    /// size semantics (only honoured here).
    #[serde(default)]
    pub icon: Option<IconSpec>,
}

/// Select which Tauri webview window is used as the settings popup.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPopupWindowRequest {
    /// Window label (as registered in `tauri.conf.json`).
    pub label: String,
}

/// Enable/disable automatically toggling the popup on left click.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutoPopupRequest {
    pub enabled: bool,
}

/// A context-menu item descriptor, shown on right click of an instance.
///
/// Mirrors the menubar plugin's `MenuItemDescriptor` one-for-one, so the same
/// menu tree can be handed to either plugin: `item`, `check`, `separator` and
/// `submenu` are all supported.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MenuItemDescriptor {
    Item {
        id: String,
        text: String,
        #[serde(default)]
        accelerator: Option<String>,
        /// Whether the item is clickable. Defaults to `true`.
        #[serde(default)]
        enabled: Option<bool>,
    },
    Check {
        id: String,
        text: String,
        /// Initial checked state. Defaults to `false`.
        #[serde(default)]
        checked: Option<bool>,
        #[serde(default)]
        accelerator: Option<String>,
    },
    Separator,
    Submenu {
        text: String,
        items: Vec<MenuItemDescriptor>,
    },
}

/// Attach/detach the right-click context menu of an instance. Pass `items:
/// None` to detach, mirroring Tauri's `setMenu(null)` semantics.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMenuRequest {
    pub id: String,
    #[serde(default)]
    pub items: Option<Vec<MenuItemDescriptor>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibilityResponse {
    pub visible: bool,
}
