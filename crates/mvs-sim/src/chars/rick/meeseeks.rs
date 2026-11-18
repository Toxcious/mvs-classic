#![allow(
    clippy::doc_lazy_continuation,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::too_many_arguments,
    reason = "the doc lists wrap by the notes' own layout; negated comparisons keep their NaN behaviour; the calls' parameters"
)]
//! Meeseeks_SpawnedActor_C ([`MeeseeksSummon`], a projectile whose mesh plays
//! montages: Rick's summon) and Buff_MeeseeksDamageWithHitstun_C. MeeSeeks_AirFlap_C is air_flap.rs.
//!
//! Meeseeks_SpawnedActor (Capsule r 40 / hh 40, ProjectileNavPlatform, not simulating at spawn; Meeseeks_SkelMesh at
//! Z -38, scale 1.155; Health 10, TakesDamage; HitBoxTags [Body], HitBoxOn? false; DestroyOnHitFighter).
//! - BeginPlay @1166: ParentProjectilePhysics' (SAP's SetTeamColor -> @4644 -> @1622: PS_MeeseeksSpawn and
//!   sfx_cha_tutorial_meeseeks_cast at the actor), then @81: OnTakeAnyDamage bound to OnTakeDamage, FacingLeft = the
//!   Instigator's BPI_IsFacingLeft (the AnimBP's too), HitBoxHandlerFacingLeft = FacingLeft, the Destructible's team,
//!   the handler's Instigator; authority, the Instigator in hitstun: K2_DestroyActor @1082.
//! - ActivateObject @3210: once (Activated); the ActivationConditions loop @1917-2594 runs the first true one: 0
//!   AttackRunLogic(false), 1 AttackGolfLogic, 2 AttackUppercutLogic, 3 AttackRunLogic(true).
//! - AttackRunLogic: PlayMontageMulticast(Meeseeks_Runnig_Montage, DestroyOnEnd false), Direction = the Instigator's
//!   forward x (Backward ? -1 : 1), HitBoxOn? = true and UpdateHitbox (HitBoxesOn([Body], HitBoxDamage, (1, 1, 1))),
//!   SetRunningMulticast @7192: Running, ProjectileMovement.Deactivate, Capsule.SetSimulatePhysics(true), FacingLeft
//!   (and the AnimBP's) = Backward ? !the Instigator's : the Instigator's (not SetFacing: the handler's facing stays),
//!   Delay(RunTime 3) -> K2_DestroyActor @66.
//! - AttackGolfLogic: PlayMontageMulticast(Meeseeks_Attack_GolfSwing_Montage, true), IsGolfing, GolfClubVis @9234
//!   (golfClub visible, the handler's IsReflectingProjectiles). AttackUppercutLogic:
//!   PlayMontageMulticast(Meeseeks_Attack_ChargeUppercut_Montage, true). Their hits are the montages' Hitframes on
//!   the actor's own handler (`registry::actor_notifies` actor_hitframe: RecentAttackID 0).
//! - PlayMontageMulticast @6771: CreateProxyObjectForPlayMontage(Meeseeks_SkelMesh, montage, 1, 0, None); OnCompleted
//!   @5046, OnBlendOut @5098, OnInterrupted @7927 -> @5073: DestroyOnEnd (the last call's) -> K2_DestroyActor.
//! - ReceiveTick @5200 (authority, Running): the wall trace and the gate @4400, the body's velocity @6089-6507, the
//!   rotation by FacingLeft.
//! - OverlapCheck / Box overlaps @3297-4381, @8861-9167: the golf swing's triggers (JumpSectionMulticast(Swing) @8630).
//! - OnTakeDamage @9302: another Meeseeks_SpawnedActor destroys both (@9421, @9457); a causer with BPI_Projectiles and
//!   BPI_SpawnedActor: attack type 2 (Light) -> HitShake @9201, else ApplyDamage(self, Damage x 4, null, self) @9817;
//!   any other causer: HitShake.
//! - Reflected @7912 -> @5130: authority K2_DestroyActor (then, Running, the gate). Unreachable in the build: its
//!   CanBeReflected is the class default false and no node writes it (`combat::reflect` refuses it); ported as read.
//! - ReceiveDestroyed @1181: (authority, !Activated: an unbind of a delegate never bound)
//!   sfx_cha_tutorial_meeseeks_despawn at (0, 0, 0).
//! Not ported, no caller in the build's ported set: ScaleUp @3251 (no caller at all), BPI_SetFacing (C018 / Garnet's
//! callers: [`set_facing`] is the graph's SetFacing for them).
//! Die With Instigator and DestroyOnExitedArena?: the world's (respawn pass, the blast-box timer).
//!
//! The hit box components move with the bones and sockets they are attached to as the montage plays (the tracks of
//! the actor's box data), the montage's pose read as Meeseeks_AnimBP has it: BlendListByBool_1 (IsFalling:
//! GetVelocity Z < -200 @1180) takes the AirSlot's track, else the DefaultSlot's (both carry each montage; only
//! Meeseeks_Runnig_Montage plays another sequence on AirSlot, Meeseeks_Attack_FlailingFall).
//! Deviation: the node's .1 s linear blend between the two slots is not made: the pose takes the slot at once;
//! IsFalling is read from the velocity at hit time, not at the AnimBP update before physics. The montage's blend-in /
//! out weight against the state machine's pose is not made either.
//! The montages' Hitframe tags:
//! - Meeseeks_Runnig_Montage: none (HitBoxTags [Body]: the Body sphere on Pelvis).
//! - Meeseeks_Attack_GolfSwing_Montage (the Hitframe at 2.41 s): Head, Body and the GolfHitBox capsule (golfClub1); its
//!   tags LeftHand / RightHand name no component (the hands' tags are "Left Hand" / "Right Hand"): those two boxes do
//!   not exist in the build either.
//! - Meeseeks_Attack_ChargeUppercut_Montage (the Hitframe at .33 s): the hands, the feet and the head; its tag
//!   HitCollision names no component (HitCollision is tagged Hit01).

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{
    ActorAnim, MontageEvent, anim_of, montage_current_section, montage_jump_to_section, montage_play,
};
use crate::actors::projectile::{
    ProjectileExt, class_def, class_p, data_fighter, hit_xy, hurt_x, hurt_y, receive_damage,
};
use crate::actors::world_actor::{actor_cls, actor_owner, actor_p};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::boxes::{BoxSampler, WorldSphere, actor_boxes, push_actor_shapes};
use crate::combat::buff_class::BuffClass;
use crate::combat::{BuffRef, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{
    actor_is_a, instigator_of, latent_frames, owner_team, sound_at_for, warn_once, warn_parts, warn_text,
};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_WORLD_DYNAMIC, TRACE_PROJECTILE_GROUND, TraceBody,
    body_info, box_overlap_actors, sphere_overlap_actors, sphere_trace_single,
};
use crate::fighter::{FighterId, facing, in_hitstun};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::sphere_settings;
use crate::registry::entries::{
    ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, MeshHooks, ProjectileHooks, ReflectHooks,
};
use crate::registry::props::{num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::is_match_fighter;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::destroy_projectile;
use crate::world::stage_query::{StageShapes, rigid_statics};
use mvs_anim::montage::same_name;
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, DEFAULT_K, Solver};
use mvs_data::fighter::Montage;
use mvs_data::value::Value;

