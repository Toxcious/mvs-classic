//! The ally spit: JakeEatAlly_Aim_C ([`JakeEatAllyAim`], the swallowed ally's
//! aim, made by Comp_JakeEat's AddHeldFighters: no spawn hook), JakeSpitAllyHitBox_C ([`JakeSpitAllyHitBox`], a
//! DealerActor: the launched ally's hit box), the ally drop's forced attack. Its state on the pawn: `JakeState::ally_aim`.
//!
//! Comp_JakeEat's DropAllyServer @11980 (eat.rs `drop_ally_server`) -> DropAllyClient @12106 with the forced
//! Spit_Ally_Attack (@12181-12394), JakeEatAlly_Aim (spawned by Comp_JakeEat @5678 from `add_held_fighters`, activated
//! by SpitTimeEnd @1835), JakeSpitAllyHitBox.
//!
//! Both are spawned by the swallowed ally's Comp_Spawner (CompSpawnActor, attach true, identity transform): the ally
//! is their owner (its SpawnedActors: Die With Instigator ends them at the ally's reset; NumberOfActorsAllowed counts
//! the ally's), whatever its character; the class data is Jake's (`actor_data` finds it through the peers).
//!
//! JakeEatAlly_Aim, attached to the ally's root at (0, 0, 0) (Capsule.SetAbsolute(false, true, true) @7518: rotation
//! and scale absolute only), its place the ally's. Comp_JakeEat @5678 passes InitialSpeed = Conv_BoolToFloat(Jake's
//! BPI_IsFacingLeft) and Instigator Jake.
//! - BeginPlay @7404 (the parent's first, @8099): the owner (the ally) a BaseFighter; it has Buff_JakEat: StoredPawn =
//!   the buff's Instigator (Jake), the buff's OnEndPlay -> DetachedFromJakeBuffCheck @8018 (BuffCheck false: destroy),
//!   Delay(0.3) -> @1095; else K2_DestroyActor @7942.
//! - @1095: SetAimLocationServer / NewRelitiveLocation (the target widget's place, the view's), then @15:
//!   FindClosestEnemyAimVector (`closest_enemy_aim`), else (0, 0.5, 0.5) when InitialSpeed > 0 (Jake facing left),
//!   (0, -0.5, 0.5) otherwise; ArrowAimVector = it; the owner's InputPreTriggered -> ActivatedLaunch @438-461;
//!   Delay(0.033) -> @557: AimSpline visible for a locally controlled owner (`shown`), both SkillShotIndicators
//!   hidden.
//! - ReceiveTick @8048 -> @1724: the aim's steering (`steer`), SetArrowRotationServer, then @3013 the arc (`arc`).
//! - ActivatedLaunch(InputType) @8135: Attack (0) or Special (1) -> @8119 BuffCheck = true, @6327 the launch
//!   (`launch`); Jump (2) or Dodge (3) -> Escape @8896: BuffCheck set -> nothing; else the owner's Buff_JakEat
//!   destroyed (its EndPlay: DetachedFromJakeBuffCheck destroys the aim too) and K2_DestroyActor @9286.
//!   ActivateObject (Comp_Spawner ActivateActors, SpitTimeEnd) -> ActivateServer @8891 -> @8119: the launch.
//! - ReceiveEndPlay @7242: MoveBuff and IsMoving are written by nothing: no effect (`moving` stays false).
// Unverified: InputPreTriggered (the head of each press logic, BaseFighter @126257 attack, @136859 special,
// @122732 jump, @123338 / @188521 dodge, ahead of every state test) is read here as the ally's buttons' rising
// edges at the aim's tick, in the order attack, special, jump, dodge (the first one acts), as Iron Giant's turret
// reads its ally; a bot ally's presses count.
// Deviation: the Delays (0.3, 0.033) and the DetachedFromJakeBuffCheck broadcast are taken at the aim's tick,
// not in the frame's latent pass or inside the buff's EndPlay.
// Unverified: AllySpitMontageMulticast @8379: CreateProxyObjectForPlayMontage(the attach parent's (the ally's)
// GetComponentByClass(SkeletalMeshComponent), Jake_Eat_Attack_Spit_Ally_Montage, 1, 0) @8450: the montage is of
// Jake's skeleton, and Montage_PlayInternal plays nothing on a mesh of another skeleton: it plays on an ally that is a
// Jake, ahead of the launch's hit reaction in the same call (`spit_montage`). GetComponentByClass(SkeletalMeshComponent)
// is taken as the main mesh (as JerryRocket's LaunchTomMulticast), every Jake skin's mesh taken to be of the
// montage's skeleton, and the play started as a graph's play is (`start_montage`, beginAttack false).
//
// JakeSpitAllyHitBox (SpawnedActorParent_C child), attached to the ally's root, HItBox a sphere of radius 100 at
// the root, InitialSpeed / MaxSpeed = the aim's ArrowAimVector Y / Z. BeginPlay @2075 -> the parent's @443,
// Delay(0.1) -> @30: the ally's HitstunHandler.HitStunEnded -> BreakHS @2085 (the ally has a FighterMovementComponent:
// destroy); authority SetHitBoxServer(the ally) @534: HitBoxDamage with KnockBackDirectionVector (InitialSpeed,
// MaxSpeed), IgnoreActor the ally, HitEachActorOnlyOnce, then @508 HitBoxOn? = true, UpdateHitbox; the ally's
// DamageManager.KnockedBack -> HitOut (destroy); Delay(0.75) -> @428 destroy. InitialLifeSpan 2 (never reached).
// Unverified: VelocityInsteadOfFacing (the packet's) takes the actor's velocity: an actor attached to the ally
// whose ProjectileMovement is taken not to move it (GetVelocity of an attached actor does not read its parent), so a
// zero velocity, and pfgDoDamage keeps the packet's direction (IgnoreFacing: X is UE +Y, sim -x) and BaseKnockBack.
// Deviation: HitStunEnded and KnockedBack are sampled at its tick (the ally's hitstun ending; `hitstun_count`
// moving for a hitstun start or extension), as house.rs samples HitStunStart.
// Deviation: the sphere against the other spawned actors and the team mates (NotifyAllyOfHit) is not tested.

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, end_buff};
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks, refused_ally_hit};
use crate::combat::packet::{DataEnum, DataVec, VacuumInfo};
use crate::combat::phases::{alive, give_vbuf, victim_pose};
use crate::consts as C;
use crate::fighter::anim::montages::start_montage;
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over, force_attack};
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, actor_y, in_hitstun, invulnerable, weight};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, normal, rotate_angle_axis, round_to_int, safe_normal};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{SizeHooks, WorldActorHooks};
use crate::registry::props::{num, obj};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use crate::world::spawning::{add_actor, after_spawned_actor, new_world_actor};
use indexmap::IndexSet;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::fighter::AttackEntry;
use mvs_data::value::Props;

