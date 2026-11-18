//! The dogpile: C015Dogpile_C ([`Dogpile`], a DealerActor), AnimNotify_C015Dogpile
//! and _FightCloudHurtbox, the pawn's DogPileTimer and the fight cloud hurt box, and its parts of the joined matchReset
//! and timerTick.
//!
//! Taz's up special: the fight cloud actor C015Dogpile_C, its two notifies and Pawn_C015's part of it
//! (BPI_SetControllingDogpile @9566, DogPileExpire @17116, BPI_SetRemainingDogpileTime @8572,
//! BPI_ActivateFightCloudHurtBox @9604-9871, the construction script's call of the latter). Offsets are of each
//! class's ubergraph.
//!
//! The montages (C015_Up_Special_Ground_Montage2, C015_Up_Special_Air_Montage3): their AnimNotify_Spawner spawns the
//! cloud at f.576 attached to the Root socket with translation (0, 0, 150) and destroys it at the notify's end
//! (DestroyAllActorsOnNotifyEnd: the world's); the hurt box and control notifies share that window. The FightCloud
//! section loops until the cloud's first hit jumps Taz's montage to FinalHit (HandleSectionJump), whose loop the third
//! cloud hit ends with a jump to Finish; with no hit, DogPileExpire (3 s after the control notify's begin) jumps to
//! Finish. The cooldown versions (C015_Up_Special_Ground / Air_Cooldown_Montage, the start table's AbilitySlot -1 rows
//! behind the slot-1 ones) have no cloud: the shared start table routes to them while slot 1 is on cooldown, and the
//! cloud's TriggerAbilityOnSpawnIndex (slot 1) is the world's at the spawn. Buff_C015AttachToDogpile and
//! Buff_C015_TeamFightCloud are the TeamFightCloud perk's (`perks_cloud`: the perk is BPI_SetRemainingDogpileTime's one
//! caller and FirstHitConnected's one binder, and it writes the cloud's HitBoxMinScale / HitBoxMaxScale /
//! WeakHitCount, its two packets and its timeline: [`upgrade_dogpile`], [`update_dogpile`]).

use super::state::{taz, taz_of, taz_w};
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, end_buff, spawn_buff};
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hit::hurt_hit;
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{
    HitShape, actor_ally_hit, actor_hit_enemy, apply_projectile_hit, dealer, hit_box_actor_hits, hit_buffs_call,
    projectile_blocks,
};
use crate::combat::packet::{DataVec, VacuumInfo};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::spawned_hits::{class_rows, hit_box_types};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::montages::instance_data;
use crate::fighter::anim::sample_boxes;
use crate::fighter::common::{actor_data, data_of, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over, pawn_prop};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::tick::jump_section_logic;
use crate::fighter::{FighterId, actor_y, facing, invulnerable, mesh_wx, mesh_wy};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, NotifyHandler, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use indexmap::IndexSet;
use mvs_data::value::{Props, Value};
use std::sync::Arc;

const CLS: Name = name!("C015Dogpile_C");
const CONTROLLING: Name = name!("C015ControllingDogpile_C");
const HURT_CLOUD: &str = "HurtBoxFightCloud";
const AIR_MONTAGE: &str = "C015_Up_Special_Air_Montage3";

// ---------------------------------------------------------------- the pawn

/// BPI_ActivateFightCloudHurtBox(Active) @9604-9871: HurtBoxFightCloud.SetVisibility(Active) (nothing of the sim: the
/// shape is not drawn); with authority SetCollisionEnabled(Active ? QueryOnly : NoCollision) and SetSphereRadius(
/// Active ? 150 : 0). The box data's shape (radius 150, on the root capsule, tags HurtBox / HurtBox_FightCloud) is the
/// on state; off it is left out of the hit tests through the fighter's `hurt_off`.
/// Deviation: `hurt_off` stands for the component's overlap events (UpdateHurtBoxes); the collision off is written
/// there too. The two part only where an UpdateHurtBoxes names HurtBox or HurtBox_FightCloud while the collision is
/// off, which no Taz montage does (their HurtBoxUpdate tags are the arm, shoulder and hand ones).
pub fn activate_fight_cloud_hurt_box(w: &mut World, f: FighterId, active: bool) {
    taz_w(w, f).fight_cloud_hurt = active;
    let ch = w.fighter(f).ch;
    let i = w.names.get(HURT_CLOUD).and_then(|n| w.fixed.samplers.get(&ch)?.by_name.get(&n).copied());
    let Some(i) = i else {
        warn_text(w, "Taz: no HurtBoxFightCloud shape in the box data");
        return;
    };
    let i = i32::try_from(i).expect("a shape index");
    let h = &mut w.fighter_mut(f).hit.hurt_off;
    if active {
        h.shift_remove(&i);
    } else {
        h.insert(i);
    }
}

