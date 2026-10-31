//! The mouce window: a tiny always-on-top, transparent, never-activating window.

pub mod position;

use crate::settings::{self, MouceSize, Settings};
use position::{clamp_to_monitors, default_position, keep_inside, panel_position, Rect};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::{Effect, EffectsBuilder};
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
pub const CHAT_LABEL: &str = "chat";
pub const CIRCLE_LABEL: &str = "circle";

/// Event to the webview: payload `"idle" | "thinking" | "looking" | "reminder"`.
pub const MOUCE_STATE_EVENT: &str = "mouce-state";
/// Event to the home window: payload is the tab to show (see `HomeTab`).
pub const HOME_TAB_EVENT: &str = "home-tab";
/// Event to the webview: payload is the full `Settings`.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// Extra logical pixels around the mouce so its glow is not clipped.
const GLOW_PADDING: u32 = 12;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// The chat panel's size and its distance from the mouce, in logical pixels.
const CHAT_SIZE: (f64, f64) = (380.0, 520.0);
const CHAT_GAP: f64 = 8.0;

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
pub fn apply_no_focus_styles(window: &tauri::WebviewWindow) -> tauri::Result<()> {
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
            "could not apply the no-focus window style",
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
                    log::warn!("could not save mouce position: {e}");
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
    // Screen pictures should show what is behind the mouce; without this they still work.
    let excluded = crate::screen::exclude_from_capture(&window);
    log_failure("keep the mouce out of screen pictures", excluded);
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

/// Focuses the window `label`, or builds it with `build` if it does not exist yet.
fn open_or_build(
    app: &AppHandle,
    label: &'static str,
    build: impl FnOnce(&AppHandle) -> tauri::Result<()> + Send + 'static,
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
        let built = build(&app);
        drop(mark);
        if let Err(e) = built {
            log::warn!("could not open {label} window: {e}");
        }
    });
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
    open_or_build(app, label, move |app| {
        let mut builder =
            WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
                .title(title)
                .inner_size(size.0, size.1)
                .min_inner_size(size.0 * 0.8, size.1 * 0.8)
                .decorations(true)
                .transparent(true)
                .center();
        if let Some(effects) = glass_effects(glass_on(app)) {
            builder = builder.effects(effects);
        }
        // The home page is always dark, so its title bar is too.
        if label == HOME_LABEL {
            builder = builder.theme(Some(tauri::Theme::Dark));
        }
        if let Some(script) = init_script {
            builder = builder.initialization_script(&script);
        }
        builder.build().map(|_| ())
    });
}

fn log_failure<E: std::fmt::Display>(what: &str, result: Result<(), E>) {
    if let Err(e) = result {
        log::warn!("could not {what}: {e}");
    }
}

/// The page windows are transparent so that, with Liquid Glass on, the page can let the
/// Mica material show through. With it off the page paints itself opaque and needs none.
fn glass_effects(on: bool) -> Option<WindowEffectsConfig> {
    on.then(|| EffectsBuilder::new().effect(Effect::Mica).build())
}

fn glass_on(app: &AppHandle) -> bool {
    app.try_state::<crate::AppState>()
        .map(|state| state.settings.lock().unwrap_or_else(|e| e.into_inner()).liquid_glass)
        .unwrap_or(true)
}

/// Gives the open page windows the material for the Liquid Glass setting. A window that keeps
/// its old material still works, so failures are only logged.
pub fn set_glass(app: &AppHandle, on: bool) {
    for label in [HOME_LABEL, FIRSTRUN_LABEL, CHAT_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            log_failure("change the window material", window.set_effects(glass_effects(on)));
        }
    }
}

/// Where the chat panel goes: beside the mouce, on the mouce's monitor. `None` when the
/// mouce or its monitor is not known (the panel then stays centred).
fn chat_position(
    app: &AppHandle,
    chat: &tauri::WebviewWindow,
) -> tauri::Result<Option<(i32, i32)>> {
    let Some(mouce) = app.get_webview_window(MOUCE_LABEL) else { return Ok(None) };
    let Some(monitor) = mouce.current_monitor()? else { return Ok(None) };
    let (pos, size) = (mouce.outer_position()?, mouce.outer_size()?);
    let anchor = Rect { x: pos.x, y: pos.y, w: size.width, h: size.height };
    let panel = chat.outer_size()?;
    let gap = (CHAT_GAP * monitor.scale_factor()).round() as i32;
    Ok(Some(panel_position(anchor, panel.width, panel.height, gap, work_area_of(&monitor))))
}

