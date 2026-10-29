//! Watches the mouse for one gesture, Ctrl + right-click, so a word can be looked up from
//! any app. The hook runs only while the dictionary is turned on.
//!
//! Windows shows mouce each mouse event before the app under the pointer gets it. mouce
//! looks only at which button went down and where. It never moves the pointer or makes
//! clicks of its own, and the one thing it changes is to hold back the Ctrl + right-click
//! (its press and its release), so the app's own menu does not open over the word. Every
//! other click goes on to the app untouched.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KillTimer, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    HC_ACTION, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN,
};

/// A point on the screen, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// What the hook tells the dictionary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// Ctrl + right-click: look up the word here.
    LookUp(Point),
    /// A click or a scroll while the card is open (to close it when that is outside it).
    Activity(Point),
}

/// The mouse events the hook tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    RightDown,
    RightUp,
    OtherDown,
    Wheel,
}

/// The modifier keys held during a right-click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub gesture: Option<Gesture>,
    /// Hold the event back from the app under the pointer.
    pub swallow: bool,
}

const PASS: Decision = Decision { gesture: None, swallow: false };

/// What to do with one mouse event. `holding_release` is set from a swallowed Ctrl +
/// right press until its release, which is swallowed too (many apps open their menu on
/// the release). Clicks made by software are passed on and never start a look-up.
pub fn decide(
    button: Button,
    at: Point,
    keys: Modifiers,
    injected: bool,
    card_open: bool,
    holding_release: &mut bool,
) -> Decision {
    match button {
        Button::RightDown if !injected && keys.ctrl && !keys.shift && !keys.alt => {
            *holding_release = true;
            Decision { gesture: Some(Gesture::LookUp(at)), swallow: true }
        }
        Button::RightUp if *holding_release => {
            *holding_release = false;
            Decision { gesture: None, swallow: true }
        }
        Button::RightDown | Button::OtherDown | Button::Wheel if card_open => {
            Decision { gesture: Some(Gesture::Activity(at)), swallow: false }
        }
        _ => PASS,
    }
}

/// Set while the word card is on screen, so other clicks and scrolls are reported to close it.
pub static CARD_OPEN: AtomicBool = AtomicBool::new(false);
/// Where gestures go. The hook procedure is a plain function, so it reaches it here.
static GESTURES: Mutex<Option<Sender<Gesture>>> = Mutex::new(None);
static HOLDING_RELEASE: AtomicBool = AtomicBool::new(false);

/// How often the hook thread checks whether it should stop.
const STOP_CHECK_MS: u32 = 200;

/// Whether a key is down right now. Only the modifier keys are ever asked about.
fn is_down(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    // SAFETY: reads one key's state; no pointers are involved.
    (unsafe { GetAsyncKeyState(key.0 as i32) } as u16) & 0x8000 != 0
}

