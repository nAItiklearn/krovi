//! Region screen capture for **Circle to Ask** and the temporary screen-awareness session
//! (docs/DECISIONS.md D-021).
//!
//! Privacy shape, enforced by the API itself:
//!
//! * There is **no** "start capturing" / "subscribe to frames" call anywhere in this module.
//!   [`capture_region`] grabs one still image of one rectangle, once, when the caller asks.
//!   Nothing here runs at startup, on a timer or in the background.
//! * The result is an in-memory [`Image`]. Nothing is written to disk, nothing is logged
//!   (`Debug` prints the size only, never pixels), and the pixel buffer is overwritten with
//!   zeros when the image is dropped (best effort).
//! * The rectangle is clipped to the virtual screen and capped at [`MAX_PIXELS`], so a bad
//!   request cannot allocate gigabytes.
//!
//! All the math (clipping, rounding from the webview's floats, BGRA to RGBA, downscaling for
//! upload, PNG encoding) is pure and unit-tested on any host. The GDI calls (`GetDC`,
//! `BitBlt`, `GetDIBits`) sit behind `#[cfg(all(windows, feature = "native"))]`; without the
//! `native` feature the entry points exist and report [`SysError::Unsupported`] / `None`.
//! Tests never capture the real screen.

use serde::{Deserialize, Serialize};

use crate::imaging;
use crate::{Result, SysError};

/// Largest region [`capture_region`] will grab, in pixels (about 80 MB of BGRA). A single
/// 4K monitor is 8.3 million pixels, two 4K monitors side by side 16.6 million.
pub const MAX_PIXELS: u64 = 20_000_000;
/// Smallest edge of a region worth capturing. A click or a twitch is not a selection.
pub const MIN_EDGE: i32 = 4;
/// Longest edge of the PNG sent to a vision provider. 1568 px is the size Anthropic documents
/// as the point past which images are only downscaled server-side, and it keeps screenshots
/// of text legible while staying a small upload.
pub const UPLOAD_MAX_EDGE: u32 = 1568;

/// An axis-aligned rectangle in **physical** pixels of the virtual screen (the union of all
/// monitors; its origin can be negative when a monitor sits left of or above the primary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Exclusive right edge (widened so it can never overflow).
    pub fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.w)
    }

    /// Exclusive bottom edge (widened so it can never overflow).
    pub fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.h)
    }

    pub fn area(&self) -> u64 {
        if self.w <= 0 || self.h <= 0 {
            0
        } else {
            self.w as u64 * self.h as u64
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// The overlap of two rectangles, or `None` when they do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        if self.is_empty() || other.is_empty() {
            return None;
        }
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            return None;
        }
        Some(Rect {
            x: left as i32,
            y: top as i32,
            w: (right - left) as i32,
            h: (bottom - top) as i32,
        })
    }

    /// The same rectangle moved by `(dx, dy)`. `None` if that would overflow `i32`.
    pub fn translated(&self, dx: i32, dy: i32) -> Option<Rect> {
        Some(Rect {
            x: self.x.checked_add(dx)?,
            y: self.y.checked_add(dy)?,
            w: self.w,
            h: self.h,
        })
    }

    /// Whole-pixel rectangle covering the float rectangle `(x, y, w, h)`: the origin is
    /// rounded down and the far edge up, so a partly covered pixel is included. `None` for
    /// NaN/infinite values, a non-positive size, or coordinates no screen could have.
    pub fn from_f64(x: f64, y: f64, w: f64, h: f64) -> Option<Rect> {
        const LIMIT: f64 = 1.0e7;
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
            return None;
        }
        if x.abs() > LIMIT || y.abs() > LIMIT || w > LIMIT || h > LIMIT {
            return None;
        }
        let left = x.floor();
        let top = y.floor();
        let right = (x + w).ceil();
        let bottom = (y + h).ceil();
        Some(Rect {
            x: left as i32,
            y: top as i32,
            w: (right - left) as i32,
            h: (bottom - top) as i32,
        })
    }
}

