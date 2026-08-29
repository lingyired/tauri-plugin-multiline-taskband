use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::MultilineTaskbandExt;

#[command]
pub(crate) async fn create<R: Runtime>(
    app: AppHandle<R>,
    payload: CreateRequest,
) -> crate::Result<()> {
    app.multiline_taskband().create(payload.id.clone(), payload.side)?;
    if let (Some(top), Some(bottom)) = (payload.top, payload.bottom) {
        app.multiline_taskband().set_text(payload.id, top, bottom)?;
    }
    Ok(())
}

#[command]
pub(crate) async fn remove<R: Runtime>(
    app: AppHandle<R>,
    payload: RemoveRequest,
) -> crate::Result<()> {
    app.multiline_taskband().remove(payload.id)
}

#[command]
pub(crate) async fn set_text<R: Runtime>(
    app: AppHandle<R>,
    payload: SetTextRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_text(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_font_sizes<R: Runtime>(
    app: AppHandle<R>,
    payload: FontSizesRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_font_sizes(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_font_family<R: Runtime>(
    app: AppHandle<R>,
    payload: SetFontFamilyRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_font_family(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_padding<R: Runtime>(
    app: AppHandle<R>,
    payload: SetPaddingRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_padding(payload.id, payload.left, payload.right)
}

#[command]
pub(crate) async fn set_side<R: Runtime>(
    app: AppHandle<R>,
    payload: SetSideRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_side(payload.id, payload.side)
}

#[command]
pub(crate) async fn set_order<R: Runtime>(
    app: AppHandle<R>,
    payload: SetOrderRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_order(payload.id, payload.order)
}

#[command]
pub(crate) async fn set_margin<R: Runtime>(
    app: AppHandle<R>,
    payload: SetMarginRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_margin(payload.margin)
}

#[command]
pub(crate) async fn set_colors<R: Runtime>(
    app: AppHandle<R>,
    payload: SetColorsRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_colors(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_bold<R: Runtime>(
    app: AppHandle<R>,
    payload: SetBoldRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_bold(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_alignment<R: Runtime>(
    app: AppHandle<R>,
    payload: SetAlignmentRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_alignment(payload.id, payload.top, payload.bottom)
}

#[command]
pub(crate) async fn set_visible<R: Runtime>(
    app: AppHandle<R>,
    payload: SetVisibleRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_visible(payload.id, payload.visible)
}

#[command]
pub(crate) async fn rect<R: Runtime>(
    app: AppHandle<R>,
    payload: IdRequest,
) -> crate::Result<Rect> {
    app.multiline_taskband().rect(payload.id)
}

#[command]
pub(crate) async fn is_visible<R: Runtime>(
    app: AppHandle<R>,
    payload: IdRequest,
) -> crate::Result<VisibilityResponse> {
    Ok(VisibilityResponse {
        visible: app.multiline_taskband().is_visible(payload.id)?,
    })
}

#[command]
pub(crate) async fn set_popup_window<R: Runtime>(
    app: AppHandle<R>,
    payload: SetPopupWindowRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_popup_window(payload.label)
}

#[command]
pub(crate) async fn set_auto_popup<R: Runtime>(
    app: AppHandle<R>,
    payload: SetAutoPopupRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_auto_popup(payload.enabled)
}

#[command]
pub(crate) async fn open_popup<R: Runtime>(
    app: AppHandle<R>,
    payload: IdRequest,
) -> crate::Result<()> {
    app.multiline_taskband().open_popup(payload.id)
}

#[command]
pub(crate) async fn close_popup<R: Runtime>(
    app: AppHandle<R>,
    payload: IdRequest,
) -> crate::Result<()> {
    app.multiline_taskband().close_popup(payload.id)
}

#[command]
pub(crate) async fn toggle_popup<R: Runtime>(
    app: AppHandle<R>,
    payload: IdRequest,
) -> crate::Result<()> {
    app.multiline_taskband().toggle_popup(payload.id)
}

#[command]
pub(crate) async fn set_menu<R: Runtime>(
    app: AppHandle<R>,
    payload: SetMenuRequest,
) -> crate::Result<()> {
    app.multiline_taskband()
        .set_menu(payload.id, payload.items)
}
