//! On-device text recognition with **Windows.Media.Ocr** (the OCR built into Windows 10/11).
//!
//! Nothing here touches the network: the engine runs locally on the pixels of an [`Image`]
//! that [`crate::capture`] grabbed, and returns the recognised text with a bounding box per
//! line (in the image's own pixel coordinates).
//!
//! The recogniser uses the user's profile languages (`TryCreateFromUserProfileLanguages`).
//! Windows ships OCR as a per-language optional feature, so a fresh install can have none;
//! that case is reported as [`OcrError::NoLanguage`] with instructions the UI can show as is.
//!
//! The pure parts (box math, text tidy-up, assembling the result, error texts) build and test
//! anywhere; the WinRT calls are behind `#[cfg(all(windows, feature = "native"))]` and run on
//! a short-lived multithreaded-apartment worker thread, like [`crate::location`].

use serde::Serialize;

use crate::capture::Image;
use crate::SysError;

/// One recognised line of text and its box in image pixels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Everything the recogniser found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrOutput {
    /// All lines joined with `\n`.
    pub text: String,
    pub lines: Vec<OcrLine>,
    /// BCP-47 tag of the language that was used (`en-US`), if Windows said.
    pub language: Option<String>,
}

/// Why recognition did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrError {
    /// No OCR language pack is installed for any of the user's languages.
    NoLanguage,
    /// The image is wider or taller than the engine accepts.
    TooLarge { max: u32 },
    /// Not available in this build (no `native` feature) or on this OS.
    Unsupported,
    /// A Windows call failed.
    Failed(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::NoLanguage => f.write_str(
                "No OCR language is installed in Windows, so text can't be read from the screen. \
Open Settings > Time & language > Language & region, choose a language, open its options and \
install the \"Optical character recognition\" feature.",
            ),
            OcrError::TooLarge { max } => write!(
                f,
                "That area is larger than Windows can read ({max} pixels on a side). Select a smaller area."
            ),
            OcrError::Unsupported => f.write_str("Reading text from the screen needs Windows 10 or 11."),
            OcrError::Failed(m) => write!(f, "Windows could not read the text ({m})."),
        }
    }
}

impl std::error::Error for OcrError {}

