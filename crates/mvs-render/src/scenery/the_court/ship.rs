//! The Court's Marvin ship (M010_MG_Ship_C; its AnimBP M010_Ship_AnimBP_C: a DefaultSlot over no source pose, the
//! reference pose), drawn from the block's `ship`.
//! - BeginPlay @369 (PandaGameState cast: the Lab's too): CreateProxyObjectForPlayMontage(Mesh, Landing, 1, 0, None)
//!   @552: the landing from 0 at the level's begin. Neither montage completes, as each one's last section loops on
//!   itself (Default -> Loop -> Loop: the landing holds M010Ship_IdlePose), so the mesh is never hidden and no auto
//!   blend-out comes.
//! - The montage (FAnimMontageInstance): its position advances by dt; past its section's end it goes on in the next
//!   section (the section start + the rest). Its weight: the blend in from 0 (the landing's BlendIn 0: at once), a
//!   montage played over another stops the old one with the new one's BlendIn, from its weight then, the stopped one
//!   advancing on until its weight is 0. The slot's pose: the montages by their weights over the reference pose by
//!   what is left (linear blends). The animations have no root motion: the root bone plays its track.
//! - Notifies, queued (1428719e0): a notify state is listed in a step when its time is at or before the position and
//!   its end after the previous one (the landing's PS_M010_Spaceship_Land from -0.0001 begins in the first tick),
//!   begins when newly listed and ends in the step after the one that passes its end; an instant fires when
//!   previous < its time <= the position. Over a section jump the step lists both ranges. AnimNotify_PlayVFX (a
//!   non-fighter owner's SpawnVFX: attached at its socket with its Location / Rotation / VFX Scale,
//!   KeepRelativeOffset, followed), deactivated at its end with KillOnNotifyEnd; Ak_OneShot posts its event at the
//!   actor's root (its AkComponent, Follow: the root does not move). A state still open when its montage instance
//!   ends ends then.
// Unverified: a stopped montage plays on with its notifies until its blend out ends (as trophys-edge's, 4.26 from
// memory); the montages' first advance in the level's first tick (the BeginPlay play counts no time before it).
// Unverified: OnMatchEnded @981 (MatchEndImpl -> MatchEnded @6067) is detected as the World being over and not started:
// the take-off montage plays in the first tick whose World is so (once per begin).

use std::collections::BTreeMap;

use mvs_vfx::{Attachment, SystemId};
use mvs_view::M4;
use mvs_view::anim::Pose;
use serde::Deserialize;

use crate::Result;
use crate::scenery::host::SceneryHost;
use crate::scenery::sky_arena::attachment_of;
use crate::stage::{bad, transform};
use crate::view::mul;

#[derive(Debug, Deserialize)]
struct Segment {
    anim: String,
    start: f64,
    end: f64,
    at: f64,
}

#[derive(Debug, Deserialize)]
struct Section {
    name: String,
    at: f64,
    next: String,
}

