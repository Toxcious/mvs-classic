//! The boxes entry's skel block (`mvs_data::boxes::SkelIndex`): for a character whose AnimBP layers poses under its
//! shapes (consts.rs SKEL), the bones the shapes ride, their poses per clip frame and its blend spaces' rows, read by
//! the sim's shape pose.
//!
//! What it holds: the bones the shapes ride with their parents, their reference pose, each shape's place on its bone,
//! the layered nodes' bone sets, the blend spaces' cooked rows, and per clip the local transforms of those bones a
//! frame.
//!
//! Space: glTF metres (translation (X, Z, Y) x 0.01, rotation (x, z, y, -w), scale (X, Z, Y)), converted here from the
//! loop's UE values: the sim's pose code and its SKEL_EPS read that space. An additive clip is kept as its delta
//! against frame RefFrameIndex of its RefPoseSeq: rotation abs x base^-1, translation abs - base, scale abs / base - 1
//! (-1 for a base scale within 1e-8 of 0), for the bones the sequence has a track for (flag 8); the other bones have
//! the identity.

use std::collections::{HashMap, HashSet};

use mvs_data::boxes::{BlendAxis, BlendSample, SkelBlend, SkelClip, SkelIndex};
use mvs_data::fighter::Map;
use mvs_data::{FighterData, Value};

use super::super::blend::bind_blend_space;
use super::super::shape::fail;
use super::consts::{SKEL_EPS, SkelSpec};
use super::frames::{ClipSource, LocalPose, PoseSource, RefSkeleton, new_pose};
use super::{BoxClip, Placed};
use crate::kind::{Cx, KindError};

/// The blend spaces of a SKEL entry, by the sim's name.
pub fn skel_blend(cx: &Cx<'_>, spec: &SkelSpec) -> Result<Map<SkelBlend>, KindError> {
    let mut out = Map::new();
    for &(k, path) in spec.blend {
        let d = bind_blend_space(cx, path)?;
        if d.samples.iter().any(|s| s.rate != 1.0) {
            return fail(format!("boxes: blend space {path}: a sample with a rate"));
        }
        out.insert(
            k.to_owned(),
            SkelBlend {
                axes: d.axes.iter().map(|a| BlendAxis { min: a.min, max: a.max, grid: a.grid }).collect(),
                samples: d.samples.iter().map(|s| BlendSample { clip: s.clip.clone(), x: s.x, y: s.y }).collect(),
                grid: d.grid.clone(),
                full: None,
            },
        );
    }
    Ok(out)
}

/// The clips the skel block keeps (`None`: all): SkelSpec.nav's rule.
pub fn skel_wanted(
    spec: &SkelSpec,
    fd: &FighterData,
    blend: &Map<SkelBlend>,
    clips: &[BoxClip],
) -> Result<Option<HashSet<String>>, KindError> {
    let Some(nav) = spec.nav else { return Ok(None) };
    let mut want: HashSet<String> = nav.iter().map(|s| (*s).to_owned()).collect();
    for v in fd.anim_set.values() {
        if let Value::Str(s) = v {
            want.insert(s.clone());
        }
    }
    for (k, a) in &fd.anim_assets {
        let Some(samples) = &a.samples else {
            return fail(format!("boxes: skel clips: anim asset {k} has no samples"));
        };
        want.extend(samples.iter().cloned());
    }
    // (absent for a set without FallBlendSpace: Tom and Jerry's)
    for s in fd.fall_space.iter().flat_map(|f| &f.samples) {
        want.insert(s.anim.clone());
    }
    for w in fd.wall_spaces.iter().flat_map(|w| [&w.enter, &w.idle]) {
        want.extend(w.samples.iter().map(|s| s.anim.clone()));
    }
    for b in blend.values() {
        want.extend(b.samples.iter().map(|s| s.clip.clone()));
    }
    let names: HashSet<&str> = clips.iter().map(|c| c.name.as_str()).collect();
    if let Some(c) = nav.iter().find(|c| !names.contains(*c)) {
        return fail(format!("boxes: skel clips: {c} is not a clip of the character"));
    }
    Ok(Some(want))
}

