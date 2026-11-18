//! Tutorial_Meeseeks, the advanced tutorials' targets (sim modes/tutorial/advanced/meeseeks/actor.rs: the stage run's
//! actors), as a tutorial draws them:
//! - CharacterMesh0: Meeseeks_SkelMesh with OverrideMaterials [Meeseeks_MAT] (the fighter draw table's
//!   Meeseeks_SkelMesh, no variant), relative (0, 0, -185), yaw -45, AnimationMode AnimationSingleNode with AnimToPlay
//!   Meeseeks_Wave_Montage, looping; ConvertToFlyingMeeseeks' PlayAnimation(Meeseeks_WaveAir_Montage, true) in the
//!   frame of the spawn (the sim's `flying`). The montage plays as one sequence of its slot track's segments (WAVE /
//!   WAVE_AIR below, from the montage assets), looped over its length. The mesh and the sequences are Rick's (the
//!   tutorial's cast loads Rick).
//! - ConvertToFlyingMeeseeks shows SM_RickJetBootleft / Right (SM_JetBoot on CharacterMesh0's fml_un_L /
//!   R_lolegTwist_att, relative (0, 0, 60) rotation (0, 180, 180) and (0, 0, -60) rotation (0, 180, 0), scale 1).
//!
//! Its particles are the renderer's (mvs-render fx/shared/tutorial.rs: they follow the bones this view gives its pose)
//! and its "+1" the scenery's.
// Unverified: the actor's rotation is taken as zero for every spawn (AerialCombat spawns at its tagged actors'
// transforms, whose rotation the stage data does not keep).
// Not played: the montages' notifies (Ak_OneShot sfx_cha_tutorial_meeseeks_cast at 0.0001 of each loop, WaveAir's
// Ak_Continuous sfx_map_tutorial_meeseeks_jetpack).

use std::collections::HashMap;

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::modes::tutorial::advanced::meeseeks::MeeseeksOwn;
use mvs_sim::world::stage_actors::{StageActor, StageActorOwn};
use mvs_vfx::math::{compose_rows, rotation_rows};

use crate::actor_view::{SkinnedActor, StaticView, Xform};
use crate::actors::ActorHost;
use crate::frame::ActorPose;

const STEP: f64 = 1.0 / 60.0;
/// The character whose draws and sequences the Meeseeks takes.
const OWNER: &str = "rick";
const MESH: &str = "Meeseeks_SkelMesh";
const BOOT: &str = "SM_JetBoot";
const REL_Z: f64 = -185.0;
const YAW: f64 = -45.0;

