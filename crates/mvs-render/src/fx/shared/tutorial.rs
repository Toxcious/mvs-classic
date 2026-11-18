//! The advanced tutorials' effects: the ownerless fire wall of TutorialMinigame_Elements'
//! ExtinguishFire, the KBI minigames' PS_MeeseeksSpawn and Tutorial_Meeseeks's
//! particles. The Meeseeks's mesh is mvs-view's (tutorial_meeseeks.rs), its "+1" the scenery block's
//! (scenery/tutorial.rs).
//!
//! The fire wall (a FireWallHazard with no instigator): the shared path
//! draws an owned actor's components alone, so its two drawn components are drawn from here, as the instigated wall's
//! (fx/shared/firewall.rs):
//! - ParticleSystem: PS_Creature_FireWall at its RelativeLocation (Z -90) and RelativeScale3D (1.25) under the Capsule
//!   root at the spawn transform's yaw, auto activated, with the Team the minigame writes after FinishSpawning
//!   (SetFloatParameter(Team, 0.0) @1175; the wall's own BeginPlay sets none with no BaseFighter instigator:
//!   FireWallHazard @558).
//! - Decal: CreatureFireGround_Decal_MAT, DecalSize (10, 256, 256), Z -130, Pitch 90, with its material's own Team (the
//!   minigame's SetScalarParameterValue(Team, 0.0) @1251 reaches None: FireWallHazard makes its dynamic material at
//!   @996, inside the instigator branch that @558 skips). At the spawn's yaw (the FireWall point's, 0) the pitch makes
//!   the decal's local X the world +Z, its local Y the world +Y and its local Z the world -X.
//!
//! The KBI minigames' puff (TutorialMinigame_KBISide @161 / @602, TutorialMinigame_KBIUp @892 / @1375):
//! SpawnEmitterAtLocation, then SetFloatParameter(Team, 1.0). FastFall_Platform's (@418) sets no Team and keeps the
//! shared no-instigator draw.
//!
//! Tutorial_Meeseeks (the stage run's actors):
//! - BeginPlay's Delay link (@70; the sim's `shown`): SpawnEmitterAtLocation(PS_MeeseeksSpawn) at the actor, rotation
//!   0, scale 1, SetFloatParameter(Team, 1.0) (@162, @337).
//! - On Hit (@439; MeeseeksEnd::Hit): SpawnEmitterAtLocation(PS_MeeseeksDestroyed) at the actor, rotation 0, scale 1,
//!   SetFloatParameter(Team, 1.0) (@467, @536).
//! - A spawner's PS_MeeseeksSpawn ahead of its destroy (MeeseeksEnd::Poof: PositioningTools' Despawn Meeseeks and End
//!   Game), Team 1.0.
//! - ConvertToFlyingMeeseeks (the sim's `flying`): ParticleSystem_JetsLeft / Right SetHiddenInGame(false) (@68, @102):
//!   PS_JetPackFlame_Looping on CharacterMesh0's fml_un_L / R_toe_end, no relative place, RelativeScale3D
//!   (0.50000507, 0.50000507, 0.5), auto activated with the actor; gone with it.
// Unverified: the decal is placed where the wall is at each drawn frame.
// Unverified: a particle Team the graph did not write (the wall's `fx_team` unset) is drawn as 0, the value the
// minigame writes.
// Unverified: the jets' systems start when the view first has the bones (the frame of the spawn, ConvertToFlyingMeeseeks
// running in it); their component's hidden time before is not simulated.

use std::collections::BTreeMap;

use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::modes::tutorial::advanced::meeseeks::{MeeseeksEnd, MeeseeksOwn};
use mvs_sim::world::stage_actors::{StageActor, StageActorOwn};
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, rotation_rows};

use crate::fx::attach::{text, xyz};
use crate::fx::chars::{CharFx, CharFxHooks, ProjRule};
use crate::fx::shared::firewall::DECAL;
use crate::fx::{FloorDecal, Follow, Fx};

