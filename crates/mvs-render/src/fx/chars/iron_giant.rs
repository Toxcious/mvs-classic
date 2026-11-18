//! The Iron Giant's effects (the CharFx hooks and the
//! classes' projectile rules; his meshes, pose and weapon glow are mvs-view chars/iron_giant.rs).
//!
//! What a match draws for him beside the meshes:
//!   His foot rockets: L_Foot_Rocket / R_Foot_Rocket (PS_C017_BoostersJump on fml_un_L_foot / fml_un_R_foot,
//!     relative scale 0.84890103, bVisible false): SetHoverValueMulticast (@22188) sets their visibility to
//!     isGliding, the reset hides them (@30 / @64); Team @1540..1833; BPI_SetBoosterScale (@27477) writes Scale =
//!     VLerp(0, 1, FuelAmount / FuelMaxAmount) on both. SCS children of BodyMesh: the pose copy's bone (boneFollow's
//!     'copy').
//!     Unverified: the hidden components are started with the pawn and tick unseen (the build's start with their
//!     tick off, and whether a hidden component simulates is native and not read).
//!   The S04 skin's electricity: PS_C017_Electricity_Shoulder_L / _R (PS_C017_Electricity_Shoulders on
//!     VFX_ShoulderBolt_L / VFX_ShoulderBolt_R) and PS_C017_Electricity_Head (PS_C017_Electricity_Head on
//!     fml_un_wp_head), SCS children of BodyMesh with relative scale 1 and bVisible false. UpdateSkinVisuals (@15,
//!     @27387) sets each one's visibility to AppliedSkin.Slug == skin_c017_s04 (@15 / @146 / @277); nothing else
//!     writes them (no Team). Drawn for an S04 pawn alone: the other skins' stay hidden all match.
//!     Unverified: they are started with the pawn (the build's templates start with their tick off; what starts it
//!     is native and not read).
//!   The ally turret's lead-in: PilotLargeAttackVisuals (@8653..8938): SpawnEmitterAttached(
//!     PS_AttackChargeUP_Short_nonLoop, the main mesh, giantEmblem_att, (150, 0, 0), rotation 0, scale 1.5,
//!     KeepRelativeOffset), no parameter; after the Delay(0.3) its K2_DestroyComponent (@66): the sim's
//!     allyTurret.bomb, the latent ticks left of that Delay.
//!   His actors' particle components (ACTOR_FX), each placed from the actor's state of the latest tick:
//!     C017_AntenaBolts, C017_Ally_Turret_Bomb, C017_Pilot_Reticle, C017_CannonShotAim, C017_CannonBall,
//!     C017_CannonExplotion and C017_DownThrusterAttack (their notes at each below).
//!   C017_DownThrusterAttack's four flame planes and C017_Pilot_Reticle's ReticuleArm: component meshes
//!     (crate::cmesh; their notes at Flames and Arm).
//!   Buff_C017_AllyBurst's warning on his team mates: PS_C017_Burst_Warning_Ally on the buffed
//!     fighter's root (AddVisuals @2530: the owner's CapsuleComponent, (0, 0, 75), scale 1, KeepRelativeOffset, Team
//!     @2995), one per new `born` of the buff. The pawn's component: it plays on after the buff and is hidden with
//!     the fighter. Unverified: the system ends by itself (its emitters' loops are not read here).
//!   The pilot's screen effect: Buff_C017_Ally_Pilot, on the authority once the pilot is placed
//!     (@616-646, the sim's pilot_stage 2): ScreenVisuals, a client event of the buff (its owner is the pilot),
//!     @7107-7476: GetAllActorsOfClass(PFGCamera)[0].K2_GetRootComponent -> SpawnEmitterAttached(PS_C017_UI, the
//!     root, no socket, (250, 0, 0), the rotation constant (Pitch -180), scale 1, KeepRelativeOffset, bAutoDestroy)
//!     -> ScreenEffect. ReceiveEndPlay (@6681-6925): ScreenEffect.K2_DestroyComponent when it is valid. Nothing else
//!     writes to it (no parameter, no visibility). Its two emitters hold one PSA_Rectangle sprite each (size 10000,
//!     local (-1, 0, 0), EPAL_X): the first reveals over its life of 1 s, the second starts at 1.03 s and stays;
//!     C017_UI_MAT takes its UVs from the pixel and reads the scene colour copy. One a pilot (the buff is not
//!     stacked).
//!     Unverified: which machine shows it. The event runs on the pilot's owning client; an offline match is taken as
//!     the build's standalone game, where a client event runs in place for every actor (any pilot's effect is on the
//!     one screen, a bot's too), an online match as the pilot's own machine alone (`Fx::own_client`).
//!     AActor::GetFunctionCallspace is 4.26's as remembered, not read from the exe; the build's local match with a
//!     second pad is not checked. The effect is placed at the camera root of the last frame before the particle
//!     tick, the view per drawn frame: the sprite covers the view at either place.

use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{AttachParentOut, Projectile, attach_parent};
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::chars::iron_giant::actor_state::ball_place;
use mvs_sim::chars::iron_giant::cannon_aim::aim_at;
use mvs_sim::chars::iron_giant::state::giant_of;
use mvs_sim::chars::iron_giant::turret::pilot_stage;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::player::Attachment;

use super::{CharFxHooks, ProjRule, component_warm, fighter_of};
use crate::cmesh::{CMeshGeometry, CMeshId, object_path};
use crate::fx::attach::{att, rot_rows, warn_once, xyz};
use crate::fx::{Follow, Fx};

const STEP: f64 = 1.0 / 60.0;

const ROCKET: &str = "PS_C017_BoostersJump";
const ROCKET_SOCKETS: [&str; 2] = ["fml_un_L_foot", "fml_un_R_foot"];
const ROCKET_SCALE: f64 = 0.84890103;
/// UpdateSkinVisuals' slug, and the electricity's systems and sockets (shoulder L, shoulder R, head).
const S04: &str = "skin_c017_s04";
const ELECTRICITY: [(&str, &str); 3] = [
    ("PS_C017_Electricity_Shoulders", "VFX_ShoulderBolt_L"),
    ("PS_C017_Electricity_Shoulders", "VFX_ShoulderBolt_R"),
    ("PS_C017_Electricity_Head", "fml_un_wp_head"),
];
const CHARGE: &str = "PS_AttackChargeUP_Short_nonLoop";
const EMBLEM: &str = "giantEmblem_att";
const ALLY_WARNING: &str = "PS_C017_Burst_Warning_Ally";
const ALLY_BURST: &str = "Buff_C017_AllyBurst_C";
const SCREEN: &str = "PS_C017_UI";
const SCREEN_AT: f64 = 250.0;

