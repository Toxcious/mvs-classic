//! A sky light's cube as the engine processes it at load: the capture of the source
//! cube, the filtered mips, the average brightness and the diffuse SH; the pre-integrated GF table every sky entry
//! holds. Pure f64 math through `mvs_math`'s fdlibm ports and V8's hypot (never std's transcendentals), so the
//! entries match a JavaScript importer's bytes. Every operation keeps that order and association.
//!
//! Capture sequence (4.26 UpdateSkyCaptureContents; the shaders are the build's, the global shader cache's):
//!   1. CopyCubemapToCubeFaceColorPS (0019dc76) into the scratch cube's mip 0: source level 0, lower hemisphere
//!      (z < 0) = lerp(colour, LowerHemisphereColor.rgb, LowerHemisphereColor.a) when bLowerHemisphereIsBlack.
//!   2. CreateCubeMips with DownsamplePS (0019df50): 9 taps of the previous mip (centre 0.25, eight at 0.09375).
//!   3. ComputeBrightnessPS (0019b609): the six axis texels of the last mip, mean rgb, dot 0.3333.
//!   4. FCubeFilterPS (0019d271) per mip into the second scratch cube: GGX importance sampling (32 / 64 Hammersley
//!      samples, mip from the sample PDF), cosine-weighted hemisphere above roughness 0.99.
//!
//! Every target is PF_FloatRGBA, so each stage is rounded to half floats.
// Unverified: the call sequence is 4.26 source (the exe's capture function was not read); texture filtering is done
// in doubles (the GPU uses 8-bit fixed-point weights) and a cube corner tap uses the mean of the two neighbouring
// faces' corner texels.

use std::f64::consts::PI;
use std::sync::OnceLock;

use mvs_math::hypot::hypot3;
use mvs_math::js;
use mvs_math::libm::{cos, log2, pow, sin};

use crate::kind::KindError;

/// The pre-integrated GF table's size: NoV along x, roughness along y.
pub const GF_WIDTH: usize = 128;
pub const GF_HEIGHT: usize = 32;

/// The sky light component's settings the capture reads (USkyLightComponent constructor defaults, exe 1429a5ff9..:
/// bLowerHemisphereIsBlack 1, CubemapResolution 128; LowerHemisphereColor (0, 0, 0, 1)).
#[derive(Clone, Debug, PartialEq)]
pub struct SkyLightSettings {
    pub resolution: u32,
    pub lower_hemisphere_is_black: bool,
    /// LowerHemisphereColor R, G, B, A.
    pub lower_hemisphere: [f64; 4],
    /// LightColor R, G, B (bytes), `None` when the property is absent (white).
    pub light_color: Option<[f64; 3]>,
    pub intensity: f64,
}

/// The source cube's level 0, six faces back to back (+X -X +Y -Y +Z -Z): FloatRGBA (half floats) or DXT1 blocks.
#[derive(Clone, Copy, Debug)]
pub struct SkyCubeSource<'a> {
    /// The texture kind's format name: `FloatRGBA` (half floats) or `DXT1`.
    pub format: &'a str,
    pub size: u32,
    pub srgb: bool,
    pub mip0: &'a [u8],
}

/// The processed sky (`ProcessedSky`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessedSky {
    /// The filtered cube's mip 0 size and mip count.
    pub size: u32,
    pub mips: u32,
    /// RGBA16F texels, mip by mip (largest first), each mip's six faces back to back, alpha 1.
    pub cube: Vec<u16>,
    pub average_brightness: f64,
    /// The light's colour x intensity (LightColor through FLinearColor(FColor)).
    pub color: Vec<f64>,
    /// Its diffuse SH: three.js basis in three axes x = UE X, y = UE Z, z = UE Y.
    pub sh: Vec<Vec<f64>>,
}