impl From<OcrError> for SysError {
    fn from(e: OcrError) -> Self {
        match e {
            OcrError::Unsupported => SysError::Unsupported,
            other => SysError::Message(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// The box `(x, y, w, h)` that contains all `boxes`. Boxes with non-finite or negative
/// values are skipped. `None` when nothing usable is left.
pub fn union_box(boxes: &[(f32, f32, f32, f32)]) -> Option<(f32, f32, f32, f32)> {
    let mut acc: Option<(f32, f32, f32, f32)> = None; // left, top, right, bottom
    for &(x, y, w, h) in boxes {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w < 0.0 || h < 0.0 {
            continue;
        }
        let (r, b) = (x + w, y + h);
        acc = Some(match acc {
            None => (x, y, r, b),
            Some((l0, t0, r0, b0)) => (l0.min(x), t0.min(y), r0.max(r), b0.max(b)),
        });
    }
    acc.map(|(l, t, r, b)| (l, t, r - l, b - t))
}

/// Han, kana, CJK punctuation and fullwidth forms: scripts written without spaces between
/// words. (Hangul is left out: Korean does use spaces.)
fn is_spaceless_script(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK symbols and punctuation
        | 0x3040..=0x309F // Hiragana
        | 0x30A0..=0x30FF // Katakana
        | 0x3400..=0x4DBF // CJK extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xF900..=0xFAFF // compatibility ideographs
        | 0xFF00..=0xFFEF // halfwidth and fullwidth forms
        | 0x20000..=0x2A6DF // CJK extension B
    )
}

/// Clean one recognised line: trim it, and drop the single spaces Windows inserts between
/// characters of Chinese and Japanese text.
pub fn tidy_line(line: &str) -> String {
    let line = line.trim();
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 && i + 1 < chars.len() {
            let (p, n) = (chars[i - 1], chars[i + 1]);
            if is_spaceless_script(p) && is_spaceless_script(n) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Build the final result from raw lines: tidy each, drop the empty ones, join with `\n`.
pub fn assemble(lines: Vec<OcrLine>, language: Option<String>) -> OcrOutput {
    let lines: Vec<OcrLine> = lines
        .into_iter()
        .filter_map(|mut l| {
            l.text = tidy_line(&l.text);
            (!l.text.is_empty()).then_some(l)
        })
        .collect();
    let text = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    OcrOutput {
        text,
        lines,
        language,
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Recognise the text in `image` with the user's OCR language.
#[cfg(all(windows, feature = "native"))]
pub fn recognize(image: &Image) -> Result<OcrOutput, OcrError> {
    native::recognize(image)
}
#[cfg(not(all(windows, feature = "native")))]
pub fn recognize(_image: &Image) -> Result<OcrOutput, OcrError> {
    Err(OcrError::Unsupported)
}

/// The OCR language Windows would use (`en-US`), or why there is none. Creates an engine and
/// reads its language; recognises nothing.
#[cfg(all(windows, feature = "native"))]
pub fn available_language() -> Result<String, OcrError> {
    native::available_language()
}
#[cfg(not(all(windows, feature = "native")))]
pub fn available_language() -> Result<String, OcrError> {
    Err(OcrError::Unsupported)
}

#[cfg(all(windows, feature = "native"))]
mod native {
    use std::sync::Once;

    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::Com::CoIncrementMTAUsage;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    use super::{assemble, union_box, OcrError, OcrLine, OcrOutput};
    use crate::capture::Image;

    /// `E_POINTER`: what windows-rs reports when a factory method returns a null object, which
    /// is how `TryCreateFromUserProfileLanguages` says "no supported language".
    const E_POINTER: i32 = 0x8000_4003_u32 as i32;

    fn fail<E: std::fmt::Display>(e: E) -> OcrError {
        OcrError::Failed(e.to_string())
    }

    /// Keep the process-wide multithreaded apartment alive until the process ends.
    ///
    /// Without this the apartment exists only while some thread that joined it is alive. Our
    /// worker threads are short-lived, and windows-rs caches the OCR factory it activated on the
    /// first one; when that thread ended and the apartment went with it, the next call used a
    /// dangling factory and crashed with an access violation (seen in this module's test).
    fn keep_mta_alive() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            // The cookie is deliberately never released.
            // SAFETY: plain call, no arguments.
            let _ = unsafe { CoIncrementMTAUsage() };
        });
    }

    /// Run `f` on a fresh multithreaded-apartment thread and wait for it. Scoped, so `f` can
    /// borrow the (large) image instead of copying it.
    fn on_mta<T: Send>(f: impl FnOnce() -> Result<T, OcrError> + Send) -> Result<T, OcrError> {
        keep_mta_alive();
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .name("krovi-ocr".into())
                .spawn_scoped(s, move || {
                    // SAFETY: initialises COM/WinRT for this thread only; a repeat call is harmless.
                    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
                    f()
                })
                .map_err(fail)?
                .join()
                .map_err(|_| OcrError::Failed("the text reader stopped unexpectedly".into()))?
        })
    }

    fn engine() -> Result<OcrEngine, OcrError> {
        OcrEngine::TryCreateFromUserProfileLanguages().map_err(|e| {
            if e.code().0 == E_POINTER {
                OcrError::NoLanguage
            } else {
                fail(e.message())
            }
        })
    }

    fn language_tag(engine: &OcrEngine) -> Option<String> {
        engine
            .RecognizerLanguage()
            .ok()?
            .LanguageTag()
            .ok()
            .map(|t| t.to_string())
    }

    pub(super) fn available_language() -> Result<String, OcrError> {
        on_mta(|| {
            let e = engine()?;
            Ok(language_tag(&e).unwrap_or_else(|| "unknown".into()))
        })
    }

    pub(super) fn recognize(image: &Image) -> Result<OcrOutput, OcrError> {
        on_mta(|| {
            let engine = engine()?;
            let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
            if image.width > max || image.height > max {
                return Err(OcrError::TooLarge { max });
            }

            // BGRA bytes -> IBuffer -> SoftwareBitmap (Bgra8, premultiplied; every pixel is
            // already opaque so premultiplying changes nothing).
            let writer = DataWriter::new().map_err(fail)?;
            writer.WriteBytes(&image.bgra).map_err(fail)?;
            let buffer = writer.DetachBuffer().map_err(fail)?;
            let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
                &buffer,
                BitmapPixelFormat::Bgra8,
                image.width as i32,
                image.height as i32,
                BitmapAlphaMode::Premultiplied,
            )
            .map_err(fail)?;

            let result = engine
                .RecognizeAsync(&bitmap)
                .map_err(fail)?
                .get()
                .map_err(fail)?;

            let mut lines = Vec::new();
            for line in result.Lines().map_err(fail)? {
                let text = line.Text().map_err(fail)?.to_string();
                let mut boxes = Vec::new();
                if let Ok(words) = line.Words() {
                    for w in words {
                        if let Ok(r) = w.BoundingRect() {
                            boxes.push((r.X, r.Y, r.Width, r.Height));
                        }
                    }
                }
                let (x, y, w, h) = union_box(&boxes).unwrap_or((0.0, 0.0, 0.0, 0.0));
                lines.push(OcrLine { text, x, y, w, h });
            }
            Ok(assemble(lines, language_tag(&engine)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> OcrLine {
        OcrLine {
            text: text.into(),
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        }
    }

    #[test]
    fn union_covers_all_boxes() {
        let u = union_box(&[(10.0, 20.0, 30.0, 10.0), (50.0, 15.0, 20.0, 40.0)]).unwrap();
        assert_eq!(u, (10.0, 15.0, 60.0, 40.0));
    }

    #[test]
    fn union_skips_garbage_and_handles_empty() {
        assert_eq!(union_box(&[]), None);
        assert_eq!(union_box(&[(f32::NAN, 0.0, 1.0, 1.0)]), None);
        assert_eq!(union_box(&[(0.0, 0.0, -1.0, 1.0)]), None);
        let u = union_box(&[(f32::NAN, 0.0, 1.0, 1.0), (5.0, 5.0, 2.0, 2.0)]).unwrap();
        assert_eq!(u, (5.0, 5.0, 2.0, 2.0));
    }

    #[test]
    fn spaces_between_cjk_characters_are_removed() {
        assert_eq!(tidy_line("你 好 世 界"), "你好世界");
        assert_eq!(tidy_line("こ ん に ち は"), "こんにちは");
        // mixed text keeps the spaces next to Latin words
        assert_eq!(tidy_line("Krovi 是 一个 工具"), "Krovi 是一个工具");
        assert_eq!(tidy_line("  hello   world  "), "hello   world");
        // Korean uses spaces, so they stay
        assert_eq!(tidy_line("안녕 하세요"), "안녕 하세요");
        assert_eq!(tidy_line("A B"), "A B");
    }

    #[test]
    fn assemble_drops_blank_lines_and_joins_with_newlines() {
        let out = assemble(
            vec![
                line("First line"),
                line("   "),
                line(""),
                line("Second line "),
            ],
            Some("en-US".into()),
        );
        assert_eq!(out.text, "First line\nSecond line");
        assert_eq!(out.lines.len(), 2);
        assert_eq!(out.language.as_deref(), Some("en-US"));
        assert_eq!(assemble(vec![], None).text, "");
    }

    #[test]
    fn the_no_language_message_tells_the_user_what_to_install() {
        let m = OcrError::NoLanguage.to_string();
        assert!(m.contains("No OCR language"));
        assert!(m.contains("Optical character recognition"));
        assert!(OcrError::TooLarge { max: 10_000 }
            .to_string()
            .contains("10000"));
        assert!(OcrError::Failed("boom".into()).to_string().contains("boom"));
    }

    #[test]
    fn conversion_to_sys_error() {
        assert!(matches!(
            SysError::from(OcrError::Unsupported),
            SysError::Unsupported
        ));
        match SysError::from(OcrError::NoLanguage) {
            SysError::Message(m) => assert!(m.contains("OCR language")),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Exercises the WinRT plumbing (bytes -> IBuffer -> SoftwareBitmap -> OcrEngine) on an image
    /// built in memory. It never touches the screen. A PC with no OCR language pack (a CI runner,
    /// say) skips it, because then there is nothing to run.
    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn the_recogniser_runs_on_a_synthetic_image() {
        if available_language().is_err() {
            eprintln!("skipped: no OCR language installed on this machine");
            return;
        }
        // flat white, so there is nothing to read
        let img = Image::new(320, 120, vec![0xFF; 320 * 120 * 4]).unwrap();
        let out = recognize(&img).expect("recognition of a blank image should succeed");
        assert!(
            out.text.trim().is_empty(),
            "read {:?} from nothing",
            out.text
        );
        assert!(out.lines.is_empty());
        assert!(out.language.is_some());
    }

    /// Draw `text` (black on white, 40 px Arial) into an offscreen 32-bit bitmap with GDI and return
    /// it as an [`Image`]. Nothing here reads the screen: the bitmap is created in memory.
    #[cfg(all(windows, feature = "native"))]
    fn render_text(text: &str, w: i32, h: i32) -> Image {
        use std::ffi::c_void;
        use windows::core::w as wide;
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GdiFlush,
            PatBlt, SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO, BITMAPINFOHEADER,
            BI_RGB, DIB_RGB_COLORS, HGDIOBJ, TRANSPARENT, WHITENESS,
        };

        let mut bits: *mut c_void = std::ptr::null_mut();
        let mut bmi = BITMAPINFO {
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
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: every GDI object created here is released before returning, and `bits` points
        // at `w * h * 4` bytes owned by the DIB section until it is deleted.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .expect("offscreen bitmap");
            let old_bmp = SelectObject(dc, HGDIOBJ(bmp.0));
            let _ = PatBlt(dc, 0, 0, w, h, WHITENESS);
            let font = CreateFontW(40, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 4, 0, wide!("Arial"));
            let old_font = SelectObject(dc, HGDIOBJ(font.0));
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0));
            let _ = TextOutW(dc, 16, 16, &utf16);
            let _ = GdiFlush();

            let mut bgra =
                std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
            crate::capture::force_opaque(&mut bgra);

            SelectObject(dc, old_font);
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(dc);
            Image::new(w as u32, h as u32, bgra).expect("image")
        }
    }

    /// The whole local pipeline on text that was drawn in memory: pixels -> OCR -> text and boxes.
    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn it_reads_text_drawn_into_an_offscreen_bitmap() {
        let lang = match available_language() {
            Ok(l) => l,
            Err(_) => {
                eprintln!("skipped: no OCR language installed on this machine");
                return;
            }
        };
        if !lang.to_ascii_lowercase().starts_with("en") {
            eprintln!("skipped: the OCR language is {lang}, the test text is English");
            return;
        }
        let img = render_text("Hello Krovi 2026", 520, 90);
        assert!(!img.is_uniform(), "the text should have been drawn");
        let out = recognize(&img).expect("recognition should succeed");
        let text = out.text.to_lowercase();
        assert!(text.contains("hello"), "read {:?}", out.text);
        assert!(
            text.contains("krovi") || text.contains("2026"),
            "read {:?}",
            out.text
        );
        assert_eq!(out.lines.len(), 1, "{:?}", out.lines);
        let l = &out.lines[0];
        // the box sits where the text was drawn: left/top margin 16 px, about one 40 px line tall
        assert!(l.x >= 5.0 && l.x < 60.0, "x {}", l.x);
        assert!(l.y >= 5.0 && l.y < 50.0, "y {}", l.y);
        assert!(l.w > 150.0 && l.w < 520.0, "w {}", l.w);
        assert!(l.h > 15.0 && l.h < 80.0, "h {}", l.h);
        assert!(out.language.is_some());
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn without_native_ocr_is_unsupported() {
        let img = Image::new(2, 2, vec![0; 16]).unwrap();
        assert_eq!(recognize(&img), Err(OcrError::Unsupported));
        assert_eq!(available_language(), Err(OcrError::Unsupported));
    }
}