/// `yawRows`: the actor yawed to its facing (the spawn rotation).
fn yaw_rows(facing: f64) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(0.0, if facing > 0.0 { -h } else { h }, 0.0)
}

/// One Giant pawn's effects.
#[derive(Debug, Default)]
struct Pawn {
    rockets: [Option<SystemId>; 2],
    /// The S04 skin's electricity (ELECTRICITY's order).
    electricity: [Option<SystemId>; 3],
    charge: Option<SystemId>,
    charging: bool,
}

/// How one of an actor's particle components is placed each tick.
#[derive(Debug)]
enum Place {
    /// `underRoot`: at `rl` (or the actor's aim, `aim`) under the actor's root yawed to its facing, rows `own` under
    /// it, scale `s`, all at the root's scale.
    Root { rl: V3, own: Rows, s: V3, aim: bool },
    /// C017_DownThrusterAttack's wind `k` (0 left, 1 right).
    Wind { k: usize, own: Rows, s: V3 },
    /// C017_AntenaBolts' ParticleSystem (scale `base`) or ElectricRing (`ring`: scale `base` x ExpandAlpha, shown
    /// while bound) on the owner's fml_un_vr_spine3.
    Spine { f: Box<Follow>, base: f64, ring: bool },
    /// C017_AntenaBolts' bolt `j` of `n`'s PS_C017_SPN_BoltElec.
    Bolt { j: usize, n: usize },
    /// C017_Ally_Turret_Bomb's component: `rel`, rows `rot` and scale `base` at the actor's `scale`.
    Bomb { rel: V3, rot: Rows, base: V3 },
    /// C017_Pilot_Reticle's AimReticule: `rel`, rows `rot`, scale `scale` at the root's scale.
    Reticle { rel: V3, rot: Rows, scale: V3 },
}

/// One live actor's components.
#[derive(Debug)]
struct Live {
    seq: i32,
    hs: Vec<(Option<SystemId>, Place)>,
    /// C017_Pilot_Reticle's ReticuleArm.
    arm: Option<Arm>,
}

/// One buffed fighter's AllyBurst warnings.
#[derive(Debug, Default)]
struct AllyWarn {
    born: Option<f64>,
    hs: Vec<SystemId>,
}

/// What The Iron Giant's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct IronGiant {
    /// Per Giant pawn (by pawn id).
    pawns: HashMap<i32, Pawn>,
    /// Per actor of ACTOR_FX.
    live: HashMap<ActorRef, Live>,
    /// Per fighter (by pawn id): the `born` of the AllyBurst buff last drawn and the warnings spawned.
    ally: HashMap<i32, AllyWarn>,
    /// Per Giant pawn: his flame planes (None: none of the four could be made).
    flames: BTreeMap<i32, Option<Flames>>,
    /// Per pilot (by pawn id): the screen effect while it is on (None: not made, no camera or no template).
    screens: BTreeMap<i32, Option<SystemId>>,
}

/// A component's LocalToWorld (FMatrix3x4) from its place, rows (the world directions of its X, Y and Z) and scale.
fn plane_matrix(loc: V3, rot: &Rows, s: V3) -> [f32; 12] {
    let mut m = [0.0; 12];
    for c in 0..3 {
        m[4 * c] = (rot[c] * s[0]) as f32;
        m[4 * c + 1] = (rot[3 + c] * s[1]) as f32;
        m[4 * c + 2] = (rot[6 + c] * s[2]) as f32;
        m[4 * c + 3] = loc[c] as f32;
    }
    m
}

/// A static mesh component of the store drawn with `material` (a component material's key), hidden, at the origin;
/// None when the mesh kind has no `mesh` (the component's geometry finds none).
fn add_plane(fx: &mut Fx, mesh: &str, material: &str, scalars: &[(&str, f64)]) -> Option<CMeshId> {
    let mesh = object_path(mesh);
    if !fx.mesh_pack().has(&mesh) {
        warn_once(format!("Iron Giant: no mesh {mesh}"));
        return None;
    }
    let geometry = CMeshGeometry::Static { mesh: mesh.into(), to_world: plane_matrix([0.0; 3], &IDENTITY, [1.0; 3]) };
    let id = fx.components.add(material, scalars, geometry);
    fx.components.get_mut(id).expect("added").shown = false;
    Some(id)
}

/// Shows or hides a component mesh, at a place when given.
fn set_plane(fx: &mut Fx, id: CMeshId, shown: bool, at: Option<[f32; 12]>) {
    let Some(m) = fx.components.get_mut(id) else { return };
    m.shown = shown;
    if let (Some(at), CMeshGeometry::Static { to_world, .. }) = (at, &mut m.geometry) {
        *to_world = at;
    }
}

const FLAME_MATERIALS: &str = "Panda_Main/Characters/C017/VFX/";
const FLAME_MESHES: &str = "Panda_Main/VFX/Meshes/";
const FLAME_PLANES: [&str; 4] = ["ThrusterLeft", "FlameSide", "ThrusterRight", "FlameSideB"];

/// One flame plane: its mesh, its template's place under the actor's root and the Reveal written last.
#[derive(Debug)]
struct Plane {
    id: CMeshId,
    rl: V3,
    rot: Rows,
    scale: V3,
    reveal: f64,
}