/// BPI_SetControllingDogpile(Active) @9566: DogPileActive = Active. True @4051: with DogPileTimer active nothing more;
/// else @3875 K2_SetTimerDelegate(DogPileExpire, DogPileDuration 3, not looping) and Comp_State_Manager
/// SpawnBuffActor(C015ControllingDogpile, self, self, 0 stacks, 0.0 s, true, false) @3982 (data only: SpeedMod 0.3
/// Override; a duration of 0 sets no life timer). False @4096: K2_ClearAndInvalidateTimerHandle(DogPileTimer), then
/// CheckHasBuff(C015ControllingDogpile) and its K2_DestroyActor.
/// The timer is set inside the montage's notify (the tick groups): pending until the frame's timer pass, so it fires
/// in the pass of frame N + timerTicks(3).
pub fn set_controlling_dogpile(w: &mut World, f: FighterId, active: bool) {
    taz_w(w, f).dog_pile_active = active;
    if active {
        if taz(w, f).dog_pile_at >= 0 {
            return;
        }
        let dur = pawn_prop(w, f, "DogPileDuration").unwrap_or(3.0);
        let tick = w.fighter(f).clock.tick;
        #[allow(clippy::cast_possible_truncation, reason = "a finite tick count")]
        let at = (crate::real(tick) + timer_ticks(Some(dur))) as i32;
        taz_w(w, f).dog_pile_at = at;
        let _ = spawn_buff(w, f, CONTROLLING, 0.0, Some(0), Some(f), true, None);
        return;
    }
    taz_w(w, f).dog_pile_at = -1;
    if let Some(b) = buff_of(w, f, CONTROLLING) {
        end_buff(w, f, b);
    }
}

/// BPI_SetRemainingDogpileTime(RemainingTime) @8572: with DogPileActive and DogPileTimer active, the timer is cleared
/// and set again for RemainingTime (not looping). Its one caller is Buff_C015_TeamFightCloud's
/// UpgradeDogpileMulticast (the TeamFightCloud perk, `perks_cloud`).
pub fn set_remaining_dogpile_time(w: &mut World, f: FighterId, remaining: Real) {
    let s = taz(w, f);
    if !(s.dog_pile_active && s.dog_pile_at >= 0) {
        return;
    }
    let tick = w.fighter(f).clock.tick;
    #[allow(clippy::cast_possible_truncation, reason = "a finite tick count (remaining > 0)")]
    let at = if remaining > 0.0 { (crate::real(tick) + timer_ticks(Some(remaining))) as i32 } else { -1 };
    taz_w(w, f).dog_pile_at = at;
}

/// DogPileExpire (the timer's delegate) @17116: Comp_Pawn_MontageNavigation.JumpToSectionMulticast(Finish, null, true,
/// false, 0.0) -> JumpToSectionLogic(Finish, null): the active montage.
fn dog_pile_timer(w: &mut World, f: FighterId) {
    let at = taz(w, f).dog_pile_at;
    if at < 0 || w.fighter(f).clock.tick < at {
        return;
    }
    taz_w(w, f).dog_pile_at = -1;
    jump_section_logic(w, f, name!("Finish"));
}

// ---------------------------------------------------------------- the notifies

/// AnimNotify_C015Dogpile's NotifyEnd. Its window (0.576 to 3.766) ends at Finish's start, the section only a jump
/// reaches (DogPileExpire, the cloud's third hit). The first dispatch after the jump lists the queued states with
/// EndTriggerTime > Previous, Previous the jump position, compared in float32 with no margin
/// (FAnimMontageInstance::HandleEvents; FAnimNotifyEvent::GetEndTriggerTime: LinkValue 0.57592595 + TriggerTimeOffset
/// 1e-4 + Duration 3.1897926 + EndTriggerTimeOffset -1e-4 = 3.7657187, one ulp past Finish's 3.7657185). The port's
/// dispatch lists a state only when its end is past the step's start by 2 EPS (its double clock's margin), which this
/// end (3.76571855 in double) is not. Recorded on the build: in C015_Up_Special_Air_Montage3 the state stays listed one
/// dispatch more (Finish's MoveCommand (0, 50) begins with C015ControllingDogpile's SpeedMod 0.3 still on, the
/// NotifyEnd comes in the next frame's dispatch); in C015_Up_Special_Ground_Montage2 it ends in the same dispatch as
/// the MoveCommand's begin, ahead of it.
/// Deviation: the air montage's end after the jump to Finish is held to the next frame's timer pass ([`timer_tick`])
/// instead of that frame's dispatch: the port's dispatch has no exact float32 compare of float32 end times after a
/// jump. The other states of that montage ending at Finish's start (AnimNotify_C015FightCloudHurtbox, the cloud's
/// AnimNotify_Spawner, SetGravity, AddInvisibility) end a frame early still.
fn dogpile_notify_end(w: &mut World, f: FighterId) {
    if in_air_finish(w, f) {
        let tick = w.fighter(f).clock.tick;
        taz_w(w, f).dog_pile_end_at = tick + 1;
        return;
    }
    set_controlling_dogpile(w, f, false);
}

