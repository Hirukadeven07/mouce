//! Circle to look up: freeze the screen under the pointer, let the user circle part of it
//! on a full-screen layer, and hand that part to the chat to ask about.
//!
//! The frozen picture lives only in memory while the layer is open; the circled part is
//! kept until the next question is asked about it (or it is removed). Nothing is saved.

use crate::ai::ImageInput;
use crate::chat::Snip;
use crate::mouce::{self, position::Rect};
use crate::screen::{self, Selection};
use crate::AppState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Event to the chat window: payload is the preview of the circled area, or null once it
/// is gone.
pub const CHAT_ATTACHMENT_EVENT: &str = "chat-attachment";

/// Starts circling on the screen under the pointer. While already circling, brings the
/// layer forward instead.
pub fn start(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let busy = state
        .circle_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err();
    if busy {
        if let Some(layer) = app.get_webview_window(mouce::CIRCLE_LABEL) {
            let _ = layer.set_focus();
        }
        return;
    }
    let app = app.clone();
    // Windows can deadlock when a window is built from the event-loop thread; build elsewhere.
    tauri::async_runtime::spawn(async move {
        if let Err(e) = begin(&app).await {
            eprintln!("could not start circling: {e}");
            ended(&app);
        }
    });
}

async fn begin(app: &AppHandle) -> Result<(), String> {
    let area = area_under_pointer(app).ok_or("Could not find the screen to circle on")?;
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, "looking");
    let frame = tauri::async_runtime::spawn_blocking(move || screen::grab(area))
        .await
        .map_err(|e| e.to_string())??;
    let state = app.try_state::<AppState>().ok_or("mouce is still starting")?;
    *state.circle_frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
    let layer = mouce::build_circle(app, area).map_err(|e| e.to_string())?;
    // However the layer closes (finished, cancelled, Alt+F4), circling is over.
    let app = app.clone();
    layer.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            ended(&app);
        }
    });
    Ok(())
}

/// Circling is over: forget the frozen picture and let the mouce rest.
fn ended(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        clear(&state);
    }
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, "idle");
}

fn clear(state: &AppState) {
    *state.circle_frame.lock().unwrap_or_else(|e| e.into_inner()) = None;
    state.circle_active.store(false, Ordering::SeqCst);
}

/// The monitor under the pointer (else the primary one), in physical pixels.
fn area_under_pointer(app: &AppHandle) -> Option<Rect> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let (pos, size) = (monitor.position(), monitor.size());
    Some(Rect { x: pos.x, y: pos.y, w: size.width, h: size.height })
}

/// The frozen picture at full size, for the layer to show.
pub fn frame_image_impl(state: &AppState) -> Result<String, String> {
    let frame = state.circle_frame.lock().unwrap_or_else(|e| e.into_inner());
    let frame = frame.as_ref().ok_or("There is nothing to circle")?;
    let edge = frame.width.max(frame.height);
    let image = screen::encode(&frame.bgra, frame.width, frame.height, edge)?;
    Ok(screen::data_url(&image))
}

/// Cuts the circled part out of the frozen picture and keeps it for the next question,
/// replacing any part circled before. Returns its preview.
pub fn finish_impl(state: &AppState, selection: Selection) -> Result<String, String> {
    let (pixels, area) = {
        let frame = state.circle_frame.lock().unwrap_or_else(|e| e.into_inner());
        let frame = frame.as_ref().ok_or("There is nothing to circle")?;
        let padding = screen::SELECTION_PADDING;
        let area = screen::selection_rect(selection, frame.width, frame.height, padding)
            .ok_or("Draw around something a bit bigger")?;
        (screen::crop(frame, area), area)
    };
    let image = screen::encode(&pixels, area.w, area.h, screen::MAX_EDGE)?;
    let preview = screen::encode(&pixels, area.w, area.h, screen::THUMBNAIL_EDGE)?;
    let thumbnail = screen::data_url(&preview);
    let snip = Snip {
        image: ImageInput { mime: "image/jpeg".to_string(), base64: image.base64 },
        thumbnail: thumbnail.clone(),
    };
    *state.circled.lock().unwrap_or_else(|e| e.into_inner()) = Some(snip);
    *state.circle_frame.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(thumbnail)
}

