use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

mod commands;
mod desktop;
mod error;
mod models;
#[cfg(target_os = "windows")]
mod native;

pub use error::{Error, Result};

use desktop::MultilineTaskband;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to
/// access the multiline-taskband APIs.
pub trait MultilineTaskbandExt<R: Runtime> {
    fn multiline_taskband(&self) -> &MultilineTaskband<R>;
}

impl<R: Runtime, T: Manager<R>> crate::MultilineTaskbandExt<R> for T {
    fn multiline_taskband(&self) -> &MultilineTaskband<R> {
        self.state::<MultilineTaskband<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("multiline-taskband")
        .invoke_handler(tauri::generate_handler![
            commands::create,
            commands::remove,
            commands::set_text,
            commands::set_font_sizes,
            commands::set_font_family,
            commands::set_padding,
            commands::set_side,
            commands::set_order,
            commands::set_margin,
            commands::set_edge_margins,
            commands::set_colors,
            commands::set_bold,
            commands::set_alignment,
            commands::set_visible,
            commands::rect,
            commands::is_visible,
            commands::set_popup_window,
            commands::set_auto_popup,
            commands::open_popup,
            commands::close_popup,
            commands::toggle_popup,
            commands::set_menu,
        ])
        .setup(|app, api| {
            let multiline_taskband = desktop::init(app, api)?;
            app.manage(multiline_taskband);
            Ok(())
        })
        .build()
}
