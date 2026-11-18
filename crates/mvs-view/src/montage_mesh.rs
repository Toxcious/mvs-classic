//! A montage mesh with a skeleton of its own and the
//! animation it plays: a clip of the character's, or a montage
//! or mesh composite asset played by its sections.

use mvs_data::fighter::FighterData;

use crate::anim::{Clips, Pose, Rig, sample_clip};
use crate::math::{self, Mat4};
use crate::mesh::MeshInfo;

/// One segment of a [`MeshMontage`]: a clip of the character's and its place on the montage's track.
#[derive(Clone, Debug)]
pub struct MeshSeg {
    pub clip: usize,
    pub start: f64,
    pub a0: f64,
    pub a1: f64,
    pub rate: f64,
    pub loops: f64,
}

/// A montage asset's first slot track (or a mesh composite's segments) as PlayAnimation on a component plays it.
#[derive(Clone, Debug)]
pub struct MeshMontage {
    pub segs: Vec<MeshSeg>,
    /// Per section its start and its next section (-1 none).
    pub sections: Vec<(f64, i64)>,
    pub length: f64,
    pub rate_scale: f64,
}

/// What a montage mesh plays.
#[derive(Clone, Debug)]
pub enum MeshClip {
    Clip(usize),
    Montage(MeshMontage),
}

/// `sameName`.
fn same_name(a: &str, b: &str) -> bool {
    a == b || a.eq_ignore_ascii_case(b)
}

fn warn(anim: &str, why: &str) -> Option<MeshClip> {
    eprintln!("warning: montage mesh: {anim} {why}: not played");
    None
}

/// `montageMeshClip`: the animation `anim` names (a loaded clip, else a montage or a mesh composite of the
/// character's data whose clips are loaded); None: nothing played (warned).
pub fn mesh_clip(clips: &Clips, data: &FighterData, anim: Option<&str>) -> Option<MeshClip> {
    let anim = anim?;
    if let Some(c) = clips.get(anim) {
        return Some(MeshClip::Clip(c));
    }
    let segs = |list: &[mvs_data::fighter::Segment]| -> Option<Vec<MeshSeg>> {
        list.iter()
            .map(|g| {
                Some(MeshSeg {
                    clip: clips.get(&g.anim)?,
                    start: g.start,
                    a0: g.a0,
                    a1: g.a1,
                    rate: g.rate,
                    loops: g.loops,
                })
            })
            .collect()
    };
    let missing = |list: &[mvs_data::fighter::Segment]| list.iter().find(|g| clips.get(&g.anim).is_none()).cloned();
    let Some(m) = data.montages.get(anim) else {
        let Some(c) = data.mesh_composites.as_ref().and_then(|c| c.get(anim)) else {
            return warn(anim, "is no loaded clip, montage or composite");
        };
        if c.segments.is_empty() {
            return warn(anim, "has no segment");
        }
        if let Some(g) = missing(&c.segments) {
            return warn(anim, &format!("plays {}, which is not loaded", g.anim));
        }
        return Some(MeshClip::Montage(MeshMontage {
            segs: segs(&c.segments)?,
            sections: vec![(0.0, -1)],
            length: c.length,
            rate_scale: c.rate_scale,
        }));
    };
    let one = (m.slots.len() == 1 && m.slots[0].segments.len() == 1).then(|| &m.slots[0].segments[0]);
    if let Some(g) = one
        && g.start == 0.0
        && g.a0 == 0.0
        && g.rate == 1.0
        && g.loops == 1.0
        && (g.a1 - m.length).abs() < 1e-4
        && m.rate_scale == 1.0
        && let Some(c) = clips.get(&g.anim)
    {
        return Some(MeshClip::Clip(c));
    }
    let track = m.slots.first().map_or(&[][..], |s| &s.segments[..]);
    if track.is_empty() {
        return warn(anim, "has no track");
    }
    if let Some(g) = missing(track) {
        return warn(anim, &format!("plays {}, which is not loaded", g.anim));
    }
    let sections = if m.sections.is_empty() {
        vec![(0.0, -1)]
    } else {
        m.sections
            .iter()
            .map(|s| {
                let next = s
                    .next
                    .as_ref()
                    .map_or(-1, |n| m.sections.iter().position(|x| same_name(&x.name, n)).map_or(-1, |i| i as i64));
                (s.t, next)
            })
            .collect()
    };
    Some(MeshClip::Montage(MeshMontage { segs: segs(track)?, sections, length: m.length, rate_scale: m.rate_scale }))
}