/// The active montage is C015_Up_Special_Air_Montage3, not blending out, at or past its Finish section.
fn in_air_finish(w: &World, f: FighterId) -> bool {
    let Some(m) = &w.fighter(f).montages.montage else { return false };
    if w.names.str(m.montage.name) != AIR_MONTAGE || m.blending_out() {
        return false;
    }
    let data = w.data.clone();
    let Some(md) = instance_data(&data, w, f, m.montage.uid) else { return false };
    md.sections.iter().find(|s| s.name == "Finish").is_some_and(|s| m.pos >= s.t)
}

/// The held NotifyEnd ([`dogpile_notify_end`]), in the timer pass ahead of DogPileTimer.
fn dogpile_end_tick(w: &mut World, f: FighterId) {
    let at = taz(w, f).dog_pile_end_at;
    if at < 0 || w.fighter(f).clock.tick < at {
        return;
    }
    taz_w(w, f).dog_pile_end_at = -1;
    set_controlling_dogpile(w, f, false);
}

/// AnimNotify_C015Dogpile (AnimNotifyState): NotifyBegin -> the owner's BPI_SetControllingDogpile(true), on an owner
/// that implements BPI_C015 (a Taz); return values only.
fn control_begin(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if taz_of(w, f).is_none() {
        return;
    }
    taz_w(w, f).dog_pile_end_at = -1;
    set_controlling_dogpile(w, f, true);
}

/// AnimNotify_C015Dogpile's NotifyEnd -> BPI_SetControllingDogpile(false) ([`dogpile_notify_end`]).
fn control_end(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if taz_of(w, f).is_some() {
        dogpile_notify_end(w, f);
    }
}

/// AnimNotify_C015FightCloudHurtbox (AnimNotifyState): NotifyBegin -> BPI_ActivateFightCloudHurtBox(true), NotifyEnd ->
/// (false), the same cast.
fn hurt_begin(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if taz_of(w, f).is_some() {
        activate_fight_cloud_hurt_box(w, f, true);
    }
}

fn hurt_end(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if taz_of(w, f).is_some() {
        activate_fight_cloud_hurt_box(w, f, false);
    }
}

// ---------------------------------------------------------------- C015Dogpile_C

