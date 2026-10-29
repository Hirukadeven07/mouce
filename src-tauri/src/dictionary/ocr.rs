//! Reads the word under the pointer from the screen's pixels, for apps that do not share
//! their text (pictures, games, some PDF viewers). It uses the text recognition built into
//! Windows, which works offline. The small picture it reads stays in memory and is
//! dropped as soon as the word is found.

use super::hook::Point;
use super::text::{self, Placed};
use crate::mouce::position::Rect;
use crate::screen;
use image::imageops::FilterType;
use image::{ImageBuffer, Rgba};
use windows::core::{Interface, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Win32::System::WinRT::IMemoryBufferByteAccess;

/// The area read around the pointer, in logical pixels: a line or two of text.
const AREA: (f64, f64) = (360.0, 72.0);
/// How far from a word the pointer may be, in pixels of the picture read.
const SLACK: f32 = 4.0;

/// The part of the screen to read around `at`, kept on its monitor (`monitor`, physical
/// pixels, with its `scale`).
pub fn area_around(at: Point, monitor: Rect, scale: f64) -> Rect {
    let w = ((AREA.0 * scale).round() as u32).min(monitor.w);
    let h = ((AREA.1 * scale).round() as u32).min(monitor.h);
    let keep = |p: i32, origin: i32, extent: u32, size: u32| {
        p.max(origin).min(origin + extent as i32 - size as i32)
    };
    Rect {
        x: keep(at.x - w as i32 / 2, monitor.x, monitor.w, w),
        y: keep(at.y - h as i32 / 2, monitor.y, monitor.h, h),
        w,
        h,
    }
}

/// Small text reads far better enlarged; at 150% scaling and up it is big enough.
fn enlargement(scale: f64) -> u32 {
    if scale < 1.5 {
        2
    } else {
        1
    }
}

/// The word at `at` (inside `area`) and the line it is on, read from the screen.
pub fn read(area: Rect, at: Point, scale: f64) -> Result<Option<(String, String)>, String> {
    let frame = screen::grab(area)?;
    let k = enlargement(scale);
    let (w, h) = (frame.width * k, frame.height * k);
    let pixels = if k == 1 {
        frame.bgra
    } else {
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(frame.width, frame.height, frame.bgra)
                .ok_or("The screen picture has the wrong size")?;
        image::imageops::resize(&image, w, h, FilterType::Triangle).into_raw()
    };
    let lines = recognize(&pixels, w, h).map_err(|e| format!("Could not read the text: {e}"))?;
    let (x, y) = (((at.x - area.x) as u32 * k) as f32, ((at.y - area.y) as u32 * k) as f32);
    for (line, words) in &lines {
        if let Some(i) = text::word_at(words, x, y, SLACK * k as f32) {
            if let Some(word) = text::clean_word(&words[i].text) {
                let context = text::sentence_around(line, 0, &word);
                return Ok(Some((word, context)));
            }
        }
    }
    Ok(None)
}

/// The lines of text in a picture (top-down BGRA rows), each with its words and where they are.
fn recognize(bgra: &[u8], w: u32, h: u32) -> windows::core::Result<Vec<(String, Vec<Placed>)>> {
    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Ignore,
    )?;
    {
        let buffer = bitmap.LockBuffer(BitmapBufferAccessMode::Write)?;
        let plane = buffer.GetPlaneDescription(0)?;
        let reference = buffer.CreateReference()?;
        let access: IMemoryBufferByteAccess = reference.cast()?;
        let mut data = std::ptr::null_mut();
        let mut capacity = 0u32;
        // SAFETY: the buffer is locked for writing until `reference` and `buffer` close
        // below; each row is copied inside its stride and the buffer's capacity.
        unsafe {
            access.GetBuffer(&mut data, &mut capacity)?;
            let row = w as usize * 4;
            let stride = plane.Stride as usize;
            let start = plane.StartIndex as usize;
            for y in 0..h as usize {
                let to = start + y * stride;
                if to + row > capacity as usize {
                    break;
                }
                std::ptr::copy_nonoverlapping(bgra[y * row..].as_ptr(), data.add(to), row);
            }
        }
        reference.Close()?;
        buffer.Close()?;
    }
    let engine = Language::CreateLanguage(&HSTRING::from("en-US"))
        .and_then(|english| OcrEngine::TryCreateFromLanguage(&english))
        .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut words = Vec::new();
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            let text = word.Text()?.to_string();
            words.push(Placed { text, x: r.X, y: r.Y, w: r.Width, h: r.Height });
        }
        lines.push((line.Text()?.to_string(), words));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn the_area_is_centred_on_the_pointer_and_scaled() {
        let a = area_around(Point { x: 960, y: 540 }, MONITOR, 1.0);
        assert_eq!(a, Rect { x: 780, y: 504, w: 360, h: 72 });
        let big = area_around(Point { x: 960, y: 540 }, MONITOR, 2.0);
        assert_eq!((big.w, big.h), (720, 144));
    }

    #[test]
    fn the_area_stays_on_the_monitor() {
        let corner = area_around(Point { x: 3, y: 1078 }, MONITOR, 1.0);
        assert_eq!((corner.x, corner.y), (0, 1080 - 72));
        let second = Rect { x: -1280, y: 0, w: 1280, h: 1024 };
        let left = area_around(Point { x: -1275, y: 10 }, second, 1.0);
        assert_eq!((left.x, left.y), (-1280, 0));
    }

    #[test]
    fn small_text_is_enlarged_before_reading() {
        assert_eq!(enlargement(1.0), 2);
        assert_eq!(enlargement(1.25), 2);
        assert_eq!(enlargement(1.5), 1);
    }
}
