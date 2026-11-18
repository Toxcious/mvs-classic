//! Trophy's E.D.G.E. (TrophyRoom): the block its scenery entry has of its own Blueprint actor, the ACME crate
//! Block `crate`.
//!
//! TROPHY_AcmeCrate_A_01_Actor_C: BeginPlay sets NextBurstTime = game time + RandomFloatInRange(10, 30) (@1414) and
//! calls QueueNextAnim (@768): when the game time is NextBurstTime or later it plays the burst montage (PlayMontage on
//! the CrateMesh, rate 1, from 0), sets the next burst time and queues again; otherwise it waits
//! RandomFloatInRange(2, 7) (Delay @902, resumes at 44), plays the shake montage and queues again. So the burst comes
//! in the tick a wait ends at or after its time, the shake of that tick replaced in the same tick, and a shake 2 to
//! 7 s after a burst starts interrupts it when it comes before the burst's end (4.78 s). The montage callbacks only
//! set a name the graph never reads.
//! AcmeCrate_AnimBP_C: its AnimGraph is the DefaultSlot over its unlinked source (the reference pose); its event graph
//! does nothing. A montage's weight blends in over its BlendIn (0.25 s), and out over the time left once that is its
//! BlendOut (0.25 s) or less; one played over another stops the old one with the new one's BlendIn. The slot's pose:
//! the montages by their weights, the reference pose by what is left (linear blends). The root bone keeps the
//! reference pose (the montages' root motion is taken out: RootMotionFromMontagesOnly; the animations' bForceRootLock
//! locks it, `sample_clip`).
//! Notifies: Ak_OneShot posts its event at the crate (the mesh's place); AnimNotify_PlayVFX (a notify state on a mesh
//! that is not a fighter's) spawns its system attached to the socket with no offset (SpawnEmitterAttached,
//! KeepRelativeOffset, scale 1) at its begin and, KillOnNotifyEnd, deactivates it at its end. The animations'
//! translations play as they are (every bone's TranslationRetargetingMode is Animation, the importer checks it).
// Unverified: a montage stopped by another (the shake over the burst) goes on to its end of blend out with its notifies
// (FAnimMontageInstance::Stop keeps a blending-out instance advancing, 4.26 source from memory), and a PlayVFX state
// whose montage is gone before its end is deactivated then.
// Deviation: the socket is on the root bone, which the montages do not move, so the fireworks are placed once at the
// socket and follow nothing. The game time is the scenery's own clock from the level's begin (GetGameTimeInSeconds
// counts from the world's start; only differences are read). RandomFloatInRange: the scenery's unseeded generator.

use std::collections::BTreeMap;

use mvs_vfx::SystemId;
use mvs_view::M4;
use mvs_view::anim::Pose;
use serde::Deserialize;
use serde_json::Value;

use super::host::SceneryHost;
use super::sky_arena::attachment_of;
use crate::Result;
use crate::stage::{bad, transform};
use crate::view::mul;

#[derive(Debug, Deserialize)]
struct Segment {
    anim: String,
    pos: f64,
    start: f64,
    end: f64,
    rate: f64,
}