/// `Dogpile` (C015Dogpile_C). A SpawnedActorParent child (no pool in the port: InitFromPool @3589, which resets
/// HasHitEnemy only, is not reached). Its components: Capsule (root, absolute scale) > ScalingComponent (hidden) >
/// HitBox (sphere 200, tag HitBox), HitBox2D (capsule radius 200, half height 750 along the depth, tag HitBox2D),
/// AllyOverlap (sphere 200: the perk's), ParticleSystem (PS_C015_FightCloud); ParticleSystem2
/// (PS_C015_FightCloud_Arms) on the Capsule.
/// - ReceiveBeginPlay @917: the parent's (HitBoxOn? true: UpdateHitbox -> HitBoxesOn([HitBox], HitBoxDamage, (1, 1, 1))
///   with the actor's AttackID), HitBoxTimeline.SetPlayRate(1 / MainDuration) @969, then @747 with authority a looping
///   0.05 s RefreshHitbox timer @781 (RefreshHitboxTimer), and HitBoxTimeline.PlayFromStart @884.
/// - HitBoxTimeline (linear 0 -> 1, length 1, rate 1/3: 3 s) update @523: Alpha = its value,
///   ScalingComponent.SetRelativeScale3D(VLerp(HitBoxMinScale 1, HitBoxMaxScale 2.5, Alpha)) @705. Finished: nothing
///   (@3844).
/// - RefreshHitbox (the 0.05 s timer) @3764: HitBoxOn? = false, UpdateHitbox (HitBoxesOff: the actors hit are
///   forgotten), Delay(0.0) -> @15: RefreshHitboxTimer still active -> HitBoxOn? = true, UpdateHitbox (on again).
/// - HitEnemyPlayer @1015: the parent's, then with !HasHitEnemy: broadcast FirstHitConnected (bound by the perk alone),
///   HasHitEnemy = true, MulticastOnHit @1127: HitBoxTimeline.Stop, HandleSectionJump @2403 (the Instigator's
///   MontageNavigation.JumpToSectionLogic(FinalHit, null) @2647; PS_C015_FightCloud_Hit at the actor, scale
///   VLerp(1, 1.3 x HitBoxMaxScale, Alpha), team colour @2840-3110), then @81 with authority: clear
///   RefreshHitboxTimer, HitBoxOn? = false, UpdateHitbox (off), the Instigator's BPI_SpawnedActor.BPI_GetAttackID (a
///   pawn does not implement it: 0 @495) into WeakHitBoxDamage.AttackID and FinalHitBoxDamage.AttackID, FightCloudHit
///   @373, and a looping HitFrequency 0.2 s FightCloudHit timer @410 (HitTimer).
/// - FightCloudHit @1184: HitBoxesOff([HitBox]); CurrentWeakHits + 1; WeakHitBoxDamage.VacuumInfo.VacuumSocketOffset =
///   ((even ? 0.5 : -0.5) x 200 x s, 200 x s x 0.3) with s = Lerp(HitBoxMinScale, HitBoxMaxScale, Alpha) @1349-1803;
///   HitBoxesOn([HitBox], count >= WeakHitCount 3 ? FinalHitBoxDamage : WeakHitBoxDamage, (1, 1, 1)) @1929; at
///   count >= 3 clear HitTimer and the Instigator's JumpToSectionMulticast(Finish, null, true, false, 0) @2345.
///   So the first hit is followed at once by weak hit 1, then weak hit 2 and the final hit on the 0.2 s timer.
///
/// The Instigator is always the spawning Taz (its kind has no foreign owner: a Disguise's copy is refused).
///
/// Model. A World.actors entry. Its step (the actor's tick: the Delay(0.0)'s resume among its latent actions, then the
/// timeline's tick, then the attachment) runs in its spawn frame too; its two looping timers run in the owner's timer
/// pass ([`timer_tick`]), and a HitBoxesOn made there or inside a hit is tested at once (the overlap updates of
/// SetCollisionEnabled); the box that is on is also tested in the world's actor hits pass each frame, with
/// HitEachActorOnlyOnce's set (true in all three packets) cleared by every HitBoxesOff.
/// Unverified: the order of the actor's latent actions against its timeline component's tick and of its timers
/// against the owner's (DogPileTimer first here) is not read; nor whether a HitBoxesOn inside the timer pass meets a
/// victim before or after the other actors' hits of that frame.
/// Unverified: the attachment takes the Root socket of the pose sampled now plus 150 straight up (the Root bone's
/// rotation is not applied to the translation), and the actor's facing (the KnockBackDirectionVector side) is the
/// Instigator's of now: attached, it turns with him. Its GetVelocity (the initial hit's AccountForAttackerVelocity)
/// is taken as 0: no movement component moves it.
/// The HitBox on the actors that are no fighter (a projectile's HurtBox, an item, a destructible actor: the types its
/// template answers) is `hit_box_actor_hits`, after the fighters of the same pass; a hit there is no HitEnemyPlayer.
/// Deviation: the circle in the play plane (the 2D box's depth not tested); an activation that ends in its first
/// enemy hit (MulticastOnHit) meets no actor in that pass.
#[derive(State, Debug, Default)]
pub struct Dogpile {
    /// `p` where a graph wrote the instance's variables (`setInstanceVars`: the perk's HitBoxMinScale /
    /// HitBoxMaxScale / MainDuration / WeakHitCount), else `None`: the class defaults.
    pub own_p: Option<Arc<Props>>,
    /// The packet the HitBox carries now (HitBoxDamage, then the weak or final one).
    pub hit: HitPacket,
    /// The packet's AttackID: the actor's at its spawn, 0 from the first hit on (@495).
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry (SpawnedActorParent).
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// Alpha: HitBoxTimeline's value.
    pub alpha: Real,
    /// HitBoxTimeline playing.
    pub growing: bool,
    /// HitBoxOn? (the HitBox's collision).
    pub hit_on: bool,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// The actors the HitBox overlaps (`hit_box_actor_hits`).
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track.
    pub once: IndexSet<ActorRef>,
    /// The gathered object types the HitBox answers.
    pub types: Vec<Real>,
    /// HasHitEnemy.
    pub has_hit_enemy: bool,
    /// CurrentWeakHits.
    pub weak_hits: i32,
    /// RefreshHitbox's Delay(0.0) pending.
    pub resume: bool,
    /// The looping timers: the world tick they were set at (-1: the handle not active) and the times they fired.
    pub refresh_set: i32,
    pub refresh_n: i32,
    pub hit_set_at: i32,
    pub hit_n: i32,
    /// WeakHitBoxDamage (the actor's copy: FightCloudHit writes its vacuum offset).
    pub weak: HitPacket,
    /// FinalHitBoxDamage.
    pub fin: HitPacket,
    /// HitBox2D's CapsuleRadius with 2D hit boxes, HitBox's SphereRadius without.
    pub radius: Real,
    /// HitBox's SphereRadius (FightCloudHit's GetUnscaledSphereRadius).
    pub sphere_r: Real,
    /// BPI_Enable2DHitboxes(Supports2DHitboxes and the owner's BPI_Use2DHitboxes).
    pub is_2d: bool,
    /// The notify's translation up.
    pub oz: Real,
}

/// The cloud's variables, while it is in the actors' list.
pub fn dogpile_of(w: &World, id: ActorId) -> Option<&Dogpile> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Dogpile(s) => Some(s),
        _ => None,
    }
}

fn dog_mut(w: &mut World, id: ActorId) -> Option<&mut Dogpile> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Dogpile(s) => Some(s),
        _ => None,
    }
}

/// The cloud's `p`: its own variables where the perk wrote them, else the class defaults.
fn props(w: &World, id: ActorId) -> &Props {
    match dogpile_of(w, id).and_then(|s| s.own_p.as_deref()) {
        Some(p) => p,
        None => actor_p(w, ActorRef::Actor(id)),
    }
}

/// `p[k]` of the cloud, as a number.
pub fn p_num(w: &World, id: ActorId, k: &str) -> Option<Real> {
    num(props(w, id), k)
}

