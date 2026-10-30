//! Gentle reminders. A timer checks the calendar every few seconds; when an event or
//! reminder is due, a small card opens beside the mouce (without taking the keyboard) and
//! the mouce glows until each one is snoozed, done or put away. There is no sound.
//!
//! Reminders wait during the user's quiet hours, and while an app fills the screen (a film,
//! a presentation, a game); they appear once that is over.

use super::date;
use super::{Item, Kind};
use crate::mouce::{self, position::Rect};
use crate::settings::Settings;
use crate::AppState;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
};

pub const CARD_LABEL: &str = "alerts";
/// Event to every window: payload is the list of `Item`s due now, sent whenever it changes.
pub const ALERTS_EVENT: &str = "calendar-alerts";
/// Event to every window: the saved events or reminders changed. No payload.
pub const CHANGED_EVENT: &str = "calendar-changed";

const CHECK_EVERY: Duration = Duration::from_secs(15);
/// How long "Snooze" waits, in milliseconds.
pub const SNOOZE_MS: i64 = 10 * 60 * 1000;
/// "Snooze for an hour", in milliseconds.
pub const SNOOZE_HOUR_MS: i64 = 60 * 60 * 1000;
/// "Snooze until tomorrow" reminds at this time the next morning, in minutes.
const TOMORROW_MORNING: u32 = 9 * 60;
/// A reminder missed by more than this (mouce was not running) is not shown.
const STALE_MS: i64 = 12 * 60 * 60 * 1000;

/// The card's width and tallest height, and its distance from the mouce, in logical pixels.
const CARD_WIDTH: f64 = 320.0;
const CARD_MAX_HEIGHT: f64 = 420.0;
const CARD_GAP: f64 = 8.0;

#[derive(Default)]
pub struct AlertsState {
    /// What the card shows, oldest first.
    due: Mutex<Vec<Item>>,
    /// Held while the card window is being built, so it is built once.
    building: Mutex<()>,
}

/// What the user did with one reminder on the card.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// "Got it": put it away.
    Dismiss,
    /// Remind again in `SNOOZE_MS`.
    Snooze,
    /// Remind again in an hour.
    SnoozeHour,
    /// Remind again tomorrow morning.
    SnoozeTomorrow,
    /// A reminder that has been done (a repeating one moves on to its next time).
    Done,
}

/// When a snooze chosen at `now` reminds again.
pub fn snooze_until(action: Action, now: i64) -> Option<i64> {
    match action {
        Action::Snooze => Some(now + SNOOZE_MS),
        Action::SnoozeHour => Some(now + SNOOZE_HOUR_MS),
        Action::SnoozeTomorrow => {
            let (today, _) = date::local_day(now)?;
            date::local_ms(today.add_days(1), TOMORROW_MORNING)
        }
        Action::Dismiss | Action::Done => None,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Adds newly due items to the ones shown. One already shown (snoozed, say) is replaced.
fn merge(shown: &mut Vec<Item>, new: Vec<Item>) {
    for item in new {
        shown.retain(|i| i.id != item.id);
        shown.push(item);
    }
}

/// Takes `id` off the card. Returns whether it was there.
fn remove(state: &AppState, id: i64) -> bool {
    let mut due = lock(&state.alerts.due);
    let before = due.len();
    due.retain(|i| i.id != id);
    due.len() != before
}

/// Adds what is due at `now` to the card. Returns whether anything was added.
pub fn check_impl(state: &AppState, now: i64) -> bool {
    match super::take_due(&state.store, now, STALE_MS) {
        Ok(new) if !new.is_empty() => {
            merge(&mut lock(&state.alerts.due), new);
            true
        }
        Ok(_) => false,
        Err(e) => {
            log::warn!("could not check for reminders: {e}");
            false
        }
    }
}

/// Whether `minute` (minutes into the day) is within quiet hours from `from` to `to`
/// ("HH:MM"). Hours that cross midnight (22:00 to 07:00) work; the same start and end is
/// never quiet.
pub fn in_quiet_hours(minute: u32, from: &str, to: &str) -> bool {
    match (date::minutes(from), date::minutes(to)) {
        (Some(from), Some(to)) if from < to => (from..to).contains(&minute),
        (Some(from), Some(to)) if from > to => minute >= from || minute < to,
        _ => false,
    }
}

/// Whether it is quiet hours at `now` by `settings`.
pub fn quiet_now(settings: &Settings, now: i64) -> bool {
    settings.quiet_hours
        && date::local_day(now).is_some_and(|(_, minute)| {
            in_quiet_hours(minute, &settings.quiet_from, &settings.quiet_to)
        })
}

/// Whether `window` covers the whole of `screen`.
fn covers(window: RECT, screen: RECT) -> bool {
    window.left <= screen.left
        && window.top <= screen.top
        && window.right >= screen.right
        && window.bottom >= screen.bottom
}

/// Whether the window in front fills its whole screen, as a film, a presentation or a game
/// does. The desktop does not count.
fn full_screen_app() -> bool {
    // SAFETY: these only ask about the window in front and its screen; each handle is checked
    // before it is used, and nothing is changed.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() || window == GetShellWindow() || window == GetDesktopWindow() {
            return false;
        }
        let mut class = [0u16; 32];
        let len = usize::try_from(GetClassNameW(window, &mut class)).unwrap_or(0);
        let class = String::from_utf16_lossy(&class[..len.min(class.len())]);
        if class == "Progman" || class == "WorkerW" {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            return false;
        }
        let mut info =
            MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(monitor, &mut info).as_bool() && covers(rect, info.rcMonitor)
    }
}

