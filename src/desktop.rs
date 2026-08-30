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

    /// Override the font family of the top/bottom line. `None`/`""` resets
    /// that line to the system default font (mirrors the menubar plugin).
    pub fn set_font_family(
        &self,
        id: String,
        top: Option<String>,
        bottom: Option<String>,
    ) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_font_family(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    pub fn set_padding(&self, id: String, left: i32, right: i32) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_padding(id, left, right)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, left, right);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Move an existing instance to the other side of the taskbar (left/right)
    /// without recreating it. Creation order is preserved, so within the new
    /// side the instance keeps its relative position.
    pub fn set_side(&self, id: String, side: Side) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_side(id, side)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, side);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Re-order an instance within its side (ascending `order`). See
    /// [`SetOrderRequest`].
    pub fn set_order(&self, id: String, order: u64) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_order(id, order)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, order);
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Set the global margin (physical px) between adjacent instances. See
    /// [`SetMarginRequest`].
    pub fn set_margin(&self, margin: i32) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_margin(margin)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = margin;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Set extra edge margins (physical px) for the left/right instance
    /// groups. See [`SetEdgeMarginsRequest`].
    pub fn set_edge_margins(
        &self,
        left: Option<i32>,
        right: Option<i32>,
    ) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_edge_margins(left, right)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (left, right);
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

    pub fn set_line_visible(&self, id: String, top: bool, bottom: bool) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_line_visible(id, top, bottom)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, top, bottom);
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

    /// Set which Tauri window is used as the popup. Call before the first open.
    pub fn set_popup_window(&self, label: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_popup_window(label)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = label;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Enable/disable automatically toggling the popup on left click.
    pub fn set_auto_popup(&self, enabled: bool) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_auto_popup(enabled)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = enabled;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Show the popup window anchored next to the given instance.
    pub fn open_popup(&self, id: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::open_popup(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Hide the popup window.
    pub fn close_popup(&self, id: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::close_popup(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Toggle the popup window's visibility, anchored next to the given
    /// instance.
    pub fn toggle_popup(&self, id: String) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::toggle_popup(id)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = id;
            Err(crate::Error::UnsupportedPlatform)
        }
    }

    /// Attach (or detach, with `None`) the right-click context menu of an
    /// instance. Selections are emitted as `multiline-taskband://{id}//menu`
    /// with `{ id, itemId }`.
    pub fn set_menu(
        &self,
        id: String,
        items: Option<Vec<MenuItemDescriptor>>,
    ) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            native::set_menu(id, items)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (id, items);
            Err(crate::Error::UnsupportedPlatform)
        }
    }
}
