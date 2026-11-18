//! Velma's evidence: the pawn's SpawnEvidence timer, Buff_EvidenceCount
//! ([`CountExt`]) and Buff_PoliceEvidence, PoliceCondition, VelmaEvidence and VelmaEvidenceNoGrav ([`Evidence`], a
//! world actor with a capsule body: the clue her team picks up); joined to timerTick. Its exports are called by
//! police.rs (`reset_evidence`), beam.rs and perks.rs (`spawn_evidence_actor`, `range_f`, `increment_evidence`,
//! `evidence_count`).
//!
//! The pawn's timer (BeginPlay @1283-1374, authority): K2_SetTimerDelegate(SpawnEvidence, RandomFloatInRange(30, 45),
//! looping) @1374: one period drawn for the pawn's life, SpawnEvidence on every expiry. Deviation: the draw is taken in
//! the pawn's first timer pass (`timer_tick`), not inside its BeginPlay: its place in the world's stream moves behind
//! the first frame's other draws (the timer counts from that pass, as a timer set in the tick groups does:
//! `timer_ticks`). A match reset (a new pawn) draws again.
//! SpawnEvidence @1983-2316: GetActorOfClass(MapRandomActorSpawnArea) @1983, FindRandomSpawnPoint @2013 (three draws, X,
//! Y, Z), BreakTransform, MakeVector(0, Y, Z), MakeTransform(that, rot(0, 90, 0), scale 1) @2214, then
//! Comp_Spawner.CompSpawnActor(VelmaEvidenceNoGrav_C, no attach, socket None, WorldTransforms, that transform,
//! direction (0, 0, 0.2), InitialSpeed 0, MaxSpeed 0, ...) @2316. A stage with no area spawns nothing here, warned.
//!
//! Buff_EvidenceCount (the pawn's passive buff: BuffSelf, Duration 0, StartingStacks 0): BeginPlay @79 (authority): the
//! owner's BPI_ResetPoliceUIWidget (UI) and EvidenceCountVisuals(EvidenceCount = 0). IncrementEvidenceCount @1058: 0 <=
//! count < 5 (a literal 5, not MaxEvidence) -> count + 1 @1243; count >= MaxEvidence (5) @1270 -> count = MaxEvidence,
//! visuals, then with AllowPoliceCall @1372: the owner's SpawnBuffActor(Buff_PoliceEvidence_C, the owner, the buff's
//! Instigator, 1 stack, 0.0 s, BreakIfBlocked true, false) @1473 (a 0.0 duration: no life timer, so it stays until the
//! police call destroys it, police.rs @5166-5333), then for an owner whose BPI_GetFighterCharacter is 19 (Velma):
//! BPI_CallPolice(true) @1857 (the pawn's input-label widget alone: no sim part), AllowPoliceCall = false @1895, VO
//! ability_police_ready (1.0) @1926, Delay(1.0) @2002 -> AllowPoliceCall = true @15. Another owner stops at @1748. Else:
//! EvidenceCountVisuals(the new count) @2057. EvidenceCountVisuals @2552: a fighter owner; 0 < count ->
//! BPI_SetEvidenceCount(count / 5.0) @2921, else (0.0) @3043. ResetEvidenceCountSever @3119 -> @3086: count = 0,
//! ResetEvidenceCountMulticast(0) @51 (count = 0 @3124), visuals(0). EndPlay @2106: count = 0, visuals, the UI bar at
//! 0, BPI_ResetPoliceUIWidget. Buff_PoliceEvidence has no graph of its own; PoliceCondition.Check:
//! CheckHasBuff(Buff_PoliceEvidence_C) @51.
//!
//! VelmaEvidence: Capsule r 50, half height 90, mass 145, LinearDamping 0.5, AngularDamping 1, InertiaTensorScale 1e5, X
//! translation and Y / Z rotation locked, Custom profile overlapping Pawn, HitBox and WorldDynamic and blocking the stage
//! and the platforms (`mvs-collision` `capsule_step`); Destructible: a sphere r 95 on the Capsule, object type HurtBox
//! (GameTraceChannel17), overlapping HitBox alone. Collectable_Timer 0.75; PerkTempHealth (Evidence, 3, CanOverride, 3
//! s). Comp_Pawn_DamageManager CanTakeDamage false. Die With Instigator (the world's), DestroyOnExitedArena? (the
//! parent's 0.1 s timer). VelmaEvidenceNoGrav: the same with bSimulatePhysics false. BeginPlay @3630 (authority):
//! SetPhysicsLinearVelocity((0, RandomFloatInRange(-200, 200), 800)) @3769, SetPhysicsAngularVelocityInDegrees((
//! RandomFloatInRange(-200, 200), 0, 0)) @3927 (both drawn for either class, applied to a simulating body alone), Init
//! Visuals @3996 (sfx_cha_velma_clue_appear on the owner @4154, PS_ShadowClone_Impact, VO ability_clue 0.2 @4637),
//! Delay(Collectable_Timer) @4010 -> @31. Comp_Spawner HandleActorPostSpawn @10992-11329: AddImpulse(
//! LocalInitialVelocity + the spawner's owner's GetVelocity / 2, velocity change), after BeginPlay; LocalInitialVelocity
//! @5559-6424 = the direction x InitialSpeed, mirrored (-1, -1, 1) for an owner facing right. @31: CanPickUp = true;
//! then for each actor of Capsule.GetOverlappingActors(BP_FighterCharacter) of the owner's team InitialPickUp(it) @862
//! (-> @6810 PickingUpFighter, @5503 the pickup below). The pickup ends the actor, so the loop ends at the first. Then
//! Delay(5.0) @886 -> @941: Geometric false (ActivateGeometric @6960 has no caller) -> TimeOutVisuals
//! (PS_ShadowClone_Impact, sfx_cha_velma_clue_destroyed on the owner @6400) and K2_DestroyActor @970. Capsule begin
//! overlap @6745 (authority) -> @4706: the owner and the other actor are BP_FighterCharacters, CanPickUp, both have
//! player data, the same team -> PickingUpFighter @5484 and @5503: PickupVisuals, the Instigator's Buff_EvidenceCount ->
//! BPI_IncrementEvidenceCount @2617; a picker that is not the Instigator, the Instigator holding Buff_KnowledgeIsPower
//! @2986 and the picker a Comp_Armor -> AddTempHealthPack(PerkTempHealth) @3302; ReleaseToPool @3347. Destructible
//! begin overlap @7024 -> @3362 (authority): the other actor's Comp_Actor_Destructible of another TeamIndex ->
//! DestroySpawnedActor(false) @15. ReceiveAnyDamage @6834: ReleaseToPool and sfx_cha_velma_clue_destroyed.
//! (CanTakeDamage false: the world's damage path makes no such call; the entry stands for the build's graph.)