/// Whether reminders should wait at `now`: quiet hours, or an app filling the screen.
fn should_wait(settings: &Settings, now: i64) -> bool {
    quiet_now(settings, now) || (settings.wait_for_full_screen && full_screen_app())
}

/// Starts the timer. Its first check is straight away, for anything due while mouce was
/// not running.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(CHECK_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else { continue };
            let now = now_ms();
            let settings = lock(&state.settings).clone();
            // Nothing is taken while waiting, so it is all still due when the wait is over.
            if should_wait(&settings, now) {
                continue;
            }
            if check_impl(&state, now) {
                announce(&app);
            }
        }
    });
}

/// What is due now, for a page that has just loaded.
pub fn current_impl(state: &AppState) -> Vec<Item> {
    lock(&state.alerts.due).clone()
}

/// Tells the mouce and the card what is due, and opens or hides the card.
fn announce(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let due = current_impl(&state);
    let _ = app.emit(ALERTS_EVENT, &due);
    if due.is_empty() {
        hide_card(app);
    } else if let Err(e) = ensure_card(app) {
        log::warn!("could not open the reminder card: {e}");
    }
}

/// Does what the user chose for `id` on the card, at `now`.
pub fn act_impl(state: &AppState, id: i64, action: Action, now: i64) -> Result<(), String> {
    match action {
        Action::Dismiss => {}
        Action::Done => {
            super::set_done(&state.store, id, true, now).map_err(|e| e.to_string())?;
        }
        snooze => {
            let at = snooze_until(snooze, now).ok_or("mouce could not tell the time")?;
            super::snooze(&state.store, id, at).map_err(|e| e.to_string())?;
        }
    }
    remove(state, id);
    Ok(())
}

pub fn act(app: &AppHandle, id: i64, action: Action) -> Result<(), String> {
    let state = app.try_state::<AppState>().ok_or("mouce is still starting")?;
    act_impl(&state, id, action, now_ms())?;
    announce(app);
    if action != Action::Dismiss {
        let _ = app.emit(CHANGED_EVENT, ());
    }
    Ok(())
}

/// Opens the home window where `id` is: the Calendar for an event, Reminders for a
/// reminder. It stays on the card.
pub fn open(app: &AppHandle, id: i64) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let kind = lock(&state.alerts.due).iter().find(|i| i.id == id).map(|i| i.kind);
    let tab = match kind {
        Some(Kind::Reminder) => mouce::HomeTab::Reminders,
        _ => mouce::HomeTab::Calendar,
    };
    mouce::open_home(app, tab);
}

/// Takes `id` off the card after it was changed or deleted in the home window.
pub fn forget(app: &AppHandle, id: i64) {
    let Some(state) = app.try_state::<AppState>() else { return };
    if remove(&state, id) {
        announce(app);
    }
}

