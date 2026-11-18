//! Iron Giant's weapon mode aimers and the cannon's burst (with
//! `mod.rs`'s fireCannon, fireDisc, burstHits and the actor entries C017_CannonShotAim_C, C017_EnergyDiscAimer_C,
//! C017_PlasmaAimer_C, C017_CannonExplotion_C): [`CannonAim`], [`DiscAim`], [`PlasmaAim`] (with their `Arrow`) and
//! [`CannonBurst`], world actors.
//!
//! The shared code reaches the cannon aim's place ([`cannon_aim_place`]) and the burst's spawn at the cannon shot's
//! death (`world::spawning` spawn_at_death: [`spawn_burst`]); the burst's place, dealer fields and packet are its
//! own fields.
//!
//! The cannon aim's spawn: AnimNotify_Spawner without AttachToActor and with socket None (Comp_Spawner
//! HandleActorPostSpawn 978-2417): the owner's actor location - (0, 0, 100) + the notify's translation (0, 0, -300),
//! rotation (0, 0, yaw 90 when the owner faces left, else -90): the actor's local X is the owner's facing at the
//! spawn, and it does not follow the owner. Its Aim component is at relative (CurrentOffset, 0, -50).
//! ReceiveBeginPlay: FacingLeft = FacingLeftAtStart = the owner's, CurrentOffset = StartingOffset.
//! ReceiveTick (locally controlled owner, ubergraph 2576-4181): MoveXAxisValue turned around when FacingLeft;
//! nothing when it is 0. step = MoveSpeed x DeltaSeconds; a stick towards the cached facing adds it to
//! CurrentOffset; a stick away takes it off only while the owner's Y <= the Aim's Y + step (FacingLeftAtStart) or
//! the owner's Y >= the Aim's Y + step (not), read from the Aim's place before this tick's write. Then (2922) the
//! actor's Z =
//! min(the owner's Z - 390, its own).
//! ActivateObject (932-2179; AnimNotify_ActivateSpawned): nothing while the owner is in hitstun; else the unit
//! vector from the owner's mesh socket fml_un_R_wrist to the Aim, its X dropped and its Y turned into the owner's
//! current facing frame, goes to the owner's Comp_Spawner.CompSpawnActor(C017_CannonShot_C, socket
//! fml_un_R_wrist, direction, 5000, 5000) and the actor destroys itself ([`fire_cannon`]). MaxRange (2500) is read
//! by nothing.
//! MulticastSetOffset (241-885), at each tick's offset write: the AnimBP's BPI_SetAimAlpha2 (the arm's aim, sim
//! state: `pose.rs`) takes clamp01 of ang / 90 under 180 and (360 - ang) / 180 from it, ang =
//! DegAtan2(d.Y, d.Z) + 180 with d the direction from the owner's mesh socket fml_un_R_wrist to the Aim, in the
//! world (the build's Y = -x: the graph does not turn it by the facing). The write at ReceiveBeginPlay
//! (ServerSetOffset(StartingOffset)) comes before AnimInstance is set and reaches nothing.
// Deviation: the actor is ticked with the other spawned actors, after the fighters; the AimVFX cursor is not
// ported; the wrist is fireCannon's (fml_un_R_wrist at either facing), from the pose as it is at the actor's tick.

use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, ProjectileExt, make_projectile};
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::elem::{charge_buff, charge_pct, instigator_charge};
use crate::combat::hits::{
    actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, hit_buffs_ally, hit_buffs_call, projectile_blocks,
};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::{put_pose, sampled_boxes};
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::{SpawnRequest, reset_aim};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_WORLD_DYNAMIC, box_overlap_actors};
use crate::fighter::{
    FighterId, actor_y, cap_radius, capsule_half, facing, in_hitstun, mesh_wx, mesh_wy, no_collision, no_hit,
};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, rotate_angle_axis, round_to_int, safe_normal};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{all_fighters, arr, flag, num, num_in, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{blast_box_loop, new_blast_bounds, spawn_box};
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, add_actor, after_spawned_actor, new_world_actor, spawn_spot};
use indexmap::IndexSet;
use mvs_data::value::{Props, Value};
use std::sync::Arc;

/// `CannonAim` (C017_CannonShotAim_C). (`x`, `y`) is the actor's place; the Aim component is at relative
/// (`offset`, 0, -50) at the root's scale.
#[derive(State, Debug, Default)]
pub struct CannonAim {
    pub x: Real,
    pub y: Real,
    /// The owner's at the spawn (the actor's local X).
    pub facing: Real,
    /// FacingLeft / FacingLeftAtStart.
    pub facing_left: bool,
    /// CurrentOffset.
    pub offset: Real,
    pub age: i32,
}