// Deviation: the pickup tests the fighters' root capsules (Pawn) against the Capsule, as segments in the play plane, once
//   a frame behind the body's step. The Capsule is WorldDynamic (Custom with no saved ObjectType: the shape's), so a
//   team-mate's hit shapes of the HitBox profile (WorldDynamic Ignore, DefaultEngine.ini 569) take no part;
//   BaseFighter's own Capsule (OverlapAllDynamic: it overlaps WorldDynamic) and an OverlapAllDynamic hit shape
//   (Pawn_C023B's ChainSawHitBox) would begin an overlap with it too and are not tested.
// Unverified: GetOverlappingActors' order (InitialPickUp's loop) is taken as the world's fighter order.
// Unverified: a minion fighter (notPlayerCharacter: the police) is taken to have no player data (GetByFighter fails): it
//   neither picks up nor is a team-mate here.
// Deviation: the Destructible sphere meets the enemy fighters' active hit shapes (HurtBox channel, the World's hit
//   spheres) alone; an enemy projectile's or item's hit box (another actor with a Comp_Actor_Destructible) does not end
//   it here.
// The Destructible sphere is a body of the traces (`evidence_bodies`): a dealer's hit box that begins to overlap it
//   runs its handler's pfgDoDamage on the clue, as on any HitActor: a projectile's or a thrown item's hit body
//   (`ActorEntry::spawned_meet_bodies`) and the sass bubbles' Box (`chars::lebron::bubble::box_hits`), both with
//   HurtBox among the types they ask (`combat::spawned_hits::actor_body_types`). Comp_Pawn_DamageManager's
//   CanTakeDamage false makes DamageAndLaunch refuse every one, so what lands is the entry rows, the dealer's dealt
//   pass and a team mate's ally rows @5931-6835 (the sass bubbles' Buff_ArmorPack, an actor buff with no hooks).
// Deviation: a fighter's Hitframes do not run pfgDoDamage on the clue (`combat::melee_actors` meets the World.actors'
//   pawn bodies, damage boxes and IsProjectile hurt boxes alone): their entry rows and dealt pass on it are not made.
// Deviation: an actor's hit box (`combat::hits::hit_box_actor_hits`: the laser's, the dogpile's) gathers its
//   seven object types, no HurtBox, so it does not meet the clue either.
// Deviation: the order inside the actor's frame (the body's step, the blast box timer, the overlaps, then the Delays) is
//   the port's.
// Deviation: the spawn's push-out (FindTeleportSpot) takes the Capsule upright against what its WorldDynamic body blocks
//   (`root_body_blockers`); the body's step meets the stage block and the nav blockers.
// Unverified: the EX_RotationConst of the three visuals calls taken as rot(0, 0, 0) (the class's bytecode not dumped).
// Unverified: a call on a None context (PickupVisuals(null)) taken to leave its return value zeroed (FFrame's null-context
//   path, not read).
// Deviation: `SpawnRequest.origin` is read from the request's `OriginX` / `OriginY` properties (the Rust request has
//   no origin field).

use mvs_collision::capsule_overlap::seg_seg_dist2;
use mvs_collision::rigid_sphere::{Body, Solver};
use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass, WorldActorState};
use crate::combat::boxes::{CH_HURT, seg_box_dist2};
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, spawn_buff};
use crate::combat::defense::add_temp_health_pack;
use crate::combat::elem::instigator_charge;
use crate::combat::hits::hitframe_packet;
use crate::combat::phases::hit_spheres;
use crate::combat::{BuffClass, BuffRef};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::{OpenPose, put_pose, sampled_boxes};
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::actor_spawn_ability;
use crate::fighter::match_side::{Foreign, SpawnRequest, post_vo};
use crate::fighter::traces::{TraceBody, body_info, sphere_static};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, facing, no_collision};
use crate::math::ue::{DEG_TO_RAD, RAD_TO_DEG, quat_rotate_vector, rotator_quaternion};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::capsule_settings;
use crate::registry::entries::{ActorHooks, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag_or, num, obj, text};
use crate::registry::{ActorEntry, BuffEntry, Condition, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, spawn_box};
use crate::world::items::{actor_of_class, find_random_spawn_point};
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{CapsuleShapeC, capsule_shape, comp_spawn_actor, new_world_actor, root_body_blockers};
use indexmap::IndexSet;