const SPAWN: &str = "PS_MeeseeksSpawn";
const DESTROYED: &str = "PS_MeeseeksDestroyed";
const JETS: &str = "PS_JetPackFlame_Looping";
/// ParticleSystem_JetsLeft / Right: their bones and RelativeScale3D.
const JET_BONES: [&str; 2] = ["fml_un_L_toe_end", "fml_un_R_toe_end"];
const JET_SCALE: [f64; 3] = [0.50000507, 0.50000507, 0.5];

/// An ownerless wall's particle component, and whether this tick saw it.
#[derive(Debug)]
struct Wall {
    h: Option<SystemId>,
    seen: bool,
}

/// A Meeseeks's particles: BeginPlay's puff spawned, its jets.
#[derive(Debug, Default)]
struct Seeks {
    puffed: bool,
    jets: [Option<SystemId>; 2],
}

/// What the module keeps between steps.
#[derive(Debug, Default)]
pub(crate) struct Tutorial {
    /// By the wall's spawn count (`Projectile::seq`).
    walls: BTreeMap<i32, Wall>,
    /// By (stage actor index, spawn tick); None once it ended.
    seeks: BTreeMap<(u32, i32), Option<Seeks>>,
}

/// The tutorial Meeseeks of the World's stage run (MeeseeksActor).
fn meeseeks(w: &World) -> impl Iterator<Item = (&StageActor, &MeeseeksOwn)> {
    w.stage_run.iter().flat_map(|r| r.actors.iter()).filter_map(|a| match &a.own {
        StageActorOwn::Meeseeks(m) => Some((a, m)),
        _ => None,
    })
}

/// Its root (UE cm).
fn at(a: &StageActor) -> [f64; 3] {
    [0.0, -f64::from(a.x), f64::from(a.y)]
}

impl Tutorial {
    fn kill_all(&mut self, fx: &mut Fx) {
        for v in std::mem::take(&mut self.walls).into_values() {
            if let Some(h) = v.h {
                fx.player.kill(h);
            }
        }
        for v in std::mem::take(&mut self.seeks).into_values().flatten() {
            for h in v.jets.into_iter().flatten() {
                fx.player.kill(h);
            }
        }
    }

    /// The walls with no instigator that are there, their particle component made at their
    /// first sight and gone with the actor.
    fn walls(&mut self, fx: &mut Fx, w: &World) {
        for v in self.walls.values_mut() {
            v.seen = false;
        }
        for (id, pr) in w.actors.projectiles.iter() {
            let ProjectileExt::FireWall(fw) = &pr.ext else { continue };
            if pr.dead || pr.owner.is_some() {
                continue;
            }
            if let Some(v) = self.walls.get_mut(&pr.seq) {
                v.seen = true;
                continue;
            }
            let c = actor_def(w, ActorRef::Projectile(id), pr.cls)
                .and_then(|d| d.components.get("ParticleSystem"))
                .and_then(Value::as_obj);
            let h = c.and_then(|c| {
                let ps = text(c, "Template")?;
                let rows = rotation_rows(0.0, f64::from(fw.spawn_yaw.unwrap_or(0.0)).to_radians(), 0.0);
                let f = Follow::Fixed {
                    seq: pr.seq,
                    rel: xyz(c.get("RelativeLocation"), 0.0),
                    rows,
                    scale: xyz(c.get("RelativeScale3D"), 1.0)[0],
                };
                fx.spawn_follow(w, ps, f, 1.0, &[("Team", f64::from(fw.fx_team.unwrap_or(0.0)))])
            });
            self.walls.insert(pr.seq, Wall { h, seen: true });
        }
        self.walls.retain(|_, v| {
            if !v.seen
                && let Some(h) = v.h
            {
                fx.player.kill(h);
            }
            v.seen
        });
    }

