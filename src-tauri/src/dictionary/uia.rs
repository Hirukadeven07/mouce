//! Reads the word under the pointer (or the phrase selected there) through Windows UI
//! Automation, the interface screen readers use. It is used read-only: mouce asks for text,
//! the current selection and where they are on screen, and never selects, scrolls, types
//! into or presses anything. Password boxes are left alone.

use super::hook::Point;
use super::text;
use windows::core::Interface;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextRange, IUIAutomationTreeWalker, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TextUnit_Paragraph, TextUnit_Word, UIA_TextPatternId,
};

/// How long to wait for an app that is slow to answer, in milliseconds.
const APP_TIMEOUT_MS: u32 = 1500;
/// How many levels up from the element under the pointer to look for its text.
const MAX_PARENTS: usize = 8;
/// How far outside a word's box the pointer may be, in pixels.
const SLACK: f64 = 2.0;
/// The most text read from the paragraph around a word.
const MAX_PARAGRAPH: i32 = 4000;
/// The most selected text read; a phrase is far shorter.
const MAX_SELECTION: i32 = 200;

/// What was found under the pointer.
#[derive(Debug, PartialEq, Eq)]
pub enum Reading {
    /// A word, or a phrase the user selected, and the sentence it is in.
    Word { word: String, context: String },
    /// A password box: mouce does not read it, by any means.
    Secret,
    /// No text the app would share; reading the pixels may still find a word.
    Nothing,
}

pub struct Reader {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
}

impl Reader {
    /// Must be made, and used, on one thread that stays alive (the dictionary's own).
    pub fn new() -> windows::core::Result<Reader> {
        // SAFETY: plain COM set-up on this thread, which keeps the reader to itself.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            if let Ok(timed) = automation.cast::<IUIAutomation2>() {
                let _ = timed.SetConnectionTimeout(APP_TIMEOUT_MS);
                let _ = timed.SetTransactionTimeout(APP_TIMEOUT_MS);
            }
            let walker = automation.ControlViewWalker()?;
            Ok(Reader { automation, walker })
        }
    }

    /// The word at `at` and the sentence it is in.
    pub fn read(&self, at: Point) -> Reading {
        // SAFETY: COM calls on objects owned by this reader; failures come back as errors.
        match unsafe { self.read_inner(POINT { x: at.x, y: at.y }) } {
            Ok(reading) => reading,
            Err(e) => {
                log::debug!("UI Automation found no text under the pointer: {e}");
                Reading::Nothing
            }
        }
    }

    unsafe fn read_inner(&self, pt: POINT) -> windows::core::Result<Reading> {
        let element = unsafe { self.automation.ElementFromPoint(pt)? };
        if unsafe { element.CurrentIsPassword() }.is_ok_and(|p| p.as_bool()) {
            return Ok(Reading::Secret);
        }
        let mut current: Option<IUIAutomationElement> = Some(element);
        for _ in 0..MAX_PARENTS {
            let Some(here) = current else { break };
            let pattern =
                unsafe { here.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) };
            if let Ok(pattern) = pattern {
                return unsafe { word_in(&pattern, pt) };
            }
            current = unsafe { self.walker.GetParentElement(&here) }.ok();
        }
        Ok(Reading::Nothing)
    }
}

unsafe fn word_in(pattern: &IUIAutomationTextPattern, pt: POINT) -> windows::core::Result<Reading> {
    if let Some(reading) = unsafe { selection_at(pattern, pt) } {
        return Ok(reading);
    }
    let range = unsafe { pattern.RangeFromPoint(pt)? };
    let word = unsafe { range.Clone()? };
    unsafe { word.ExpandToEnclosingUnit(TextUnit_Word)? };
    // The nearest word is returned even when the pointer is over a picture or empty space.
    let boxes = unsafe { boxes_of(&word) };
    if !text::boxes_hold(&boxes, pt.x as f64, pt.y as f64, SLACK) {
        return Ok(Reading::Nothing);
    }
    let raw = unsafe { word.GetText(64)? }.to_string();
    let Some(clean) = text::clean_word(&raw) else { return Ok(Reading::Nothing) };
    let context = unsafe { paragraph_sentence(&range, &word, &clean) }.unwrap_or_default();
    Ok(Reading::Word { word: clean, context })
}

/// The selected text under the pointer, when the user selected a phrase and clicked in it.
/// The selection is only read, never changed.
unsafe fn selection_at(pattern: &IUIAutomationTextPattern, pt: POINT) -> Option<Reading> {
    let selected = unsafe { pattern.GetSelection() }.ok()?;
    for i in 0..unsafe { selected.Length() }.unwrap_or(0) {
        let Ok(range) = (unsafe { selected.GetElement(i) }) else { continue };
        // An empty selection (just the caret) has no boxes; it is not what was pointed at.
        let boxes = unsafe { boxes_of(&range) };
        if boxes.is_empty() || !text::boxes_hold(&boxes, pt.x as f64, pt.y as f64, SLACK) {
            continue;
        }
        let raw = unsafe { range.GetText(MAX_SELECTION) }.ok()?.to_string();
        let phrase = text::clean_phrase(&raw)?;
        let context = unsafe { paragraph_sentence(&range, &range, &phrase) }.unwrap_or_default();
        return Some(Reading::Word { word: phrase, context });
    }
    None
}

/// The sentence around `word`, from the paragraph that holds it.
unsafe fn paragraph_sentence(
    range: &IUIAutomationTextRange,
    word: &IUIAutomationTextRange,
    clean: &str,
) -> windows::core::Result<String> {
    let paragraph = unsafe { range.Clone()? };
    unsafe { paragraph.ExpandToEnclosingUnit(TextUnit_Paragraph)? };
    let text = unsafe { paragraph.GetText(MAX_PARAGRAPH)? }.to_string();
    // The text before the word says where in the paragraph it is.
    let before = unsafe { paragraph.Clone()? };
    unsafe {
        before.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            word,
            TextPatternRangeEndpoint_Start,
        )?
    };
    let offset = unsafe { before.GetText(MAX_PARAGRAPH)? }.to_string().chars().count();
    Ok(text::sentence_around(&text, offset, clean))
}

/// The screen boxes of `range` as (left, top, width, height) in turn; empty if unknown.
unsafe fn boxes_of(range: &IUIAutomationTextRange) -> Vec<f64> {
    let Ok(array) = (unsafe { range.GetBoundingRectangles() }) else { return Vec::new() };
    if array.is_null() {
        return Vec::new();
    }
    let values = unsafe { doubles(array) }.unwrap_or_default();
    let _ = unsafe { SafeArrayDestroy(array) };
    values
}

/// Copies a one-dimensional array of doubles.
unsafe fn doubles(array: *mut SAFEARRAY) -> windows::core::Result<Vec<f64>> {
    let (low, high) = unsafe { (SafeArrayGetLBound(array, 1)?, SafeArrayGetUBound(array, 1)?) };
    let count = (high - low + 1).max(0) as usize;
    let mut data = std::ptr::null_mut();
    unsafe { SafeArrayAccessData(array, &mut data)? };
    // SAFETY: the array holds `count` doubles and stays locked while they are copied.
    let values = unsafe { std::slice::from_raw_parts(data as *const f64, count) }.to_vec();
    let _ = unsafe { SafeArrayUnaccessData(array) };
    Ok(values)
}