/// C017_DownThrusterAttack's flame planes (StaticMeshComponents):
///   ThrusterLeft / ThrusterRight: SM_Plane_CornerPivot with C017_BoosterFire_MAT under LeftFlame / RightFlame;
///   FlameSide / FlameSideB: SM_Plane_CornerPivot_Slanted with C017_FireMesh01_MAT.
/// SetTeamColor (@6036..6198) writes Team = SelectFloat(0.6, the colour value, IsLocalOnline) on all four.
/// ReceiveTick, per foot (left @2289..3762 with LeftFoot = FacingLeft, right @3887..5418 with not FacingLeft):
///   Thruster.Reveal = Lerp(1, 0, FClamp(NormalizeToRange(Abs(MainTransforms.Scale.Z), 50, 375), 0, 1)) (@2645 /
///     @4243), the scale's Z being half the trace's distance on a hit and FlameLength / 2 on a miss (TraceFlame @865,
///     @3930): 1 with the floor within 100, 0 without a floor.
///   Side.SetVisibility(HitGround) (@2699 / @4297);
///   Side.SetWorldScale3D(FMax(Abs(S.Y), 50) x SelectFloat(-3, 3, LeftFoot), S.X x 4, S.X x Lerp(0, 4,
///     FClamp(NormalizeToRange(Abs(S.Y), 0, 250), 0, 1))) (@3289 / @4916) with S = SecondaryTransforms.Scale =
///     (FlameBaseScale.X, +-(FlameLength - distance) / 2, FlameBaseScale.Z) on a hit (TraceFlame @2137, @3050);
///     FlameBaseScale is LeftFlame's component scale (the construction script @159: the template's 100);
///   Side.K2_SetWorldLocation(SecondaryTransforms.Location.X + 50, HitLocation.Y + SelectFloat(-100, 100, LeftFoot),
///     HitLocation.Z) (@3658 / @5314), the X being the foot box's (the play plane's);
///   Side.K2_SetWorldRotation((Pitch 0, Yaw 90, Roll 0)) (@3762 / @5418; the constant from the bytecode dump).
/// Without a hit the side keeps its last world scale and rotation and goes to (50, +-100, 0) hidden.
/// The sim's flame[k] is FlameLength - the distance (-1 on a miss) and boxes[2k] the foot's main box.
/// The planes are made with the match, one set per Iron Giant (NumberOfActorsAllowed 1), and shown while his actor
/// lives; the match's end takes them.
// Unverified: before the actor's first tick the planes are at their templates' places (the sides shown, as the
// components are); the planes are placed at the sim's tick, not at the drawn fighter (as the winds).
#[derive(Debug)]
struct Flames {
    planes: [Option<Plane>; 4],
    team: f64,
    len: f64,
    base: V3,
    on: bool,
}

impl Flames {
    fn new(fx: &mut Fx, w: &World, fid: mvs_sim::fighter::FighterId) -> Option<Flames> {
        let actor = w.char_data(fid).fighter.actors.as_ref().and_then(|a| a.get("C017_DownThrusterAttack_C"));
        let empty = Props::new();
        let cs = actor.map_or(&empty, |a| &a.components);
        let team = fx.team(w.fighter(fid).id);
        let planes = FLAME_PLANES.map(|name| {
            let c = comp(cs, name)?;
            let sm = c.get("StaticMesh").and_then(Value::as_str)?;
            let mat = c.get("OverrideMaterials").and_then(Value::as_arr)?.first()?.as_str()?;
            let id = add_plane(
                fx,
                &format!("{FLAME_MESHES}{sm}"),
                &format!("{FLAME_MATERIALS}{mat}#local"),
                &[("Team", team)],
            )?;
            Some(Plane {
                id,
                rl: xyz(c.get("RelativeLocation"), 0.0),
                rot: rot_rows(c.get("RelativeRotation")),
                scale: xyz(c.get("RelativeScale3D"), 1.0),
                reveal: -1.0,
            })
        });
        if planes.iter().all(Option::is_none) {
            return None;
        }
        let len = actor.and_then(|a| a.p.get("FlameLength")).and_then(Value::as_f64).unwrap_or(0.0);
        let base = xyz(comp(cs, "LeftFlame").and_then(|c| c.get("RelativeScale3D")), 1.0);
        Some(Flames { planes, team, len, base, on: false })
    }

    fn begin(&mut self) {
        self.on = false;
    }

    /// The thruster actor's tick.
    fn place(&mut self, fx: &mut Fx, t: &mvs_sim::chars::iron_giant::actor_state::DownThruster) {
        self.on = true;
        let root = yaw_rows(f64::from(t.facing));
        // a component at its template's place under the actor's root
        // Deviation: the planes are drawn at scale 1 under a grown root (the sim's sizeScale); the thruster's
        // shapes and its other components take the scale.
        let own = |p: &Plane| {
            let mut loc = [0.0, -f64::from(t.x), f64::from(t.y)];
            for (k, v) in loc.iter_mut().enumerate() {
                *v += p.rl[0] * root[k] + p.rl[1] * root[3 + k] + p.rl[2] * root[6 + k];
            }
            plane_matrix(loc.map(f64::from), &compose_rows(&p.rot, &root), p.scale)
        };
        let clamp01 = |v: f64| v.clamp(0.0, 1.0);
        for k in 0..2 {
            let m = t.boxes.get(2 * k);
            let r = t.flame.get(k).copied().unwrap_or(-1.0);
            let hit = r >= 0.0;
            if let Some(th) = &mut self.planes[2 * k] {
                set_plane(fx, th.id, true, Some(own(th)));
                // (the template's value before the first tick: the material's default, 0)
                let reveal = if m.is_some() {
                    1.0 - clamp01(
                        ((if hit { (self.len - f64::from(r)) / 2.0 } else { self.len / 2.0 }) - 50.0) / (375.0 - 50.0),
                    )
                } else {
                    0.0
                };
                if reveal != th.reveal {
                    th.reveal = reveal;
                    if let Some(c) = fx.components.get_mut(th.id) {
                        c.set_scalar("Team", self.team);
                        c.set_scalar("Reveal", reveal);
                    }
                }
            }
            let Some(side) = &self.planes[2 * k + 1] else { continue };
            let Some(m) = m else {
                set_plane(fx, side.id, true, Some(own(side)));
                continue;
            };
            if !hit {
                set_plane(fx, side.id, false, None);
                continue;
            }
            let left_foot = (t.facing < 0.0) == (k == 0);
            let sy = r / 2.0;
            let loc = [
                50.0,
                -m.x + if left_foot { -100.0 } else { 100.0 },
                mvs_sim::math::narrow(f64::from(t.y) - (self.len - f64::from(r))),
            ];
            let scale = [
                sy.max(50.0) * if left_foot { -3.0 } else { 3.0 },
                mvs_sim::math::narrow(self.base[0] * 4.0),
                mvs_sim::math::narrow(self.base[0] * 4.0 * clamp01(f64::from(sy / 250.0))),
            ];
            let rot = rotation_rows(0.0, std::f64::consts::FRAC_PI_2, 0.0);
            set_plane(fx, side.id, true, Some(plane_matrix(loc.map(f64::from), &rot, scale.map(f64::from))));
        }
    }

    fn end(&self, fx: &mut Fx) {
        if self.on {
            return;
        }
        for p in self.planes.iter().flatten() {
            set_plane(fx, p.id, false, None);
        }
    }
}

const ARM_MAT: &str = "Panda_Main/Characters/C017/VFX/C017_UI_Reticule_Inner_MAT#local";
const ARM_MESHES: &str = "Panda_Main/Characters/C017/VFX/";

