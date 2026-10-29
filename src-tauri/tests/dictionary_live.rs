//! Reads a word, and a selected phrase, from a real window on screen, through UI Automation
//! and through text recognition. It needs a desktop to draw on, so it only runs when asked:
//! `cargo test --test dictionary_live -- --ignored`.

use mouce_lib::dictionary::hook::Point;
use mouce_lib::dictionary::{ocr, uia};
use mouce_lib::mouce::position::Rect;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, SendMessageW, TranslateMessage,
    MSG, PM_REMOVE, WINDOW_STYLE, WS_BORDER, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

/// Asks a text box where a character is (from the Controls area, not enabled here).
const EM_POSFROMCHAR: u32 = 0x00D6;
/// Selects characters in a text box.
const EM_SETSEL: u32 = 0x00B1;
const TEXT: &str = "It rained. The ubiquitous pigeons gathered in the square!";
const WORD: &str = "ubiquitous";
const PHRASE: &str = "ubiquitous pigeons";

static CLOSE: AtomicBool = AtomicBool::new(false);

// The app runs per-monitor DPI aware (from its manifest), so every point is in physical
// pixels; this test process says the same here. Declared directly to keep the HiDpi area
// out of the app's `windows` features.
#[link(name = "user32")]
unsafe extern "system" {
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    fn GetDpiForWindow(window: HWND) -> u32;
}
const PER_MONITOR_AWARE_V2: isize = -4;

/// Shows `TEXT` in a plain Windows text box, sends the box, the screen point in the middle
/// of `WORD` and the display scale, and keeps the box up until `CLOSE` is set.
fn show_text(at: mpsc::Sender<(isize, Point, f64)>) {
    unsafe {
        let style = WS_POPUP | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(0x0004); // ES_MULTILINE
        let edit = CreateWindowExW(
            WS_EX_TOPMOST,
            w!("EDIT"),
            &HSTRING::from(TEXT),
            style,
            200,
            200,
            900,
            120,
            None,
            None,
            None,
            None,
        )
        .expect("create a text box");
        let char_at = |i: usize| {
            let packed = SendMessageW(edit, EM_POSFROMCHAR, Some(WPARAM(i)), Some(LPARAM(0))).0;
            let mut p =
                POINT { x: (packed & 0xffff) as i16 as i32, y: (packed >> 16) as i16 as i32 };
            let _ = ClientToScreen(edit, &mut p);
            p
        };
        let start = TEXT.find(WORD).unwrap();
        let (a, b) = (char_at(start), char_at(start + WORD.len()));
        let scale = GetDpiForWindow(edit) as f64 / 96.0;
        let middle = Point { x: (a.x + b.x) / 2, y: a.y + (6.0 * scale) as i32 };
        at.send((edit.0 as isize, middle, scale)).unwrap();
        let mut msg = MSG::default();
        while !CLOSE.load(Ordering::SeqCst) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = DestroyWindow(edit);
    }
}

#[test]
#[ignore]
fn reads_a_word_from_a_real_window() {
    unsafe { SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) };
    let (to, from) = mpsc::channel();
    let window = std::thread::spawn(move || show_text(to));
    let (edit, at, scale) = from.recv().unwrap();
    std::thread::sleep(Duration::from_millis(400));

    let reader = uia::Reader::new().expect("UI Automation");
    let reading = reader.read(at);
    let screen = Rect { x: 0, y: 0, w: 1200 * scale as u32, h: 800 * scale as u32 };
    let seen = ocr::read(ocr::area_around(at, screen, scale), at, scale);
    let beside = reader.read(Point { x: at.x, y: at.y + (60.0 * scale) as i32 });
    // Select the phrase, the way a user would before Ctrl + right-clicking inside it.
    let start = TEXT.find(PHRASE).unwrap();
    let (from_char, to_char) = (WPARAM(start), LPARAM((start + PHRASE.len()) as isize));
    unsafe { SendMessageW(HWND(edit as _), EM_SETSEL, Some(from_char), Some(to_char)) };
    let selected = reader.read(at);

    CLOSE.store(true, Ordering::SeqCst);
    window.join().unwrap();

    assert_eq!(
        reading,
        uia::Reading::Word {
            word: WORD.into(),
            context: "The ubiquitous pigeons gathered in the square!".into()
        },
        "UI Automation"
    );
    let (word, line) = seen.expect("text recognition ran").expect("a word was read");
    assert_eq!(word, WORD, "text recognition");
    assert!(line.contains("pigeons"), "{line}");
    assert_eq!(beside, uia::Reading::Nothing, "empty space below the text is no word");
    assert_eq!(
        selected,
        uia::Reading::Word {
            word: PHRASE.into(),
            context: "The ubiquitous pigeons gathered in the square!".into()
        },
        "a click inside a selection reads the selected phrase"
    );
}