use super::state::{jake_mut, jake_of};

/// JakeEatAlly_Aim's class.
pub const ALLY_AIM: &str = "JakeEatAlly_Aim_C";

/// JakeSpitAllyHitBox's class.
pub const ALLY_HITBOX: &str = "JakeSpitAllyHitBox_C";

const JAK_EAT: Name = name!("Buff_JakEat_C"); // eat.rs's (not imported: eat.rs imports this module)
const SPIT_ALLY_ATTACK: &str = "Jake_Eat_Attack_Spit_Ally_Attack_Montage";
const SPIT_ALLY_MONTAGE: Name = name!("Jake_Eat_Attack_Spit_Ally_Montage");

/// Multiply_FloatFloat(3.0, 9.8) x -100 @4197-4235.
fn gravity() -> Real {
    (3.0 * 9.8) * -100.0
}

/// `JakeEatAllyAim` (JakeEatAlly_Aim_C). The fields the view reads: `x`, `y`, `arc` (the sim x, y per point, flat),
/// `count` (the points), `moving` (IsMoving: written by nothing in the build: false), `ally` (the aim's owner; the
/// build's StoredPawn is Jake: `jake`).
#[derive(State, Debug)]
pub struct JakeEatAllyAim {
    pub x: Real,
    pub y: Real,
    pub arc: Vec<Real>,
    pub count: i32,
    pub moving: bool,
    /// The swallowed ally (the actor's owner).
    pub ally: FighterId,
    /// StoredPawn (Buff_JakEat's Instigator).
    pub jake: FighterId,
    /// ArrowAimVector (UE Y, Z; X is 0 after every write).
    pub aim_y: Real,
    pub aim_z: Real,
    /// BuffCheck.
    pub buff_check: bool,
    /// The BeginPlay Delay(0.3)'s tick: the tick @1095 runs (-1: done).
    pub bind_at: Real,
    /// InputPreTriggered -> ActivatedLaunch.
    pub bound: bool,
    /// The Delay(0.033) -> @557 (-1: none).
    pub shown_at: Real,
    /// AimSpline's visibility (for a locally controlled owner).
    pub shown: bool,
    /// The owner's buttons at the last tick (attack 1, special 2, jump 4, dodge 8).
    pub prev: i32,
    /// LastVectorIndex.
    pub last_index: i32,
    /// AimSpline.SetStartAndEnd @6062 (sim x, y): start, start tangent, end (LastVector), end tangent.
    pub spline: [Real; 8],
    /// HitBoxDamage (KnockBackDirectionVector written by the launch @6997).
    pub hit: HitPacket,
}

