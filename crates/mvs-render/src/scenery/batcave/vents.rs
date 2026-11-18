//! Batcave's vents as the level script drives them (mvs-sim stages/batcave/vents.rs): the two fogs
//! (FogOcclusionBP_C, the block `batcave`) and the two fans' blades (the
//! block `batcaveMotion`).
//! - A fog: its `particle` going to 1 (ActivateFogVisuals @6992): PS_SmokeNew at its place, the one before
//!   deactivated (@7507), sfx_map_batcave_steam_occlusion on the fog (@7562); from 1 (DeactivateFogVisuals @6586):
//!   mix_stop_all_fade_onesecond on the fog (@6704), then to 2 (dissolving): Dissolve 1 (@6822) and Deactivate
//!   (@6876), to 0: destroyed (@15). The fog's own game object is a sound key of its own. Its smoke is spawned by a
//!   timer: first ticked the next frame.
//! - The fans (Batcave_FanBlades_C), in the sim's VentsState.fans order (the left cycle's Batcave_FanBlades_2, the
//!   right's Batcave_FanBlades2). BeginPlay @176: AdjustedRotationRate = RotationRate (60). ReceiveTick @208..505:
//!   Rotation -= DeltaSeconds x AdjustedRotationRate, + 360 when <= -360, and the blade's relative rotation set to
//!   MakeRotator(Roll = Rotation, 0, 0): a roll about the blade's own X under the actor's root (the blade's template
//!   has no relative transform, so its world transform is the instance's x the roll).
//! - The speed-up: the level script's StartSpeedup / StopSpeedup (Map_Batcave @1190 / @68 left, @5106 / @284 right)
//!   are the sim's VentsState.fans going true / false, read from the World each tick. StartSpeedup @562:
//!   SpawnEmitterAtLocation(PS_Batcave_VentAir, the level's FanBlade(s|2)EmitterPosition / Rotation, scale 1, auto
//!   destroy), then @139 SpeedUp.PlayFromStart; StopSpeedup @632 -> @102 ReverseFromEnd. The update @15:
//!   AdjustedRotationRate = Lerp(RotationRate 60, SmokeRotationRate 420, Alpha), Alpha the timeline's linear curve
//!   (keys (-0.002135247, 0), (1, 1), length 1): from the start it is 0.00213, so a stopped fan keeps turning at 60.77
//!   deg/s after its first speed-up, as the build's does.
//!   The timeline's tick (FTimeline::TickTimeline 142e46330): the position moves by PlayRate x dt in float32 and the
//!   update fires; it finishes only when the new position goes strictly past the length (or below 0 reversed),
//!   clamped there. The level timer that starts or stops it runs after the tick groups, so a new rate turns the blade
//!   from the next tick. The vent air spawned in the timer pass first ticks the frame after.
//! - A world first seen with a fan already sped up (the scenery joining a match under way) takes it at full speed
//!   with no vent air; a rollback that undoes a start is seen as a stop (the timeline reversed), as nothing in the sim
//!   holds the timeline.
//! - The drawn roll is the latest two ticks' blended by the frame's alpha (as the platforms').
// Deviation: the sim keeps no tick of a fog's activation, so a second activation while one is active (a state that
// does not change) draws and posts nothing, and a destroy by the dissolve's Delay (@6926 -> @15) after a new
// activation posts the stop as a deactivation does.
// Unverified: PlayFromStart / ReverseFromEnd setting the position (0 / the length) and firing the update at once is
// 4.26 source, no exe read; the timeline ticks in TG_PrePhysics with no owner prerequisite (UTimelineComponent ctor),
// so its order against the fan's own tick is not fixed by code: taken as after it.

use mvs_sim::stages::batcave::BatcaveFog;
use mvs_sim::world::stage::StageOwn;
use mvs_vfx::{Attachment, SystemId};
use mvs_view::M4;
use serde::Deserialize;
use serde_json::Value;

use super::super::host::{InstanceId, SceneryHost};
use crate::Result;
use crate::scenery::the_court::rows_of;
use crate::stage::{bad, transform};
use crate::view::mul;

/// A fog of the block's `fogs`.
#[derive(Debug, Deserialize)]
pub(super) struct FogData {
    pub(super) actor: String,
    system: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
    /// The root's place (UE): where its sounds are posted.
    root: [f64; 3],
    sound: String,
    stop: String,
}

/// A fog as it is drawn.
#[derive(Debug)]
pub(super) struct Fog {
    pub(super) d: FogData,
    at: Attachment,
    /// Its sound key (`batcaveFog.<actor>`).
    key: String,
    fx: Option<SystemId>,
    /// The sim's `particle` last seen.
    particle: i32,
}