#[derive(Debug, Deserialize)]
struct Notify {
    at: f64,
    end: Option<f64>,
    sound: Option<String>,
    system: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Montage {
    length: f64,
    blend_in: f64,
    blend_out: f64,
    segments: Vec<Segment>,
    notifies: Vec<Notify>,
}

#[derive(Debug, Deserialize)]
struct Socket {
    bone: String,
    loc: [f64; 3],
    rot: [f64; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrateBlock {
    skeletal: String,
    burst_delay: [f64; 2],
    shake_delay: [f64; 2],
    shake: Montage,
    burst: Montage,
    socket: BTreeMap<String, Socket>,
}

/// A montage instance on the DefaultSlot (FAnimMontageInstance): its position, its blend weight and the blend it is
/// in (from -> to over time), whether it is stopping (blending out: it plays on, its notifies with it).
#[derive(Debug)]
struct Playing {
    /// 0 the shake, 1 the burst.
    m: usize,
    pos: f64,
    weight: f64,
    from: f64,
    to: f64,
    blend: f64,
    blend_time: f64,
    out: bool,
    /// Per notify, the system of an active PlayVFX notify state.
    vfx: Vec<Option<SystemId>>,
}

#[derive(Debug)]
struct Crate {
    skeletal: String,
    burst_delay: [f64; 2],
    shake_delay: [f64; 2],
    /// The shake and the burst.
    montages: [Montage; 2],
    socket: Socket,
    playing: Vec<Playing>,
    rest: Pose,
    sample: Pose,
    acc: Pose,
    now: f64,
    next_burst: f64,
    wait: f64,
}

impl Crate {
    fn make(data: &Value, host: &mut SceneryHost) -> Result<Crate> {
        let what = "scenery trophyroom: crate";
        let d: CrateBlock = serde_json::from_value(data.clone()).map_err(|e| bad(what, e.to_string()))?;
        let k = host
            .skeletal_index(&d.skeletal)
            .ok_or_else(|| bad(what, format!("{} is not a skeletal mesh of the file", d.skeletal)))?;
        let socket = d.socket.into_iter().find(|(n, _)| n == "AcmeCrateSocket").map(|(_, s)| s);
        let socket = socket.ok_or_else(|| bad(what, "no AcmeCrateSocket"))?;
        for m in [&d.shake, &d.burst] {
            for s in &m.segments {
                let c = host.state.clips.get(&s.anim).ok_or_else(|| bad(what, format!("{} is not loaded", s.anim)))?;
                if host.state.clips.clips[c].anim.rate_scale != 1.0 {
                    return Err(bad(what, format!("{} has a RateScale", s.anim)));
                }
            }
        }
        // (the AnimBP's pose, the block's from now on: no clip)
        if let Some(a) = host.skeletal(&d.skeletal) {
            a.clip = None;
        }
        host.drive(&d.skeletal);
        let rest = host.rest(&d.skeletal).cloned().unwrap_or_default();
        let _ = k;
        let mut b = Crate {
            skeletal: d.skeletal,
            burst_delay: d.burst_delay,
            shake_delay: d.shake_delay,
            montages: [d.shake, d.burst],
            socket,
            playing: Vec::new(),
            sample: Pose::new(rest.n),
            acc: Pose::new(rest.n),
            rest,
            now: 0.0,
            next_burst: 0.0,
            wait: 0.0,
        };
        b.start(host);
        Ok(b)
    }

    /// (at the level's begin, after its effects ended: the systems are gone already)
    fn start(&mut self, host: &mut SceneryHost) {
        self.playing.clear();
        self.now = 0.0;
        self.next_burst = self.now + host.rand(self.burst_delay);
        self.queue(host);
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        self.start(host);
    }

    fn queue(&mut self, host: &mut SceneryHost) {
        if self.now >= self.next_burst {
            self.play(1);
            self.next_burst = self.now + host.rand(self.burst_delay);
            self.queue(host);
            return;
        }
        self.wait = host.rand(self.shake_delay);
    }

    fn play(&mut self, m: usize) {
        let blend_in = self.montages[m].blend_in;
        for p in self.playing.iter_mut().filter(|p| !p.out) {
            // stopped with the new one's BlendIn, from its weight now
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
            vfx: vec![None; n],
        });
    }

    /// The socket's world transform (UE, row-vector): its bone as posed, the socket's own transform under it.
    fn socket_world(&self, host: &SceneryHost) -> Option<M4> {
        let bone = host.bone_world(&self.skeletal, &self.socket.bone)?;
        let local = transform(self.socket.loc, self.socket.rot, [1.0; 3]);
        Some(mul(&local, &bone))
    }

    /// The crate's place (the mesh's component).
    fn crate_at(&self, host: &SceneryHost) -> [f64; 3] {
        let k = host.skeletal_index(&self.skeletal).expect("the crate's skeletal mesh");
        let m = host.state.skeletal_world(k);
        [m[3][0], m[3][1], m[3][2]]
    }

    /// The notifies whose time is in (from, to]: the exe's rule (FUN_1428719e0): a montage starts at 0, so a notify
    /// at or before 0 never fires.
    fn notifies(&mut self, host: &mut SceneryHost, i: usize, from: f64, to: f64) {
        let m = self.playing[i].m;
        for k in 0..self.montages[m].notifies.len() {
            let nt = &self.montages[m].notifies[k];
            if nt.at > from && nt.at <= to {
                if let Some(sound) = &nt.sound {
                    let at = self.crate_at(host);
                    host.sound(sound, at, None);
                }
                if let (Some(system), Some(w)) = (&nt.system, self.socket_world(host)) {
                    let a = attachment_of(&w);
                    self.playing[i].vfx[k] = host.vfx().and_then(|v| v.spawn_level(system, &a, false, &[], 0, false));
                }
            }
            if let Some(end) = nt.end
                && end > from
                && end <= to
                && let Some(id) = self.playing[i].vfx[k].take()
                && let Some(v) = host.vfx()
            {
                v.deactivate(id);
            }
        }
    }

    fn end_vfx(host: &mut SceneryHost, p: &mut Playing) {
        for slot in &mut p.vfx {
            if let (Some(id), Some(v)) = (slot.take(), host.vfx()) {
                v.deactivate(id);
            }
        }
    }

    /// One world tick: the wait, then the montages advance (the slot's drive, after the block's tick),
    /// blend and fire their notifies, and the pose is their weighted sum over the reference pose.
    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.now += dt;
        self.wait -= dt;
        if self.wait <= 0.0 {
            self.play(0);
            self.queue(host);
        }
        for i in (0..self.playing.len()).rev() {
            let m = self.playing[i].m;
            let length = self.montages[m].length;
            let from = self.playing[i].pos;
            let to = (from + dt).min(length);
            self.playing[i].pos = to;
            self.notifies(host, i, from, to);
            let blend_out = self.montages[m].blend_out;
            let p = &mut self.playing[i];
            // auto blend-out once the time left is the BlendOut or less, over the time left
            let left = length - p.pos;
            if !p.out && left <= blend_out {
                p.out = true;
                p.from = p.weight;
                p.to = 0.0;
                p.blend = 0.0;
                p.blend_time = left;
            }
            p.blend += dt;
            p.weight = if p.blend_time > 0.0 && p.blend < p.blend_time {
                p.from + (p.to - p.from) * (p.blend / p.blend_time)
            } else {
                p.to
            };
            if (p.out && p.weight <= 0.0) || p.pos >= length {
                let mut p = self.playing.remove(i);
                Crate::end_vfx(host, &mut p);
            }
        }
        self.write_pose(host);
    }