/// `JakeSpitAllyHitBox` (JakeSpitAllyHitBox_C): a DealerActor attached to the launched ally.
#[derive(State, Debug, Default)]
pub struct JakeSpitAllyHitBox {
    pub attack_id: i32,
    /// HitBoxDamage.
    pub hit: HitPacket,
    pub x: Real,
    pub y: Real,
    /// HitBoxOn?.
    pub on: bool,
    /// HItBox's at scale 1 (the root's scale is taken at the read).
    pub radius: Real,
    /// The aim's ArrowAimVector Y, Z (the packet's direction vector).
    pub dir_y: Real,
    pub dir_z: Real,
    /// The Delay(0.1) -> @30.
    pub bind_at: Real,
    /// The Delay(0.75) -> @428 (-1: none).
    pub die_at: Real,
    pub bound: bool,
    /// HitStunEnded (sampled).
    pub stun_was: bool,
    /// KnockedBack (sampled: `hitstun_count`).
    pub stun_seq: i32,
    /// The fighters (ids) hit once.
    pub hit_set: IndexSet<i32>,
}

impl Default for JakeEatAllyAim {
    fn default() -> Self {
        // (the placeholder fighter ids: a record's load overwrites them; `Id` has no Default)
        JakeEatAllyAim {
            x: 0.0,
            y: 0.0,
            arc: Vec::new(),
            count: 0,
            moving: false,
            ally: FighterId::new(0, 0),
            jake: FighterId::new(0, 0),
            aim_y: 0.0,
            aim_z: 0.0,
            buff_check: false,
            bind_at: 0.0,
            bound: false,
            shown_at: -1.0,
            shown: false,
            prev: 0,
            last_index: 0,
            spline: [0.0; 8],
            hit: HitPacket::default(),
        }
    }
}

/// The JakeEatAlly_Aim_C's place (its `x`, `y`).
pub fn aim_place(_w: &World, _a: &WorldActor, s: &JakeEatAllyAim) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeSpitAllyHitBox_C's place (its `x`, `y`).
pub fn hit_box_place(_w: &World, _a: &WorldActor, s: &JakeSpitAllyHitBox) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeSpitAllyHitBox_C as a dealer (`DealerActor`): its place, facing (1) and AttackID.
pub fn dealer_parts(_w: &World, _a: &WorldActor, s: &JakeSpitAllyHitBox) -> (Real, Real, Real, i32) {
    (s.x, s.y, 1.0, s.attack_id)
}

/// The JakeSpitAllyHitBox_C's hit packet (`hit`).
pub fn hit(s: &JakeSpitAllyHitBox) -> &HitPacket {
    &s.hit
}

fn aim_of(w: &World, id: ActorId) -> Option<&JakeEatAllyAim> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JakeEatAllyAim(s) => Some(s),
        _ => None,
    }
}

fn aim_mut(w: &mut World, id: ActorId) -> Option<&mut JakeEatAllyAim> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JakeEatAllyAim(s) => Some(s),
        _ => None,
    }
}

fn hit_box_of(w: &World, id: ActorId) -> Option<&JakeSpitAllyHitBox> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JakeSpitAllyHitBox(s) => Some(s),
        _ => None,
    }
}

fn hit_box_mut(w: &mut World, id: ActorId) -> Option<&mut JakeSpitAllyHitBox> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JakeSpitAllyHitBox(s) => Some(s),
        _ => None,
    }
}

fn mark_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// GetDirectionUnitVector: GetSafeNormal(to - from) (InvSqrt; zero under 1e-8 squared).
fn dir(x0: Real, y0: Real, x1: Real, y1: Real) -> (Real, Real) {
    let [_, ux, uy] = safe_normal([0.0, x1 - x0, y1 - y0], SMALL_NUMBER);
    (ux, uy)
}

/// FindClosestEnemyAimVector (fn @0-1641): over every BaseFighter, by GetDistanceTo strictly below the best (from
/// 1e6), of another team than the aim's (the ally's) and without Buff_JakEat; the direction to its location raised by
/// Lerp(200, 2000, clamp01(NormalizeToRange(distance, 800, 3500))). UE (Y, Z), or none.
fn closest_enemy_aim(w: &World, a: &JakeEatAllyAim) -> Option<(Real, Real)> {
    let mut best: Real = 1_000_000.0;
    let mut found: Option<FighterId> = None;
    for o in match_fighters(w) {
        if !alive(w, o) {
            continue;
        }
        let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
        let d = ((ox - a.x) * (ox - a.x) + (oy - a.y) * (oy - a.y)).sqrt();
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(d < best): NaN refuses")]
        if !(d < best) || w.fighter(o).team.team == w.fighter(a.ally).team.team {
            continue;
        }
        if buff_of(w, o, JAK_EAT).is_some() {
            continue;
        }
        best = d;
        found = Some(o);
    }
    let f = found?;
    let k = min(max((best - 800.0) / (3500.0 - 800.0), 0.0), 1.0);
    let lift = 200.0 + ((2000.0 - 200.0) * k);
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let (ux, uy) = dir(a.x, a.y, fx, fy + lift);
    Some(((-ux), uy))
}