/// `Arrow`: the aimers' arrow (C017_PlasmaAimer_C's and C017_EnergyDiscAimer_C's, statement for statement).
///
/// C017_PlasmaAimer_C (the weapon mode side special): the Spawner notify has StoreAimInfo, BPI_SetAimInfo
/// (ubergraph 39-1287) copies the notify's AimInfo (angles -45..45, AimSpeed 50) over the class defaults and sets
/// the arrow to StartAimDirection (0, 1, 0), its Y kept when the owner faces left and negated otherwise (a world
/// vector; sim x = -Y), and FacingLeft.
/// ReceiveTick (1983-6168): the notify's LockAimDirection has no effect (`if not (LockAimDirection) goto 2827` at
/// 2804, 2827 being the next statement), so the arrow turns as ProjectileAimer's does (`aimer_tick`): on the left
/// stick MoveXAxis is SelectFloat(0, MoveX, true) = 0, so only MoveY counts; the input angle is
/// DegAtan2(0, -sign(MoveY)) and the arrow turns away from it at AimSpeed deg/s (stick up tilts the shot up), held
/// without input, with a difference >= 179, or within 1 degree (rounded) of the limit on that side, the step
/// clamped to the rounded distance to that limit. The arrow's X is dropped; it is not normalised. FacingLeft is the
/// value stored by BPI_SetAimInfo.
/// The owner's AnimBP: ReceiveBeginPlay (1292-1845) calls BPI_SetAimDirection(0); every tick ends in
/// SetArrowRotationServer -> SetArrowRotationMulticast (5129, 9728-11414), turned or held:
/// BPI_SetAimDirection(NormalizeToRange(angle, start + AimInfo.AimAngleMin, start + AimInfo.AimAngleMax) x 2 - 1),
/// the angles DegAtan2(Z, FacingLeft ? Y : -Y) of the arrow and of StartAimVector (the elevation over the span: 1
/// at the upper limit, -1 at the lower), not clamped. C017_AnimBP keeps it as AimDirection / AimDirectionInverted
/// (4843, 12791), the alphas of the WeaponAdditive / AimUp / AimDown slots. The same call sets the owner's
/// Comp_Spawner.ProjectileAimVector (10126; read by UseAimActorForLaunch, off in his notifies).
/// What is drawn: nothing. The actor's Arrow (ArrowComponent), Sphere (StaticMesh Sphere with AimPunchMat) and
/// SkillShotIndicator (WidgetComponent, UI_Skillshot_Aimpunch_Target, 128 x 128, screen space) all have bVisible
/// false in the class; the graph's only visibility call is TurnOnArrowUI's Arrow.SetVisibility(false) (11431), and
/// UpdateSkillshotUI (7810: Widget_AimPunchLine, PS_SupermanSkillBounds) has no caller in this class. HideUI (9340)
/// destroys PS_SkillBounds, never made, and the rest is for fighter character 10. The aim shows through the pose
/// alone.
// Unverified: the aimer is taken to tick every frame from its spawn frame on, with the other spawned actors; doubles
// instead of floats.
#[derive(State, Debug, Default)]
pub struct Arrow {
    /// The owner's at BPI_SetAimInfo: the arrow's side.
    pub facing: Real,
    /// StartAimVector (UE Y, Z).
    pub sy: Real,
    pub sz: Real,
    /// ArrowAimVector.
    pub ay: Real,
    pub az: Real,
    /// The stored notify's properties (StoreAimInfo's `AimInfo` and the rest the shot is spawned with).
    pub notify: Arc<Props>,
}

/// The aimer's AimInfo number.
fn info_num(n: &Props, k: &str) -> Real {
    num_in(n, "AimInfo", k).unwrap_or(0.0)
}

impl Arrow {
    /// BPI_SetAimInfo and ReceiveBeginPlay (the owner's `AimDirection` write is the caller's).
    fn new(owner_facing: Real, notify: Arc<Props>) -> Arrow {
        let s = obj(&notify, "AimInfo").and_then(|i| obj(i, "StartAimDirection"));
        let (sy, sz) = (s.and_then(|s| num(s, "Y")).unwrap_or(0.0), s.and_then(|s| num(s, "Z")).unwrap_or(0.0));
        let sy = if owner_facing < 0.0 { sy } else { -sy };
        Arrow { facing: owner_facing, sy, sz, ay: sy, az: sz, notify }
    }

    /// ReceiveTick: the arrow turned with the stick's Y `my`; the value the owner's AimDirection takes.
    fn step(&mut self, my: Real) -> Real {
        self.turn(my);
        let n = &*self.notify;
        let fl = self.facing < 0.0;
        let el = |vy: Real, vz: Real| deg_atan2(vz, if fl { vy } else { -vy });
        let (min_a, max_a) = (info_num(n, "AimAngleMin"), info_num(n, "AimAngleMax"));
        let lo = el(self.sy, self.sz) + min_a;
        let hi = el(self.sy, self.sz) + max_a;
        let ang = el(self.ay, self.az);
        let alpha = if lo == hi { if ang >= hi { 1.0 } else { 0.0 } } else { (ang - min(lo, hi)) / (hi - lo).abs() };
        alpha * 2.0 - 1.0
    }

    fn turn(&mut self, my: Real) {
        let n = &*self.notify;
        let fl = self.facing < 0.0;
        let (max_a, min_a) = (info_num(n, "AimAngleMax"), info_num(n, "AimAngleMin"));
        // DegAtan2, and RotateAngleAxis about (1, 0, 0) (the aimer family's nodes)
        let deg = |y: Real, z: Real| deg_atan2(y, z);
        let rot = |y: Real, z: Real, d: Real| {
            let r = rotate_angle_axis([0.0, y, z], d, [1.0, 0.0, 0.0]);
            (r[1], r[2])
        };
        // { mult, pos, diff }
        let near = |a1: Real, a2: Real| {
            let d0 = a1 + 180.0 - (a2 + 180.0);
            let s = if d0.abs() < (d0 - 360.0).abs() { d0 } else { d0 - 360.0 };
            let r = if s.abs() < (d0 + 360.0).abs() { s } else { d0 + 360.0 };
            (if r < 0.0 { 1.0 } else { -1.0 }, r < 0.0, crate::real(round_to_int(r)))
        };
        let cur = deg(self.ay, self.az);
        let la = rot(self.sy, self.sz, if fl { min_a } else { -max_a });
        let lb = rot(self.sy, self.sz, if fl { max_a } else { -min_a });
        let da = near(cur, deg(la.0, la.1));
        let db = near(cur, deg(lb.0, lb.1));
        // Deviation: no right stick path (unused by his notifies).
        let my = if obj(n, "AimInfo").is_some_and(|i| flag(i, "UseRightStickToAim")) { 0.0 } else { my };
        let dt = near(
            cur,
            deg(
                0.0,
                if my == 0.0 {
                    0.0
                } else if my > 0.0 {
                    -1.0
                } else {
                    1.0
                },
            ),
        );
        let sel = (if dt.1 { db.2 } else { da.2 }).abs();
        if dt.2.abs() >= 179.0 || (max_a != min_a && sel <= 1.0) || my == 0.0 {
            return;
        }
        let step = num_in(n, "AimInfo", "AimSpeed").unwrap_or(0.0) * DELTA_SECONDS;
        let by = (if max_a == min_a { step } else { min(max(step, 0.0), sel) }) * dt.0;
        (self.ay, self.az) = rot(self.ay, self.az, by);
    }
}