const ROW: usize = 10;

pub struct SkelBlock {
    spec: &'static SkelSpec,
    names: Vec<usize>,
    ix: HashMap<usize, i32>,
    shape_bone: Vec<i32>,
    shape_loc: Vec<f64>,
    shape_rot: Vec<f64>,
    com: i32,
    blend: Map<SkelBlend>,
    want: Option<HashSet<String>>,
    clips: Map<SkelClip>,
    blob: Vec<f32>,
    bases: HashMap<String, Vec<f64>>,
}

/// One clip's recorder (SkelBlock::begin).
pub struct SkelClipRec {
    name: String,
    nf: usize,
    frames: Vec<f64>,
    row: Vec<f64>,
    base: Option<Vec<f64>>,
    tracked: Vec<bool>,
}

impl SkelBlock {
    pub fn new(
        ref_: &RefSkeleton,
        spec: &'static SkelSpec,
        shapes: &[Placed],
        shape_bone: &[i32],
        com: i32,
        blend: Map<SkelBlend>,
        want: Option<HashSet<String>>,
    ) -> Result<SkelBlock, KindError> {
        let mut need = vec![false; ref_.bones.len()];
        let mut roots: Vec<i32> = shape_bone.iter().copied().filter(|b| *b >= 0).collect();
        for k in spec.keep {
            let b = ref_.index(k);
            if b < 0 {
                return fail(format!("boxes: skel bone {k} not in the mesh"));
            }
            roots.push(b);
        }
        if com >= 0 {
            roots.push(com);
        }
        for b in roots {
            let mut b = b;
            while b >= 0 && !need[b as usize] {
                need[b as usize] = true;
                b = ref_.parents[b as usize];
            }
        }
        let names: Vec<usize> = (0..need.len()).filter(|b| need[*b]).collect();
        let ix = names.iter().enumerate().map(|(i, b)| (*b, i as i32)).collect();
        for (_, tops) in spec.masks {
            if let Some(t) = tops.iter().find(|t| ref_.index(t) < 0) {
                return fail(format!("boxes: skel mask bone {t} not in the mesh"));
            }
        }
        Ok(SkelBlock {
            spec,
            names,
            ix,
            shape_bone: shape_bone.to_vec(),
            shape_loc: shapes.iter().flat_map(|s| [s.loc[0] * 0.01, s.loc[2] * 0.01, s.loc[1] * 0.01]).collect(),
            shape_rot: shapes.iter().flat_map(|s| [s.rot[0], s.rot[2], s.rot[1], -s.rot[3]]).collect(),
            com,
            blend,
            want,
            clips: Map::new(),
            blob: Vec::new(),
            bases: HashMap::new(),
        })
    }

    pub fn wants(&self, name: &str) -> bool {
        self.want.as_ref().is_none_or(|w| w.contains(name))
    }

    /// The bones' glTF rows of a pose into out at o (ROW floats a bone).
    fn rows(&self, p: &LocalPose, out: &mut [f64], o: usize) {
        for (i, &b) in self.names.iter().enumerate() {
            let k = o + i * ROW;
            out[k] = p.t[b * 3] * 0.01;
            out[k + 1] = p.t[b * 3 + 2] * 0.01;
            out[k + 2] = p.t[b * 3 + 1] * 0.01;
            out[k + 3] = p.r[b * 4];
            out[k + 4] = p.r[b * 4 + 2];
            out[k + 5] = p.r[b * 4 + 1];
            out[k + 6] = -p.r[b * 4 + 3];
            out[k + 7] = p.s[b * 3];
            out[k + 8] = p.s[b * 3 + 2];
            out[k + 9] = p.s[b * 3 + 1];
        }
    }

