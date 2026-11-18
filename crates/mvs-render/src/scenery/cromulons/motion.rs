//! Cromulons' presentation that moves (the `cromulons` block,
//! ../cromulons.rs, draws through this). The speakers' drawing is not here.
//! - Comp_Parallax (../parallax.rs) of the seven heads (Cromulon_Head_C), the two helicopters
//!   (SchwiftyHelicopter_Blueprint_C), each a skeletal mesh that moves with its actor, and of the Background level's
//!   Sky_BGCard (`parallax` on the instance). The level's other Comp_Parallax owners draw nothing: Cube2 / Cube_5,
//!   the 13 clouds of BackCloudGroup1-3, DiscoBall_Legacy and Planet_Rings_1 (Large), Planet_8, planets_01_14 and
//!   Schwifty_Sky_2 (Small) are bHidden, so the level script's cloud move (@15..3594: ActorStartLocation.Y +=
//!   DeltaSeconds x speed, back to 14000 at CloudDistance) moves nothing that is drawn and is left out.
//! - MoveHelicopter (SchwiftyHelicopter_Blueprint, each tick, not scaled by DeltaSeconds): ActorStartLocation +=
//!   HelicopterVelocity (@0..68); then with the actor's location Y (K2_GetActorLocation @117) below HeliRightBound
//!   the start goes to (X, HeliLeftBound, RandomFloatInRange(HeightMin, HeightMax)) (@205..426), else above
//!   HeliLeftBound to (X, HeliRightBound, the same random) (@568..789); equal does not wrap. Their single-node
//!   animation plays from the file.
//! - Head2's idle (Map_M009 / Map_M009_small @1107..1307): BeginPlay polls GameState.HasMatchStarted, and while
//!   false waits Delay(1.0) (resume @1107) and polls again; once true Head2.Head.PlayAnimation(Cromulun_Head_Idle_01,
//!   looping) (@1188). The file has Head2 paused at 0 (bSavedPlaying false: rate 0); it plays at rate 1
//!   (SavedPlayRate unset). The level's BeginPlay runs inside HandleMatchIsWaitingToStart, before StartMatch: the
//!   first poll is false.
//! - The audio visualizer (CromulonAudioVisualizer Tick @1739..1425): the level script's tick (@4300..4916) copies the
//!   global game parameters mus_freq_analyzer_01..04 (GetRTPCValue, the music's playing id; the four meters on the
//!   music's user aux buses write them, eScope Global: SceneryHost::rtpc) into Frequencys[0..3]. The visualizer's
//!   tick then sets, for each of its BeatMeshes in key order (the construction order: `visualizer` on the instance),
//!   Scroll = max(0.1, 0.1 x trunc(10 x ((F + 48) / 48) x Lerp(0.75, 1, |i + 1 - M / 2| / (M / 2)))),
//!   F = Frequencys[i % 4], M the number of planes (M / 2 an integer division). The meters keep F in their range
//!   [-48, 0], so trunc(...) is 0..10: Scroll takes ten values; a plane's instance takes the one of its value when it
//!   changes; before the first tick the planes keep the material's own (Scroll 0.5).
//! - Small's LEGACY_Schwifty_Ground (Schwifty_Ground_Skeleton_AnimBlueprint_C; its FrontLocation / BackLocation from
//!   Cube2 / Cube_5, @4995..5283) is bHidden with its component bVisible false: not drawn, so its AnimBP is not
//!   ported; the large level's reference is None.
// Deviation: RandomFloatInRange is the scenery's unseeded stream (SceneryHost::rand: scenery is no part of the
// simulation); the build's is the global FMath::FRand stream, no seed of the match's.
// Unverified: the actor's location MoveHelicopter reads is taken as its Comp_Parallax's move of the tick before (the
// start before this tick's add, at the camera offset of the last drawn frame): the order of the actor's tick and its
// component's is not read.
// Unverified: ReadyToStartMatch (native, no Blueprint override) is taken true from the first tick on, so the second
// poll, 1 s after BeginPlay, starts Head2's idle.
// Unverified: the analyzers' value by playing id taken as the global one; the level script's tick taken before the
// visualizer's (same tick).
// Deviation: the port's music plays into its output bus alone, without its user aux sends, so those meters never
// run: a parameter nothing has set is taken as the meters' floor, fMin -48 (what a running meter writes in silence;
// the build would read the InitBank default 0 only before its music starts), and every plane shows Scroll 0.1 until
// the sends are ported.
// Deviation: a second begin does not put the planes back to the material's own Scroll before the first tick (the
// host has no call that drops an instance's scalar): they keep the last tick's value for the frames before it.

use serde::Deserialize;
use serde_json::Value;

use super::super::host::{InstanceId, SceneryHost};
use super::super::parallax::{ParallaxActor, ParallaxCamera};
use crate::Result;
use crate::stage::{bad, transform};

/// The level's Delay between two HasMatchStarted polls.
const POLL: f64 = 1.0;
/// The level script's game parameters (Frequencys[0..3]).
pub(super) const ANALYZERS: [&str; 4] =
    ["mus_freq_analyzer_01", "mus_freq_analyzer_02", "mus_freq_analyzer_03", "mus_freq_analyzer_04"];
