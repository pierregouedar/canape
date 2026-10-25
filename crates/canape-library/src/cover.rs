//! Cover art, decoded and scaled down to the size it is shown at: a
//! 600x900 JPEG costs 2 MB decoded, a 186x279 tile 150 KB. The shell only
//! keeps the covers it shows.
//!
//! Portrait art is cropped to fill the tile. Landscape or square art (a
//! Steam header, an Xbox logo) is shown whole, centred on a blurred and
//! darkened copy of itself.

use std::io;
use std::path::Path;

/// An 8-bit RGB image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Transparent pixels are laid on the shell's dark surface colour.
const MATTE: [u8; 3] = [0x14, 0x1a, 0x26];
/// Largest image decoded, per side: anything bigger is not cover art.
const MAX_SIDE: usize = 4096;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Decodes a JPEG or PNG file and fits it in `width` x `height`.
pub fn load(path: &Path, width: u32, height: u32) -> io::Result<Rgb> {
    let bytes = std::fs::read(path)?;
    Ok(fit(&decode(&bytes)?, width, height))
}

/// Decodes a JPEG or PNG file and crops it to fill `width` x `height`,
/// whatever its shape (a game page's wide art).
pub fn load_filled(path: &Path, width: u32, height: u32) -> io::Result<Rgb> {
    let src = decode(&std::fs::read(path)?)?;
    let (w, h) = (width.max(1), height.max(1));
    if src.width == 0 || src.height == 0 {
        return Err(invalid("empty image"));
    }
    Ok(Rgb {
        width: w,
        height: h,
        pixels: resample(&src, crop(&src, w, h), w, h),
    })
}

/// Decodes JPEG or PNG bytes, told apart by their signature.
pub fn decode(bytes: &[u8]) -> io::Result<Rgb> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        decode_jpeg(bytes)
    } else if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes)
    } else {
        Err(invalid("not a JPEG or PNG image"))
    }
}

fn decode_jpeg(bytes: &[u8]) -> io::Result<Rgb> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;

    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(MAX_SIDE)
        .set_max_height(MAX_SIDE);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder
        .decode()
        .map_err(|e| invalid(format!("JPEG: {e:?}")))?;
    let info = decoder.info().ok_or_else(|| invalid("JPEG: no header"))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if pixels.len() != (width * height * 3) as usize {
        return Err(invalid("JPEG: unexpected pixel format"));
    }
    Ok(Rgb {
        width,
        height,
        pixels,
    })
}