/// Why a requested region was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipError {
    /// Zero or negative width/height.
    Empty,
    /// The region does not touch any monitor.
    OffScreen,
    /// Smaller than [`MIN_EDGE`] in some direction after clipping.
    TooSmall,
    /// More than [`MAX_PIXELS`].
    TooLarge,
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ClipError::Empty => "The selected area is empty.",
            ClipError::OffScreen => "The selected area is outside the screen.",
            ClipError::TooSmall => "The selected area is too small. Drag around something larger.",
            ClipError::TooLarge => "The selected area is too large. Select a smaller area.",
        })
    }
}

impl std::error::Error for ClipError {}

impl From<ClipError> for SysError {
    fn from(e: ClipError) -> Self {
        SysError::Message(e.to_string())
    }
}

/// Clip `rect` to `screen` (the virtual screen) and validate the result.
pub fn clip_to_screen(rect: Rect, screen: Rect) -> std::result::Result<Rect, ClipError> {
    if rect.is_empty() {
        return Err(ClipError::Empty);
    }
    let clipped = rect.intersect(&screen).ok_or(ClipError::OffScreen)?;
    if clipped.w < MIN_EDGE || clipped.h < MIN_EDGE {
        return Err(ClipError::TooSmall);
    }
    if clipped.area() > MAX_PIXELS {
        return Err(ClipError::TooLarge);
    }
    Ok(clipped)
}

/// A captured still image: tightly packed 32-bit **BGRA**, top row first, alpha forced opaque.
///
/// Lives in memory only. `Debug` shows the size, never pixels. The buffer is zeroed on drop.
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        wipe(&mut self.bgra);
    }
}

/// Overwrite `buf` with zeros in a way the optimiser may not remove as a dead store.
fn wipe(buf: &mut [u8]) {
    buf.fill(0);
    std::hint::black_box(&*buf);
}

/// Set every alpha byte of a BGRA buffer to 255. GDI leaves it undefined (usually 0), and a
/// premultiplied bitmap with alpha 0 would read as fully transparent.
pub fn force_opaque(bgra: &mut [u8]) {
    for px in bgra.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

/// BGRA to RGBA (swap the first and third byte of every pixel).
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut out = bgra.to_vec();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

impl Image {
    /// Wrap a BGRA buffer. `None` when the size is zero or the buffer length is wrong.
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Option<Image> {
        let want = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if width == 0 || height == 0 || bgra.len() != want {
            return None;
        }
        Some(Image {
            width,
            height,
            bgra,
        })
    }

    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// True when every pixel has the same colour as the first. A real capture of a window
    /// is almost never one flat colour; an all-black result usually means protected content
    /// (DRM video, some games) that Windows will not hand over.
    pub fn is_uniform(&self) -> bool {
        match self.bgra.get(..4) {
            Some(first) => self.bgra.chunks_exact(4).all(|p| p == first),
            None => true,
        }
    }

    /// PNG of the image, scaled down (never up) so its longer side is at most `max_edge`
    /// pixels; `0` means full size. `None` if encoding fails.
    pub fn to_png(&self, max_edge: u32) -> Option<Vec<u8>> {
        let rgba = bgra_to_rgba(&self.bgra);
        if max_edge > 0 && self.width.max(self.height) > max_edge {
            let (w, h) = imaging::fit_within(self.width, self.height, max_edge);
            let small = imaging::downscale_rgba(self.width, self.height, &rgba, w, h);
            imaging::encode_png(w, h, &small)
        } else {
            imaging::encode_png(self.width, self.height, &rgba)
        }
    }

    /// [`Image::to_png`], base64-encoded for a JSON request body.
    pub fn to_png_base64(&self, max_edge: u32) -> Option<String> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        self.to_png(max_edge).map(|png| STANDARD.encode(png))
    }
}

// ---------------------------------------------------------------------------
// Screen geometry + capture (native)
// ---------------------------------------------------------------------------

/// Bounding rectangle of all monitors in physical pixels, if it can be read.
#[cfg(all(windows, feature = "native"))]
pub fn virtual_screen() -> Option<Rect> {
    native::virtual_screen()
}
/// Bounding rectangle of all monitors; always `None` without the `native` feature.
#[cfg(not(all(windows, feature = "native")))]
pub fn virtual_screen() -> Option<Rect> {
    None
}

/// The primary monitor's rectangle (where Krovi's pill lives).
#[cfg(all(windows, feature = "native"))]
pub fn primary_monitor() -> Option<Rect> {
    native::primary_monitor()
}
#[cfg(not(all(windows, feature = "native")))]
pub fn primary_monitor() -> Option<Rect> {
    None
}