/// C017_Pilot_Reticle's ReticuleArm (SM_C017_ReticuleArrow, its default material C017_UI_Reticule_Inner_MAT: masked,
/// so in the base pass) under AimReticule at its relative place and scale, both under the AimReticule's scale and
/// rotation; the root's scale (the sim's sizeScale) over both. It goes with the actor.
// Unverified: the arm's look (texture, mask, colour) is not compared with the build's.
#[derive(Debug)]
struct Arm {
    id: CMeshId,
    off: V3,
    base: V3,
    rot: Rows,
}

impl Arm {
    fn new(fx: &mut Fx, x: &WorldActor, cs: &Props) -> Option<Arm> {
        let WorldActorState::PilotReticle(r) = &x.state else { return None };
        let c = comp(cs, "AimReticule")?;
        template(c)?;
        let ac = comp(cs, "ReticuleArm")?;
        let sm = ac.get("StaticMesh").and_then(Value::as_str)?;
        let actor = yaw_rows(if r.flip { -1.0 } else { 1.0 });
        let own = if r.flip {
            rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
        } else {
            rot_rows(c.get("RelativeRotation"))
        };
        let rot = compose_rows(&own, &actor);
        let (rl, scale) = (xyz(c.get("RelativeLocation"), 0.0), xyz(c.get("RelativeScale3D"), 1.0));
        let (al, als) = (xyz(ac.get("RelativeLocation"), 0.0), xyz(ac.get("RelativeScale3D"), 1.0));
        let off = [0, 1, 2].map(|k| {
            let rel = rl[0] * actor[k] + rl[1] * actor[3 + k] + rl[2] * actor[6 + k];
            rel + al[0] * scale[0] * rot[k] + al[1] * scale[1] * rot[3 + k] + al[2] * scale[2] * rot[6 + k]
        });
        let base = [0, 1, 2].map(|k| als[k] * scale[k]);
        let id = add_plane(fx, &format!("{ARM_MESHES}{sm}"), ARM_MAT, &[])?;
        Some(Arm { id, off, base, rot })
    }

    fn place(&self, fx: &mut Fx, x: &WorldActor) {
        let WorldActorState::PilotReticle(r) = &x.state else { return };
        let g = x.size_scale.unwrap_or(1.0);
        let loc = [
            self.off[0] * f64::from(g),
            self.off[1] * f64::from(g) - f64::from(r.x),
            self.off[2] * f64::from(g) + f64::from(r.y),
        ];
        set_plane(fx, self.id, true, Some(plane_matrix(loc, &self.rot, self.base.map(|v| v * f64::from(g)))));
    }
}

/// The actor's root: its place, facing and size scale, from its state (None: not one of his classes here).
fn root(w: &World, x: &WorldActor) -> Option<(f64, f64, f64)> {
    Some(match &x.state {
        WorldActorState::AntenaBolts(a) => (f64::from(a.x), f64::from(a.y), f64::from(a.facing)),
        WorldActorState::CannonBall(b) => {
            let (px, py) = ball_place(w, x, b)?;
            (f64::from(px), f64::from(py), f64::from(b.facing))
        }
        WorldActorState::CannonAim(a) => (f64::from(a.x), f64::from(a.y), f64::from(a.facing)),
        WorldActorState::CannonBurst(b) => (f64::from(b.x), f64::from(b.y), f64::from(b.facing)),
        WorldActorState::DownThruster(t) => (f64::from(t.x), f64::from(t.y), f64::from(t.facing)),
        WorldActorState::TurretBomb(b) => (f64::from(b.x), f64::from(b.y), f64::from(b.facing)),
        WorldActorState::PilotReticle(r) => (f64::from(r.x), f64::from(r.y), if r.flip { -1.0 } else { 1.0 }),
        _ => return None,
    })
}

/// The Aim component's place (C017_CannonShotAim / C017_CannonBall: AimVFX.K2_SetRelativeLocation(MakeVector(Offset,
/// 0, -50)) each tick, @189 / @1395): the sim's aim.
fn aim_of(w: &World, x: &WorldActor) -> Option<(f64, f64)> {
    match &x.state {
        WorldActorState::CannonBall(b) => Some(b.aim(w, x)).map(|(a, b)| (f64::from(a), f64::from(b))),
        WorldActorState::CannonAim(a) => Some(aim_at(x, a)).map(|(a, b)| (f64::from(a), f64::from(b))),
        _ => None,
    }
}

fn comp<'a>(cs: &'a Props, name: &str) -> Option<&'a Props> {
    cs.get(name).and_then(Value::as_obj)
}

fn template(c: &Props) -> Option<&str> {
    c.get("Template").and_then(Value::as_str).filter(|t| !t.is_empty() && *t != "None")
}

/// `underRoot`: a component's place rule under the actor's root (`world_scale`: the construction script's).
fn under_root(c: Option<&Props>, world_scale: Option<f64>, aim: bool) -> Option<(String, Place)> {
    let c = c?;
    let s = world_scale.map_or_else(|| xyz(c.get("RelativeScale3D"), 1.0), |v| [v; 3]);
    let place =
        Place::Root { rl: xyz(c.get("RelativeLocation"), 0.0), own: rot_rows(c.get("RelativeRotation")), s, aim };
    Some((template(c)?.to_owned(), place))
}

impl IronGiant {
    /// The S04 electricity's live systems on pawn `id`.
    #[cfg(test)]
    pub(crate) fn electricity_live(&self, id: i32, player: &mvs_vfx::player::Player) -> usize {
        self.pawns.get(&id).map_or(0, |p| p.electricity.iter().flatten().filter(|h| player.alive(**h)).count())
    }

    /// The screen effect: each pilot's PS_C017_UI at the camera root while the pilot is placed (pilot_stage 2) on a
    /// fighter whose client events this machine runs; placed anew at each step, before the particle tick.
    fn place_screens(&mut self, fx: &mut Fx, w: &World) {
        let at = fx.camera.map(|r| {
            let rot = compose_rows(
                &rotation_rows(-std::f64::consts::PI, 0.0, 0.0),
                &[r.front[0], r.front[1], r.front[2], r.right[0], r.right[1], r.right[2], r.up[0], r.up[1], r.up[2]],
            );
            att([0, 1, 2].map(|k| r.pos[k] + SCREEN_AT * r.front[k]), &rot, None)
        });
        for fid in w.all_ids() {
            let id = w.fighter(fid).id;
            let on = pilot_stage(w, fid) == 2 && fx.own_client(w, id);
            match (on, self.screens.get(&id)) {
                // (no camera yet: made at a later step)
                (true, None) => {
                    if let Some(a) = &at {
                        let h = fx.player.spawn_attached(SCREEN, a, 1.0, &[]);
                        self.screens.insert(id, h);
                    }
                }
                (true, Some(Some(h))) => {
                    if let Some(a) = &at {
                        fx.player.attach(*h, a);
                    }
                }
                (false, Some(_)) => {
                    if let Some(Some(h)) = self.screens.remove(&id) {
                        fx.player.kill(h);
                    }
                }
                _ => {}
            }
        }
    }

