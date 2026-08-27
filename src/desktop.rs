use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, Runtime};

use crate::models::*;

/// Access to the multiline-taskband APIs.
pub struct MultilineTaskband<R: Runtime>(tauri::AppHandle<R>);

#[cfg(target_os = "windows")]
use crate::native::windows as native;

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &tauri::AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<MultilineTaskband<R>> {
    #[cfg(target_os = "windows")]
    {
        // The Windows native module stores the app handle as the concrete
        // `Wry` runtime; clone first (bump the refcount) then transmute the
        // copy so the original `app` stays valid for the plugin's lifetime.
        let app_wry: tauri::AppHandle<tauri::Wry> =
            unsafe { std::mem::transmute_copy(&app.clone()) };
        native::start(app_wry);
    }
    Ok(MultilineTaskband(app.clone()))
}

impl<R: Runtime> MultilineTaskband<R> {
    pub fn create(&self, id: String, side: Side) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::create(id, side)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, side);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn remove(&self, id: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::remove(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_text(&self, id: String, top: String, bottom: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_text(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_font_sizes(&self, id: String, top: f64, bottom: f64) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_font_sizes(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_layout(&self, id: String, layout: i32) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_layout(id, layout)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, layout);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_colors(
        &self,
        id: String,
        top: ColorStyle,
        bottom: ColorStyle,
    ) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_colors(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_bold(&self, id: String, top: bool, bottom: bool) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_bold(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_alignment(&self, id: String, top: i32, bottom: i32) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_alignment(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_visible(&self, id: String, visible: bool) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_visible(id, visible)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, visible);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn rect(&self, id: String) -> crate::Result<Rect> {
        #[cfg(target_os = "windows")]
        {
            native::rect(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn is_visible(&self, id: String) -> crate::Result<bool> {
        #[cfg(target_os = "windows")]
        {
            native::is_visible(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }
}
