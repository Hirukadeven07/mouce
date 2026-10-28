//! "Look at my screen" and "Circle to look up": read-only copies of one monitor's pixels,
//! kept in memory only.
//!
//! The pixels are read with GDI (`BitBlt` from the screen into a memory bitmap), cropped,
//! scaled down and encoded as JPEG in memory. Nothing is written to disk, and the mouce and
//! chat windows ask Windows to leave them out of the copy, so the AI sees what is behind them.

use crate::mouce::position::Rect;
use base64::Engine as _;
use serde::Deserialize;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageBuffer, Rgb};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

/// Longest side of the image sent to the AI. Providers downscale anything larger anyway,
/// and a smaller image keeps requests quick.
pub const MAX_EDGE: u32 = 1568;
/// Longest side of the small preview of a circled area shown in the chat.
pub const THUMBNAIL_EDGE: u32 = 240;
/// The smallest circled area, in screen pixels, worth asking about.
pub const MIN_SELECTION: u32 = 8;
/// Extra screen pixels kept around a circled area, so its edges are not cut off.
pub const SELECTION_PADDING: u32 = 12;
const JPEG_QUALITY: u8 = 80;

/// A frozen picture of one monitor (top-down BGRA rows), kept while the user circles part
/// of it.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Part of an image as fractions (0 to 1) of its width and height.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A screen image ready to send: base64 JPEG and its size after scaling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenImage {
    pub base64: String,
    pub width: u32,
    pub height: u32,
}

/// Asks Windows to leave `window` out of screen copies (Windows 10 2004 and later).
pub fn exclude_from_capture(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?;
    // SAFETY: `hwnd` is the live handle of a window owned by this process.
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|e| tauri::Error::Io(std::io::Error::other(e)))
}

/// Leaves `window` out of screen copies until dropped, then lets it be copied again. For a
/// window that should stay out of one picture only, not out of the user's own screenshots.
pub struct CaptureExclusion(tauri::WebviewWindow);

impl CaptureExclusion {
    pub fn new(window: tauri::WebviewWindow) -> tauri::Result<CaptureExclusion> {
        exclude_from_capture(&window)?;
        Ok(CaptureExclusion(window))
    }
}

impl Drop for CaptureExclusion {
    fn drop(&mut self) {
        if let Ok(hwnd) = self.0.hwnd() {
            // SAFETY: `hwnd` is the live handle of a window owned by this process.
            let _ = unsafe { SetWindowDisplayAffinity(hwnd, WDA_NONE) };
        }
    }
}

/// Copies the pixels of `area` (physical pixels, virtual-screen coordinates) as
/// top-down BGRA rows.
fn copy_pixels(area: Rect) -> Result<Vec<u8>, String> {
    let (w, h) = (area.w as i32, area.h as i32);
    if w <= 0 || h <= 0 {
        return Err("The screen has no size".to_string());
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // Negative height asks for top-down rows.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; area.w as usize * area.h as usize * 4];
    // SAFETY: every handle is created here, checked, and released before returning; the
    // pixel buffer holds exactly `w * h` 32-bit pixels, as `info` describes.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("Could not read the screen".to_string());
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let result = if memory.is_invalid() || bitmap.is_invalid() {
            Err("Could not read the screen".to_string())
        } else {
            let previous = SelectObject(memory, bitmap.into());
            let copied = BitBlt(memory, 0, 0, w, h, Some(screen), area.x, area.y, SRCCOPY);
            SelectObject(memory, previous);
            match copied {
                Err(e) => Err(format!("Could not read the screen: {e}")),
                Ok(()) => {
                    let rows = GetDIBits(
                        memory,
                        bitmap,
                        0,
                        h as u32,
                        Some(pixels.as_mut_ptr().cast()),
                        &mut info,
                        DIB_RGB_COLORS,
                    );
                    if rows == h {
                        Ok(())
                    } else {
                        Err("Could not read the screen".to_string())
                    }
                }
            }
        };
        if !bitmap.is_invalid() {
            let _ = DeleteObject(bitmap.into());
        }
        if !memory.is_invalid() {
            let _ = DeleteDC(memory);
        }
        ReleaseDC(None, screen);
        result?;
    }
    Ok(pixels)
}

/// The size that fits inside `max_edge` on both sides, keeping the aspect ratio.
fn fit_within(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= max_edge {
        return (w, h);
    }
    let scale = max_edge as f64 / longest as f64;
    let fit = |v: u32| ((v as f64 * scale).round() as u32).max(1);
    (fit(w), fit(h))
}

/// Scales top-down BGRA pixels to fit `max_edge` and encodes them as base64 JPEG.
pub fn encode(bgra: &[u8], w: u32, h: u32, max_edge: u32) -> Result<ScreenImage, String> {
    if bgra.len() != w as usize * h as usize * 4 {
        return Err("The screen image has the wrong size".to_string());
    }
    let rgb: Vec<u8> = bgra
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0]])
        .collect();
    let full: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(w, h, rgb).ok_or("The screen image has the wrong size")?;
    let (tw, th) = fit_within(w, h, max_edge);
    let scaled = if (tw, th) == (w, h) {
        full
    } else {
        image::imageops::resize(&full, tw, th, FilterType::Triangle)
    };
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode(scaled.as_raw(), tw, th, ExtendedColorType::Rgb8)
        .map_err(|e| format!("Could not encode the screen image: {e}"))?;
    Ok(ScreenImage {
        base64: base64::engine::general_purpose::STANDARD.encode(jpeg),
        width: tw,
        height: th,
    })
}

/// Copies `area` of the screen and returns it ready to send to the AI.
pub fn capture(area: Rect) -> Result<ScreenImage, String> {
    let frame = grab(area)?;
    encode(&frame.bgra, frame.width, frame.height, MAX_EDGE)
}