fn build_chat(app: &AppHandle) -> tauri::Result<()> {
    let mut builder =
        WebviewWindowBuilder::new(app, CHAT_LABEL, WebviewUrl::App("index.html".into()))
            .title("Chat with mouce")
            .inner_size(CHAT_SIZE.0, CHAT_SIZE.1)
            .min_inner_size(300.0, 360.0)
            .always_on_top(true)
            .transparent(true)
            .center()
            .visible(false)
            // The chat page is always dark, so its title bar is too.
            .theme(Some(tauri::Theme::Dark));
    if let Some(effects) = glass_effects(glass_on(app)) {
        builder = builder.effects(effects);
    }
    let window = builder.build()?;
    if let Some((x, y)) = chat_position(app, &window)? {
        window.set_position(PhysicalPosition::new(x, y))?;
    }
    // Screen pictures should show what is behind the panel; without this they still work.
    let excluded = crate::screen::exclude_from_capture(&window);
    log_failure("keep the chat out of screen pictures", excluded);
    window.show()?;
    window.set_focus()
}

/// Builds the full-screen layer for circling, covering `area` (physical pixels). It stays
/// hidden until its page has the frozen picture on screen (`circle_ready`).
pub fn build_circle(app: &AppHandle, area: Rect) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, CIRCLE_LABEL, WebviewUrl::App("index.html".into()))
        .title("Circle to look up")
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    window.set_position(PhysicalPosition::new(area.x, area.y))?;
    window.set_size(PhysicalSize::new(area.w, area.h))?;
    Ok(window)
}

/// Focuses the chat panel beside the mouce, creating it first if needed.
pub fn open_chat(app: &AppHandle) {
    open_or_build(app, CHAT_LABEL, build_chat);
}

/// What a click on the mouce opens.
#[derive(Debug, PartialEq, Eq)]
pub enum ClickTarget {
    Chat,
    Home,
}

/// The first click opens the chat. With the chat already on screen, a click on the mouce
/// is the way to the home window.
pub fn click_target(chat_on_screen: bool) -> ClickTarget {
    if chat_on_screen {
        ClickTarget::Home
    } else {
        ClickTarget::Chat
    }
}

/// Closes the chat panel, if it is open. The conversation lives on in Rust.
pub fn close_chat(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CHAT_LABEL) {
        log_failure("close the chat panel", window.close());
    }
}

/// Whether the chat panel is open and not minimised.
pub fn chat_on_screen(app: &AppHandle) -> bool {
    app.get_webview_window(CHAT_LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// The monitor the mouce is on (else the primary one), in physical pixels.
pub fn screen_area(app: &AppHandle) -> Option<Rect> {
    let monitor = app
        .get_webview_window(MOUCE_LABEL)
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let (pos, size) = (monitor.position(), monitor.size());
    Some(Rect { x: pos.x, y: pos.y, w: size.width, h: size.height })
}

/// The home window tabs Rust can ask for. The webview has more tabs; these are the ones
/// the tray, a second launch and the reminder card open directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeTab {
    Home,
    Chat,
    Calendar,
    Reminders,
    Settings,
}

impl HomeTab {
    pub fn as_str(self) -> &'static str {
        match self {
            HomeTab::Home => "home",
            HomeTab::Chat => "chat",
            HomeTab::Calendar => "calendar",
            HomeTab::Reminders => "reminders",
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
    fn a_click_opens_the_chat_first_and_home_once_the_chat_is_showing() {
        assert_eq!(click_target(false), ClickTarget::Chat);
        assert_eq!(click_target(true), ClickTarget::Home);
    }

    #[test]
    fn home_tabs_have_the_names_the_webview_expects() {
        assert_eq!(HomeTab::Home.as_str(), "home");
        assert_eq!(HomeTab::Chat.as_str(), "chat");
        assert_eq!(HomeTab::Calendar.as_str(), "calendar");
        assert_eq!(HomeTab::Reminders.as_str(), "reminders");
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
