//! Gizmo's popcorn and hide box: C023A_PopCorn ([`Popcorn`], the one class
//! with extra dealing boxes: HitBox beside ShotGunHitBox, OverlapTrigger) and C023A_ProjectileBarrier (the hide box:
//! `actors::barrier`'s model under its own begin overlap, [`HIDE_BOX_HOOKS`]). No pawn hook.
//!
//! Classes and their sim kind:
//! - C023A_PopCorn_C: a SpawnedActorParent child with a ProjectileMovement (bounce 0.2, gravity 2, homing magnitude
//!   1e6), a HurtBox and hit boxes, spawned by C023A_Attack_Popcorn_Montage's ConditionalSpawner: a Projectile (the
//!   shared movement, homing, hit flow, lifetime and blast box are the projectile's).
//! - C023A_ProjectileBarrier_C: a SpawnedActorParent child (BoxHide), `actors::barrier`'s sphere model under its own
//!   begin overlap ([`BarrierHooks::begin`]): a world actor, drawn by Gizmo's actors() render hook.
//!
//! Popcorn's overrides of the shared projectile code are called from `actors::projectile`: [`hit_xy`],
//! [`hurt_dist2`], [`extra_hit_boxes`], [`yaw_x`] (its HurtBox is a capsule: `hurt_core` gives none for it).
//! The extra boxes: `combat::phases::projectile_test` tests box `i` of [`extra_hit_boxes`] through [`box_hit`],
//! [`box_hit_set`], [`box_ally_over`], [`box_shapes`] and [`box_meets_fighter`].

use indexmap::IndexSet;
use mvs_data::value::Props;

use crate::Real;
use crate::State;
use crate::actors::barrier::{bar, bar_mut};
use crate::actors::projectile::{
    Projectile, ProjectileExt, class_def, class_p, hurt_x, hurt_y, set_bounciness, set_gravity_scale, set_hitbox,
    set_instance_vars, stop_simulating,
};
use crate::actors::world_actor::{actor_cls, actor_entry, actor_owner};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::buff_of;
use crate::combat::elem::{charge_pct, clear_homing, elem_of, set_homing};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::spawned_hits::damage_spawned_actor;
use crate::combat::{BuffClass, DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_def, latent_frames, lookup_owner, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_DYNAMIC, OBJ_WORLD_STATIC, TraceBody, body_info,
    sphere_overlap_actors,
};
use crate::fighter::{FighterId, cap_radius, capsule_half, facing, invulnerable, no_collision};
use crate::math::ue::{KINDA_SMALL_NUMBER, clamped_to_size, normal, rot_from_x, rotate_angle_axis, size2d};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{arr, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `POPCORN`.
pub const POPCORN: &str = "C023A_PopCorn_C";
/// `HIDE_BOX`.
pub const HIDE_BOX: &str = "C023A_ProjectileBarrier_C";
const BOUNCE: Name = name!("Buff_C023A_Bounce_C");

/// PopcornGrow's Grow track (CurveFloat_0, timeline length 3, not looped): [time, value, arrive, leave]; the last key's
/// interp mode (linear) has no segment after it. FRichCurve::Eval of a cubic segment as `music.rs` bubble_curve (the
/// stored tangents, weights none).
const GROW_KEYS: [[Real; 4]; 3] = [
    [0.0, 0.0, 1.4724706, 1.4724721],
    [0.73972124, 0.7166486, 0.56903744, 0.56903774],
    [2.4639812, 0.99762225, 0.0, 0.0],
];
const GROW_LENGTH: Real = 3.0;
/// ShotGunTimeline's Alpha track (CurveFloat_1: linear (0, 0) -> (0.2, 1)), length 0.2.
const SHOT_LENGTH: Real = 0.2;
/// OverlapTrigger.SphereRadius (on the Capsule, bAbsoluteScale: not grown).
const OVERLAP_R: Real = 105.0;
/// HitBox.SphereRadius (tag HitBox), times PopcornGrow's scale.
const HIT_R: Real = 60.0;
/// ShotGunHitBox.SphereRadius (tag ShotGun), times the shot's scale.
const SHOT_R: Real = 60.0;
/// HurtBox capsule radius / half height, times PopcornGrow's scale.
const HURT_R: Real = 60.0;
const HURT_HH: Real = 90.0;

/// `Popcorn extends Projectile` (C023A_PopCorn_C): the actor's variables and the view's fields (`hidden`,
/// `grow_scale`).
#[derive(State, Debug)]
pub struct Popcorn {
    /// ParticleSystem (PS_C023A_PopcornBall) hidden: SetVisibility(false) at Explode @2837 and at Shotgun @12131.
    pub hidden: bool,
    /// The ParticleSystem's relative scale, VLerp(1, 2, Grow) of PopcornGrow (@13996-14059).
    pub grow_scale: Real,
    /// PopcornGrow's playback position (Play at BeginPlay).
    pub grow_pos: Real,
    /// HitGateOpen (the Delay(0.3)).
    pub hit_gate: bool,
    /// Explode's DoOnce closed.
    pub exploded: bool,
    /// NumberOfPops.
    pub pops: i32,
    /// World tick Explode set PopIntervalTimer at (-1: none).
    pub pop_start: i32,
    /// World tick BeginPlay set ExplodeTimer (ExplotionGateCheck, looping) at.
    pub gate_start: i32,
    /// The age the RetriggerableDelay(0.3) of the last launch resumes at (-1: none).
    pub bump_at: Real,
    /// LastAttacker.
    pub last_attacker: Option<ActorRef>,
    /// HomingTarget (the car).
    pub homing_target: Option<ProjectileId>,
    /// ShotGunHitDamage as Shotgun leaves it (its BaseKnockBack / KnockBackScaling writes add up over calls).
    pub shot_hit: HitPacket,
    /// The last Shotgun's Fire.
    pub shot_fire: bool,
    /// ShotGunTimeline's playback position (-1: not playing).
    pub shot_pos: Real,
    /// ShotStartLocation.
    pub shot_x: Real,
    pub shot_y: Real,
    /// Normal(ShotVector) in the sim's axes.
    pub shot_nx: Real,
    pub shot_ny: Real,
    /// ShotGunHitBox's world location while the shot is on.
    pub box_x: Real,
    pub box_y: Real,
    /// HitBox as a box beside ShotGunHitBox from the first Shotgun on (hit / hit list are then ShotGunHitBox's).
    pub root_box: Option<RootBox>,
    /// OverlapTrigger's begin of the overlap (`allyOver` of its box).
    pub trig_ally_over: IndexSet<i32>,
    /// The OverlapTrigger's overlaps as the last test left them (a begin overlap is a new entry).
    pub in_trigger: Vec<ProjectileId>,
}

impl Default for Popcorn {
    fn default() -> Self {
        Self {
            hidden: false,
            grow_scale: 1.0,
            grow_pos: 0.0,
            hit_gate: false,
            exploded: false,
            pops: 0,
            pop_start: -1,
            gate_start: -1,
            bump_at: -1.0,
            last_attacker: None,
            homing_target: None,
            shot_hit: HitPacket::default(),
            shot_fire: false,
            shot_pos: -1.0,
            shot_x: 0.0,
            shot_y: 0.0,
            shot_nx: 0.0,
            shot_ny: 0.0,
            box_x: 0.0,
            box_y: 0.0,
            root_box: None,
            trig_ally_over: IndexSet::new(),
            in_trigger: Vec::new(),
        }
    }
}

/// `PopcornRootBox`: HitBox while the shot is on: the root's sphere of 60 x PopcornGrow's scale, with its own entry of
/// HitBoxesToDamage (ActivateHitbox @16694: the spawn's HitBoxDamage, which no later HitBoxesOn rewrites) and the hit
/// list of that packet's AttackTag ("Hit"; ShotGunHitDamage's is "None": CheckIfActorHasBeenHit @2906,
/// TrackActorsHitByTag @5290 key the lists by AttackTag).
/// Deviation: it is not tested against other spawned actors while the shot is on (the world's actor test reads
/// ShotGunHitBox alone, `hit_near`).
#[derive(State, Debug, Default)]
pub struct RootBox {
    pub hit: HitPacket,
    pub hit_set: IndexSet<i32>,
    pub ally_over: IndexSet<i32>,
}

/// Which extra box a place of [`extra_hit_boxes`] is: with the root box the list is [root, trigger], else [trigger].
/// OverlapTrigger's own begin overlap (the arrow / car sensor) meets PhysicsBody and ProjectileDestructible.
/// Deviation: its other listed channels and HitBox-type bodies are left out; the test is the actors the arrow / car
/// classes are met by. Deviation: the trigger box is not tested against other spawned actors' bodies
/// (ProjectileDestructible, DamageDestructible, ItemDestructible): the world's actor test reads the actor's own box
/// alone. Unverified: the begin overlaps of HitBox and OverlapTrigger with one fighter in one tick: HitBox first.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PopBox {
    Root,
    Trigger,
}