/// The monitor that contains (or is nearest to) the physical point `(x, y)`.
#[cfg(all(windows, feature = "native"))]
pub fn monitor_at(x: i32, y: i32) -> Option<Rect> {
    native::monitor_at(x, y)
}
#[cfg(not(all(windows, feature = "native")))]
pub fn monitor_at(_x: i32, _y: i32) -> Option<Rect> {
    None
}

/// The monitor under the mouse cursor.
#[cfg(all(windows, feature = "native"))]
pub fn monitor_at_cursor() -> Option<Rect> {
    let (x, y) = native::cursor_pos()?;
    native::monitor_at(x, y)
}
#[cfg(not(all(windows, feature = "native")))]
pub fn monitor_at_cursor() -> Option<Rect> {
    None
}

/// Capture one still image of `rect` (physical virtual-screen pixels, clipped to the
/// screen) with GDI `BitBlt(SRCCOPY | CAPTUREBLT)`. One call, one frame; no stream.
#[cfg(all(windows, feature = "native"))]
pub fn capture_region(rect: Rect) -> Result<Image> {
    let screen = native::virtual_screen()
        .ok_or_else(|| SysError::Message("Windows did not report the screen size.".into()))?;
    let clipped = clip_to_screen(rect, screen)?;
    native::grab(clipped)
}
#[cfg(not(all(windows, feature = "native")))]
pub fn capture_region(_rect: Rect) -> Result<Image> {
    Err(SysError::Unsupported)
}

/// Hide (`exclude = true`) or show (`false`) one of our own windows to screen capture, so a
/// frame taken while it is on screen does not contain it (Windows 10 2004 and later:
/// `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`). The window itself stays visible to
/// the user. Returns whether Windows accepted the change.
#[cfg(all(windows, feature = "native"))]
pub fn exclude_window_from_capture(hwnd: isize, exclude: bool) -> bool {
    native::set_capture_exclusion(hwnd, exclude)
}
#[cfg(not(all(windows, feature = "native")))]
pub fn exclude_window_from_capture(_hwnd: isize, _exclude: bool) -> bool {
    false
}

