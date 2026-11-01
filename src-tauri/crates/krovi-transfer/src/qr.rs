//! QR codes as inline SVG, rendered in pure Rust. Always black modules on a white card with a
//! 4-module quiet zone, regardless of the app theme: phone scanners need that contrast.
//!
//! The renderer merges horizontal runs of dark modules into one rectangle each, which cuts a
//! typical code from ~10 KB to ~4 KB and avoids the hairline seams that per-module squares show
//! when the SVG is scaled to a non-integer size.

use qrcode::{Color, EcLevel, QrCode};

const QUIET: usize = 4;

/// SVG markup (no XML prolog, no fixed size: CSS decides) for `text`, or `None` if it cannot
/// be encoded.
pub fn svg_for(text: &str) -> Option<String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let w = code.width();
    let colors = code.to_colors();
    let dim = w + 2 * QUIET;
    let mut d = String::new();
    for y in 0..w {
        let mut x = 0;
        while x < w {
            if colors[y * w + x] == Color::Dark {
                let start = x;
                while x < w && colors[y * w + x] == Color::Dark {
                    x += 1;
                }
                let n = x - start;
                d.push_str(&format!("M{} {}h{n}v1h-{n}z", start + QUIET, y + QUIET));
            } else {
                x += 1;
            }
        }
    }
    Some(format!(
        "<svg role=\"img\" aria-label=\"QR code\" viewBox=\"0 0 {dim} {dim}\" shape-rendering=\"crispEdges\">\
<rect width=\"{dim}\" height=\"{dim}\" fill=\"#ffffff\"/><path d=\"{d}\" fill=\"#000000\"/></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rasterise our path data back into a grid (dark = true), the way a browser would.
    fn raster(svg: &str, dim: usize) -> Vec<Vec<bool>> {
        let d = svg
            .split("<path d=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let mut grid = vec![vec![false; dim]; dim];
        for cmd in d.split('z').filter(|s| !s.is_empty()) {
            // M{x} {y}h{n}v1h-{n}
            let rest = cmd.strip_prefix('M').unwrap();
            let (xy, tail) = rest.split_once('h').unwrap();
            let (x, y) = xy.split_once(' ').unwrap();
            let n: usize = tail.split('v').next().unwrap().parse().unwrap();
            let (x, y): (usize, usize) = (x.parse().unwrap(), y.parse().unwrap());
            for i in 0..n {
                grid[y][x + i] = true;
            }
        }
        grid
    }

    #[test]
    fn path_reproduces_the_exact_qr_matrix() {
        let text = "http://192.168.1.42:51234/s/AbCdEfGhIjKlMnOpQrStUvWxYz0";
        let svg = svg_for(text).unwrap();
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).unwrap();
        let w = code.width();
        let dim = w + 2 * QUIET;
        assert!(svg.contains(&format!("viewBox=\"0 0 {dim} {dim}\"")));
        let grid = raster(&svg, dim);
        let colors = code.to_colors();
        for y in 0..dim {
            for x in 0..dim {
                let expect = x >= QUIET
                    && y >= QUIET
                    && x < QUIET + w
                    && y < QUIET + w
                    && colors[(y - QUIET) * w + (x - QUIET)] == Color::Dark;
                assert_eq!(grid[y][x], expect, "module ({x},{y})");
            }
        }
    }

    #[test]
    fn markup_is_inline_safe_and_unsized() {
        let s = svg_for("http://10.0.0.2:50000/r/token").unwrap();
        assert!(s.starts_with("<svg"));
        assert!(!s.contains("<?xml"));
        let root = &s[..s.find('>').unwrap()];
        assert!(
            !root.contains(" width=\"") && !root.contains(" height=\""),
            "{root}"
        );
        assert!(root.contains("viewBox") && root.contains("aria-label=\"QR code\""));
        assert!(s.contains("#ffffff") && s.contains("#000000"));
        // nothing that could run or fetch
        assert!(!s.contains("<script") && !s.contains("href") && !s.contains("onload"));
    }

    #[test]
    fn typical_code_is_small_and_worst_case_url_fits() {
        let url = format!("http://192.168.100.100:65535/s/{}", "A".repeat(27));
        assert!(url.len() <= 62, "{}", url.len()); // QR version 4-M byte capacity
        let s = svg_for(&url).unwrap();
        // merged runs: ~4 KB where one square per module was ~10 KB
        assert!(s.len() < 6000, "svg is {} bytes", s.len());
    }
}