/// The cloud of this id when it is a dogpile still in the actors' list: its owner and whether it is dead.
pub fn dogpile_owner(w: &World, id: ActorId) -> Option<(FighterId, bool)> {
    let a = w.actors.actors.get(id)?;
    matches!(a.state, WorldActorState::Dogpile(_)).then_some((a.owner, a.dead))
}

/// Alpha, HasHitEnemy of a dogpile still in the actors' list.
pub fn dogpile_alpha(w: &World, id: ActorId) -> Option<Real> {
    dogpile_of(w, id).map(|s| s.alpha)
}

pub fn dogpile_has_hit(w: &World, id: ActorId) -> bool {
    dogpile_of(w, id).is_some_and(|s| s.has_hit_enemy)
}

/// ScalingComponent's scale: VLerp(HitBoxMinScale, HitBoxMaxScale, Alpha) (uniform).
fn scale(w: &World, id: ActorId) -> Real {
    let lo = p_num(w, id, "HitBoxMinScale").unwrap_or(1.0);
    let hi = p_num(w, id, "HitBoxMaxScale").unwrap_or(1.0);
    let alpha = dogpile_of(w, id).map_or(0.0, |s| s.alpha);
    lo + (hi - lo) * alpha
}

/// ScalingComponent's world scale: its relative one under the root's (SetActorScale3D: the absolute Capsule's).
pub fn world_scale(w: &World, id: ActorId) -> Real {
    let s = scale(w, id);
    match w.actors.actors.get(id).and_then(|a| a.size_scale) {
        Some(k) => s * k,
        None => s,
    }
}

/// Attached at the owner's Root socket plus the notify's translation (Mesh.GetSocketLocation in his pose; his actor
/// location when the socket is not sampled).
fn follow(w: &World, f: FighterId, oz: Real) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let sampler = w.fixed.samplers.get(&ch);
    let i = w.names.get("socket:Root").and_then(|n| sampler?.by_name.get(&n).copied());
    let mut pose: Vec<f32> = Vec::new();
    let o = i.filter(|_| sample_boxes(w, f, &mut pose)).and_then(|i| {
        let i = usize::try_from(i).expect("a box index");
        sampler?.offsets.get(i).copied()
    });
    let Some(o) = o else {
        warn_text(w, "C015Dogpile_C: the Root socket not sampled (the owner location taken)");
        return (w.fighter(f).body.x, actor_y(w, f) + oz);
    };
    let o = usize::try_from(o).expect("a box offset");
    (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]) + oz)
}

/// The Dogpile's place (its `x`, `y`).
pub fn place(_w: &World, _a: &WorldActor, s: &Dogpile) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The Dogpile as a dealer (`DealerActor`): its place, facing (its owner's) and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &Dogpile) -> (Real, Real, Real, i32) {
    (s.x, s.y, facing(w, a.owner), s.attack_id)
}

/// The Dogpile's HitBoxDamage packet (`hit`).
pub fn hit(s: &Dogpile) -> &HitPacket {
    &s.hit
}

/// A looping timer of Rate T set at world tick `set` (outside the timer pass: pending until that frame's pass, which
/// starts it without counting the frame) that has fired n times is due in the pass of tick `now` when the counted
/// deltas pass (n + 1) x T: FTimerManager adds the Rate to the expire time at each fire.
fn loop_due(now: i32, set: i32, n: i32, rate: Real) -> bool {
    set >= 0 && crate::consts::clock(crate::real(now - set)) > f64::from(n + 1) * f64::from(rate)
}

/// HitBoxesOff([HitBox]) / UpdateHitbox off: the collision off, the actors hit forgotten (SetCollisionEnabled(0)
/// @17186 ends the overlaps: HitBoxesOn @16817 begins them again).
fn box_off(s: &mut Dogpile) {
    s.hit_on = false;
    s.hit_set.clear();
    s.once.clear();
    s.over.clear();
}

/// The HitBox where it is now: its circle in the play plane, extruded along the depth with 2D hit boxes (HitBox2D:
/// half height 750 x the scale), a sphere on the plane without.
fn hit_sphere(w: &World, id: ActorId) -> Option<WorldSphere> {
    let s = dogpile_of(w, id)?;
    let r = s.radius * world_scale(w, id);
    Some(WorldSphere { x: s.x, y: s.y, z: if s.is_2d { None } else { Some(0.0) }, r, ..WorldSphere::default() })
}