#[cfg(all(windows, feature = "native"))]
mod native {
    use std::ffi::c_void;
    use std::mem::size_of;

    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, HGDIOBJ, HMONITOR, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SetWindowDisplayAffinity, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };

    use super::{force_opaque, Image, Rect};
    use crate::{Result, SysError};

    fn err(what: &str) -> SysError {
        SysError::Message(format!("Screen capture failed ({what})."))
    }

    pub(super) fn virtual_screen() -> Option<Rect> {
        // SAFETY: plain metric queries with constant indices.
        let (x, y, w, h) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        (w > 0 && h > 0).then_some(Rect { x, y, w, h })
    }

    pub(super) fn cursor_pos() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: `p` outlives the call.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x, p.y))
    }

    fn monitor_rect(m: HMONITOR) -> Option<Rect> {
        if m.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `mi` is a correctly sized, initialised MONITORINFO.
        if !unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool() {
            return None;
        }
        let r = mi.rcMonitor;
        Some(Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        })
    }

    pub(super) fn monitor_at(x: i32, y: i32) -> Option<Rect> {
        // SAFETY: by-value point, nearest-monitor fallback never returns an invalid handle
        // on a machine with a display.
        monitor_rect(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) })
    }

    pub(super) fn primary_monitor() -> Option<Rect> {
        // SAFETY: as above, with the primary-monitor fallback.
        monitor_rect(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) })
    }

    pub(super) fn set_capture_exclusion(hwnd: isize, exclude: bool) -> bool {
        if hwnd == 0 {
            return false;
        }
        let affinity = if exclude {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        // SAFETY: the handle is one of this process's own windows (the caller passes it);
        // a stale handle just makes the call fail.
        unsafe { SetWindowDisplayAffinity(HWND(hwnd as *mut c_void), affinity) }.is_ok()
    }

    /// One `BitBlt` of the already validated `r` from the screen into a top-down 32-bit DIB.
    pub(super) fn grab(r: Rect) -> Result<Image> {
        // SAFETY: the screen DC is released before returning.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(err("screen DC"));
            }
            let image = grab_from(screen, r);
            ReleaseDC(None, screen);
            image
        }
    }

    /// Copy `r` out of the device context `src` (the screen DC in the app; an offscreen one in the
    /// tests) into a top-down 32-bit BGRA [`Image`] with opaque alpha.
    ///
    /// # Safety
    /// `src` must be a valid device context for the duration of the call.
    pub(super) unsafe fn grab_from(src: HDC, r: Rect) -> Result<Image> {
        let (w, h) = (r.w, r.h);
        let mut bgra = vec![0u8; w as usize * h as usize * 4];

        // SAFETY: every handle created below is released on every path before returning;
        // `bgra` is exactly `w * h * 4` bytes, which is what GetDIBits writes for a 32-bit
        // top-down DIB of that size.
        unsafe {
            let mem = CreateCompatibleDC(src);
            if mem.is_invalid() {
                return Err(err("memory DC"));
            }
            let bmp = CreateCompatibleBitmap(src, w, h);
            if bmp.is_invalid() {
                let _ = DeleteDC(mem);
                return Err(err("bitmap"));
            }

            let old = SelectObject(mem, HGDIOBJ(bmp.0));
            let blt = BitBlt(mem, 0, 0, w, h, src, r.x, r.y, SRCCOPY | CAPTUREBLT);
            // GetDIBits needs the bitmap deselected from every DC.
            SelectObject(mem, old);

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // negative: top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let lines = GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(bgra.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);

            if blt.is_err() {
                return Err(err("copy"));
            }
            if lines != h {
                return Err(err("read"));
            }
        }

        force_opaque(&mut bgra);
        Image::new(w as u32, h as u32, bgra).ok_or_else(|| err("size"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 1920, 1080);

    #[test]
    fn a_region_inside_the_screen_is_unchanged() {
        let r = Rect::new(100, 200, 300, 150);
        assert_eq!(clip_to_screen(r, SCREEN), Ok(r));
    }

    #[test]
    fn a_region_hanging_off_the_edge_is_clipped() {
        let r = Rect::new(1800, 1000, 400, 400);
        assert_eq!(
            clip_to_screen(r, SCREEN),
            Ok(Rect::new(1800, 1000, 120, 80))
        );
        let r = Rect::new(-50, -30, 200, 100);
        assert_eq!(clip_to_screen(r, SCREEN), Ok(Rect::new(0, 0, 150, 70)));
    }

    #[test]
    fn a_secondary_monitor_left_of_the_primary_has_negative_coordinates() {
        let vs = Rect::new(-1920, 0, 3840, 1080);
        let on_left = Rect::new(-1500, 100, 400, 300);
        assert_eq!(clip_to_screen(on_left, vs), Ok(on_left));
        let straddling = Rect::new(-100, 0, 300, 100);
        assert_eq!(clip_to_screen(straddling, vs), Ok(straddling));
    }

    #[test]
    fn empty_offscreen_tiny_and_huge_regions_are_refused() {
        assert_eq!(
            clip_to_screen(Rect::new(10, 10, 0, 50), SCREEN),
            Err(ClipError::Empty)
        );
        assert_eq!(
            clip_to_screen(Rect::new(10, 10, -5, 50), SCREEN),
            Err(ClipError::Empty)
        );
        assert_eq!(
            clip_to_screen(Rect::new(5000, 5000, 100, 100), SCREEN),
            Err(ClipError::OffScreen)
        );
        // touching the edge but not overlapping
        assert_eq!(
            clip_to_screen(Rect::new(1920, 0, 100, 100), SCREEN),
            Err(ClipError::OffScreen)
        );
        assert_eq!(
            clip_to_screen(Rect::new(10, 10, 3, 200), SCREEN),
            Err(ClipError::TooSmall)
        );
        // a 6000x4000 virtual screen: the whole thing is 24 Mpx, over the cap
        let big = Rect::new(0, 0, 6000, 4000);
        assert_eq!(clip_to_screen(big, big), Err(ClipError::TooLarge));
        // the same region clipped to a normal screen is fine
        assert!(clip_to_screen(big, SCREEN).is_ok());
    }

    #[test]
    fn extreme_values_do_not_overflow() {
        let r = Rect::new(i32::MAX - 5, i32::MAX - 5, i32::MAX, i32::MAX);
        assert_eq!(clip_to_screen(r, SCREEN), Err(ClipError::OffScreen));
        // right/bottom edges are -1 (widened math), so this ends just short of the screen
        let r = Rect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(clip_to_screen(r, SCREEN), Err(ClipError::OffScreen));
        // a huge rectangle that does cover the screen clips down to exactly the screen
        let r = Rect::new(-1_000_000, -1_000_000, 2_000_000, 2_000_000);
        assert_eq!(clip_to_screen(r, SCREEN), Ok(SCREEN));
        assert_eq!(Rect::new(i32::MAX, 0, 10, 10).translated(1, 0), None);
        assert_eq!(
            Rect::new(10, 10, 5, 5).translated(-10, 3),
            Some(Rect::new(0, 13, 5, 5))
        );
    }

    #[test]
    fn float_rectangles_round_outward_and_garbage_is_rejected() {
        assert_eq!(
            Rect::from_f64(10.4, 20.6, 100.2, 50.0),
            Some(Rect::new(10, 20, 101, 51))
        );
        assert_eq!(
            Rect::from_f64(-0.5, 0.0, 10.0, 10.0),
            Some(Rect::new(-1, 0, 11, 10))
        );
        assert_eq!(Rect::from_f64(f64::NAN, 0.0, 10.0, 10.0), None);
        assert_eq!(Rect::from_f64(0.0, 0.0, f64::INFINITY, 10.0), None);
        assert_eq!(Rect::from_f64(0.0, 0.0, 0.0, 10.0), None);
        assert_eq!(Rect::from_f64(0.0, 0.0, 10.0, -1.0), None);
        assert_eq!(Rect::from_f64(1.0e12, 0.0, 10.0, 10.0), None);
    }

    #[test]
    fn intersection_basics() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(
            a.intersect(&Rect::new(50, 50, 100, 100)),
            Some(Rect::new(50, 50, 50, 50))
        );
        assert_eq!(a.intersect(&Rect::new(100, 0, 10, 10)), None);
        assert_eq!(a.intersect(&Rect::new(0, 0, 0, 10)), None);
        assert_eq!(a.area(), 10_000);
        assert_eq!(Rect::new(0, 0, -3, 5).area(), 0);
    }

    fn solid(w: u32, h: u32, b: u8, g: u8, r: u8) -> Image {
        let mut v = Vec::new();
        for _ in 0..(w * h) {
            v.extend_from_slice(&[b, g, r, 255]);
        }
        Image::new(w, h, v).unwrap()
    }

    #[test]
    fn force_opaque_touches_only_alpha() {
        let mut px = vec![1, 2, 3, 0, 4, 5, 6, 77];
        force_opaque(&mut px);
        assert_eq!(px, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue() {
        assert_eq!(
            bgra_to_rgba(&[10, 20, 30, 255, 1, 2, 3, 4]),
            vec![30, 20, 10, 255, 3, 2, 1, 4]
        );
    }

    #[test]
    fn image_new_validates_the_buffer() {
        assert!(Image::new(0, 4, vec![]).is_none());
        assert!(Image::new(2, 2, vec![0; 15]).is_none());
        assert!(Image::new(2, 2, vec![0; 16]).is_some());
        assert!(Image::new(u32::MAX, u32::MAX, vec![0; 16]).is_none());
    }

    #[test]
    fn png_has_the_right_colours_and_size() {
        // a BGRA pixel (b=10,g=20,r=30) must come out as RGBA (30,20,10)
        let img = solid(4, 3, 10, 20, 30);
        let png = img.to_png(0).unwrap();
        assert_eq!(imaging::sniff_mime(&png), Some("image/png"));
        let (w, h, rgba) = imaging::decode_png(&png).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(&rgba[..4], &[30, 20, 10, 255]);
    }

    #[test]
    fn upload_png_is_scaled_down_but_never_up() {
        let big = solid(3000, 1500, 1, 2, 3);
        let png = big.to_png(UPLOAD_MAX_EDGE).unwrap();
        let (w, h, _) = imaging::decode_png(&png).unwrap();
        assert_eq!((w, h), (1568, 784));
        let small = solid(200, 100, 1, 2, 3);
        let png = small.to_png(UPLOAD_MAX_EDGE).unwrap();
        let (w, h, _) = imaging::decode_png(&png).unwrap();
        assert_eq!((w, h), (200, 100));
    }

    #[test]
    fn base64_png_decodes_back_to_a_png() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let b64 = solid(8, 8, 9, 9, 9).to_png_base64(0).unwrap();
        let bytes = STANDARD.decode(b64).unwrap();
        assert_eq!(imaging::sniff_mime(&bytes), Some("image/png"));
    }

    #[test]
    fn uniform_detection() {
        assert!(solid(16, 16, 0, 0, 0).is_uniform());
        let mut img = solid(16, 16, 0, 0, 0);
        img.bgra[4 * 20] = 9;
        assert!(!img.is_uniform());
    }

    #[test]
    fn debug_never_prints_pixels() {
        let img = solid(2, 2, 123, 45, 67);
        let s = format!("{img:?}");
        assert_eq!(s, "Image(2x2)");
    }

    #[test]
    fn wipe_zeroes_the_buffer() {
        let mut v = vec![0xAB; 64];
        wipe(&mut v);
        assert!(v.iter().all(|b| *b == 0));
    }

    /// Screen geometry only (sizes and positions of monitors, never their contents).
    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn monitor_geometry_is_consistent() {
        let Some(vs) = virtual_screen() else {
            eprintln!("skipped: Windows reports no screen on this machine");
            return;
        };
        assert!(vs.w > 0 && vs.h > 0, "{vs:?}");
        let primary = primary_monitor().expect("a primary monitor");
        assert!(primary.w > 0 && primary.h > 0);
        assert_eq!(
            primary.intersect(&vs),
            Some(primary),
            "primary {primary:?} not inside {vs:?}"
        );
        // the monitor nearest to its own centre is itself
        let centre = monitor_at(primary.x + primary.w / 2, primary.y + primary.h / 2);
        assert_eq!(centre, Some(primary));
        // the monitor under the cursor, if known, is inside the virtual screen too
        if let Some(m) = monitor_at_cursor() {
            assert_eq!(m.intersect(&vs), Some(m));
        }
        // a point far outside every monitor still resolves to the nearest one
        assert!(monitor_at(i32::MAX / 2, i32::MAX / 2).is_some());
        // a window handle of 0 is refused, never "excluded"
        assert!(!exclude_window_from_capture(0, true));
    }

    /// The real GDI copy (`BitBlt` + `GetDIBits`) out of an offscreen memory device context, so the
    /// row order, byte order, sub-rectangle offset and forced-opaque alpha are checked without ever
    /// reading the screen.
    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn the_gdi_copy_is_top_down_bgra_and_opaque() {
        use std::ffi::c_void;
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
        };

        let (w, h) = (6i32, 4i32);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: all handles are created and released inside this block; `bits` stays valid
        // while the DIB section exists.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .expect("offscreen bitmap");
            let old = SelectObject(dc, HGDIOBJ(bmp.0));

            // source: rows 0-1 pure red, rows 2-3 pure blue; alpha left at 0 on purpose
            let px = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let o = ((y * w + x) * 4) as usize;
                    let (b, g, r) = if y < 2 { (0, 0, 255) } else { (255, 0, 0) };
                    px[o..o + 4].copy_from_slice(&[b, g, r, 0]);
                }
            }

            // columns 1..5, rows 1..4
            let img = native::grab_from(dc, Rect::new(1, 1, 4, 3)).expect("copy");
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(dc);

            assert_eq!((img.width, img.height), (4, 3));
            let row = |y: usize| &img.bgra[y * 16..y * 16 + 4];
            assert_eq!(
                row(0),
                &[0, 0, 255, 255],
                "source row 1 is red, alpha forced opaque"
            );
            assert_eq!(row(1), &[255, 0, 0, 255], "source row 2 is blue");
            assert_eq!(row(2), &[255, 0, 0, 255]);
            // and it round-trips to a PNG with red as RGB (255, 0, 0)
            let (_, _, rgba) = imaging::decode_png(&img.to_png(0).unwrap()).unwrap();
            assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        }
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn without_native_nothing_can_capture() {
        assert!(matches!(
            capture_region(Rect::new(0, 0, 100, 100)),
            Err(SysError::Unsupported)
        ));
        assert_eq!(virtual_screen(), None);
        assert_eq!(monitor_at_cursor(), None);
        assert!(!exclude_window_from_capture(1, true));
    }
}
