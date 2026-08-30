const COMMANDS: &[&str] = &[
    "create",
    "remove",
    "set_text",
    "set_font_sizes",
    "set_font_family",
    "set_padding",
    "set_side",
    "set_order",
    "set_margin",
    "set_edge_margins",
    "set_colors",
    "set_bold",
    "set_alignment",
    "set_visible",
    "set_line_visible",
    "rect",
    "is_visible",
    "set_popup_window",
    "set_auto_popup",
    "open_popup",
    "close_popup",
    "toggle_popup",
    "set_menu",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
