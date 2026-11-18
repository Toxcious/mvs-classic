//! The enemy spit: ProjectileAimer_SpitJake_C ([`SpitAimer`], the aim of the spit
//! while Jake holds a swallowed enemy), JakeSpitCharacterHitBox_C ([`SpitHitBox`], a DealerActor: the spat fighter's
//! hit box); joined to inputPreTriggered (Comp_JakeEat's JakeSpitEvent: the spit and the ally drop). Its state on the
//! pawn: `JakeState::spit_aimer`.
//!
//! Comp_JakeEat's JakeSpitEvent @8046 (input 1 -> eat.rs `spit_server`, input 3 -> `drop_ally_server`),
//! ProjectileAimer_SpitJake (SetAimInfo, the turn, the arc @9510-9620 and its use @3734-5005, the launch with
//! SetWallBounceVerticalMin, UnpauseTimer @3572), JakeSpitCharacterHitBox (a DealerActor).
//!
//! Jake_Eat_Attack_Spit_Montage (data, the special while he holds an enemy): the Spawner at .0255 for 1.2248
//! (ProjectileAimer_SpitJake_C, AttachToActor, socket None, translation Z 75, DestroyAllActorsOnNotifyEnd), the
//! ButtonHoldSectionJump .137-.983 (End on the special's release), ActivateSpawned at 1.026 (all of the class).
// Unverified: a special press refused afterwards (hitstun, buff stun, wall slide, no attack) leaves the EatTimer
// paused by SpitClient with no aimer to unpause it, as the build's graph has it (the timer runs again only at an
// aimer's ActivateObject).
// Deviation: the aimer's tick runs in the world's actor pass (the frame's spawn included, behind the owner's notifies);
// ActivateObject in the activate pass of the frame of the notify; the build's tick group and order against Jake's mesh
// tick are not read.
// Deviation: the AimSpline's mesh keeps its width at a root scale other than 1 (the view's spline is drawn at scale 1).
// Deviation (SetAimInfo): the overlap is `sphere_overlap_actors` (the Pawn bodies it gathers; the swallowed fighter has
// its collision off, Buff_JakEat's SetCollision, and is not met); its order is the traces' order.
// Unverified: VelocityInsteadOfFacing is taken to find no velocity for the hit box (the attached actor's
// ProjectileMovement is deactivated by the parent's BeginPlay; SpawnedActorParentVelocity is not read for this class),
// so the packet's vector gives the direction.
// Deviation: the hit box's KnockedBack and HitStunEnded are sampled in its step (`knocked_back_count`,
// `hitstun_end_time` against the values at the bind), not taken inside the broadcasts; the Delay and the life span fire
// in its late pass (as house.rs). Its hit box meets fighters alone (no other spawned actor, no projectile).

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::has_buff;
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hit::launch::bpi_set_wall_bounce_vertical_min;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, direct_hit, projectile_blocks};
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::consts as C;
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_PAWN, sphere_overlap_actors};
use crate::fighter::{FighterId, actor_y, facing, invulnerable, weight};
use crate::math::ue::{normal, rotate_angle_axis, round_to_int};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{ActorEntry, ActorHooks, CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawning::{add_actor, after_spawned_actor, new_world_actor, request_props};
use indexmap::IndexSet;

use super::eat::{drop_ally_server, spit_server, unpause_eat_timer};
use super::pawn::set_aim_direction;
use super::state::{jake_mut, jake_of};

/// ProjectileAimer_SpitJake's class.
pub const SPIT_AIMER: &str = "ProjectileAimer_SpitJake_C";

/// JakeSpitCharacterHitBox's class.
pub const SPIT_HITBOX: &str = "JakeSpitCharacterHitBox_C";

const JAK_EAT: Name = name!("Buff_JakEat_C");

/// The arc's gravity: 3 x 9.8 x -100.
fn grav() -> Real {
    (3.0 * 9.8) * -100.0
}

/// `SpitAimer` (ProjectileAimer_SpitJake_C). The fields the view reads: `x`, `y`, `arc` (the sim x, y per point, flat)
/// and `count` (the points in use).
#[derive(State, Debug)]
pub struct SpitAimer {
    pub x: Real,
    pub y: Real,
    pub arc: Vec<Real>,
    pub count: i32,
    /// StartAimVector Y, Z.
    pub sy: Real,
    pub sz: Real,
    /// ArrowAimVector Y, Z (X is 0).
    pub ay: Real,
    pub az: Real,
    /// FacingLeft (its own variable: the owner's IsFacingLeft from its first tick on).
    pub fl: bool,
    /// The attach's Z offset from the owner's location (the spawn point is 100 below it).
    pub up: Real,
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

impl Default for SpitAimer {
    /// The field initial values.
    fn default() -> Self {
        SpitAimer {
            x: 0.0,
            y: 0.0,
            arc: Vec::new(),
            count: 0,
            sy: 0.0,
            sz: 0.0,
            ay: 0.0,
            az: 0.0,
            fl: false,
            up: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
        }
    }
}

/// `SpitHitBox` (JakeSpitCharacterHitBox_C): a DealerActor attached to the spat fighter.
#[derive(State, Debug)]
pub struct SpitHitBox {
    /// The spat fighter (the attach parent).
    pub parent: FighterId,
    /// HitBoxDamage.
    pub hit: HitPacket,
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// HItBox's radius at HitBoxScale (the root's scale is taken at the read).
    pub radius: Real,
    /// The ProjectileMovement.InitialSpeed / MaxSpeed the packet's direction takes.
    pub initial_speed: Real,
    pub max_speed: Real,
    /// The BeginPlay Delay(.1)'s tick (-1: fired).
    pub on_at: Real,
    /// InitialLifeSpan's tick.
    pub end_at: Real,
    pub bound: bool,
    /// The parent's `knocked_back_count` at the bind.
    pub kb_seq: i32,
    /// The parent's `hitstun_end_time` at the bind.
    pub stun_end: Real,
    /// The fighters (ids) hit once.
    pub hit_set: IndexSet<i32>,
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

impl Default for SpitHitBox {
    fn default() -> Self {
        SpitHitBox {
            parent: FighterId::new(0, 0), // (a placeholder: a record's load overwrites it; `Id` has no Default)
            hit: HitPacket::default(),
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            hit_on: false,
            radius: 0.0,
            initial_speed: 0.0,
            max_speed: 0.0,
            on_at: 0.0,
            end_at: 0.0,
            bound: false,
            kb_seq: 0,
            stun_end: 0.0,
            hit_set: IndexSet::new(),
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
        }
    }
}

/// The ProjectileAimer_SpitJake_C's place (its `x`, `y`).
pub fn aimer_place(_w: &World, _a: &WorldActor, s: &SpitAimer) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeSpitCharacterHitBox_C's place (its `x`, `y`).
pub fn hit_box_place(_w: &World, _a: &WorldActor, s: &SpitHitBox) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeSpitCharacterHitBox_C as a dealer (`DealerActor`): its place, facing (-1) and AttackID.
pub fn dealer_parts(_w: &World, _a: &WorldActor, s: &SpitHitBox) -> (Real, Real, Real, i32) {
    (s.x, s.y, -1.0, s.attack_id)
}

/// The JakeSpitCharacterHitBox_C's hit packet (`hit`).
pub fn hit(s: &SpitHitBox) -> &HitPacket {
    &s.hit
}

/// `inputPreTriggered`: JakeSpitEvent @8046, bound to Jake's InputPreTriggered while `eat.spit_bound` (+= @6285,
/// -= @9445): input 1 (Special) -> SpitServer @8152; input 3 (Evade) -> DropAllyServer @8137; any other input
/// nothing. The special press's InputPreTriggered is the first statement of its event (BaseFighter @136859), so
/// SpitServer's IsAttacking test comes ahead of the press starting Jake_Eat_Attack_Spit_Montage.
fn input_pre_triggered(w: &mut World, f: FighterId, input: i32, _dir: i32) {
    let Some(s) = jake_of(w, f) else { return };
    if !s.eat.spit_bound {
        return;
    }
    if input == 1 {
        spit_server(w, f);
    } else if input == 3 {
        drop_ally_server(w, f);
    }
}

/// The hooks this file joins in Jake's CharHooks (`input_pre_triggered`).
pub fn hooks(h: &mut CharHooks) {
    h.input_pre_triggered = Some(input_pre_triggered);
}

fn aimer_of(w: &World, id: ActorId) -> Option<&SpitAimer> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::SpitAimer(s) => Some(s),
        _ => None,
    }
}

fn aimer_mut(w: &mut World, id: ActorId) -> Option<&mut SpitAimer> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SpitAimer(s) => Some(s),
        _ => None,
    }
}