/// `PlasmaAim` (C017_PlasmaAimer_C).
///
/// ActivateObject (ubergraph 6280-7691): nothing while the owner is in hitstun; else the owner's
/// Comp_Spawner.CompSpawnActor(C017_Plasma_Shot_C, the stored AttachToActor / ParentActor / AttachSocket /
/// WorldTransforms / SetTransforms, the arrow's vector with its Y turned into the owner's current facing frame,
/// the stored InitialSpeed x PowerMultiplier (1: LaunchPowerMultSpeed is 0), max(the stored MaxSpeed, that), the
/// stored charge fields and AttackID), then K2_DestroyActor ([`fire_plasma`]).
#[derive(State, Debug, Default)]
pub struct PlasmaAim {
    pub attack_id: i32,
    /// StoredAimActor: the Spawner notify's properties.
    pub notify: Arc<Props>,
    pub arrow: Arrow,
}

/// The disc aimer's shots' directions, in the owner's facing frame and not unit vectors (the launch speed is 6000 x
/// their size).
pub const DISC_DIRS: [[Real; 3]; 3] = [[0.0, 0.6, -0.4], [0.0, 0.65, -0.35], [0.0, 0.75, -0.25]];

/// `DiscAim` (C017_EnergyDiscAimer_C, the weapon mode side normal). The Spawner notify has StoreAimInfo, so the
/// owner's Comp_Spawner keeps its data as StoredAimActor.
/// ActivateObject (ubergraph 2474-2775, then 15-1221): nothing while the owner is in hitstun; behind a once-gate,
/// for i = 0 .. NumberShot (2: three shots): the owner's Comp_Spawner.CompSpawnActor(C017_DiscShot_C, the stored
/// AttachToActor / ParentActor / AttachSocket / WorldTransforms / SetTransforms, direction i, 6000, 6000, the stored
/// AttackID), then Delay(0.15); after the last Delay K2_DestroyActor ([`DISC_DIRS`], [`fire_disc`]).
/// The arrow turns with the stick as C017_PlasmaAimer's does (LockAimDirection branches to the same statement,
/// ubergraph 7278 / 7301), but the activation does not read it: it only feeds the AnimBP.
/// ReceiveBeginPlay (10754-11478) and ReceiveTick (6457-10642) are C017_PlasmaAimer's statement for statement
/// (BPI_SetAimDirection(0), then the arrow and BPI_SetAimDirection every tick, the Delays' ticks included): [`Arrow`].
// Unverified: the Delay counts from the activation's frame, the actor ticking after the fighters.
#[derive(State, Debug, Default)]
pub struct DiscAim {
    pub age: i32,
    /// The once-gate.
    pub active: bool,
    /// The loop's index.
    pub shot: i32,
    /// Delay(0.15), the latent ticks left (-1 before the activation).
    pub wait: Real,
    pub attack_id: i32,
    pub notify: Arc<Props>,
    /// Read by the AnimBP alone.
    pub arrow: Arrow,
}

/// `CannonBurst` (C017_CannonExplotion_C; SpawnedActorParent; spawned at C017_CannonShot's death by its
/// SpawnActorAtDeath, with the shot's transform): MaxLifetime 0.5, HitBoxOn? from the start, and one hitbox, the
/// UpwardBurst box (extent (200, 200, 400) at relative (0, 0, 367.02), tag HitBox). ReceiveBeginPlay only sets the
/// emitter's Team and posts the impact sound ([`spawn_burst`]). The class's own fields; the `SimActor` ones
/// (lifetimePause, dodge, id, dead, charge, elem, sizeScale, cls, owner) are the [`WorldActor`]'s. Its data (`p`) is
/// the class's; `vx` and `vy` are 0 (readonly).
// Deviation: the box is tested in the play plane (its 200 of depth is taken to cover every hurt capsule); its first
// tick is the world's next pass over the actors; the PS_C017_CanonImpact emitter is not drawn.
#[derive(State, Debug, Default)]
pub struct CannonBurst {
    pub age: i32,
    pub hit: HitPacket,
    /// The box's half extents in the play plane.
    pub hx: Real,
    pub hy: Real,
    /// Its centre above the actor.
    pub up_y: Real,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// The begins of the box's overlaps that are no enemy hit: the team mates' fighter ids.
    pub ally_over: IndexSet<i32>,
    /// The other actors' keys (a kind letter and the id: `shockwave::actor_key`).
    pub actor_over: IndexSet<(u8, i32)>,
    /// The handler's hit list for HitEachActorOnlyOnce on actors (`shockwave::actor_key`).
    pub actor_once: IndexSet<(u8, i32)>,
    pub life_ticks: i32,
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    /// The dying shot's pitch in degrees, for the renderer.
    pub pitch: Real,
}

// ------------------------------------------------------------------ state access

fn cannon(w: &World, id: ActorId) -> Option<&CannonAim> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::CannonAim(s) => Some(s),
        _ => None,
    }
}

fn cannon_mut(w: &mut World, id: ActorId) -> Option<&mut CannonAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::CannonAim(s) => Some(s),
        _ => None,
    }
}