    /// An additive clip's base rows: frame RefFrameIndex of its RefPoseSeq (no root lock).
    fn base(&mut self, cs: &ClipSource<'_>, src: &mut PoseSource<'_, '_>) -> Result<Vec<f64>, KindError> {
        let (clip, frame) = cs.skel_base()?;
        let key = format!("{}:{frame}", clip.path);
        if let Some(b) = self.bases.get(&key) {
            return Ok(b.clone());
        }
        let Some(l) = src.load(&clip)? else {
            return fail(format!("boxes: skel: the base {} of {} is on another Skeleton", clip.path, cs.name));
        };
        let mut bc = src.open(&l)?;
        if bc.additive {
            return fail(format!("boxes: skel: the base {} of {} is additive", clip.path, cs.name));
        }
        let mut p = new_pose(src.ref_.bones.len());
        src.frame(&mut bc, frame, &mut p);
        let mut b = vec![0.0; self.names.len() * ROW];
        self.rows(&p, &mut b, 0);
        self.bases.insert(key, b.clone());
        Ok(b)
    }

    /// Starts a clip: its frames are given in order (the pose after the root lock, before anything else).
    pub fn begin(&mut self, cs: &ClipSource<'_>, src: &mut PoseSource<'_, '_>) -> Result<SkelClipRec, KindError> {
        let n = self.names.len();
        let nf = cs.num_frames;
        let base = if cs.additive { Some(self.base(cs, src)?) } else { None };
        Ok(SkelClipRec {
            name: cs.name.clone(),
            nf,
            frames: vec![0.0; nf * n * ROW],
            row: vec![0.0; n * ROW],
            base,
            tracked: self.names.iter().map(|b| cs.tracked[*b]).collect(),
        })
    }

    /// The index's skel at `offset` floats of the section, and its floats.
    pub fn finish(self, ref_: &RefSkeleton, offset: u32) -> Result<(SkelIndex, Vec<f32>), KindError> {
        let names = &self.names;
        let mut rest = Vec::with_capacity(names.len() * ROW);
        for &b in names {
            rest.extend([ref_.t[b * 3] * 0.01, ref_.t[b * 3 + 2] * 0.01, ref_.t[b * 3 + 1] * 0.01]);
            rest.extend([ref_.r[b * 4], ref_.r[b * 4 + 2], ref_.r[b * 4 + 1], -ref_.r[b * 4 + 3]]);
            rest.extend([ref_.s[b * 3], ref_.s[b * 3 + 2], ref_.s[b * 3 + 1]]);
        }
        let under = |b: usize, tops: &HashSet<i32>| {
            let mut b = b as i32;
            while b >= 0 {
                if tops.contains(&b) {
                    return true;
                }
                b = ref_.parents[b as usize];
            }
            false
        };
        let mut masks = Map::new();
        for (k, tops) in self.spec.masks {
            let t: HashSet<i32> = tops.iter().map(|n| ref_.index(n)).collect();
            let m: Vec<i32> = names.iter().enumerate().filter(|(_, b)| under(**b, &t)).map(|(i, _)| i as i32).collect();
            masks.insert((*k).to_owned(), m);
        }
        let mut blend = Map::new();
        for (k, bs) in &self.blend {
            let mut add = Vec::new();
            for s in &bs.samples {
                let Some(c) = self.clips.get(&s.clip) else {
                    return fail(format!("boxes: blend space {k}: {} is not a clip of the character", s.clip));
                };
                add.push(c.a.is_some_and(|a| a != 0));
            }
            let any = add.iter().any(|a| *a);
            if any && !add.iter().all(|a| *a) {
                return fail(format!("boxes: blend space {k}: additive and full pose samples together"));
            }
            let mut b = bs.clone();
            if !any {
                b.full = Some(1);
            }
            blend.insert(k.clone(), b);
        }
        let ix = |b: i32| if b >= 0 { self.ix.get(&(b as usize)).copied().unwrap_or(-1) } else { -1 };
        let index = SkelIndex {
            offset,
            bones: names.iter().map(|b| ref_.bones[*b].clone()).collect(),
            parents: names.iter().map(|b| ix(ref_.parents[*b])).collect(),
            rest,
            shape_bone: self.shape_bone.iter().map(|b| ix(*b)).collect(),
            shape_loc: self.shape_loc.clone(),
            shape_rot: self.shape_rot.clone(),
            com: ix(self.com),
            masks,
            clips: self.clips,
            blend,
        };
        Ok((index, self.blob))
    }
}