fn hit_box_of(w: &World, id: ActorId) -> Option<&SpitHitBox> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::SpitHitBox(s) => Some(s),
        _ => None,
    }
}

fn hit_box_mut(w: &mut World, id: ActorId) -> Option<&mut SpitHitBox> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SpitHitBox(s) => Some(s),
        _ => None,
    }
}

fn aim_max(p: &mvs_data::value::Props) -> Real {
    num(p, "AimAngleMax").unwrap_or(0.0)
}

fn aim_min(p: &mvs_data::value::Props) -> Real {
    num(p, "AimAngleMin").unwrap_or(0.0)
}

fn deg_atan2(y: Real, x: Real) -> Real {
    crate::math::ue::deg_atan2(y, x)
}

/// RotateAngleAxis(V, deg, (1, 0, 0)): Y' = C Y - S Z, Z' = S Y + C Z.
fn rot_x(y: Real, z: Real, d: Real) -> (Real, Real) {
    let [_, y2, z2] = rotate_angle_axis([0.0, y, z], d, [1.0, 0.0, 0.0]);
    (y2, z2)
}

/// FindNearAngleDirection(Angle1, Angle2): the wrapped difference, ReturnMult 1 when it is negative (else -1),
/// Positive (it is negative), AngleDifrence = Round(it): (mult, positive, difference).
fn near(a1: Real, a2: Real) -> (Real, bool, Real) {
    let d0 = (a1 + 180.0) - (a2 + 180.0);
    let (dm, dp) = ((d0 - 360.0), (d0 + 360.0));
    let s = if d0.abs() < dm.abs() { d0 } else { dm };
    let r = if s.abs() < dp.abs() { s } else { dp };
    (if r < 0.0 { 1.0 } else { -1.0 }, r < 0.0, crate::math::real(round_to_int(r)))
}