/// The analyzer buses' AkMeterFX fMin (the InitBank's buses 64502571..74).
pub(super) const METER_MIN: f64 = -48.0;

/// MoveHelicopter's values of a helicopter and the AkEvent of its Env_Audio_Emit child.
#[derive(Clone, Debug, Deserialize)]
pub(super) struct Helicopter {
    pub(super) skeletal: String,
    velocity: [f64; 3],
    height: [f64; 2],
    bounds: [f64; 2],
    #[serde(default)]
    event: Option<String>,
}

/// A head of the block's: its skeletal mesh, `start` for the one the level starts (Head2).
#[derive(Clone, Debug)]
pub(super) struct MotionHead {
    pub(super) skeletal: String,
    pub(super) start: bool,
}

/// A helicopter as it moves: its Comp_Parallax owner (an index into `owners`) and its sound's game object and place.
#[derive(Debug)]
struct Heli {
    h: Helicopter,
    p: usize,
    key: String,
    at: Option<[f64; 3]>,
}

/// A visualizer plane: its instances and its Scroll step drawn (-1: the material's own).
#[derive(Debug)]
struct Plane {
    ids: Vec<InstanceId>,
    step: i32,
}

/// The block's moving parts.
#[derive(Debug)]
pub(super) struct Motion {
    cam: ParallaxCamera,
    owners: Vec<ParallaxActor>,
    helis: Vec<Heli>,
    posted: bool,
    /// Head2: the heads the level starts.
    start_heads: Vec<String>,
    /// The Delay left before the next poll; < 0 once the idle plays.
    wait: f64,
    planes: Vec<Plane>,
    /// Frequencys (the class default 0).
    freq: [f64; 4],
    half: i64,
}

/// A `parallax: [Y, Z]` field.
fn parallax_of(fields: &serde_json::Map<String, Value>, what: &str) -> Result<Option<[f64; 2]>> {
    let Some(v) = fields.get("parallax") else { return Ok(None) };
    match v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
        Some(&[y, z]) => Ok(Some([y, z])),
        _ => Err(bad(what, format!("parallax {v}"))),
    }
}

impl Motion {
    /// The instances and skeletal meshes it moves, claimed from the entry (holder, ready).
    pub(super) fn make(
        heads: &[MotionHead],
        helicopters: &[Helicopter],
        planes: usize,
        host: &mut SceneryHost,
    ) -> Result<Motion> {
        let what = "scenery: cromulons";
        let mut owners: Vec<ParallaxActor> = Vec::new();
        // the instances: the visualizer's planes, the parallax owners
        let mut plane_list: Vec<Plane> = (0..planes).map(|_| Plane { ids: Vec::new(), step: -1 }).collect();
        let mut plane_key: Option<String> = None;
        let mut moved = Vec::new();
        for (k, inst) in host.instances().iter().enumerate() {
            let id = InstanceId(k as u32);
            if let Some(v) = inst.fields.get("visualizer") {
                let pl = v.as_u64().and_then(|v| plane_list.get_mut(v as usize));
                let mat = match inst.materials.as_slice() {
                    [Some(m)] => Some(m),
                    _ => None,
                };
                let (Some(pl), Some(mat)) = (pl, mat) else {
                    return Err(bad(what, format!("visualizer plane {}", inst.name)));
                };
                if plane_key.as_ref().is_some_and(|p| p != mat) {
                    return Err(bad(what, format!("visualizer plane {}", inst.name)));
                }
                plane_key = Some(mat.clone());
                pl.ids.push(id);
                continue;
            }
            let Some(par) = parallax_of(&inst.fields, &inst.name)? else { continue };
            moved.push((inst.name.clone(), inst.loc, par, Some((id, transform(inst.loc, inst.rot, inst.scale)))));
        }
        // the heads' and helicopters' skeletal meshes (their Comp_Parallax moves the whole actor)
        for k in host.skeletals() {
            let ours = heads.iter().any(|h| h.skeletal == k.name) || helicopters.iter().any(|h| h.skeletal == k.name);
            if !ours {
                continue;
            }
            let Some(par) = parallax_of(&k.fields, &k.name)? else { continue };
            moved.push((k.name.clone(), k.loc, par, None));
        }
        let mut helis = Vec::new();
        for (name, loc, par, inst) in moved {
            if owners.iter().any(|o| o.name == name) {
                return Err(bad(what, format!("parallax {name} twice")));
            }
            owners.push(ParallaxActor::new(&name, loc, par));
            let p = owners.len() - 1;
            match inst {
                Some((id, base)) => owners[p].add(id, base),
                None => {
                    let k = host.skeletal_index(&name).expect("a skeletal mesh of the entry");
                    let s = &host.skeletals()[k];
                    owners[p].add_skeletal(&name, transform(s.loc, s.rot, s.scale));
                    if let Some(h) = helicopters.iter().find(|h| h.skeletal == name) {
                        helis.push(Heli { h: h.clone(), p, key: String::new(), at: None });
                    }
                }
            }
        }
        if helis.len() != helicopters.len() {
            return Err(bad(what, "a helicopter without its Comp_Parallax"));
        }
        // (in the block's order)
        helis.sort_by_key(|x| helicopters.iter().position(|h| h.skeletal == x.h.skeletal));
        if plane_list.iter().any(|p| p.ids.is_empty()) {
            return Err(bad(what, "visualizer planes"));
        }
        let mut start_heads = Vec::new();
        for h in heads.iter().filter(|h| h.start) {
            match host.skeletal(&h.skeletal) {
                Some(a) if a.rate == 0.0 && a.clip.is_some() => start_heads.push(h.skeletal.clone()),
                _ => return Err(bad(what, format!("{} is not a paused animation", h.skeletal))),
            }
        }
        Ok(Motion {
            cam: ParallaxCamera::default(),
            owners,
            helis,
            posted: false,
            start_heads,
            wait: POLL,
            planes: plane_list,
            freq: [0.0; 4],
            half: (planes / 2) as i64,
        })
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        self.cam.begin(host);
        for o in &mut self.owners {
            o.begin();
        }
        for h in &self.start_heads {
            if let Some(a) = host.skeletal(h) {
                a.rate = 0.0;
            }
        }
        self.wait = POLL;
        self.posted = false;
        self.freq = [0.0; 4];
        for pl in &mut self.planes {
            pl.step = -1;
        }
    }