#[derive(Debug, Deserialize)]
struct Notify {
    at: f64,
    end: Option<f64>,
    system: Option<String>,
    socket: Option<String>,
    loc: Option<[f64; 3]>,
    rot: Option<[f64; 3]>,
    scale: Option<[f64; 3]>,
    #[serde(default)]
    kill: bool,
    sound: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Montage {
    length: f64,
    blend_in: f64,
    segments: Vec<Segment>,
    sections: Vec<Section>,
    notifies: Vec<Notify>,
}

#[derive(Debug, Deserialize)]
struct Socket {
    bone: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShipBlock {
    skeletal: String,
    landing: Montage,
    take_off: Montage,
    sockets: BTreeMap<String, Socket>,
    root: [f64; 3],
}

/// A vfx notify's place: its socket's bone, and the socket's x the notify's relative transform (UE, row-vector).
#[derive(Debug)]
struct Follow {
    bone: String,
    local: M4,
}

#[derive(Debug)]
struct Playing {
    /// 0 the landing, 1 the take-off.
    m: usize,
    pos: f64,
    weight: f64,
    from: f64,
    to: f64,
    blend: f64,
    blend_time: f64,
    out: bool,
    /// Per notify state, listed in the last step.
    listed: Vec<bool>,
    vfx: Vec<Option<SystemId>>,
}

#[derive(Debug)]
pub(super) struct Ship {
    skeletal: String,
    root: [f64; 3],
    montages: [Montage; 2],
    /// Per montage, per notify: where a vfx notify's system follows.
    follows: [Vec<Option<Follow>>; 2],
    playing: Vec<Playing>,
    /// The systems that follow their socket, with the montage and the notify they came of.
    followers: Vec<(SystemId, usize, usize)>,
    rest: Option<Pose>,
    sample: Pose,
    acc: Pose,
    /// The step's ranges: (from, to] or, over a section jump, (from, end] and [start, to].
    ranges: Vec<f64>,
    /// OnMatchEnded has played the take-off in this match.
    took_off: bool,
}

impl Ship {
    pub(super) fn make(d: ShipBlock, host: &SceneryHost) -> Result<Ship> {
        let what = "scenery: the court ship";
        if host.skeletal_index(&d.skeletal).is_none() {
            return Err(bad(what, format!("{} is not a skeletal mesh of the file", d.skeletal)));
        }
        let follows_of = |m: &Montage| -> Result<Vec<Option<Follow>>> {
            m.notifies
                .iter()
                .map(|nt| {
                    if nt.system.is_none() {
                        return Ok(None);
                    }
                    let name = nt.socket.as_deref().unwrap_or("");
                    let sock =
                        d.sockets.get(name).ok_or_else(|| bad(what, format!("socket {name} not in the file")))?;
                    if host.bone_world(&d.skeletal, &sock.bone).is_none() {
                        return Err(bad(what, format!("no bone {}", sock.bone)));
                    }
                    let n =
                        transform(nt.loc.unwrap_or([0.0; 3]), nt.rot.unwrap_or([0.0; 3]), nt.scale.unwrap_or([1.0; 3]));
                    let s = transform(sock.loc, sock.rot, sock.scale);
                    // (the socket's then the notify's: row-vector, the notify's first)
                    Ok(Some(Follow { bone: sock.bone.clone(), local: mul(&n, &s) }))
                })
                .collect()
        };
        let follows = [follows_of(&d.landing)?, follows_of(&d.take_off)?];
        for m in [&d.landing, &d.take_off] {
            if m.segments.is_empty() || m.sections.is_empty() {
                return Err(bad(what, "a montage with no segment or section"));
            }
            // (the montages' animations are loaded with the block)
            for s in &m.segments {
                let Some(c) = host.state.clips.get(&s.anim) else {
                    return Err(bad(what, format!("the animation {} is not loaded", s.anim)));
                };
                if host.state.clips.clips[c].anim.rate_scale != 1.0 {
                    return Err(bad(what, format!("{} has a RateScale", s.anim)));
                }
            }
        }
        Ok(Ship {
            skeletal: d.skeletal,
            root: d.root,
            montages: [d.landing, d.take_off],
            follows,
            playing: Vec::new(),
            followers: Vec::new(),
            rest: None,
            sample: Pose::default(),
            acc: Pose::default(),
            ranges: Vec::new(),
            took_off: false,
        })
    }

    /// The section `pos` is in: its end and the section it goes on in.
    fn section_end(m: &Montage, pos: f64) -> (f64, Option<&Section>) {
        let mut i = 0;
        for (k, s) in m.sections.iter().enumerate() {
            if pos >= s.at {
                i = k;
            }
        }
        let end = m.sections.get(i + 1).map_or(m.length, |s| s.at);
        let next = m.sections.iter().find(|s| s.name.eq_ignore_ascii_case(&m.sections[i].next));
        (end, next)
    }