/// The clue's class.
pub const EVIDENCE: &str = "VelmaEvidence_C";
/// The clue's class, as a name.
pub const EVIDENCE_N: Name = crate::name!("VelmaEvidence_C");
/// Its child without gravity (the pawn's own timer spawns it).
pub const EVIDENCE_NO_GRAV: &str = "VelmaEvidenceNoGrav_C";
const EVIDENCE_NO_GRAV_N: Name = crate::name!("VelmaEvidenceNoGrav_C");
/// The count buff.
pub const EVIDENCE_COUNT: &str = "Buff_EvidenceCount_C";
const EVIDENCE_COUNT_N: Name = crate::name!("Buff_EvidenceCount_C");
/// The buff a full count gives (PoliceCondition reads it).
pub const POLICE_EVIDENCE: &str = "Buff_PoliceEvidence_C";
/// [`POLICE_EVIDENCE`], as a name.
pub const POLICE_EVIDENCE_N: Name = crate::name!("Buff_PoliceEvidence_C");
const KNOWLEDGE_IS_POWER_N: Name = crate::name!("Buff_KnowledgeIsPower_C");
/// PickupVisuals @5833 (drawn without a Team: the class's ownEmitter).
const PICKUP_VFX: Name = crate::name!("PS_ShaggyScoobySnacksPickup");
/// Init Visuals @4277 / TimeOutVisuals @6311.
const IMPACT_VFX: Name = crate::name!("PS_ShadowClone_Impact");

/// `Evidence` (VelmaEvidence_C, VelmaEvidenceNoGrav_C): the class's own fields; the `SimActor` ones are the
/// [`WorldActor`]'s. `x`, `y` are the Capsule's centre, `spin` its roll for the drawing.
#[derive(State, Debug, Default)]
pub struct Evidence {
    /// Capsule.CapsuleRadius.
    pub radius: Real,
    /// Capsule.CapsuleHalfHeight.
    pub half: Real,
    /// Destructible.SphereRadius.
    pub grab_radius: Real,
    /// Capsule bSimulatePhysics.
    pub simulating: bool,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// rad/s about X, the sim's sign.
    pub wc: Real,
    /// rad, the sim's sign.
    pub angle: Real,
    /// The Capsule's roll in degrees, UE's sign.
    pub spin: Real,
    /// CanPickUp.
    pub can_pick_up: bool,
    /// Past Delay(Collectable_Timer): the Delay(5.0) runs.
    pub collectable: bool,
    /// Ticks left of the running Delay.
    pub wait: i32,
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The fighters whose root capsule overlaps the Capsule, by id.
    pub over: IndexSet<i32>,
}

/// Buff_EvidenceCount's extension (`CountState`: `{ count, allow, wait, from }`).
#[derive(State, Debug, Default)]
pub struct CountExt {
    /// EvidenceCount.
    pub count: i32,
    /// AllowPoliceCall.
    pub allow: bool,
    /// Ticks left of Delay(1.0) @2002, 0: not running.
    pub wait: i32,
    /// The buff's Instigator.
    pub from: Option<FighterId>,
}

/// `rangeF`: RandomFloatInRange(lo, hi) on the World's stream, float32.
pub fn range_f(w: &mut World, lo: Real, hi: Real) -> Real {
    let r = w.random_float();
    lo + ((hi - lo) * r)
}

// ------------------------------------------------------------------ the pawn's timer and SpawnEvidence

/// Pawn_Velma SpawnEvidence @1983-2316: the evidence from the pawn. Caller: the pawn's timer (below).
fn spawn_evidence(w: &mut World, f: FighterId) {
    let Some(area) = actor_of_class(w) else { return };
    let at = find_random_spawn_point(w, &area);
    spawn_evidence_actor(w, f, EVIDENCE_NO_GRAV_N, -at[1], at[2], 0.0, 0.0, 0.0, 0.2);
}

/// evidence.rs's part of the joined timerTick (before police.rs's and beam.rs's): the pawn's SpawnEvidence timer, in
/// the
/// frame's timer pass.
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b): NaN reads as not yet")]
pub fn timer_tick(w: &mut World, f: FighterId) {
    let Some(s) = super::state::velma_of(w, f) else { return };
    let tick = w.fighter(f).clock.tick;
    if s.evidence.period == 0.0 {
        let period = range_f(w, 30.0, 45.0); // @1336 (the Deviation above: drawn here, not in BeginPlay)
        let s = &mut super::state::velma_w(w, f).evidence;
        s.period = period;
        s.start = tick;
        s.timer_at = s.start + to_int32(crate::combat::hit::knockback::timer_ticks(Some(period)));
        return;
    }
    if tick < s.evidence.timer_at {
        return;
    }
    // looping: the engine re-arms by ExpireTime += Rate, so expiry n is due at the first frame past n x Rate from the
    // set, not n x the first expiry's frames
    let s = &mut super::state::velma_w(w, f).evidence;
    s.fired += 1;
    let due = crate::real(s.fired + 1) * s.period;
    let mut k: i32 = 1;
    while !(crate::consts::clock(crate::real(k)) > f64::from(due)) {
        k += 1;
    }
    s.timer_at = s.start + k;
    spawn_evidence(w, f);
}

