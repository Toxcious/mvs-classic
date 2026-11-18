//! The import's own cook of a texture the build ships uncompressed with one mip that a player sees only small (two
//! ring-out sprites): its top mip scaled down to at most [`COOK_SIDE`] a side, a mip chain made, every mip encoded
//! as DXT5 (BC3: an alpha block of eight steps, a colour block of four).
// Deviation: the build draws these at their cooked size (B8G8R8A8, one mip); the store keeps them smaller and
// compressed (store size and load hitch), where a player cannot tell them apart in a ring-out.
// Unverified: the scale and the mips average in linear light for an sRGB texture (as UE's mip generation does for
// an sRGB source), alpha straight; the encoder is the port's own (principal-axis endpoints), not the build's cooker.

/// The cooked top mip's longest side.
pub const COOK_SIDE: u32 = 1024;

/// The textures cooked (object paths).
pub const COOKED: &[&str] = &[
    "/Game/Panda_Main/VFX_Ringouts/LassoofTruth/T_WW7x6.T_WW7x6",
    "/Game/Panda_Main/VFX_Ringouts/RO_PFGArrival/T_Founders_3_Rays_2K.T_Founders_3_Rays_2K",
];

/// One mip of a cook: its size and its BC3 blocks.
pub struct CookedMip {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

fn to_linear(c: u8) -> f32 {
    let x = f32::from(c) / 255.0;
    if x <= 0.040_45 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// The top size: the longest side [`COOK_SIDE`] (or as it is when smaller), the other in proportion, each a multiple
/// of 4 (a BC texture's top mip) and at least 4.
pub fn cooked_size(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= COOK_SIDE {
        return (w.next_multiple_of(4), h.next_multiple_of(4));
    }
    let side = |n: u32| ((f64::from(n) * f64::from(COOK_SIDE) / f64::from(long) / 4.0).round() as u32 * 4).max(4);
    (side(w), side(h))
}

/// The spans of source texels each of `dst` texels covers when `src` texels are scaled to `dst`: (first texel,
/// weights), the weights summing to 1.
fn spans(src: u32, dst: u32) -> Vec<(usize, Vec<f32>)> {
    let scale = f64::from(src) / f64::from(dst);
    (0..dst)
        .map(|i| {
            let (a, b) = (f64::from(i) * scale, f64::from(i + 1) * scale);
            let first = a.floor() as usize;
            let last = (b.ceil() as usize).min(src as usize).max(first + 1);
            let w: Vec<f32> =
                (first..last).map(|k| ((b.min(k as f64 + 1.0) - a.max(k as f64)).max(0.0) / scale) as f32).collect();
            (first, w)
        })
        .collect()
}

/// `rgba` (RGBA8, `w` x `h`) as linear (sRGB decoded when `srgb`) float RGBA scaled by area to `dw` x `dh`.
fn scale(rgba: &[u8], w: u32, h: u32, dw: u32, dh: u32, srgb: bool) -> Vec<[f32; 4]> {
    let lin = |c: u8, ch: usize| if srgb && ch < 3 { to_linear(c) } else { f32::from(c) / 255.0 };
    let (sx, sy) = (spans(w, dw), spans(h, dh));
    // rows first (each source row narrowed to dw), then columns
    let mut rows = vec![[0f32; 4]; dw as usize * h as usize];
    for y in 0..h as usize {
        let row = &rgba[y * w as usize * 4..(y + 1) * w as usize * 4];
        for (x, (first, ws)) in sx.iter().enumerate() {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let p = &row[(first + k) * 4..(first + k) * 4 + 4];
                for (ch, a) in acc.iter_mut().enumerate() {
                    *a += lin(p[ch], ch) * wt;
                }
            }
            rows[y * dw as usize + x] = acc;
        }
    }
    let mut out = vec![[0f32; 4]; dw as usize * dh as usize];
    for (y, (first, ws)) in sy.iter().enumerate() {
        for x in 0..dw as usize {
            let mut acc = [0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let p = rows[(first + k) * dw as usize + x];
                for ch in 0..4 {
                    acc[ch] += p[ch] * wt;
                }
            }
            out[y * dw as usize + x] = acc;
        }
    }
    out
}

/// The next mip: each texel the mean of the 2 x 2 above it (an odd side's last row or column taken once).
fn halve(px: &[[f32; 4]], w: u32, h: u32) -> (Vec<[f32; 4]>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![[0f32; 4]; nw as usize * nh as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0f32; 4];
            let mut n = 0.0;
            for (sx, sy) in [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)] {
                if sx < w && sy < h {
                    let p = px[(sy * w + sx) as usize];
                    for ch in 0..4 {
                        acc[ch] += p[ch];
                    }
                    n += 1.0;
                }
            }
            out[(y * nw + x) as usize] = acc.map(|a| a / n);
        }
    }
    (out, nw, nh)
}

/// The texels as RGBA8 (sRGB encoded when `srgb`).
fn bytes_of(px: &[[f32; 4]], srgb: bool) -> Vec<[u8; 4]> {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    px.iter().map(|p| [0, 1, 2, 3].map(|ch| if srgb && ch < 3 { byte(to_srgb(p[ch])) } else { byte(p[ch]) })).collect()
}