/// FindNearAngleDirection(Angle1, Angle2) (fn @0-776): the shortest of the three differences, ReturnMult 1 below 0
/// else -1, AngleDifrence Round of it: (mult, diff).
fn near_angle(a1: Real, a2: Real) -> (Real, Real) {
    let s0 = (a1 + 180.0) - (a2 + 180.0);
    let s1 = s0 - 360.0;
    let s2 = s0 + 360.0;
    let sel = if s0.abs() < s1.abs() { s0 } else { s1 };
    let sel2 = if sel.abs() < s2.abs() { sel } else { s2 };
    (if sel2 < 0.0 { 1.0 } else { -1.0 }, crate::math::real(round_to_int(sel2)))
}

/// @1724-2930: the owner's BPI_GetMoveAxisValues (MoveX above 0 the stick right, MoveY above 0 up); target =
/// DegAtan2(MoveX sign, -MoveY sign); the arrow's angle DegAtan2(Y, Z); RotateAngleAxis about X by AimSpeed x DT x
/// ReturnMult, held when |AngleDifrence| >= 179 or the stick is centred; X zeroed.
fn steer(w: &World, p: &Props, ally: FighterId, aim: &mut (Real, Real)) {
    let i = w.fighter(ally).controls.inp;
    let (mx, my) = (i.x, i.y);
    let sf1 = if my == 0.0 {
        0.0
    } else if my > 0.0 {
        -1.0
    } else {
        1.0
    };
    let sf3 = if mx == 0.0 {
        0.0
    } else if mx > 0.0 {
        1.0
    } else {
        -1.0
    };
    let target = deg_atan2(sf3, sf1);
    let cur = deg_atan2(aim.0, aim.1);
    let (mult, diff) = near_angle(cur, target);
    if diff.abs() >= 179.0 || (sf1 == 0.0 && sf3 == 0.0) {
        return;
    }
    let turn = (num(p, "AimSpeed").unwrap_or(175.0) * C::DT) * mult;
    let [_, y, z] = rotate_angle_axis([0.0, aim.0, aim.1], turn, [1.0, 0.0, 0.0]);
    *aim = (y, z);
}

/// @3013-6062: the launch velocity Normal((0, Y, Z)) x the knockback of the aim's HitBoxDamage at the owner's
/// Comp_Actor_Damage (CurrentDamage, Weight); the path from the actor's location (radius 50, channel 2, SimFrequency
/// 15, MaxSimTime 2, gravity -2940); ArcLength summed from the actor's location to the first point at or past
/// AimDistance (LastVectorIndex), LastVector pulled back along that segment by the overshoot; AimSpline.SetStartAndEnd.
/// The view's alone.
fn arc(w: &mut World, p: &Props, id: ActorId) {
    let Some(a) = aim_of(w, id) else { return };
    let (o, ax, ay, aim_y, aim_z, mut last_index, mut spline) =
        (a.ally, a.x, a.y, a.aim_y, a.aim_z, a.last_index, a.spline);
    let (hdmg, hscale, hbase) =
        (a.hit.damage.unwrap_or(0.0), a.hit.knock_back_scaling.unwrap_or(0.0), a.hit.base_knock_back.unwrap_or(0.0));
    let cd = w.fighter(o).stats.damage;
    let dmg = hdmg;
    let [_, ny, nz] = normal([0.0, aim_y, aim_z], 1e-4);
    let curve = ((cd * cd) * num(p, "KnockBackExpCurveMult").unwrap_or(0.0)) + 1.0;
    let wdiv = 200.0 / (weight(w, o) + 100.0);
    let base = (dmg / 10.0) + ((dmg * cd) / 20.0);
    let kb = curve * (((((base * wdiv) * 1.4) + 18.0) * hscale) + hbase);
    let mut pts = std::mem::take(&mut aim_mut_unchecked(w, id).arc);
    // sim x = -UE Y: linear in X
    let n = crate::actors::predict_path::predict_projectile_path(
        w,
        ax,
        ay,
        -ny * kb,
        nz * kb,
        15.0,
        2.0,
        gravity(),
        &mut pts,
    );
    let dist = num(p, "AimDistance").unwrap_or(1100.0);
    let mut len: Real = 0.0;
    for i in 0..n {
        let (px, py) = if i == 0 { (ax, ay) } else { (pts[2 * i - 2], pts[2 * i - 1]) };
        let (cx, cy) = (pts[2 * i], pts[2 * i + 1]);
        len += ((cx - px) * (cx - px) + (cy - py) * (cy - py)).sqrt();
        if len < dist {
            continue;
        }
        last_index = i32::try_from(i).expect("the arc's index fits");
        // At i 0 the Array_Get(-1) gives an empty vector (the stock out-of-range answer).
        let (qx, qy) = if i > 0 { (pts[2 * i - 2], pts[2 * i - 1]) } else { (0.0, 0.0) };
        let (ux, uy) = dir(qx, qy, cx, cy);
        let over = len - dist;
        spline[4] = qx - (ux * over);
        spline[5] = qy - (uy * over);
        break;
    }
    let (sx, sy) = dir(ax, ay, pts[4], pts[5]);
    spline[0] = ax + (sx * 100.0);
    spline[1] = ay + (sy * 100.0);
    spline[2] = sx * dist;
    spline[3] = sy * dist;
    let j = usize::try_from(last_index + 1).expect("a non-negative index");
    let (ex, ey) = if j < n { dir(spline[4], spline[5], pts[2 * j], pts[2 * j + 1]) } else { (0.0, 0.0) };
    spline[6] = ex * dist;
    spline[7] = ey * dist;
    let a = aim_mut_unchecked(w, id);
    a.arc = pts;
    a.count = i32::try_from(n).expect("the arc's point count fits");
    a.last_index = last_index;
    a.spline = spline;
}