// ------------------------------------------------------------------ Buff_EvidenceCount

fn count_of(w: &World, b: BuffRef) -> Option<&CountExt> {
    match &buff(w, b)?.ext {
        BuffExt::EvidenceCount(e) => Some(e),
        _ => None,
    }
}

fn count_mut(w: &mut World, b: BuffRef) -> Option<&mut CountExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::EvidenceCount(e) => Some(e),
        _ => None,
    }
}

/// EvidenceCountVisuals @2552: BPI_SetEvidenceCount on a Velma owner (the cast to BPI_Velma; another owner: nothing).
fn count_visuals(w: &mut World, f: FighterId, count: i32) {
    if let Some(s) = super::state::velma_mut(w, f) {
        s.evidence_ui = if 0 < count { crate::real(count) / 5.0 } else { 0.0 };
    }
}

/// IncrementEvidenceCount @1058 on the owner `f`'s buff `b`.
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b): NaN reads as not yet")]
fn increment(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = count_mut(w, b) else { return };
    if !(e.count < 5 && e.count >= 0) {
        return;
    }
    let n = e.count + 1;
    e.count = n;
    let max_ev = crate::fighter::common::buff_def(w, ActorRef::Fighter(f), EVIDENCE_COUNT_N)
        .and_then(|d| num(d, "MaxEvidence"))
        .unwrap_or(5.0);
    if !(crate::real(n) >= max_ev) {
        count_visuals(w, f, n);
        return;
    }
    // (count = MaxEvidence, stored as the number; MaxEvidence is 5 in the data)
    let max_n = to_int32(max_ev);
    let Some(e) = count_mut(w, b) else { return };
    e.count = max_n;
    let (allow, from) = (e.allow, e.from);
    count_visuals(w, f, max_n);
    if !allow {
        return;
    }
    spawn_buff(w, f, POLICE_EVIDENCE_N, 0.0, Some(1), from, true, Some(f)); // @1473
    if w.fighter(f).ch != name!("velma") {
        return; // @1717: BPI_GetFighterCharacter == 19, Pawn_Velma alone
    }
    // BPI_CallPolice(true) @1857: the input-label widget alone (no sim part).
    let wait = to_int32(latent_frames(1.0));
    if let Some(e) = count_mut(w, b) {
        e.allow = false;
        post_vo_ready(w, f);
        if let Some(e) = count_mut(w, b) {
            e.wait = wait;
        }
    }
}

/// VO ability_police_ready (1.0) @1926.
fn post_vo_ready(w: &mut World, f: FighterId) {
    post_vo(w, f, name!("ability_police_ready"), None, 1.0);
}

/// The live Buff_EvidenceCount of `f` (Comp_Actor_Parent's children that are the class: IsSoftRef, an exact class).
fn count_buff(w: &World, f: FighterId) -> Option<BuffRef> {
    buff_of(w, f, EVIDENCE_COUNT_N)
}

/// Buff_EvidenceCount BPI_IncrementEvidenceCount @3114 -> @2537, `n` times, on f's buff. Callers: VelmaEvidence's
/// pickup (below) and Buff_Studied (perks.rs: its IncrementEvidenceCount call @847, the same function).
pub fn increment_evidence(w: &mut World, f: FighterId, n: i32) {
    for _ in 0..n {
        if let Some(b) = count_buff(w, f) {
            increment(w, f, b);
        }
    }
}

/// `evidenceCount`: the EvidenceCount of f's Buff_EvidenceCount (Buff_Studied's read @781), `None` without the buff.
pub fn evidence_count(w: &World, f: FighterId) -> Option<i32> {
    count_of(w, count_buff(w, f)?).map(|e| e.count)
}

/// `resetEvidence`: Buff_EvidenceCount ResetEvidenceCountSever @3119 -> @3086: count = 0, the multicast @51 / @3124,
/// visuals(0). Caller: the police call @5598 (police.rs).
pub fn reset_evidence(w: &mut World, f: FighterId) {
    let Some(b) = count_buff(w, f) else { return };
    if let Some(e) = count_mut(w, b) {
        e.count = 0;
    }
    count_visuals(w, f, 0);
}

fn count_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    let allow =
        crate::combat::buffs::inst_def_of(w, b).is_none_or(|d| d.get("AllowPoliceCall") != Some(&Value::Bool(false)));
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::EvidenceCount(CountExt { count: 0, allow, wait: 0, from });
    }
    count_visuals(w, f, 0); // @251 (@214 BPI_ResetPoliceUIWidget: UI)
}

fn count_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    if let Some(e) = count_mut(w, b)
        && e.wait > 0
    {
        e.wait -= 1;
        if e.wait == 0 {
            e.allow = true; // @15
        }
    }
    false
}

fn count_end(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some(e) = count_mut(w, b) {
        e.count = 0; // @2220
    }
    count_visuals(w, f, 0); // @2243 (the bar at 0 @2341, BPI_ResetPoliceUIWidget @2499: UI)
}

/// PoliceCondition_C: the owner holds a Buff_PoliceEvidence.
fn police_condition(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, POLICE_EVIDENCE_N).is_some()
}