/// Builds the card window, hidden, unless it already exists. It shows itself once its page
/// has laid out what is due (`fit_card`).
fn ensure_card(app: &AppHandle) -> tauri::Result<()> {
    let state = app.try_state::<AppState>();
    let _building = state.as_ref().map(|s| lock(&s.alerts.building));
    if app.get_webview_window(CARD_LABEL).is_some() {
        return Ok(());
    }
    let window =
        tauri::WebviewWindowBuilder::new(app, CARD_LABEL, WebviewUrl::App("index.html".into()))
            .title("mouce reminder")
            .inner_size(CARD_WIDTH, 160.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .build()?;
    // A reminder must not take the keyboard from whatever the user is typing in.
    mouce::apply_no_focus_styles(&window)?;
    if let Err(e) = crate::screen::exclude_from_capture(&window) {
        log::warn!("could not keep the reminder card out of screen pictures: {e}");
    }
    Ok(())
}

fn hide_card(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(CARD_LABEL) {
        if let Err(e) = mouce::hide(&window) {
            log::warn!("could not hide the reminder card: {e}");
        }
    }
}

/// The card's page has laid out what is due at `height` logical pixels: size it and show it
/// beside the mouce, without taking focus.
pub fn fit_card(app: &AppHandle, height: f64) -> Result<(), String> {
    let state = app.try_state::<AppState>().ok_or("mouce is still starting")?;
    if lock(&state.alerts.due).is_empty() {
        return Ok(());
    }
    let card = app.get_webview_window(CARD_LABEL).ok_or("The reminder card is gone")?;
    let anchor = app.get_webview_window(mouce::MOUCE_LABEL).ok_or("The mouce is gone")?;
    let fail = |e: tauri::Error| e.to_string();
    let monitor = anchor
        .current_monitor()
        .map_err(fail)?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or("Could not find the screen")?;
    let scale = monitor.scale_factor();
    let height = height.clamp(80.0, CARD_MAX_HEIGHT);
    let size = ((CARD_WIDTH * scale).round() as u32, (height * scale).round() as u32);
    let (pos, mouce_size) =
        (anchor.outer_position().map_err(fail)?, anchor.outer_size().map_err(fail)?);
    let at = Rect { x: pos.x, y: pos.y, w: mouce_size.width, h: mouce_size.height };
    let wa = monitor.work_area();
    let work = Rect { x: wa.position.x, y: wa.position.y, w: wa.size.width, h: wa.size.height };
    let gap = (CARD_GAP * scale).round() as i32;
    let (x, y) = mouce::position::panel_position(at, size.0, size.1, gap, work);
    card.set_size(PhysicalSize::new(size.0, size.1)).map_err(fail)?;
    card.set_position(PhysicalPosition::new(x, y)).map_err(fail)?;
    mouce::show_without_focus(&card).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{self, date::Day, Input};
    use crate::settings::Settings;
    use crate::store::AppStore;

    const MIN: i64 = 60_000;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let secrets = crate::secrets::Secrets::new("com.mouce.app.test.alerts.unused");
        (dir, AppState::new(store, Settings::default(), secrets))
    }

    /// 30 October 2026 at `time` on this computer's clock.
    fn at(time: &str) -> i64 {
        date::local_ms(Day::parse("2026-10-30").unwrap(), date::minutes(time).unwrap()).unwrap()
    }

    /// Something on 30 October 2026 that reminds at `time`.
    fn add(state: &AppState, kind: Kind, title: &str, time: &str) -> Item {
        let input = Input {
            kind,
            title: title.into(),
            date: "2026-10-30".into(),
            time: Some(time.into()),
            end_time: None,
            remind_before: (kind == Kind::Event).then_some(0),
            repeat: None,
        };
        calendar::add(&state.store, &input, at("00:00")).unwrap()
    }

    fn shown(state: &AppState) -> Vec<String> {
        current_impl(state).into_iter().map(|i| i.title).collect()
    }

    #[test]
    fn due_items_go_on_the_card_once() {
        let (_d, st) = state();
        add(&st, Kind::Reminder, "Later", "10:00");
        add(&st, Kind::Event, "Meeting", "09:00");
        assert!(!check_impl(&st, at("08:59")));
        assert!(check_impl(&st, at("09:00")));
        assert!(!check_impl(&st, at("09:30")), "nothing new");
        assert!(check_impl(&st, at("10:00")));
        assert_eq!(shown(&st), ["Meeting", "Later"]);
    }

    #[test]
    fn dismissing_takes_it_off_the_card_only() {
        let (_d, st) = state();
        let e = add(&st, Kind::Event, "Meeting", "09:00");
        check_impl(&st, at("09:00"));
        act_impl(&st, e.id, Action::Dismiss, at("09:00")).unwrap();
        assert!(shown(&st).is_empty());
        assert!(calendar::get(&st.store, e.id).unwrap().is_some());
        assert!(!check_impl(&st, at("11:00")), "it does not come back");
    }

    #[test]
    fn snoozing_brings_it_back_later() {
        let (_d, st) = state();
        let r = add(&st, Kind::Reminder, "Call", "09:00");
        check_impl(&st, at("09:00"));
        act_impl(&st, r.id, Action::Snooze, at("09:01")).unwrap();
        assert!(shown(&st).is_empty());
        assert!(!check_impl(&st, at("09:10")));
        assert!(check_impl(&st, at("09:11")));
        assert_eq!(shown(&st), ["Call"]);
        act_impl(&st, r.id, Action::SnoozeHour, at("09:11")).unwrap();
        assert!(!check_impl(&st, at("10:10")));
        assert!(check_impl(&st, at("10:11")));
    }

    #[test]
    fn snoozing_until_tomorrow_reminds_in_the_morning() {
        let now = at("22:15");
        let tomorrow = Day::parse("2026-10-31").unwrap();
        assert_eq!(snooze_until(Action::SnoozeTomorrow, now), date::local_ms(tomorrow, 9 * 60));
        assert_eq!(snooze_until(Action::Snooze, now), Some(now + 10 * MIN));
        assert_eq!(snooze_until(Action::SnoozeHour, now), Some(now + 60 * MIN));
        assert_eq!(snooze_until(Action::Done, now), None);
    }

    #[test]
    fn done_marks_the_reminder_done() {
        let (_d, st) = state();
        let r = add(&st, Kind::Reminder, "Call", "09:00");
        check_impl(&st, at("09:00"));
        act_impl(&st, r.id, Action::Done, at("09:00")).unwrap();
        assert!(shown(&st).is_empty());
        assert!(calendar::get(&st.store, r.id).unwrap().unwrap().done);
    }

    #[test]
    fn a_snoozed_item_due_again_is_shown_once() {
        let mut shown = Vec::new();
        let item = |title: &str| Item {
            id: 1,
            kind: Kind::Reminder,
            title: title.into(),
            date: "2026-10-30".into(),
            start_date: "2026-10-30".into(),
            time: None,
            end_time: None,
            remind_before: None,
            repeat: None,
            alert_at: Some(1),
            done: false,
        };
        merge(&mut shown, vec![item("old")]);
        merge(&mut shown, vec![item("new")]);
        assert_eq!(shown, vec![item("new")]);
    }

    #[test]
    fn actions_have_the_names_the_page_sends() {
        let parsed: Vec<Action> =
            serde_json::from_str(r#"["dismiss","snooze","snooze_hour","snooze_tomorrow","done"]"#)
                .unwrap();
        assert_eq!(
            parsed,
            [
                Action::Dismiss,
                Action::Snooze,
                Action::SnoozeHour,
                Action::SnoozeTomorrow,
                Action::Done
            ]
        );
    }

    #[test]
    fn quiet_hours_can_cross_midnight() {
        let m = |t: &str| date::minutes(t).unwrap();
        assert!(in_quiet_hours(m("23:00"), "22:00", "07:00"));
        assert!(in_quiet_hours(m("06:59"), "22:00", "07:00"));
        assert!(!in_quiet_hours(m("07:00"), "22:00", "07:00"));
        assert!(!in_quiet_hours(m("12:00"), "22:00", "07:00"));
        assert!(in_quiet_hours(m("13:30"), "13:00", "14:00"));
        assert!(!in_quiet_hours(m("14:00"), "13:00", "14:00"));
        assert!(!in_quiet_hours(m("13:00"), "13:00", "13:00"), "an empty range is never quiet");
        assert!(!in_quiet_hours(m("13:00"), "noon", "14:00"));
    }

    #[test]
    fn quiet_hours_only_count_when_turned_on() {
        let quiet = Settings { quiet_hours: true, ..Settings::default() };
        assert!(quiet_now(&quiet, at("23:30")));
        assert!(!quiet_now(&quiet, at("12:00")));
        assert!(!quiet_now(&Settings::default(), at("23:30")));
        let off_both = Settings { wait_for_full_screen: false, ..Settings::default() };
        assert!(!should_wait(&off_both, at("23:30")));
    }

    #[test]
    fn a_window_covering_its_screen_is_full_screen() {
        let screen = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(covers(screen, screen));
        assert!(covers(RECT { left: -8, top: -8, right: 1928, bottom: 1088 }, screen));
        assert!(!covers(RECT { left: 0, top: 0, right: 1920, bottom: 1040 }, screen), "taskbar");
    }
}