/// The live aim's state (the caller has just read it).
fn aim_mut_unchecked(w: &mut World, id: ActorId) -> &mut JakeEatAllyAim {
    aim_mut(w, id).expect("a live aim")
}

/// AllySpitMontageMulticast @8379 on the ally (the header).
fn spit_montage(w: &mut World, o: FighterId) {
    if jake_of(w, o).is_none() {
        return;
    }
    start_montage(w, o, SPIT_ALLY_MONTAGE, &PROXY_NONE, None);
    w.fighter_mut(o).charge.begin_attack = false;
}

/// @6327: the owner's Buff_JakEat (none: nothing, the aim stays) destroyed, AllySpitMontageMulticast @6588 (above), the
/// owner's Comp_Spawner CompSpawnActor(JakeSpitAllyHitBox, attach, Instigator the owner, InitialSpeed Y, MaxSpeed Z)
/// @6739, HitBoxDamage.KnockBackDirectionVector = (Y, Z) @6997, StoredPawn's (Jake's) HitBoxHandler pfgDoDamage(null,
/// the owner, HitBoxDamage, no rows) @7104 (`refused_ally_hit`: the team mate's ForceKnockback reaction),
/// K2_DestroyActor @7227.
fn launch(w: &mut World, id: ActorId) {
    let Some(a) = aim_mut(w, id) else { return };
    a.buff_check = true; // @8119
    let (o, jake, aim_z, aim_y) = (a.ally, a.jake, a.aim_z, a.aim_y);
    let Some(b) = buff_of(w, o, JAK_EAT) else { return };
    // The buff's EndPlay (eat_buffs.rs) runs Comp_JakeEat RemoveHeldFighter @964: a team mate gets Buff_ArmorPack
    // (ArmorStruct BuffArmorPack: 10, 2 hits) @7773 here, ahead of the pfgDoDamage below; its CalculateArmor @7560
    // skips the packs for a dealer of the victim's team (Comp_Armor @237), so the ally flies and keeps the pack.
    end_buff(w, o, b);
    spit_montage(w, o);
    // the hit box's ProjectileMovement speeds as Comp_Spawner's BPI_SetInitialMovement @8407 writes them from the
    // CompSpawnActor @6739 (direction (0,0,0), no momentum): InitialSpeed 0, MaxSpeed max(0, Z)
    spawn_hit_box(w, o, 0.0, max(0.0, aim_z));
    let Some(a) = aim_mut(w, id) else { return };
    a.hit.knock_back_direction_vector = Some(DataVec { x: Some(aim_y), y: Some(aim_z), z: None });
    let hit = a.hit.clone();
    refused_ally_hit(w, jake, o, &hit, false, ActorRef::Fighter(jake));
    mark_dead(w, id);
}

/// Escape @8896.
fn escape(w: &mut World, id: ActorId) {
    let Some(a) = aim_of(w, id) else { return };
    if a.buff_check {
        return;
    }
    let o = a.ally;
    if let Some(b) = buff_of(w, o, JAK_EAT) {
        end_buff(w, o, b);
    }
    mark_dead(w, id);
}