/// UKismetMathLibrary::NormalizeToRange.
fn normalize_to_range(v: Real, lo: Real, hi: Real) -> Real {
    if lo == hi {
        return if v < lo { 0.0 } else { 1.0 };
    }
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
    (v - lo) / (hi - lo)
}

/// SetArrowRotationServer -> SetArrowRotationMulticast @13824: ArrowAimVector = the vector; the owner's
/// Comp_Spawner.ProjectileAimVector = ArrowAimVector @14378; BPI_SetAimDirection(NormalizeToRange(DegAtan2(Z, Y),
/// DegAtan2(Sz, Sy) + AimAngleMin, ... + AimAngleMax) x 2 - 1) on the owner's AnimBP @15404 (the SelectFloat on
/// literal true takes the unmirrored Y both times). Its ReceiveEndPlay does not reset ProjectileAimVector (no
/// ResetAim, unlike ProjectileAimer's): the last value stays.
fn set_arrow(w: &mut World, f: FighterId, a: &mut SpitAimer, p: &mvs_data::value::Props, y: Real, z: Real) {
    a.ay = y;
    a.az = z;
    let pa = &mut w.fighter_mut(f).items.proj_aim;
    pa.y = y;
    pa.z = z;
    let base = deg_atan2(a.sz, a.sy);
    let n = normalize_to_range(deg_atan2(z, y), base + aim_min(p), base + aim_max(p));
    set_aim_direction(w, f, (n * 2.0) - 1.0);
}

/// BPI_SetAimInfo @15927 -> @15467 (Comp_Spawner's call after the spawn): AimType 0 -> ArrowAimVector =
/// StartAimVector, the Arrow shown (the view's). Then @15937 (authority) SphereOverlapActors(location + (FacingLeft ?
/// (0, -500, 0) : (0, 500, 0)), 500, [Pawn], BP_FighterCharacter, ignore the Instigator) and the loop @5334: each
/// fighter with player data of another team than the aimer's Comp_Actor_Destructible (the owner's) ->
/// SetArrowRotationServer(..., Normal(its location + (0, 300, 0) - the aimer's, 1e-4)); the last one found wins.
/// FacingLeft is still the class default false here (its tick has not run), so the sphere's centre is at UE Y + 500
/// (sim x - 500) whichever way Jake faces: in front of him facing left, behind him facing right; and the vector is a
/// world one read in the aimer's own frame (Y forward). As the build has it.
fn set_aim_info(w: &mut World, f: FighterId, a: &mut SpitAimer, p: &mvs_data::value::Props) {
    let cx = a.x + if a.fl { 500.0 } else { -500.0 };
    let found = sphere_overlap_actors(w, cx, a.y, 500.0, &[crate::real(OBJ_PAWN)], &[Some(ActorRef::Fighter(f))], -1.0);
    for t in found {
        let ActorRef::Fighter(t) = t else { continue };
        if w.fighter(t).team.team == w.fighter(f).team.team {
            continue;
        }
        // UE (0, dY, dZ) with dY = (-t.x + 300) - (-a.x), dZ = t's Z - a's Z.
        let dy = ((-w.fighter(t).body.x) + 300.0) - (-a.x);
        let dz = actor_y(w, t) - a.y;
        let [_, ny, nz] = normal([0.0, dy, dz], 1e-4);
        set_arrow(w, f, a, p, ny, nz);
    }
}