/// A montage's slot track: sequence, StartPos, AnimStartTime, AnimEndTime per segment (play rate 1, one loop each).
type Segment = (&'static str, f64, f64, f64);
const WAVE: [Segment; 3] = [
    ("Meeseeks_Spawn", 0.0, 0.0, 0.76666665),
    ("Meeseeks_Stand", 0.76666665, 0.0, 0.083333336),
    ("Meeseeks_Wave", 0.84999996, 0.0, 1.0833334),
];
const WAVE_AIR: [Segment; 3] = [
    ("Meeseeks_AirSpawn", 0.0, 0.0, 0.9166667),
    ("Meeseeks_AirWave", 0.9166667, 0.0, 1.3333334),
    ("Meeseeks_AirHover", 2.25, 0.0, 0.6666667),
];

/// SM_RickJetBootleft / Right: bone, relative location, relative rotation (pitch, yaw, roll in degrees).
const BOOTS: [(&str, [f64; 3], [f64; 3]); 2] = [
    ("fml_un_L_lolegTwist_att", [0.0, 0.0, 60.0], [0.0, 180.0, 180.0]),
    ("fml_un_R_lolegTwist_att", [0.0, 0.0, -60.0], [0.0, 180.0, 0.0]),
];

fn length_of(m: &[Segment]) -> f64 {
    let l = m[m.len() - 1];
    l.1 + l.3 - l.2
}

#[derive(Debug)]
struct View {
    sk: SkinnedActor,
    /// The boots, made at the first frame it flies.
    boots: Vec<StaticView>,
}

/// The Meeseeks' views, by (stage actor index, spawn tick); None: not drawn (gone, or its assets not loaded).
#[derive(Debug, Default)]
pub(crate) struct MeeseeksViews {
    views: HashMap<(u32, i32), Option<View>>,
    warned: bool,
    /// A sequence not loaded was warned (once).
    seq_warned: bool,
}

/// The tutorial Meeseeks of the World's stage run.
fn meeseeks(w: &World) -> impl Iterator<Item = (&StageActor, &MeeseeksOwn)> {
    w.stage_run.iter().flat_map(|r| r.actors.iter()).filter_map(|a| match &a.own {
        StageActorOwn::Meeseeks(m) => Some((a, m)),
        _ => None,
    })
}

impl MeeseeksViews {
    fn make(&mut self, host: &ActorHost<'_>) -> Option<View> {
        let made = host
            .draws
            .get(OWNER)
            .and_then(|d| d.actor(MESH, None))
            .and_then(|(path, keys)| Some(SkinnedActor::new(host.mesh(&path)?, &keys)));
        if made.is_none() && !self.warned {
            self.warned = true;
            eprintln!("warning: Tutorial_Meeseeks: not drawn (Rick's {MESH} or its materials are not loaded)");
        }
        Some(View { sk: made?, boots: Vec::new() })
    }

    /// Each live Meeseeks's mesh at `alpha` between the last two ticks; a dead one's view
    /// gone.
    pub(crate) fn present(&mut self, host: &mut ActorHost<'_>, w: &World, alpha: f64, out: &mut Vec<ActorPose>) {
        let live: Vec<(u32, i32)> = meeseeks(w).filter(|(a, _)| !a.dead).map(|(a, m)| (a.index, m.spawned)).collect();
        self.views.retain(|k, _| live.contains(k));
        for (a, m) in meeseeks(w) {
            if a.dead {
                continue;
            }
            let key = (a.index, m.spawned);
            if !self.views.contains_key(&key) {
                let v = self.make(host);
                self.views.insert(key, v);
            }
            let Some(Some(v)) = self.views.get_mut(&key) else { continue };
            let segs: &[Segment] = if m.flying { &WAVE_AIR } else { &WAVE };
            let t = ((f64::from(w.flow.tick - m.spawned - 1) + alpha).max(0.0) * STEP) % length_of(segs);
            let s = segs.iter().rfind(|g| t >= g.1).unwrap_or(&segs[0]);
            if let Some(clips) = host.clips.get_mut(OWNER)
                && let Some(c) = clips.get(s.0)
            {
                v.sk.play(clips, c, s.2 + (t - s.1).min(s.3 - s.2), false);
            } else if !self.seq_warned {
                self.seq_warned = true;
                eprintln!("warning: Tutorial_Meeseeks: the sequence {} is not loaded", s.0);
            }
            let rot = rotation_rows(0.0, YAW.to_radians(), 0.0);
            v.sk.set(&Xform { loc: [0.0, -f64::from(a.x), f64::from(a.y) + REL_Z], rot, scale: [1.0; 3] });
            let mut bones = Vec::new();
            v.sk.bones_world(&mut bones);
            let mut meshes = Vec::new();
            v.sk.draw(&mut meshes);
            if m.flying {
                if v.boots.is_empty() {
                    v.boots = BOOTS.iter().filter_map(|_| StaticView::of_char(host, OWNER, BOOT)).collect();
                }
                for (boot, (bone, loc, r)) in v.boots.iter_mut().zip(BOOTS) {
                    let Some((_, b)) = bones.iter().find(|(n, _)| n.eq_ignore_ascii_case(bone)) else { continue };
                    let off = [0, 1, 2].map(|c| (0..3).map(|k| loc[k] * b.rot[3 * k + c] * b.scale[k]).sum::<f64>());
                    let rel = rotation_rows(r[0].to_radians(), r[1].to_radians(), r[2].to_radians());
                    boot.set(&Xform {
                        loc: [0, 1, 2].map(|k| b.loc[k] + off[k]),
                        rot: compose_rows(&rel, &b.rot),
                        scale: b.scale,
                    });
                    boot.draw(&mut meshes);
                }
            }
            out.push(ActorPose { meshes, actor: Some(ActorRef::Stage(a.index)), bones, ..ActorPose::default() });
        }
    }
}
