//! The library's backdrop: a soft glow in the selected game's colour, drawn
//! at a quarter of the window's size and stretched over it.

/// Size of the drawn glow; the window stretches it to its own size.
const WIDTH: usize = 320;
const HEIGHT: usize = 180;
/// Opacity at the centre of the glow.
const STRENGTH: f32 = 0.38;

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

/// The glow as RGBA pixels, `WIDTH` x `HEIGHT`: an ellipse towards the top
/// right, fading out from its centre.
pub fn render(color: [u8; 3]) -> (Vec<u8>, u32, u32) {
    let mut out = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = ((x as f32 + 0.5) / WIDTH as f32 - 0.74) / 0.35;
            let dy = ((y as f32 + 0.5) / HEIGHT as f32 - 0.22) / 0.44;
            let d = (dx * dx + dy * dy).sqrt();
            let alpha = STRENGTH * (1.0 - smoothstep(0.1, 1.4, d));
            out.extend(color);
            out.push((alpha * 255.0).round() as u8);
        }
    }
    (out, WIDTH as u32, HEIGHT as u32)
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
        let (pixels, w, h) = render([255, 0, 0]);
        assert_eq!(pixels.len(), (w * h * 4) as usize);
        let alpha = |x: u32, y: u32| pixels[((y * w + x) * 4 + 3) as usize];
        let centre = alpha(w * 74 / 100, h * 22 / 100);
        assert!(centre > 80);
        assert!(alpha(0, h - 1) < 5);
    }
}