/// Copies `area` of the screen as a frame to circle part of.
pub fn grab(area: Rect) -> Result<Frame, String> {
    Ok(Frame { width: area.w, height: area.h, bgra: copy_pixels(area)? })
}

/// The pixels a selection covers in a `width` x `height` image, grown by `padding` on every
/// side and kept inside the image. `None` when the selection is smaller than
/// `MIN_SELECTION` pixels either way, or not a real number.
pub fn selection_rect(sel: Selection, width: u32, height: u32, padding: u32) -> Option<Rect> {
    if ![sel.x, sel.y, sel.w, sel.h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (w, h) = (width as f64, height as f64);
    let left = (sel.x.clamp(0.0, 1.0) * w).floor();
    let top = (sel.y.clamp(0.0, 1.0) * h).floor();
    let right = ((sel.x + sel.w).clamp(0.0, 1.0) * w).ceil();
    let bottom = ((sel.y + sel.h).clamp(0.0, 1.0) * h).ceil();
    if right - left < MIN_SELECTION as f64 || bottom - top < MIN_SELECTION as f64 {
        return None;
    }
    let pad = padding as f64;
    let (left, top) = ((left - pad).max(0.0), (top - pad).max(0.0));
    let (right, bottom) = ((right + pad).min(w), (bottom + pad).min(h));
    Some(Rect {
        x: left as i32,
        y: top as i32,
        w: (right - left) as u32,
        h: (bottom - top) as u32,
    })
}

/// The pixels of `area` cut out of a frame. `area` must lie inside it.
pub fn crop(frame: &Frame, area: Rect) -> Vec<u8> {
    let row = frame.width as usize * 4;
    let (x, w) = (area.x as usize * 4, area.w as usize * 4);
    (area.y as usize..area.y as usize + area.h as usize)
        .flat_map(|y| &frame.bgra[y * row + x..y * row + x + w])
        .copied()
        .collect()
}

/// An image the webview can show directly (the content security policy allows `data:`).
pub fn data_url(image: &ScreenImage) -> String {
    format!("data:image/jpeg;base64,{}", image.base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: Selection = Selection { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    #[test]
    fn a_selection_becomes_padded_pixels_inside_the_image() {
        let sel = Selection { x: 0.25, y: 0.5, w: 0.25, h: 0.25 };
        assert_eq!(selection_rect(sel, 400, 200, 0), Some(Rect { x: 100, y: 100, w: 100, h: 50 }));
        assert_eq!(selection_rect(sel, 400, 200, 10), Some(Rect { x: 90, y: 90, w: 120, h: 70 }));
        assert_eq!(selection_rect(WHOLE, 400, 200, 10), Some(Rect { x: 0, y: 0, w: 400, h: 200 }));
    }

    #[test]
    fn a_selection_past_the_edges_is_kept_inside() {
        let past = Selection { x: -0.5, y: 0.9, w: 0.75, h: 0.5 };
        assert_eq!(selection_rect(past, 100, 100, 0), Some(Rect { x: 0, y: 90, w: 25, h: 10 }));
    }

    #[test]
    fn a_tiny_or_broken_selection_is_refused() {
        let dot = Selection { x: 0.5, y: 0.5, w: 0.001, h: 0.001 };
        assert_eq!(selection_rect(dot, 1000, 1000, 12), None);
        let nan = Selection { x: f64::NAN, ..WHOLE };
        assert_eq!(selection_rect(nan, 100, 100, 0), None);
    }

    #[test]
    fn crop_cuts_out_the_rows_and_columns_asked_for() {
        // A 3 x 2 frame whose pixels are numbered 0..6 in their first byte.
        let bgra: Vec<u8> = (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let frame = Frame { width: 3, height: 2, bgra };
        let part = crop(&frame, Rect { x: 1, y: 0, w: 2, h: 2 });
        let firsts: Vec<u8> = part.chunks(4).map(|p| p[0]).collect();
        assert_eq!(firsts, vec![1, 2, 4, 5]);
    }

    #[test]
    fn data_url_is_a_jpeg_data_url() {
        let img = ScreenImage { base64: "QUJD".into(), width: 1, height: 1 };
        assert_eq!(data_url(&img), "data:image/jpeg;base64,QUJD");
    }

    #[test]
    fn small_images_keep_their_size() {
        assert_eq!(fit_within(800, 600, 1568), (800, 600));
        assert_eq!(fit_within(1568, 10, 1568), (1568, 10));
    }

    #[test]
    fn large_images_shrink_to_the_longest_edge() {
        assert_eq!(fit_within(2560, 1600, 1568), (1568, 980));
        assert_eq!(fit_within(1600, 2560, 1568), (980, 1568));
        assert_eq!(fit_within(10_000, 1, 1568), (1568, 1));
    }

    #[test]
    fn encode_makes_a_scaled_jpeg() {
        let (w, h) = (40, 20);
        let bgra: Vec<u8> = (0..w * h).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let img = encode(&bgra, w, h, 16).unwrap();
        assert_eq!((img.width, img.height), (16, 8));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&img.base64)
            .unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "JPEG start-of-image marker");
    }

    /// Reads the real screen, so it only runs when asked: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn copies_real_screen_pixels() {
        let area = Rect { x: 0, y: 0, w: 320, h: 200 };
        let pixels = copy_pixels(area).unwrap();
        assert_eq!(pixels.len(), 320 * 200 * 4);
        assert!(pixels.chunks(4).any(|p| p[..3] != [0, 0, 0]), "the copy is all black");
        assert!(capture(area).unwrap().base64.len() > 100);
    }

    #[test]
    fn encode_rejects_a_buffer_of_the_wrong_length() {
        assert!(encode(&[0; 12], 2, 2, 100).is_err());
    }
}
