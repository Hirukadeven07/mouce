//! "Look at my screen": a read-only copy of one monitor's pixels, kept in memory only.
//!
//! The pixels are read with GDI (`BitBlt` from the screen into a memory bitmap), scaled
//! down and encoded as JPEG in memory. Nothing is written to disk, and the mouce and chat
//! windows ask Windows to leave them out of the copy, so the AI sees what is behind them.

use crate::mouce::position::Rect;
use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageBuffer, Rgb};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

/// Longest side of the image sent to the AI. Providers downscale anything larger anyway,
/// and a smaller image keeps requests quick.
pub const MAX_EDGE: u32 = 1568;
const JPEG_QUALITY: u8 = 80;

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
    let pixels = copy_pixels(area)?;
    encode(&pixels, area.w, area.h, MAX_EDGE)
}

#[cfg(test)]
mod tests {
    use super::*;

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