/// ReceiveTick @7237: the turn @9702-11988: the limits a = RotateAngleAxis(Start, AimAngleMax, X), b = (Start,
/// AimAngleMin) (the SelectFloats on literal true: not mirrored by the facing, unlike ProjectileAimer's), angles
/// DegAtan2(Y, Z); the input angle DegAtan2(sign MoveX, -sign MoveY); FindNearAngleDirection(arrow, input) gives the
/// side; no turn with |its difference| >= 179, within 1 degree of the limit on that side (AimAngleMax != Min) or
/// without input; else a step of AimSpeed x DT clamped to that limit's rounded distance, turned by ReturnMult (away
/// from the input's angle). ArrowAimVector = (0, Y, Z), not normalised; SetArrowRotationServer every tick, turned or
/// not. The stick is not mirrored by the facing either.
fn turn(w: &mut World, f: FighterId, a: &mut SpitAimer, p: &mvs_data::value::Props) {
    let (mx_, mn_) = (aim_max(p), aim_min(p));
    let la = rot_x(a.sy, a.sz, mx_);
    let da2 = deg_atan2(la.0, la.1);
    let lb = rot_x(a.sy, a.sz, mn_);
    let da3 = deg_atan2(lb.0, lb.1);
    let da4 = deg_atan2(a.ay, a.az);
    let d0 = near(da4, da3).2;
    let d1 = near(da4, da2).2;
    let (mx, my) = (w.fighter(f).controls.inp.x, w.fighter(f).controls.inp.y);
    let ix = if mx == 0.0 {
        0.0
    } else if mx > 0.0 {
        1.0
    } else {
        -1.0
    };
    let iy = if my == 0.0 {
        0.0
    } else if my > 0.0 {
        -1.0
    } else {
        1.0
    };
    let (n_mult, n_pos, n_diff) = near(da4, deg_atan2(ix, iy));
    let sel = (if n_pos { d1 } else { d0 }).abs();
    let hold = n_diff.abs() >= 179.0 || (mx_ != mn_ && sel <= 1.0) || (mx == 0.0 && my == 0.0);
    let (mut y, mut z) = (a.ay, a.az);
    if !hold {
        let step0 = num(p, "AimSpeed").unwrap_or(0.0) * C::DELTA_SECONDS;
        let step = if mx_ == mn_ { step0 } else { min(max(step0, 0.0), sel) };
        (y, z) = rot_x(y, z, step * n_mult);
    }
    set_arrow(w, f, a, p, y, z);
}

/// The arc @7909-9681, every tick: the owner's IsFacingLeft; SpitFighter[0]'s Comp_Actor_Damage (none: CurrentDamage
/// and Weight read as 0); the speed q = (CurrentDamage^2 x KnockBackExpCurveMult + 1) x (((Damage / 10 + Damage x
/// CurrentDamage / 20) x 200 / (Weight + 100) x 1.4 + 18) x KnockBackScaling + BaseKnockBack) (the aimer's
/// HitBoxDamage); the velocity Normal((0, Y, Z), 1e-4) x q, its Y negated facing right; PredictProjectilePath from
/// the aimer at that velocity, SimFrequency 15, MaxSimTime 2, gravity 3 x 9.8 x -100: ArcPositions. The arc is the
/// view's alone: the launch takes the arrow, not the arc.
fn arc(w: &World, f: FighterId, a: &mut SpitAimer, p: &mvs_data::value::Props) {
    let fl = facing(w, f) < 0.0;
    let v = jake_of(w, f).and_then(|j| j.eat.spit_fighter.first().copied());
    let (d, wt) = match v {
        Some(v) => (w.fighter(v).stats.damage, weight(w, v)),
        None => (0.0, 0.0),
    };
    let none = mvs_data::value::Props::new();
    let hit = obj(p, "HitBoxDamage").unwrap_or(&none);
    let dmg = num(hit, "Damage").unwrap_or(0.0);
    let exp_mult = ((d * d) * num(p, "KnockBackExpCurveMult").unwrap_or(0.0)) + 1.0;
    let wf = 200.0 / (wt + 100.0);
    let s = ((dmg / 10.0) + ((dmg * d) / 20.0)) * wf;
    let kb =
        (((s * 1.4) + 18.0) * num(hit, "KnockBackScaling").unwrap_or(0.0)) + num(hit, "BaseKnockBack").unwrap_or(0.0);
    let q = exp_mult * kb;
    let [_, ny, nz] = normal([0.0, a.ay, a.az], 1e-4);
    let (vy, vz) = ((ny * q), (nz * q));
    // UE Y = FacingLeft ? vy : -vy; sim x = -UE Y.
    let mut out = std::mem::take(&mut a.arc);
    let n = crate::actors::predict_path::predict_projectile_path(
        w,
        a.x,
        a.y,
        if fl { -vy } else { vy },
        vz,
        15.0,
        2.0,
        grav(),
        &mut out,
    );
    a.arc = out;
    a.count = i32::try_from(n).expect("the arc's point count fits");
}

