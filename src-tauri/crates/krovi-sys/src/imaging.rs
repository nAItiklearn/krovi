//! Pure image helpers shared by the clipboard reader and the media-artwork reader:
//! PNG encode/decode, aspect-preserving box downscale, size-capped thumbnails,
//! data URLs, and magic-byte mime sniffing. No OS bindings, so everything here is
//! unit-tested on any host.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Longest edge of a clipboard-history thumbnail.
pub const THUMB_MAX_EDGE: u32 = 256;
/// Largest accepted thumbnail PNG (≈200 KB, see docs/CONTRACTS.md §2).
pub const THUMB_CAP_BYTES: usize = 200 * 1024;

/// A downscaled preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
}

/// Encode tightly packed RGBA8 as a PNG (fast compression — these are previews and
/// copy-back sidecars, not archival).
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header().ok()?.write_image_data(rgba).ok()?;
    }
    Some(out)
}

/// Decode any 8/16-bit PNG into tightly packed RGBA8.
pub fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

/// Size that fits `max_edge` on the longer side, keeping aspect, never upscaling,
/// never below 1px.
pub fn fit_within(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest == 0 || longest <= max_edge {
        return (width.max(1), height.max(1));
    }
    let scale = f64::from(max_edge) / f64::from(longest);
    (
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    )
}

