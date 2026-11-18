//! The Court's two hoops as the sim plays them (M010_Hoop_A_01_BP_C; stages/the_court/hoop.rs),
//! drawn from the block's `hoops` (`plays` the clip path of each
//! PlayAnimation call, `explosion`):
//! - The SkeletalMesh: the reference pose (the AnimationBlueprint mode with no AnimClass) until the first
//!   PlayAnimation(asset, false) (@381 / @428 / @540 / @652 / @764 / @876): then the single node plays that asset
//!   from 0 at rate 1, not looping, and holds its last pose. The sim's M010Hoop.anim / anim_at (a world tick): the
//!   pose is the asset at (tick - anim_at) x the tick's seconds.
//! - HoopExplosionFXMulticast @113: SpawnEmitterAtLocation(PS_Hoop_Explosion, the HoopExplosionCollision sphere's
//!   world location, rot 0, scale 1, auto destroyed) @312, at M010Hoop.exploded_at (its HR_Front is the anim above).
//! - BasketScoredUIMulticast @1025-1943: UI_Functions.PopText('+{points}', lifetime 2, text colour white, location
//!   (0, -400 x SignOfFloat(actor scale X), 1200), space 3 (projected from the attach actor), scale 2, justification
//!   1 (centre), delay 0, no icons or audio, attach actor the hoop), at M010Hoop.scored_at / points.
//!
//! A play, an explosion or a score is shown once per sim tick it names; one older than a few ticks (a rollback's
//! replay, a Lab reset) is only marked seen.
// Unverified: the play's first pose is its 0 in the tick it starts (the overlap comes in the leftover pass, after the
// mesh's own animation tick in the build: its first advance is likely the next tick's).

use std::sync::Arc;

use mvs_sim::consts::DELTA_SECONDS;
use mvs_sim::world::stage_actors::StageActorOwn;
use mvs_vfx::Attachment;
use serde::Deserialize;

use crate::Result;
use crate::scenery::host::{PopSpec, SceneryHost, stage_actor};
use crate::stage::bad;

/// An event older than this when first seen is not shown.
const LATE: i32 = 6;
const POP_COLOR: [f64; 4] = [1.0, 1.0, 1.0, 1.0];