fn follow(w: &World, f: FighterId, a: &mut SpitAimer) {
    a.x = w.fighter(f).body.x;
    a.y = actor_y(w, f) + a.up;
}

/// The kind's `spawn`: the class data, the owner's attach, the blast-box loop, SetAimInfo.
fn aimer_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls).unwrap_or_else(|| {
        panic!("{SPIT_AIMER}: no actor data for its spawn (the importer gives every spawned class)")
    });
    let p = &def.p;
    let none = mvs_data::value::Props::new();
    let sv = obj(p, "StartAimVector");
    let (sy, sz) = match sv {
        Some(s) => (num(s, "Y").unwrap_or(0.0), num(s, "Z").unwrap_or(0.0)),
        None => (0.8, 0.2),
    };
    let np = request_props(w, f, r);
    let tr = obj(&np, "Transforms").and_then(|t| obj(t, "Translation")).unwrap_or(&none);
    let socket = text(&np, "Socket").is_some_and(|s| !s.is_empty() && s != "None");
    let truthy = |v: Option<Real>| v.is_some_and(|v| v != 0.0 && !v.is_nan());
    if !flag(&np, "AttachToActor")
        || socket
        || flag(&np, "WorldTransforms")
        || truthy(num(tr, "X"))
        || truthy(num(tr, "Y"))
    {
        warn_text(
            w,
            &format!("{SPIT_AIMER}: a spawn other than attached to the actor with a Z translation is not ported"),
        );
    }
    let up = num(tr, "Z").unwrap_or(0.0);
    let mut a = SpitAimer {
        sy,
        sz,
        ay: sy,
        az: sz,
        up,
        // the spawn point (owner - 100 + Z) until Comp_Spawner's attach @3852 -> @10515 (identity relative transform
        // plus the Z) after BPI_SetAimInfo @3806: the follow below moves it there
        x: w.fighter(f).body.x,
        y: (actor_y(w, f) + (up - 100.0)),
        ..SpitAimer::default()
    };
    // SpawnedActorParent ReceiveBeginPlay: the blast-box loop, NumberOfActorsAllowed 1 @5864-6515 (add_actor); then
    // its own BeginPlay @6436 (the AimSpline's material and team colour: the view's).
    let (mut x, mut y) = (a.x, a.y);
    let force = flag(p, "ForceSpawnPositionInsideBlastBox");
    let dead = blast_box_loop(
        &spawn_box(w),
        &mut x,
        &mut y,
        None,
        force,
        &mut a.blast_bounds,
        flag(p, "DestroyOnExitedArena?"),
    );
    a.x = x;
    a.y = y;
    set_aim_info(w, f, &mut a, p);
    follow(w, f, &mut a); // the attach @10232 -> @10515
    (WorldActorState::SpitAimer(a), dead)
}

/// The rest of the spawn once the World holds the actor: the live aimer on the pawn (`JakeState::spit_aimer`).
fn aimer_spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if let Some(j) = jake_mut(w, f) {
        j.spit_aimer = Some(ActorRef::Actor(id));
    }
}

/// Runs `body` with the aimer's state taken out of its actor (the body reads and writes the World besides).
fn with_aimer(w: &mut World, id: ActorId, body: impl FnOnce(&mut World, FighterId, &mut SpitAimer)) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let Some(s) = aimer_mut(w, id) else { return };
    let mut a = std::mem::take(s);
    body(w, f, &mut a);
    if let Some(s) = aimer_mut(w, id) {
        *s = a;
    }
}

/// SpawnedActorParent CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Projectile.expiry:
/// the spawn frame is not counted.
fn left_arena(w: &World, id: ActorId, a: &mut SpitAimer) -> bool {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    blast_timer_exit_rounded(doe, &mut a.blast_at, &a.blast_bounds, crate::real(a.age - 1), a.x, a.y)
}