/// The summon's class.
pub const MEESEEKS_SUMMON: Name = name!("Meeseeks_SpawnedActor_C");
const MESH: Name = name!("Meeseeks_SkelMesh");
const RUN_MONTAGE: Name = name!("Meeseeks_Runnig_Montage");
const GOLF_MONTAGE: Name = name!("Meeseeks_Attack_GolfSwing_Montage");
const UPPER_MONTAGE: Name = name!("Meeseeks_Attack_ChargeUppercut_Montage");

/// The modes of the ActivationConditions loop (the index of the first true condition): -1 not activated.
const MODE_RUN: i32 = 0;
const MODE_GOLF: i32 = 1;
const MODE_UPPERCUT: i32 = 2;
const MODE_RUN_BACK: i32 = 3;

/// Meeseeks_AnimBP @1180.
const FALL_VZ: Real = -200.0;

/// `MeeseeksSummon extends Projectile` (Meeseeks_SpawnedActor_C).
#[derive(State, Debug)]
pub struct MeeseeksSummon {
    /// Its Meeseeks_SkelMesh's montages (`anim`, `actors::actor_montage::anim_of`).
    pub anim: ActorAnim,
    /// The ActivationConditions index ActivateObject ran (`MODE_*`), -1 before.
    pub mode: i32,
    /// ActivationConditions, a bit per index (BPI_SetActivationConditions).
    pub conds: i32,
    /// FacingLeft (the actor's rotation and the AnimBP's IsFacingLeft).
    pub facing_left: bool,
    /// Comp_Pawn_HitBoxHandler.HitBoxHandlerFacingLeft.
    pub handler_left: bool,
    /// Direction.Y's side in the sim's x: +1 toward +x (UE Direction.Y < 0), 0 before the run.
    pub dir: Real,
    /// Direction is the zero vector (AttackRunLogic with no Instigator): the tick's select takes -1 and SetFacing's
    /// +-|Y| keeps it zero, so the run never turns.
    pub dir_zero: bool,
    /// Running.
    pub running: bool,
    /// IsGolfing (golfClub visible: GolfClubVis).
    pub golfing: bool,
    /// PlayMontageMulticast's DestroyOnEnd, the last call's.
    pub destroy_on_end: bool,
    /// The Capsule body's linear velocity while it simulates (sim plane).
    pub bvx: Real,
    pub bvy: Real,
    pub bspin: Real,
    /// The gate @4400 closed.
    pub turn_closed: bool,
    /// The age its Delay(.5) fires at.
    pub turn_at: i32,
    /// The age the run's Delay(RunTime) fires at.
    pub end_at: i32,
    /// The Hitframe's HitBoxesOn on the actor's handler (one notify per montage): on, its index, tags and scale.
    pub frame_on: bool,
    pub frame_idx: i32,
    pub frame_tags: Vec<Name>,
    pub frame_scale: Real,
    /// The age HitShake.PlayFromStart ran (the view's timeline: the mesh's X offset), -1 never.
    pub shake_at: i32,
    /// ReceiveAnyDamage's Damage, taken at the hit's HitByEnemy ([`any_damage`] / [`hit_by`]).
    pub pending_dmg: Real,
    /// (view only) OnTakeAnyDamage's broadcasts, which Meeseeks_AnimBP's OnTakeDamage takes: the age of the latest
    /// and their count.
    pub any_damage_at: i32,
    pub any_damage_n: i32,
    /// GetInstigator(): the spawning fighter's id.
    pub inst: i32,
    /// The enemy fighters OverlapCheck overlaps (ids).
    pub over_f: Vec<i32>,
    /// The spawned actors OverlapCheck overlaps (ids).
    pub over_a: Vec<i32>,
    /// The spawned actors Box overlaps (ids).
    pub box_a: Vec<i32>,
    /// The fighters whose DamageManager.KnockedBack is bound to CheckGolfSwing (ids).
    pub bind_f: Vec<i32>,
    /// Their knockedBackCount at the bind (or at the last swing the bind gave).
    pub bind_kb: Vec<i32>,
}

