//! The animation kind's runtime value and its sampler: one MVA3 entry, its
//! cooked keys decoded when sampled as UAnimSequence::GetBonePose does, into a pose in the build's bone-local space by
//! skeleton bone index. The block formats, the key lookup and the root lock follow the build's compressed layout.

use crate::curve::eval_curve;
use crate::format::{ANIM_FORMAT_VERSION, ANIM_MAGIC, AnimCurveKey, AnimHeader, anim_track_table, read_json_bin};
use crate::pose::BonePose;
use indexmap::IndexMap;
use mvs_math::js;

const NO_SCALE_STREAM: i32 = -2;
const FMT_FLOAT96: u32 = 1;
const FMT_FIXED48: u32 = 2;
const FMT_INTERVAL32: u32 = 3;

// ERootMotionRootLock.
pub const ROOT_LOCK_REF_POSE: f64 = 0.0;
pub const ROOT_LOCK_FIRST_FRAME: f64 = 1.0;
pub const ROOT_LOCK_ZERO: f64 = 2.0;

/// A Step clip's position counts as on a key from this far (in frames) before it.
pub const STEP_EPS: f64 = 1e-5;

/// `framePosition`: the frame position of `time` in a sequence of `frames` frames and `length` seconds.
pub fn frame_position(time: f64, length: f64, frames: f64, step: bool) -> f64 {
    if frames <= 1.0 || !(length > 0.0) {
        return 0.0;
    }
    let t = time / length;
    let p = (if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        t
    }) * (frames - 1.0);
    if step { js::min(p + STEP_EPS, frames - 1.0) } else { p }
}

// track rows of AnimClip.info: bone, then per slot (rotation, translation, scale) its block's byte offset and its
// frame table's element index
const INFO: usize = 7;
const I_ROT: usize = 1;
const I_TRANS: usize = 3;
const I_SCALE: usize = 5;

// Stored components by flags & 7.
const NC: [usize; 8] = [0, 1, 1, 2, 1, 2, 2, 3];

const fn align4(n: i64) -> i64 {
    (n + 3) & !3
}
const F_INV16: f64 = (1.0f64 / 32767.0) as f32 as f64;
const F_INV511: f64 = (1.0f64 / 511.0) as f32 as f64;
const F_INV1023: f64 = (1.0f64 / 1023.0) as f32 as f64;

/// The key span being read and the decoded key (kept as fields: k0, k1, ka, kx..kw).
#[derive(Default, Clone, Copy)]
struct Key {
    k0: usize,
    k1: usize,
    ka: f64,
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

pub struct AnimClip<'a> {
    pub path: String,
    pub skeleton: String,
    pub frames: f64,
    pub length: f64,
    pub rate_scale: f64,
    pub step: bool,
    pub root_motion: bool,
    pub force_root_lock: bool,
    pub root_lock: f64,
    pub normalized_root_motion_scale: bool,
    pub additive_type: f64,
    pub tracks: usize,
    pub bone_end: usize,
    pub byte_length: usize,
    pub still: bool,
    pub info: Vec<i32>,
    pub root_track: i32,
    // the data section, read as u32 / f32 words (word) and as the u8 / u16 frame tables
    bytes: &'a [u8],
    wide: bool,
    curve_names: Vec<String>,
    curves: Vec<Vec<AnimCurveKey>>,
    curve_index: IndexMap<String, usize>,
    fpos: f64,
}