/// The HitBox against the fighters' hurt shapes, by the handler's pfgDoDamage gates in the projectile test's order. A
/// hit that lands (Hit true) is the actor's HitEnemyPlayer. A team mate gets pfgDoDamage as anyone (C015Dogpile has no
/// ally logic of its own) behind the same gates: the entry rows, the ally rows @5931-6835 (the class has no rows) and
/// NotifyAllyOfHit @21344 (`actor_ally_hit`); DamageAndLaunch between them refuses him (CanBeHitBy). The owner gets
/// nothing.
fn test_box(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, dead) = (a.owner, a.dead);
    let Some(s) = dog_mut(w, id) else { return };
    if !s.hit_on || dead {
        s.over.clear();
        return;
    }
    let team = w.fighter(owner).team.team;
    let mut shapes: Option<WorldSphere> = None;
    for vic in match_fighters(w) {
        let Some(s) = dogpile_of(w, id) else { return };
        let vid = w.fighter(vic).id;
        if vic == owner || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let ally = w.fighter(vic).team.team == team;
        let Some(vp) = victim_pose(w, vic) else { continue };
        if shapes.is_none() {
            shapes = hit_sphere(w, id);
        }
        let Some(sp) = shapes.as_ref() else {
            give_vbuf(w, vp);
            return;
        };
        let met = hurt_hit(w, vic, std::slice::from_ref(sp), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let Some(s) = dogpile_of(w, id) else { return };
        let (x, y, attack_id) = (s.x, s.y, s.attack_id);
        let hp = s.hit.clone();
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hp.do_los_check.unwrap_or(false) && los_blocked(w, x, y, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            if !ally {
                let tick = w.flow.tick;
                attack_dodged(w, me, vic, attack_id, hp.base_knock_back.unwrap_or(0.0), tick, None, false);
            }
            continue;
        }
        dodge_over(w, me, vic);
        if hp.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = dog_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        if ally {
            hit_buffs_call(w, owner, &rows); // the entry rows @842-2821
            let (d, mult) = (dealer(w, me), charge_buff(w, me));
            actor_ally_hit(w, &d, vic, &hp, &rows, mult);
            continue;
        }
        let pct = charge_pct(w, me);
        let d = dealer(w, me);
        if !apply_projectile_hit(w, &d, vic, &hp, &rows, V2 { x, y }, None, &[], pct) {
            continue;
        }
        actor_hit_enemy(w, &dealer(w, me), vic);
        // HitEnemyPlayer @1015. MulticastOnHit makes its own activation (FightCloudHit), tested inside it against
        // every fighter; what this activation had left to meet is left with it. Broadcast FirstHitConnected @1082
        // ahead of HasHitEnemy = true (its one binder: the TeamFightCloud perk).
        if !dogpile_of(w, id).is_some_and(|s| s.has_hit_enemy) {
            super::perks_cloud::first_hit(w, id);
            if let Some(s) = dog_mut(w, id) {
                s.has_hit_enemy = true;
            }
            on_hit(w, id);
            return;
        }
    }
    let Some(sp) = hit_sphere(w, id) else { return };
    let Some(s) = dog_mut(w, id) else { return };
    let (types, hit) = (s.types.clone(), s.hit.clone());
    let (mut over, mut once) = (std::mem::take(&mut s.over), std::mem::take(&mut s.once));
    hit_box_actor_hits(w, me, &hit, &types, Some(HitShape::Sphere { x: sp.x, y: sp.y, r: sp.r }), &mut over, &mut once);
    if let Some(s) = dog_mut(w, id) {
        s.over = over;
        s.once = once;
    }
}

/// MulticastOnHit @1127 (offline: the authority's branch runs too).
fn on_hit(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    if let Some(s) = dog_mut(w, id) {
        s.growing = false; // HitBoxTimeline.Stop
    }
    // HandleSectionJump @2403: the Instigator's JumpToSectionLogic(FinalHit, null), then the hit emitter.
    jump_section_logic(w, owner, name!("FinalHit"));
    let hi = p_num(w, id, "HitBoxMaxScale").unwrap_or(1.0);
    let Some(s) = dogpile_of(w, id) else { return };
    let (x, y, alpha) = (s.x, s.y, s.alpha);
    let fighter = w.fighter(owner).id;
    w.events.push(Event::ProjectileDeath {
        cls: CLS,
        fighter,
        vfx: Some(name!("PS_C015_FightCloud_Hit")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: (1.0 + (1.3 * hi - 1.0) * alpha),
        tint: None,
        to: None,
    });
    let Some(s) = dog_mut(w, id) else { return };
    s.refresh_set = -1; // K2_ClearAndInvalidateTimerHandle(RefreshHitboxTimer)
    s.resume = false; // its Delay's resume finds the handle cleared
    box_off(s);
    // the Instigator is no BPI_SpawnedActor (@495)
    s.weak.attack_id = Some(0);
    s.fin.attack_id = Some(0);
    s.attack_id = 0;
    fight_cloud_hit(w, id);
    // HitTimer (HitFrequency, looping): set inside the hit, in the tick groups.
    let count = p_num(w, id, "WeakHitCount").unwrap_or(3.0);
    let dead = w.actors.actors.get(id).is_none_or(|a| a.dead);
    let tick = w.flow.tick;
    if let Some(s) = dog_mut(w, id)
        && !dead
        && crate::real(s.weak_hits) < count
    {
        s.hit_set_at = tick;
        s.hit_n = 0;
    }
}

/// FightCloudHit @1184.
fn fight_cloud_hit(w: &mut World, id: ActorId) {
    let s_ = scale(w, id);
    let count = p_num(w, id, "WeakHitCount").unwrap_or(3.0);
    let Some(s) = dog_mut(w, id) else { return };
    box_off(s);
    s.weak_hits += 1;
    let n = s.weak_hits;
    // HitBox.GetUnscaledSphereRadius (200) and Lerp(HitBoxMinScale, HitBoxMaxScale, Alpha) (@1472-1757).
    let rad = s.sphere_r;
    let side = if n % 2 == 0 { 0.5 } else { -0.5 };
    let off = DataVec { x: Some((side * rad) * s_), y: Some((rad * s_) * 0.3), z: None };
    s.weak.vacuum_info.get_or_insert_with(VacuumInfo::default).vacuum_socket_offset = Some(off);
    let last = crate::real(n) >= count;
    s.hit = if last { s.fin.clone() } else { s.weak.clone() };
    s.hit_on = true;
    test_box(w, id);
    if !last {
        return;
    }
    let Some(s) = dog_mut(w, id) else { return };
    s.hit_set_at = -1; // K2_ClearAndInvalidateTimerHandle(HitTimer)
    let Some(owner) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    jump_section_logic(w, owner, name!("Finish"));
}

/// SpawnedActorParent CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?).
fn left_arena(w: &mut World, id: ActorId) -> bool {
    let doe = flag(props(w, id), "DestroyOnExitedArena?");
    let Some(s) = dog_mut(w, id) else { return false };
    let bounds = s.blast_bounds;
    blast_timer_exit(doe, &mut s.blast_at, &bounds, crate::real(s.age - 1), s.x, s.y)
}

fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let dur = p_num(w, id, "MainDuration").unwrap_or(3.0);
    let Some(s) = dog_mut(w, id) else { return };
    s.age += 1;
    // RefreshHitbox's Delay(0.0) resume @15: with RefreshHitboxTimer still active, the box on again.
    if s.resume {
        s.resume = false;
        if s.refresh_set >= 0 {
            s.hit_on = true;
        }
    }
    // HitBoxTimeline's tick: Position + DeltaSeconds x PlayRate, clamped to its length 1.
    if s.growing {
        s.alpha = min(s.alpha + (DELTA_SECONDS * (1.0 / dur)), 1.0);
        if s.alpha >= 1.0 {
            s.growing = false;
        }
    }
    let oz = s.oz;
    let (x, y) = follow(w, owner, oz);
    if let Some(s) = dog_mut(w, id) {
        s.x = x;
        s.y = y;
    }
    if left_arena(w, id)
        && let Some(a) = w.actors.actors.get_mut(id)
    {
        a.dead = true;
    }
}