/// A half float's value (`fromHalf`).
pub fn from_half(h: u16) -> f64 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 31);
    let f = f64::from(h & 1023);
    if e == 0 {
        s * f * pow(2.0, -24.0)
    } else if e == 31 {
        if f != 0.0 { f64::NAN } else { s * f64::INFINITY }
    } else {
        s * pow(2.0, f64::from(e - 15)) * (1.0 + f / 1024.0)
    }
}

/// The half float nearest `v` (`toHalf`), ties to even.
pub fn to_half(v: f64) -> u16 {
    if v.is_nan() {
        return 0x7e00;
    }
    let s: u32 = if v < 0.0 || (v == 0.0 && v.is_sign_negative()) { 0x8000 } else { 0 };
    let a = v.abs();
    if a >= 65520.0 {
        return (s | 0x7c00) as u16;
    }
    if a < pow(2.0, -25.0) {
        return s as u16;
    }
    let mut e = log2(a).floor();
    if pow(2.0, e) > a {
        e -= 1.0;
    } else if pow(2.0, e + 1.0) <= a {
        e += 1.0;
    }
    let up = |r: f64, d: f64| if d > 0.5 || (d == 0.5 && (r as i64) & 1 != 0) { 1.0 } else { 0.0 };
    if e < -14.0 {
        let m = a / pow(2.0, -24.0);
        let r = m.floor();
        let d = m - r;
        return (f64::from(s) + (r + up(r, d))) as u16;
    }
    let m = (a / pow(2.0, e) - 1.0) * 1024.0;
    let r = m.floor();
    let d = m - r;
    let q = (((e + 15.0) as i32) << 10) as f64 + r + up(r, d);
    (s | (q as u32)) as u16
}

type V3 = [f64; 3];

// Cube helpers (D3D face layout; directions in UE world axes).
fn face_dir(f: usize, sc: f64, tc: f64) -> V3 {
    match f {
        0 => [1.0, -tc, -sc],
        1 => [-1.0, -tc, sc],
        2 => [sc, 1.0, tc],
        3 => [sc, -1.0, -tc],
        4 => [sc, -tc, 1.0],
        _ => [-sc, -tc, -1.0],
    }
}

const FACE_NORMAL: [V3; 6] =
    [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];

/// The face a direction hits and its (s, t) there.
fn dir_face(x: f64, y: f64, z: f64) -> (usize, f64, f64) {
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    if ax >= ay && ax >= az {
        return if x > 0.0 { (0, -z / ax, -y / ax) } else { (1, z / ax, -y / ax) };
    }
    if ay >= az {
        return if y > 0.0 { (2, x / ay, z / ay) } else { (3, x / ay, -z / ay) };
    }
    if z > 0.0 { (4, x / az, -y / az) } else { (5, -x / az, -y / az) }
}

/// A cube's mip: `6 * n * n` rgb texels as doubles.
struct Mip {
    n: usize,
    d: Vec<f64>,
}

fn new_mip(n: usize) -> Mip {
    Mip { n, d: vec![0.0; 6 * n * n * 3] }
}

fn quantise(mut m: Mip) -> Mip {
    for v in &mut m.d {
        *v = from_half(to_half(*v));
    }
    m
}

/// `Math.min(n - 1, Math.max(0, v))` as an index.
fn clamp_i(v: f64, n: usize) -> usize {
    js::min((n - 1) as f64, js::max(0.0, v)) as usize
}

/// The index of texel (i, j) of face f; one step outside reads the neighbouring face (seamless filtering).
fn texel_index(m: &Mip, f: usize, i: i64, j: i64) -> usize {
    let n = m.n;
    let ni = n as i64;
    let (oi, oj) = (i < 0 || i >= ni, j < 0 || j >= ni);
    if !oi && !oj {
        return 3 * ((f * n + j as usize) * n + i as usize);
    }
    let nf = n as f64;
    let mut sc = (2.0 * (i as f64 + 0.5)) / nf - 1.0;
    let mut tc = (2.0 * (j as f64 + 0.5)) / nf - 1.0;
    if oi {
        sc = js::sign(sc);
    }
    if oj {
        tc = js::sign(tc);
    }
    let p = face_dir(f, sc, tc);
    let nr = FACE_NORMAL[f];
    let (g, s2, t2) = dir_face(p[0] - nr[0] / nf, p[1] - nr[1] / nf, p[2] - nr[2] / nf);
    let i2 = clamp_i((((s2 + 1.0) / 2.0) * nf).floor(), n);
    let j2 = clamp_i((((t2 + 1.0) / 2.0) * nf).floor(), n);
    3 * ((g * n + j2) * n + i2)
}