// ------------------------------------------------------------------ VelmaEvidence, VelmaEvidenceNoGrav

fn ev_of(w: &World, id: ActorId) -> Option<&Evidence> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Evidence(s) => Some(s),
        _ => None,
    }
}

fn ev_mut(w: &mut World, id: ActorId) -> Option<&mut Evidence> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Evidence(s) => Some(s),
        _ => None,
    }
}

/// The VelmaEvidence's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &Evidence) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// `spawnEvidenceActor`: Comp_Spawner CompSpawnActor of an evidence class `cls` for its owner `f` (the Instigator),
/// WorldTransforms, at (x, y) in the sim plane, direction (dx, dy, dz) in UE axes x InitialSpeed `speed`: the push-out
/// (FindTeleportSpot), the parent's BeginPlay loop, BeginPlay, then the world's list (`comp_spawn_actor`: the actor's
/// number, AfterSpawnedActor), HandleActorPostSpawn's impulse already in the BeginPlay's velocity. Callers: the pawn's
/// timer (above), perks.rs (Buff_SpawnEvidence), beam.rs (VelmaHealBeam SpawnEvidence, its own Z). `None` when the
/// class is not in the owner's data.
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
pub fn spawn_evidence_actor(
    w: &mut World,
    f: FighterId,
    cls: Name,
    x: Real,
    y: Real,
    dx: Real,
    dy: Real,
    speed: Real,
    dz: Real,
) -> Option<ActorId> {
    let ap = crate::fighter::common::actor_def(w, ActorRef::Fighter(f), cls).map(|a| a.p.clone());
    let reg: &'static crate::registry::Registry = w.registry;
    let class = match reg.actors.get(w.names.str(cls)).map(|e| e.class) {
        Some(ActorClass::World(c)) => Some(c),
        _ => None,
    };
    let (Some(ap), Some(class)) = (ap, class) else {
        let msg = format!("spawned actor not implemented ({})", w.names.str(cls));
        warn_text(w, &msg);
        return None;
    };
    actor_spawn_ability(w, f, &ap);
    let mut d = Props::new();
    d.insert("X".to_owned(), Value::Num(f64::from(dx)));
    d.insert("Y".to_owned(), Value::Num(f64::from(dy)));
    d.insert("Z".to_owned(), Value::Num(f64::from(dz)));
    let mut p = Props::new();
    p.insert("WorldTransforms".to_owned(), Value::Bool(true));
    p.insert("InitialMovementDirection".to_owned(), Value::Obj(d));
    p.insert("InitialSpeed".to_owned(), Value::Num(f64::from(speed)));
    p.insert("OriginX".to_owned(), Value::Num(f64::from(x)));
    p.insert("OriginY".to_owned(), Value::Num(f64::from(y)));
    let r = SpawnRequest {
        cls,
        n: crate::events::NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: instigator_charge(w, f),
        aim: None,
        props: Some(std::sync::Arc::new(p)),
        instigator: None,
        origin: None,
    };
    let (state, dead) = evidence_spawn(w, f, &r);
    let a = new_world_actor(w, cls, class, f, dead, state);
    let id = w.actors.actors.insert(a);
    comp_spawn_actor(w, id, r.charge, f);
    Some(id)
}

/// The kind's spawn: the state made from the request (the BeginPlay's part that comes ahead of the list).
fn evidence_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let rp = r.props.clone().unwrap_or_default();
    let (ox, oy) = match (num(&rp, "OriginX"), num(&rp, "OriginY")) {
        (Some(x), Some(y)) => (x, y),
        _ => (w.fighter(f).body.x, actor_y(w, f)),
    };
    let data = w.data.clone();
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls);
    let p = a.map(|a| &a.p);
    let cap = a.and_then(|a| obj(&a.components, "Capsule"));
    let mut ev = Evidence {
        radius: cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0),
        half: cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(0.0),
        grab_radius: a
            .and_then(|a| obj(&a.components, "Destructible"))
            .and_then(|d| num(d, "SphereRadius"))
            .unwrap_or(0.0),
        simulating: cap.and_then(|c| obj(c, "BodyInstance")).is_some_and(|b| flag_or(b, "bSimulatePhysics", false)),
        x: ox,
        y: oy,
        wait: to_int32(latent_frames(p.and_then(|p| num(p, "Collectable_Timer")).unwrap_or(0.0))),
        blast_at: f64::from(0.1_f32),
        ..Evidence::default()
    };
    if ev.radius == 0.0 || ev.half == 0.0 || ev.grab_radius == 0.0 {
        let msg = format!("{}: no Capsule or Destructible size in the data", w.names.str(r.cls));
        warn_text(w, &msg);
    }
    if ev.radius != 0.0 {
        // PostActorConstruction's FindTeleportSpot: the Capsule upright against what its body blocks.
        let body = cap.and_then(|c| obj(c, "BodyInstance"));
        let boxes = root_body_blockers(w, body, r.cls, name!("spawn push-out"));
        if let Some(boxes) = boxes {
            let shape = capsule_shape(
                Some(CapsuleShapeC {
                    capsule_radius: cap.and_then(|c| num(c, "CapsuleRadius")),
                    capsule_half_height: cap.and_then(|c| num(c, "CapsuleHalfHeight")),
                }),
                1.0,
                0.0,
                1.0,
            );
            let at = crate::world::spawning::find_teleport_spot_shape_in(boxes.view(), ev.x, ev.y, &shape);
            ev.x = at.x;
            ev.y = at.y;
        }
    }
    let destroy_on_exit = p.is_some_and(|p| flag_or(p, "DestroyOnExitedArena?", false));
    let force = p.is_some_and(|p| flag_or(p, "ForceSpawnPositionInsideBlastBox", false));
    let sb = spawn_box(w);
    let gone = blast_box_loop(&sb, &mut ev.x, &mut ev.y, None, force, &mut ev.blast_bounds, destroy_on_exit);
    if gone {
        return (WorldActorState::Evidence(ev), true); // the parent's BeginPlay loop
    }
    // BeginPlay @3684 / @3842: both draws whatever the body.
    let lin = range_f(w, -200.0, 200.0);
    let ang = range_f(w, -200.0, 200.0);
    if ev.simulating {
        ev.vx = -lin;
        ev.vy = 800.0;
        ev.wc = -ang * DEG_TO_RAD;
        // HandleActorPostSpawn @11329: + LocalInitialVelocity + the owner's velocity / 2 (a velocity change).
        let d = obj(&rp, "InitialMovementDirection");
        let sp = num(&rp, "InitialSpeed").unwrap_or(0.0);
        let (fvx, fvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
        let fc = facing(w, f);
        ev.vx = ev.vx + (d.and_then(|d| num(d, "Y")).unwrap_or(0.0) * sp) * fc + (fvx / 2.0);
        ev.vy = ev.vy + (d.and_then(|d| num(d, "Z")).unwrap_or(0.0) * sp) + (fvy / 2.0);
    }
    // Init Visuals @3996.
    let fid = w.fighter(f).id;
    w.events.push(sound_at(fid, name!("sfx_cha_velma_clue_appear"), ev.x, ev.y));
    impact(w, r.cls, fid, ev.x, ev.y); // @4277
    post_vo(w, f, name!("ability_clue"), None, 0.2);
    (WorldActorState::Evidence(ev), false)
}