/// The preview of the circled area waiting to be asked about, if there is one.
pub fn attachment_impl(state: &AppState) -> Option<String> {
    state.circled.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.thumbnail.clone())
}

/// Forgets the circled area waiting to be asked about.
pub fn discard_impl(state: &AppState) {
    *state.circled.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Closes the layer; its `Destroyed` event ends the circling.
pub fn close_layer(app: &AppHandle) {
    match app.get_webview_window(mouce::CIRCLE_LABEL) {
        Some(layer) => {
            if let Err(e) = layer.close() {
                eprintln!("could not close the circle layer: {e}");
                ended(app);
            }
        }
        None => ended(app),
    }
}

/// Tells an open chat what is waiting to be asked about.
pub fn announce_attachment(app: &AppHandle, preview: Option<String>) {
    let _ = app.emit_to(mouce::CHAT_LABEL, CHAT_ATTACHMENT_EVENT, preview);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::Frame;
    use crate::settings::Settings;
    use crate::store::AppStore;
    use base64::Engine as _;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let secrets = crate::secrets::Secrets::new("com.mouce.app.test.circle.unused");
        (dir, AppState::new(store, Settings::default(), secrets))
    }

    fn frozen(st: &AppState, width: u32, height: u32) {
        let bgra = vec![200u8; width as usize * height as usize * 4];
        *st.circle_frame.lock().unwrap() = Some(Frame { width, height, bgra });
    }

    fn jpeg_size(data_url: &str) -> usize {
        let b64 = data_url.strip_prefix("data:image/jpeg;base64,").expect("a jpeg data url");
        base64::engine::general_purpose::STANDARD.decode(b64).unwrap().len()
    }

    #[test]
    fn finishing_keeps_the_circled_part_and_forgets_the_frame() {
        let (_d, st) = state();
        frozen(&st, 400, 300);
        let sel = Selection { x: 0.25, y: 0.25, w: 0.5, h: 0.5 };
        let preview = finish_impl(&st, sel).unwrap();
        assert!(jpeg_size(&preview) > 0);
        assert!(st.circle_frame.lock().unwrap().is_none(), "the frozen picture is gone");
        let snip = st.circled.lock().unwrap().clone().unwrap();
        assert_eq!(snip.image.mime, "image/jpeg");
        assert_eq!(snip.thumbnail, preview);
        assert_eq!(attachment_impl(&st), Some(preview));
    }

    #[test]
    fn a_too_small_circle_keeps_the_frame_to_try_again() {
        let (_d, st) = state();
        frozen(&st, 400, 300);
        let dot = Selection { x: 0.5, y: 0.5, w: 0.001, h: 0.001 };
        assert_eq!(finish_impl(&st, dot).unwrap_err(), "Draw around something a bit bigger");
        assert!(st.circle_frame.lock().unwrap().is_some());
        assert_eq!(attachment_impl(&st), None);
    }

    #[test]
    fn there_is_nothing_to_finish_or_show_without_a_frame() {
        let (_d, st) = state();
        let whole = Selection { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        assert_eq!(finish_impl(&st, whole).unwrap_err(), "There is nothing to circle");
        assert_eq!(frame_image_impl(&st).unwrap_err(), "There is nothing to circle");
    }

    #[test]
    fn the_frame_is_shown_as_a_jpeg() {
        let (_d, st) = state();
        frozen(&st, 64, 32);
        assert!(jpeg_size(&frame_image_impl(&st).unwrap()) > 0);
    }

    #[test]
    fn discarding_forgets_the_circled_part() {
        let (_d, st) = state();
        frozen(&st, 100, 100);
        finish_impl(&st, Selection { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }).unwrap();
        discard_impl(&st);
        assert_eq!(attachment_impl(&st), None);
    }

    #[test]
    fn clearing_ends_circling() {
        let (_d, st) = state();
        frozen(&st, 10, 10);
        st.circle_active.store(true, Ordering::SeqCst);
        clear(&st);
        assert!(st.circle_frame.lock().unwrap().is_none());
        assert!(!st.circle_active.load(Ordering::SeqCst));
    }
}
