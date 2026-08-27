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
pub(crate) async fn set_layout<R: Runtime>(
    app: AppHandle<R>,
    payload: LayoutRequest,
) -> crate::Result<()> {
    app.multiline_taskband().set_layout(payload.id, payload.layout)
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