    /// A new `born` of a fighter's AllyBurst buff spawns the warning on its root; each is hidden with the
    /// fighter (`hidden`: dead or invisible).
    fn place_ally_burst(&mut self, fx: &mut Fx, w: &World) {
        for fid in w.all_ids() {
            let fi = w.fighter(fid);
            let id = fi.id;
            let a = self.ally.entry(id).or_default();
            a.hs.retain(|h| fx.player.alive(*h));
            // Deviation: it is hidden with meshHidden, which also counts the smoke's per-viewer hiding.
            let hide = crate::fx::mesh_hidden(w, fid);
            for h in &a.hs {
                fx.player.set_visible(*h, !hide);
            }
            let b = fi.buffs.buffs.iter().find(|b| b.frames > 0.0 && w.names.str(b.cls) == ALLY_BURST);
            let Some(born) = b.and_then(|b| b.born) else { continue };
            if a.born == Some(f64::from(born)) {
                continue;
            }
            a.born = Some(f64::from(born));
            if fx.player.emitter_count(ALLY_WARNING).is_none() {
                warn_once(format!("Iron Giant: {ALLY_WARNING} is not loaded"));
                continue;
            }
            let team = fx.team(id);
            if let Some(h) = fx.spawn_follow(
                w,
                ALLY_WARNING,
                Follow::RootScaled { id, loc: [0.0, 0.0, 75.0] },
                1.0,
                &[("Team", team)],
            ) {
                if hide {
                    fx.player.set_visible(h, false);
                }
                a.hs.push(h);
            }
        }
    }

    /// `pawn` and `turret`: each Giant's foot rockets and the turret's lead-in.
    fn place_pawns(&mut self, fx: &mut Fx, w: &World) {
        for fid in w.all_ids() {
            let Some(gs) = giant_of(w, fid) else { continue };
            let id = w.fighter(fid).id;
            let f = match self.flames.entry(id) {
                std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::btree_map::Entry::Vacant(e) => e.insert(Flames::new(fx, w, fid)),
            };
            if let Some(f) = f {
                f.begin();
            }
            let p = self.pawns.entry(id).or_default();
            let fuel = gs.hover_rage.as_ref().map_or(0.0, |h| h.fuel_fraction());
            for (k, socket) in ROCKET_SOCKETS.iter().enumerate() {
                if p.rockets[k].is_none_or(|h| !fx.player.alive(h)) {
                    let team = fx.team(id);
                    let f = fx.socket_follow(w, id, Some(socket), [0.0; 3], &IDENTITY, [ROCKET_SCALE; 3]).copy();
                    p.rockets[k] = fx.attach_to_mesh(w, id, ROCKET, socket, f, &[("Team", team)]);
                }
                if let Some(h) = p.rockets[k] {
                    fx.player.set_visible(h, gs.gliding);
                    fx.player.set_param(h, "Scale", f64::from(fuel));
                }
            }
            if fx.pawns.get(&id).is_some_and(|p| p.slug.eq_ignore_ascii_case(S04)) {
                for (k, (ps, socket)) in ELECTRICITY.iter().enumerate() {
                    if p.electricity[k].is_none_or(|h| !fx.player.alive(h)) {
                        let f = fx.socket_follow(w, id, Some(socket), [0.0; 3], &IDENTITY, [1.0; 3]).copy();
                        p.electricity[k] = fx.attach_to_mesh(w, id, ps, socket, f, &[]);
                    }
                }
            }
            let charging = gs.ally_turret.as_ref().is_some_and(|t| t.bomb > 0);
            if charging && !p.charging {
                let f = fx.socket_follow(w, id, Some(EMBLEM), [150.0, 0.0, 0.0], &IDENTITY, [1.5; 3]);
                p.charge = fx.attach_to_mesh(w, id, CHARGE, EMBLEM, f, &[]);
            } else if !charging
                && p.charging
                && let Some(h) = p.charge.take()
            {
                fx.player.kill(h);
            }
            p.charging = charging;
        }
    }