fn buttons(w: &World, f: FighterId) -> i32 {
    let i = w.fighter(f).controls.inp;
    i32::from(i.attack)
        | (i32::from(i.special) << 1)
        | (i32::from(i.jump) << 2)
        | (i32::from(i.dodge || i.neutral_evade) << 3)
}

fn aim_step(w: &mut World, id: ActorId) {
    let Some(a) = aim_of(w, id) else { return };
    let ally = a.ally;
    let (x, y) = (w.fighter(ally).body.x, actor_y(w, ally));
    let Some(a) = aim_mut(w, id) else { return };
    a.x = x;
    a.y = y;
    // DetachedFromJakeBuffCheck (the buff's OnEndPlay, bound at BeginPlay).
    if !a.buff_check && buff_of(w, ally, JAK_EAT).is_none() {
        mark_dead(w, id);
        return;
    }
    let t = crate::real(w.flow.tick);
    let cur_buttons = buttons(w, ally);
    let Some(a) = aim_of(w, id) else { return };
    if a.bind_at >= 0.0 && t >= a.bind_at {
        let v = closest_enemy_aim(w, a);
        let Some(a) = aim_mut(w, id) else { return };
        a.bind_at = -1.0;
        a.aim_y = v.map_or(-0.5, |v| v.0); // facingLeft is always false: (0, -0.5, 0.5)
        a.aim_z = v.map_or(0.5, |v| v.1);
        a.bound = true;
        a.prev = cur_buttons;
        a.shown_at = t + latent_frames(0.033) - 1.0;
    }
    let Some(a) = aim_mut(w, id) else { return };
    let mut aim = (a.aim_y, a.aim_z);
    if a.shown_at >= 0.0 && t >= a.shown_at {
        a.shown_at = -1.0;
        a.shown = true;
    }
    if a.bound {
        let pr = cur_buttons & !a.prev;
        a.prev = cur_buttons;
        if pr & 3 != 0 {
            launch(w, id);
            return;
        }
        if pr & 12 != 0 {
            escape(w, id);
            return;
        }
    }
    let p = actor_p(w, ActorRef::Actor(id)).clone();
    steer(w, &p, ally, &mut aim);
    let a = aim_mut_unchecked(w, id);
    (a.aim_y, a.aim_z) = aim;
    arc(w, &p, id);
}

/// ActivateObject -> ActivateServer @8891 -> @8119.
fn aim_activate(w: &mut World, id: ActorId) {
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        launch(w, id);
    }
}

fn aim_end(w: &mut World, id: ActorId) {
    let Some(a) = aim_of(w, id) else { return };
    let jake = a.jake;
    if let Some(j) = jake_mut(w, jake)
        && j.ally_aim == Some(ActorRef::Actor(id))
    {
        j.ally_aim = None;
    }
}

/// `spawnAllyAim`: Comp_JakeEat @5678: the ally's Comp_Spawner CompSpawnActor(JakeEatAlly_Aim, attach, Instigator
/// Jake). The spawner's NumberOfActorsAllowed (1) ends the ally's older ones; BeginPlay without Buff_JakEat on the ally
/// destroys it at once. Caller: `add_held_fighters` (eat.rs), from Buff_JakEat's attach check (the buff is on the
/// ally).
pub fn spawn_ally_aim(w: &mut World, f: FighterId, ally: FighterId) {
    let data = w.data.clone();
    let Some(cls) = w.names.get(ALLY_AIM) else { return };
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(ally), cls) else {
        warn_text(w, &format!("{ALLY_AIM} not exported"));
        return;
    };
    let v = obj(&d.p, "ArrowAimVector");
    let none = Props::new();
    let (ax, ay) = (v.map_or(1.0, |v| num(v, "Y").unwrap_or(0.0)), v.map_or(0.0, |v| num(v, "Z").unwrap_or(0.0)));
    let s = JakeEatAllyAim {
        ally,
        jake: f,
        aim_y: ax,
        aim_z: ay,
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        bind_at: crate::real(w.flow.tick) + latent_frames(0.3) - 1.0,
        x: w.fighter(ally).body.x,
        y: actor_y(w, ally),
        arc: Vec::new(),
        count: 0,
        moving: false,
        buff_check: false,
        bound: false,
        shown_at: -1.0,
        shown: false,
        prev: 0,
        last_index: 0,
        spline: [0.0; 8],
    };
    let dead = buff_of(w, ally, JAK_EAT).is_none(); // @7768 -> @7942
    let actor =
        new_world_actor(w, cls, WorldActorClass::JakeEatAllyAim, ally, dead, WorldActorState::JakeEatAllyAim(s));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, ally);
    add_actor(w, id, charge, true);
    if !dead && let Some(j) = jake_mut(w, f) {
        j.ally_aim = Some(ActorRef::Actor(id));
    }
    after_spawned_actor(w, ally, ActorRef::Actor(id)); // the ally's CompSpawnActor: HandleActorPostSpawn @12116
}

