//! The keyboard shortcut that opens the chat from any app. Windows tells mouce only when
//! this exact key combination is pressed; no other typing is seen.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut};

pub const DEFAULT_CHAT_HOTKEY: &str = "Ctrl+Alt+M";

/// Checks a shortcut the user typed, such as "Ctrl+Alt+M". Blank means no shortcut. A
/// shortcut needs Ctrl, Alt or the Windows key, so it can never swallow ordinary typing.
pub fn parse(text: &str) -> Result<Option<Shortcut>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let shortcut: Shortcut = text.parse().map_err(|_| {
        format!("\"{text}\" isn't a shortcut mouce understands. Try something like Ctrl+Alt+M.")
    })?;
    if !shortcut.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        return Err(
            "A shortcut needs Ctrl, Alt or the Windows key, so it doesn't get in the way of typing"
                .to_string(),
        );
    }
    Ok(Some(shortcut))
}

/// Swaps the chat shortcut from `old` to `new`. If `new` cannot be used (for example,
/// another app already has it), `old` is put back and the error says why.
pub fn replace(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    let next = parse(new)?;
    let previous = parse(old).ok().flatten();
    let shortcuts = app.global_shortcut();
    if let Some(p) = previous {
        let _ = shortcuts.unregister(p);
    }
    if let Some(n) = next {
        if let Err(e) = shortcuts.register(n) {
            if let Some(p) = previous {
                let _ = shortcuts.register(p);
            }
            return Err(format!("mouce couldn't use {}: {e}", new.trim()));
        }
    }
    Ok(())
}

/// Starts listening for the chat shortcut.
pub fn register(app: &AppHandle, text: &str) -> Result<(), String> {
    replace(app, "", text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Code;

    #[test]
    fn the_default_is_ctrl_alt_m() {
        let s = parse(DEFAULT_CHAT_HOTKEY).unwrap().unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(s.key, Code::KeyM);
    }

    #[test]
    fn spelling_and_spacing_do_not_matter() {
        assert_eq!(parse(" ctrl + alt + m ").unwrap(), parse("Ctrl+Alt+M").unwrap());
        assert_eq!(parse("Control+Shift+K").unwrap().unwrap().key, Code::KeyK);
        assert!(parse("Super+F2").unwrap().is_some());
    }

    #[test]
    fn blank_means_no_shortcut() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("  ").unwrap(), None);
    }

    #[test]
    fn a_shortcut_without_ctrl_alt_or_windows_is_refused() {
        for text in ["M", "Shift+M", "F5"] {
            let err = parse(text).unwrap_err();
            assert!(err.contains("needs Ctrl, Alt or the Windows key"), "{text}: {err}");
        }
    }

    #[test]
    fn nonsense_is_refused_with_an_example() {
        for text in ["Ctrl+Alt+", "Ctrl+Banana", "Ctrl+M+K"] {
            let err = parse(text).unwrap_err();
            assert!(err.contains("Try something like Ctrl+Alt+M"), "{text}: {err}");
        }
    }
}