fn disc_mut(w: &mut World, id: ActorId) -> Option<&mut DiscAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::DiscAim(s) => Some(s),
        _ => None,
    }
}

fn plasma_mut(w: &mut World, id: ActorId) -> Option<&mut PlasmaAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::PlasmaAim(s) => Some(s),
        _ => None,
    }
}

fn burst(w: &World, id: ActorId) -> Option<&CannonBurst> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::CannonBurst(s) => Some(s),
        _ => None,
    }
}

fn burst_mut(w: &mut World, id: ActorId) -> Option<&mut CannonBurst> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::CannonBurst(s) => Some(s),
        _ => None,
    }
}

fn kill(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The cannon aim's place (its `x`, `y`).
pub fn cannon_aim_place(_w: &World, _a: &WorldActor, s: &CannonAim) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

// ------------------------------------------------------------------ the cannon aim

/// The Aim component's place: its relative (offset, 0, -50) at the root's scale.
pub fn aim_at(a: &WorldActor, s: &CannonAim) -> (Real, Real) {
    let g = a.size_scale.unwrap_or(1.0);
    (s.x + s.offset * s.facing * g, s.y - 50.0 * g)
}

/// The owner's mesh socket fml_un_R_wrist in the play plane: the mesh's place plus the sampled pose's socket
/// (Deviation: the socket at the actor's tick, as `fire_cannon` takes it).
fn wrist(w: &World, f: FighterId) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let at = w.fixed.samplers.get(&ch).and_then(|b| {
        let si = *b.by_name.get(&name!("socket:fml_un_R_wrist"))?;
        b.offsets.get(usize::try_from(si).ok()?).copied()
    });
    let (mut wx, mut wy) = (mesh_wx(w, f), mesh_wy(w, f));
    if let Some(o) = at
        && let Some(pose) = sampled_boxes(w, f)
    {
        let o = usize::try_from(o).expect("a box offset");
        wx += crate::real(pose[o]) * facing(w, f);
        wy += crate::real(pose[o + 1]);
        put_pose(w, pose);
    }
    (wx, wy)
}

/// MulticastSetOffset's BPI_SetAimAlpha2.
fn aim_alpha(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, Some(s)) = (a.owner, cannon(w, id)) else { return };
    let (ax, ay) = aim_at(a, s);
    if !super::state::giant_of(w, f).is_some_and(|g| g.giant_anim.is_some()) {
        return;
    }
    let (wx, wy) = wrist(w, f);
    let ang = deg_atan2(-(ax - wx), ay - wy) + 180.0;
    let alpha = min(max(if ang >= 180.0 { (360.0 - ang) / 180.0 } else { ang / 90.0 }, 0.0), 1.0);
    if let Some(ga) = super::state::giant_mut(w, f).and_then(|g| g.giant_anim.as_mut()) {
        ga.aim_alpha2 = alpha;
    }
}

/// The kind's `spawn` (`new CannonAim`, PostActorConstruction's FindTeleportSpot ahead of BeginPlay).
fn cannon_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let face = facing(w, f);
    let x = w.fighter(f).body.x + t.and_then(|t| num(t, "Y")).unwrap_or(0.0) * face;
    let y = actor_y(w, f) - 100.0 + t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
    let at = spawn_spot(w, &SpawnSpotR { cls: r.cls, p: np }, &SpawnSpotA { components: a.components.clone() }, x, y);
    let s = CannonAim {
        x: at.x,
        y: at.y,
        facing: face,
        facing_left: face < 0.0,
        offset: num(&a.p, "StartingOffset").unwrap_or(0.0),
        age: 0,
    };
    (WorldActorState::CannonAim(s), false)
}

/// ReceiveTick.
fn cannon_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let speed = actor_data(w, &data, ActorRef::Fighter(f), cls).and_then(|d| num(&d.p, "MoveSpeed")).unwrap_or(0.0);
    let (mx, ox, oy) = (w.fighter(f).controls.inp.x, w.fighter(f).body.x, actor_y(w, f));
    let Some(s) = cannon(w, id) else { return };
    let fl = s.facing_left;
    // the Aim's place before this tick's write
    let (ax, _) = aim_at(a, s);
    let adj = if fl { -mx } else { mx };
    let Some(s) = cannon_mut(w, id) else { return };
    s.age += 1;
    if adj != 0.0 {
        let d = speed * DELTA_SECONDS;
        if adj > 0.0 {
            s.offset += d;
        } else if if fl { ax <= ox + d } else { ax >= ox + d } {
            s.offset -= d;
        }
        aim_alpha(w, id);
    }
    if let Some(s) = cannon_mut(w, id) {
        s.y = min(oy - 390.0, s.y);
    }
}

/// C017_CannonShotAim ActivateObject (ubergraph 932-2179): see [`CannonAim`]. The spawn goes through the owner's
/// Comp_Spawner with AttackID 0 (the owner's attack count) and the socket fml_un_R_wrist; the direction is read from
/// the main mesh's own fml_un_R_wrist (@1475). GetSocket (Comp_Spawner @0-960) leaves the name at either facing: its
/// test for the left token ignores case and finds the `l_` of `fml_`, the candidate it then builds (every `l_`
/// replaced: fmR_un_R_wrist) is no socket, and the input is returned. The main mesh has no mirror node, so the
/// socket facing left is the same bone reflected with the pawn: the sim's pose times the facing.
// Deviation: the socket's depth is left out of the unit vector, so the launch is the full 5000 (the graph takes the
// vector in 3D and drops X after, @1643-1968).
fn fire_cannon(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if in_hitstun(w, f) {
        return;
    }
    kill(w, id);
    let cls = name!("C017_CannonShot_C");
    if !crate::fighter::common::data_of(w, f)
        .fighter
        .actors
        .as_ref()
        .is_some_and(|m| m.contains_key("C017_CannonShot_C"))
    {
        return;
    }
    let (Some(a), Some(s)) = (w.actors.actors.get(id), cannon(w, id)) else { return };
    let (ax, ay) = aim_at(a, s);
    let (wx, wy) = wrist(w, f);
    let (dx, dy) = (ax - wx, ay - wy);
    let n = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetSafeNormal
    let face = facing(w, f);
    let dir = [0.0, n[1] * face, n[2]];
    let n = shot_props(None, &[], dir, 5000.0, 5000.0, Some("fml_un_R_wrist"));
    let attack = w.fighter(f).decay.attack_count;
    let pr = make_projectile(w, cls, ProjectileClass::C017CannonShot, Some(f), attack, &n, None, MakeOpts::default());
    let pr = w.actors.projectiles.insert(pr);
    crate::world::spawning::add_projectile(w, pr, None);
}

