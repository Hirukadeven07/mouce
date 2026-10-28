//! The mouce window: a tiny always-on-top, transparent, never-activating window.

pub mod position;

use crate::settings::{self, Settings, MouceSize};
use position::{clamp_to_monitors, default_position, keep_inside, Rect};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_HIDE, SW_SHOWNOACTIVATE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

pub const MOUCE_LABEL: &str = "mouce";
pub const HOME_LABEL: &str = "home";
pub const FIRSTRUN_LABEL: &str = "firstrun";

/// Event to the webview: payload `"idle" | "thinking" | "looking" | "reminder"`.
pub const MOUCE_STATE_EVENT: &str = "mouce-state";
/// Event to the home window: payload is the tab to show (see `HomeTab`).
pub const HOME_TAB_EVENT: &str = "home-tab";
/// Event to the webview: payload is the full `Settings`.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// Extra logical pixels around the mouce so its glow is not clipped.
const GLOW_PADDING: u32 = 12;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

fn window_logical_size(size: MouceSize) -> u32 {
    size.px() + GLOW_PADDING
}

fn work_area_of(m: &Monitor) -> Rect {
    let wa = m.work_area();
    Rect { x: wa.position.x, y: wa.position.y, w: wa.size.width, h: wa.size.height }
}

/// Saved position if it still fits fully on some monitor, else bottom-right of the primary one.
fn initial_position(app: &AppHandle, settings: &Settings) -> tauri::Result<(i32, i32)> {
    let logical = window_logical_size(settings.mouce_size);
    if let Some(saved) = settings.mouce_position {
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
    let needed = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
    // SAFETY: `hwnd` is the live handle of a window owned by this process.
    let applied = unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | needed);
        GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
    };
    // Never-stealing-focus is a core property of the mouce: fail loudly if it did not stick.
    if applied & needed != needed {
        return Err(tauri::Error::Io(std::io::Error::other(
            "could not apply the no-focus window style to the mouce",
        )));
    }
    Ok(())
}

/// Shows the mouce without activating it, so it never takes keyboard focus from the user.
///
/// `window.show()` is not usable here: tao drops its "do not focus" marker once a window is
/// built, so a later `show()` calls `ShowWindow(SW_SHOW)`, which activates the window.
/// `SW_SHOWNOACTIVATE` shows it and leaves the foreground window alone. tao's own "visible"
/// flag is not touched (it stays as built), so the mouce is always shown and hidden through
/// this function and `hide`, never through `window.show()` / `window.hide()`. Visibility
/// queries (`is_visible`) ask Windows directly and stay correct.
pub fn show_without_focus(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?;
    // SAFETY: `hwnd` is the live handle of a window owned by this process. The return value
    // is the previous visibility, not an error.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

/// Hides the mouce. Pairs with `show_without_focus` (see there for why not `window.hide()`).
pub fn hide(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?;
    // SAFETY: as in `show_without_focus`.
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    Ok(())
}

/// Debounced persistence of the mouce position after the user drags it.
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
                // Save while holding the lock so a concurrent settings update cannot be
                // overwritten by this (older) snapshot.
                let mut s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.mouce_position = Some(pos);
                if let Err(e) = settings::save(&state.store, &s) {
                    eprintln!("could not save mouce position: {e}");
                }
            }
        });
    });
}

pub fn create(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let logical = window_logical_size(settings.mouce_size) as f64;
    let (x, y) = initial_position(app, settings)?;

    let window = WebviewWindowBuilder::new(app, MOUCE_LABEL, WebviewUrl::App("index.html".into()))
        .title("mouce")
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
    show_without_focus(&window)?;
    Ok(())
}

/// Applies a new mouce size to the live window, keeping it inside the work area.
pub fn resize(app: &AppHandle, size: MouceSize) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(MOUCE_LABEL) else { return Ok(()) };
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