    fn advance(ranges: &mut Vec<f64>, m: &Montage, p: &mut Playing, dt: f64) {
        ranges.clear();
        let mut from = p.pos;
        let mut left = dt;
        for _ in 0..8 {
            let (end, next) = Ship::section_end(m, from);
            match next {
                Some(next) if from + left >= end => {
                    ranges.extend([from, end]);
                    left -= end - from;
                    from = next.at;
                }
                _ => {
                    let to = (from + left).min(m.length);
                    ranges.extend([from, to]);
                    p.pos = to;
                    return;
                }
            }
        }
        p.pos = from;
    }

    fn attachment(&self, host: &SceneryHost, f: &Follow) -> Option<Attachment> {
        let bone = host.bone_world(&self.skeletal, &f.bone)?;
        Some(attachment_of(&mul(&f.local, &bone)))
    }

    fn spawn(&mut self, host: &mut SceneryHost, m: usize, k: usize) -> Option<SystemId> {
        let nt = &self.montages[m].notifies[k];
        let (system, f) = (nt.system.as_deref()?, self.follows[m][k].as_ref()?);
        let at = self.attachment(host, f)?;
        let id = host.vfx()?.spawn_level(system, &at, false, &[], 0, false)?;
        self.followers.push((id, m, k));
        Some(id)
    }

    fn end_notify(host: &mut SceneryHost, nt: &Notify, p: &mut Playing, k: usize) {
        if nt.kill
            && let Some(id) = p.vfx[k]
            && let Some(vfx) = host.vfx()
        {
            vfx.deactivate(id);
        }
        p.vfx[k] = None;
    }

    fn notifies(&mut self, host: &mut SceneryHost, i: usize) {
        let m = self.playing[i].m;
        for k in 0..self.montages[m].notifies.len() {
            let nt = &self.montages[m].notifies[k];
            let listed = self.ranges.chunks(2).any(|r| match nt.end {
                None => nt.at > r[0] && nt.at <= r[1],
                Some(end) => nt.at <= r[1] && end > r[0],
            });
            if nt.end.is_none() {
                if listed {
                    if let Some(sound) = &nt.sound {
                        host.sound(sound, self.root, None);
                    }
                    self.spawn(host, m, k);
                }
                continue;
            }
            let was = self.playing[i].listed[k];
            if listed && !was {
                let sound = nt.sound.clone();
                let id = self.spawn(host, m, k);
                self.playing[i].vfx[k] = id;
                if let Some(sound) = sound {
                    host.sound(&sound, self.root, None);
                }
            } else if !listed && was {
                Ship::end_notify(host, &self.montages[m].notifies[k], &mut self.playing[i], k);
            }
            self.playing[i].listed[k] = listed;
        }
    }

    fn end_all(host: &mut SceneryHost, montages: &[Montage; 2], p: &mut Playing) {
        for k in 0..p.vfx.len() {
            if p.listed[k] {
                Ship::end_notify(host, &montages[p.m].notifies[k], p, k);
            }
        }
    }

    fn play(&mut self, m: usize) {
        let blend_in = self.montages[m].blend_in;
        for p in self.playing.iter_mut().filter(|p| !p.out) {
            p.out = true;
            p.from = p.weight;
            p.to = 0.0;
            p.blend = 0.0;
            p.blend_time = blend_in;
        }
        let n = self.montages[m].notifies.len();
        self.playing.push(Playing {
            m,
            pos: 0.0,
            weight: 0.0,
            from: 0.0,
            to: 1.0,
            blend: 0.0,
            blend_time: blend_in,
            out: false,
            listed: vec![false; n],
            vfx: vec![None; n],
        });
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        // the AnimBP's pose, the block's from now on (the clip none)
        if self.rest.is_none() {
            host.drive(&self.skeletal);
            let rest = host.rest(&self.skeletal).cloned().unwrap_or_default();
            self.sample = Pose::new(rest.n);
            self.acc = Pose::new(rest.n);
            self.rest = Some(rest);
        }
        let mut playing = std::mem::take(&mut self.playing);
        for p in &mut playing {
            Ship::end_all(host, &self.montages, p);
        }
        self.took_off = false;
        // BeginPlay @552
        self.play(0);
    }

