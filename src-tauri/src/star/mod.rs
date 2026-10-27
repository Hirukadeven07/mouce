//! The star window: a tiny always-on-top, transparent, never-activating window.

pub mod position;

use crate::settings::{self, Settings, StarSize};
use position::{clamp_to_monitors, default_position, keep_inside, Rect};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

pub const STAR_LABEL: &str = "star";
pub const SETTINGS_LABEL: &str = "settings";

/// Event to the webview: payload `"idle" | "thinking" | "looking" | "reminder"`.
pub const STAR_STATE_EVENT: &str = "star-state";
/// Event to the webview: payload is the full `Settings`.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// Extra logical pixels around the star so its glow is not clipped.
const GLOW_PADDING: u32 = 12;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

fn window_logical_size(size: StarSize) -> u32 {
    size.px() + GLOW_PADDING
}

fn work_area_of(m: &Monitor) -> Rect {
    let wa = m.work_area();
    Rect { x: wa.position.x, y: wa.position.y, w: wa.size.width, h: wa.size.height }
}

/// Saved position if it still fits fully on some monitor, else bottom-right of the primary one.
fn initial_position(app: &AppHandle, settings: &Settings) -> tauri::Result<(i32, i32)> {
    let logical = window_logical_size(settings.star_size);
    if let Some(saved) = settings.star_position {
        for m in app.available_monitors()? {
            let physical = (logical as f64 * m.scale_factor()).round() as u32;
            if let Some(p) = clamp_to_monitors(saved, physical, &[work_area_of(&m)]) {
                return Ok(p);
            }
        }
    }
    match app.primary_monitor()? {
        Some(m) => Ok(default_position(work_area_of(&m), m.scale_factor(), logical)),
        None => Ok((0, 0)),
    }
}

/// Makes the window non-activating (clicks never steal focus) and hides it from Alt+Tab.
fn apply_no_focus_styles(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?;
    // SAFETY: `hwnd` is the live handle of a window owned by this process.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
    }
    Ok(())
}

/// Debounced persistence of the star position after the user drags it.
fn watch_moves(app: &AppHandle, window: &tauri::WebviewWindow) {
    let generation = Arc::new(AtomicU64::new(0));
    let app = app.clone();
    window.on_window_event(move |event| {
        let WindowEvent::Moved(pos) = event else { return };
        // Windows reports far-negative coordinates for minimized windows; never persist those.
        if pos.x <= -32000 || pos.y <= -32000 {
            return;
        }
        let pos = (pos.x, pos.y);
        let ticket = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = generation.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SAVE_DEBOUNCE).await;
            if generation.load(Ordering::SeqCst) != ticket {
                return;
            }
            if let Some(state) = app.try_state::<crate::AppState>() {
                let snapshot = {
                    let mut s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
                    s.star_position = Some(pos);
                    s.clone()
                };
                if let Err(e) = settings::save(&state.store, &snapshot) {
                    eprintln!("could not save star position: {e}");
                }
            }
        });
    });
}

pub fn create(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let logical = window_logical_size(settings.star_size) as f64;
    let (x, y) = initial_position(app, settings)?;

    let window = WebviewWindowBuilder::new(app, STAR_LABEL, WebviewUrl::App("index.html".into()))
        .title("Star")
        .inner_size(logical, logical)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()?;

    window.set_position(PhysicalPosition::new(x, y))?;
    apply_no_focus_styles(&window)?;
    watch_moves(app, &window);
    window.show()?;
    Ok(())
}

/// Applies a new star size to the live window, keeping it inside the work area.
pub fn resize(app: &AppHandle, size: StarSize) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(STAR_LABEL) else { return Ok(()) };
    let logical = window_logical_size(size) as f64;

    match window.current_monitor()? {
        Some(m) => {
            let physical = (logical * m.scale_factor()).round() as u32;
            let pos = window.outer_position()?;
            let fitted = keep_inside((pos.x, pos.y), physical, work_area_of(&m));
            window.set_size(PhysicalSize::new(physical, physical))?;
            if fitted != (pos.x, pos.y) {
                window.set_position(PhysicalPosition::new(fitted.0, fitted.1))?;
            }
        }
        None => window.set_size(LogicalSize::new(logical, logical))?,
    }
    Ok(())
}

/// Focuses the Settings window, creating it first if needed.
pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // Windows can deadlock when a window is built from the event-loop thread; build elsewhere.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let built =
            WebviewWindowBuilder::new(&app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
                .title("Star Settings")
                .inner_size(480.0, 640.0)
                .decorations(true)
                .center()
                .build();
        if let Err(e) = built {
            eprintln!("could not open settings window: {e}");
        }
    });
}