fn held_modifiers() -> Modifiers {
    Modifiers { ctrl: is_down(VK_CONTROL), shift: is_down(VK_SHIFT), alt: is_down(VK_MENU) }
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let button = match wparam.0 as u32 {
            WM_RBUTTONDOWN => Some(Button::RightDown),
            WM_RBUTTONUP => Some(Button::RightUp),
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => Some(Button::OtherDown),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Some(Button::Wheel),
            _ => None,
        };
        if let Some(button) = button {
            // SAFETY: for WH_MOUSE_LL with HC_ACTION, Windows passes a valid MSLLHOOKSTRUCT.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let at = Point { x: info.pt.x, y: info.pt.y };
            let keys =
                if button == Button::RightDown { held_modifiers() } else { Modifiers::default() };
            let mut holding = HOLDING_RELEASE.load(Ordering::SeqCst);
            let decision = decide(
                button,
                at,
                keys,
                info.flags & LLMHF_INJECTED != 0,
                CARD_OPEN.load(Ordering::SeqCst),
                &mut holding,
            );
            HOLDING_RELEASE.store(holding, Ordering::SeqCst);
            if let Some(gesture) = decision.gesture {
                if let Some(to) = GESTURES.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    let _ = to.send(gesture);
                }
            }
            if decision.swallow {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on unchanged, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The running hook. Dropping it removes the hook and waits for its thread to end.
pub struct Hook {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        HOLDING_RELEASE.store(false, Ordering::SeqCst);
    }
}

/// Starts watching for Ctrl + right-click, sending gestures to `to`.
pub fn start(to: Sender<Gesture>) -> std::io::Result<Hook> {
    *GESTURES.lock().unwrap_or_else(|e| e.into_inner()) = Some(to);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("mouce-dictionary-hook".into())
        .spawn(move || run(&flag))?;
    Ok(Hook { stop, thread: Some(thread) })
}

/// Installs the hook and pumps this thread's messages, which is when Windows calls it,
/// until told to stop.
fn run(stop: &AtomicBool) {
    // SAFETY: the hook procedure has the signature Windows expects and lives for the whole
    // program. A low-level hook needs no module handle. The hook and timer belong to this
    // thread and are removed before it ends.
    unsafe {
        let hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), None, 0) {
            Ok(hook) => hook,
            Err(e) => {
                log::warn!("could not watch for Ctrl + right-click: {e}");
                return;
            }
        };
        // Wakes the loop below now and then, so it can see the stop flag.
        let timer = SetTimer(None, 0, STOP_CHECK_MS, None);
        let mut msg = MSG::default();
        while !stop.load(Ordering::SeqCst) {
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                break;
            }
        }
        let _ = KillTimer(None, timer);
        let _ = UnhookWindowsHookEx(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Point = Point { x: 5, y: 7 };
    const CTRL: Modifiers = Modifiers { ctrl: true, shift: false, alt: false };

    #[test]
    fn ctrl_right_click_looks_up_and_holds_back_press_and_release() {
        let mut holding = false;
        let down = decide(Button::RightDown, AT, CTRL, false, false, &mut holding);
        assert_eq!(down, Decision { gesture: Some(Gesture::LookUp(AT)), swallow: true });
        assert!(holding);
        // The release is held back even if Ctrl was let go first.
        let up = decide(Button::RightUp, AT, Modifiers::default(), false, false, &mut holding);
        assert_eq!(up, Decision { gesture: None, swallow: true });
        assert!(!holding);
    }

    #[test]
    fn a_plain_right_click_is_never_touched() {
        let mut holding = false;
        let none = Modifiers::default();
        assert_eq!(decide(Button::RightDown, AT, none, false, false, &mut holding), PASS);
        assert_eq!(decide(Button::RightUp, AT, none, false, false, &mut holding), PASS);
    }

    #[test]
    fn other_modifier_mixes_and_software_clicks_pass() {
        let mut holding = false;
        for keys in [
            Modifiers { shift: true, ..CTRL },
            Modifiers { alt: true, ..CTRL },
            Modifiers { ctrl: false, shift: true, alt: false },
        ] {
            assert_eq!(decide(Button::RightDown, AT, keys, false, false, &mut holding), PASS);
        }
        assert_eq!(decide(Button::RightDown, AT, CTRL, true, false, &mut holding), PASS);
        assert!(!holding);
    }

    #[test]
    fn clicks_and_scrolls_are_reported_only_while_the_card_is_open() {
        let mut holding = false;
        let none = Modifiers::default();
        assert_eq!(decide(Button::OtherDown, AT, none, false, false, &mut holding), PASS);
        assert_eq!(decide(Button::Wheel, AT, none, false, false, &mut holding), PASS);
        let open = decide(Button::OtherDown, AT, none, false, true, &mut holding);
        assert_eq!(open, Decision { gesture: Some(Gesture::Activity(AT)), swallow: false });
        let scroll = decide(Button::Wheel, AT, none, false, true, &mut holding);
        assert_eq!(scroll, Decision { gesture: Some(Gesture::Activity(AT)), swallow: false });
        let right = decide(Button::RightDown, AT, none, false, true, &mut holding);
        assert_eq!(right.gesture, Some(Gesture::Activity(AT)));
        assert!(!right.swallow, "a plain right-click still reaches the app");
    }
}