/// The notify of a shot's spawn: the stored notify's members `keys` the graph hands on, then the direction and the
/// speeds (an object literal; its undefined members are absent here). `socket`: a socket the graph names itself.
fn shot_props(
    from: Option<&Props>,
    keys: &[&str],
    dir: [Real; 3],
    speed: Real,
    max_speed: Real,
    socket: Option<&str>,
) -> Props {
    let mut p = Props::new();
    if let Some(s) = socket {
        p.insert("Socket".into(), Value::Str(s.into()));
    }
    for k in keys {
        if let Some(v) = from.and_then(|n| n.get(*k)) {
            p.insert((*k).into(), v.clone());
        }
    }
    let mut d = Props::new();
    for (k, v) in ["X", "Y", "Z"].into_iter().zip(dir) {
        d.insert(k.into(), Value::Num(f64::from(v)));
    }
    p.insert("InitialMovementDirection".into(), Value::Obj(d));
    p.insert("InitialSpeed".into(), Value::Num(f64::from(speed)));
    p.insert("MaxSpeed".into(), Value::Num(f64::from(max_speed)));
    p
}

// ------------------------------------------------------------------ the plasma and disc aimers

/// The aimers' `spawn`: `new PlasmaAim(f, r.attackId, a, r.p)` / `new DiscAim(...)`: the arrow (BPI_SetAimInfo) and
/// ReceiveBeginPlay's BPI_SetAimDirection(0).
fn aimer_notify(w: &mut World, f: FighterId, r: &SpawnRequest) -> Arc<Props> {
    let np = Arc::new(crate::world::spawning::request_props(w, f, r));
    w.fighter_mut(f).pose.aim_direction = 0.0;
    np
}

fn plasma_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let notify = aimer_notify(w, f, r);
    let arrow = Arrow::new(facing(w, f), notify.clone());
    (WorldActorState::PlasmaAim(PlasmaAim { attack_id: r.attack_id, notify, arrow }), false)
}

fn disc_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let notify = aimer_notify(w, f, r);
    let arrow = Arrow::new(facing(w, f), notify.clone());
    (
        WorldActorState::DiscAim(DiscAim {
            age: 0,
            active: false,
            shot: 0,
            wait: -1.0,
            attack_id: r.attack_id,
            notify,
            arrow,
        }),
        false,
    )
}

/// The arrow's ReceiveTick with the owner's stick; the owner's AimDirection and Comp_Spawner.ProjectileAimVector
/// (SetArrowRotationServer's multicast: C017_PlasmaAimer @10126, C017_EnergyDiscAimer @5152).
fn arrow_tick(w: &mut World, f: FighterId, arrow: &mut Arrow) {
    let my = w.fighter(f).controls.inp.y;
    let dir = arrow.step(my);
    let fi = w.fighter_mut(f);
    fi.pose.aim_direction = dir;
    fi.items.proj_aim.y = arrow.ay;
    fi.items.proj_aim.z = arrow.az;
}

fn plasma_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(s) = plasma_mut(w, id) else { return };
    let mut arrow = std::mem::take(&mut s.arrow);
    arrow_tick(w, f, &mut arrow);
    if let Some(s) = plasma_mut(w, id) {
        s.arrow = arrow;
    }
}

/// One actor tick of the disc aimer: true when the next shot is due.
fn disc_tick(w: &mut World, id: ActorId) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let f = a.owner;
    let data = w.data.clone();
    let shots = actor_data(w, &data, ActorRef::Fighter(f), a.cls).and_then(|d| num(&d.p, "NumberShot")).unwrap_or(0.0);
    let Some(s) = disc_mut(w, id) else { return false };
    s.age += 1;
    let mut arrow = std::mem::take(&mut s.arrow);
    arrow_tick(w, f, &mut arrow);
    let Some(s) = disc_mut(w, id) else { return false };
    s.arrow = arrow;
    if s.wait <= 0.0 {
        return false;
    }
    s.wait -= 1.0;
    if s.wait > 0.0 {
        return false;
    }
    s.shot += 1;
    if crate::real(s.shot) > shots {
        kill(w, id);
        return false;
    }
    s.wait = latent_frames(0.15);
    true
}

fn disc_step(w: &mut World, id: ActorId) {
    if disc_tick(w, id) {
        fire_disc(w, id);
    }
}

/// ActivateObject: the first shot, behind the once-gate.
fn disc_activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let hitstun = in_hitstun(w, f);
    let Some(s) = disc_mut(w, id) else { return };
    if s.active || hitstun {
        return;
    }
    s.active = true;
    s.shot = 0;
    s.wait = latent_frames(0.15);
    fire_disc(w, id);
}

/// ReceiveEndPlay: Comp_Spawner.ResetAim (C017_PlasmaAimer @9631, C017_EnergyDiscAimer @4657), not in the Lab
/// reset's pass.
fn aimer_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if !crate::world::flow::reset_pass(w) {
        reset_aim(w, f);
    }
}