fn add_texel(out: &mut V3, m: &Mip, f: usize, i: i64, j: i64, w: f64) {
    let ni = m.n as i64;
    if (i < 0 || i >= ni) && (j < 0 || j >= ni) {
        // Corner: the mean of the two neighbouring faces' corner texels.
        let cj = clamp_i(j as f64, m.n) as i64;
        add_texel(out, m, f, i, cj, w / 2.0);
        let ci = clamp_i(i as f64, m.n) as i64;
        add_texel(out, m, f, ci, j, w / 2.0);
        return;
    }
    let o = texel_index(m, f, i, j);
    out[0] += m.d[o] * w;
    out[1] += m.d[o + 1] * w;
    out[2] += m.d[o + 2] * w;
}

fn bilinear(out: &mut V3, m: &Mip, x: f64, y: f64, z: f64, w: f64) {
    let (f, sc, tc) = dir_face(x, y, z);
    let n = m.n as f64;
    let u = ((sc + 1.0) / 2.0) * n - 0.5;
    let v = ((tc + 1.0) / 2.0) * n - 0.5;
    let (fi, fj) = (u.floor(), v.floor());
    let (fu, fv) = (u - fi, v - fj);
    if m.n == 1 {
        add_texel(out, m, f, 0, 0, w);
        return;
    }
    let (i, j) = (fi as i64, fj as i64);
    add_texel(out, m, f, i, j, w * (1.0 - fu) * (1.0 - fv));
    add_texel(out, m, f, i + 1, j, w * fu * (1.0 - fv));
    add_texel(out, m, f, i, j + 1, w * (1.0 - fu) * fv);
    add_texel(out, m, f, i + 1, j + 1, w * fu * fv);
}

fn trilinear(out: &mut V3, cube: &[Mip], x: f64, y: f64, z: f64, level: f64, w: f64) {
    let l = js::min((cube.len() - 1) as f64, js::max(0.0, level));
    let l0 = l.floor();
    let fl = l - l0;
    let l0i = l0 as usize;
    bilinear(out, &cube[l0i], x, y, z, w * (1.0 - fl));
    if fl > 0.0 {
        bilinear(out, &cube[l0i + 1], x, y, z, w * fl);
    }
}

