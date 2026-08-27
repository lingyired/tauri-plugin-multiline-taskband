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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRequest {
    pub id: String,
    /// 0 = emphasis-bottom (default, small label on top / large value below),
    /// 1 = emphasis-top (the vertical mirror), 2 = equal (both lines share one
    /// size, vertically centered & symmetric).
    pub layout: i32,
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
/// render bold, overriding the weight `layout` would otherwise derive. `false`
/// leaves the line's weight to the layout.
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibilityResponse {
    pub visible: bool,
}