    /// The child Env_Audio_Emit's ManualStart (the helicopter's AkEvent on its own AkComponent, which moves with the
    /// actor: `frame` places it).
    // Unverified: posted at the first tick with a World, not at BeginPlay (the scenery has no World at begin); the
    // stage data's emitter is the one whose actor is the helicopter's.
    fn post(&mut self, host: &mut SceneryHost) {
        let Some(w) = host.world() else { return };
        self.posted = true;
        let emitters = w.data.stage.audio.as_ref().map_or(&[][..], |a| a.emitters.as_slice());
        let mut posts = Vec::new();
        for x in &mut self.helis {
            x.at = None;
            let Some(event) = &x.h.event else { continue };
            let e = emitters
                .iter()
                .find(|m| m.actor.as_deref() == Some(&x.h.skeletal))
                .unwrap_or_else(|| panic!("scenery: cromulons helicopter {} without its Env_Audio_Emit", x.h.skeletal));
            x.key = format!("{}.{}", e.actor.as_deref().unwrap_or(""), e.component.as_deref().unwrap_or(""));
            let at = [e.at[0], e.at[1], e.at[2]];
            x.at = Some(at);
            posts.push((event.clone(), x.key.clone(), at));
        }
        for (event, key, at) in posts {
            host.sound_on(&event, &key, at);
        }
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost, dt: f64) {
        // the level's poll
        if self.wait >= 0.0 {
            self.wait -= dt;
            if self.wait <= 0.0 {
                self.wait = -1.0;
                for h in &self.start_heads {
                    if let Some(a) = host.skeletal(h) {
                        a.time = 0.0;
                        a.looping = true;
                        a.rate = 1.0;
                    }
                }
            }
        }
        // the helicopters' sound
        if !self.posted {
            self.post(host);
        }
        // MoveHelicopter
        for x in &self.helis {
            let p = &mut self.owners[x.p];
            let y = p.start[1] + self.cam.dy * p.factor[0];
            for (s, v) in p.start.iter_mut().zip(x.h.velocity) {
                *s += v;
            }
            if y < x.h.bounds[1] {
                p.start[1] = x.h.bounds[0];
                p.start[2] = host.rand(x.h.height);
            } else if y > x.h.bounds[0] {
                p.start[1] = x.h.bounds[1];
                p.start[2] = host.rand(x.h.height);
            }
        }
        // the level script's Frequencys, then the visualizer's Scroll
        let f32r = |v: f64| f64::from(v as f32);
        for (f, name) in self.freq.iter_mut().zip(ANALYZERS) {
            *f = host.rtpc(name).unwrap_or(METER_MIN);
        }
        let half = self.half as f64;
        for (i, pl) in self.planes.iter_mut().enumerate() {
            let norm = f32r(f32r(self.freq[i % 4] + 48.0) / 48.0);
            let d = (i as f64 + 1.0 - half).abs();
            let lerp = f32r(0.75 + f32r(0.25 * f32r(d / half)));
            let k = f32r(f32r(norm * lerp) * 10.0).trunc().clamp(1.0, 10.0) as i32;
            if k == pl.step {
                continue;
            }
            pl.step = k;
            let scroll = (f32r(0.1 * f64::from(k))).max(0.1) as f32;
            for &id in &pl.ids {
                host.vfx_material_scalars(id, 0, &[("Scroll", scroll)]);
            }
        }
    }

    pub(super) fn frame(&mut self, host: &mut SceneryHost) {
        self.cam.frame(host);
        for o in &mut self.owners {
            o.frame(&self.cam, host);
        }
        for x in &self.helis {
            let Some(at) = x.at else { continue };
            let o = self.owners[x.p].offset;
            host.sound_place(&x.key, [at[0] + o[0], at[1] + o[1], at[2] + o[2]]);
        }
    }
}