/// Fills a mip texel by texel from `fn(face, s, t, out)`, then rounds it to half floats.
fn each_texel(mut m: Mip, mut f: impl FnMut(usize, f64, f64, &mut V3)) -> Mip {
    let n = m.n;
    let nf = n as f64;
    for face in 0..6 {
        for j in 0..n {
            for i in 0..n {
                let mut tmp = [0.0; 3];
                f(face, (2.0 * (i as f64 + 0.5)) / nf - 1.0, (2.0 * (j as f64 + 0.5)) / nf - 1.0, &mut tmp);
                let o = 3 * ((face * n + j) * n + i);
                m.d[o..o + 3].copy_from_slice(&tmp);
            }
        }
    }
    quantise(m)
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: V3) -> V3 {
    let l = hypot3(a[0], a[1], a[2]);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn srgb_byte(v: f64) -> f64 {
    if v / 255.0 <= 0.04045 { v / 255.0 / 12.92 } else { pow((v / 255.0 + 0.055) / 1.055, 2.4) }
}

/// The Hammersley sequence's radical inverse (32-bit reverse).
fn bitrev(i: u32) -> f64 {
    f64::from(i.reverse_bits())
}

/// The source cube's level 0 as rgb doubles. A DXT1 cube (an LDR cube: sRGB decoded by the sampler; SRGB is UTexture's
/// default and the cooked asset does not override it) is decoded block by block.
fn source_mip(src: &SkyCubeSource<'_>) -> Result<Mip, KindError> {
    let n = src.size as usize;
    let mut m = new_mip(n);
    let b = src.mip0;
    if src.format == "DXT1" {
        if !src.srgb {
            return Err(KindError::Failed("sky: a linear BC1 sky cube".into()));
        }
        if b.len() != (6 * n * n) / 2 {
            return Err(KindError::Failed(format!("sky: a {n} px BC1 cube's level 0 is {} bytes", b.len())));
        }
        let lin: Vec<f64> = (0..256).map(|k| srgb_byte(f64::from(k))).collect();
        let c565 = |c: u16| -> V3 {
            [
                f64::from((c >> 11) & 31) * 255.0 / 31.0,
                f64::from((c >> 5) & 63) * 255.0 / 63.0,
                f64::from(c & 31) * 255.0 / 31.0,
            ]
        };
        let bn = n / 4;
        for f in 0..6 {
            for by in 0..bn {
                for bx in 0..bn {
                    let o = 8 * ((f * bn + by) * bn + bx);
                    let v0 = u16::from_le_bytes([b[o], b[o + 1]]);
                    let v1 = u16::from_le_bytes([b[o + 2], b[o + 3]]);
                    let bits = u32::from_le_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]);
                    let (a, c) = (c565(v0), c565(v1));
                    let pal: [V3; 4] = if v0 > v1 {
                        [
                            a,
                            c,
                            [(2.0 * a[0] + c[0]) / 3.0, (2.0 * a[1] + c[1]) / 3.0, (2.0 * a[2] + c[2]) / 3.0],
                            [(a[0] + 2.0 * c[0]) / 3.0, (a[1] + 2.0 * c[1]) / 3.0, (a[2] + 2.0 * c[2]) / 3.0],
                        ]
                    } else {
                        [a, c, [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0, (a[2] + c[2]) / 2.0], [0.0; 3]]
                    };
                    for k in 0..16 {
                        let col = pal[((bits >> (2 * k)) & 3) as usize];
                        let i = 3 * ((f * n + by * 4 + (k >> 2)) * n + bx * 4 + (k & 3));
                        // the decoded 8-bit value through the sRGB table (Unverified: the GPU's BC1 interpolation precision)
                        for ch in 0..3 {
                            m.d[i + ch] = lin[js::round(col[ch]) as usize];
                        }
                    }
                }
            }
        }
    } else {
        if b.len() != 6 * n * n * 8 {
            return Err(KindError::Failed(format!("sky: a {n} px FloatRGBA cube's level 0 is {} bytes", b.len())));
        }
        for k in 0..6 * n * n {
            for c in 0..3 {
                let o = 8 * k + 2 * c;
                m.d[3 * k + c] = from_half(u16::from_le_bytes([b[o], b[o + 1]]));
            }
        }
    }
    Ok(m)
}