fn sound_at(fighter: i32, ev: Name, x: Real, y: Real) -> Event {
    Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None }
}

fn death_event(cls: Name, fighter: i32, vfx: Name, x: Real, y: Real, scale: Real) -> Event {
    Event::ProjectileDeath { cls, fighter, vfx: Some(vfx), x, y, ox: 0.0, rot: [0.0; 3], scale, tint: None, to: None }
}

/// Init Visuals @4277 / TimeOutVisuals @6311: SpawnEmitterAtLocation(PS_ShadowClone_Impact, the Capsule's location, scale
/// 0.75) (the projectileDeath event's Team is the owner's colour index: the view's).
fn impact(w: &mut World, cls: Name, fighter: i32, x: Real, y: Real) {
    w.events.push(death_event(cls, fighter, IMPACT_VFX, x, y, 0.75));
}

fn evidence_scale(w: &mut World, a: ActorId, k: Real, old: Real) {
    if let Some(ev) = ev_mut(w, a) {
        ev.radius *= k / old;
        ev.half *= k / old;
        ev.grab_radius *= k / old;
    }
}

/// The parent's CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Projectile.expiry counts it.
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b): NaN reads as not yet")]
fn left_arena(p: Option<&Props>, a: &mut Evidence) -> bool {
    let on = p.is_some_and(|p| flag_or(p, "DestroyOnExitedArena?", false));
    if !on || !(crate::consts::clock(crate::real(a.age) - 1.0) > a.blast_at) {
        return false;
    }
    a.blast_at += f64::from(0.1_f32);
    check_blast_box_exit(a.x, a.y, &a.blast_bounds)
}

fn evidence_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(owner), cls);
    let set = def.and_then(|d| capsule_settings(w, obj(&d.components, "Capsule"), w.names.str(cls))).map(|c| c.set);
    let floor = sphere_static(w, w.stage.ground);
    let mut plats = std::mem::take(&mut w.scratch.sphere_statics);
    plats.clear();
    let shapes = w.blocker_shapes(BlockKind::Nav);
    for &g in &shapes.boxes {
        plats.push(sphere_static(w, g));
    }
    plats.extend(shapes.polys.iter().map(crate::world::stage_query::PolyBlock::rigid));
    let p = def.map(|d| &d.p);
    let Some(ev) = ev_mut(w, id) else {
        w.scratch.sphere_statics = plats;
        return;
    };
    ev.age += 1;
    if ev.simulating
        && let Some(set) = set
    {
        let mut b = Body {
            x: f64::from(ev.x),
            y: f64::from(ev.y),
            vx: f64::from(ev.vx),
            vy: f64::from(ev.vy),
            radius: f64::from(ev.radius),
            angle: f64::from(ev.angle),
            wc: f64::from(ev.wc),
            half: f64::from(ev.half),
            ..Body::default()
        };
        Solver::default().capsule_step(&mut b, &floor, Some(&plats), DELTA_SECONDS, &set);
        (ev.x, ev.y, ev.vx, ev.vy, ev.angle, ev.wc) = (
            crate::math::narrow(b.x),
            crate::math::narrow(b.y),
            crate::math::narrow(b.vx),
            crate::math::narrow(b.vy),
            crate::math::narrow(b.angle),
            crate::math::narrow(b.wc),
        );
        ev.spin = -ev.angle * RAD_TO_DEG;
    }
    let left = left_arena(p, ev);
    w.scratch.sphere_statics = plats;
    if left || enemy_hit_over(w, id) {
        set_dead(w, id); // DestroySpawnedActor(true) / DestroySpawnedActor(false) @15
        return;
    }
    pickups(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let Some(ev) = ev_mut(w, id) else { return };
    if ev.wait <= 0 {
        return;
    }
    ev.wait -= 1;
    if ev.wait > 0 {
        return;
    }
    if !ev.collectable {
        ev.collectable = true;
        ev.can_pick_up = true; // @31
        initial_pick_up(w, id);
        if !w.actors.actors.get(id).is_some_and(|a| a.dead)
            && let Some(ev) = ev_mut(w, id)
        {
            ev.wait = to_int32(latent_frames(5.0)); // @886
        }
        return;
    }
    // @941: Geometric false -> TimeOutVisuals @956, K2_DestroyActor @970.
    let (x, y) = (ev.x, ev.y);
    let oid = w.fighter(owner).id;
    impact(w, cls, oid, x, y); // @6311
    w.events.push(sound_at(oid, name!("sfx_cha_velma_clue_destroyed"), x, y));
    set_dead(w, id);
}

