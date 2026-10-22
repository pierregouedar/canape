//! The library's backdrop: a soft glow in the selected game's colour that
//! fades into the page background towards the shelves. Drawn opaque at the
//! window's own size, so the software renderer only copies it: stretching
//! a small image and blending a gradient over it on every frame cost most
//! of the library's frame time.

/// Opacity at the centre of the glow.
const STRENGTH: f32 = 0.38;
/// Share of the window's height where the backdrop has faded out: below,
/// the window's own background shows.
const FADED_AT: f32 = 0.8;

/// A vivid version of the average colour of RGB pixels, so a dark or grey
/// cover still tints the page.
pub fn tint(pixels: &[u8]) -> [u8; 3] {
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    // Every 7th pixel is plenty for an average.
    for px in pixels.chunks_exact(3).step_by(7) {
        for c in 0..3 {
            sum[c] += u64::from(px[c]);
        }
        n += 1;
    }
    if n == 0 {
        return [0x80, 0x80, 0x80];
    }
    let mean = sum.map(|s| s as f32 / n as f32 / 255.0);
    let (h, s, _) = hsv(mean);
    from_hsv(h, s.max(0.55), 0.85)
}

/// A colour of its own for a game without art, from its id.
pub fn fallback(id: &str) -> [u8; 3] {
    // FNV-1a: stable across runs and platforms.
    let hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    from_hsv((hash % 360) as f32, 0.65, 0.85)
}

/// How much of the colour each pixel takes, 0 to 255, for one window
/// size: computed once, then [`paint`]ed in each game's colour.
pub struct Mask {
    pub width: u32,
    pub height: u32,
    weights: Vec<u8>,
}

/// The glow for a `width` x `height` window (physical pixels): an ellipse
/// towards the top right, fading out from its centre and towards the
/// bottom. Covers the top [`FADED_AT`] of the window.
pub fn mask(width: u32, window_height: u32) -> Mask {
    let height = ((window_height as f32 * FADED_AT).ceil() as u32).max(1);
    let (w, wh) = (width.max(1) as f32, window_height.max(1) as f32);
    let mut weights = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        let ty = (y as f32 + 0.5) / wh;
        let fade = 1.0 - smoothstep(0.05, FADED_AT, ty);
        let dy = (ty - 0.22) / 0.44;
        for x in 0..width {
            let dx = ((x as f32 + 0.5) / w - 0.74) / 0.35;
            let d = (dx * dx + dy * dy).sqrt();
            let alpha = STRENGTH * (1.0 - smoothstep(0.1, 1.4, d)) * fade;
            weights.push((alpha * 255.0).round() as u8);
        }
    }
    Mask {
        width,
        height,
        weights,
    }
}

/// The backdrop in `color` over `background`, as opaque RGB pixels of the
/// mask's size.
pub fn paint(mask: &Mask, color: [u8; 3], background: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(mask.weights.len() * 3);
    for &w in &mask.weights {
        let w = u32::from(w);
        for c in 0..3 {
            let (fg, bg) = (u32::from(color[c]), u32::from(background[c]));
            out.push(((fg * w + bg * (255 - w) + 127) / 255) as u8);
        }
    }
    out
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hsv([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { delta / max };
    (h, s, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|v| ((v + m) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_keeps_the_hue_and_brightens() {
        // Dark blue cover.
        let pixels = [10u8, 20, 60].repeat(100);
        let [r, g, b] = tint(&pixels);
        assert!(b > r && b > g);
        assert_eq!(b, 217);
    }

    #[test]
    fn fallback_is_stable() {
        assert_eq!(fallback("steam:10"), fallback("steam:10"));
        assert_ne!(fallback("steam:10"), fallback("steam:20"));
    }

    #[test]
    fn glow_fades_from_the_top_right() {
        let mask = mask(320, 180);
        assert_eq!(mask.height, 144);
        let pixels = paint(&mask, [255, 0, 0], [11, 12, 15]);
        assert_eq!(pixels.len(), (320 * 144 * 3) as usize);
        let red = |x: u32, y: u32| pixels[((y * 320 + x) * 3) as usize];
        assert!(red(320 * 74 / 100, 180 * 22 / 100) > 80);
        // Faded into the background at the bottom and on the left.
        assert_eq!(red(0, 143), 11);
        assert!(red(319, 143) <= 12);
    }
}