/// `meshMontageAt`: the sequence and its time `age` seconds after PlayAnimation.
pub fn mesh_montage_at(mm: &MeshMontage, age: f64, looped: bool) -> (usize, f64) {
    let secs = &mm.sections;
    let end = |k: usize| if k + 1 < secs.len() { secs[k + 1].0 } else { mm.length };
    // SetMontageLoop: the chain from the first section; a chain that ends gets its last section linked to the first
    let mut last = 0usize;
    let mut ends = false;
    let mut k = 0usize;
    for _ in 0..secs.len() {
        let nx = secs[k].1;
        if nx < 0 {
            ends = true;
            break;
        }
        k = nx as usize;
        last = k;
    }
    let next = |k: usize| -> i64 { if ends && k == last { if looped { 0 } else { -1 } } else { secs[k].1 } };
    let mut k = 0usize;
    let mut pos = secs[0].0;
    let mut left = (age * mm.rate_scale).max(0.0);
    for _ in 0..1000 {
        let e = end(k);
        if pos + left < e {
            pos += left;
            break;
        }
        let nx = next(k);
        if nx < 0 {
            pos = e;
            break;
        }
        left -= e - pos;
        let nx = nx as usize;
        // a section that loops on itself: whole turns taken at once
        if nx == k && e > secs[k].0 {
            left %= e - secs[k].0;
        }
        k = nx;
        pos = secs[k].0;
    }
    let mut g = &mm.segs[0];
    for s in &mm.segs {
        if pos >= s.start {
            g = s;
        }
    }
    let span = g.a1 - g.a0;
    let local = (pos - g.start).max(0.0) * g.rate.abs();
    let t = if span > 0.0 && local >= span && g.loops > 1.0 && local < span * g.loops {
        local % span
    } else {
        local.min(span)
    };
    (g.clip, if g.rate < 0.0 { g.a1 - t } else { g.a0 + t })
}

/// A skinned montage mesh's own skeleton posed (SocketMesh: its bones, their reference pose, the clip it plays).
#[derive(Debug)]
pub struct SocketSkin {
    rig: Rig,
    rest: Pose,
    parents: Vec<i32>,
    pub pose: Pose,
    pub clip: Option<MeshClip>,
}

impl SocketSkin {
    pub fn new(info: &MeshInfo, clips: &Clips, clip: Option<MeshClip>) -> SocketSkin {
        let sk = info.skeleton.as_ref().expect("a skinned montage mesh");
        SocketSkin {
            rig: Rig::bind(clips, sk.bones.clone()),
            rest: info.rest.clone(),
            parents: sk.parents.clone(),
            pose: info.rest.clone(),
            clip,
        }
    }

    /// `poseMontageMesh`: the animation's position (not looping: held at its end); a montage asset's by its sections.
    /// No clip: the pose it has stays.
    pub fn pose_at(&mut self, clips: &mut Clips, time: f64, looped: bool) {
        self.pose_locked(clips, time, looped, -1);
    }

    /// The skeleton's top bone (-1: none): the bone a sequence's root lock replaces.
    pub fn root(&self) -> i32 {
        self.parents.iter().position(|&p| p < 0).map_or(-1, |i| i as i32)
    }