impl Default for MeeseeksSummon {
    fn default() -> Self {
        MeeseeksSummon {
            anim: ActorAnim::new(MESH),
            mode: -1,
            conds: 0,
            facing_left: false,
            handler_left: false,
            dir: 0.0,
            dir_zero: false,
            running: false,
            golfing: false,
            destroy_on_end: false,
            bvx: 0.0,
            bvy: 0.0,
            bspin: 0.0,
            turn_closed: false,
            turn_at: -1,
            end_at: -1,
            frame_on: false,
            frame_idx: -1,
            frame_tags: Vec::new(),
            frame_scale: 1.0,
            shake_at: -1,
            pending_dmg: 0.0,
            any_damage_at: -1,
            any_damage_n: 0,
            inst: -1,
            over_f: Vec::new(),
            over_a: Vec::new(),
            box_a: Vec::new(),
            bind_f: Vec::new(),
            bind_kb: Vec::new(),
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn of(p: &Projectile) -> &MeeseeksSummon {
    match &p.ext {
        ProjectileExt::MeeseeksSummon(s) => s,
        e => panic!("Meeseeks_SpawnedActor_C without its state: {e:?}"),
    }
}

fn of_mut(p: &mut Projectile) -> &mut MeeseeksSummon {
    match &mut p.ext {
        ProjectileExt::MeeseeksSummon(s) => s,
        e => panic!("Meeseeks_SpawnedActor_C without its state: {e:?}"),
    }
}

fn is_summon(w: &World, a: ActorRef) -> bool {
    matches!(a, ActorRef::Projectile(id) if w.actors.projectiles.get(id).is_some_and(|p| matches!(p.ext, ProjectileExt::MeeseeksSummon(_))))
}

fn dead(w: &World, id: ProjectileId) -> bool {
    w.actors.projectiles.get(id).is_none_or(|p| p.dead)
}

fn instigator(w: &World, id: ProjectileId) -> Option<FighterId> {
    instigator_of(w, of(pr(w, id)).inst)
}

// ---------------------------------------------------------------- the class's data

/// The class's montage `name` (`montageOf`: the owner's character data, else a peer's, as `actorDef`'s rule).
fn montage_def<'a>(w: &'a World, owner: FighterId, name: &str) -> Option<&'a Montage> {
    let of_fighter = |f: FighterId| {
        let ch = w.fighter(f).ch;
        crate::fighter::common::char_data(w, ch).fighter.actor_montages.as_ref()?.get(name)
    };
    of_fighter(owner).or_else(|| crate::world::pawns::match_fighters_iter(w).find_map(of_fighter))
}

/// The Capsule's radius and half height, and the mesh's scale (the class's static data).
fn capsule(w: &World, p: &Projectile) -> (Real, Real) {
    let c = class_def(w, p).and_then(|d| obj(&d.components, "Capsule"));
    let g = |k: &str| c.and_then(|c| num(c, k)).unwrap_or(0.0);
    (g("CapsuleRadius"), g("CapsuleHalfHeight"))
}

fn mesh_scale(w: &World, p: &Projectile) -> Real {
    class_def(w, p)
        .and_then(|d| obj(&d.components, MESH_KEY))
        .and_then(|c| obj(c, "RelativeScale3D"))
        .and_then(|s| num(s, "X"))
        .unwrap_or(1.0)
}

const MESH_KEY: &str = "Meeseeks_SkelMesh";

/// The constructor's tail: the Body sphere's (HitBoxTags) radius is the hit radius the other actors' damage boxes are
/// met with (`hitNear`).
fn construct(w: &World, p: &mut Projectile) {
    let (r, hh) = capsule(w, p);
    if r != hh {
        warn_text(w, "Meeseeks_SpawnedActor_C: the Capsule is not a sphere: stepped as one");
    }
    let body =
        class_def(w, p).and_then(|d| obj(&d.components, "Body")).and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    p.hit_radius = body * mesh_scale(w, p) * p.scale;
}

/// `make`: FacingLeft is the Instigator's BPI_IsFacingLeft @222-454; ActivationConditions of the class data.
fn make(w: &World, f: FighterId, r: &SpawnRequest) -> ProjectileExt {
    let left = facing(w, f) < 0.0;
    let mut s =
        MeeseeksSummon { facing_left: left, handler_left: left, inst: w.fighter(f).id, ..MeeseeksSummon::default() };
    let conds = crate::fighter::common::actor_class_def(w, f, r.cls).and_then(|d| d.p.get("ActivationConditions"));
    if let Some(Value::Arr(a)) = conds {
        for (i, v) in a.iter().enumerate() {
            if matches!(v, Value::Bool(true)) && i < 31 {
                s.conds |= 1 << i;
            }
        }
    }
    ProjectileExt::MeeseeksSummon(s)
}

// ---------------------------------------------------------------- the boxes

/// `hitboxActive` of Projectile itself (HitBoxOn? and a live collision).
fn base_hitbox_active(p: &Projectile) -> bool {
    p.hit_on && !p.dead && !p.no_collision
}

/// `hitboxActive`: HitBoxOn? (the class's [Body] box) or a Hitframe's boxes.
pub fn hitbox_active(p: &Projectile) -> bool {
    base_hitbox_active(p) || (of(p).frame_on && !p.dead && !p.no_collision)
}

/// The box tracks at the montage's position (the slot of Meeseeks_AnimBP's IsFalling); `None` with no montage.
fn pose(w: &World, p: &Projectile, t: &crate::combat::boxes::BoxTables, bs: &BoxSampler) -> Option<Vec<f32>> {
    let s = of(p);
    let Some(m) = s.anim.m.as_ref() else {
        warn_text(w, "Meeseeks_SpawnedActor_C: hit boxes on with no montage: none");
        return None;
    };
    let name = w.names.str(m.montage.name);
    let def = montage_def(w, data_fighter(p), name)?;
    let want = if (if s.running { s.bvy } else { p.vy }) < FALL_VZ { "AirSlot" } else { "DefaultSlot" };
    // A montage with no track on that slot: the build's Slot node passes its source pose through (no montage pose).
    let Some(segs) = def.slots.iter().find(|sl| sl.slot == want).map(|sl| &sl.segments) else {
        warn_parts(w, &["Meeseeks_SpawnedActor_C: ", name, " has no ", want, " track: no hit boxes"]);
        return None;
    };
    let seg = m.seg_at(segs).map(|i| &segs[i]);
    let mut out = vec![0.0_f32; usize::try_from(t.stride).unwrap_or(0)];
    let ok = seg.is_some_and(|sg| BoxSampler::sample(t, &sg.anim, crate::math::narrow(m.seg_time(sg)), &mut out));
    if seg.is_some() && !ok {
        warn_parts(w, &["Meeseeks_SpawnedActor_C: no track of ", seg.map_or("", |sg| sg.anim.as_str())]);
    }
    let _ = bs;
    ok.then_some(out)
}

/// `hitShapes`: the boxes on now: the class's HitBoxTags at HitBoxScale, the Hitframe's tags at its scale
/// (GetComponentsByTag on the mesh's components: an FName, without case), where the montage's pose puts them now. A
/// tag with no component deals nothing.
pub fn hit_shapes(w: &World, p: &Projectile, out: &mut Vec<WorldSphere>) {
    let s = of(p);
    let on = base_hitbox_active(p);
    if !(on || s.frame_on) {
        return;
    }
    let Some((bs, t)) = actor_boxes(w, data_fighter(p), p.cls) else { return };
    let Some(pose) = pose(w, p, &t, bs) else { return };
    // Facing left: Meeseeks_AnimBP's AnimGraph runs the pose through WAnimationMirrorGraphNode (bEnabled <-
    // FacingLeft, Meeseeks_MirrorTable AXIS_Y, L/R bones swapped) under the actor's yaw +-90 (@2822 / @6566 /
    // @8521): mirror and turn are a reflection, the depth kept; AdjustTagForFacing's L/R tag swap maps each tag to the
    // reflection of its right-facing place.
    // Unverified: the mirror node's effect on socket offsets is native and not read.
    let fac = if s.facing_left { -1.0 } else { 1.0 };
    if on {
        let scale = obj(class_p(w, p), "HitBoxScale").and_then(|v| num(v, "X")).unwrap_or(1.0);
        if let Some(Value::Arr(tags)) = class_p(w, p).get("HitBoxTags") {
            for tag in tags.iter().filter_map(Value::as_str) {
                push_actor_shapes(out, &t, bs, &pose, bs.tagged(tag), p.x, p.y, fac, true, scale, p.scale);
            }
        }
    }
    if s.frame_on {
        for &tag in &s.frame_tags {
            push_actor_shapes(
                out,
                &t,
                bs,
                &pose,
                bs.tagged(w.names.str(tag)),
                p.x,
                p.y,
                fac,
                true,
                s.frame_scale,
                p.scale,
            );
        }
    }
}

/// `hitNear`: the point that stands for the boxes against another actor's sphere or box at (tx, ty) with the radius
/// `hit_radius`: on the line to the box nearest it (its cut through the play plane), at `hit_radius` past that box's
/// reach, so that "within hit_radius + r" holds when one of the boxes is within r.
// Deviation: the other actor's box is met as a circle in the play plane.
pub fn hit_near(w: &World, p: &Projectile, tx: Real, ty: Real) -> V2 {
    let mut shapes = Vec::new();
    hit_shapes(w, p, &mut shapes);
    let (mut best, mut bx, mut by) = (Real::INFINITY, p.x, p.y);
    for s in &shapes {
        let z = s.z.unwrap_or(0.0);
        let r = s.r * s.r - z * z;
        if r <= 0.0 {
            continue;
        }
        let d = ((s.x - tx) * (s.x - tx) + (s.y - ty) * (s.y - ty)).sqrt() - r.sqrt();
        if d < best {
            best = d;
            bx = s.x;
            by = s.y;
        }
    }
    if best == Real::INFINITY {
        let (x, y) = hit_xy(w, p);
        return V2 { x, y };
    }
    let (dx, dy) = (bx - tx, by - ty);
    let l = (dx * dx + dy * dy).sqrt();
    let k = max(0.0, best + p.hit_radius);
    if l > 0.0 { V2 { x: tx + (dx / l) * k, y: ty + (dy / l) * k } } else { V2 { x: tx, y: ty } }
}

/// `yawX`: pfgDoDamage's ComingFromLeft for a hit without VelocityInsteadOfFacing: HitBoxHandlerFacingLeft.
pub fn yaw_x(p: &Projectile) -> Real {
    if of(p).handler_left { -1.0 } else { 1.0 }
}

/// `handlerFacingLeft`: KnockBackMirror's side (pfgDoDamage @11892): Meeseeks_Base @139 / @1207, Meeseeks_SpawnedActor
/// @454 / @2781.
pub fn handler_facing_left(p: &Projectile) -> bool {
    of(p).handler_left
}

/// `hitVelocity`: GetVelocity, the simulating body's while running.
pub fn hit_velocity(p: &Projectile) -> Option<(Real, Real)> {
    match &p.ext {
        ProjectileExt::MeeseeksSummon(s) if s.running => Some((s.bvx, s.bvy)),
        _ => None,
    }
}

/// HurtBox (r 100, 169.17 above the root; ProjectileDestructible): the hits of fighters and spawned actors meet it
/// through pfgDoDamage's DamageAndLaunch (Comp_Actor_Destructible IsProjectile false).
// Deviation: HurtBoxCapsule (r 60, hh 150 at the same place, tag HurtBox) is not a damage box here (the traces have
// it: [`bodies`]); it lies inside the sphere but for its ends (150 above and below the centre, 60 wide).
fn damage_box(w: &World, id: ProjectileId) -> Option<WorldSphere> {
    let p = pr(w, id);
    (!p.dead).then(|| WorldSphere {
        x: hurt_x(p),
        y: hurt_y(p),
        z: Some(0.0),
        r: p.hurt_radius,
        ..WorldSphere::default()
    })
}

/// HurtBoxCapsule (HurtBox profile, ECC_GameTraceChannel17; r 60, half height 150 at the HurtBox's place, both under
/// Capsule): no graph touches it, so it stays on. It answers Portal and HitBox with Overlap, so a portal's
/// PortalOverlap2 (which overlaps HurtBox) meets a Meeseeks through it. Upright, scaled as the HurtBox sphere is
/// (hurt_radius / its SphereRadius 100).
fn bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    if p.dead || !(p.hurt_radius > 0.0) {
        return;
    }
    let Some(d) = class_def(w, p) else { return };
    let c = obj(&d.components, "HurtBoxCapsule");
    let Some(info) = body_info(c, None) else { return };
    let Some(c) = c else { return };
    if !info.on {
        return;
    }
    let k = p.hurt_radius / obj(&d.components, "HurtBox").and_then(|h| num(h, "SphereRadius")).unwrap_or(100.0);
    let g = |n: &str| num(c, n).unwrap_or(0.0);
    let (r, hh) = (g("CapsuleRadius") * k, g("CapsuleHalfHeight") * k);
    let half = max(hh - r, 0.0);
    let (x, y) = (hurt_x(p), hurt_y(p));
    out.push(TraceBody {
        ax: x,
        ay: y - half,
        bx: x,
        by: y + half,
        hx: 0.0,
        hy: 0.0,
        r,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("HurtBoxCapsule"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

// ---------------------------------------------------------------- the actions

/// SetFacing @2595 (BPI_SetFacing's): FacingLeft and the AnimBP's, the handler's facing, the rotation; Running:
/// Direction.Y = left ? |Y| : -|Y| (@2947-3182; a zero Direction stays zero).
pub fn set_facing(w: &mut World, id: ProjectileId, left: bool) {
    let p = pm(w, id);
    p.facing = if left { -1.0 } else { 1.0 };
    let s = of_mut(p);
    s.facing_left = left;
    s.handler_left = left;
    if s.running && !s.dir_zero {
        s.dir = if left { -1.0 } else { 1.0 };
    }
}

/// BPI_ReverseMoveDirection @0 -> ubergraph @1113-1142: SetFacing(!FacingLeft); no parent call (`combat::reverse`).
pub fn reverse_move(w: &mut World, id: ProjectileId) {
    let left = of(pr(w, id)).facing_left;
    set_facing(w, id, !left);
}

/// BPI_SetActivationConditions (SpawnedActorParent @4520 Array_Set).
pub fn set_condition(w: &mut World, id: ProjectileId, n: i32, value: bool) {
    if !(0..=30).contains(&n) {
        warn_text(w, &format!("Meeseeks_SpawnedActor_C: activation condition {n} out of range"));
        return;
    }
    let s = of_mut(pm(w, id));
    if value {
        s.conds |= 1 << n;
    } else {
        s.conds &= !(1 << n);
    }
}

/// The gate @4400 (the tick's wall hit, Reflected): closed, SetFacing(!FacingLeft), Delay(.5) -> reopen. `in_tick`: from
/// ReceiveTick, ahead of that tick's latent actions, which count this frame.
fn gated_turn(w: &mut World, id: ProjectileId, in_tick: bool) {
    if of(pr(w, id)).turn_closed {
        return;
    }
    let left = of(pr(w, id)).facing_left;
    of_mut(pm(w, id)).turn_closed = true;
    set_facing(w, id, !left);
    let p = pm(w, id);
    let at = p.age + to_int32(latent_frames(0.5)) - i32::from(in_tick);
    of_mut(p).turn_at = at;
}

/// PlayMontageMulticast @6771.
fn play_montage(w: &mut World, id: ProjectileId, name: Name, destroy_on_end: bool) {
    of_mut(pm(w, id)).destroy_on_end = destroy_on_end;
    montage_play(w, ActorRef::Projectile(id), name, 1.0, 0.0, true);
}

/// JumpSectionMulticast(Swing) @8630: no jump while the current section is Swing; Montage_JumpToSection on the montage
/// playing (a montage without the section: nothing).
fn jump_swing(w: &mut World, id: ProjectileId) {
    let a = ActorRef::Projectile(id);
    if same_name(w.names.str(montage_current_section(w, a)), "Swing") {
        return;
    }
    let Some(m) = anim_of(w, a).and_then(|an| an.m.as_ref()) else { return };
    let Some(def) = montage_def(w, data_fighter(pr(w, id)), w.names.str(m.montage.name)) else { return };
    if !def.sections.iter().any(|x| same_name(&x.name, "Swing")) {
        return;
    }
    montage_jump_to_section(w, a, name!("Swing"));
}

/// AttackRunLogic(Backward), from ActivateObject (the owner's ActivateActors, ahead of the actor's tick this frame: the
/// Delay counts it).
fn run_logic(w: &mut World, id: ProjectileId, backward: bool) {
    let f = instigator(w, id);
    play_montage(w, id, RUN_MONTAGE, false);
    let run_time = match num(class_p(w, pr(w, id)), "RunTime") {
        Some(t) => t,
        None => {
            warn_once(w, name!("Meeseeks_SpawnedActor_C data missing: RunTime"));
            0.0
        }
    };
    let fl = f.map(|f| facing(w, f));
    let p = pm(w, id);
    let s = of_mut(p);
    // @44 GetActorForwardVector on None: the zero vector, whose Y is not below 0 at the tick's select (-1 here).
    s.dir = fl.map_or(-1.0, |fa| fa * if backward { -1.0 } else { 1.0 });
    s.dir_zero = fl.is_none();
    crate::actors::projectile::set_hitbox(p, true);
    // SetRunningMulticast @7192 (its velocity argument is not read).
    let s = of_mut(p);
    s.running = true;
    p.stopped = true; // ProjectileMovement.Deactivate
    // Unverified: SetSimulatePhysics(true) is taken to start the body at rest (the deactivated movement's velocity is
    // not handed to the body); the tick sets its horizontal velocity before the first step.
    let s = of_mut(p);
    s.bvx = 0.0;
    s.bvy = 0.0;
    // @7728: the failed cast's IsFacingLeft is false (@7850).
    let inst_left = fl.is_some_and(|fa| fa < 0.0);
    let left = if backward { !inst_left } else { inst_left };
    of_mut(p).facing_left = left;
    p.facing = if left { -1.0 } else { 1.0 };
    let at = p.age + to_int32(latent_frames(run_time));
    of_mut(p).end_at = at;
}

/// ActivateObject: the loop over ActivationConditions (the class default has four entries).
fn activate(w: &mut World, id: ProjectileId) {
    if pr(w, id).activated {
        return;
    }
    pm(w, id).activated = true;
    let n = match class_p(w, pr(w, id)).get("ActivationConditions") {
        Some(Value::Arr(a)) => a.len(),
        _ => 4,
    };
    for i in 0..n {
        if of(pr(w, id)).conds & (1 << i) == 0 {
            continue;
        }
        if i > 3 {
            return; // the switch has no case: the loop breaks with nothing run
        }
        let i = i32::try_from(i).unwrap_or(i32::MAX);
        of_mut(pm(w, id)).mode = i;
        match i {
            MODE_RUN => run_logic(w, id, false),
            MODE_GOLF => {
                play_montage(w, id, GOLF_MONTAGE, true);
                of_mut(pm(w, id)).golfing = true; // IsGolfing, GolfClubVis @9234
            }
            MODE_UPPERCUT => play_montage(w, id, UPPER_MONTAGE, true),
            MODE_RUN_BACK => run_logic(w, id, true),
            _ => {}
        }
        return;
    }
}

/// Meeseeks_SpawnedActor SetTeamColor (@4644 -> @1622): the mesh's team material (the view's), PS_MeeseeksSpawn and
/// sfx_cha_tutorial_meeseeks_cast at the actor.
fn spawn_visuals(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let (cls, x, y, fid) = (p.cls, p.x, p.y, p.owner.map_or(p.owner_id, |o| w.fighter(o).id));
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_MeeseeksSpawn")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    let inst = instigator(w, id);
    sound_at_for(w, inst, name!("sfx_cha_tutorial_meeseeks_cast"), x, y);
}

// ---------------------------------------------------------------- the overlaps

/// EX_AddMulticastDelegate: AddUnique, one binding per fighter.
fn bind(s: &mut MeeseeksSummon, f: i32, kb: i32) {
    if s.bind_f.contains(&f) {
        return;
    }
    s.bind_f.push(f);
    s.bind_kb.push(kb);
}

fn unbind(s: &mut MeeseeksSummon, f: i32) {
    if let Some(i) = s.bind_f.iter().position(|&x| x == f) {
        s.bind_f.remove(i);
        s.bind_kb.remove(i);
    }
}

/// The OverlapCheck sphere (r 220, 200.6 above the root) and the Box under it (extent 250 x 250 x 50, 220 below the
/// sphere), their begin and end events against the bodies their Custom profiles overlap (Pawn, PhysicsBody,
/// DamageDestructible of the gathered types). Both components are WorldDynamic (Custom, no ObjectType), and a spawned
/// actor's body that answers WorldDynamic with Ignore is no overlap (`traces` actors_of's responder).
// Deviation: tested once a tick in the actor's tick against the bodies where they are; a fighter's bodies' response is
// not read (Pawn and HurtBox overlap WorldDynamic).
fn overlaps(w: &mut World, id: ProjectileId) {
    let me = ActorRef::Projectile(id);
    let (x, y, k) = {
        let p = pr(w, id);
        (p.x, p.y, p.scale)
    };
    let Some(owner) = pr(w, id).owner else { return };
    let team = w.fighter(owner).team.team;
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_DAMAGE_DESTRUCTIBLE)];
    let responder = crate::real(OBJ_WORLD_DYNAMIC);
    let cy = y + 200.61841 * k;
    let (mut f_now, mut a_now, mut b_now) = (Vec::new(), Vec::new(), Vec::new());
    for o in sphere_overlap_actors(w, x, cy, 220.0 * k, &types, &[Some(me)], responder) {
        match o {
            ActorRef::Fighter(fi) if is_match_fighter(w, fi) => {
                if w.fighter(fi).team.team != team {
                    f_now.push(fi);
                }
            }
            ActorRef::Projectile(oi) if !dead(w, oi) && owner_team(w, o) != team => a_now.push(pr(w, oi).seq),
            _ => {}
        }
    }
    let (over_f, over_a, box_a) = {
        let s = of(pr(w, id));
        (s.over_f.clone(), s.over_a.clone(), s.box_a.clone())
    };
    // OverlapCheck end @8861: an enemy fighter's bind removed @9103.
    let f_ids: Vec<i32> = f_now.iter().map(|&fi| w.fighter(fi).id).collect();
    for &fi in &over_f {
        if !f_ids.contains(&fi) {
            unbind(of_mut(pm(w, id)), fi);
        }
    }
    // OverlapCheck begin @3576.
    let golfing = of(pr(w, id)).golfing;
    for (&fi, &fid) in f_now.iter().zip(&f_ids) {
        if over_f.contains(&fid) {
            continue;
        }
        if golfing && in_hitstun(w, fi) {
            jump_swing(w, id); // @3954
        } else {
            let kb = w.fighter(fi).attackers.knocked_back_count;
            bind(of_mut(pm(w, id)), fid, kb); // @4033 (golfing, not in hitstun) / @4190 -> @3982 (not golfing)
        }
    }
    if golfing {
        for a in &a_now {
            if !over_a.contains(a) {
                jump_swing(w, id); // @4162
            }
        }
    }
    {
        let s = of_mut(pm(w, id));
        s.over_f = f_ids;
        s.over_a = a_now;
    }
    // Box begin @3297: golfing, an enemy spawned actor -> Swing @15.
    for o in box_overlap_actors(w, x, cy - 220.0 * k, 250.0 * k, 50.0 * k, 250.0 * k, &types, &[Some(me)], responder) {
        if let ActorRef::Projectile(oi) = o
            && !dead(w, oi)
            && owner_team(w, o) != team
        {
            b_now.push(pr(w, oi).seq);
        }
    }
    if golfing {
        for a in &b_now {
            if !box_a.contains(a) {
                jump_swing(w, id);
            }
        }
    }
    of_mut(pm(w, id)).box_a = b_now;
}

/// CheckGolfSwing @4033 -> @9196 -> JumpSectionMulticast(Swing), on a bound fighter's KnockedBack.
// Deviation: the broadcast is polled at the actor's tick (the fighter's knocked-back count, as `items::meeseeks`), not
// inside the hit.
fn poll_binds(w: &mut World, id: ProjectileId) {
    let mut i = 0;
    while i < of(pr(w, id)).bind_f.len() {
        let fid = of(pr(w, id)).bind_f[i];
        // A bound pawn that is gone (a mode's pawn despawned): its destroy ends the overlap, whose end @8861 unbinds
        // it @9103.
        let Some(fi) = w.by_id(fid) else {
            unbind(of_mut(pm(w, id)), fid);
            continue;
        };
        let k = w.fighter(fi).attackers.knocked_back_count;
        let s = of_mut(pm(w, id));
        if k != s.bind_kb[i] {
            s.bind_kb[i] = k;
            jump_swing(w, id);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------- the tick

/// ReceiveTick @5200 (authority, Running), the KnockedBack poll and the overlaps, then the latent actions: true is
/// K2_DestroyActor @66.
fn step(w: &mut World, id: ProjectileId) -> bool {
    if of(pr(w, id)).running {
        let (x, y, left, max_speed) = {
            let p = pr(w, id);
            (p.x, p.y, of(p).facing_left, p.max_speed)
        };
        let x0 = x + if left { -50.0 } else { 50.0 };
        let x1 = x0 + if left { -75.0 } else { 75.0 };
        let h =
            sphere_trace_single(w, x0, y, x1, y, 50.0, TRACE_PROJECTILE_GROUND, &[], Some(ActorRef::Projectile(id)));
        if let Some(h) = h
            && h.blocking
            && (h.nx.abs() > 1e-4 || h.ny.abs() > 1e-4)
            && h.nx.abs() > h.ny.abs()
        {
            gated_turn(w, id, true);
        }
        // @6089-6507: (V.X, Direction.Y < 0 ? MaxSpeed : -MaxSpeed, min(V.Z, 10)); @6629 the rotation by FacingLeft.
        let p = pm(w, id);
        let left = of(p).facing_left;
        let s = of_mut(p);
        s.bvx = s.dir * max_speed;
        s.bvy = min(s.bvy, 10.0);
        p.facing = if left { -1.0 } else { 1.0 };
    }
    // The KnockedBack poll ahead of the overlaps: CheckGolfSwing runs inside the hit in the build, while the knocked back
    // fighter is still bound (the overlap's end would unbind him first).
    poll_binds(w, id);
    overlaps(w, id);
    // The latent actions.
    let p = pm(w, id);
    let age = p.age;
    let s = of_mut(p);
    if age == s.turn_at {
        s.turn_closed = false; // @43
    }
    age == s.end_at // @66 K2_DestroyActor
}

/// The body's step while it simulates (SetSimulatePhysics(true) in the run), after the actor's and the mesh's ticks.
fn physics(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    if !of(p).running || p.mobility.is_some() {
        return;
    }
    let bi = class_def(w, p).and_then(|d| obj(&d.components, "Capsule")).and_then(|c| obj(c, "BodyInstance"));
    let Some(settings) = sphere_settings(w, bi, "Meeseeks_SpawnedActor_C") else { return };
    let (radius, _) = capsule(w, p);
    let (no_collision, scale) = (p.no_collision, p.scale);
    // ProjectileNavPlatform: WorldStatic and the platforms (World.blockers('nav')).
    let list = if no_collision { StageShapes::default() } else { w.blocker_shapes(BlockKind::Nav) };
    let ground =
        if no_collision { Ground { left: 0.0, right: 0.0, top: -1e9, bottom: -1e9 - 1.0 } } else { w.stage.ground };
    let statics = rigid_statics(w, &list);
    let floor = sphere_static(w, ground);
    let p = pm(w, id);
    let (px, py) = (p.x, p.y);
    let s = of_mut(p);
    let mut b = Body {
        x: f64::from(px),
        y: f64::from(py),
        vx: f64::from(s.bvx),
        vy: f64::from(s.bvy),
        spin: f64::from(s.bspin),
        radius: f64::from(radius * scale),
        ..Body::default()
    };
    let plats = if no_collision { None } else { Some(statics.as_slice()) };
    Solver::default().sphere_step(&mut b, &floor, plats, DELTA_SECONDS, &settings, DEFAULT_K);
    (p.x, p.y) = (crate::math::narrow(b.x), crate::math::narrow(b.y));
    let s = of_mut(p);
    (s.bvx, s.bvy, s.bspin) = (crate::math::narrow(b.vx), crate::math::narrow(b.vy), crate::math::narrow(b.spin));
}

/// BeginPlay: the visuals, then @1082: authority, the Instigator in hitstun -> K2_DestroyActor (@946: no
/// HitstunHandler on None, no test).
fn begin(w: &mut World, id: ProjectileId) {
    spawn_visuals(w, id);
    if instigator(w, id).is_some_and(|f| in_hitstun(w, f)) {
        destroy_projectile(w, id, false);
    }
}

/// ReceiveDestroyed @1181.
fn destroyed(w: &mut World, id: ProjectileId) {
    let inst = instigator(w, id);
    sound_at_for(w, inst, name!("sfx_cha_tutorial_meeseeks_despawn"), 0.0, 0.0);
}

// ---------------------------------------------------------------- the damage

/// ParentProjectilePhysics' ReceiveAnyDamage needs the causer's attack type: taken at [`hit_by`], which follows at
/// once with the same causer (`spawned_hits`: nothing runs between the two).
fn any_damage(w: &mut World, id: ProjectileId, dmg: Real, _by: ActorRef) -> bool {
    of_mut(pm(w, id)).pending_dmg = dmg;
    false
}

/// OnTakeAnyDamage broadcast (the actor alive after ReceiveAnyDamage): the AnimBP's handler (view only).
fn any_damage_broadcast(w: &mut World, id: ProjectileId) {
    let p = pm(w, id);
    let age = p.age;
    let s = of_mut(p);
    s.any_damage_at = age;
    s.any_damage_n += 1;
}

/// OnTakeDamage @9302 (OnTakeAnyDamage, broadcast after ReceiveAnyDamage when that left the actor alive).
fn on_take_damage(w: &mut World, id: ProjectileId, dmg: Real, by: Option<ActorRef>) {
    if by == Some(ActorRef::Projectile(id)) {
        return; // @9332
    }
    if let Some(b @ ActorRef::Projectile(o)) = by
        && is_summon(w, b)
    {
        // @9421 the causer, then @9457 self.
        if !dead(w, o) {
            destroy_projectile(w, o, false);
        }
        if !dead(w, id) {
            destroy_projectile(w, id, false);
        }
        return;
    }
    // @9472-9586: BPI_Projectiles (ParentProjectilePhysics and its children; HarleyBomb_C, a DealerActor in the port) and
    // BPI_SpawnedActor; BPI_GetProjectileAttackType @9679.
    let cls = by.and_then(|b| actor_cls(w, b));
    let proj = match (by, cls) {
        (Some(b), Some(c)) => {
            c == name!("HarleyBomb_C")
                || actor_owner(w, b).is_some_and(|o| {
                    actor_is_a(w, ActorRef::Fighter(o), c, name!("ParentProjectilePhysics_C")) == Some(true)
                })
        }
        _ => false,
    };
    let kind = match by {
        Some(ActorRef::Projectile(o)) => pr(w, o).attack_type,
        Some(b) => num(actor_p(w, b), "ProjectileAttackType").map_or(0, to_int32),
        None => 0,
    };
    if !proj || kind == 2 {
        let p = pm(w, id);
        let age = p.age;
        of_mut(p).shake_at = age; // HitShake.PlayFromStart @9201
        return;
    }
    // @9817: ApplyDamage(self, Damage x 4, null, self, null): ParentProjectilePhysics' ReceiveAnyDamage with the actor as
    // its own causer (type 0); the OnTakeDamage that follows pops at @9332.
    let at = pr(w, id).attack_type;
    if receive_damage(w, id, dmg * 4.0, at) {
        if !dead(w, id) {
            destroy_projectile(w, id, false);
        }
    } else {
        any_damage_broadcast(w, id);
    }
}

/// `hitBy`: the hit's HitByEnemy: the damage [`any_damage`] took, ReceiveAnyDamage (Health 10, TakesDamage), then
/// OnTakeAnyDamage.
fn hit_by(
    w: &mut World,
    id: ProjectileId,
    _ally: bool,
    _dx: Real,
    _dy: Real,
    _bkb: Real,
    attacker_type: i32,
    by: Option<ActorRef>,
) {
    let dmg = of(pr(w, id)).pending_dmg;
    if dmg == 0.0 {
        return;
    }
    of_mut(pm(w, id)).pending_dmg = 0.0;
    // ReceiveAnyDamage: a destroy there skips the OnTakeAnyDamage broadcast.
    if receive_damage(w, id, dmg, if attacker_type < 0 { 0 } else { attacker_type }) {
        if !dead(w, id) {
            destroy_projectile(w, id, false);
        }
        return;
    }
    any_damage_broadcast(w, id);
    on_take_damage(w, id, dmg, by);
}

/// The handler's IsReflectingProjectiles: false from the spawn's UpdateHitbox (the class's HitBoxDamage), true from
/// GolfClubVis @9268, then the golf swing's Hitframe packet's ReflectProjectiles (true) at its HitBoxesOn (@8697) and
/// false at its HitBoxesOff (@7272). A golfing Meeseeks is the one mode with the flag (ActivateObject runs once).
fn reflecting_projectiles(w: &World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let s = of(p);
    s.golfing && if s.frame_on { p.hit.reflect_projectiles == Some(true) } else { s.frame_idx < 0 }
}

/// Reflected @5130: authority K2_DestroyActor (then, Running, the gate on the dead actor).
fn reflected(w: &mut World, a: ActorRef) {
    if let ActorRef::Projectile(id) = a
        && !dead(w, id)
    {
        destroy_projectile(w, id, false);
    }
}

fn reflect_multicast(w: &mut World, a: ActorRef, _owner: FighterId, _pause: bool) {
    if let ActorRef::Projectile(id) = a {
        spawn_visuals(w, id);
    }
}

// ---------------------------------------------------------------- the mesh

/// OnCompleted @5046, OnBlendOut @5098, OnInterrupted @7927 -> @5073.
fn montage_event(w: &mut World, a: ActorRef, _e: MontageEvent, _montage: Name) {
    let ActorRef::Projectile(id) = a else { return };
    if of(pr(w, id)).destroy_on_end && !dead(w, id) {
        destroy_projectile(w, id, true);
    }
}

/// The Hitframe's HitBoxesOn on the actor's handler: its packet replaces HitBoxDamage, the actors hit forgotten.
fn hit_boxes_on(w: &mut World, a: ActorRef, tags: &[Name], packet: &HitPacket, scale: [Real; 3], idx: u32) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pm(w, id);
    p.hit = packet.clone();
    let s = of_mut(p);
    s.frame_on = true;
    s.frame_idx = i32::try_from(idx).unwrap_or(i32::MAX);
    s.frame_tags = tags.to_vec();
    s.frame_scale = scale[0];
    p.hit_set.clear();
    p.hit_actors.clear();
}

fn hit_boxes_off(w: &mut World, a: ActorRef, _tags: &[Name], idx: u32) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pm(w, id);
    if of(p).frame_idx != i32::try_from(idx).unwrap_or(i32::MAX) {
        return;
    }
    of_mut(p).frame_on = false;
    p.hit_set.clear();
    p.hit_actors.clear();
}

// ---------------------------------------------------------------- Buff_MeeseeksDamageWithHitstun_C

/// DealtDamageModCondition: the victim's Comp_Pawn_HitstunHandler valid and CheckHitstunActive @145, returned @190 (no
/// handler: false @214). Its data: DealtDamageMods.DamageMod 2.0, Multiply. Only Buff_C020_Meeseeks (the Meeseeks perk,
/// perks.rs) spawns it, on the summon's own Comp_State_Manager: the summon's hit runs it in its dealt pass, so a summon
/// deals double damage to a fighter in hitstun. The fighter-side entry is the same condition (no caller puts the class
/// on a fighter). A HitActor that is no fighter (the fighter-side pass on a projectile, item or actor) has no
/// Comp_Pawn_HitstunHandler (SpawnedActorParent's SCS has none): false @214. SpawnedPlatformParent's children carry
/// one, but none of them is a HitActor of a fighter's hit box in the port (no hurt box, damage box or pawn body), and
/// the condition is on no fighter.
fn victim_in_hitstun(w: &World, victim: ActorRef) -> bool {
    matches!(victim, ActorRef::Fighter(v) if in_hitstun(w, v))
}

fn dealt_cond(
    w: &mut World,
    victim: ActorRef,
    _o: FighterId,
    _d: FighterId,
    _h: &HitPacket,
    _b: BuffRef,
    _hb: bool,
) -> bool {
    victim_in_hitstun(w, victim)
}

fn actor_dealt_cond(w: &mut World, _host: ActorRef, _b: u32, victim: ActorRef, _h: &HitPacket, _hb: bool) -> bool {
    victim_in_hitstun(w, victim)
}

/// This file's registry entries (Meeseeks_SpawnedActor_C, MeeSeeks_AirFlap_C, Buff_MeeseeksDamageWithHitstun_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ProjectileHooks {
        construct: Some(construct),
        bodies: Some(bodies),
        make: Some(make),
        begin: Some(begin),
        reflect: ReflectHooks {
            reflected: Some(reflected),
            multicast: Some(reflect_multicast),
            ..ReflectHooks::default()
        },
        step: Some(step),
        physics: Some(physics),
        reflecting_projectiles: Some(reflecting_projectiles),
        damage_box: Some(damage_box),
        hit_by: Some(hit_by),
        activate: Some(activate),
        any_damage: Some(any_damage),
        destroyed: Some(destroyed),
        reverse_move: Some(reverse_move),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "Meeseeks_SpawnedActor_C",
        ActorEntry {
            mesh: Some(MeshHooks {
                montage_event: Some(montage_event),
                hit_boxes_on: Some(hit_boxes_on),
                hit_boxes_off: Some(hit_boxes_off),
                ..MeshHooks::default()
            }),
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::MeeseeksSpawnedActor),
                ActorHooks::Projectile(hooks),
            )
        },
    )?;
    super::air_flap::register(r)?;
    r.buffs.enter(
        "Buff_MeeseeksDamageWithHitstun_C",
        BuffEntry {
            actor: Some(ActorBuffHooks { dealt_cond: Some(actor_dealt_cond), ..ActorBuffHooks::default() }),
            ..BuffEntry::new(
                BuffClass::BuffMeeseeksDamageWithHitstun,
                BuffHooks { dealt_cond: Some(dealt_cond), ..BuffHooks::default() },
            )
        },
    )
}