fn set_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ReceiveAnyDamage @6834: ReleaseToPool (@6880) and sfx_cha_velma_clue_destroyed on the actor (@6894).
fn evidence_any_damage(w: &mut World, id: ActorId, _dmg: Real, _by: ActorRef) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let Some(ev) = ev_of(w, id) else { return };
    let (x, y) = (ev.x, ev.y);
    set_dead(w, id);
    let oid = w.fighter(owner).id;
    w.events.push(sound_at(oid, name!("sfx_cha_velma_clue_destroyed"), x, y));
}

/// The Capsule against a fighter's root capsule (Pawn): both as segments in the play plane.
fn capsule_over(w: &World, a: &Evidence, f: FighterId) -> bool {
    if w.fighter(f).life.dead != 0.0 || no_collision(w, f) {
        return false;
    }
    let l = max(a.half - a.radius, 0.0);
    // the Capsule's Z axis turned by its rotation (roll = the body's angle, FRotator::Quaternion): UE (Y, Z) =
    // (sin, cos) x l, sim x = -Y
    let r = quat_rotate_vector(rotator_quaternion([0.0, 0.0, a.angle * RAD_TO_DEG]), [0.0, 0.0, l]);
    let (ux, uy) = (-r[1], r[2]);
    let r = cap_radius(w, f);
    let s = max(capsule_half(w, f) - r, 0.0);
    let cy = actor_y(w, f);
    let rr = a.radius + r;
    let fx = w.fighter(f).body.x;
    // the shared segment test (segSegDist2)
    seg_seg_dist2(a.x + ux, a.y + uy, a.x - ux, a.y - uy, fx, cy - s, fx, cy + s) <= rr * rr
}

/// A fighter the pickup takes: player data on both sides (no minion), the owner's team.
fn team_mate(w: &World, owner: FighterId, f: FighterId) -> bool {
    !w.fighter(f).life.not_player_character
        && !w.fighter(owner).life.not_player_character
        && w.fighter(f).team.team == w.fighter(owner).team.team
}

/// The Capsule's begin overlaps of the frame (@6745 -> @4706), with CanPickUp: the first team-mate that came in.
fn pickups(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    for f in match_fighters(w) {
        let Some(ev) = ev_of(w, id) else { return };
        let over = capsule_over(w, ev, f);
        let fid = w.fighter(f).id;
        let can = ev.can_pick_up;
        let Some(ev) = ev_mut(w, id) else { return };
        if !over {
            ev.over.shift_remove(&fid);
            continue;
        }
        if !ev.over.insert(fid) {
            continue;
        }
        if !can || !team_mate(w, owner, f) {
            continue;
        }
        pick_up(w, id, f);
        return;
    }
}

/// @31-862: the overlapping fighters of the owner's team, InitialPickUp(the first).
fn initial_pick_up(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    for f in match_fighters(w) {
        let fid = w.fighter(f).id;
        if !ev_of(w, id).is_some_and(|ev| ev.over.contains(&fid)) || !team_mate(w, owner, f) {
            continue;
        }
        pick_up(w, id, f);
        return;
    }
}

/// @5503-3347: the pickup by `f`.
fn pick_up(w: &mut World, id: ActorId, f: FighterId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (inst, cls) = (a.owner, a.cls);
    let Some(ev) = ev_of(w, id) else { return };
    let (x, y) = (ev.x, ev.y);
    let iid = w.fighter(inst).id;
    // PickupVisuals(null) @5503: its EnemyFighter is None, so K2_GetActorLocation on it @5677 returns the zero vector
    // and the sound is posted at the world's origin @5727; the emitter is at the actor's own location @5833.
    w.events.push(sound_at(iid, name!("sfx_cha_velma_clue_pickup"), 0.0, 0.0));
    w.events.push(death_event(cls, iid, PICKUP_VFX, x, y, 1.0));
    increment_evidence(w, inst, 1); // @2617
    if f != inst && buff_of(w, inst, KNOWLEDGE_IS_POWER_N).is_some() {
        let data = w.data.clone();
        let t = actor_data(w, &data, ActorRef::Fighter(inst), cls).and_then(|a| obj(&a.p, "PerkTempHealth"));
        let kind =
            t.and_then(|t| text(t, "TempHealthType")).map_or(name!("None"), |s| crate::registry::props::name(w, s));
        add_temp_health_pack(
            w,
            f,
            kind,
            t.and_then(|t| num(t, "HealthAmount")).unwrap_or(0.0),
            t.is_some_and(|t| flag_or(t, "CanOverride", false)),
            t.and_then(|t| num(t, "Time")).unwrap_or(0.0),
        ); // @3302
    }
    set_dead(w, id); // ReleaseToPool @3347
}