/// `dropAllyClient`: Comp_JakeEat DropAllyClient(Jake) @12106: Target cast to BaseFighter, its BPI_ForceAttack @12394
/// with {RequiredButtonInput 1, RequiredDirectionInput 4, Jake_Eat_Attack_Spit_Ally_Attack_Montage, AbilitySlot -1,
/// AllowPreAttackRotation, no conditions}; then goto 3573, the BackupCheck loop (RemoveHeldFighter for a held fighter
/// without Buff_JakEat): every held ally still has its buff here (`drop_ally_server` destroys it after this call),
/// and an enemy without one is the component tick's (eat.rs), so nothing changes. Caller: `drop_ally_server` @3410
/// (eat.rs), ahead of its destroy of the ally's Buff_JakEat @3433 (whose EndPlay ends the aim:
/// DetachedFromJakeBuffCheck).
pub fn drop_ally_client(w: &mut World, f: FighterId) {
    let row = AttackEntry {
        required_button_input: 1,
        required_direction_input: 4,
        attack_montage: SPIT_ALLY_ATTACK.to_string(),
        ability_slot: Some(-1),
        display_name: None,
        conditions: Vec::new(),
        allow_pre_attack_rotation: true,
    };
    force_attack(w, f, &row);
}

/// `activateAllyAim`: the swallowed ally's Comp_Spawner ActivateActors(JakeEatAlly_Aim) (Comp_JakeEat SpitTimeEnd
/// @1835, the eat timer's end with an ally held): every live aim of the ally (one: NumberOfActorsAllowed). Caller:
/// `spit_time_end` (eat.rs).
pub fn activate_ally_aim(w: &mut World, _f: FighterId, ally: FighterId) {
    let Some(cls) = w.names.get(ALLY_AIM) else { return };
    let mut i = 0;
    while let Some(id) = w.actors.actors.nth(i) {
        i += 1;
        let x = w.actors.actors.get(id).expect("a live actor id");
        if x.cls == cls && x.owner == ally && !x.dead {
            launch(w, id);
        }
    }
}

// ------------------------------------------------------------------ JakeSpitAllyHitBox

fn spawn_hit_box(w: &mut World, ally: FighterId, y: Real, z: Real) {
    let data = w.data.clone();
    let Some(cls) = w.names.get(ALLY_HITBOX) else { return };
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(ally), cls) else {
        warn_text(w, &format!("{ALLY_HITBOX} not exported"));
        return;
    };
    let none = Props::new();
    let s = JakeSpitAllyHitBox {
        attack_id: num(&d.p, "AttackID").map_or(-1, to_int32),
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        radius: obj(&d.components, "HItBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(100.0),
        dir_y: y,
        dir_z: z,
        bind_at: crate::real(w.flow.tick) + latent_frames(0.1) - 1.0,
        x: w.fighter(ally).body.x,
        y: actor_y(w, ally),
        on: false,
        die_at: -1.0,
        bound: false,
        stun_was: false,
        stun_seq: 0,
        hit_set: IndexSet::new(),
    };
    let actor = new_world_actor(
        w,
        cls,
        WorldActorClass::JakeSpitAllyHitBox,
        ally,
        false,
        WorldActorState::JakeSpitAllyHitBox(s),
    );
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, ally);
    add_actor(w, id, charge, true);
    after_spawned_actor(w, ally, ActorRef::Actor(id)); // the ally's CompSpawnActor: HandleActorPostSpawn @12116
}

/// SetHitBoxServer @534-2069: the packet the hit box carries from the bind on.
fn bound_packet(mut hit: HitPacket, dir_y: Real, dir_z: Real) -> HitPacket {
    hit.knock_back_direction_vector = Some(DataVec { x: Some(dir_y), y: Some(dir_z), z: None });
    hit.ignore_actor = Some(true);
    hit.destroy_projectiles = Some(false);
    hit.reflect_projectiles = Some(false);
    hit.vacuum_knockback = Some(false);
    hit.direct_knockback_towards_socket = Some(false);
    hit.knockback_target_socket = Some(name!("None"));
    hit.knockback_socket_offset = Some(DataVec { x: Some(0.0), y: Some(0.0), z: None });
    hit.controller_rumble = None;
    hit.play_on_hit_effects = Some(true);
    hit.apply_hit_pause_to_damage_dealer = Some(true);
    hit.attack_decay_type = Some(DataEnum::Value(0));
    hit.attack_decay_name = Some(name!("None"));
    hit.additive_kb_mode = Some(DataEnum::Value(0));
    hit.kb_scaling_starting_dmg = Some(0.0);
    hit.hit_each_actor_only_once = Some(true);
    hit.account_for_velocity_with_vacuum_knockback = Some(true);
    hit.ignore_kbi = Some(false);
    hit.kbi_multiplier = Some(1.0);
    hit.vacuum_info = Some(VacuumInfo {
        use_vacuum_knock_back: Some(false),
        vacuum_strength_alpha: Some(0.0),
        vacuum_towards_instigating_fighter: Some(false),
        vacuum_target_socket: Some(name!("None")),
        vacuum_socket_offset: Some(DataVec { x: Some(0.0), y: Some(0.0), z: None }),
        vacuum_duration: Some(0.0),
        vacuum_pause: Some(0.0),
        force_small_hit_effects: Some(false),
        account_for_attacker_velocity: Some(true),
    });
    hit
}