/// Area-average downscale of RGBA8 (alpha-weighted, so transparent pixels do not
/// darken edges). `new_w/new_h` must not exceed the source size.
pub fn downscale_rgba(w: u32, h: u32, rgba: &[u8], new_w: u32, new_h: u32) -> Vec<u8> {
    let (w, h, nw, nh) = (w as usize, h as usize, new_w as usize, new_h as usize);
    let mut out = vec![0u8; nw * nh * 4];
    for dy in 0..nh {
        let y0 = dy * h / nh;
        let y1 = (((dy + 1) * h) / nh).max(y0 + 1).min(h);
        for dx in 0..nw {
            let x0 = dx * w / nw;
            let x1 = (((dx + 1) * w) / nw).max(x0 + 1).min(w);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                let row = y * w * 4;
                for x in x0..x1 {
                    let p = row + x * 4;
                    let alpha = u64::from(rgba[p + 3]);
                    r += u64::from(rgba[p]) * alpha;
                    g += u64::from(rgba[p + 1]) * alpha;
                    b += u64::from(rgba[p + 2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            let o = (dy * nw + dx) * 4;
            if a > 0 {
                out[o] = (r / a) as u8;
                out[o + 1] = (g / a) as u8;
                out[o + 2] = (b / a) as u8;
            }
            out[o + 3] = (a / n.max(1)) as u8;
        }
    }
    out
}

/// Build a PNG thumbnail no larger than `cap_bytes`: start at `max_edge` and
/// shrink by 25% steps (down to 32px) until it fits. `None` if it never does.
pub fn make_thumbnail(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_edge: u32,
    cap_bytes: usize,
) -> Option<Thumb> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    let mut edge = max_edge.max(1);
    loop {
        let (tw, th) = fit_within(width, height, edge);
        let pixels = if (tw, th) == (width, height) {
            std::borrow::Cow::Borrowed(rgba)
        } else {
            std::borrow::Cow::Owned(downscale_rgba(width, height, rgba, tw, th))
        };
        let png = encode_png(tw, th, &pixels)?;
        if png.len() <= cap_bytes {
            return Some(Thumb {
                width: tw,
                height: th,
                png,
            });
        }
        if edge <= 32 {
            return None;
        }
        edge = (edge * 3 / 4).max(32);
    }
}

/// `data:<mime>;base64,<payload>`.
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

/// Inverse of [`data_url`]; `None` for anything that is not a base64 data URL.
pub fn parse_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (head, payload) = rest.split_once(',')?;
    let mime = head.strip_suffix(";base64")?;
    let bytes = STANDARD.decode(payload).ok()?;
    Some((mime.to_string(), bytes))
}

/// Stable 64-bit FNV-1a (unlike `DefaultHasher`, its output never changes between
/// Rust releases, so it is safe to persist in file names).
pub fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// File name of the full-resolution sidecar for an image clip, derived from the
/// clip's own content (its thumbnail data URL): 16 hex digits + `.png`.
pub fn sidecar_name(content: &str) -> String {
    format!("{:016x}.png", fnv1a64(content))
}

/// Whether `name` has the shape [`sidecar_name`] produces (so pruning only ever
/// deletes files Krovi itself wrote).
pub fn is_sidecar_name(name: &str) -> bool {
    name.len() == 20
        && name.ends_with(".png")
        && name.as_bytes()[..16].iter().all(u8::is_ascii_hexdigit)
}

/// Image mime type from magic bytes.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 256) as u8,
                    255,
                ]);
            }
        }
        v
    }

    #[test]
    fn png_round_trips_rgba() {
        let rgba = gradient(8, 5);
        let png = encode_png(8, 5, &rgba).unwrap();
        assert_eq!(sniff_mime(&png), Some("image/png"));
        let (w, h, back) = decode_png(&png).unwrap();
        assert_eq!((w, h), (8, 5));
        assert_eq!(back, rgba);
    }

    #[test]
    fn encode_rejects_bad_buffers() {
        assert!(encode_png(0, 4, &[]).is_none());
        assert!(encode_png(2, 2, &[0; 15]).is_none());
        assert!(decode_png(b"not a png").is_none());
    }

    #[test]
    fn fit_within_keeps_aspect_and_never_upscales() {
        assert_eq!(fit_within(1000, 500, 250), (250, 125));
        assert_eq!(fit_within(500, 1000, 250), (125, 250));
        assert_eq!(fit_within(100, 50, 256), (100, 50));
        assert_eq!(fit_within(10_000, 1, 100), (100, 1)); // never collapses to 0
    }

    #[test]
    fn downscale_averages_blocks() {
        // 2x2 -> 1x1 : average of four opaque pixels.
        let src = [
            10, 20, 30, 255, //
            30, 40, 50, 255, //
            50, 60, 70, 255, //
            70, 80, 90, 255,
        ];
        let out = downscale_rgba(2, 2, &src, 1, 1);
        assert_eq!(out, vec![40, 50, 60, 255]);
    }

    #[test]
    fn downscale_is_alpha_weighted() {
        // One opaque white pixel + one fully transparent black pixel: the colour of
        // the result must stay white (not grey), at half alpha.
        let src = [255, 255, 255, 255, 0, 0, 0, 0];
        let out = downscale_rgba(2, 1, &src, 1, 1);
        assert_eq!(&out[..3], &[255, 255, 255]);
        assert_eq!(out[3], 127);
    }

    #[test]
    fn thumbnail_shrinks_to_edge_and_fits_cap() {
        let rgba = gradient(640, 400);
        let t = make_thumbnail(640, 400, &rgba, 256, THUMB_CAP_BYTES).unwrap();
        assert_eq!((t.width, t.height), (256, 160));
        assert!(t.png.len() <= THUMB_CAP_BYTES);
        let (w, h, _) = decode_png(&t.png).unwrap();
        assert_eq!((w, h), (256, 160));
    }

    #[test]
    fn thumbnail_steps_down_until_under_a_tight_cap() {
        // Noisy image: incompressible-ish, so a tiny cap forces extra shrink steps.
        let (w, h) = (300u32, 300u32);
        let mut seed = 0x2545F491u32;
        let mut rgba = Vec::new();
        for _ in 0..(w * h) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            rgba.extend_from_slice(&[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 255]);
        }
        let big = make_thumbnail(w, h, &rgba, 256, 10_000_000).unwrap();
        let small = make_thumbnail(w, h, &rgba, 256, 40_000).unwrap();
        assert!(small.width < big.width);
        assert!(small.png.len() <= 40_000);
        // an impossible cap yields None rather than an oversize blob
        assert!(make_thumbnail(w, h, &rgba, 256, 100).is_none());
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let rgba = gradient(20, 10);
        let t = make_thumbnail(20, 10, &rgba, 256, THUMB_CAP_BYTES).unwrap();
        assert_eq!((t.width, t.height), (20, 10));
    }

    #[test]
    fn data_url_round_trips() {
        let url = data_url("image/png", &[1, 2, 3, 250]);
        assert!(url.starts_with("data:image/png;base64,"));
        assert_eq!(
            parse_data_url(&url),
            Some(("image/png".into(), vec![1, 2, 3, 250]))
        );
        assert_eq!(parse_data_url("https://x"), None);
        assert_eq!(parse_data_url("data:text/plain,hi"), None); // not base64
        assert_eq!(parse_data_url("data:image/png;base64,@@@"), None);
    }

    #[test]
    fn fnv1a_matches_the_published_vectors() {
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(
            fnv1a64("data:image/png;base64,AAA"),
            fnv1a64("data:image/png;base64,AAB")
        );
    }

    #[test]
    fn sidecar_names_round_trip_the_shape_check() {
        let n = sidecar_name("data:image/png;base64,xyz");
        assert_eq!(n.len(), 20);
        assert!(is_sidecar_name(&n));
        assert_eq!(n, sidecar_name("data:image/png;base64,xyz"));
        for not_ours in [
            "notes.txt",
            "readme.png",
            "ZZZZZZZZZZZZZZZZ.png",
            "0123456789abcdef.jpg",
            "",
        ] {
            assert!(!is_sidecar_name(not_ours), "{not_ours}");
        }
    }

    #[test]
    fn sniffs_common_formats() {
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_mime(b"GIF89a.."), Some("image/gif"));
        assert_eq!(sniff_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_mime(b"hello"), None);
    }
}
