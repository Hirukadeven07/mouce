use crate::star;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const PAUSE_LABEL: &str = "Pause star";
const SHOW_LABEL: &str = "Show star";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let pause_item = MenuItem::with_id(app, "pause", PAUSE_LABEL, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &pause_item, &separator, &quit_item])?;

    let pause_for_handler = pause_item.clone();
    let mut tray = TrayIconBuilder::with_id("star-tray")
        .tooltip("Star")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "settings" => star::open_settings(app),
            "pause" => toggle_star(app, &pause_for_handler),
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
                star::open_settings(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// What the "pause" menu entry does next, given whether the star is on screen now.
#[derive(Debug, PartialEq, Eq)]
struct Toggle {
    /// True: bring the star back; false: hide it.
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

fn toggle_star(app: &AppHandle, pause_item: &MenuItem<tauri::Wry>) {
    let Some(window) = app.get_webview_window(star::STAR_LABEL) else { return };
    // `is_visible` asks Windows itself (IsWindowVisible), so it stays correct even though
    // the star is shown and hidden with raw Win32 calls (see `star::show_without_focus`).
    let next = toggle_for(window.is_visible().unwrap_or(true));
    let result = if next.show {
        star::show_without_focus(&window)
    } else {
        star::hide(&window)
    };
    match result {
        Ok(()) => {
            let _ = pause_item.set_text(next.label);
        }
        Err(e) => eprintln!("could not toggle the star: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visible_star_is_hidden_and_the_menu_offers_to_show_it() {
        assert_eq!(toggle_for(true), Toggle { show: false, label: "Show star" });
    }

    #[test]
    fn a_hidden_star_is_shown_and_the_menu_offers_to_pause_it() {
        assert_eq!(toggle_for(false), Toggle { show: true, label: "Pause star" });
    }
}