fn sound_at(w: &mut World, fighter: i32, ev: Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// C017_EnergyDiscAimer's loop (see [`DiscAim`]): one C017_DiscShot through the owner's Comp_Spawner with the stored
/// notify's socket and transforms. C017_DiscShot ReceiveBeginPlay posts its cast sound at its emitter, ahead of the
/// test at the spawn place (DestroyOnHitFighter: its death events).
fn fire_disc(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let Some(WorldActorState::DiscAim(da)) = w.actors.actors.get(id).map(|a| &a.state) else { return };
    let cls = name!("C017_DiscShot_C");
    let Some(dir) = usize::try_from(da.shot).ok().and_then(|i| DISC_DIRS.get(i)).copied() else { return };
    if !crate::fighter::common::data_of(w, f).fighter.actors.as_ref().is_some_and(|m| m.contains_key("C017_DiscShot_C"))
    {
        return;
    }
    let n = shot_props(
        Some(&da.notify),
        &["Socket", "Transforms", "AttachToActor", "ChargeAffectsInitialSpeed"],
        dir,
        6000.0,
        6000.0,
        None,
    );
    let attack = da.attack_id;
    let mut pr = make_projectile(w, cls, ProjectileClass::C017DiscShot, Some(f), attack, &n, None, MakeOpts::default());
    // ReceiveBeginPlay @596
    pr.ext = ProjectileExt::DiscShot(super::actor_state::DiscShot {
        owner_facing: if facing(w, f) < 0.0 { -1.0 } else { 1.0 },
    });
    let pr = w.actors.projectiles.insert(pr);
    crate::world::spawning::add_projectile(w, pr, Some(disc_cast_sound));
}

fn disc_cast_sound(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (Some(f), x, y) = (p.owner, p.x, p.y) else { return };
    let fid = w.fighter(f).id;
    sound_at(w, fid, name!("sfx_cha_C017_weaponmode_energydisc_cast_shot"), x, y);
}

/// C017_PlasmaAimer ActivateObject (see [`PlasmaAim`]). C017_Plasma_Shot ReceiveBeginPlay starts its loop sound.
fn fire_plasma(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if in_hitstun(w, f) {
        return;
    }
    kill(w, id);
    let cls = name!("C017_Plasma_Shot_C");
    if !crate::fighter::common::data_of(w, f)
        .fighter
        .actors
        .as_ref()
        .is_some_and(|m| m.contains_key("C017_Plasma_Shot_C"))
    {
        return;
    }
    let Some(WorldActorState::PlasmaAim(pa)) = w.actors.actors.get(id).map(|a| &a.state) else { return };
    let speed = num(&pa.notify, "InitialSpeed").unwrap_or(0.0);
    let max_speed = max(num(&pa.notify, "MaxSpeed").unwrap_or(0.0), speed);
    let dir = [0.0, if facing(w, f) < 0.0 { pa.arrow.ay } else { -pa.arrow.ay }, pa.arrow.az];
    let n = shot_props(
        Some(&pa.notify),
        &["Socket", "Transforms", "AttachToActor", "ChargeAffectsInitialSpeed", "ChargeSpeedMultiplier"],
        dir,
        speed,
        max_speed,
        None,
    );
    let attack = pa.attack_id;
    let pr = make_projectile(w, cls, ProjectileClass::C017PlasmaShot, Some(f), attack, &n, None, MakeOpts::default());
    let pr = w.actors.projectiles.insert(pr);
    crate::world::spawning::add_projectile(w, pr, Some(super::plasma::plasma_begin));
}

// ------------------------------------------------------------------ the cannon burst

/// `spawnAtDeath`'s C017_CannonExplotion_C branch: the burst at the dying shot `pr`
/// (class `cls`), its impact sound, its post-spawn and its hits at the spawn place.
///
/// SpawnDeathActorWithThisActorsTransform: at the shot's place. C017_CannonExplotion ReceiveBeginPlay: the impact
/// sound at its PS_IronGiant_CanonImpact component (50 under the actor). The shot's own rotation: its pitch only
/// where its movement has bRotationFollowsVelocity (C017_CannonShot has not: it keeps the spawner's yaw, pitch 0, so
/// the burst stands upright).
/// Comp_Spawner HandleActorPostSpawn @12116 AfterSpawnedActor, ahead of its BPI_UpdateHitbox @12653 -> SpawnedActorParent
/// @4657 / @18336 / @15092 HitBoxesOn @8531, SetCollisionEnabled(3) @16817: the tagged UpwardBurst box overlaps from
/// the spawn on, so it is tested at its spawn place in its spawn tick, wherever in the step the shot died. The
/// regular hits pass of this step skips whom it hit (HitEachActorOnlyOnce). The other World.actors with their own hit
/// loop deal no damage through a PostSpawn box (C017_AntenaBolts, C017_ShockwaveActor, ShaggyShockwaveActor,
/// HarleyBomb).
/// C017_CannonExplotion ReceiveBeginPlay @10 calls the parent's first: GrantBuffsFromInstigator (`add_actor`), then
/// the blast-box loop at the shot's place (`world::blast::blast_box_loop`; DestroyOnExitedArena? the parent's default,
/// no ForceSpawnPositionInsideBlastBox): a shot that died outside the jump-out area leaves a burst destroyed in its
/// spawn, which deals nothing (AfterSpawnedActor skips it and its box is never tested). The class's own part (the
/// sound) runs on behind the destroy. The burst never moves, so its CheckBlastBoxExit timer never finds it outside the
/// bounds the loop stored: not kept.
// Unverified: the post-spawn BPI_UpdateHitbox is taken to make no overlap for an actor destroyed in its BeginPlay.
pub fn spawn_burst(w: &mut World, pr: ProjectileId, cls: Name) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (Some(owner), px, py, pfacing, ppitch, pcls) = (p.owner, p.x, p.y, p.facing, p.pitch, p.cls) else { return };
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(owner), cls) else { return };
    let follows = actor_data(w, &data, ActorRef::Fighter(owner), pcls)
        .and_then(|d| obj(&d.components, "ProjectileMovement"))
        .and_then(|m| m.get("bRotationFollowsVelocity"))
        .is_some_and(|v| matches!(v, Value::Bool(true)));
    let b = obj(&def.components, "UpwardBurst");
    // MaxLifetime is a timer: it fires at the first timer tick with counted deltas > its rate (as Projectile).
    let life = num(&def.p, "MaxLifetime").unwrap_or(0.0);
    let mut k = 0;
    if life > 0.0 {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the loop's test, NaN included")]
        while !(crate::consts::clock(crate::real(k)) > f64::from(life)) {
            k += 1;
        }
    }
    let state = CannonBurst {
        hit: HitPacket::from_props(&w.names, obj(&def.p, "HitBoxDamage").unwrap_or(&Props::new()), None),
        hx: b.and_then(|b| num_in(b, "BoxExtent", "Y")).unwrap_or(0.0),
        hy: b.and_then(|b| num_in(b, "BoxExtent", "Z")).unwrap_or(0.0),
        up_y: b.and_then(|b| num_in(b, "RelativeLocation", "Z")).unwrap_or(0.0),
        life_ticks: if life > 0.0 { k + 1 } else { 0 },
        attack_id: w.fighter(owner).decay.attack_count,
        x: px,
        y: py,
        facing: pfacing,
        pitch: if follows { ppitch } else { 0.0 },
        ..CannonBurst::default()
    };
    let (bx, by) = (state.x, state.y);
    let a = new_world_actor(
        w,
        cls,
        WorldActorClass::C017CannonExplotion,
        owner,
        false,
        WorldActorState::CannonBurst(state),
    );
    let id = w.actors.actors.insert(a);
    add_actor(w, id, instigator_charge(w, owner), true); // the passive buffs, SpawnedActorParent's BeginPlay
    let force = flag(&def.p, "ForceSpawnPositionInsideBlastBox");
    let doe = flag(&def.p, "DestroyOnExitedArena?");
    let (mut lx, mut ly, mut bounds) = (bx, by, new_blast_bounds());
    if blast_box_loop(&spawn_box(w), &mut lx, &mut ly, None, force, &mut bounds, doe)
        && let Some(a) = w.actors.actors.get_mut(id)
    {
        a.dead = true;
    }
    // the loop's place (moved inside with ForceSpawnPositionInsideBlastBox), where the class's part runs on
    if let Some(s) = burst_mut(w, id) {
        (s.x, s.y) = (lx, ly);
    }
    let fid = w.fighter(owner).id;
    sound_at(w, fid, name!("sfx_cha_C017_weaponsmode_cannonattack_normal_missle_impact"), lx, ly - 50.0);
    after_spawned_actor(w, owner, ActorRef::Actor(id));
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        burst_hits(w, id);
    }
}

