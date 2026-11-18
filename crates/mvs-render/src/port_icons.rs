//! The port's own UI icons, which the build's icon set has no picture for: white flat glyphs on transparent drawn in
//! code, the size and stroke weight of the build's `ui_icons_*` (128 x 128, strokes about 14 texels), served by name
//! wherever a store picture of that name would be.

/// An open eye: the custom lobby's room code is shown.
pub const EYE: &str = "port_icon_eye";
/// An eye struck through: the room code is masked.
pub const EYE_OFF: &str = "port_icon_eye_off";

/// The side of every icon, as the build's `ui_icons_*` (128 x 128 with eight mips).
pub const SIZE: u32 = 128;

// The eye, in texels of the 128 square: an almond of two arcs meeting in points, a stroke as heavy as the build's
// lock, and a round iris inside it.
const EYE_HALF_W: f32 = 54.0;
const EYE_HALF_H: f32 = 37.0;
const STROKE: f32 = 14.0;
const IRIS: f32 = 18.0;
// The slash, corner to corner, and the clear space cut around it.
const SLASH_HALF_LEN: f32 = 62.0;
const SLASH_HALF_W: f32 = 7.0;
const SLASH_GAP: f32 = 7.0;

/// Whether `name` is one of the port's own icons.
pub fn is_port_icon(name: &str) -> bool {
    name == EYE || name == EYE_OFF
}

/// The icon `name`'s mips, largest first, each `SIZE >> i` square of RGBA8 (white, the coverage in alpha); None for a
/// name that is not the port's.
pub fn mips(name: &str) -> Option<Vec<Vec<u8>>> {
    let off = match name {
        EYE => false,
        EYE_OFF => true,
        _ => return None,
    };
    let mut out = vec![raster(off)];
    let mut side = SIZE as usize;
    while side > 1 {
        let next = down(out.last().expect("a mip"), side);
        side /= 2;
        out.push(next);
    }
    Some(out)
}

/// Mip 0: every texel's coverage from the glyph's signed distance (negative inside), anti-aliased over one texel.
fn raster(off: bool) -> Vec<u8> {
    let n = SIZE as usize;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let d = if off { eye_off(p) } else { eye(p) };
            let a = (0.5 - d).clamp(0.0, 1.0);
            let o = (y * n + x) * 4;
            px[o..o + 3].fill(255);
            px[o + 3] = (a * 255.0).round() as u8;
        }
    }
    px
}

/// The next mip: each 2 x 2 texels' alpha averaged (the colour is white throughout).
fn down(px: &[u8], side: usize) -> Vec<u8> {
    let h = side / 2;
    let mut out = vec![255u8; h * h * 4];
    for y in 0..h {
        for x in 0..h {
            let a = |dx: usize, dy: usize| px[((2 * y + dy) * side + 2 * x + dx) * 4 + 3] as u32;
            out[(y * h + x) * 4 + 3] = ((a(0, 0) + a(1, 0) + a(0, 1) + a(1, 1) + 2) / 4) as u8;
        }
    }
    out
}

fn eye(p: (f32, f32)) -> f32 {
    let c = SIZE as f32 / 2.0;
    let (x, y) = (p.0 - c, p.1 - c);
    // Each lid an arc through the corners (±EYE_HALF_W, 0) and its peak (0, ∓EYE_HALF_H): its circle's radius and
    // centre on the far side of the middle.
    let r = (EYE_HALF_W * EYE_HALF_W + EYE_HALF_H * EYE_HALF_H) / (2.0 * EYE_HALF_H);
    let k = r - EYE_HALF_H;
    let upper = x.hypot(y - k) - r;
    let lower = x.hypot(y + k) - r;
    let almond = upper.max(lower);
    let outline = almond.max(-(almond + STROKE));
    let iris = x.hypot(y) - IRIS;
    outline.min(iris)
}

fn eye_off(p: (f32, f32)) -> f32 {
    let c = SIZE as f32 / 2.0;
    let (x, y) = (p.0 - c, p.1 - c);
    // top left to bottom right, at 45 degrees: u along the slash, v across it
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let (u, v) = ((x + y) * s, (y - x) * s);
    let slash = rect(u, v, SLASH_HALF_LEN, SLASH_HALF_W);
    let gap = rect(u, v, SLASH_HALF_LEN + SLASH_GAP, SLASH_HALF_W + SLASH_GAP);
    eye(p).max(-gap).min(slash)
}

/// The signed distance to a rectangle of half sides `hu`, `hv` about the origin.
fn rect(u: f32, v: f32, hu: f32, hv: f32) -> f32 {
    let (qu, qv) = (u.abs() - hu, v.abs() - hv);
    qu.max(0.0).hypot(qv.max(0.0)) + qu.max(qv).min(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both icons are 128 square with a full mip chain down to one texel, white, and drawn: about the coverage of
    /// the build's lock (5536 texels of alpha in its 128 square), transparent at the border.
    #[test]
    fn the_eyes_are_drawn_at_the_build_icons_size() {
        for name in [EYE, EYE_OFF] {
            let m = mips(name).unwrap();
            assert_eq!(m.len(), 8, "{name}");
            for (i, px) in m.iter().enumerate() {
                let side = (SIZE >> i) as usize;
                assert_eq!(px.len(), side * side * 4, "{name} mip {i}");
            }
            let px = &m[0];
            assert!(px.chunks(4).all(|t| t[..3] == [255, 255, 255]), "{name}: white");
            let cover: f64 = px.chunks(4).map(|t| t[3] as f64 / 255.0).sum();
            assert!((3000.0..7000.0).contains(&cover), "{name}: coverage {cover}");
            let n = SIZE as usize;
            let border = (0..n).flat_map(|i| [i, (n - 1) * n + i, i * n, i * n + n - 1]);
            assert!(border.into_iter().all(|t| px[t * 4 + 3] == 0), "{name}: a clear border");
            // anti-aliased: partial coverage along the edges
            assert!(px.chunks(4).any(|t| t[3] > 0 && t[3] < 255), "{name}: soft edges");
        }
        assert!(mips("ui_icons_locked").is_none());
        assert!(is_port_icon(EYE) && is_port_icon(EYE_OFF) && !is_port_icon("ui_icons_locked"));
    }
}