fn decode_png(bytes: &[u8]) -> io::Result<Rgb> {
    let mut decoder = png::Decoder::new(io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| invalid(format!("PNG: {e}")))?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(invalid("PNG: too large"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| invalid("PNG: too large"))?;
    let mut buf = vec![0; size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| invalid(format!("PNG: {e}")))?;
    let channels = frame.color_type.samples();
    let mut pixels = Vec::with_capacity(w * h * 3);
    for row in buf[..frame.buffer_size()].chunks_exact(frame.line_size) {
        for px in row[..w * channels].chunks_exact(channels) {
            let (rgb, alpha) = match channels {
                1 => ([px[0]; 3], 255),
                2 => ([px[0]; 3], px[1]),
                3 => ([px[0], px[1], px[2]], 255),
                _ => ([px[0], px[1], px[2]], px[3]),
            };
            for (c, m) in rgb.iter().zip(MATTE) {
                let a = u32::from(alpha);
                pixels.push(((u32::from(*c) * a + u32::from(m) * (255 - a) + 127) / 255) as u8);
            }
        }
    }
    Ok(Rgb {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

/// A source rectangle, in source pixels.
#[derive(Clone, Copy)]
struct Area {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// For each destination pixel along one axis, the source pixels it covers
/// and their share (box filter).
fn coverage(start: f64, len: f64, out: u32, limit: u32) -> Vec<Vec<(usize, f32)>> {
    let step = len / f64::from(out);
    (0..out)
        .map(|i| {
            let (a, b) = (start + f64::from(i) * step, start + f64::from(i + 1) * step);
            let mut taps = Vec::new();
            let mut p = a.floor();
            while p < b {
                let share = (p + 1.0).min(b) - p.max(a);
                if share > 0.0 {
                    let index = (p.max(0.0) as usize).min(limit as usize - 1);
                    taps.push((index, share as f32));
                }
                p += 1.0;
            }
            let total: f32 = taps.iter().map(|t| t.1).sum();
            taps.iter_mut().for_each(|t| t.1 /= total);
            taps
        })
        .collect()
}

/// `area` of `src` scaled to `w` x `h`: averaged when shrinking, bilinear
/// when enlarging.
fn resample(src: &Rgb, area: Area, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    let at = |x: usize, y: usize| (y * src.width as usize + x) * 3;
    if area.w >= f64::from(w) && area.h >= f64::from(h) {
        let xs = coverage(area.x, area.w, w, src.width);
        let ys = coverage(area.y, area.h, h, src.height);
        for row in &ys {
            for col in &xs {
                let mut sum = [0f32; 3];
                for &(y, wy) in row {
                    for &(x, wx) in col {
                        let i = at(x, y);
                        for (total, value) in sum.iter_mut().zip(&src.pixels[i..i + 3]) {
                            *total += f32::from(*value) * wx * wy;
                        }
                    }
                }
                out.extend(sum.map(|v| v.round().clamp(0.0, 255.0) as u8));
            }
        }
    } else {
        let (sx, sy) = (area.w / f64::from(w), area.h / f64::from(h));
        let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max - 1));
        for j in 0..h {
            let fy = clamp(area.y + (f64::from(j) + 0.5) * sy - 0.5, src.height);
            let (y0, ty) = (fy.floor() as usize, (fy - fy.floor()) as f32);
            let y1 = (y0 + 1).min(src.height as usize - 1);
            for i in 0..w {
                let fx = clamp(area.x + (f64::from(i) + 0.5) * sx - 0.5, src.width);
                let (x0, tx) = (fx.floor() as usize, (fx - fx.floor()) as f32);
                let x1 = (x0 + 1).min(src.width as usize - 1);
                for c in 0..3 {
                    let p = |x, y| f32::from(src.pixels[at(x, y) + c]);
                    let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
                    let bottom = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
                    out.push((top + (bottom - top) * ty).round() as u8);
                }
            }
        }
    }
    out
}

/// The centred part of `src` with the aspect ratio of `w` x `h`.
fn crop(src: &Rgb, w: u32, h: u32) -> Area {
    let (sw, sh) = (f64::from(src.width), f64::from(src.height));
    let target = f64::from(w) / f64::from(h);
    if sw / sh > target {
        let cw = sh * target;
        Area {
            x: (sw - cw) / 2.0,
            y: 0.0,
            w: cw,
            h: sh,
        }
    } else {
        let ch = sw / target;
        Area {
            x: 0.0,
            y: (sh - ch) / 2.0,
            w: sw,
            h: ch,
        }
    }
}

/// RGBA copy of `img` with its corners cut round, anti-aliased, to
/// `radius` pixels: the software renderer does not clip images to a
/// rounded rectangle.
pub fn rounded(img: &Rgb, radius: f32) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let r = radius.clamp(0.0, w.min(h) as f32 / 2.0);
    let mut out = Vec::with_capacity(w * h * 4);
    for (i, px) in img.pixels.chunks_exact(3).enumerate() {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        // Distance into the corner square, from the arc's centre.
        let dx = (r - x).max(x - (w as f32 - r)).max(0.0);
        let dy = (r - y).max(y - (h as f32 - r)).max(0.0);
        let alpha = if dx > 0.0 && dy > 0.0 {
            (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0)
        } else {
            1.0
        };
        out.extend_from_slice(px);
        out.push((alpha * 255.0).round() as u8);
    }
    out
}

/// `src` fitted in `w` x `h` (see the module comment).
pub fn fit(src: &Rgb, w: u32, h: u32) -> Rgb {
    let (w, h) = (w.max(1), h.max(1));
    let full = Area {
        x: 0.0,
        y: 0.0,
        w: f64::from(src.width),
        h: f64::from(src.height),
    };
    let ratio = (f64::from(src.width) / f64::from(src.height)) / (f64::from(w) / f64::from(h));
    if src.width == 0 || src.height == 0 {
        return Rgb {
            width: w,
            height: h,
            pixels: MATTE.repeat((w * h) as usize),
        };
    }
    if (0.8..=1.25).contains(&ratio) {
        return Rgb {
            width: w,
            height: h,
            pixels: resample(src, crop(src, w, h), w, h),
        };
    }

    // Backdrop: the art filling the tile, shrunk to a few pixels and blown
    // up again (a cheap blur), then darkened.
    let (bw, bh) = ((w / 16).max(2), (h / 16).max(2));
    let small = Rgb {
        width: bw,
        height: bh,
        pixels: resample(src, crop(src, bw, bh), bw, bh),
    };
    let mut pixels = resample(
        &small,
        Area {
            x: 0.0,
            y: 0.0,
            w: f64::from(bw),
            h: f64::from(bh),
        },
        w,
        h,
    );
    pixels
        .iter_mut()
        .for_each(|p| *p = (u16::from(*p) * 2 / 5) as u8);

    // The whole art, as wide as the tile (or as tall, if it is taller).
    let scale = (f64::from(w) / full.w).min(f64::from(h) / full.h);
    let (fw, fh) = (
        ((full.w * scale).round() as u32).clamp(1, w),
        ((full.h * scale).round() as u32).clamp(1, h),
    );
    let art = resample(src, full, fw, fh);
    let (ox, oy) = ((w - fw) / 2, (h - fh) * 2 / 5);
    for row in 0..fh {
        let dst = (((oy + row) * w + ox) * 3) as usize;
        let from = (row * fw * 3) as usize;
        pixels[dst..dst + (fw * 3) as usize].copy_from_slice(&art[from..from + (fw * 3) as usize]);
    }
    Rgb {
        width: w,
        height: h,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> Rgb {
        Rgb {
            width: w,
            height: h,
            pixels: rgb.repeat((w * h) as usize),
        }
    }

    fn px(img: &Rgb, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.width + x) * 3) as usize;
        [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2]]
    }

    #[test]
    fn portrait_art_fills_the_tile() {
        // Left half red, right half blue: the crop keeps both, centred.
        let mut src = solid(60, 90, [255, 0, 0]);
        for y in 0..90 {
            for x in 30..60 {
                let i = ((y * 60 + x) * 3) as usize;
                src.pixels[i..i + 3].copy_from_slice(&[0, 0, 255]);
            }
        }
        let out = fit(&src, 20, 30);
        assert_eq!(
            (out.width, out.height, out.pixels.len()),
            (20, 30, 20 * 30 * 3)
        );
        assert_eq!(px(&out, 0, 15), [255, 0, 0]);
        assert_eq!(px(&out, 19, 15), [0, 0, 255]);
        // Enlarging works too.
        assert_eq!(fit(&src, 120, 180).pixels.len(), 120 * 180 * 3);
    }

    #[test]
    fn landscape_art_is_framed() {
        let out = fit(&solid(460, 215, [200, 200, 200]), 100, 150);
        assert_eq!(out.pixels.len(), 100 * 150 * 3);
        // The art across the whole width, darkened backdrop above it.
        assert_eq!(px(&out, 50, 60), [200, 200, 200]);
        assert_eq!(px(&out, 50, 2), [80, 80, 80]);
    }

    #[test]
    fn rounded_corners_are_transparent() {
        let img = solid(40, 60, [10, 20, 30]);
        let rgba = rounded(&img, 8.0);
        assert_eq!(rgba.len(), 40 * 60 * 4);
        let alpha = |x: usize, y: usize| rgba[(y * 40 + x) * 4 + 3];
        for (x, y) in [(0, 0), (39, 0), (0, 59), (39, 59)] {
            assert_eq!(alpha(x, y), 0, "corner {x},{y}");
        }
        assert_eq!(alpha(20, 0), 255);
        assert_eq!(alpha(0, 30), 255);
        assert_eq!(alpha(20, 30), 255);
        assert_eq!(&rgba[..3], &[10, 20, 30]);
        // Along the arc, partly covered.
        assert!((1..255).contains(&alpha(1, 3)));
        // No radius: opaque everywhere.
        assert!(rounded(&img, 0.0).chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn decodes_fixture_images() {
        let jpg = load(&fixture("images/cover.jpg"), 40, 60).unwrap();
        assert_eq!((jpg.width, jpg.height), (40, 60));
        let png = decode(&std::fs::read(fixture("images/logo.png")).unwrap()).unwrap();
        assert_eq!((png.width, png.height), (8, 8));
        // Transparent corner on the matte, opaque centre as drawn.
        assert_eq!(px(&png, 0, 0), MATTE);
        assert_eq!(px(&png, 4, 4), [255, 255, 255]);
        assert!(decode(b"GIF89a").is_err());
        assert!(decode(&[0xff, 0xd8, 0, 0]).is_err());
        // Wide art fills its band whatever the image's shape.
        let wide = load_filled(&fixture("images/cover.jpg"), 310, 100).unwrap();
        assert_eq!(
            (wide.width, wide.height, wide.pixels.len()),
            (310, 100, 310 * 100 * 3)
        );
    }
}
