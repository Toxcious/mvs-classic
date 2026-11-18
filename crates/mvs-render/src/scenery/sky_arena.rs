//! Sky Arena (M011): the blocks its scenery entry has of its own Blueprint actors and of the Animation sub-level's
//! level Blueprint: `floating`, `arm`, `ship`.
// M011_Whip_A_01_M's world position offset needs no code here: the scenery draws
// with the cooked vertex shader, which has it (`scenery.rs`).

use std::sync::Arc;

use mvs_vfx::{Attachment, SystemId};
use serde::Deserialize;
use serde_json::Value;

use super::host::SceneryHost;
use super::{CurveKey, InstanceId, rich_curve};
use crate::Result;
use crate::stage::{bad, transform};
use crate::view::mul;
use mvs_view::M4;

/// The stage's block of one key of its entry.
#[derive(Debug)]
pub struct SkyArena {
    key: String,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Floating(Floating),
    Arm(Arm),
    Ship(Box<Ship>),
}

/// The world transform as an Attachment: its location, its rotation's rows and its scale (Matrix4.decompose: a
/// negative determinant negates the X scale).
pub(super) fn attachment_of(m: &M4) -> Attachment {
    let len = |r: &[f64; 4]| (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    let mut s = [len(&m[0]), len(&m[1]), len(&m[2])];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det < 0.0 {
        s[0] = -s[0];
    }
    let row = |i: usize| -> [f64; 3] {
        let k = if s[i] == 0.0 { 0.0 } else { 1.0 / s[i] };
        [m[i][0] * k, m[i][1] * k, m[i][2] * k]
    };
    Attachment { loc: [m[3][0], m[3][1], m[3][2]], rot: [row(0), row(1), row(2)], scale: Some(s) }
}

impl SkyArena {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<SkyArena> {
        let what = format!("scenery m011: {key}");
        let read = |e: serde_json::Error| bad(&what, e.to_string());
        let kind = match key {
            "floating" => Kind::Floating(Floating::make(serde_json::from_value(data.clone()).map_err(read)?, host)),
            "arm" => Kind::Arm(Arm::make(serde_json::from_value(data.clone()).map_err(read)?, host)),
            "ship" => Kind::Ship(Box::new(Ship::make(serde_json::from_value(data.clone()).map_err(read)?, host))),
            _ => return Err(bad(&what, "not a Sky Arena block")),
        };
        Ok(SkyArena { key: key.to_owned(), kind })
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        let _ = &self.key;
        match &mut self.kind {
            Kind::Floating(b) => b.elapsed = 0.0,
            Kind::Arm(b) => b.begin(host),
            Kind::Ship(b) => b.begin(host),
        }
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        match &mut self.kind {
            Kind::Floating(b) => b.tick(host, dt),
            Kind::Arm(b) => b.tick(host, dt),
            Kind::Ship(b) => b.tick(host, dt),
        }
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        let _ = (host, alpha);
    }

    pub fn end(&mut self, host: &mut SceneryHost) {
        let _ = host;
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FloatingBlock {
    #[serde(rename = "move")]
    move_curves: Vec<Vec<CurveKey>>,
    rotate: Vec<Vec<CurveKey>>,
    length: [f64; 2],
    offset: [f64; 2],
    move_amplitude: [f64; 2],
    rotate_amplitude: [f64; 2],
}

#[derive(Debug)]
struct Floater {
    id: InstanceId,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
    length: f64,
    offset: f64,
    move_amp: f64,
    rotate_amp: f64,
}

/// The level Blueprint's floating objects (every actor tagged Floating): InitializeFloatingAnimation at BeginPlay
/// keeps each one's location and rotation and draws its animation length, time offset and amplitudes
/// (RandomFloatInRange). AnimateFloatingObjects, from ReceiveTick: Elapsed Time += DeltaSeconds; per object the curve
/// input is ((Elapsed Time + Time Offset) % Anim Length) / Anim Length; location = initial + Move Curves x Move
/// Amplitude; rotation = ComposeRotators(MakeRotator(Roll X, Pitch Y, Yaw Z of Rotate Curves x Rotate Amplitude),
/// initial): the curve's rotation first, then the initial one.
// Deviation: RandomFloatInRange is the scenery's unseeded stream (`SceneryHost::rand`).
#[derive(Debug)]
struct Floating {
    d: FloatingBlock,
    floaters: Vec<Floater>,
    elapsed: f64,
}

impl Floating {
    fn make(d: FloatingBlock, host: &mut SceneryHost) -> Floating {
        let mut floaters: Vec<Floater> = host
            .instances()
            .iter()
            .enumerate()
            .filter(|(_, i)| i.fields.get("floating").and_then(Value::as_bool) == Some(true))
            .map(|(k, i)| Floater {
                id: InstanceId(k as u32),
                loc: i.loc,
                rot: i.rot,
                scale: i.scale,
                length: 0.0,
                offset: 0.0,
                move_amp: 0.0,
                rotate_amp: 0.0,
            })
            .collect();
        for f in &mut floaters {
            f.length = host.rand(d.length);
            f.offset = host.rand(d.offset);
            f.move_amp = host.rand(d.move_amplitude);
            f.rotate_amp = host.rand(d.rotate_amplitude);
        }
        Floating { d, floaters, elapsed: 0.0 }
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        if self.floaters.is_empty() {
            return;
        }
        self.elapsed += dt;
        let d = &self.d;
        let at = |c: &[Vec<CurveKey>], k: usize, t: f64| c.get(k).map_or(0.0, |keys| rich_curve(keys, t));
        for f in &self.floaters {
            let t = ((self.elapsed + f.offset) % f.length) / f.length;
            let loc: [f64; 3] = std::array::from_fn(|k| at(&d.move_curves, k, t) * f.move_amp + f.loc[k]);
            let a = f.rotate_amp;
            // (MakeRotator(Roll X, Pitch Y, Yaw Z), then the initial rotation: row-vector, the curve's first)
            let turn = transform(
                [0.0; 3],
                [at(&d.rotate, 1, t) * a, at(&d.rotate, 2, t) * a, at(&d.rotate, 0, t) * a],
                [1.0; 3],
            );
            let base = transform([0.0; 3], f.rot, [1.0; 3]);
            let mut m = mul(&turn, &base);
            for (i, row) in m.iter_mut().take(3).enumerate() {
                for v in row.iter_mut().take(3) {
                    *v *= f.scale[i];
                }
            }
            m[3] = [loc[0], loc[1], loc[2], 1.0];
            host.place(f.id, Some(m));
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArmBlock {
    skeletal: String,
    delay: [f64; 2],
    anim: String,
    bitten_at: f64,
}

/// M011_Arm_C: BeginPlay waits RandomFloatInRange, then plays the grab animation once on the Mesh (PlayAnimation, not
/// looping: it holds its last frame) and, each tick until it is true, swaps the donut for the bitten one when the
/// animation's curve show_DonutBitten_01 is over 0.9 (bittenAt: the time of the curve's key). The actor ticks before
/// its mesh's animation, so the test reads the previous tick's position. Its Donut and DonutBitten are the instances
/// attached to a bone of its Mesh (DonutBitten hidden).
#[derive(Debug)]
struct Arm {
    d: ArmBlock,
    /// 0 waiting, 1 grabbing, 2 bitten; 2 with no mesh to play.
    state: u8,
    wait: f64,
    /// The Donut instances and the DonutBitten ones.
    plain: Vec<InstanceId>,
    bitten: Vec<InstanceId>,
    /// The mesh's clip and loop as the file has them.
    start: Option<(Option<Arc<str>>, bool)>,
}

impl Arm {
    fn make(d: ArmBlock, host: &mut SceneryHost) -> Arm {
        let start = host.skeletal(&d.skeletal).map(|a| (a.clip.clone(), a.looping));
        let (mut plain, mut bitten) = (Vec::new(), Vec::new());
        for (k, i) in host.instances().iter().enumerate() {
            let parent = i.attach.as_ref().and_then(|a| a.get(0)).and_then(Value::as_str);
            if parent == Some(d.skeletal.as_str()) {
                if i.hidden { bitten.push(InstanceId(k as u32)) } else { plain.push(InstanceId(k as u32)) }
            }
        }
        let wait = host.rand(d.delay);
        Arm { state: if start.is_some() { 0 } else { 2 }, d, wait, plain, bitten, start }
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        let Some((clip, looping)) = self.start.clone() else { return };
        if let Some(a) = host.skeletal(&self.d.skeletal) {
            a.clip = clip;
            a.looping = looping;
        }
        if self.state == 2 {
            host.show(&self.plain, true);
            host.show(&self.bitten, false);
        }
        self.state = 0;
        self.wait = host.rand(self.d.delay);
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        let Some(a) = host.skeletal(&self.d.skeletal) else { return };
        let time = a.time;
        if self.state == 0 {
            self.wait -= dt;
            if self.wait > 0.0 {
                return;
            }
            a.clip = Some(Arc::from(self.d.anim.as_str()));
            a.looping = false;
            a.time = 0.0;
            self.state = 1;
        } else if self.state == 1 && time >= self.d.bitten_at {
            host.show(&self.plain, false);
            host.show(&self.bitten, true);
            self.state = 2;
        }
    }
}

#[derive(Debug, Deserialize)]
struct Effect {
    system: String,
    at: Vec<f64>,
    sound: String,
}

#[derive(Debug, Deserialize)]
struct Thrust {
    system: String,
    at: Vec<f64>,
    sound: String,
    stop: Vec<f64>,
    offset: [f64; 3],
}

#[derive(Debug, Deserialize)]
pub(super) struct ShipBlock {
    delay: [f64; 2],
    length: f64,
    #[serde(rename = "move")]
    move_curves: Vec<Vec<CurveKey>>,
    scale: Vec<Vec<CurveKey>>,
    rings: Effect,
    thrust: Thrust,
}

/// M011_Ship_C: hidden at BeginPlay; the level Blueprint calls Animate once, RandomFloatInRange seconds after
/// BeginPlay: the Mesh shows and the timeline plays. Each timeline update adds the MoveRoamingEye track's value to
/// the Mesh's relative location (K2_AddLocalOffset: per tick, not per second) and sets its relative scale to the
/// ScaleRoamingEye track's; at the end the Mesh hides.
/// The timeline's event tracks: PlayRings spawns the rings system at the Mesh's world transform
/// (SpawnEmitterAtLocation), PlayThrust the thruster attached to the Mesh at its offset (SpawnEmitterAttached,
/// SnapToTargetIncludingScale), StopThrust and the timeline's end destroy the thruster component. An event fires in
/// the tick the position passes its key (key time in [old position, new position)), before that tick's update
/// (Unverified: FTimeline::SetPlaybackPosition from the 4.26 source, from memory): the Mesh is where the previous
/// tick left it.
// Deviation: RandomFloatInRange is the scenery's unseeded stream.
#[derive(Debug)]
struct Ship {
    d: ShipBlock,
    meshes: Vec<InstanceId>,
    /// The Mesh's place in the actor's root (UE, row-vector).
    root: M4,
    /// The Mesh's world transform as the last tick left it.
    world: M4,
    /// 0 waiting, 1 playing, 2 done, 3 no mesh.
    state: u8,
    wait: f64,
    pos: f64,
    offset: [f64; 3],
    thruster: Option<SystemId>,
}

/// A key time in [from, to).
fn passed(keys: &[f64], from: f64, to: f64) -> bool {
    keys.iter().any(|&k| k >= from && k < to)
}

impl Ship {
    fn make(d: ShipBlock, host: &mut SceneryHost) -> Ship {
        let mut meshes = Vec::new();
        let mut root = transform([0.0; 3], [0.0; 3], [1.0; 3]);
        for (k, i) in host.instances().iter().enumerate() {
            if i.fields.get("ship").and_then(Value::as_bool) == Some(true) {
                meshes.push(InstanceId(k as u32));
                root = transform(i.loc, i.rot, i.scale);
            }
        }
        // (BeginPlay hides it)
        host.show(&meshes, false);
        let state = if meshes.is_empty() { 3 } else { 0 };
        let wait = host.rand(d.delay);
        Ship { d, meshes, root, world: root, state, wait, pos: 0.0, offset: [0.0; 3], thruster: None }
    }

    fn begin(&mut self, host: &mut SceneryHost) {
        if self.meshes.is_empty() {
            return;
        }
        if self.state == 1 {
            host.show(&self.meshes, false);
        }
        // (ended with the level)
        self.thruster = None;
        self.world = self.root;
        self.state = 0;
        self.wait = host.rand(self.d.delay);
    }

    /// The Mesh's world transform at `off` in its own space, as an Attachment.
    fn at(&self, off: [f64; 3]) -> Attachment {
        let w = &self.world;
        let mut a = attachment_of(w);
        let p: [f64; 3] = std::array::from_fn(|c| off[0] * w[0][c] + off[1] * w[1][c] + off[2] * w[2][c] + w[3][c]);
        a.loc = p;
        a
    }

    fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        if self.state == 0 {
            self.wait -= dt;
            if self.wait > 0.0 {
                return;
            }
            self.offset = [0.0; 3];
            self.pos = 0.0;
            host.show(&self.meshes, true);
            self.state = 1;
            return;
        }
        if self.state != 1 {
            return;
        }
        let (from, pos) = (self.pos, (self.pos + dt).min(self.d.length));
        self.pos = pos;
        if passed(&self.d.rings.at, from, pos) {
            let a = self.at([0.0; 3]);
            if let Some(v) = host.vfx() {
                v.spawn_level(&self.d.rings.system, &a, true, &[], 0, false);
            }
            host.sound(&self.d.rings.sound, a.loc, None);
        }
        if passed(&self.d.thrust.at, from, pos) {
            let a = self.at(self.d.thrust.offset);
            self.thruster = host.vfx().and_then(|v| v.spawn_level(&self.d.thrust.system, &a, false, &[], 0, false));
            host.sound(&self.d.thrust.sound, self.at([0.0; 3]).loc, None);
        }
        if passed(&self.d.thrust.stop, from, pos) {
            self.kill_thruster(host);
        }
        let at = |c: &[Vec<CurveKey>], k: usize| c.get(k).map_or(0.0, |keys| rich_curve(keys, pos));
        for k in 0..3 {
            self.offset[k] += at(&self.d.move_curves, k);
        }
        let scale = [at(&self.d.scale, 0), at(&self.d.scale, 1), at(&self.d.scale, 2)];
        let local = transform(self.offset, [0.0; 3], scale);
        self.world = mul(&local, &self.root);
        for &id in &self.meshes {
            host.place(id, Some(self.world));
        }
        if pos >= self.d.length {
            host.show(&self.meshes, false);
            self.kill_thruster(host);
            self.state = 2;
            return;
        }
        // (the thruster follows the Mesh: VfxPlayer's follow, before its tick)
        if let Some(id) = self.thruster {
            let a = self.at(self.d.thrust.offset);
            match host.vfx() {
                Some(v) if v.alive(id) => v.attach(id, &a),
                _ => self.thruster = None,
            }
        }
    }

    fn kill_thruster(&mut self, host: &mut SceneryHost) {
        if let (Some(id), Some(v)) = (self.thruster.take(), host.vfx()) {
            v.kill(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_material::jsonbin::JsonBin;
    use mvs_view::mesh::MeshInfo;

    use super::super::{NullSound, SceneryHeader, SceneryHost, SceneryRun};

    /// The floating objects move, the arm's donut is swapped for the bitten one once its grab passes the curve, the
    /// ship shows for its timeline and hides again.
    #[test]
    fn m011_blocks_run() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let pack = store.kind("scenery").unwrap();
        let Some(path) = pack.paths().find(|p| p.ends_with("/m011.m011")).map(str::to_owned) else {
            eprintln!("no m011 scenery: nothing to check");
            return;
        };
        let bytes = pack.bytes(&path).unwrap();
        let s = JsonBin::split(b"MVSC", &bytes, &path).unwrap().parse::<SceneryHeader>(&path).unwrap().scenery;
        let meshes = s.skeletal.iter().map(|k| Arc::new(MeshInfo::load(&store, &k.mesh).unwrap())).collect();
        let mut run = SceneryRun::new(&store, 0, Vec::new(), s.instances, s.skeletal, meshes, &s.rest).unwrap();
        assert_eq!(run.blocks.len(), 3);
        let n = run.state.instances.len();
        let floating = (0..n).filter(|&i| run.state.instances[i].fields.get("floating").is_some()).count();
        let ship = (0..n).find(|&i| run.state.instances[i].fields.get("ship").is_some()).unwrap();
        let donut = (0..n).find(|&i| run.state.instances[i].name.ends_with(".Donut")).unwrap();
        let bitten = (0..n).find(|&i| run.state.instances[i].name.ends_with(".DonutBitten")).unwrap();
        assert!(floating > 10, "{floating} floating instances");
        assert!(!run.state.shown[ship] && run.state.shown[donut] && !run.state.shown[bitten]);
        let mut sound = NullSound;
        {
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
            for b in &mut run.blocks {
                b.begin(&mut host);
            }
        }
        let (mut ship_on, mut ship_off_after) = (0, false);
        for _ in 0..60 * 200 {
            let mut host = SceneryHost { state: &mut run.state, vfx: None, sound: &mut sound, world: None };
            for b in &mut run.blocks {
                b.tick(&mut host, 1.0 / 60.0);
            }
            run.state.tick_skeletal(1.0 / 60.0);
            if run.state.shown[ship] {
                ship_on += 1;
            } else if ship_on > 0 {
                ship_off_after = true;
            }
        }
        let moved = run.state.placed.iter().filter(|p| p.is_some()).count();
        eprintln!("m011: {moved} placed, ship shown {ship_on} ticks, donut {}", run.state.shown[donut]);
        assert!(moved > floating);
        assert!(ship_on > 0 && ship_off_after, "the ship shows and hides");
        assert!(!run.state.shown[donut] && run.state.shown[bitten], "the arm bit the donut");
    }
}