/// One actor tick; MaxLifetime ends it.
fn burst_step(w: &mut World, id: ActorId) {
    let Some(b) = burst_mut(w, id) else { return };
    b.age += 1;
    if b.life_ticks > 0 && b.age >= b.life_ticks {
        kill(w, id);
    }
}

/// The box (centre, half extents in the play plane) over the fighter's root capsule: the actor location, the
/// current half height and radius, upright (as World.meleeTest's rootCapsuleHit); a capsule with its collision off
/// (Fighter.noCollision) is not met.
fn over_capsule(w: &World, vic: FighterId, cx: Real, cy: Real, hx: Real, hy: Real) -> bool {
    if no_collision(w, vic) {
        return false;
    }
    let r = cap_radius(w, vic);
    let h = max(capsule_half(w, vic) - r, 0.0);
    let y = actor_y(w, vic);
    let dx = max((w.fighter(vic).body.x - cx).abs() - hx, 0.0);
    let dy = max(max(y - h - (cy + hy), cy - hy - (y + h)), 0.0);
    dx * dx + dy * dy <= r * r
}

/// C017_CannonExplotion: its UpwardBurst box (tag HitBox, on for the actor's life). Also called at the spawn
/// ([`spawn_burst`]).
/// The hit box deals through Comp_Pawn_HitBoxHandler OnComponentBeginOverlap (@7466 -> pfgDoDamage @7502) with no
/// team or class filter: a team mate other than the Instigator gets NotifyAllyOfHit (@21344) at the begin of his
/// overlap (none while invulnerable: @4608 -> @7452 pops), and every other actor whose body overlaps the box gets
/// the call at its begin (`shockwave::actor_victim_hit`). The box (no BodyInstance in the template: the
/// UShapeComponent ctor's OverlapAllDynamic, object type WorldDynamic) meets a body that answers WorldDynamic with
/// Overlap or Block: not a projectile's HurtBox nor a SeismicReactor's DamageBox (both Ignore it). On a fighter
/// that is the root capsule alone (the Pawn profile Blocks WorldDynamic; OverlapAllDynamic leaves HurtBox at the
/// channel's default Ignore, DefaultEngine.ini), never the hurt shapes: [`over_capsule`].
// Unverified: OverlapAllDynamic's responses taken as the ResponseArray-less defaults.
fn burst_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls, g) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let Some(b) = burst(w, id) else { return };
    let (cx, cy, hx, hy) = (b.x, b.y + b.up_y * g, b.hx * g, b.hy * g);
    let (attack_id, hit) = (b.attack_id, b.hit.clone());
    let team = w.fighter(f).team.team;
    // a minion too (the begin overlaps meet any pawn)
    for vic in all_fighters(w) {
        let vid = w.fighter(vic).id;
        let (dead, hidden) = (w.fighter(vic).life.dead != 0.0, no_hit(w, vic));
        if w.fighter(vic).team.team == team {
            if vic == f {
                continue;
            }
            if dead || hidden || !over_capsule(w, vic, cx, cy, hx, hy) {
                if let Some(b) = burst_mut(w, id) {
                    b.ally_over.shift_remove(&vid);
                }
                continue;
            }
            let Some(b) = burst_mut(w, id) else { return };
            if !b.ally_over.insert(vid) {
                continue;
            }
            let rows = crate::combat::spawned_hits::class_rows(w, me);
            hit_buffs_call(w, f, &rows); // the entry rows (@0-1265), ahead of the pops
            if projectile_blocks(w, &dealer(w, me), vic) || crate::fighter::invulnerable(w, vic) {
                continue;
            }
            let mult = charge_buff(w, me);
            hit_buffs_ally(w, f, vic, &rows, mult); // @5931-6835
            ally_hit(w, &dealer(w, me), vic, &hit);
            continue;
        }
        let Some(b) = burst(w, id) else { return };
        if dead || hidden || b.hit_set.contains(&vid) {
            continue;
        }
        if !over_capsule(w, vic, cx, cy, hx, hy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if crate::fighter::invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(b) = burst_mut(w, id)
        {
            b.hit_set.insert(vid);
        }
        let rows = crate::combat::spawned_hits::class_rows(w, me);
        let d = dealer(w, me);
        let at = V2 { x: burst_x(w, id), y: burst_y(w, id) };
        if apply_projectile_hit(w, &d, vic, &hit, &rows, at, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    burst_actor_hits(w, id, f, cls, cx, cy, hx, hy, g);
}

fn burst_x(w: &World, id: ActorId) -> Real {
    burst(w, id).map_or(0.0, |b| b.x)
}

fn burst_y(w: &World, id: ActorId) -> Real {
    burst(w, id).map_or(0.0, |b| b.y)
}

/// The box's other overlaps: the actors that are no fighter, each the call of `actor_victim_hit` at its begin.
#[allow(clippy::too_many_arguments, reason = "the locals")]
fn burst_actor_hits(
    w: &mut World,
    id: ActorId,
    f: FighterId,
    cls: Name,
    cx: Real,
    cy: Real,
    hx: Real,
    hy: Real,
    g: Real,
) {
    let me = ActorRef::Actor(id);
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), cls) else { return };
    let Some(boxc) = obj(&def.components, "UpwardBurst") else { return };
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    let depth = num_in(boxc, "BoxExtent", "X").unwrap_or(0.0) * g;
    let types = crate::chars::shaggy::shockwave::overlap_types(boxc);
    let found = box_overlap_actors(w, cx, cy, hx, hy, depth, &types, &[Some(me)], crate::real(OBJ_WORLD_DYNAMIC));
    let mut now: IndexSet<(u8, i32)> = IndexSet::new();
    let packet = obj(&def.p, "HitBoxDamage").cloned().unwrap_or_default();
    let rows: Vec<Props> = obj(&def.p, "On Hit Buffs")
        .map(|o| arr(o, "OnHitBuffs").iter().filter_map(Value::as_obj).cloned().collect())
        .unwrap_or_default();
    for o in found {
        if matches!(o, ActorRef::Fighter(_)) {
            continue; // the fighters: above
        }
        let Some(key) = crate::chars::shaggy::shockwave::actor_key(w, o) else { continue };
        now.insert(key);
        let Some(b) = burst(w, id) else { return };
        if b.actor_over.contains(&key) || w.actors.actors.get(id).is_none_or(|a| a.dead) {
            continue;
        }
        let mut once = std::mem::take(&mut burst_mut(w, id).expect("found above").actor_once);
        crate::chars::shaggy::shockwave::actor_victim_hit(w, me, o, &packet, &rows, Some(&mut once));
        if let Some(b) = burst_mut(w, id) {
            b.actor_once = once;
        }
    }
    if let Some(b) = burst_mut(w, id) {
        b.actor_over = now;
    }
}