    /// ACTOR_FX's `make`: the actor's components and how each is placed.
    fn make(fx: &mut Fx, w: &World, x: &WorldActor, cs: &Props) -> Vec<(String, Place, f64)> {
        let mut out = Vec::new();
        let mut put = |p: Option<(String, Place)>| {
            if let Some((t, p)) = p {
                out.push((t, p, 1.0));
            }
        };
        match &x.state {
            // C017_AntenaBolts (attached to the owner's mesh at fml_un_vr_spine3, identity): ParticleSystem
            // (PS_C017_SPN_Pulse, scale 2.75) and ElectricRing (PS_C017_ElectricRing, bVisible false), Team on both
            // (@13105 / @13163). The ring is shown with the ProtectiveBuff (@8660, bolts > 0) and its relative scale is
            // Timeline_0's ExpandAlpha (@13298). The bolts: SpawnEmitterAttached(PS_C017_SPN_BoltElec, the bolt,
            // None, 0, 0, scale 0.6, KeepRelativeOffset) with Team (@4614), under the bolt's scale 10.
            // Unverified: the ring is hidden again at @9608 (with the ShieldWidget, when a counter falls below 0; not
            // decoded): taken as the last bolt lost (the sim's unbind). The hidden ring ticks unseen. The Capsule's
            // scale is absolute: the bone's does not reach the components (the sim's sizeScale does).
            // ReceiveBeginPlay @6536: SpawnEmitterAttached(PS_C017_Burst_Warning_01, the owner's main mesh,
            // giantEmblem_att, 0, 0, scale 1, KeepRelativeOffset, auto destroy), Team @6837: the pawn's component, so
            // it outlives the actor.
            WorldActorState::AntenaBolts(a) => {
                let o = w.fighter(x.owner).id;
                let team = fx.team(o);
                let f = fx.socket_follow(w, o, Some(EMBLEM), [0.0; 3], &IDENTITY, [1.0; 3]);
                fx.attach_to_mesh(w, o, "PS_C017_Burst_Warning_01", EMBLEM, f, &[("Team", team)]);
                for (name, ring) in [("ParticleSystem", false), ("ElectricRing", true)] {
                    let Some(c) = comp(cs, name) else { continue };
                    let Some(t) = template(c) else { continue };
                    let base = xyz(c.get("RelativeScale3D"), 1.0)[0];
                    let f = fx.socket_follow(w, o, Some("fml_un_vr_spine3"), [0.0; 3], &IDENTITY, [1.0; 3]);
                    out.push((t.to_owned(), Place::Spine { f: Box::new(f), base, ring }, 1.0));
                }
                let n = mvs_view::bolt_count(a);
                for j in 0..n {
                    out.push(("PS_C017_SPN_BoltElec".to_owned(), Place::Bolt { j, n }, 1.0));
                }
            }
            // C017_CannonShotAim / C017_CannonBall: AimVFX at the sim's aim; Team @2427 / @3457 and in the
            // construction script.
            WorldActorState::CannonAim(_) | WorldActorState::CannonBall(_) => {
                put(under_root(comp(cs, "AimVFX"), None, true))
            }
            // PS_IronGiant_CanonImpact (PS_C017_CanonImpact at (0, 0, -50)); the construction script sets its world
            // scale 1, Team @233. Unverified: the actor's rotation is taken as its facing's yaw (the sim keeps no
            // pitch for it).
            WorldActorState::CannonBurst(_) => put(under_root(comp(cs, "PS_IronGiant_CanonImpact"), Some(1.0), false)),
            // C017_DownThrusterAttack: the two PS_C017_Boosters at (+-150, 0, 0) (Team @5869 / @5927) and the two
            // PS_C017_NADAIR_ThrusterWind (scale 0.5, Wind1 yawed 180; no Team). Every tick (@3330..3710,
            // @4986..5366, with or without a floor hit) each wind gets K2_SetWorldLocation(SecondaryTransforms.
            // Location.X + 50, HitLocation.Y + SelectFloat(-100, 100, FacingLeft for the left foot / not FacingLeft
            // for the right), HitLocation.Z), and TraceFlame leaves HitLocation (0, 0, 0) without a hit (@5147): the
            // wind is at the world's origin then. The sim's boxes are left main, left side, right main, right side;
            // a foot hit the floor when its side box is not its main box, at the main box's lower end.
            // Unverified: the first frame's place (before the actor's first tick) is the component's own.
            WorldActorState::DownThruster(_) => {
                put(under_root(comp(cs, "PS_IronGiant_Boosters"), None, false));
                put(under_root(comp(cs, "PS_IronGiant_Boosters1"), None, false));
                for (k, name) in ["PS_IronGiant_BoostersWind1", "PS_IronGiant_BoostersWind2"].iter().enumerate() {
                    let Some(c) = comp(cs, name) else { continue };
                    let Some(t) = template(c) else { continue };
                    let place = Place::Wind {
                        k,
                        own: rot_rows(c.get("RelativeRotation")),
                        s: xyz(c.get("RelativeScale3D"), 1.0),
                    };
                    // (no Team: the winds' graph writes none)
                    out.push((t.to_owned(), place, -1.0));
                }
            }
            // C017_Ally_Turret_Bomb (the sim's TurretBomb). Its one drawn component is PS_C017_Turret_Projectile01
            // (PS_C017_SPF_Projectile, no relative transform) on the HitBox (relative scale 0.5) on the Capsule root
            // (bAbsoluteScale); the two sphere meshes have bVisible false and bHiddenInGame. The Capsule's relative
            // scale, which bAbsoluteScale makes its world scale: the sim's `scale`. Rotation: zero (Comp_C017_Ally_
            // Turret @671, MoveComponentTo's target @2666): the world's axes for either facing. Team (@3881-3932).
            // Unverified: drawn at the sim's tick place, and the template's bStartWithTickEnabled false is taken as
            // every auto-activated component's here.
            WorldActorState::TurretBomb(_) => {
                const BOMB_FX: &str = "PS_C017_Turret_Projectile01";
                if let Some(c) = comp(cs, BOMB_FX)
                    && let Some(t) = template(c)
                {
                    let par: AttachParentOut = attach_parent(cs, BOMB_FX);
                    let (rl, rs) = (xyz(c.get("RelativeLocation"), 0.0), xyz(c.get("RelativeScale3D"), 1.0));
                    let pr: Rows = std::array::from_fn(|k| par.rot[k]).map(f64::from);
                    let rel = [0, 1, 2].map(|k| {
                        f64::from(par.loc[k])
                            + f64::from(par.scale[0]) * rl[0] * pr[k]
                            + f64::from(par.scale[1]) * rl[1] * pr[3 + k]
                            + f64::from(par.scale[2]) * rl[2] * pr[6 + k]
                    });
                    let rot = compose_rows(&rot_rows(c.get("RelativeRotation")), &pr);
                    let base = [0, 1, 2].map(|k| f64::from(par.scale[k]) * rs[k]);
                    out.push((
                        t.to_owned(),
                        Place::Bomb { rel: rel.map(f64::from), rot, base: base.map(f64::from) },
                        1.0,
                    ));
                }
            }
            // C017_Pilot_Reticle: AimReticule (PS_C017_UI_Reticule) on the Capsule root (bAbsoluteScale) at relative
            // (0, 0, -1200.69), yaw 90, scale 1.5. The actor's rotation is its spawn's: yaw 90 for an owner facing
            // left, -90 facing right (Comp_Spawner @1385-1432); ReceiveBeginPlay @1017-1077: for an owner facing left
            // AimReticule.K2_SetRelativeRotation((0, -90, 0)), so the component's world yaw is 0 for either facing.
            // Both read the owner's facing at the spawn: the sim's `flip`. No graph writes a Team on it.
            WorldActorState::PilotReticle(r) => {
                if let Some(c) = comp(cs, "AimReticule")
                    && let Some(t) = template(c)
                {
                    let actor = yaw_rows(if r.flip { -1.0 } else { 1.0 });
                    let own = if r.flip {
                        rotation_rows(0.0, -std::f64::consts::FRAC_PI_2, 0.0)
                    } else {
                        rot_rows(c.get("RelativeRotation"))
                    };
                    let rot = compose_rows(&own, &actor);
                    let rl = xyz(c.get("RelativeLocation"), 0.0);
                    let rel = [0, 1, 2].map(|k| rl[0] * actor[k] + rl[1] * actor[3 + k] + rl[2] * actor[6 + k]);
                    out.push((
                        t.to_owned(),
                        Place::Reticle { rel, rot, scale: xyz(c.get("RelativeScale3D"), 1.0) },
                        -1.0,
                    ));
                }
            }
            _ => {}
        }
        out
    }