    /// The montages one tick on, their notifies, and the slot's pose written; then the followers moved.
    pub(super) fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        // OnMatchEnded @981
        if !self.took_off && host.world().is_some_and(|w| w.flow.match_over && !w.flow.match_started) {
            self.took_off = true;
            self.play(1);
        }
        for i in (0..self.playing.len()).rev() {
            let m = self.playing[i].m;
            Ship::advance(&mut self.ranges, &self.montages[m], &mut self.playing[i], dt);
            self.notifies(host, i);
            let p = &mut self.playing[i];
            p.blend += dt;
            p.weight = if p.blend_time > 0.0 && p.blend < p.blend_time {
                p.from + (p.to - p.from) * (p.blend / p.blend_time)
            } else {
                p.to
            };
            if p.out && p.weight <= 0.0 {
                let mut p = self.playing.remove(i);
                Ship::end_all(host, &self.montages, &mut p);
            }
        }
        self.write_pose(host);
        self.follow(host);
    }

    fn write_pose(&mut self, host: &mut SceneryHost) {
        let Some(rest) = &self.rest else { return };
        let mut total = 0.0;
        self.acc.t.fill(0.0);
        self.acc.r.fill(0.0);
        self.acc.s.fill(0.0);
        for p in &self.playing {
            if p.weight <= 0.0 {
                continue;
            }
            let m = &self.montages[p.m];
            let mut s = &m.segments[0];
            for x in &m.segments {
                if p.pos >= x.at {
                    s = x;
                }
            }
            let t = s.end.min(s.start + (p.pos - s.at));
            // (every clip is checked loaded in make)
            host.sample(&self.skeletal, &s.anim, t, &mut self.sample);
            add_pose(&mut self.acc, &self.sample, p.weight);
            total += p.weight;
        }
        if total < 1.0 {
            add_pose(&mut self.acc, rest, 1.0 - total);
        }
        let norm = if total > 1.0 { 1.0 / total } else { 1.0 };
        let acc = &self.acc;
        let Some(pose) = host.pose(&self.skeletal) else { return };
        for b in 0..acc.n {
            for c in 0..3 {
                pose.t[b * 3 + c] = (f64::from(acc.t[b * 3 + c]) * norm) as f32;
                pose.s[b * 3 + c] = (f64::from(acc.s[b * 3 + c]) * norm) as f32;
            }
            let r = &acc.r[b * 4..b * 4 + 4];
            let l = r.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>().sqrt();
            let l = if l == 0.0 { 1.0 } else { l };
            for (o, &x) in pose.r[b * 4..b * 4 + 4].iter_mut().zip(r) {
                *o = (f64::from(x) / l) as f32;
            }
        }
    }