/// The capture and filter passes over the source cube, and the light's colour and SH (`processSky`).
pub fn process_sky(light: &SkyLightSettings, source: &SkyCubeSource<'_>) -> Result<ProcessedSky, KindError> {
    let res = light.resolution as usize;
    let num_mips_f = log2(f64::from(light.resolution)) + 1.0;
    if num_mips_f.fract() != 0.0 || !num_mips_f.is_finite() {
        return Err(KindError::Failed(format!("sky: CubemapResolution {res} is not a power of two")));
    }
    let num_mips = num_mips_f as usize;
    let lower_black = light.lower_hemisphere_is_black;
    let lhc = light.lower_hemisphere;
    let lower: V3 = [lhc[0], lhc[1], lhc[2]];
    let lhc_a = lhc[3];
    let src = source_mip(source)?;
    let src_n = src.n;

    // 1. Copy (0019dc76). A source cube of another size is read with the bilinear sampler at level 0.
    let mut scratch: Vec<Mip> = vec![each_texel(new_mip(res), |f, sc, tc, out| {
        let d = face_dir(f, sc, tc);
        bilinear(out, &src, d[0], d[1], d[2], 1.0);
        if d[2] < 0.0 && lower_black {
            for c in 0..3 {
                out[c] += lhc_a * (lower[c] - out[c]);
            }
        }
    })];

    // 2. Mip chain (0019df50).
    const TAPS: [[f64; 2]; 8] =
        [[-0.7, -0.7], [0.7, -0.7], [-0.7, 0.7], [0.7, 0.7], [0.0, -1.0], [-1.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    for mip in 1..num_mips {
        let size = 1usize << (num_mips - mip - 1);
        let off = 4.0 / size as f64;
        let prev = &scratch[mip - 1];
        let next = each_texel(new_mip(size), |f, sc, tc, out| {
            let c = face_dir(f, sc, tc);
            let tz = norm(c);
            let tx = norm(cross(face_dir(f, sc, tc + 1.0), tz));
            let ty = cross(tz, tx);
            bilinear(out, prev, c[0], c[1], c[2], 0.25);
            for [a, b] in TAPS {
                bilinear(
                    out,
                    prev,
                    c[0] + (a * tx[0] + b * ty[0]) * off,
                    c[1] + (a * tx[1] + b * ty[1]) * off,
                    c[2] + (a * tx[2] + b * ty[2]) * off,
                    0.375 * 0.25,
                );
            }
        });
        scratch.push(next);
    }

    // 3. Average brightness (0019b609), read back from a half-float target.
    let last = &scratch[num_mips - 1];
    let mut avg = [0.0; 3];
    for f in 0..6 {
        for (c, a) in avg.iter_mut().enumerate() {
            *a += last.d[3 * f + c];
        }
    }
    let avg = avg.map(|v| v * 0.166667);
    let average_brightness = from_half(to_half((avg[0] + avg[1] + avg[2]) * 0.3333));

    // 4. Filter (0019d271).
    let solid_angle_texel = ((4.0 * PI) / (6.0 * res as f64 * res as f64)) * 2.0;
    let mut filtered: Vec<Mip> = Vec::new();
    for mip in 0..num_mips {
        let roughness = pow(2.0, (1.0 - ((num_mips as f64 - 2.0) - mip as f64)) / 1.2);
        let samples: u32 = if roughness < 0.1 { 32 } else { 64 };
        let m2 = pow(roughness, 4.0);
        // The mip's samples in tangent space, made once (each depends on the sample and the mip alone): direction, mip
        // level and weight, five numbers each. Above roughness 0.99 the 64 cosine-weighted samples; else the GGX samples
        // whose L lies above the surface (the others add nothing).
        let mut taps: Vec<f64> = Vec::new();
        if roughness > 0.99 {
            for i in 0..64u32 {
                let phi = 2.0 * PI * ((f64::from(i) * 0.015625) % 1.0);
                let ey = bitrev(i) * 2.3283064365386963e-10;
                let z = ey.sqrt();
                let st = (1.0 - z).sqrt(); // as compiled: sqrt(1 - cos), not sqrt(1 - cos^2)
                let level = 0.5 * log2(1.0 / (z * 20.371832) / solid_angle_texel);
                taps.extend([st * cos(phi), st * sin(phi), z, level, 0.015625]);
            }
        } else if roughness >= 0.01 {
            for i in 0..samples {
                let phi = 2.0 * PI * ((f64::from(i) / f64::from(samples)) % 1.0);
                let ey = bitrev(i) * 2.3283064365386963e-10;
                let ct2 = (1.0 - ey) / (1.0 + (m2 - 1.0) * ey);
                let ct = ct2.sqrt();
                let st = (1.0 - ct2).sqrt();
                let h: V3 = [st * cos(phi), st * sin(phi), ct];
                let l: V3 = [2.0 * ct * h[0], 2.0 * ct * h[1], 2.0 * ct * h[2] - 1.0];
                if l[2] > 0.0 {
                    let d = (ct * m2 - ct) * ct + 1.0;
                    let pdf = (m2 / (PI * d * d)) * 0.25;
                    taps.extend([
                        l[0],
                        l[1],
                        l[2],
                        0.5 * log2(1.0 / (f64::from(samples) * pdf) / solid_angle_texel),
                        l[2],
                    ]);
                }
            }
        }
        let scratch = &scratch;
        filtered.push(each_texel(new_mip(1usize << (num_mips - 1 - mip)), |f, sc, tc, out| {
            let c = face_dir(f, sc, tc);
            if roughness < 0.01 {
                bilinear(out, &scratch[0], c[0], c[1], c[2], 1.0);
                return;
            }
            let n = norm(c);
            // GetTangentBasis.
            let sg = if n[2] >= 0.0 { 1.0 } else { -1.0 };
            let a = -1.0 / (sg + n[2]);
            let b = n[0] * n[1] * a;
            let x: V3 = [1.0 + sg * a * n[0] * n[0], sg * b, -sg * n[0]];
            let y: V3 = [b, sg + a * n[1] * n[1], -n[1]];
            let world = |l0: f64, l1: f64, l2: f64| -> V3 {
                [
                    l0 * x[0] + l1 * y[0] + l2 * n[0],
                    l0 * x[1] + l1 * y[1] + l2 * n[1],
                    l0 * x[2] + l1 * y[2] + l2 * n[2],
                ]
            };
            if roughness > 0.99 {
                for t in taps.as_chunks::<5>().0.iter() {
                    let w = world(t[0], t[1], t[2]);
                    trilinear(out, scratch, w[0], w[1], w[2], t[3], t[4]);
                }
                return;
            }
            let mut weight = 0.0;
            let mut acc = [0.0; 3];
            for t in taps.as_chunks::<5>().0.iter() {
                let w = world(t[0], t[1], t[2]);
                trilinear(&mut acc, scratch, w[0], w[1], w[2], t[3], t[4]);
                weight += t[4];
            }
            for k in 0..3 {
                out[k] = acc[k] / weight;
            }
        }));
    }

    // The cube's texels as RGBA16F.
    let texels: usize = filtered.iter().map(|m| 6 * m.n * m.n).sum();
    let mut cube = Vec::with_capacity(texels * 4);
    for m in &filtered {
        for k in 0..6 * m.n * m.n {
            for c in 0..3 {
                cube.push(to_half(m.d[3 * k + c]));
            }
            cube.push(0x3c00);
        }
    }

    // The diffuse SH: the source cube's mip 0 with the lower hemisphere rule, times the light's colour and intensity.
    // Unverified: LightColor is taken through FLinearColor(FColor) (sRGB decode, 4.26 source).
    let color: Vec<f64> = match light.light_color {
        Some(c) => c.iter().map(|&v| srgb_byte(v) * light.intensity).collect(),
        None => vec![light.intensity; 3],
    };
    let mut sh = vec![vec![0.0; 3]; 9];
    let sn = src_n as f64;
    for f in 0..6 {
        for j in 0..src_n {
            for i in 0..src_n {
                let sc = (2.0 * (i as f64 + 0.5)) / sn - 1.0;
                let tc = (2.0 * (j as f64 + 0.5)) / sn - 1.0;
                let d = norm(face_dir(f, sc, tc));
                let w = 4.0 / (sn * sn * pow(1.0 + sc * sc + tc * tc, 1.5));
                let s = 3 * ((f * src_n + j) * src_n + i);
                let mut c = [src.d[s], src.d[s + 1], src.d[s + 2]];
                if d[2] < 0.0 && lower_black {
                    for k in 0..3 {
                        c[k] += lhc_a * (lower[k] - c[k]);
                    }
                }
                let (x, y, z) = (d[0], d[2], d[1]);
                let basis = [
                    0.282095,
                    0.488603 * y,
                    0.488603 * z,
                    0.488603 * x,
                    1.092548 * x * y,
                    1.092548 * y * z,
                    0.315392 * (3.0 * z * z - 1.0),
                    1.092548 * x * z,
                    0.546274 * (x * x - y * y),
                ];
                for (k, bs) in basis.iter().enumerate() {
                    for ch in 0..3 {
                        sh[k][ch] += c[ch] * color[ch] * bs * w;
                    }
                }
            }
        }
    }
    Ok(ProcessedSky {
        size: light.resolution,
        mips: u32::try_from(num_mips).expect("a mip count"),
        cube,
        average_brightness,
        color,
        sh,
    })
}

/// The pre-integrated GF table, GF_WIDTH x GF_HEIGHT half-float pairs (`preIntegratedGF`): PreIntegratedGF (exe
/// 141d8bee4..): 128 x 32 PF_G16R16, 128 Hammersley samples of GGX with the Smith joint approximation; R = scale,
/// G = bias. One table for every sky, made at its first use and kept.
pub fn pre_integrated_gf() -> Vec<u16> {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(make_gf).clone()
}

fn make_gf() -> Vec<u16> {
    let fr = js::fround;
    let mut gf = vec![0u16; GF_WIDTH * GF_HEIGHT * 2];
    for y in 0..GF_HEIGHT {
        let rough = fr((y as f64 + 0.5) / GF_HEIGHT as f64);
        let m = fr(rough * rough);
        let mm = fr(m * m);
        for x in 0..GF_WIDTH {
            let nov = fr((x as f64 + 0.5) / GF_WIDTH as f64);
            let vx = fr((1.0 - nov * nov).sqrt());
            let (mut a, mut b) = (0.0, 0.0);
            for i in 0..128u32 {
                let e2 = fr(bitrev(i) * 2.3283064365386963e-10);
                let phi = fr(f64::from(i) * 0.04908738657832146);
                let ct2 = fr((1.0 - e2) / ((mm - 1.0) * e2 + 1.0));
                let ct = fr(ct2.sqrt());
                let st = fr((1.0 - ct2).sqrt());
                let voh = fr(cos(phi) * st * vx + ct * nov);
                let nol = js::max(fr(ct * 2.0 * voh - nov), 0.0);
                let noh = js::max(ct, 0.0);
                let vohc = js::max(voh, 0.0);
                if nol > 0.0 {
                    let vis = fr(0.5 / ((nol * (1.0 - m) + m) * nov + (nov * (1.0 - m) + m) * nol));
                    let t = fr(vis * nol * (4.0 / noh) * vohc);
                    let fc = fr(pow(1.0 - vohc, 5.0));
                    b = fr(b + fc * t);
                    a = fr(a + (1.0 - fc) * t);
                }
            }
            let q = |v: f64| (js::min(1.0, js::max(0.0, fr(v * 0.0078125))) * 65535.0 + 0.5).trunc() as u16;
            gf[2 * (y * GF_WIDTH + x)] = q(a);
            gf[2 * (y * GF_WIDTH + x) + 1] = q(b);
        }
    }
    gf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves() {
        for h in [0u16, 1, 0x3c00, 0x7bff, 0x8000, 0xc000, 0x0400, 0x03ff] {
            assert_eq!(to_half(from_half(h)), h);
        }
        assert_eq!(to_half(1.0), 0x3c00);
        assert_eq!(to_half(65520.0), 0x7c00);
        assert_eq!(to_half(f64::NAN), 0x7e00);
        assert_eq!(to_half(-0.0), 0x8000);
        // a tie goes to the even mantissa
        assert_eq!(to_half(1.0 + 1.0 / 2048.0), 0x3c00);
        assert_eq!(to_half(1.0 + 3.0 / 2048.0), 0x3c02);
    }

    #[test]
    fn faces_round_trip() {
        for f in 0..6 {
            let d = face_dir(f, 0.25, -0.5);
            let (g, s, t) = dir_face(d[0], d[1], d[2]);
            assert_eq!((g, s, t), (f, 0.25, -0.5));
        }
    }
}