#[derive(Debug, Deserialize)]
pub(super) struct Play {
    anim: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HoopBlock {
    actor: String,
    pub(super) skeletal: String,
    plays: Vec<Play>,
    explosion: String,
}

#[derive(Debug)]
struct Hoop {
    d: HoopBlock,
    /// The `plays` clip paths, as the animation takes them.
    clips: Vec<Arc<str>>,
    /// StageData.placed index, found at the first world.
    placed: Option<usize>,
    /// The actor's root (UE), the pop text's attach actor.
    actor_loc: [f64; 3],
    /// -400 x sign(scale X).
    pop_z: f64,
    /// The explosion sphere's centre.
    explode_at: Option<Attachment>,
    anim_at: i32,
    exploded_at: i32,
    scored_at: i32,
}

#[derive(Debug)]
pub(super) struct Hoops {
    hoops: Vec<Hoop>,
}

/// An asset's name: `/Game/A/B.B` is `B`.
fn asset_name(path: &str) -> &str {
    path.rsplit(['.', '/']).next().unwrap_or(path)
}

/// JavaScript's Math.sign for the pop's side (NaN and 0 are their own).
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

impl Hoops {
    pub(super) fn make(blocks: Vec<HoopBlock>, host: &mut SceneryHost) -> Result<Hoops> {
        let what = "scenery: the court";
        let mut hoops = Vec::new();
        for d in blocks {
            if host.skeletal_index(&d.skeletal).is_none() {
                return Err(bad(what, format!("the hoop {} is not a skeletal mesh of the file", d.skeletal)));
            }
            for p in &d.plays {
                if host.state.clips.get(&p.anim).is_none() {
                    return Err(bad(what, format!("the hoop animation {} is not loaded", p.anim)));
                }
            }
            hoops.push(Hoop {
                clips: d.plays.iter().map(|p| Arc::from(p.anim.as_str())).collect(),
                d,
                placed: None,
                actor_loc: [0.0; 3],
                pop_z: 0.0,
                explode_at: None,
                anim_at: -1,
                exploded_at: -1,
                scored_at: -1,
            });
        }
        Ok(Hoops { hoops })
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        for h in &mut self.hoops {
            // the reference pose until the sim's first PlayAnimation
            if let Some(a) = host.skeletal(&h.d.skeletal) {
                a.clip = None;
                a.looping = false;
                a.rate = 0.0;
                a.time = 0.0;
            }
            h.placed = None;
            h.anim_at = -1;
            h.exploded_at = -1;
            h.scored_at = -1;
        }
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost) {
        let Some(w) = host.world() else { return };
        if w.stage_run.is_none() {
            return;
        }
        let tick = w.flow.tick;
        let fresh = |at: i32| at >= 0 && tick - at <= LATE;
        for h in &mut self.hoops {
            if h.placed.is_none() {
                let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
                let i = placed
                    .iter()
                    .position(|p| p.name == h.d.actor)
                    .unwrap_or_else(|| panic!("scenery: the court hoop {} is not in the stage data", h.d.actor));
                let p = &placed[i];
                h.placed = Some(i);
                h.actor_loc = [p.loc[0], p.loc[1], p.loc[2]];
                h.pop_z = -400.0 * sign(p.scale[0]);
                let solids = w.data.stage.solids.as_deref().unwrap_or(&[]);
                h.explode_at = p
                    .solids
                    .iter()
                    .flatten()
                    .filter_map(|&s| solids.get(usize::try_from(s).ok()?))
                    .find(|s| s.name.ends_with(".HoopExplosionCollision"))
                    .map(|s| Attachment {
                        loc: [s.center[0], s.center[1], s.center[2]],
                        rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                        scale: Some([1.0; 3]),
                    });
            }
            let Some(a) = h.placed.and_then(|i| stage_actor(w, i as u32)) else { continue };
            let StageActorOwn::M010Hoop(m) = &a.own else {
                panic!("scenery: the court hoop {} is not an M010Hoop of the sim", h.d.actor)
            };
            if m.anim_at != h.anim_at || m.anim_at >= 0 {
                let t = if m.anim_at >= 0 { f64::from(tick - m.anim_at) * f64::from(DELTA_SECONDS) } else { 0.0 };
                let clip = if m.anim_at >= 0 && m.anim_at != h.anim_at {
                    let name = m.anim.map(|n| w.names.str(n)).unwrap_or("");
                    let k = h.d.plays.iter().position(|p| asset_name(&p.anim) == name).unwrap_or_else(|| {
                        panic!("scenery: the court hoop plays {name}, not one of its PlayAnimation calls")
                    });
                    Some(Some(h.clips[k].clone()))
                } else if m.anim_at < 0 {
                    Some(None)
                } else {
                    None
                };
                h.anim_at = m.anim_at;
                if let Some(a) = host.skeletal(&h.d.skeletal) {
                    if let Some(c) = clip {
                        a.clip = c;
                        a.looping = false;
                        a.rate = 0.0;
                    }
                    a.time = t;
                }
            }
            if m.exploded_at != h.exploded_at {
                h.exploded_at = m.exploded_at;
                if fresh(m.exploded_at)
                    && let Some(at) = &h.explode_at
                    && let Some(vfx) = host.vfx()
                {
                    vfx.spawn_level(&h.d.explosion, at, false, &[], 0, false);
                }
            }
            if m.scored_at != h.scored_at {
                h.scored_at = m.scored_at;
                if fresh(m.scored_at) {
                    host.pop(PopSpec {
                        text: format!("+{}", m.points),
                        lifetime: 2.0,
                        text_color: POP_COLOR,
                        location: [0.0, h.pop_z, 1200.0],
                        scale: 2.0,
                        justification: 1,
                        delay: 0.0,
                        icon_right: None,
                        fighter: None,
                        at: Some(h.actor_loc),
                    });
                }
            }
        }
    }
}
