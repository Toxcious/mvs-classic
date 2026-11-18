//! PNG out of a read-back picture, and the per-channel difference of two pictures (half floats to 8 bits, PNG in
//! and out).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub fn f16_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) as u32) << 31;
    let e = ((h >> 10) & 31) as u32;
    let m = (h & 1023) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s
        } else {
            // subnormal: m x 2^-24
            return f32::from_bits(s) + if s != 0 { -(m as f32) } else { m as f32 } * (1.0 / 16_777_216.0);
        }
    } else if e == 31 {
        s | 0x7f80_0000 | (m << 13)
    } else {
        s | ((e + 112) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

/// RGBA16F rows as RGBA8 by the trial's picture map: round(255 x clamp(c, 0, 1) ^ (1 / 2.2)), alpha 255.
pub fn half_to_gamma(px: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() / 2);
    for t in px.as_chunks::<8>().0 {
        for c in 0..3 {
            let v = f16_to_f32(u16::from_le_bytes([t[2 * c], t[2 * c + 1]]));
            out.push((255.0 * v.clamp(0.0, 1.0).powf(1.0 / 2.2)).round() as u8);
        }
        out.push(255);
    }
    out
}

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())
}

/// A PNG as RGBA8 rows.
pub fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().ok_or("png size")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        c => return Err(format!("{}: colour type {c:?}", path.display())),
    };
    Ok((w, h, rgba))
}

pub struct Diff {
    /// Mean of |a - b| over every pixel's R, G, B (0..255).
    pub mean: f64,
    /// Share of pixels whose largest channel difference is over 8 and over 32.
    pub over8: f64,
    pub over32: f64,
    /// Mean signed difference per channel (a - b).
    pub bias: [f64; 3],
    /// Per region of a 4 x 4 grid (row by row): the mean difference.
    pub grid: Vec<f64>,
    /// The difference picture: |a - b| x 4 per channel.
    pub picture: Vec<u8>,
}

pub fn diff(a: &[u8], b: &[u8], w: u32, h: u32) -> Diff {
    let (w, h) = (w as usize, h as usize);
    let mut sum = 0.0;
    let mut bias = [0.0; 3];
    let (mut n8, mut n32) = (0usize, 0usize);
    let mut grid = vec![(0.0, 0usize); 16];
    let mut picture = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = 4 * (y * w + x);
            let mut big = 0;
            let mut px = 0.0;
            for c in 0..3 {
                let d = a[i + c] as i32 - b[i + c] as i32;
                big = big.max(d.abs());
                px += d.abs() as f64;
                bias[c] += d as f64;
                picture[i + c] = (d.abs() * 4).min(255) as u8;
            }
            picture[i + 3] = 255;
            sum += px;
            n8 += (big > 8) as usize;
            n32 += (big > 32) as usize;
            let g = &mut grid[(y * 4 / h) * 4 + x * 4 / w];
            g.0 += px / 3.0;
            g.1 += 1;
        }
    }
    let n = (w * h) as f64;
    Diff {
        mean: sum / (3.0 * n),
        over8: n8 as f64 / n,
        over32: n32 as f64 / n,
        bias: bias.map(|s| s / n),
        grid: grid.iter().map(|(s, k)| s / *k as f64).collect(),
        picture,
    }
}