    /// Where component `p` of actor `x` is this tick, and whether it is shown.
    fn at(fx: &Fx, w: &World, x: &WorldActor, p: &Place) -> Option<(Attachment, bool)> {
        let (ax, ay, facing) = root(w, x)?;
        let g = x.size_scale.unwrap_or(1.0);
        Some(match p {
            Place::Root { rl, own, s, aim } => {
                let rot = yaw_rows(facing);
                let (mut loc, l) = ([0.0, -ax, ay], *rl);
                if *aim {
                    let (mx, my) = aim_of(w, x)?;
                    loc = [0.0, -mx, my];
                } else {
                    for k in 0..3 {
                        loc[k] += f64::from(g) * (l[0] * rot[k] + l[1] * rot[3 + k] + l[2] * rot[6 + k]);
                    }
                }
                (att(loc, &compose_rows(own, &rot), Some(s.map(|v| v * f64::from(g)))), true)
            }
            Place::Wind { k, own, s } => {
                let WorldActorState::DownThruster(t) = &x.state else { return None };
                let rot = compose_rows(own, &yaw_rows(facing));
                let (Some(m), Some(side)) = (t.boxes.get(2 * k), t.boxes.get(2 * k + 1)) else {
                    return Some((att([0.0, -ax, ay], &rot, Some(*s)), true));
                };
                let hit = side.y != m.y || side.hy != m.hy;
                let d = if (facing < 0.0) == (*k == 0) { -100.0 } else { 100.0 };
                let loc = [50.0, (if hit { -m.x } else { 0.0 }) + d, if hit { m.y - m.hy } else { 0.0 }];
                (att(loc.map(f64::from), &rot, Some(*s)), true)
            }
            Place::Spine { f, base, ring } => {
                let WorldActorState::AntenaBolts(a) = &x.state else { return None };
                let at = fx.place(w, f)?;
                let s = base * if *ring { mvs_view::bolt_expand(a) } else { 1.0 } * f64::from(g);
                (Attachment { scale: Some([s; 3]), ..at }, !*ring || a.bound)
            }
            Place::Bolt { j, n } => {
                let WorldActorState::AntenaBolts(a) = &x.state else { return None };
                let (loc, rot) = mvs_view::bolt_at(a, *j, *n, mvs_view::bolt_expand(a), f64::from(g));
                let s = BOLT_SCALE * 0.6 * f64::from(g);
                (att(loc, &rot, Some([s; 3])), a.bolt_on(mvs_sim::math::narrow(*j as f64)))
            }
            Place::Bomb { rel, rot, base } => {
                let WorldActorState::TurretBomb(b) = &x.state else { return None };
                let s = b.scale;
                (
                    att(
                        [rel[0] * f64::from(s), rel[1] * f64::from(s) - ax, rel[2] * f64::from(s) + ay],
                        rot,
                        Some(base.map(|v| v * f64::from(s))),
                    ),
                    true,
                )
            }
            Place::Reticle { rel, rot, scale } => (
                att(
                    [rel[0] * f64::from(g), rel[1] * f64::from(g) - ax, rel[2] * f64::from(g) + ay],
                    rot,
                    Some(scale.map(|v| v * f64::from(g))),
                ),
                true,
            ),
        })
    }

    /// `place`'s actor part: ACTOR_FX's make and tick per live actor; K2_DestroyActor takes the components.
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = HashSet::new();
        for (id, x) in w.actors.actors.iter() {
            if x.dead || root(w, x).is_none() {
                continue;
            }
            let r = ActorRef::Actor(id);
            seen.insert(r);
            if self.live.get(&r).is_none_or(|l| l.seq != x.seq) {
                if let Some(old) = self.live.remove(&r) {
                    kill_all(fx, &old);
                }
                let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
                let def = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(from), x.cls);
                let empty = Props::new();
                let cs = def.map_or(&empty, |d| &d.components);
                let comps = Self::make(fx, w, x, cs);
                let team = fx.team(x.owner_id);
                let mut hs = Vec::with_capacity(comps.len());
                for (t, p, tm) in comps {
                    let h = Self::at(fx, w, x, &p).and_then(|(at, _)| {
                        let params: &[(&str, f64)] = if tm < 0.0 { &[] } else { &[("Team", team)] };
                        fx.player.spawn_attached(&t, &at, 1.0, params)
                    });
                    hs.push((h, p));
                }
                let arm = Arm::new(fx, x, cs);
                self.live.insert(r, Live { seq: x.seq, hs, arm });
            }
            let Some(l) = self.live.get(&r) else { continue };
            if let Some(a) = &l.arm {
                a.place(fx, x);
            }
            if let WorldActorState::DownThruster(t) = &x.state
                && let Some(Some(f)) = self.flames.get_mut(&x.owner_id)
            {
                f.place(fx, t);
            }
            for (h, p) in &l.hs {
                let Some(h) = *h else { continue };
                if let Some((at, shown)) = Self::at(fx, w, x, p) {
                    fx.player.attach(h, &at);
                    fx.player.set_visible(h, shown);
                }
            }
        }
        let gone: Vec<ActorRef> = self.live.keys().filter(|r| !seen.contains(*r)).copied().collect();
        let mut gone: Vec<Live> = gone.iter().filter_map(|r| self.live.remove(r)).collect();
        gone.sort_by_key(|l| l.seq);
        for l in &gone {
            kill_all(fx, l);
        }
        for f in self.flames.values().flatten() {
            f.end(fx);
        }
    }
}

const BOLT_SCALE: f64 = 10.0;

fn kill_all(fx: &mut Fx, l: &Live) {
    for h in l.hs.iter().filter_map(|(h, _)| *h) {
        fx.player.kill(h);
    }
    if let Some(a) = &l.arm {
        fx.components.remove(a.id);
    }
}

impl CharFxHooks for IronGiant {
    /// The down thruster's flame planes and the pilot reticle's arm (the warm-up, `Fx::warm`).
    fn warm(&mut self, _fx: &mut Fx, w: &World, out: &mut Vec<mvs_view::WarmMesh>) {
        let Some(f) = fighter_of(w, mvs_sim::chars::iron_giant::KEY) else { return };
        let Some(actors) = w.char_data(f).fighter.actors.as_ref() else { return };
        if let Some(a) = actors.get("C017_DownThrusterAttack_C") {
            for c in FLAME_PLANES.iter().filter_map(|n| comp(&a.components, n)) {
                let sm = c.get("StaticMesh").and_then(Value::as_str);
                let mat = c.get("OverrideMaterials").and_then(Value::as_arr).and_then(|m| m.first()?.as_str());
                if let (Some(sm), Some(mat)) = (sm, mat) {
                    let mesh = object_path(&format!("{FLAME_MESHES}{sm}"));
                    out.push(component_warm(Some(&mesh), &format!("{FLAME_MATERIALS}{mat}#local")));
                }
            }
        }
        for a in actors.values() {
            if let Some(sm) = comp(&a.components, "ReticuleArm").and_then(|c| c.get("StaticMesh")?.as_str()) {
                out.push(component_warm(Some(&object_path(&format!("{ARM_MESHES}{sm}"))), ARM_MAT));
            }
        }
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_pawns(fx, w);
        if !self.pawns.is_empty() {
            self.place_screens(fx, w);
        }
        self.place_ally_burst(fx, w);
        self.place_actors(fx, w);
    }