/// Its two timers in the owner's timer pass.
fn timers(w: &mut World, id: ActorId) {
    if w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return;
    }
    let now = w.flow.tick;
    if let Some(s) = dog_mut(w, id)
        && loop_due(now, s.refresh_set, s.refresh_n, 0.05)
    {
        s.refresh_n += 1;
        box_off(s); // RefreshHitbox @3764
        s.resume = true;
    }
    let rate = p_num(w, id, "HitFrequency").unwrap_or(0.2);
    if let Some(s) = dog_mut(w, id)
        && loop_due(now, s.hit_set_at, s.hit_n, rate)
    {
        s.hit_n += 1;
        fight_cloud_hit(w, id);
    }
}

/// What Buff_C015_TeamFightCloud writes into the cloud (UpgradeDogpileMulticast @7907-8568): HitBoxTimeline.
/// SetPlayRate(1 / duration) (the actor's MainDuration, which only BeginPlay's SetPlayRate reads, stands for the rate:
/// the step reads it each tick), HitBoxMaxScale, HitBoxMinScale, SetMinimumRemainingAlpha(r) @3629
/// (HitBoxTimeline.SetNewTime(FClamp(Alpha, 0, 1 - r)): Alpha = that position, the play state as it was).
pub fn upgrade_dogpile(w: &mut World, id: ActorId, min_scale: Real, max_scale: Real, duration: Real, min_alpha: Real) {
    set_instance_vars(
        w,
        id,
        &[("HitBoxMinScale", min_scale), ("HitBoxMaxScale", max_scale), ("MainDuration", duration)],
    );
    if let Some(s) = dog_mut(w, id) {
        s.alpha = min(max(s.alpha, 0.0), 1.0 - min_alpha);
    }
}

/// UpdateDogpile @9575-9712: WeakHitCount, WeakHitBoxDamage, FinalHitBoxDamage (a copy each; the weak one's
/// VacuumSocketOffset is FightCloudHit's to write).
pub fn update_dogpile(w: &mut World, id: ActorId, hits: Real, weak: &Props, fin: &Props) {
    set_instance_vars(w, id, &[("WeakHitCount", hits)]);
    let mut wk = HitPacket::from_props(&w.names, weak, None);
    wk.vacuum_info.get_or_insert_with(VacuumInfo::default);
    let fp = HitPacket::from_props(&w.names, fin, None);
    if let Some(s) = dog_mut(w, id) {
        s.weak = wk;
        s.fin = fp;
    }
}

/// `setInstanceVars`: the instance takes its own copy of the class defaults with the new values.
fn set_instance_vars(w: &mut World, id: ActorId, vars: &[(&str, Real)]) {
    let mut p = props(w, id).clone();
    for &(k, v) in vars {
        p.insert(k.into(), Value::Num(f64::from(v)));
    }
    if let Some(s) = dog_mut(w, id) {
        s.own_p = Some(Arc::new(p));
    }
}