impl Fog {
    pub(super) fn new(d: FogData) -> Fog {
        Fog {
            at: Attachment { loc: d.loc, rot: rows_of(d.rot), scale: Some(d.scale) },
            key: format!("batcaveFog.{}", d.actor),
            fx: None,
            particle: 0,
            d,
        }
    }

    /// The level begins: the systems of the level before are gone (VfxPlayer.endAll).
    pub(super) fn begin(&mut self) {
        self.fx = None;
        self.particle = 0;
    }

    /// One tick of the World's fog `a` (None: the World has no such actor, its particle 0).
    pub(super) fn tick(&mut self, host: &mut SceneryHost, a: Option<&BatcaveFog>) {
        let p = a.map_or(0, |a| a.particle);
        if p == self.particle {
            return;
        }
        if p == 1 {
            if let (Some(id), Some(v)) = (self.fx, host.vfx()) {
                v.deactivate(id);
            }
            self.fx = host.vfx().and_then(|v| v.spawn_level(&self.d.system, &self.at, true, &[], 0, false));
            host.sound(&self.d.sound, self.d.root, Some(&self.key));
        } else {
            if self.particle == 1 {
                host.sound(&self.d.stop, self.d.root, Some(&self.key));
            }
            if p == 2 {
                if let (Some(id), Some(v)) = (self.fx, host.vfx()) {
                    if a.is_some_and(|a| a.dissolve) {
                        v.set_param(id, "Dissolve", 1.0);
                    }
                    v.deactivate(id);
                }
            } else if let (Some(id), Some(v)) = (self.fx.take(), host.vfx()) {
                v.kill(id);
            }
        }
        self.particle = p;
    }
}

type V3 = [f64; 3];