    /// `pose_at` with the sequence's root lock on bone `root` (-1: sampled as it is; poseSocketMesh's).
    pub fn pose_locked(&mut self, clips: &mut Clips, time: f64, looped: bool, root: i32) {
        let (c, t) = match &self.clip {
            None => return,
            Some(MeshClip::Montage(m)) => mesh_montage_at(m, time, looped),
            Some(MeshClip::Clip(c)) => {
                let len = clips.clips[*c].anim.length;
                (*c, if looped && len > 0.0 { time % len } else { time })
            }
        };
        sample_clip(clips, c, &self.rig, &self.rest, t, &mut self.pose, root);
    }

    /// The bones in the mesh's own space (three's matrix world under the instance root: the plain product).
    pub fn bones_cs(&self) -> Vec<Mat4> {
        let p = &self.pose;
        let mut out: Vec<Mat4> = Vec::with_capacity(p.n);
        for i in 0..p.n {
            let f = |v: &[f32], k: usize| f64::from(v[k]);
            let t = [f(&p.t, i * 3), f(&p.t, i * 3 + 1), f(&p.t, i * 3 + 2)];
            let s = [f(&p.s, i * 3), f(&p.s, i * 3 + 1), f(&p.s, i * 3 + 2)];
            let local = math::compose(t, p.quat(i), s);
            let m = match self.parents[i] {
                pa if pa >= 0 => math::mul(&out[pa as usize], &local),
                _ => local,
            };
            out.push(m);
        }
        out
    }
}

/// `ownBoneLocal`: the matrix, under a view bone of world matrix `bone`, of what hangs on CharacterMesh0's bone of the
/// same name with the relative matrix `rel` while the view is reflected (facing left, a pawn without a mirror node):
/// B^-1 * Rx * B * rel, Rx the reflection of the depth axis (three x) about the view root's depth `root_x`.
pub fn own_bone_local(bone: &Mat4, root_x: f64, rel: &Mat4) -> Mat4 {
    let mut rx = math::compose([0.0; 3], [0.0, 0.0, 0.0, 1.0], [-1.0, 1.0, 1.0]);
    rx[12] = 2.0 * root_x;
    math::mul(&math::mul(&math::mul(&math::invert(bone), &rx), bone), rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(clip: usize, start: f64, a1: f64) -> MeshSeg {
        MeshSeg { clip, start, a0: 0.0, a1, rate: 1.0, loops: 1.0 }
    }

    #[test]
    fn a_section_that_loops_on_itself_wraps() {
        // Enter (0..1, next Loop), Loop (1..3, next itself): C020_Emote_MeeseeksCheer_Montage's shape
        let mm = MeshMontage {
            segs: vec![seg(0, 0.0, 1.0), seg(1, 1.0, 2.0)],
            sections: vec![(0.0, 1), (1.0, 1)],
            length: 3.0,
            rate_scale: 1.0,
        };
        assert_eq!(mesh_montage_at(&mm, 0.5, false), (0, 0.5));
        let (c, t) = mesh_montage_at(&mm, 5.5, false);
        assert_eq!(c, 1);
        assert!((t - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_chain_that_ends_holds_unless_looped() {
        let mm = MeshMontage { segs: vec![seg(0, 0.0, 2.0)], sections: vec![(0.0, -1)], length: 2.0, rate_scale: 1.0 };
        assert_eq!(mesh_montage_at(&mm, 3.0, false), (0, 2.0));
        let (_, t) = mesh_montage_at(&mm, 3.0, true);
        assert!((t - 1.0).abs() < 1e-9);
    }

    #[test]
    fn own_bone_local_reflects_depth_about_the_bone() {
        // a bone at depth 2 unturned: the holder under it goes to the bone's own reflection, x -> -x - 4 in its space
        let bone = math::compose([2.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
        let rel = math::compose([0.5, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
        let m = own_bone_local(&bone, 0.0, &rel);
        assert!((m[12] + 4.5).abs() < 1e-9 && m[13].abs() < 1e-9 && (m[0] + 1.0).abs() < 1e-9);
    }
}