// ------------------------------------------------------------------ the entries

/// This file's registry entries (the actor entries of C017_CannonShotAim_C, C017_EnergyDiscAimer_C,
/// C017_PlasmaAimer_C and C017_CannonExplotion_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let size = Some(SizeHooks::default());
    let enter = |r: &mut Registry, name: &'static str, class: WorldActorClass, hooks: WorldActorHooks| {
        r.actors.enter(name, ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks)))
    };
    enter(
        r,
        "C017_CannonShotAim_C",
        WorldActorClass::C017CannonShotAim,
        WorldActorHooks {
            spawn: Some(cannon_spawn),
            activate: Some(fire_cannon),
            step: Some(cannon_step),
            size,
            ..WorldActorHooks::default()
        },
    )?;
    enter(
        r,
        "C017_EnergyDiscAimer_C",
        WorldActorClass::C017EnergyDiscAimer,
        WorldActorHooks {
            spawn: Some(disc_spawn),
            activate: Some(disc_activate),
            step: Some(disc_step),
            end: Some(aimer_end),
            size,
            ..WorldActorHooks::default()
        },
    )?;
    enter(
        r,
        "C017_PlasmaAimer_C",
        WorldActorClass::C017PlasmaAimer,
        WorldActorHooks {
            spawn: Some(plasma_spawn),
            activate: Some(fire_plasma),
            step: Some(plasma_step),
            end: Some(aimer_end),
            size,
            ..WorldActorHooks::default()
        },
    )?;
    enter(
        r,
        "C017_CannonExplotion_C",
        WorldActorClass::C017CannonExplotion,
        WorldActorHooks { step: Some(burst_step), hits: Some(burst_hits), size, ..WorldActorHooks::default() },
    )
}