    /// The followed systems at their sockets as the pose stands (VfxPlayer's follow, before its tick); the dead ones
    /// dropped.
    fn follow(&mut self, host: &mut SceneryHost) {
        let mut k = 0;
        while k < self.followers.len() {
            let (id, m, n) = self.followers[k];
            if !host.vfx().is_some_and(|v| v.alive(id)) {
                self.followers.swap_remove(k);
                continue;
            }
            let at = self.follows[m][n].as_ref().and_then(|f| self.attachment(host, f));
            if let (Some(at), Some(vfx)) = (at, host.vfx()) {
                vfx.attach(id, &at);
            }
            k += 1;
        }
    }
}

/// pose += w x o (the rotations by the shortest arc against what is summed so far), stored float32.
fn add_pose(acc: &mut Pose, o: &Pose, w: f64) {
    for i in 0..acc.t.len() {
        acc.t[i] = (f64::from(acc.t[i]) + f64::from(o.t[i]) * w) as f32;
        acc.s[i] = (f64::from(acc.s[i]) + f64::from(o.s[i]) * w) as f32;
    }
    for b in (0..acc.r.len()).step_by(4) {
        let dot: f64 = (0..4).map(|c| f64::from(acc.r[b + c]) * f64::from(o.r[b + c])).sum();
        let sw = if dot < 0.0 { -w } else { w };
        for c in 0..4 {
            acc.r[b + c] = (f64::from(acc.r[b + c]) + f64::from(o.r[b + c]) * sw) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn montage() -> Montage {
        serde_json::from_value(serde_json::json!({
            "blendIn": 0, "length": 2,
            "segments": [{ "anim": "A", "at": 0, "start": 0, "end": 1.5 }],
            "sections": [{ "at": 0, "name": "Default", "next": "Loop" }, { "at": 1.5, "name": "Loop", "next": "loop" }],
            "notifies": []
        }))
        .unwrap()
    }

    /// Past a section's end the montage goes on in its next section, the step's two ranges listed; the loop section
    /// jumps to itself.
    #[test]
    fn montage_advances_over_sections() {
        let m = montage();
        let mut p = Playing {
            m: 0,
            pos: 1.4,
            weight: 1.0,
            from: 0.0,
            to: 1.0,
            blend: 0.0,
            blend_time: 0.0,
            out: false,
            listed: Vec::new(),
            vfx: Vec::new(),
        };
        let mut ranges = Vec::new();
        Ship::advance(&mut ranges, &m, &mut p, 0.2);
        assert!((p.pos - 1.6).abs() < 1e-12, "{}", p.pos);
        // (1.4 to the Default section's end at 1.5, then 0.1 into Loop)
        assert_eq!(ranges.len(), 4);
        Ship::advance(&mut ranges, &m, &mut p, 0.5);
        // (1.6 + 0.4 reaches the loop section's end, 2: back at 1.5 with 0.1 left)
        assert!((p.pos - 1.6).abs() < 1e-12, "{}", p.pos);
        assert_eq!(ranges.len(), 4);
    }

    /// Two poses at half weight each: the translations' mean, the rotations' normalised sum on the shortest arc.
    #[test]
    fn poses_blend_by_weight() {
        let mut a = Pose::new(1);
        a.t.copy_from_slice(&[2.0, 0.0, 0.0]);
        a.r.copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
        let mut b = Pose::new(1);
        b.r.copy_from_slice(&[0.0, 0.0, 0.0, -1.0]);
        let mut acc = Pose::new(1);
        add_pose(&mut acc, &a, 0.5);
        add_pose(&mut acc, &b, 0.5);
        assert_eq!(acc.t, [1.0, 0.0, 0.0]);
        assert_eq!(acc.r, [0.0, 0.0, 0.0, 1.0]);
    }

    /// A world transform with a mirrored X: its X scale negative, the rotation rows a rotation.
    #[test]
    fn attachment_decomposes() {
        let m = transform([1.0, 2.0, 3.0], [0.0, 90.0, 0.0], [-2.0, 3.0, 4.0]);
        let at = attachment_of(&m);
        assert_eq!(at.loc, [1.0, 2.0, 3.0]);
        let s = at.scale.unwrap();
        assert!((s[0] + 2.0).abs() < 1e-12 && (s[1] - 3.0).abs() < 1e-12 && (s[2] - 4.0).abs() < 1e-12);
        let r = rows_of_yaw90();
        let close = at.rot.iter().flatten().zip(r.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12);
        assert!(close, "{:?}", at.rot);
    }

    fn rows_of_yaw90() -> [[f64; 3]; 3] {
        super::super::rows_of([0.0, 90.0, 0.0])
    }
}