#[derive(Debug, Deserialize)]
struct Emitter {
    loc: V3,
    rot: V3,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FanData {
    instance: String,
    rotation_rate: f64,
    smoke_rate: f64,
    rotation: f64,
    emitter: Emitter,
}

#[derive(Debug, Deserialize)]
struct SpeedUp {
    length: f64,
    keys: Vec<[f64; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MotionData {
    fans: Vec<FanData>,
    speed_up: SpeedUp,
    vent_air: String,
}

#[derive(Debug)]
struct Fan {
    d: FanData,
    id: InstanceId,
    /// The blade's UE transform at the data place (no roll).
    base: M4,
    /// Rotation (deg) after the latest tick, and after the tick before.
    rot: f64,
    prev: f64,
    /// AdjustedRotationRate (deg/s).
    rate: f64,
    /// The SpeedUp timeline's position (s), and its direction: forward 1, reversed -1, stopped 0.
    pos: f64,
    dir: f64,
    /// The sim's fan state last seen, None before the first World.
    on: Option<bool>,
}

/// The SpeedUp curve at `t` (linear keys, held past either end).
fn curve(keys: &[[f64; 2]], t: f64) -> f64 {
    if t <= keys[0][0] {
        return keys[0][1];
    }
    for i in 1..keys.len() {
        let [t1, v1] = keys[i];
        if t <= t1 {
            let [t0, v0] = keys[i - 1];
            return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
        }
    }
    keys[keys.len() - 1][1]
}

/// x rounded to float32 (Math.fround).
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}

/// The `batcaveMotion` block: the vents' fans.
#[derive(Debug)]
pub struct BatcaveMotion {
    fans: Vec<Fan>,
    length: f64,
    keys: Vec<[f64; 2]>,
    vent_air: String,
}

impl BatcaveMotion {
    pub fn make(key: &str, data: &Value, host: &mut SceneryHost) -> Result<BatcaveMotion> {
        let what = format!("scenery batcave: {key}");
        let d: MotionData = serde_json::from_value(data.clone()).map_err(|e| bad(&what, e.to_string()))?;
        if d.speed_up.keys.is_empty() {
            return Err(bad(&what, "the SpeedUp curve has no key"));
        }
        let fans = d
            .fans
            .into_iter()
            .map(|fd| {
                let mut found = host.instances().iter().enumerate().filter(|(_, i)| i.name == fd.instance);
                let Some((k, i)) = found.next() else {
                    return Err(bad(&what, format!("batcave fan {} not in the file", fd.instance)));
                };
                if found.next().is_some() {
                    return Err(bad(&what, format!("batcave fan {} twice", fd.instance)));
                }
                if i.attach.is_some() {
                    return Err(bad(&what, format!("batcave fan {} is attached", fd.instance)));
                }
                Ok(Fan {
                    id: InstanceId(k as u32),
                    base: transform(i.loc, i.rot, i.scale),
                    rot: fd.rotation,
                    prev: fd.rotation,
                    rate: fd.rotation_rate,
                    pos: 0.0,
                    dir: 0.0,
                    on: None,
                    d: fd,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(BatcaveMotion { fans, length: d.speed_up.length, keys: d.speed_up.keys, vent_air: d.vent_air })
    }

    /// Fan `k`'s AdjustedRotationRate now.
    #[cfg(test)]
    pub(crate) fn rate(&self, k: usize) -> f64 {
        self.fans[k].rate
    }

    fn update(keys: &[[f64; 2]], f: &mut Fan) {
        let a = curve(keys, f.pos);
        f.rate = f.d.rotation_rate + (f.d.smoke_rate - f.d.rotation_rate) * a;
    }

    /// The blade at the roll `deg`: the instance's transform x the roll (a roll about the blade's own X).
    fn draw(host: &mut SceneryHost, f: &Fan, deg: f64) {
        host.place(f.id, Some(mul(&transform([0.0; 3], [0.0, 0.0, deg], [1.0; 3]), &f.base)));
    }

    pub fn begin(&mut self, host: &mut SceneryHost) {
        for f in &mut self.fans {
            f.rot = f.d.rotation;
            f.prev = f.d.rotation;
            // BeginPlay @176
            f.rate = f.d.rotation_rate;
            f.pos = 0.0;
            f.dir = 0.0;
            f.on = None;
            BatcaveMotion::draw(host, f, f.rot);
        }
    }

    pub fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        // (the stage state's vents field)
        let vents = match host.world.and_then(|w| w.stage_run.as_ref()).map(|r| &r.s) {
            Some(StageOwn::Batcave(s)) => Some(s.vents.fans),
            _ => None,
        };
        for i in 0..self.fans.len() {
            let f = &mut self.fans[i];
            // ReceiveTick @208..505
            f.prev = f.rot;
            let mut r = fround(f.rot - dt * f.rate);
            if r <= -360.0 {
                r = fround(r + 360.0);
            }
            f.rot = r;
            // the SpeedUp timeline's tick
            if f.dir != 0.0 {
                f.pos = fround(f.pos + dt * f.dir);
                if f.pos > self.length {
                    f.pos = self.length;
                    f.dir = 0.0;
                } else if f.pos < 0.0 {
                    f.pos = 0.0;
                    f.dir = 0.0;
                }
                BatcaveMotion::update(&self.keys, f);
            }
            // the level script's StartSpeedup / StopSpeedup (its timers and Delays, after the tick groups)
            let Some(on) = vents.map(|v| v.get(i).copied().unwrap_or(false)) else { continue };
            if f.on == Some(on) {
                continue;
            }
            match f.on {
                None => {
                    if on {
                        f.pos = self.length;
                        BatcaveMotion::update(&self.keys, f);
                    }
                }
                Some(_) if on => {
                    // @562
                    let e = &f.d.emitter;
                    let at = Attachment { loc: e.loc, rot: rows_of(e.rot), scale: Some([1.0; 3]) };
                    if let Some(v) = host.vfx() {
                        v.spawn_level(&self.vent_air, &at, true, &[], 0, false);
                    }
                    // PlayFromStart @139
                    f.pos = 0.0;
                    f.dir = 1.0;
                    BatcaveMotion::update(&self.keys, f);
                }
                Some(_) => {
                    // ReverseFromEnd @102
                    f.pos = self.length;
                    f.dir = -1.0;
                    BatcaveMotion::update(&self.keys, f);
                }
            }
            f.on = Some(on);
        }
    }

    pub fn frame(&mut self, host: &mut SceneryHost, alpha: f64) {
        for f in &self.fans {
            let mut d = f.rot - f.prev;
            if d > 180.0 {
                d -= 360.0;
            } else if d < -180.0 {
                d += 360.0;
            }
            BatcaveMotion::draw(host, f, f.prev + d * alpha);
        }
    }

    pub fn end(&mut self, _host: &mut SceneryHost) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SpeedUp curve: held before its first key and past its last, linear between.
    #[test]
    fn speed_up_curve() {
        let keys = [[-0.002135247, 0.0], [1.0, 1.0]];
        assert_eq!(curve(&keys, -1.0), 0.0);
        assert_eq!(curve(&keys, 2.0), 1.0);
        let a = curve(&keys, 0.0);
        assert!((a - 0.002130698).abs() < 1e-8, "{a}");
        // (a stopped fan after its first speed-up: 60.77 deg/s)
        assert!((60.0 + 360.0 * a - 60.767).abs() < 1e-3);
    }
}