impl<'a> AnimClip<'a> {
    pub fn new(entry: &'a [u8], path: &str) -> Result<AnimClip<'a>, String> {
        let (json, body) = read_json_bin(ANIM_MAGIC, entry)?;
        let mut h: AnimHeader = serde_json::from_slice(json).map_err(|e| format!("{path}: animation header: {e}"))?;
        if h.version != ANIM_FORMAT_VERSION {
            return Err(format!("{path}: animation entry v{}, the sampler reads v{ANIM_FORMAT_VERSION}", h.version));
        }
        // a Map from the curves: a name given twice keeps its first place and its last keys
        let mut curve_names: Vec<String> = Vec::new();
        let mut curves: Vec<Vec<AnimCurveKey>> = Vec::new();
        let mut curve_index = IndexMap::new();
        for c in std::mem::take(&mut h.curves) {
            match curve_index.get(&c.name) {
                Some(&i) => curves[i] = c.keys,
                None => {
                    curve_index.insert(c.name.clone(), curves.len());
                    curve_names.push(c.name.clone());
                    curves.push(c.keys);
                }
            }
        }
        let (offset, length) = (h.data.offset, h.data.length);
        if offset + length > body.len() {
            return Err(format!("{path}: data section past the body"));
        }
        let d = &body[offset..offset + length];
        let table = anim_track_table(&h, body)?;
        let n = table.len() / 4;
        let mut clip = AnimClip {
            path: path.to_string(),
            skeleton: h.skeleton.clone(),
            frames: h.frames,
            length: h.length,
            rate_scale: h.rate_scale,
            step: h.interpolation == 1.0,
            root_motion: h.root_motion,
            force_root_lock: h.force_root_lock,
            root_lock: h.root_lock,
            normalized_root_motion_scale: h.normalized_root_motion_scale,
            additive_type: h.additive.as_ref().map_or(0.0, |a| a.kind),
            tracks: n,
            bone_end: 0,
            byte_length: entry.len(),
            still: true,
            info: vec![0; n * INFO],
            root_track: -1,
            bytes: d,
            wide: h.frames > 255.0,
            curve_names,
            curves,
            curve_index,
            fpos: 0.0,
        };
        let mut bone_end = 0usize;
        let mut root_track = -1;
        for i in 0..n {
            let bone = table[i * 4];
            if bone < 0 {
                return Err(format!("{path}: track {i} has bone {bone}"));
            }
            bone_end = bone_end.max(bone as usize + 1);
            if bone == 0 {
                root_track = i as i32;
            }
            let o = i * INFO;
            clip.info[o] = bone;
            clip.block(table[i * 4 + 1], o, I_ROT, i)?;
            clip.block(table[i * 4 + 2], o, I_TRANS, i)?;
            clip.block(table[i * 4 + 3], o, I_SCALE, i)?;
        }
        clip.bone_end = bone_end;
        clip.root_track = root_track;
        let mut still = true;
        let mut i = 0;
        while i < n * INFO && still {
            for part in [I_ROT, I_TRANS, I_SCALE] {
                let at = clip.info[i + part];
                if at >= 0 && (clip.word(at as usize >> 2) & 0xffffff) > 1 {
                    still = false;
                }
            }
            i += INFO;
        }
        clip.still = still;
        Ok(clip)
    }

    /// `trackBone`.
    pub fn track_bone(&self, i: usize) -> i32 {
        self.info[i * INFO]
    }

    /// `trackHasScale`.
    pub fn track_has_scale(&self, i: usize) -> bool {
        self.info[i * INFO + I_SCALE] != NO_SCALE_STREAM
    }

    fn block(&mut self, at: i32, row: usize, part: usize, track: usize) -> Result<(), String> {
        let p = match part {
            I_ROT => "rotation",
            I_TRANS => "translation",
            _ => "scale",
        };
        // (the label of an error: built only for one)
        let what = |path: &str| format!("{path} track {track} {p}");
        let rotation = part == I_ROT;
        let slot = row + part;
        self.info[slot] = at;
        self.info[slot + 1] = -1;
        if at == -1 || (at == NO_SCALE_STREAM && part == I_SCALE) {
            return Ok(());
        }
        let size = self.bytes.len() as i64;
        if at < 0 || at & 3 != 0 || at as i64 + 4 > size {
            return Err(format!("{}: block at {at} outside the {size}-byte data or not 4-aligned", what(&self.path)));
        }
        let h = self.word(at as usize >> 2);
        let fmt = h >> 28;
        let flags = (h >> 24) & 15;
        let keys = (h & 0xffffff) as i64;
        let nc: i64 = if fmt == FMT_FLOAT96 && (flags & 7) == 0 {
            3
        } else {
            ((flags & 1) + ((flags >> 1) & 1) + ((flags >> 2) & 1)) as i64
        };
        let len = if fmt == FMT_FLOAT96 {
            4 * nc * keys
        } else if fmt == FMT_FIXED48 {
            2 * nc * keys
        } else if fmt == FMT_INTERVAL32 && !rotation {
            8 * nc + 4 * keys
        } else {
            return Err(format!(
                "{}: format {fmt} has no decoder here (1 Float96NoW, 2 Fixed48NoW, 3 IntervalFixed32NoW for a vector)",
                what(&self.path)
            ));
        };
        if keys < 1 {
            return Err(format!("{}: no key", what(&self.path)));
        }
        let mut end = at as i64 + align4(4 + len);
        if flags & 8 != 0 {
            self.info[slot + 1] = (if self.wide { end >> 1 } else { end }) as i32;
            end = align4(end + keys * if self.wide { 2 } else { 1 });
        }
        if end > size {
            return Err(format!("{}: block at {at} runs past the {size}-byte data", what(&self.path)));
        }
        Ok(())
    }

    /// `curve`: the value of curve `name` at `time` (s), None when the sequence has no such curve.
    pub fn curve(&self, name: &str, time: f64) -> Option<f64> {
        self.curve_index.get(name).map(|&i| eval_curve(&self.curves[i], time))
    }

    /// `curveNames`: the curves' names, in the entry's order.
    pub fn curve_names(&self) -> &[String] {
        &self.curve_names
    }

    /// `sample`: the pose at `time` (s of the sequence) into `out`, by skeleton bone index.
    pub fn sample(
        &mut self,
        time: f64,
        ref_: &BonePose,
        out: &mut BonePose,
        extract_root_motion: bool,
        unlocked: bool,
    ) {
        assert!(
            out.n >= self.bone_end && ref_.n == out.n,
            "{}: sampled into a pose of {} bones (ref {}; tracks reach bone {})",
            self.path,
            out.n,
            ref_.n,
            self.bone_end as i64 - 1
        );
        let additive = self.additive_type != 0.0;
        if additive {
            out.additive_identity();
        } else {
            out.copy(ref_);
        }
        self.fpos = frame_position(time, self.length, self.frames, self.step);
        let lock = !unlocked && (self.force_root_lock || (extract_root_motion && self.root_motion));
        let mut o = 0;
        for i in 0..self.tracks {
            if !(lock && i as i32 == self.root_track) {
                self.track(o, self.info[o] as usize, out);
            }
            o += INFO;
        }
        if lock {
            self.lock_root(ref_, out);
        }
    }

    fn track(&self, o: usize, bone: usize, out: &mut BonePose) {
        let info = &self.info;
        self.rotation(info[o + I_ROT], info[o + I_ROT + 1], &mut out.r, bone * 4);
        self.vector(info[o + I_TRANS], info[o + I_TRANS + 1], &mut out.t, bone * 3);
        let s = info[o + I_SCALE];
        if s != NO_SCALE_STREAM {
            self.vector(s, info[o + I_SCALE + 1], &mut out.s, bone * 3);
        }
    }

    fn lock_root(&mut self, ref_: &BonePose, out: &mut BonePose) {
        let lock = self.root_lock;
        if lock == ROOT_LOCK_FIRST_FRAME {
            // the root track at time 0 (none: the root keeps its start value)
            if self.root_track >= 0 {
                let fpos = self.fpos;
                self.fpos = 0.0;
                self.track(self.root_track as usize * INFO, 0, out);
                self.fpos = fpos;
            }
            return;
        }
        if lock == ROOT_LOCK_ZERO {
            out.t[..3].fill(0.0);
            out.r[..4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            out.s[..3].fill(1.0);
        } else if lock == ROOT_LOCK_REF_POSE {
            out.t[..3].copy_from_slice(&ref_.t[..3]);
            out.s[..3].copy_from_slice(&ref_.s[..3]);
            out.r[..4].copy_from_slice(&ref_.r[..4]);
        } else {
            panic!("{}: RootMotionRootLock {lock}", self.path);
        }
        if self.additive_type != 0.0 {
            for k in 0..3 {
                out.s[k] = (out.s[k] as f64 - 1.0) as f32;
            }
        }
    }

    #[inline]
    fn word(&self, i: usize) -> u32 {
        let b: [u8; 4] = self.bytes[i * 4..i * 4 + 4].try_into().unwrap();
        u32::from_le_bytes(b)
    }

    #[inline]
    fn f32_at(&self, word: usize) -> f64 {
        f32::from_bits(self.word(word)) as f64
    }

    #[inline]
    fn u16_at(&self, i: usize) -> u32 {
        u16::from_le_bytes([self.bytes[i * 2], self.bytes[i * 2 + 1]]) as u32
    }

    /// `span`: the key span of the block at `at` (its frame table at element `tab`, -1 none) for fpos.
    fn span(&self, at: i32, tab: i32, k: &mut Key) {
        let nk = (self.word(at as usize >> 2) & 0xffffff) as usize;
        if nk < 2 {
            k.k0 = 0;
            k.k1 = 0;
            k.ka = 0.0;
            return;
        }
        let nkf = nk as f64;
        if tab < 0 {
            // multiplied before the division, so a position on a key is exact
            let p = if nkf == self.frames {
                self.fpos
            } else if self.frames > 1.0 {
                (self.fpos * (nkf - 1.0)) / (self.frames - 1.0)
            } else {
                0.0
            };
            let k0 = js::min(p.floor(), nkf - 1.0);
            let k1 = js::min(k0 + 1.0, nkf - 1.0);
            k.k0 = k0 as usize;
            k.k1 = k1 as usize;
            k.ka = if self.step || k1 == k0 { 0.0 } else { p - k0 };
            return;
        }
        if self.fpos >= self.frames - 1.0 {
            // t >= 1: the last key, share 0
            k.k0 = nk - 1;
            k.k1 = nk - 1;
            k.ka = 0.0;
            return;
        }
        let tab = tab as usize;
        let t = |i: usize| -> f64 { if self.wide { self.u16_at(tab + i) as f64 } else { self.bytes[tab + i] as f64 } };
        let fr = js::max(0.0, js::min(self.fpos.floor(), self.frames - 2.0));
        let mut lo = 0usize;
        let mut hi = nk - 1;
        while lo < hi {
            // the last key at or before frame fr
            let mid = (lo + hi + 1) >> 1;
            if t(mid) <= fr {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        k.k0 = lo;
        k.k1 = (lo + 1).min(nk - 1);
        if self.step || k.k1 == k.k0 {
            k.ka = 0.0;
            return;
        }
        let f0 = t(k.k0);
        let a = (self.fpos - f0) / js::max(1.0, t(k.k1) - f0);
        k.ka = if a < 0.0 {
            0.0
        } else if a > 1.0 {
            1.0
        } else {
            a
        };
    }

    /// `key`: key `ki` of the block at `at` into x, y, z (and w for a rotation).
    fn key(&self, at: i32, rotation: bool, ki: usize, k: &mut Key) {
        let at = at as usize;
        let h = self.word(at >> 2);
        let fmt = h >> 28;
        let flags = ((h >> 24) & 7) as usize;
        let mut x = 0.0;
        let mut y = 0.0;
        let mut z = 0.0;
        if fmt == FMT_FLOAT96 {
            if flags == 0 {
                let p = (at >> 2) + 1 + ki * 3;
                x = self.f32_at(p);
                y = self.f32_at(p + 1);
                z = self.f32_at(p + 2);
            } else {
                let mut p = (at >> 2) + 1 + ki * NC[flags];
                if flags & 1 != 0 {
                    x = self.f32_at(p);
                    p += 1;
                }
                if flags & 2 != 0 {
                    y = self.f32_at(p);
                    p += 1;
                }
                if flags & 4 != 0 {
                    z = self.f32_at(p);
                }
            }
        } else if fmt == FMT_FIXED48 {
            let mut p = (at >> 1) + 2 + ki * NC[flags];
            if rotation {
                let q = |p: usize| js::fround((self.u16_at(p) as f64 - 32767.0) * F_INV16);
                if flags & 1 != 0 {
                    x = q(p);
                    p += 1;
                }
                if flags & 2 != 0 {
                    y = q(p);
                    p += 1;
                }
                if flags & 4 != 0 {
                    z = q(p);
                }
            } else {
                if flags & 1 != 0 {
                    x = self.u16_at(p) as f64 - 255.0;
                    p += 1;
                }
                if flags & 2 != 0 {
                    y = self.u16_at(p) as f64 - 255.0;
                    p += 1;
                }
                if flags & 4 != 0 {
                    z = self.u16_at(p) as f64 - 255.0;
                }
            }
        } else {
            // Interval32 (a vector): (min, range) a stored component, then the keys
            let nc = NC[flags];
            let mut b = (at >> 2) + 1;
            let v = self.word(b + 2 * nc + ki);
            let f = js::fround;
            if flags & 1 != 0 {
                x = f(f(f(((v & 1023) as f64 - 511.0) * F_INV511) * self.f32_at(b + 1)) + self.f32_at(b));
                b += 2;
            }
            if flags & 2 != 0 {
                y = f(f(f((((v >> 10) & 2047) as f64 - 1023.0) * F_INV1023) * self.f32_at(b + 1)) + self.f32_at(b));
                b += 2;
            }
            if flags & 4 != 0 {
                z = f(f(f(((v >> 21) as f64 - 1023.0) * F_INV1023) * self.f32_at(b + 1)) + self.f32_at(b));
            }
        }
        k.x = x;
        k.y = y;
        k.z = z;
        if rotation {
            let f = js::fround;
            let w2 = f(f(f(1.0 - f(x * x)) - f(y * y)) - f(z * z));
            k.w = if w2 > 0.0 { f(w2.sqrt()) } else { 0.0 };
        }
    }

    fn vector(&self, at: i32, tab: i32, out: &mut [f32], o: usize) {
        if at == -1 {
            // zero translation, zero scale
            out[o..o + 3].fill(0.0);
            return;
        }
        let mut k = Key::default();
        self.span(at, tab, &mut k);
        self.key(at, false, k.k0, &mut k);
        if k.ka == 0.0 {
            out[o] = k.x as f32;
            out[o + 1] = k.y as f32;
            out[o + 2] = k.z as f32;
            return;
        }
        let (x0, y0, z0, a) = (k.x, k.y, k.z, k.ka);
        self.key(at, false, k.k1, &mut k);
        out[o] = (x0 + (k.x - x0) * a) as f32;
        out[o + 1] = (y0 + (k.y - y0) * a) as f32;
        out[o + 2] = (z0 + (k.z - z0) * a) as f32;
    }

    fn rotation(&self, at: i32, tab: i32, out: &mut [f32], o: usize) {
        if at == -1 {
            out[o..o + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            return;
        }
        let mut k = Key::default();
        self.span(at, tab, &mut k);
        self.key(at, true, k.k0, &mut k);
        if k.ka == 0.0 {
            out[o] = k.x as f32;
            out[o + 1] = k.y as f32;
            out[o + 2] = k.z as f32;
            out[o + 3] = k.w as f32;
            return;
        }
        let (x0, y0, z0, w0, a) = (k.x, k.y, k.z, k.w, k.ka);
        self.key(at, true, k.k1, &mut k);
        let b = if x0 * k.x + y0 * k.y + z0 * k.z + w0 * k.w < 0.0 { -a } else { a };
        let c = 1.0 - a;
        let x = x0 * c + k.x * b;
        let y = y0 * c + k.y * b;
        let z = z0 * c + k.z * b;
        let w = w0 * c + k.w * b;
        let l2 = x * x + y * y + z * z + w * w;
        if l2 < 1e-8 {
            // FQuat::Normalize below SMALL_NUMBER: the identity
            out[o..o + 4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            return;
        }
        let s = 1.0 / l2.sqrt();
        out[o] = (x * s) as f32;
        out[o + 1] = (y * s) as f32;
        out[o + 2] = (z * s) as f32;
        out[o + 3] = (w * s) as f32;
    }
}