fn aimer_step(w: &mut World, id: ActorId) {
    with_aimer(w, id, |w, f, a| {
        a.age += 1;
        follow(w, f, a);
        if left_arena(w, id, a) {
            if let Some(x) = w.actors.actors.get_mut(id) {
                x.dead = true;
            }
            return;
        }
        a.fl = facing(w, f) < 0.0; // @7732
        let p = actor_p(w, ActorRef::Actor(id)).clone();
        let t = num(&p, "AimType").unwrap_or(0.0);
        if t == 0.0 || t == 3.0 {
            turn(w, f, a, &p);
        }
        arc(w, f, a, &p);
    });
}

/// The packet of the launch's direct hit: HitBoxDamage's Damage, BaseKnockBack and KnockBackScaling,
/// KnockBackDirectionVector (Y, Z) of the arrow, AttackTag Spit, HitReaction 2, HitPauseTime 0, HitStunMult 1,
/// ForceKnockback, ApplyHitPauseToDamageDealer, AdditiveKBMode 0, KBI_Multiplier 1, the rest false / null / 0 (the
/// fighter's own handler, the X by Jake's facing); KnockBackDirection the same vector.
fn spit_packet(hit: &mvs_data::value::Props, ay: Real, az: Real) -> HitPacket {
    let v = DataVec { x: Some(ay), y: Some(az), z: None };
    HitPacket {
        sparse: true,
        damage: Some(num(hit, "Damage").unwrap_or(0.0)),
        base_knock_back: Some(num(hit, "BaseKnockBack").unwrap_or(0.0)),
        knock_back_scaling: Some(num(hit, "KnockBackScaling").unwrap_or(0.0)),
        knock_back_direction_vector: Some(v),
        knock_back_direction: Some(v),
        attack_tag: Some(name!("Spit")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(1.0),
        attack_id: Some(0),
        force_knockback: Some(true),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        attack_decay_type: Some(DataEnum::Value(0)),
        attack_decay_name: Some(name!("None")),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(false),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// ActivateObject @12030: HideUI (the view's); FighterChar's CheckHitstunActive -> nothing (the aimer stays until the
/// Spawner's end); else (authority) the loop @5306 over the owner's Comp_JakeEat.SpitFighter, each one with a
/// Buff_JakEat (BPI_CheckIfHasBuff): its BPI_SetWallBounceVerticalMin(true, .3) @1076; the owner's Comp_Spawner
/// CompSpawnActor(JakeSpitCharacterHitBox, attached to it, socket None, InitialSpeed = FacingLeft ? Y : -Y, MaxSpeed =
/// Z) @1465 (`spawn_hit_box`); the owner's HitBoxHandler pfgDoDamage(null, it, the packet) @3072 (`direct_hit`). Then
/// Comp_JakeEat.UnpauseTimer @3572 and K2_DestroyActor @3608. The SpitFighter array is read live per pass (the length
/// @3336): the hit's KnockedBack ends the fighter's Buff_JakEat (eat_buffs.rs), which takes it out of HeldFighters,
/// not out of SpitFighter.
fn aimer_activate(w: &mut World, id: ActorId) {
    let Some(x) = w.actors.actors.get(id) else { return };
    if x.dead {
        return;
    }
    let o = x.owner;
    if w.fighter(o).attackers.stun_source || w.fighter(o).buffs.buff_stunned {
        return; // CheckHitstunActive
    }
    let Some(s) = aimer_of(w, id) else { return };
    let (ay, az) = (s.ay, s.az);
    let none = mvs_data::value::Props::new();
    let hit = obj(actor_p(w, ActorRef::Actor(id)), "HitBoxDamage").unwrap_or(&none).clone();
    let mut i = 0;
    while let Some(v) = jake_of(w, o).and_then(|j| j.eat.spit_fighter.get(i).copied()) {
        i += 1;
        if !has_buff(w, v, JAK_EAT) {
            continue;
        }
        bpi_set_wall_bounce_vertical_min(w, v, true, 0.3);
        // the hitbox's ProjectileMovement InitialSpeed / MaxSpeed from Comp_Spawner's BPI_SetInitialMovement @8407:
        // ActivateObject @1465 passes direction (0,0,0), AddMomentumAlpha 0, MovementSpeedAddsToInitialSpeed false, so
        // InitialSpeed 0 and MaxSpeed max(|LocalInitialVelocity| 0, the MaxSpeed argument Z)
        spawn_hit_box(w, o, v, 0.0, max(0.0, az));
        let p = spit_packet(&hit, ay, az);
        direct_hit(w, o, v, &p);
    }
    unpause_eat_timer(w, o);
    if let Some(x) = w.actors.actors.get_mut(id) {
        x.dead = true;
    }
}

/// ReceiveEndPlay @13740: HideUI (the view's); the live aimer on the pawn cleared.
fn aimer_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    if let Some(j) = jake_mut(w, o)
        && j.spit_aimer == Some(ActorRef::Actor(id))
    {
        j.spit_aimer = None;
    }
}

// ------------------------------------------------------------------ JakeSpitCharacterHitBox

/// CompSpawnActor(JakeSpitCharacterHitBox_C) through Jake's Comp_Spawner: it joins his SpawnedActors (`add_actor`:
/// the charge copy and the granted buffs), its BeginPlay (the Delay), then AfterSpawnedActor (the owner's Buff_Enlarge
/// binding and the buffs').
/// Spawned by the aimer's ActivateObject, attached to the spat fighter (socket None, the identity relative transform:
/// at its location), InitialLifeSpan 2. HItBox: a sphere of 100 (tag HitBox) on the root.
fn spawn_hit_box(w: &mut World, o: FighterId, v: FighterId, initial_speed: Real, max_speed: Real) {
    let data = w.data.clone();
    let cls = w.names.get(SPIT_HITBOX).expect("the class is interned at bind");
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(o), cls) else { return };
    let p = &def.p;
    let none = mvs_data::value::Props::new();
    let radius = obj(&def.components, "HItBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0)
        * obj(p, "HitBoxScale").and_then(|s| num(s, "X")).unwrap_or(1.0);
    let t = crate::real(w.fighter(o).clock.tick);
    let mut s = SpitHitBox {
        parent: v,
        hit: HitPacket::from_props(&w.names, obj(p, "HitBoxDamage").unwrap_or(&none), None),
        attack_id: w.fighter(o).decay.attack_count, // CompSpawnActor's AttackID 0: the owner's attack count
        // follow: K2_AttachToActor @10335 + SetActorRelativeTransform(identity) @10515
        x: w.fighter(v).body.x,
        y: actor_y(w, v),
        hit_on: false,
        radius,
        initial_speed,
        max_speed,
        on_at: t + latent_frames(0.1) - 1.0,
        end_at: t + timer_ticks(num(p, "InitialLifeSpan")),
        bound: false,
        kb_seq: 0,
        stun_end: 0.0,
        hit_set: IndexSet::new(),
        age: 0,
        blast_at: 0.1,
        blast_bounds: new_blast_bounds(),
    };
    let force = flag(p, "ForceSpawnPositionInsideBlastBox");
    let (mut x, mut y) = (s.x, s.y);
    let dead = blast_box_loop(
        &spawn_box(w),
        &mut x,
        &mut y,
        None,
        force,
        &mut s.blast_bounds,
        flag(p, "DestroyOnExitedArena?"),
    );
    s.x = x;
    s.y = y;
    let cls_name = cls;
    let actor =
        new_world_actor(w, cls_name, WorldActorClass::JakeSpitCharacterHitBox, o, dead, WorldActorState::SpitHitBox(s));
    let id = w.actors.actors.insert(actor);
    let charge = crate::combat::elem::instigator_charge(w, o);
    add_actor(w, id, charge, true);
    after_spawned_actor(w, o, ActorRef::Actor(id));
}

fn hit_box_follow(w: &mut World, id: ActorId) {
    let Some(parent) = hit_box_of(w, id).map(|s| s.parent) else { return };
    let (x, y) = (w.fighter(parent).body.x, actor_y(w, parent));
    if let Some(s) = hit_box_mut(w, id) {
        s.x = x;
        s.y = y;
    }
}

fn hit_box_step(w: &mut World, id: ActorId) {
    if let Some(s) = hit_box_mut(w, id) {
        s.age += 1;
    }
    hit_box_follow(w, id);
    let Some(s) = hit_box_of(w, id) else { return };
    let v = s.parent;
    let (bound, kb, se) = (s.bound, s.kb_seq, s.stun_end);
    if bound && (w.fighter(v).attackers.knocked_back_count != kb || w.fighter(v).hit.hitstun_end_time != se) {
        // HitOut / HitStunEnded: K2_DestroyActor
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::SpitHitBox(s) = &mut a.state else { return };
    if blast_timer_exit_rounded(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y) {
        a.dead = true;
    }
}

/// BeginPlay @553 (authority) Delay(.1) -> @15: SetHitBoxServer(the attach parent) @594: HitBoxDamage = the class's
/// packet with KnockBackDirectionVector (ProjectileMovement.InitialSpeed, MaxSpeed) and IgnoreActor the parent, KBI 1
/// (the rest copied); HitBoxOn? true, UpdateHitbox. Then the parent's DamageManager KnockedBack -> HitOut and
/// HitstunHandler HitStunEnded -> HitStunEnded bound: both K2_DestroyActor. InitialLifeSpan at its tick.
fn hit_box_late(w: &mut World, id: ActorId) {
    let Some(o) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let t = crate::real(w.fighter(o).clock.tick);
    let Some(s) = hit_box_of(w, id) else { return };
    let (fire, parent, end) = (s.on_at >= 0.0 && t >= s.on_at, s.parent, t >= s.end_at);
    let (kb, se) = (w.fighter(parent).attackers.knocked_back_count, w.fighter(parent).hit.hitstun_end_time);
    let Some(s) = hit_box_mut(w, id) else { return };
    if fire {
        s.on_at = -1.0;
        // SetHitBoxServer @594.
        s.hit.knock_back_direction_vector = Some(DataVec { x: Some(s.initial_speed), y: Some(s.max_speed), z: None });
        s.hit.kbi_multiplier = Some(1.0);
        s.hit_on = true;
        s.bound = true;
        s.kb_seq = kb;
        s.stun_end = se;
    }
    if end && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true; // InitialLifeSpan
    }
}

/// HItBox (object type HitBox) where it is now, none while it is off.
fn hit_box_sphere(w: &World, id: ActorId) -> Option<WorldSphere> {
    let (a, s) = (w.actors.actors.get(id)?, hit_box_of(w, id)?);
    if !s.hit_on || a.dead {
        return None;
    }
    Some(WorldSphere {
        x: s.x,
        y: s.y,
        // No depth (spit.rs SHAPE has none): the overlap is measured in the play plane alone.
        z: None,
        r: s.radius * a.size_scale.unwrap_or(1.0),
        ..WorldSphere::default()
    })
}

fn hit_box_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    match hit_box_sphere(w, id) {
        Some(sp) => {
            out.push(sp);
            true
        }
        None => false,
    }
}

/// The hit: SpawnedActorParent's own handler on the HItBox's overlaps with the hurt shapes of the instigator's enemies
/// (the parent excepted: IgnoreActor), once each (HitEachActorOnlyOnce). Its direction: IgnoreFacing, the packet's X a
/// world UE Y (sim x = -X: `facing` -1, no velocity).
fn hit_box_hits(w: &mut World, id: ActorId) {
    let Some(sphere) = hit_box_sphere(w, id) else { return };
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let Some(s) = hit_box_of(w, id) else { return };
    let (parent, attack_id, hit) = (s.parent, s.attack_id, s.hit.clone());
    let team = w.fighter(o).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = hit_box_of(w, id) else { return };
        if vic == parent
            || w.fighter(vic).team.team == team
            || !alive(w, vic)
            || no_hit(w, vic)
            || s.hit_set.contains(&vid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = hurt_hit(w, vic, std::slice::from_ref(&sphere), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = hit_box_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &[], V2 { x: sphere.x, y: sphere.y }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// ReceiveEndPlay @2322 -> @329: the parent's BPI_SetWallBounceVerticalMin(false, .3) (in the Lab reset's pass the
/// build's EndPlay writes the old pawn; the new one starts from the CDO).
fn hit_box_end(w: &mut World, id: ActorId) {
    let Some(s) = hit_box_of(w, id) else { return };
    let parent = s.parent;
    if !reset_pass(w) {
        bpi_set_wall_bounce_vertical_min(w, parent, false, 0.3);
    }
}

/// This file's registry entries (ProjectileAimer_SpitJake_C, JakeSpitCharacterHitBox_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        SPIT_AIMER,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::ProjectileAimerSpitJake),
            ActorHooks::World(WorldActorHooks {
                // no shape: the arc is a world path from the actor
                size: Some(SizeHooks::default()),
                spawn: Some(aimer_spawn),
                spawned: Some(aimer_spawned),
                step: Some(aimer_step),
                activate: Some(aimer_activate),
                end: Some(aimer_end),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        SPIT_HITBOX,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeSpitCharacterHitBox),
            ActorHooks::World(WorldActorHooks {
                // HItBox at the root's scale; nothing drawn
                size: Some(SizeHooks::default()),
                hit_shapes: Some(hit_box_shapes),
                step: Some(hit_box_step),
                late: Some(hit_box_late),
                hits: Some(hit_box_hits),
                end: Some(hit_box_end),
                ..WorldActorHooks::default()
            }),
        ),
    )
}