/// The kind's `spawn`: the state around the class data, the parent's BeginPlay blast-box loop, and RefreshHitboxTimer
/// set at BeginPlay @781, inside the spawn (the tick groups).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::Dogpile(Dogpile::default()), true);
    };
    let (p, comps) = (&a.p, &a.components);
    let empty = Props::new();
    let wk = obj(p, "WeakHitBoxDamage").unwrap_or(&empty);
    let mut weak = HitPacket::from_props(&w.names, wk, None);
    weak.vacuum_info.get_or_insert_with(VacuumInfo::default).vacuum_socket_offset =
        Some(DataVec { x: Some(0.0), y: Some(0.0), z: None });
    let fin = HitPacket::from_props(&w.names, obj(p, "FinalHitBoxDamage").unwrap_or(&empty), None);
    let is_2d = flag(p, "Supports2DHitboxes") && data_of(w, f).def.use_2d;
    // USphereComponent's default radius when the template has none
    let sphere_r = obj(comps, "HitBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0);
    let radius =
        if is_2d { obj(comps, "HitBox2D").and_then(|c| num(c, "CapsuleRadius")).unwrap_or(22.0) } else { sphere_r };
    let types = hit_box_types(obj(comps, if is_2d { "HitBox2D" } else { "HitBox" }));
    let np = crate::world::spawning::request_props(w, f, r);
    let attached = flag(&np, "AttachToActor");
    let root = np.get("Socket").and_then(Value::as_str) == Some("Root");
    if !attached || !root || flag(&np, "WorldTransforms") {
        warn_text(w, "C015Dogpile_C: a spawn other than attached at the Root socket is not ported");
    }
    let oz = obj(&np, "Transforms").and_then(|t| obj(t, "Translation")).and_then(|t| num(t, "Z")).unwrap_or(0.0);
    let (x, y) = follow(w, f, oz);
    let mut s = Dogpile {
        own_p: None,
        hit: HitPacket::from_props(&w.names, obj(p, "HitBoxDamage").unwrap_or(&empty), None),
        attack_id: r.attack_id,
        x,
        y,
        age: 0,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        alpha: 0.0,
        growing: true,
        hit_on: flag(p, "HitBoxOn?"),
        types,
        refresh_set: -1,
        hit_set_at: -1,
        weak,
        fin,
        radius,
        sphere_r,
        is_2d,
        oz,
        ..Dogpile::default()
    };
    // the parent's BeginPlay loop
    let force = flag(p, "ForceSpawnPositionInsideBlastBox");
    let doe = flag(p, "DestroyOnExitedArena?");
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe);
    s.refresh_set = w.flow.tick; // BeginPlay @781
    (WorldActorState::Dogpile(s), dead)
}

/// The cloud's OnEndPlay, which the TeamFightCloud perk binds (`perks_cloud::dogpile_end`).
fn end(w: &mut World, id: ActorId) {
    super::perks_cloud::dogpile_end(w, id);
}

/// dogpile.rs's part of the joined matchReset: UserConstructionScript @94: BPI_ActivateFightCloudHurtBox(false), behind
/// the parent's (which turns every HurtBox shape on). A match reset spawns a new pawn.
pub fn match_reset(w: &mut World, f: FighterId) {
    activate_fight_cloud_hurt_box(w, f, false);
}

/// dogpile.rs's part of the joined timerTick: the held NotifyEnd, the pawn's DogPileTimer, then the timers of the
/// clouds he spawned.
pub fn timer_tick(w: &mut World, f: FighterId) {
    dogpile_end_tick(w, f);
    dog_pile_timer(w, f);
    let list = w.fighter(f).items.spawned_actors.clone();
    for x in list {
        let ActorRef::Actor(id) = x else { continue };
        if dogpile_of(w, id).is_some() && w.actors.actors.get(id).is_some_and(|a| !a.dead) {
            timers(w, id);
        }
    }
}

/// This file's registry entries (the two notifies, C015Dogpile_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_C015Dogpile_C",
        NotifyHandler { begin: Some(control_begin), end: Some(control_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C015FightCloudHurtbox_C",
        NotifyHandler { begin: Some(hurt_begin), end: Some(hurt_end), ..NotifyHandler::default() },
    )?;
    // The HitBox and AllyOverlap read the world scale (FightCloudHit's offsets are unscaled radii), the view too.
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        size: Some(SizeHooks::default()),
        step: Some(step),
        hits: Some(test_box),
        end: Some(end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        "C015Dogpile_C",
        ActorEntry::new(ActorClass::World(WorldActorClass::C015Dogpile), ActorHooks::World(hooks)),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_due_counts_the_deltas_after_the_set_frame() {
        // set at tick 10, 0.05 s: 2 deltas are not past it, 3 (0.0500004) are
        assert!(!loop_due(12, 10, 0, 0.05));
        assert!(loop_due(13, 10, 0, 0.05));
        assert!(!loop_due(14, -1, 0, 0.05));
    }
}