impl SkelClipRec {
    pub fn frame(&mut self, blk: &SkelBlock, f: usize, p: &LocalPose) {
        let n = blk.names.len();
        let o = f * n * ROW;
        let Some(base) = &self.base else {
            blk.rows(p, &mut self.frames, o);
            return;
        };
        blk.rows(p, &mut self.row, 0);
        let (row, frames) = (&self.row, &mut self.frames);
        for i in 0..n {
            let (k, r) = (o + i * ROW, i * ROW);
            if !self.tracked[i] {
                frames[k..k + ROW].fill(0.0);
                frames[k + 6] = 1.0;
                continue;
            }
            for j in 0..3 {
                frames[k + j] = row[r + j] - base[r + j];
            }
            // r x conj(base r)
            let (ax, ay, az, aw) = (row[r + 3], row[r + 4], row[r + 5], row[r + 6]);
            let (bx, by, bz, bw) = (-base[r + 3], -base[r + 4], -base[r + 5], base[r + 6]);
            frames[k + 3] = aw * bx + ax * bw + ay * bz - az * by;
            frames[k + 4] = aw * by - ax * bz + ay * bw + az * bx;
            frames[k + 5] = aw * bz + ax * by - ay * bx + az * bw;
            frames[k + 6] = aw * bw - ax * bx - ay * by - az * bz;
            for j in 0..3 {
                let bs = base[r + 7 + j];
                frames[k + 7 + j] = if bs.abs() > 1e-8 { row[r + 7 + j] / bs - 1.0 } else { -1.0 };
            }
        }
    }

    pub fn end(self, blk: &mut SkelBlock) {
        let n = blk.names.len();
        let (nf, frames) = (self.nf, &self.frames);
        let mut flags = Vec::with_capacity(n);
        for i in 0..n {
            let mut fl = 0u32;
            for (bit, a, b) in [(1, 0, 3), (2, 3, 7), (4, 7, 10)] {
                let moves = (1..nf)
                    .any(|f| (a..b).any(|k| (frames[(f * n + i) * ROW + k] - frames[i * ROW + k]).abs() > SKEL_EPS));
                if moves {
                    fl |= bit;
                }
            }
            if self.base.is_some() && self.tracked[i] {
                fl |= 8;
            }
            flags.push(fl);
        }
        let c = SkelClip { o: blk.blob.len() as u32, v: flags.clone(), a: self.base.is_some().then_some(1) };
        blk.clips.insert(self.name.clone(), c);
        let blob = &mut blk.blob;
        // (a clip of no frames: the first frame's floats are undefined there, NaN in the float32 section)
        blob.extend((0..n * ROW).map(|k| frames.get(k).map_or(f32::NAN, |v| *v as f32)));
        for f in 0..nf {
            for (i, &fl) in flags.iter().enumerate() {
                let o = (f * n + i) * ROW;
                if fl & 1 != 0 {
                    blob.extend(frames[o..o + 3].iter().map(|v| *v as f32));
                }
                if fl & 2 != 0 {
                    blob.extend(frames[o + 3..o + 7].iter().map(|v| *v as f32));
                }
                if fl & 4 != 0 {
                    blob.extend(frames[o + 7..o + 10].iter().map(|v| *v as f32));
                }
            }
        }
    }
}