    /// The Lab's reset: new pawns (the player's systems cleared), the World's actors gone (the reticle's arm with
    /// them), the flame planes hidden.
    fn reset(&mut self, fx: &mut Fx) {
        let mut arms: Vec<(i32, CMeshId)> =
            self.live.drain().filter_map(|(_, l)| l.arm.map(|a| (l.seq, a.id))).collect();
        arms.sort_by_key(|a| a.0);
        for (_, id) in arms {
            fx.components.remove(id);
        }
        self.pawns.clear();
        self.screens.clear();
        for a in self.ally.values_mut() {
            for h in a.hs.drain(..) {
                fx.player.kill(h);
            }
            a.born = None;
        }
        for f in self.flames.values_mut().flatten() {
            f.begin();
            f.end(fx);
        }
    }

    fn end(&mut self, fx: &mut Fx) {
        let mut live: Vec<Live> = self.live.drain().map(|(_, l)| l).collect();
        live.sort_by_key(|l| l.seq);
        for l in &live {
            kill_all(fx, l);
        }
        // (Fx::end takes every component mesh)
        self.flames.clear();
        for h in std::mem::take(&mut self.screens).into_values().flatten() {
            fx.player.kill(h);
        }
    }
}

// ---- the classes' projectile rules --------------------------------------------------------------

/// The plasma shot's growth: Timeline GowAlpha's CurveFloat_0 keys (time, value, arrive, leave), cubic.
const PLASMA_GROW: [[f64; 4]; 2] = [[0.0, 0.0, 4.917335, 4.9173374], [0.35, 1.0, 0.0, 0.0]];

/// C017_DiscShot BeginPlay @596..656: with the owner facing left the component's relative location (-40, 0, 0)
/// becomes (40, 0, 0) (the mesh is yawed -90: the system stays on the same side of the play plane).
fn disc_rel(w: &World, pr: &Projectile) -> Option<V3> {
    let owner = pr.owner?;
    (w.fighter(owner).body.facing_dir < 0.0).then_some([40.0, 0.0, 0.0])
}

/// C017_Plasma_Shot: world scale 1 at construction, then under the growing mesh (0.2 + 0.8 x GowAlpha).
fn plasma_parent(_w: &World, pr: &Projectile, cs: &Props, name: &str) -> AttachParentOut {
    let mut par = attach_parent(cs, name);
    let s = 0.2 + 0.8 * mvs_view::giant_cubic(&PLASMA_GROW, (f64::from(pr.age) * STEP).min(0.35));
    for v in par.scale.iter_mut() {
        *v = mvs_sim::math::narrow(f64::from(*v) * s);
    }
    par
}

/// WeaponModeMissile: its components under the turning StaticMesh (mvs-view's missile roll).
fn missile_parent(w: &World, pr: &Projectile, cs: &Props, name: &str) -> AttachParentOut {
    let mut cs = cs.clone();
    if let Some(Value::Obj(sm)) = cs.get_mut("StaticMesh") {
        let mut r = Props::new();
        r.insert("Pitch".to_owned(), Value::Num(-90.0));
        r.insert("Yaw".to_owned(), Value::Num(0.0));
        r.insert("Roll".to_owned(), Value::Num(mvs_view::missile_roll(w, pr)));
        sm.insert("RelativeRotation".to_owned(), Value::Obj(r));
    }
    attach_parent(&cs, name)
}

/// The missile's trail is shown with the launch (Delay(DurationToGravity) -> Delay(0.033) ->
/// ParticleSystem.SetVisibility(true) @178): the roll back to 0 at the same tick.
// Unverified: the hidden trail is started with the actor and ticks unseen.
fn missile_shown(w: &World, pr: &Projectile) -> bool {
    mvs_view::missile_launched(w, pr)
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    // Construction script @10: ParticleSystem.SetWorldScale3D(1) (the StaticMesh parent has scale 8). Team @306.
    ("C017_CannonShot_C", ProjRule { world_scale: Some(1.0), ..ProjRule::NONE }),
    // The same (@10), Team @439.
    ("C017_DiscShot_C", ProjRule { world_scale: Some(1.0), rel_loc: Some(disc_rel), ..ProjRule::NONE }),
    // World scale 1 at construction, then under the growing mesh; no Team in the graph.
    (
        "C017_Plasma_Shot_C",
        ProjRule { world_scale: Some(1.0), no_team: true, parent: Some(plasma_parent), ..ProjRule::NONE },
    ),
    // Construction script: SetWorldScale3D(0.5); Team @3435. The Capsule's scale is the projectile's own.
    ("C017_Bomb_C", ProjRule { world_scale: Some(0.5), ..ProjRule::NONE }),
    // ParticleSystem (PS_C017_RocketTrail, bVisible false until the launch) and PS_TomJerry_RocketHitRibbon
    // (PS_C017_RocketRibbon), both under the turning StaticMesh; SetTeamColor @1439..2213 writes Team on both.
    (
        "WeaponModeMissile_C",
        ProjRule {
            parent: Some(missile_parent),
            shown: Some(missile_shown),
            more: &["PS_TomJerry_RocketHitRibbon"],
            ..ProjRule::NONE
        },
    ),
    // ParticleSystem (PS_C017_ScrapRibbon, scale (2, 1, 1) under the empty StaticMesh) and PS_ScrapGlow
    // (PS_Finn_CoinGlow00 on the Capsule, yaw -90); Team on the glow @2075 (the ribbon takes the parent class's).
    // Deviation: the glow's TranslucencySortPriority -1 is not applied (it is sorted with the other systems).
    ("C017_ThrownScrap_C", ProjRule { more: &["PS_ScrapGlow"], ..ProjRule::NONE }),
    // The ally turret's shot: a ParentProjectilePhysics child with no graph of its own (the component template alone).
    ("C017_TenticalFastShot_C", ProjRule::NONE),
];
