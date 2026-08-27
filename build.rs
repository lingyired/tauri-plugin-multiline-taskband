const COMMANDS: &[&str] = &[
    "create",
    "remove",
    "set_text",
    "set_font_sizes",
    "set_layout",
    "set_colors",
    "set_bold",
    "set_alignment",
    "set_visible",
    "rect",
    "is_visible",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