    fn write_pose(&mut self, host: &mut SceneryHost) {
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
                if p.pos >= x.pos {
                    s = x;
                }
            }
            let t = s.end.min(s.start + (p.pos - s.pos) * s.rate);
            // (the montages' animations are loaded with the block)
            assert!(
                host.sample(&self.skeletal, &s.anim, t, &mut self.sample),
                "scenery: the crate animation {} is not loaded",
                s.anim
            );
            add_pose(&mut self.acc, &self.sample, p.weight);
            total += p.weight;
        }
        if total < 1.0 {
            add_pose(&mut self.acc, &self.rest, 1.0 - total);
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
}

/// The stage's block of one key of its entry.
#[derive(Debug)]
pub struct TrophysEdge {
    block: Box<Crate>,
}

impl TrophysEdge {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<TrophysEdge> {
        if key != "crate" {
            return Err(bad(&format!("scenery trophyroom: {key}"), "not a Trophy's Edge block"));
        }
        Ok(TrophysEdge { block: Box::new(Crate::make(data, host)?) })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        self.block.begin(host);
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        self.block.tick(host, dt);
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        let _ = (host, alpha);
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        let _ = host;
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
    use std::sync::Arc;

    use mvs_material::jsonbin::JsonBin;
    use mvs_view::mesh::MeshInfo;

    use super::super::{SceneryHeader, SceneryHost, SceneryRun};
    use crate::scenery::SceneryBlock;

    /// The crate shakes and bursts: over a minute of ticks its pose leaves the reference pose, goes back to it once
    /// the montages are done, and the root bone keeps the reference pose.
    #[test]
    fn crate_plays_its_montages() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let Some(path) = pack.paths().find(|p| p.ends_with("/trophyroom_2platforms.trophyroom_2platforms")) else {
            eprintln!("no trophyroom scenery: nothing to check");
            return;
        };
        let path = path.to_owned();
        let bytes = pack.bytes(&path).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        assert!(matches!(run.blocks[0], SceneryBlock::TrophysEdge(_)));
        let name = run.state.skeletal[0].name.clone();
        let rest = run.state.skins[0].info.rest.clone();
        let mut sound = super::super::NullSound;
        let (mut moved, mut still, mut peak) = (0, 0, 0.0f32);
        for _ in 0..60 * 60 {
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
            run.blocks[0].tick(&mut host, 1.0 / 60.0);
            let pose = host.pose(&name).unwrap().clone();
            let d = pose.t.iter().zip(&rest.t).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            peak = peak.max(d);
            if d > 1e-4 {
                moved += 1;
            } else {
                still += 1;
            }
            // (the root bone is locked to the reference pose)
            assert_eq!(pose.t[..3], rest.t[..3]);
        }
        eprintln!("crate: {moved} ticks moved, {still} still, peak translation {peak}");
        assert!(moved > 60 && still > 60);
    }
}