/// The cook of `rgba` (RGBA8, `w` x `h`): its mips, largest first, down to 1 x 1.
pub fn cook_dxt5(rgba: &[u8], w: u32, h: u32, srgb: bool) -> Vec<CookedMip> {
    let (dw, dh) = cooked_size(w, h);
    let mut level = scale(rgba, w, h, dw, dh, srgb);
    let (mut lw, mut lh) = (dw, dh);
    let mut out = Vec::new();
    loop {
        out.push(CookedMip { width: lw, height: lh, bytes: encode_bc3(&bytes_of(&level, srgb), lw, lh) });
        if lw == 1 && lh == 1 {
            return out;
        }
        (level, lw, lh) = halve(&level, lw, lh);
    }
}

/// `px` (`w` x `h`) as BC3 blocks, row by row; a block past an edge repeats the edge's texels.
pub fn encode_bc3(px: &[[u8; 4]], w: u32, h: u32) -> Vec<u8> {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity(bw as usize * bh as usize * 16);
    for by in 0..bh {
        for bx in 0..bw {
            let block: [[u8; 4]; 16] = std::array::from_fn(|i| {
                let x = (bx * 4 + (i as u32 % 4)).min(w - 1);
                let y = (by * 4 + (i as u32 / 4)).min(h - 1);
                px[(y * w + x) as usize]
            });
            out.extend_from_slice(&alpha_block(&block));
            out.extend_from_slice(&color_block(&block));
        }
    }
    out
}

/// BC3's alpha half (BC4's eight-step mode: a0 > a1).
fn alpha_block(b: &[[u8; 4]; 16]) -> [u8; 8] {
    let a0 = b.iter().map(|p| p[3]).max().unwrap_or(0);
    let a1 = b.iter().map(|p| p[3]).min().unwrap_or(0);
    let mut out = [0u8; 8];
    out[0] = a0;
    out[1] = a1;
    if a0 == a1 {
        return out;
    }
    let steps: [i32; 8] = std::array::from_fn(|k| match k {
        0 => i32::from(a0),
        1 => i32::from(a1),
        k => ((8 - k as i32) * i32::from(a0) + (k as i32 - 1) * i32::from(a1)) / 7,
    });
    let mut bits = 0u64;
    for (i, p) in b.iter().enumerate() {
        let a = i32::from(p[3]);
        let k = (0..8).min_by_key(|&k| (steps[k] - a).abs()).unwrap_or(0);
        bits |= (k as u64) << (3 * i);
    }
    out[2..].copy_from_slice(&bits.to_le_bytes()[..6]);
    out
}

fn to_565(c: [f32; 3]) -> u16 {
    let q = |x: f32, max: f32| (x.clamp(0.0, 255.0) / 255.0 * max).round() as u16;
    (q(c[0], 31.0) << 11) | (q(c[1], 63.0) << 5) | q(c[2], 31.0)
}

