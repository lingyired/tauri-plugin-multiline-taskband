const COMMANDS: &[&str] = &[
    "create",
    "remove",
    "set_text",
    "set_font_sizes",
    "set_font_family",
    "set_padding",
    "set_colors",
    "set_bold",
    "set_alignment",
    "set_visible",
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