fn hit_box_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let (x, y) = (w.fighter(o).body.x, actor_y(w, o));
    let t = crate::real(w.flow.tick);
    let (hs, seq) = (in_hitstun(w, o), w.fighter(o).attackers.hitstun_count);
    let Some(s) = hit_box_mut(w, id) else { return };
    s.x = x;
    s.y = y;
    if s.bound {
        // BreakHS (HitStunEnded) / HitOut (KnockedBack), bound at @30 / @332.
        if (s.stun_was && !hs) || seq != s.stun_seq {
            mark_dead(w, id);
            return;
        }
        s.stun_was = hs;
    }
    if s.bind_at >= 0.0 && t >= s.bind_at {
        s.bind_at = -1.0;
        s.bound = true;
        s.stun_was = hs;
        s.stun_seq = seq;
        let hit = std::mem::take(&mut s.hit);
        s.hit = bound_packet(hit, s.dir_y, s.dir_z);
        s.on = true;
        s.die_at = t + latent_frames(0.75) - 1.0;
    }
    if s.die_at >= 0.0 && t >= s.die_at {
        mark_dead(w, id);
    }
}

/// HItBox (object type HitBox) where it is now, none while it is off.
fn hit_box_sphere(w: &World, id: ActorId) -> Option<WorldSphere> {
    let (a, s) = (w.actors.actors.get(id)?, hit_box_of(w, id)?);
    if !s.on || a.dead {
        return None;
    }
    Some(WorldSphere {
        x: s.x,
        y: s.y,
        z: Some(0.0),
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

/// The HItBox sphere against the enemies' hurt shapes, the handler's pfgDoDamage gates in the projectile test's order
/// (as house.rs); IgnoreActor the ally. The direction: see VelocityInsteadOfFacing above.
fn hit_box_hits(w: &mut World, id: ActorId) {
    let Some(sphere) = hit_box_sphere(w, id) else { return };
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let Some(s) = hit_box_of(w, id) else { return };
    let (attack_id, hit) = (s.attack_id, s.hit.clone());
    let team = w.fighter(o).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = hit_box_of(w, id) else { return };
        if vic == o
            || w.fighter(vic).team.team == team
            || !alive(w, vic)
            || crate::combat::phases::no_hit_by_team(w, team, vic)
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
        if let Some(s) = hit_box_mut(w, id) {
            s.hit_set.insert(vid);
        }
        let d = hit.knock_back_direction_vector.unwrap_or_default();
        let dv = V2 { x: -d.x.unwrap_or(0.0), y: d.y.unwrap_or(0.0) };
        let dl = dealer(w, me);
        if apply_projectile_hit(
            w,
            &dl,
            vic,
            &hit,
            &[],
            V2 { x: sphere.x, y: sphere.y },
            Some(dv),
            &[],
            charge_pct(w, me),
        ) {
            actor_hit_enemy(w, &dl, vic);
        }
    }
}

/// This file's registry entries (JakeEatAlly_Aim_C, JakeSpitAllyHitBox_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        ALLY_AIM,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeEatAllyAim),
            ActorHooks::World(WorldActorHooks {
                // no shape: the arc is a world path. Deviation: the AimSpline's mesh keeps its width at a root scale
                // other than 1, as the spit aimer's
                size: Some(SizeHooks::default()),
                step: Some(aim_step),
                activate: Some(aim_activate),
                end: Some(aim_end),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        ALLY_HITBOX,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeSpitAllyHitBox),
            ActorHooks::World(WorldActorHooks {
                // the hit shapes read the scale; nothing drawn
                size: Some(SizeHooks::default()),
                hit_shapes: Some(hit_box_shapes),
                step: Some(hit_box_step),
                hits: Some(hit_box_hits),
                ..WorldActorHooks::default()
            }),
        ),
    )
}