/// Labels of windows that are being built right now, so two quick requests for the same
/// window never build it twice.
#[derive(Default)]
pub struct OpeningWindows(Mutex<HashSet<&'static str>>);

impl OpeningWindows {
    /// Marks `label` as being built. Returns false if it already is.
    pub fn try_begin(&self, label: &'static str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(label)
    }

    /// Marks `label` as no longer being built (the build finished or failed).
    pub fn finish(&self, label: &'static str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(label);
    }
}

/// Clears the "being built" mark for its window when dropped, whatever happens.
struct OpeningMark {
    app: AppHandle,
    label: &'static str,
}

impl OpeningMark {
    fn try_new(app: &AppHandle, label: &'static str) -> Option<OpeningMark> {
        let state = app.try_state::<crate::AppState>()?;
        state.opening.try_begin(label).then(|| OpeningMark { app: app.clone(), label })
    }
}

impl Drop for OpeningMark {
    fn drop(&mut self) {
        if let Some(state) = self.app.try_state::<crate::AppState>() {
            state.opening.finish(self.label);
        }
    }
}

fn focus_existing(app: &AppHandle, label: &'static str) -> bool {
    let Some(w) = app.get_webview_window(label) else { return false };
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
    true
}

/// Focuses the window `label`, creating it first if needed. `init_script` runs in a newly
/// built window before the page loads.
fn open_window(
    app: &AppHandle,
    label: &'static str,
    title: &'static str,
    size: (f64, f64),
    init_script: Option<String>,
) {
    if focus_existing(app, label) {
        return;
    }
    // A build is already under way for this label: it will show the window.
    let Some(mark) = OpeningMark::try_new(app, label) else { return };
    // The previous build may have finished between the check above and taking the mark.
    if focus_existing(app, label) {
        return;
    }
    // Windows can deadlock when a window is built from the event-loop thread; build elsewhere.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut builder =
            WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
                .title(title)
                .inner_size(size.0, size.1)
                .min_inner_size(size.0 * 0.8, size.1 * 0.8)
                .decorations(true)
                .center();
        if let Some(script) = init_script {
            builder = builder.initialization_script(&script);
        }
        let built = builder.build();
        drop(mark);
        if let Err(e) = built {
            eprintln!("could not open {label} window: {e}");
        }
    });
}

/// The home window tabs Rust can ask for. The webview has more tabs; these are the ones
/// the tray and a second launch open directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeTab {
    Home,
    Settings,
}

impl HomeTab {
    pub fn as_str(self) -> &'static str {
        match self {
            HomeTab::Home => "home",
            HomeTab::Settings => "settings",
        }
    }
}

/// Tells a newly built home window which tab to start on. Only fixed tab names go in.
fn initial_tab_script(tab: HomeTab) -> String {
    format!("window.__MOUCE_HOME_TAB = \"{}\";", tab.as_str())
}

/// Focuses the home window on `tab`, creating it first if needed.
pub fn open_home(app: &AppHandle, tab: HomeTab) {
    // An open window switches tabs on this event; a new one starts on the tab from its script.
    let _ = app.emit_to(HOME_LABEL, HOME_TAB_EVENT, tab.as_str());
    open_window(app, HOME_LABEL, "mouce", (780.0, 600.0), Some(initial_tab_script(tab)));
}

/// Focuses the first-run welcome window, creating it first if needed.
pub fn open_firstrun(app: &AppHandle) {
    open_window(app, FIRSTRUN_LABEL, "Welcome to mouce", (520.0, 600.0), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_can_only_be_marked_once_until_finished() {
        let opening = OpeningWindows::default();
        assert!(opening.try_begin(HOME_LABEL));
        assert!(!opening.try_begin(HOME_LABEL), "second request while building");
        assert!(opening.try_begin(FIRSTRUN_LABEL), "other windows are independent");
        opening.finish(HOME_LABEL);
        assert!(opening.try_begin(HOME_LABEL), "free again after the build finished");
        assert!(!opening.try_begin(FIRSTRUN_LABEL));
    }

    #[test]
    fn finishing_an_unmarked_window_is_harmless() {
        let opening = OpeningWindows::default();
        opening.finish(HOME_LABEL);
        assert!(opening.try_begin(HOME_LABEL));
    }

    #[test]
    fn home_tabs_have_the_names_the_webview_expects() {
        assert_eq!(HomeTab::Home.as_str(), "home");
        assert_eq!(HomeTab::Settings.as_str(), "settings");
    }

    #[test]
    fn initial_tab_script_sets_only_the_tab_name() {
        assert_eq!(
            initial_tab_script(HomeTab::Settings),
            r#"window.__MOUCE_HOME_TAB = "settings";"#
        );
    }
}