    /// The Decal component at the wall's place, through its spawn yaw.
    fn decals(fx: &mut Fx, w: &World) {
        for (id, pr) in w.actors.projectiles.iter() {
            let ProjectileExt::FireWall(fw) = &pr.ext else { continue };
            if pr.dead || pr.owner.is_some() {
                continue;
            }
            let Some(dc) = actor_def(w, ActorRef::Projectile(id), pr.cls)
                .and_then(|d| d.components.get("Decal"))
                .and_then(Value::as_obj)
            else {
                continue;
            };
            let rl = xyz(dc.get("RelativeLocation"), 0.0);
            let s = f64::from(pr.scale);
            let yaw = f64::from(fw.spawn_yaw.unwrap_or(0.0)).to_radians();
            let (sy, cy) = yaw.sin_cos();
            let sz = crate::fx::shared::firewall::decal_size(dc);
            let sc = xyz(dc.get("RelativeScale3D"), 1.0);
            fx.decals.push(FloorDecal {
                actor: pr.seq,
                material: text(dc, "DecalMaterial").unwrap_or(DECAL).to_string(),
                loc: [
                    s * (rl[0] * cy - rl[1] * sy),
                    -f64::from(pr.x) + s * (rl[0] * sy + rl[1] * cy),
                    f64::from(pr.y) + s * rl[2],
                ],
                rows: [0.0, 0.0, 1.0, -sy, cy, 0.0, -cy, -sy, 0.0],
                extent: [0, 1, 2].map(|k| s * sz[k] * sc[k]),
                team: None,
                alpha: None,
                dissolve: None,
            });
        }
    }

    /// Each Meeseeks's BeginPlay puff, its jets while it flies, its end's effect.
    fn meeseeks(&mut self, fx: &mut Fx, w: &World) {
        for (a, m) in meeseeks(w) {
            let key = (a.index, m.spawned);
            let v = match self.seeks.get_mut(&key) {
                Some(Some(v)) => v,
                Some(None) => continue,
                None if a.dead => {
                    // (spawned and gone between two presented ticks)
                    self.seeks.insert(key, None);
                    ended(fx, a, m);
                    continue;
                }
                None => self.seeks.entry(key).or_insert(Some(Seeks::default())).as_mut().expect("just made"),
            };
            if !v.puffed && m.shown >= 0 {
                v.puffed = true;
                fx.player.spawn(SPAWN, at(a), [0.0; 3], [1.0; 3], &[("Team", 1.0)]);
            }
            if a.dead {
                for h in v.jets.into_iter().flatten() {
                    fx.player.kill(h);
                }
                self.seeks.insert(key, None);
                ended(fx, a, m);
                continue;
            }
            if m.flying {
                for (k, bone) in JET_BONES.into_iter().enumerate() {
                    if v.jets[k].is_some() {
                        continue;
                    }
                    let f = Follow::Bone {
                        a: ActorRef::Stage(a.index),
                        bone: bone.to_owned(),
                        loc: [0.0; 3],
                        rel: IDENTITY,
                        scale: JET_SCALE,
                    };
                    v.jets[k] = fx.spawn_follow(w, JETS, f, 1.0, &[]);
                }
            }
        }
    }
}

/// The end's effect (the "+1" is the scenery block's).
fn ended(fx: &mut Fx, a: &StageActor, m: &MeeseeksOwn) {
    let ps = match m.end {
        MeeseeksEnd::Poof => SPAWN,
        MeeseeksEnd::Hit => DESTROYED,
        MeeseeksEnd::None | MeeseeksEnd::Plain => return,
    };
    fx.player.spawn(ps, at(a), [0.0; 3], [1.0; 3], &[("Team", 1.0)]);
}

impl CharFxHooks for Tutorial {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.walls(fx, w);
        self.meeseeks(fx, w);
    }

    fn frame(&mut self, fx: &mut Fx, w: &World, _alpha: f64) {
        Tutorial::decals(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.kill_all(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.kill_all(fx);
    }
}

/// The KBI minigames' PS_MeeseeksSpawn at the event's place, rotation 0, Team 1.0.
fn kbi_puff(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx: Some(v), x, y, ox, scale, .. } = e else { return false };
    fx.player.spawn(
        w.names.str(*v),
        [f64::from(*ox), -f64::from(*x), f64::from(*y)],
        [0.0; 3],
        [f64::from(*scale); 3],
        &[("Team", 1.0)],
    );
    true
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    ("TutorialMinigame_KBISide_C", ProjRule { own_emitter: Some(kbi_puff), ..ProjRule::NONE }),
    ("TutorialMinigame_KBIUp_C", ProjRule { own_emitter: Some(kbi_puff), ..ProjRule::NONE }),
];