fn pop_box(pc: &Popcorn, i: usize) -> PopBox {
    if pc.root_box.is_some() && i == 0 { PopBox::Root } else { PopBox::Trigger }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn pc_of(p: &Projectile) -> &Popcorn {
    match &p.ext {
        ProjectileExt::Popcorn(c) => c,
        _ => panic!("a popcorn hook on a projectile that is none"),
    }
}

fn pc(w: &World, id: ProjectileId) -> &Popcorn {
    pc_of(pr(w, id))
}

fn pcm(w: &mut World, id: ProjectileId) -> &mut Popcorn {
    match &mut pm(w, id).ext {
        ProjectileExt::Popcorn(c) => c,
        _ => panic!("a popcorn hook on a projectile that is none"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a popcorn has an owner")
}

// ---------------------------------------------------------------- the timelines

/// PopcornGrow's Grow curve at `t`.
fn grow_curve(t: Real) -> Real {
    let k = &GROW_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    if t >= k[k.len() - 1][0] {
        return k[k.len() - 1][1];
    }
    let mut i = 0;
    while t >= k[i + 1][0] {
        i += 1;
    }
    let (a, b) = (k[i], k[i + 1]);
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

/// ShotGunTimeline's Alpha at `t`.
fn shot_curve(t: Real) -> Real {
    min(max(t / SHOT_LENGTH, 0.0), 1.0)
}

/// `setGrow`: HitBox and HurtBox at the relative scale `s` (PopcornGrow's update): the hit circle is HitBox's (HitBox2D:
/// none in the class; the base constructor finds no radius) and the HurtBox a capsule (the base takes a sphere of 32).
fn set_grow(p: &mut Projectile, s: Real) {
    let ProjectileExt::Popcorn(c) = &mut p.ext else { return };
    c.grow_scale = s;
    if c.shot_pos < 0.0 {
        p.hit_radius = HIT_R * s * p.scale;
    }
    p.hurt_radius = HURT_R * s * p.scale;
}

// ---------------------------------------------------------------- the overrides

/// Popcorn's `hurtDist2`: the HurtBox capsule in the play plane (upright, on the Capsule at its origin): its segment's
/// half length is the half height less the radius.
pub fn hurt_dist2(p: &Projectile, x: Real, y: Real) -> Real {
    let c = pc_of(p);
    let h = (HURT_HH - HURT_R) * c.grow_scale * p.scale;
    let (hx, hy) = (hurt_x(p), hurt_y(p));
    let cy = min(max(y, hy - h), hy + h);
    let (dx, dy) = (x - hx, y - cy);
    dx * dx + dy * dy
}

/// Popcorn's `hitX` / `hitY`: the box the world's test deals: HitBox at the root, then ShotGunHitBox where the shot's
/// timeline puts it (HitBoxTags [ShotGun]); HitBox goes on dealing beside it, and OverlapTrigger from the spawn on
/// ([`extra_hit_boxes`]). Every packet the popcorn's handler deals once it has exploded or shot (ExplodeHitBoxDamage,
/// ShotGunHitDamage) has IgnoreFacing: `projectileHitDir`'s X along the actor's side is (1, y) -> sim -x, as pfgDoDamage
/// @11068 has it for any handler. Before that its HitBoxDamage has a zero direction (the side changes nothing) and the
/// ball is drawn along its velocity.
pub fn hit_xy(p: &Projectile) -> (Real, Real) {
    let c = pc_of(p);
    if c.shot_pos >= 0.0 { (c.box_x, c.box_y) } else { (p.x, p.y) }
}

/// Popcorn's `extraHitBoxes`: the root box and the trigger box once shot, else the trigger box.
pub fn extra_hit_boxes(w: &World, id: ProjectileId) -> usize {
    if pc(w, id).root_box.is_some() { 2 } else { 1 }
}

/// The packet of extra box `i` (`ExtraHitBox.hit`): the trigger's is the root box's once it exists, else the actor's.
pub fn box_hit(p: &Projectile, i: usize) -> &HitPacket {
    let c = pc_of(p);
    let _ = i;
    match &c.root_box {
        Some(r) => &r.hit,
        None => &p.hit,
    }
}

/// The hit list of extra box `i` (`ExtraHitBox.hitSet`): the trigger's is the root box's once it exists.
pub fn box_hit_set(p: &Projectile, _i: usize) -> &IndexSet<i32> {
    match &pc_of(p).root_box {
        Some(r) => &r.hit_set,
        None => &p.hit_set,
    }
}

/// [`box_hit_set`] to write.
pub fn box_hit_set_mut(p: &mut Projectile, i: usize) -> &mut IndexSet<i32> {
    let _ = i;
    let Projectile { ext, hit_set, .. } = p;
    match ext {
        ProjectileExt::Popcorn(c) => match &mut c.root_box {
            Some(r) => &mut r.hit_set,
            None => hit_set,
        },
        _ => panic!("a popcorn box on a projectile that is none"),
    }
}

/// The begin of the overlap kept for extra box `i` (`ExtraHitBox.allyOver`).
pub fn box_ally_over_mut(p: &mut Projectile, i: usize) -> &mut IndexSet<i32> {
    let ProjectileExt::Popcorn(c) = &mut p.ext else { panic!("a popcorn box on a projectile that is none") };
    match (pop_box(c, i), &mut c.root_box) {
        (PopBox::Root, Some(r)) => &mut r.ally_over,
        _ => &mut c.trig_ally_over,
    }
}

/// `ExtraHitBox.shapes()` of box `i`: the root box's sphere of 60 x PopcornGrow's scale at the actor, the trigger's
/// sphere of 105 at the actor.
pub fn box_shapes(p: &Projectile, i: usize, out: &mut Vec<WorldSphere>) {
    let c = pc_of(p);
    match pop_box(c, i) {
        PopBox::Root => {
            let r = HIT_R * c.grow_scale * p.scale;
            out.push(WorldSphere { x: p.x, y: p.y, z: p.sphere_3d.then_some(0.0), r, ..Default::default() });
        }
        PopBox::Trigger => {
            out.push(WorldSphere { x: p.x, y: p.y, z: None, r: OVERLAP_R * p.scale, ..Default::default() });
        }
    }
}

/// `ExtraHitBox.meetsFighter` of box `i` (`None`: the box has none, the shapes meet his hurt capsules). OverlapTrigger's
/// profile (object type WorldStatic) overlaps Pawn and leaves the HurtBox channel at its default (Ignore,
/// DefaultEngine.ini): it meets a fighter's root capsule (the Pawn profile blocks WorldStatic, so the pair overlaps),
/// not his hurt boxes; pfgDoDamage reads no part of the other component. The sphere against the root capsule (upright,
/// at the actor location) in the play plane; a touch is no overlap.
pub fn box_meets_fighter(w: &World, id: ProjectileId, i: usize, vic: FighterId) -> Option<bool> {
    let p = pr(w, id);
    if pop_box(pc_of(p), i) != PopBox::Trigger {
        return None;
    }
    if no_collision(w, vic) {
        return Some(false);
    }
    let r = cap_radius(w, vic);
    let half = capsule_half(w, vic) - r;
    let f = w.fighter(vic);
    let cy = min(max(p.y, crate::fighter::actor_y(w, vic) - half), crate::fighter::actor_y(w, vic) + half);
    Some(size2d([p.x - f.body.x, p.y - cy]) < OVERLAP_R * p.scale + r)
}

/// Popcorn's `yawX`: -1 while hidden, else the parent's.
pub fn yaw_x(p: &Projectile) -> Real {
    if pc_of(p).hidden {
        return -1.0;
    }
    if p.rot_x != 0.0 { p.rot_x } else { p.spawn_side.unwrap_or(p.facing) }
}

// ---------------------------------------------------------------- the actor's functions

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let p = pr(w, id);
    let e = Event::Sound { fighter: p.owner_id, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn emitter(w: &mut World, id: ProjectileId, vfx: Name, x: Real, y: Real, scale: Real, rot: [Real; 3]) {
    let p = pr(w, id);
    let e = Event::ProjectileDeath {
        cls: p.cls,
        fighter: p.owner_id,
        vfx: Some(vfx),
        x,
        y,
        ox: 0.0,
        rot,
        scale,
        tint: None,
        to: None,
    };
    w.events.push(e);
}

/// Comp_State_Manager.CheckHasBuff(Buff_Elemental_Fire) on the popcorn itself (a fire buff spawned on the actor by a
/// fire hit).
fn on_fire(w: &World, id: ProjectileId) -> bool {
    elem_of(w, ActorRef::Projectile(id), BuffClass::BuffElementalFire).is_some()
}

/// ProjectileMovement.Deactivate: the movement ticks no more and keeps its Velocity (UMovementComponent::Deactivate).
fn deactivate(w: &mut World, id: ProjectileId) {
    let p = pm(w, id);
    let (vx, vy) = (p.vx, p.vy);
    stop_simulating(p);
    p.vx = vx;
    p.vy = vy;
}

/// Explode(ByArrow, ArrowVelocity) @2729: a DoOnce; ArrowActivate = ByArrow and Arrow Velocity (every caller passes
/// false and (0, 0, 0): ExplodeArrowDamage is never dealt, and the pops' arrow offsets are never picked), the ball
/// hidden, broadcast HasExploded((0, 0, 800)) (`music::pop_corn_exploded`), PopIntervalTimer
/// K2_SetTimerDelegate(PopDamage, IntervalTime, looping) @2926 and PopDamage now @3010; not ArrowActivate:
/// ProjectileMovement.Deactivate @3039.
fn explode(w: &mut World, id: ProjectileId) {
    if pc(w, id).exploded || pr(w, id).dead {
        return;
    }
    let tick = w.flow.tick;
    let c = pcm(w, id);
    c.exploded = true;
    c.hidden = true;
    super::music::pop_corn_exploded(w, id, 0.0, 800.0);
    pcm(w, id).pop_start = tick;
    pop_damage(w, id);
    if !pr(w, id).dead {
        deactivate(w, id);
    }
}

/// The last pop's packet over `base`: 5 / 1250 / 8 with the direction (other's UE Y >= its own ? (1, 1) : (-1, 1))
/// @1158-1254.
fn last_pop_packet(base: &HitPacket, other_x: Real, pop_x: Real) -> HitPacket {
    let mut p = base.clone();
    p.damage = Some(5.0);
    p.base_knock_back = Some(1250.0);
    p.knock_back_scaling = Some(8.0);
    p.knock_back_direction_vector =
        Some(DataVec { x: Some(if other_x <= pop_x { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
    p
}

/// PopDamage @6844: NumberOfPops + 1; SphereOverlapActors(the actor location + the pop's offset, pops >= TotalPops ?
/// 200 : 120, [PhysicsBody, Pawn, WorldDynamic], the owner ignored) @7587. The offset by NumberOfPops: 1 (0, 0, -100),
/// 2 (0, 100, 100), else 0 (UE (X, Y, Z); sim x = -Y). For each actor with a Comp_Actor_Destructible IsVulnerableTo the
/// popcorn's TeamIndex (another team): the last pop rewrites ExplodeHitBoxDamage to 5 / 1250 / 8 with the direction;
/// the others deal ExplodeHitBoxDamage as it stands (the class's 1 / 800 / 1, (1, 1));
/// Comp_Pawn_HitBoxHandler.pfgDoDamage(no hit box, the actor, the packet, no buffs) @1393 / @2510. Then
/// ExplodeVisuals(offset, last) @2187 and on the last pop K2_DestroyActor @2267.
fn pop_damage(w: &mut World, id: ProjectileId) {
    let me = ActorRef::Projectile(id);
    let total = num(class_p(w, pr(w, id)), "TotalPops").unwrap_or(4.0);
    let c = pcm(w, id);
    c.pops += 1;
    let pops = c.pops;
    let last = crate::real(pops) >= total;
    let ox = if pops == 2 { -100.0 } else { 0.0 };
    let oy = if pops == 1 {
        -100.0
    } else if pops == 2 {
        100.0
    } else {
        0.0
    };
    let (px, py) = (pr(w, id).x, pr(w, id).y);
    let (cx, cy) = (px + ox, py + oy);
    let own = owner(w, id);
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN), crate::real(OBJ_WORLD_DYNAMIC)];
    let found = sphere_overlap_actors(
        w,
        cx,
        cy,
        if last { 200.0 } else { 120.0 },
        &types,
        &[Some(ActorRef::Fighter(own))],
        -1.0,
    );
    let team = w.fighter(own).team.team;
    let base = {
        let p = pr(w, id);
        obj(class_p(w, p), "ExplodeHitBoxDamage").map(|b| HitPacket::from_props(&w.names, b, None))
    }
    .unwrap_or_default();
    let tick = w.flow.tick;
    for o in found {
        if pr(w, id).dead {
            break;
        }
        match o {
            ActorRef::Fighter(vic) => {
                if w.fighter(vic).team.team == team || w.fighter(vic).life.dead > 0.0 {
                    continue;
                }
                let packet = if last { last_pop_packet(&base, w.fighter(vic).body.x, px) } else { base.clone() };
                let dl = dealer(w, me);
                if crate::combat::phases::no_hit_by_team(w, team, vic) || projectile_blocks(w, &dl, vic) {
                    continue;
                }
                if invulnerable(w, vic) {
                    let aid = pr(w, id).attack_id;
                    attack_dodged(w, me, vic, aid, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
                    continue;
                }
                dodge_over(w, me, vic);
                let d = packet.knock_back_direction_vector.unwrap_or_default();
                let dir = V2 { x: -d.x.unwrap_or(0.0), y: d.y.unwrap_or(0.0) };
                let pct = charge_pct(w, me);
                if apply_projectile_hit(w, &dl, vic, &packet, &[], V2 { x: px, y: py }, Some(dir), &[], pct) {
                    actor_hit_enemy(w, &dl, vic);
                }
            }
            ActorRef::Projectile(op) => {
                let (dead, cls, ox_) = {
                    let o_ = pr(w, op);
                    (o_.dead, o_.cls, o_.x)
                };
                if dead || owner_team(w, o) == team {
                    continue;
                }
                let destructible = actor_def(w, ActorRef::Fighter(lookup_owner(w, o)), cls)
                    .is_some_and(|d| d.components.contains_key("Comp_Actor_Destructible"));
                if !destructible {
                    continue;
                }
                let packet = if last { last_pop_packet(&base, ox_, px) } else { base.clone() };
                damage_spawned_actor(w, me, op, &packet);
            }
            ActorRef::Item(_) => warn_text(w, &format!("{POPCORN}: a pop met an item (not hit)")),
            _ => {}
        }
    }
    if pr(w, id).dead {
        return;
    }
    // ExplodeVisuals(Offset, LastHit) @6228: PS_C023A_PopcornExplosion01 at the location + offset, scale 0.75 for the
    // last pop, else 0.5; sfx_cha_C023a_popcorn_impact_fire at the actor location.
    emitter(w, id, name!("PS_C023A_PopcornExplosion01"), cx, cy, if last { 0.75 } else { 0.5 }, [0.0; 3]);
    sound(w, id, name!("sfx_cha_C023a_popcorn_impact_fire"));
    if last {
        crate::world::spawning::destroy_projectile(w, id, false);
    }
}

/// Shotgun(Fire, ShotVector) @12131 (shot: the vector in the sim's axes): the ball hidden; the cone emitter
/// (PS_C023A_PopcornExplosion_Cone, _Fire with Fire) at the location turned along the shot (FindLookAtRotation);
/// sfx_cha_C023a_popcorn_impact at the location; ProjectileMovement.Deactivate @13018; Fire: On Hit Buffs =
/// FireShotBuffs @13068. ShotGunHitDamage: BaseKnockBack = (Fire ? BKB + 200 : BKB) x (n.Z < 0 ? Lerp(1, 0.25, |n.Z|)
/// : Lerp(1, 1.25, n.Z)), KnockBackScaling = Fire ? KBS + 3 : KBS, KnockBackDirectionVector = (n.Y, n.Z) (n =
/// Normal(ShotVector)) @13449-13797, written on the variable (a second shot adds again); HitBoxDamage = it,
/// HitBoxTags = [ShotGun], HitBoxOn? and UpdateHitbox @13833-13922; ShotStartLocation = the location;
/// ShotGunTimeline.PlayFromStart @3076.
fn shotgun(w: &mut World, id: ProjectileId, fire: bool, sx: Real, sy: Real) {
    pcm(w, id).hidden = true;
    let n = normal([0.0, sx, sy], KINDA_SMALL_NUMBER); // @10703
    let (nx, ny) = (n[1], n[2]);
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    // FindLookAtRotation(location, location + RotateAngleAxis(shot, 0, X) x 100) @12213-12403, in UE axes (Y = -x):
    // MakeFromX of the difference, then FMatrix::Rotator (142f6cad0).
    let s = rotate_angle_axis([0.0, -sx, sy], 0.0, [1.0, 0.0, 0.0]);
    let to = [s[0] * 100.0, -x + s[1] * 100.0, y + s[2] * 100.0];
    let [pitch, yaw, _] = rot_from_x([to[0], to[1] - -x, to[2] - y]);
    let vfx = if fire { name!("PS_C023A_PopcornExplosion_Cone_Fire") } else { name!("PS_C023A_PopcornExplosion_Cone") };
    emitter(w, id, vfx, x, y, 1.0, [pitch, yaw, 0.0]);
    sound(w, id, name!("sfx_cha_C023a_popcorn_impact"));
    deactivate(w, id);
    if fire && let Some(v) = class_p(w, pr(w, id)).get("FireShotBuffs").cloned() {
        set_instance_vars(w, id, vec![("On Hit Buffs".to_string(), v)]);
    }
    let z = ny;
    let mult = if z < 0.0 { 1.0 + (0.25 - 1.0) * z.abs() } else { 1.0 + (1.25 - 1.0) * z };
    let p = pm(w, id);
    let ProjectileExt::Popcorn(c) = &mut p.ext else { unreachable!("a popcorn") };
    let sg = &mut c.shot_hit;
    let bkb = sg.base_knock_back.unwrap_or(0.0);
    sg.base_knock_back = Some((if fire { bkb + 200.0 } else { bkb }) * mult);
    let kbs = sg.knock_back_scaling.unwrap_or(0.0);
    sg.knock_back_scaling = Some(if fire { kbs + 3.0 } else { kbs });
    sg.knock_back_direction_vector = Some(DataVec { x: Some(-nx), y: Some(ny), z: None });
    // UpdateHitbox -> SpawnedActorParent @15092 -> @8531 HitBoxesOn([ShotGun]): ShotGunHitBox on with this packet,
    // HitBox (and OverlapTrigger, tagged HitBox too) left on with their entries; HitBoxesOn clears no hit list. The
    // lists are the packets' AttackTags' (RootBox): ShotGunHitDamage's "None" has hit no one at the first shot, and
    // the "Hit" list moves to the root box; a second shot inside the first's 0.2 s keeps both.
    let shot = c.shot_hit.clone();
    if c.root_box.is_none() {
        c.root_box =
            Some(RootBox { hit: p.hit.clone(), hit_set: std::mem::take(&mut p.hit_set), ally_over: IndexSet::new() });
        p.hit_actors.clear();
    }
    p.hit = shot;
    set_hitbox(p, true);
    let ProjectileExt::Popcorn(c) = &mut p.ext else { unreachable!("a popcorn") };
    c.shot_fire = fire;
    c.shot_x = x;
    c.shot_y = y;
    c.shot_nx = nx;
    c.shot_ny = ny;
    c.shot_pos = 0.0;
    // Until the timeline's first update ShotGunHitBox is where its template has it: at the root, relative scale 1.
    c.box_x = x;
    c.box_y = y;
    p.hit_radius = SHOT_R * p.scale;
}

/// ShotGunTimeline's update @10527: ShotGunHitBox's relative scale Lerp(0.5, Fire ? 5.5 : 4.5, Alpha) and its world
/// location VLerp(ShotStartLocation, ShotStartLocation + Normal(ShotVector) x (Fire ? ShotLengthFire : ShotLength),
/// Alpha) @10947. True: the timeline finished (@11000 K2_DestroyActor).
fn shot_tick(w: &mut World, id: ProjectileId) -> bool {
    let far_fire = num(class_p(w, pr(w, id)), "ShotLengthFire").unwrap_or(800.0);
    let far_plain = num(class_p(w, pr(w, id)), "ShotLength").unwrap_or(500.0);
    let p = pm(w, id);
    let ProjectileExt::Popcorn(c) = &mut p.ext else { unreachable!("a popcorn") };
    c.shot_pos += DELTA_SECONDS;
    let done = c.shot_pos >= SHOT_LENGTH;
    if done {
        c.shot_pos = SHOT_LENGTH;
    }
    let a = shot_curve(c.shot_pos);
    let far = if c.shot_fire { far_fire } else { far_plain };
    let scale = 0.5 + ((if c.shot_fire { 5.5 } else { 4.5 }) - 0.5) * a;
    c.box_x = c.shot_x + c.shot_nx * far * a;
    c.box_y = c.shot_y + c.shot_ny * far * a;
    p.hit_radius = SHOT_R * scale * p.scale;
    done
}

/// Homing(Actor, Velocity, SetVelocity, Fire, SoftFollow) @11491 (the car's call: velocity 0, SetVelocity false): the
/// same target: nothing; MaxSpeed = SetVelocity && Fire ? |Velocity| : 1500; Bounciness 0; HomingTarget, IsHoming,
/// HomingTargetComponent = its root, bIsHomingProjectile @11015, its OnEndPlay bound to Stop Homming.
fn homing(w: &mut World, id: ProjectileId, t: ProjectileId) {
    if pc(w, id).homing_target == Some(t) {
        return;
    }
    let mag = {
        let p = pr(w, id);
        class_def(w, p)
            .and_then(|d| obj(&d.components, "ProjectileMovement"))
            .and_then(|m| num(m, "HomingAccelerationMagnitude"))
            .unwrap_or(0.0)
    };
    let p = pm(w, id);
    p.max_speed = 1500.0;
    set_bounciness(p, 0.0);
    pcm(w, id).homing_target = Some(t);
    set_homing(w, ActorRef::Projectile(id), ActorRef::Projectile(t), mag);
}

/// Stop Homming @11113 (the target's EndPlay; ShouldStopVelocity false) and InteruptHoming(StopVelocity) @12107:
/// HomingTarget none, IsHoming false, bIsHomingProjectile false, Bounciness 0; with ShouldStopVelocity MaxSpeed =
/// max(|Velocity| x 0.2, 250).
fn stop_homing(w: &mut World, id: ProjectileId, stop_velocity: bool) {
    pcm(w, id).homing_target = None;
    clear_homing(w, ActorRef::Projectile(id));
    let p = pm(w, id);
    set_bounciness(p, 0.0);
    if stop_velocity {
        p.max_speed = max(size2d([p.vx, p.vy]) * 0.2, 250.0); // VSize @11298
    }
}

/// @8790: KnockValue = Normal((0, X, Y) of the hit's direction, or (0, 0, 1) when both are 0) x FMax(BKB, 1300) x 3,
/// LastAttacker, LaunchFromHitMulticast(KnockValue) @10267 (InteruptHoming(true); MaxSpeed 1500; gravity 0.5;
/// Velocity; RetriggerableDelay(0.3)), sfx_cha_C023a_popcorn_bump on itself.
/// Deviation (as `chars::steven::watermelon` knockback_self): the direction is taken in the world's horizontal sense
/// (`hitBy`'s dx); the build's MakeVector(0, X, Y) puts the hit param's X on UE Y as it comes.
fn bump(w: &mut World, id: ProjectileId, by: Option<ActorRef>, dx: Real, dy: Real, bkb: Real) {
    let (x, mut y) = (dx, dy);
    if x == 0.0 && y == 0.0 {
        y = 1.0;
    }
    let n = normal([0.0, x, y], KINDA_SMALL_NUMBER); // @9145
    let k = max(bkb, 1300.0) * 3.0;
    pcm(w, id).last_attacker = by;
    stop_homing(w, id, true);
    let at = latent_frames(0.3);
    let p = pm(w, id);
    p.max_speed = 1500.0;
    set_gravity_scale(p, 0.5);
    p.vx = n[1] * k;
    p.vy = n[2] * k;
    let age = crate::real(p.age);
    pcm(w, id).bump_at = age + at;
    sound(w, id, name!("sfx_cha_C023a_popcorn_bump"));
}

/// The arrow's begin over OverlapTrigger @4002-5450: StoredVelocity = the arrow's velocity, or with none
/// RotateAngleAxis(facing left ? (0, 2500, 0) : (0, -2500, 0), (left ? 60 : -60) x AimDirection, X) of the arrow owner
/// (AimDirection from a C023A_AnimBP: Gizmo's; another owner's cast fails and the vector stays unturned);
/// Shotgun(arrow IsFire OR its own fire, StoredVelocity) @5225; the arrow owner's Comp_HoldCharge.ClearCharges() @5350
/// (`bow::hold_charge_clear`); the arrow's K2_DestroyActor @5386; broadcast HasExploded(StoredVelocity) @5422.
fn arrow_hit(w: &mut World, id: ProjectileId, arrow: ProjectileId) {
    let fire = on_fire(w, id);
    let (o, mut vx, mut vy, arrow_fire) = {
        let a = pr(w, arrow);
        let f = match &a.ext {
            ProjectileExt::Arrow(x) => x.fire,
            _ => false,
        };
        (a.owner.expect("an arrow has an owner"), a.vx, a.vy, f)
    };
    if size2d([vx, vy]) == 0.0 {
        let left = facing(w, o) < 0.0;
        let uy = if left { 2500.0 } else { -2500.0 };
        let deg = if super::state::gizmo_of(w, o).is_some() {
            (if left { 60.0 } else { -60.0 }) * w.fighter(o).pose.aim_direction
        } else {
            0.0
        };
        let r = rotate_angle_axis([0.0, uy, 0.0], deg, [1.0, 0.0, 0.0]); // @4865
        vx = -r[1];
        vy = r[2];
    }
    shotgun(w, id, arrow_fire || fire, vx, vy);
    super::bow::hold_charge_clear(w, o);
    if !pr(w, arrow).dead {
        crate::world::spawning::destroy_projectile(w, arrow, false);
    }
    super::music::pop_corn_exploded(w, id, vx, vy);
}

/// OverlapTrigger's begin overlap @3762 (authority), with an actor implementing BPI_SpawnedActor of the popcorn's
/// TeamIndex: a C023A_Arrow_Projectile (`arrow_hit`), or a C023A_Car_SpawnedActor: HasOverheat OR its own fire ->
/// Explode @5783, then Homing(the car, 0, false, that, true) @5871.
fn trigger_begin(w: &mut World, id: ProjectileId, o: ProjectileId) {
    if owner_team(w, ActorRef::Projectile(o)) != owner_team(w, ActorRef::Projectile(id)) {
        return;
    }
    if matches!(pr(w, o).ext, ProjectileExt::Arrow(_)) {
        arrow_hit(w, id, o);
    } else if super::car::is_car(w, ActorRef::Projectile(o)) {
        let overheat = super::car::car_of(&pr(w, o).ext).is_some_and(|c| c.has_overheat);
        if overheat || on_fire(w, id) {
            explode(w, id);
        }
        if !pr(w, id).dead {
            homing(w, id, o);
        }
    }
}

/// A projectile of the popcorn's team other than itself within reach of OverlapTrigger (perf: the query is skipped
/// without one).
fn same_team_near(w: &World, id: ProjectileId) -> bool {
    let reach = OVERLAP_R + 400.0;
    let (me, team) = (pr(w, id), owner_team(w, ActorRef::Projectile(id)));
    let mut i = 0;
    while let Some(o) = w.actors.projectiles.nth(i) {
        i += 1;
        if o == id {
            continue;
        }
        let p = pr(w, o);
        if p.dead || owner_team(w, ActorRef::Projectile(o)) != team {
            continue;
        }
        if (p.x - me.x).abs() < reach && (p.y - me.y).abs() < reach {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------- C023A_PopCorn

/// The popcorn's `make`.
fn popcorn_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::Popcorn(Popcorn::default())
}

/// The popcorn's constructor tail: `shotHit = { ...p.ShotGunHitDamage }` and `setGrow(1)`.
fn popcorn_construct(w: &World, p: &mut Projectile) {
    let none = Props::new();
    let shot = HitPacket::from_props(&w.names, obj(class_p(w, p), "ShotGunHitDamage").unwrap_or(&none), None);
    if let ProjectileExt::Popcorn(c) = &mut p.ext {
        c.shot_hit = shot;
    }
    set_grow(p, 1.0);
}

/// The popcorn's `begin` (ReceiveBeginPlay @3109: the parent's (NumberOfActorsAllowed 2: past the limit the oldest
/// popcorn of the owner's SpawnedActors goes), Delay(0.3), PopcornGrow.Play(), Delay(1.0), NewBuffSpawned bound to
/// OnBuffAdded (needs ExplodeGate, which no node writes: never true), ExplodeTimer =
/// K2_SetTimerDelegate(ExplotionGateCheck, ExplodeGateTime 0.5, looping) @3364; SetTeamColor (the view's)).
fn popcorn_begin(w: &mut World, id: ProjectileId) {
    let tick = w.flow.tick;
    pcm(w, id).gate_start = tick;
}

/// The popcorn's `step`: the actor's tick, behind its movement's.
/// Deviation (as `music.rs`): the Delays resume here; the two timers fire here in the frame the world's timer pass
/// would fire them, ahead of that pass, every timerTicks(rate) ticks (a looping timer's float drift not followed); the
/// timelines tick here; OverlapTrigger's begin overlaps are tested here once a tick against where everything then is
/// (an arrow or car moving into it raises it in its own move in the build, earlier in the frame), a new entry since the
/// last test being a begin overlap, none at the spawn.
fn popcorn_step(w: &mut World, id: ProjectileId) -> bool {
    let age = crate::real(pr(w, id).age);
    if age == latent_frames(0.3) {
        // @15: gravity 0.4, MaxSpeed 250, HitGateOpen.
        let p = pm(w, id);
        set_gravity_scale(p, 0.4);
        p.max_speed = 250.0;
        pcm(w, id).hit_gate = true;
    }
    if age == latent_frames(1.0) {
        // @117: Velocity = ClampVectorSize(Velocity, -400, 400), gravity 0.1.
        let p = pm(w, id);
        let c = clamped_to_size([0.0, p.vx, p.vy], -400.0, 400.0);
        (p.vx, p.vy) = (c[1], c[2]);
        set_gravity_scale(p, 0.1);
    }
    if age == pc(w, id).bump_at {
        // @281: gravity 1, MaxSpeed 250.
        pcm(w, id).bump_at = -1.0;
        let p = pm(w, id);
        set_gravity_scale(p, 1.0);
        p.max_speed = 250.0;
    }
    if pc(w, id).grow_pos < GROW_LENGTH {
        // PopcornGrow's update @14205 / @14100 / @13996: HitBox, HurtBox and the ball at VLerp(1, 2, Grow).
        let c = pcm(w, id);
        c.grow_pos = min(c.grow_pos + DELTA_SECONDS, GROW_LENGTH);
        let s = 1.0 + grow_curve(c.grow_pos);
        set_grow(pm(w, id), s);
    }
    if pc(w, id).homing_target.is_some_and(|t| w.actors.projectiles.get(t).is_none_or(|p| p.dead)) {
        stop_homing(w, id, false);
    }
    if pc(w, id).shot_pos >= 0.0 && shot_tick(w, id) {
        return true;
    }
    // ExplotionGateCheck @7892: the popcorn's own Buff_Elemental_Fire -> Explode.
    let tick = w.flow.tick;
    let gate = timer_ticks(Some(num(class_p(w, pr(w, id)), "ExplodeGateTime").unwrap_or(0.5)));
    let gate_start = pc(w, id).gate_start;
    if tick > gate_start && crate::real(tick - gate_start) % gate == 0.0 && on_fire(w, id) {
        explode(w, id);
    }
    if pr(w, id).dead {
        return false;
    }
    // PopIntervalTimer -> PopDamage.
    let every = timer_ticks(Some(num(class_p(w, pr(w, id)), "IntervalTime").unwrap_or(0.1)));
    let pop_start = pc(w, id).pop_start;
    if pop_start >= 0 && tick > pop_start && crate::real(tick - pop_start) % every == 0.0 {
        pop_damage(w, id);
    }
    if pr(w, id).dead {
        return false;
    }
    // OverlapTrigger's begin overlaps (arrows and cars of its team).
    if !same_team_near(w, id) && pc(w, id).in_trigger.is_empty() {
        return false;
    }
    let (x, y, scale) = (pr(w, id).x, pr(w, id).y, pr(w, id).scale);
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)];
    let found = sphere_overlap_actors(
        w,
        x,
        y,
        OVERLAP_R * scale,
        &types,
        &[Some(ActorRef::Projectile(id))],
        crate::real(OBJ_WORLD_STATIC),
    );
    let now: Vec<ProjectileId> = found
        .into_iter()
        .filter_map(|o| match o {
            ActorRef::Projectile(op)
                if matches!(pr(w, op).ext, ProjectileExt::Arrow(_)) || super::car::is_car(w, o) =>
            {
                Some(op)
            }
            _ => None,
        })
        .collect();
    let was = std::mem::replace(&mut pcm(w, id).in_trigger, now.clone());
    for o in now {
        if pr(w, id).dead {
            break;
        }
        if !was.contains(&o) && w.actors.projectiles.get(o).is_some_and(|p| !p.dead) {
            trigger_begin(w, id, o);
        }
    }
    false
}

/// The popcorn's `hitBy` (OnHitByAlly @8082 / OnHitByEnemy @10476: OnHitByDamageParam and OnHitByAttacker stored; the
/// ally's ends for a C023A_Arrow_Projectile attacker @8193; then HitGateOpen @8208. A fighter attacker @8256: the
/// popcorn's Instigator with Buff_C023A_Bounce -> its HandleBounceOnAuthority, BounceSuccessful -> Explode @8761;
/// else the bump @8790. Another attacker: LastAttacker itself: nothing @9456; the car: the bump; a PopCorn, PianoNote
/// or MusicPickup: nothing @9821; a Destructible of its own team: the bump @9983; else its
/// BPI_GetProjectileAttackType (0 without the interface) in IgnoreProjectileTypes ([2, 4]): nothing, otherwise
/// DeathVisuals (PS_C023A_PopcornDeath at the location) and DestroySpawnedActor(false) @10212-10226.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn popcorn_hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    bkb: Real,
    attacker_type: i32,
    by: Option<ActorRef>,
) {
    let by_arrow = matches!(by, Some(ActorRef::Projectile(a)) if matches!(pr(w, a).ext, ProjectileExt::Arrow(_)));
    if ally && by_arrow {
        return;
    }
    if !pc(w, id).hit_gate {
        return;
    }
    if let Some(ActorRef::Fighter(f)) = by {
        if f == owner(w, id) && buff_of(w, f, BOUNCE).is_some() && super::perks::popcorn_bounce(w, id, f) {
            explode(w, id);
            return;
        }
        bump(w, id, by, dx, dy, bkb);
        return;
    }
    if by == pc(w, id).last_attacker {
        return;
    }
    if let Some(a) = by {
        if super::car::is_car(w, a) {
            return bump(w, id, by, dx, dy, bkb);
        }
        if let Some(cls) = actor_cls(w, a) {
            let s = w.names.str(cls);
            if s == POPCORN || s == super::music::PIANO_NOTE || s == super::music::MUSIC_PICKUP {
                return;
            }
        }
        if let Some(o) = actor_owner(w, a)
            && w.fighter(o).team.team == w.fighter(owner(w, id)).team.team
        {
            return bump(w, id, by, dx, dy, bkb);
        }
    }
    let ignore = arr(class_p(w, pr(w, id)), "IgnoreProjectileTypes");
    let t = if attacker_type < 0 { 0 } else { attacker_type };
    if ignore.iter().any(|v| v.as_f64() == Some(f64::from(crate::real(t)))) {
        return;
    }
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    emitter(w, id, name!("PS_C023A_PopcornDeath"), x, y, 1.0, [0.0; 3]);
    if !pr(w, id).dead {
        crate::world::spawning::destroy_projectile(w, id, false);
    }
}

/// The popcorn's `bodies` (`popcornBodies`): the HurtBox capsule (ProjectileDestructible; the base's sphere entry is
/// skipped for a class with its own `hurt_dist2`) while hurtRadius > 0, and OverlapTrigger (a sphere of 105,
/// WorldStatic, on from its template: Custom with no CollisionEnabled). HitBox and ShotGunHitBox (HitBox type) are the
/// hit flow's, as every projectile's.
fn popcorn_bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = pr(w, id);
    let comps = class_def(w, p).map(|d| &d.components);
    let hurt = body_info(comps.and_then(|c| obj(c, "HurtBox")), None);
    if let Some(info) = hurt.filter(|i| i.on)
        && p.hurt_radius > 0.0
    {
        let h = (HURT_HH - HURT_R) * pc_of(p).grow_scale * p.scale;
        let (hx, hy) = (hurt_x(p), hurt_y(p));
        out.push(TraceBody {
            ax: hx,
            ay: hy - h,
            bx: hx,
            by: hy + h,
            hx: 0.0,
            hy: 0.0,
            r: p.hurt_radius,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: name!("HurtBox"),
            actor: Some(a),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
    let trig = body_info(comps.and_then(|c| obj(c, "OverlapTrigger")), None);
    if let Some(info) = trig.filter(|i| i.on && i.kind >= 0) {
        out.push(TraceBody {
            ax: p.x,
            ay: p.y,
            bx: p.x,
            by: p.y,
            hx: 0.0,
            hy: 0.0,
            r: OVERLAP_R * p.scale,
            z: Real::NAN,
            object_type: info.kind,
            profile: info.profile,
            component: name!("OverlapTrigger"),
            actor: Some(a),
            platform: -1,
            blocks: info.blocks,
            poly: None,
            floor: None,
        });
    }
}

// ---------------------------------------------------------------- C023A_ProjectileBarrier

/// `hideBoxBegin`: the hide box's own StopProjectiles begin overlap @567 (spawned by C023A_Special_BoxHide_Montage's
/// AnimNotify_Spawner, attached to Gizmo at (0, 0, 25), destroyed at the notify's end): the other cast to
/// SpawnedActorParent (every Projectile), NOT BPI_EnvironmentalHazard and another TeamIndex @642-851 -> the other's
/// Blocked(GetOwner()) @881 (`blocked`: its DestroyOnBlocked destroys it), BlockedVFX(the barrier's own
/// K2_GetActorLocation) @926-954: sfx_cha_wonderwoman_bracerblock at the literal (0, 0, 0) with the Instigator @998 and
/// PS_HitImpact_Directional at the location, scale 1.5 @1088. Unlike ProjectileBarrier's: no
/// ShouldActorBeBlockedAndDestroyed (DestroyOnBlocked is Blocked's own test), no K2_DestroyActor of the other, no
/// BlockedBuffandMontage, and the effect is at the barrier.
/// Not ported (as `actors::barrier`): the owner's Buff_ProjectileBarrierDefense SetActive(true) @1795-2182.
fn hide_box_begin(w: &mut World, a: ActorId, p: ProjectileId) {
    let o = ActorRef::Projectile(p);
    let Some(act) = w.actors.actors.get(a) else { return };
    let (f, cls) = (act.owner, act.cls);
    if actor_entry(w, o).is_some_and(|e| e.hazard.is_some()) || owner_team(w, o) == w.fighter(f).team.team {
        return;
    }
    let Some((x, y)) = bar(w, a).map(|b| (b.x, b.y)) else { return };
    if let Some(b) = bar_mut(w, a) {
        b.blocked += 1;
    }
    crate::combat::hits::blocked(w, o, ActorRef::Fighter(f));
    let fid = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_wonderwoman_bracerblock"),
        x: Some(0.0),
        y: Some(0.0),
        hit: None,
        pooled: false,
        key: None,
    });
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_HitImpact_Directional")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.5,
        tint: None,
        to: None,
    });
}

/// The hide box's barrier hooks (`projectileBarrier({ begin: hideBoxBegin })`): `actors::barrier::child_hooks`'s arm.
pub const HIDE_BOX_HOOKS: crate::actors::barrier::BarrierHooks =
    crate::actors::barrier::BarrierHooks { blocks: None, get_ball_back: None, begin: Some(hide_box_begin) };

/// This file's registry entries: the popcorn, the hide box.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let popcorn = ProjectileHooks {
        make: Some(popcorn_make),
        construct: Some(popcorn_construct),
        begin: Some(popcorn_begin),
        step: Some(popcorn_step),
        hit_by: Some(popcorn_hit_by),
        bodies: Some(popcorn_bodies),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        POPCORN,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C023APopCorn), ActorHooks::Projectile(popcorn)),
    )?;
    r.actors.enter(HIDE_BOX, crate::actors::barrier::entry(WorldActorClass::C023AProjectileBarrier))
}
