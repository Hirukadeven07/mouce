use crate::mouce;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const PAUSE_LABEL: &str = "Pause mouce";
const SHOW_LABEL: &str = "Show mouce";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let home_item = MenuItem::with_id(app, "home", "Open mouce", true, None::<&str>)?;
    let chat_item = MenuItem::with_id(app, "chat", "Chat with mouce", true, None::<&str>)?;
    let circle_item = MenuItem::with_id(app, "circle", "Circle to look up", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let pause_item = MenuItem::with_id(app, "pause", PAUSE_LABEL, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &home_item,
            &chat_item,
            &circle_item,
            &settings_item,
            &pause_item,
            &separator,
            &quit_item,
        ],
    )?;

    let pause_for_handler = pause_item.clone();
    let mut tray = TrayIconBuilder::with_id("mouce-tray")
        .tooltip("mouce")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "home" => mouce::open_home(app, mouce::HomeTab::Home),
            "chat" => mouce::open_chat(app),
            "circle" => crate::circle::start(app),
            "settings" => mouce::open_home(app, mouce::HomeTab::Settings),
            "pause" => toggle_mouce(app, &pause_for_handler),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                mouce::open_home(tray.app_handle(), mouce::HomeTab::Home);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// What the "pause" menu entry does next, given whether the mouce is on screen now.
#[derive(Debug, PartialEq, Eq)]
struct Toggle {
    /// True: bring the mouce back; false: hide it.
    show: bool,
    /// The menu text after the toggle.
    label: &'static str,
}

fn toggle_for(visible_now: bool) -> Toggle {
    if visible_now {
        Toggle { show: false, label: SHOW_LABEL }
    } else {
        Toggle { show: true, label: PAUSE_LABEL }
    }
}

fn toggle_mouce(app: &AppHandle, pause_item: &MenuItem<tauri::Wry>) {
    let Some(window) = app.get_webview_window(mouce::MOUCE_LABEL) else { return };
    // `is_visible` asks Windows itself (IsWindowVisible), so it stays correct even though
    // the mouce is shown and hidden with raw Win32 calls (see `mouce::show_without_focus`).
    let next = toggle_for(window.is_visible().unwrap_or(true));
    let result = if next.show {
        mouce::show_without_focus(&window)
    } else {
        mouce::hide(&window)
    };
    match result {
        Ok(()) => {
            let _ = pause_item.set_text(next.label);
        }
        Err(e) => log::warn!("could not toggle the mouce: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visible_mouce_is_hidden_and_the_menu_offers_to_show_it() {
        assert_eq!(toggle_for(true), Toggle { show: false, label: "Show mouce" });
    }

    #[test]
    fn a_hidden_mouce_is_shown_and_the_menu_offers_to_pause_it() {
        assert_eq!(toggle_for(false), Toggle { show: true, label: "Pause mouce" });
    }
}
