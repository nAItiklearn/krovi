//! Manual probe for one-shot region capture and on-device OCR, against the real desktop.
//! It is NOT a test and is never run by `cargo test`: it reads a piece of YOUR screen, so it only
//! runs when you start it yourself.
//!
//! ```text
//! cargo run -p krovi-sys --features native --example capture_probe
//! cargo run -p krovi-sys --features native --example capture_probe -- 100 100 800 300
//! ```
//!
//! With no arguments it captures the top-left 800x300 pixels of the monitor under the cursor; four
//! numbers `x y width height` (physical pixels of the virtual screen) pick another rectangle.
//! It prints geometry, timings, whether the picture is one flat colour, the OCR language, the number
//! of lines and the first 400 characters of text. Nothing is written to disk.
//!
//! Things to look for on a real PC: the rectangle is the one you expected on a 125% / 150% display and
//! on a monitor left of the primary (negative x); the text matches what is on screen; with no OCR
//! language installed the message tells you what to install.

use std::time::Instant;

use krovi_sys::capture::{self, Rect};
use krovi_sys::ocr;

fn main() {
    let args: Vec<i32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();

    println!("virtual screen : {:?}", capture::virtual_screen());
    println!("primary monitor: {:?}", capture::primary_monitor());
    let monitor = capture::monitor_at_cursor().or_else(capture::primary_monitor);
    println!("under cursor   : {monitor:?}");

    let region = match (args.as_slice(), monitor) {
        ([x, y, w, h], _) => Rect::new(*x, *y, *w, *h),
        (_, Some(m)) => Rect::new(m.x, m.y, 800.min(m.w), 300.min(m.h)),
        _ => {
            eprintln!("no monitor found");
            return;
        }
    };
    println!("capturing      : {region:?}");

    let t = Instant::now();
    let image = match capture::capture_region(region) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("capture failed: {e}");
            return;
        }
    };
    println!(
        "captured       : {}x{} in {} ms, flat colour: {}",
        image.width,
        image.height,
        t.elapsed().as_millis(),
        image.is_uniform()
    );
    println!(
        "png for upload : {:?} bytes (longest side capped at {})",
        image.to_png(capture::UPLOAD_MAX_EDGE).map(|p| p.len()),
        capture::UPLOAD_MAX_EDGE
    );

    println!("ocr language   : {:?}", ocr::available_language());
    let t = Instant::now();
    match ocr::recognize(&image) {
        Ok(o) => {
            println!(
                "ocr            : {} lines, {} chars in {} ms, language {:?}",
                o.lines.len(),
                o.text.chars().count(),
                t.elapsed().as_millis(),
                o.language
            );
            let head: String = o.text.chars().take(400).collect();
            println!("--- text (first 400 chars) ---\n{head}\n------------------------------");
            for l in o.lines.iter().take(5) {
                println!("  line at ({:.0},{:.0}) {}x{}: {}", l.x, l.y, l.w as i32, l.h as i32, l.text);
            }
        }
        Err(e) => println!("ocr            : {e}"),
    }
}