fn from_565(c: u16) -> [i32; 3] {
    let (r, g, b) = (i32::from(c >> 11), i32::from((c >> 5) & 63), i32::from(c & 31));
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// BC3's colour half (BC1's four-colour mode, c0 > c1): the endpoints at the ends of the texels' principal axis,
/// inset by a sixteenth of the span; each texel the nearest of the four.
fn color_block(b: &[[u8; 4]; 16]) -> [u8; 8] {
    let rgb: Vec<[f32; 3]> = b.iter().map(|p| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]).collect();
    let mean = rgb.iter().fold([0f32; 3], |m, c| [m[0] + c[0] / 16.0, m[1] + c[1] / 16.0, m[2] + c[2] / 16.0]);
    let mut cov = [0f32; 6];
    for c in &rgb {
        let d = [c[0] - mean[0], c[1] - mean[1], c[2] - mean[2]];
        cov[0] += d[0] * d[0];
        cov[1] += d[0] * d[1];
        cov[2] += d[0] * d[2];
        cov[3] += d[1] * d[1];
        cov[4] += d[1] * d[2];
        cov[5] += d[2] * d[2];
    }
    // (the power iteration starts from the covariance's row of the widest channel: a start of (1, 1, 1) is
    // orthogonal to the axis of two channels that move against each other, and stays put)
    let rows = [[cov[0], cov[1], cov[2]], [cov[1], cov[3], cov[4]], [cov[2], cov[4], cov[5]]];
    let widest = [0, 3, 5].iter().enumerate().max_by(|a, b| cov[*a.1].total_cmp(&cov[*b.1])).map_or(0, |(k, _)| k);
    let start = rows[widest];
    let start_len = (start[0] * start[0] + start[1] * start[1] + start[2] * start[2]).sqrt();
    let mut axis = if start_len < 1e-6 { [0.577_35, 0.577_35, 0.577_35] } else { start.map(|x| x / start_len) };
    for _ in 0..8 {
        let v = [
            cov[0] * axis[0] + cov[1] * axis[1] + cov[2] * axis[2],
            cov[1] * axis[0] + cov[3] * axis[1] + cov[4] * axis[2],
            cov[2] * axis[0] + cov[4] * axis[1] + cov[5] * axis[2],
        ];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len < 1e-6 {
            break;
        }
        axis = v.map(|x| x / len);
    }
    let dot = |c: &[f32; 3]| (c[0] - mean[0]) * axis[0] + (c[1] - mean[1]) * axis[1] + (c[2] - mean[2]) * axis[2];
    let (lo, hi) = rgb.iter().map(dot).fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
    let inset = (hi - lo) / 16.0;
    let at = |t: f32| [mean[0] + axis[0] * t, mean[1] + axis[1] * t, mean[2] + axis[2] * t];
    let (mut c0, mut c1) = (to_565(at(hi - inset)), to_565(at(lo + inset)));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    if c0 == c1 {
        return out;
    }
    let (e0, e1) = (from_565(c0), from_565(c1));
    let pal: [[i32; 3]; 4] =
        [e0, e1, std::array::from_fn(|k| (2 * e0[k] + e1[k]) / 3), std::array::from_fn(|k| (e0[k] + 2 * e1[k]) / 3)];
    let mut bits = 0u32;
    for (i, p) in b.iter().enumerate() {
        let d = |q: &[i32; 3]| (0..3).map(|k| (q[k] - i32::from(p[k])).pow(2)).sum::<i32>();
        let k = (0..4).min_by_key(|&k| d(&pal[k])).unwrap_or(0);
        bits |= (k as u32) << (2 * i);
    }
    out[4..8].copy_from_slice(&bits.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BC3 block decoded (the D3D rules), RGBA8.
    fn decode(block: &[u8]) -> [[u8; 4]; 16] {
        let (a0, a1) = (i32::from(block[0]), i32::from(block[1]));
        let mut ab = [0u8; 8];
        ab[..6].copy_from_slice(&block[2..8]);
        let abits = u64::from_le_bytes(ab);
        let alpha = |k: usize| -> u8 {
            let v = match k {
                0 => a0,
                1 => a1,
                k if a0 > a1 => ((8 - k as i32) * a0 + (k as i32 - 1) * a1) / 7,
                6 => 0,
                7 => 255,
                k => ((6 - k as i32) * a0 + (k as i32 - 1) * a1) / 5,
            };
            v as u8
        };
        let c0 = u16::from_le_bytes([block[8], block[9]]);
        let c1 = u16::from_le_bytes([block[10], block[11]]);
        let (e0, e1) = (from_565(c0), from_565(c1));
        let pal: [[i32; 3]; 4] = [
            e0,
            e1,
            std::array::from_fn(|k| (2 * e0[k] + e1[k]) / 3),
            std::array::from_fn(|k| (e0[k] + 2 * e1[k]) / 3),
        ];
        let cbits = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
        std::array::from_fn(|i| {
            let c = pal[((cbits >> (2 * i)) & 3) as usize];
            [c[0] as u8, c[1] as u8, c[2] as u8, alpha(((abits >> (3 * i)) & 7) as usize)]
        })
    }

    #[test]
    fn a_block_comes_back_close() {
        let px: Vec<[u8; 4]> = (0..16).map(|i| [(i * 16) as u8, (255 - i * 16) as u8, 128, (i * 17) as u8]).collect();
        let enc = encode_bc3(&px, 4, 4);
        assert_eq!(enc.len(), 16);
        let dec = decode(&enc);
        // (red and green move against each other: four colours over a 240 span are at most 40 off a texel, eight
        // alpha steps over 255 at most 19)
        for (p, d) in px.iter().zip(&dec) {
            for (ch, most) in [40, 40, 40, 19].into_iter().enumerate() {
                assert!((i32::from(p[ch]) - i32::from(d[ch])).abs() <= most, "{p:?} came back {d:?}");
            }
        }
        // one colour, one alpha: exact to 565
        let flat = vec![[200u8, 100, 50, 77]; 16];
        let dec = decode(&encode_bc3(&flat, 4, 4));
        assert!(dec.iter().all(|d| d[3] == 77 && (i32::from(d[0]) - 200).abs() <= 4));
    }

    #[test]
    fn sizes_and_chain() {
        assert_eq!(cooked_size(4096, 3511), (1024, 876));
        assert_eq!(cooked_size(2012, 2515), (820, 1024));
        let rgba = vec![255u8; 64 * 40 * 4];
        let mips = cook_dxt5(&rgba, 64, 40, true);
        assert_eq!(mips.len(), 7);
        assert_eq!((mips[0].width, mips[0].height), (64, 40));
        assert_eq!((mips[6].width, mips[6].height), (1, 1));
        for m in &mips {
            assert_eq!(m.bytes.len() as u32, m.width.div_ceil(4) * m.height.div_ceil(4) * 16);
        }
        // white stays white through the linear scale
        let dec = &mips[0].bytes;
        assert_eq!(&dec[8..10], &0xffffu16.to_le_bytes());
    }

    #[test]
    fn area_spans_sum_to_one() {
        for (s, d) in [(4096, 1024), (3511, 876), (2012, 820), (5, 3)] {
            for (_, w) in spans(s, d) {
                assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
            }
        }
    }
}