/// A WorldSphere of a hit box against the Destructible sphere at (x, y), radius r (the World's rule).
fn sphere_meets(s: &crate::combat::boxes::WorldSphere, x: Real, y: Real, r: Real) -> bool {
    let rr = s.r + r;
    if s.bx.is_some() {
        return seg_box_dist2(s, x, y, 0.0, x, y, 0.0) <= rr * rr;
    }
    let (ex, ey, ez) = (s.x - x, s.y - y, s.z.unwrap_or(0.0));
    ex * ex + ey * ey + ez * ez <= rr * rr
}

/// The Destructible sphere's begin overlap with an enemy's active hit shape (@7024 -> @3362).
fn enemy_hit_over(w: &mut World, id: ActorId) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let owner = a.owner;
    let Some(ev) = ev_of(w, id) else { return false };
    let (x, y, gr) = (ev.x, ev.y, ev.grab_radius);
    let team = w.fighter(owner).team.team;
    for f in match_fighters(w) {
        let fi = w.fighter(f);
        if fi.life.dead != 0.0 || fi.team.team == team || fi.montages.active_hits.is_empty() || no_collision(w, f) {
            continue;
        }
        let Some(pose) = sampled_boxes(w, f) else { continue };
        let hs: Vec<_> = w
            .fighter(f)
            .montages
            .active_hits
            .values()
            .map(|h| (h.n, h.copy_pose.map(|(anim, time)| OpenPose { anim, time })))
            .collect();
        let mut met = false;
        for (n, copy) in hs {
            let hp = hitframe_packet(w, f, n);
            if hit_spheres(w, f, &hp, &pose, CH_HURT, 0.0, Real::INFINITY, copy)
                .iter()
                .any(|s| sphere_meets(s, x, y, gr))
            {
                met = true;
                break;
            }
        }
        put_pose(w, pose);
        if met {
            return true;
        }
    }
    false
}

/// The Destructible sphere as a body of the traces (`ActorKind.bodies`): at the Capsule's centre (no relative
/// location), its radius at the root's scale (`grab_radius`), round in depth; its template's body: Custom, object
/// type HurtBox (ECC_GameTraceChannel17), query on, Overlap to HitBox alone (no trace channel blocked). What meets it:
/// the object queries for HurtBox whose responder is a HitBox: the projectiles' and items' hit bodies through
/// `spawned_meet_bodies` and the sass bubbles' Box (`actor_body_types`).
fn evidence_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let WorldActorState::Evidence(ev) = &a.state else { return };
    if a.dead || ev.grab_radius.is_nan() || ev.grab_radius <= 0.0 {
        return;
    }
    let comp =
        actor_data(w, &w.data, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "Destructible"));
    let Some(info) = body_info(comp, None).filter(|i| i.on) else { return };
    out.push(TraceBody {
        ax: ev.x,
        ay: ev.y,
        bx: ev.x,
        by: ev.y,
        hx: 0.0,
        hy: 0.0,
        r: ev.grab_radius,
        z: 0.0,
        object_type: info.kind,
        profile: info.profile,
        component: name!("Destructible"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// `evidenceKind`.
fn evidence_hooks() -> WorldActorHooks {
    WorldActorHooks {
        size: Some(SizeHooks { scale: Some(evidence_scale), ..SizeHooks::default() }),
        bodies: Some(evidence_bodies),
        spawn: Some(evidence_spawn),
        step: Some(evidence_step),
        any_damage: Some(evidence_any_damage),
        ..WorldActorHooks::default()
    }
}

/// This file's registry entries: Buff_EvidenceCount_C, Buff_PoliceEvidence_C (no hooks), PoliceCondition_C,
/// VelmaEvidence_C and VelmaEvidenceNoGrav_C (its child: the same kind under the parent's class); a projectile's or an
/// item's hit body meets the clue's bodies (`spawned_meet_bodies`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        EVIDENCE_COUNT,
        BuffEntry::new(
            BuffClass::BuffEvidenceCount,
            BuffHooks {
                begin: Some(count_begin),
                tick: Some(count_tick),
                end: Some(count_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(POLICE_EVIDENCE, BuffEntry::new(BuffClass::BuffPoliceEvidence, BuffHooks::default()))?;
    r.conditions.enter("PoliceCondition_C", Condition::Fn(police_condition))?;
    let mut ev =
        ActorEntry::new(ActorClass::World(WorldActorClass::VelmaEvidence), ActorHooks::World(evidence_hooks()));
    ev.spawned_meet_bodies = true;
    r.actors.enter(EVIDENCE, ev)?;
    let mut no_grav =
        ActorEntry::new(ActorClass::World(WorldActorClass::VelmaEvidenceNoGrav), ActorHooks::World(evidence_hooks()));
    no_grav.parents = &[EVIDENCE];
    no_grav.spawned_meet_bodies = true;
    r.actors.enter(EVIDENCE_NO_GRAV, no_grav)
}
