//! Velma's carry run: VelmaCarryRun_Actor ([`VelmaCarryRun`], a dealer world
//! actor: her run while carrying an ally or an enemy, its pick-ups, the buttons it binds and the release) and
//! Buff_Velma_Carry_Speed. It calls capture.rs (`start_capture`) and pawn.rs (`set_carrying`).
//!
//! Velma_Special_Pickup_HoldPose_Montage spawns the actor at 0.0001 s (attached to the owner, no socket, no
//! translation), activates it at 2.7905 s (ActivateSpawned) and ends it with the montage (DestroyAllActorsOnNotifyEnd).
//! Offsets are the class ubergraph's.
//!
//! The run is attached to Velma's root: its HitBox (sphere 125 at (100, 0, 40), forward along the root's X: sim x by her
//! facing) and its JumpOnCollision (sphere 200 x 1.25 at Z 233.75, DetectPawn) overlap the fighters' root capsules
//! (`sphere_hits`).
//!
//! - ReceiveBeginPlay @2470: BaseDamage = HitBoxDamage, BaseMaxAceleration = the owner's MaxAcceleration, TeamIndex = the
//!   owner's; HitStunStart -> HitStunned and KnockedBack -> K2_DestroyActor bound; BPI_IsCarrying(false) @1876.
//! - ActivateObject -> SetMovementVariableMulticast @1354: CanHit, CheckToHit() (with `Percent Charged` at its class
//!   default: it is written at @379, after the call), JumpGate, CanJump, CruseControlActive, the owner's AirControl = 1
//!   and MaxAcceleration, Percent Charged, Buff_Velma_Carry_Speed, sfx_cha_jake_foley_horse_footstep.
//! - ReceiveTick @697: AddMovementInput((0, -1, 0), owner facing side) @1107, the facing flip -> TurnGate false, CanHit
//!   false, RetriggerableDelay(0.2) -> TurnGate -> RetriggerableDelay(0.1) -> CanHit, CheckToHit().
//! - CheckToHit @10260 / HitBox BeginOverlap @15416: a fighter of another team not in ActorsHitCooldown: CarryInt >= 1
//!   a hit (Damage and BaseKnockBack lerped by Percent Charged), else the capture (Buff_VelmaAttachToAlly on it).
//! - JumpOnCollision @6224 / @8147, ButtonPressed @6834 (an ally's InputTriggered: ArmorPack both ways), UpdateCarryNumber,
//!   CarryBuffDestroyed @18407, Fall @18708, ReceiveEndPlay @18912 -> @9705 (the release: an ally launched, an enemy
//!   hit by the release packet @3821-4854).
//!
//! Deviation: the overlaps' begin and end are the port's (a fighter's root capsule not touched at the last test and
//!   touched now), tested once a frame in the world's actor hit pass, the HitBox before the JumpOnCollision. Only
//!   fighters are tested: the HitBox's BPI_DestructibleObject branches (Map_M009 / Batcave destructibles) and
//!   HitBoxHandler.HitNonPlayer -> CanTurn are not ported (as the horse's).
//! Deviation (as chars/jake/horse.rs): the owner's HitStunStart and KnockedBack are sampled in the step: a KnockedBack
//!   since the last step, or a buff stun that began since then.
//! Unverified: a minion's team for the jump-on is 0 (GetByFighter: a minion has no MatchPlayerData and no PlayerState
//!   offline, so it fails, and GetTeamIndex on None gives its default). Not read live.
//! Unverified: the RetriggerableDelays count their start frame, as the port's other latent Delays; the 0.1 s one started
//!   by the 0.2 s one's end is counted from that frame.
//! Deviation: CarryBuffDestroyed (a PickUpBuffs buff's OnEndPlay) runs inside the actor's EndPlay for the buffs it ends,
//!   at [`carry_buff_ended`]'s call for a hook that makes it, else at the actor's next step (the buff found ended there).
//! Deviation: the data check of the spawn (its attachment, its two spheres against the port's) is not ported.
//! Not ported: the JumpOn prompt widget (`jump_on_ui` is the sim's part, for the HUD), BPI_IncrementCoopActions,
//!   Audio_Turn.
//!
//! Buff_Velma_Carry_Speed: UserConstructionScript SpeedMod = Lerp(1, SpeedMod 3, FMin(the owner's PercentCharged, 1));
//! ReceiveBeginPlay the owner's CMC BrakingFriction 10000, BrakingFrictionFactor 10000, GroundFriction 10; ReceiveEndPlay
//! the CharacterBase values (0, 5, 3).
//! Deviation: MinAnalogWalkSpeed is not modelled by the port: its write (the crouched walk speed, which stays after
//!   the run) is dropped. MaxWalkSpeedCrouched is written back with its own value: nothing.

use mvs_data::value::Value;

use crate::Real;
use crate::State;
use crate::actors::tether_shape::sphere_hits;
use crate::actors::world_actor::actor_p;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass, WorldActorState};
use crate::combat::buffs::{buff, buff_of, end_buff, inst_view, set_buff_values, spawn_buff};
use crate::combat::defense::add_temp_health_pack;
use crate::combat::elem::charge_pct;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer};
use crate::combat::{BuffClass, BuffRef, DataEnum, DataVec, HitPacket};
use crate::consts::{AIR_CONTROL, BRAKING_FRICTION_FACTOR, GROUND_FRICTION};
use crate::events::Event;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::{MontageSlot, slot_jump_to_section};
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::nav_jump;
use crate::fighter::moves::{Impulse, V2};
use crate::fighter::moves::{is_falling, max_accel, set_cmc_friction, set_max_accel};
use crate::fighter::{FighterId, actor_y, facing, grounded, invulnerable, no_collision};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{num_or, obj};
use crate::registry::signals::FighterSignal;
use crate::registry::{ActorEntry, BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;

use super::capture::{ATTACH_TO_ALLY_N, start_capture};
use super::pawn::set_carrying;
use super::state::{velma_of, velma_w};

pub const CARRY_RUN: &str = "VelmaCarryRun_Actor_C";
pub const CARRY_SPEED: &str = "Buff_Velma_Carry_Speed_C";
const CARRY_SPEED_N: Name = name!("Buff_Velma_Carry_Speed_C");
const HOLD_POSE: Name = name!("Velma_Special_Pickup_HoldPose_Montage");
const ARMOR_PACK: Name = name!("Buff_ArmorPack_C");

/// A sphere of the run from the actor (the root): forward, up and its radius (the class data's).
struct Sphere {
    x: Real,
    y: Real,
    r: Real,
}
const HITBOX: Sphere = Sphere { x: 100.0, y: 40.0, r: 125.0 };
const JUMPON: Sphere = Sphere { x: 0.0, y: 233.75, r: 250.0 };

/// A PickUpBuffs entry: a buff and the fighter it is on.
#[derive(State, Copy, PartialEq, Debug)]
pub struct PickUp {
    pub f: FighterId,
    pub b: BuffRef,
}

/// `VelmaCarryRun` (VelmaCarryRun_Actor_C): the class's fields. Sim axes.
#[derive(State, Debug)]
pub struct VelmaCarryRun {
    /// BaseDamage (HitBoxDamage at BeginPlay).
    pub base: HitPacket,
    /// HitBoxDamage, which the hits write.
    pub hit: HitPacket,
    /// The AttackID of the spawn request.
    pub attack_id: i32,
    /// Its root's place.
    pub x: Real,
    pub y: Real,
    /// The section the actor last sent the owner's montage to (`RunEnd`, `Fall`; none).
    pub section: Name,
    /// JumpOnAlly.
    pub jump_on_ally: Option<FighterId>,
    pub jump_on_ui: bool,
    /// CarriedAllyFighter and CarriedEnemyFighter.
    pub carried_ally: Option<FighterId>,
    pub carried_enemy: Option<FighterId>,
    /// CarryInt.
    pub carry_int: i32,
    /// FacingLeft.
    pub facing_left: bool,
    /// TurnGate (class default true).
    pub turn_gate: bool,
    pub can_hit: bool,
    /// CruseControlActive.
    pub cruise: bool,
    pub jump_gate: bool,
    pub can_jump: bool,
    /// Percent Charged.
    pub pct: Real,
    /// Ticks left of RetriggerableDelay(0.2) -> @15 and (0.1) -> @81, 0: none.
    pub turn_wait: Real,
    pub hit_wait: Real,
    /// In ReceiveEndPlay (IsActorBeingDestroyed).
    pub ending: bool,
    /// TeamIndex.
    pub team: i32,
    /// BaseMaxAceleration.
    pub base_max_accel: Real,
    /// The fighters whose InputTriggered -> ButtonPressed is bound.
    pub bound: Vec<FighterId>,
    /// PickUpBuffs.
    pub pick_ups: Vec<PickUp>,
    /// The PickUpBuffs whose OnEndPlay -> CarryBuffDestroyed has not run yet.
    pub watch: Vec<BuffRef>,
    /// The fighters (their ids) the HitBox and the JumpOnCollision touched at the last test.
    pub inside_hit: Vec<i32>,
    pub inside_jump: Vec<i32>,
    /// ActorsHitCooldown: its keys (fighter ids) and their GetGameTimeInSeconds.
    pub hit_ids: Vec<i32>,
    pub hit_times: Vec<Real>,
    /// The owner's knockedBackCount at the last step, and HitStunStart's samples.
    pub kb_seq: i32,
    pub stun_was: bool,
    pub stun_age: Real,
}

impl Default for VelmaCarryRun {
    fn default() -> Self {
        Self {
            base: HitPacket::default(),
            hit: HitPacket::default(),
            attack_id: 0,
            x: 0.0,
            y: 0.0,
            section: Name::NONE,
            jump_on_ally: None,
            jump_on_ui: false,
            carried_ally: None,
            carried_enemy: None,
            carry_int: 0,
            facing_left: false,
            turn_gate: true,
            can_hit: false,
            cruise: false,
            jump_gate: false,
            can_jump: false,
            pct: 0.0,
            turn_wait: 0.0,
            hit_wait: 0.0,
            ending: false,
            team: 0,
            base_max_accel: 0.0,
            bound: Vec::new(),
            pick_ups: Vec::new(),
            watch: Vec::new(),
            inside_hit: Vec::new(),
            inside_jump: Vec::new(),
            hit_ids: Vec::new(),
            hit_times: Vec::new(),
            kb_seq: 0,
            stun_was: false,
            stun_age: 0.0,
        }
    }
}

/// The run's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &VelmaCarryRun) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The run's DealerActor parts: its place, the owner's facing (the actor's velocity is 0, so a facing hit takes the
/// facing) and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &VelmaCarryRun) -> (Real, Real, Real, i32) {
    (s.x, s.y, facing(w, a.owner), s.attack_id)
}

/// The run's HitBoxDamage packet (`hit`).
pub fn hit(s: &VelmaCarryRun) -> &HitPacket {
    &s.hit
}

/// The release packet of ReceiveEndPlay @3821-4854 (the rest of FHitBoxDamage at its zero / false / None).
fn release_packet() -> HitPacket {
    HitPacket {
        sparse: true,
        damage: Some(7.0),
        base_knock_back: Some(1800.0),
        knock_back_scaling: Some(14.0),
        knock_back_direction_vector: Some(DataVec { x: Some(-0.35), y: Some(1.0), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(4)),
        hit_pause_time: Some(0.25),
        hit_stun_mult: Some(1.0),
        on_hit_effect_override: Some(name!("PS_Velma_HitThrow")),
        directional_knockback: Some(false),
        velocity_instead_of_facing: Some(false),
        knock_back_mirror: Some(true),
        ignore_facing: Some(false),
        attack_id: Some(0),
        armor_pierce: Some(true),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        hit_each_actor_only_once: Some(false),
        account_for_velocity_with_vacuum_knockback: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + ((b - a) * t)
}

fn st(w: &World, id: ActorId) -> &VelmaCarryRun {
    match &w.actors.actors.get(id).expect("a live world actor").state {
        WorldActorState::VelmaCarryRun(s) => s,
        _ => panic!("a VelmaCarryRun_Actor_C state"),
    }
}

fn st_mut(w: &mut World, id: ActorId) -> &mut VelmaCarryRun {
    match &mut w.actors.actors.get_mut(id).expect("a live world actor").state {
        WorldActorState::VelmaCarryRun(s) => s,
        _ => panic!("a VelmaCarryRun_Actor_C state"),
    }
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live world actor").owner
}

fn is_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn kill(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn valid(w: &World, f: Option<FighterId>) -> bool {
    f.is_some_and(|f| w.fighter(f).life.dead <= 0.0)
}

fn live(w: &World, f: FighterId, b: BuffRef) -> bool {
    let _ = f;
    buff(w, b).is_some_and(|x| x.frames > 0.0)
}

/// The sound event on the World's sink.
fn sound(w: &mut World, f: FighterId, ev: Name, x: Real, y: Real) {
    let fighter = w.fighter(f).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

/// `follow`: the root at the owner's actor location.
fn follow(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let (x, y) = (w.fighter(o).body.x, actor_y(w, o));
    let s = st_mut(w, id);
    s.x = x;
    s.y = y;
}

/// BPI_Velma BPI_IsCarrying on the owner (a failed cast: nothing).
fn is_carrying(w: &mut World, f: FighterId, on: bool) {
    if velma_of(w, f).is_some() {
        set_carrying(w, f, on);
    }
}

/// BaseFighter IsHoldingAlly (BPI_GetIsHoldingAlly). Only this class writes it on a Velma; any other fighter's is
/// false here.
pub fn is_holding_ally(w: &World, f: FighterId) -> bool {
    velma_of(w, f).is_some_and(|v| v.carry.holding_ally)
}

fn set_holding_ally(w: &mut World, f: FighterId, on: bool) {
    if velma_of(w, f).is_some() {
        velma_w(w, f).carry.holding_ally = on;
    }
}

/// The live carry run of a Velma (VelmaState.carryRun), none for none.
pub fn carry_run_of(w: &World, f: FighterId) -> Option<ActorId> {
    match velma_of(w, f)?.carry_run {
        Some(ActorRef::Actor(id))
            if !is_dead(w, id) && matches!(w.actors.actors.get(id)?.state, WorldActorState::VelmaCarryRun(_)) =>
        {
            Some(id)
        }
        _ => None,
    }
}

/// PlayerDataManager.GetByFighter(it).GetTeamIndex (see the header): a minion's is 0.
fn team_of(w: &World, f: FighterId) -> i32 {
    let fi = w.fighter(f);
    if fi.life.is_minion { 0 } else { fi.team.team }
}

/// UpdateCarryNumber. `sink_world`: false from a FighterSignal (no World): the post goes through the owner's events.
fn update_carry_number(w: &mut World, id: ActorId, add: bool, sink_world: bool) {
    if !add {
        st_mut(w, id).carry_int -= 1;
        return;
    }
    st_mut(w, id).carry_int += 1;
    let o = owner_of(w, id);
    let (fighter, x, y) = (w.fighter(o).id, w.fighter(o).body.x, actor_y(w, o));
    let ev = Event::Sound {
        fighter,
        ev: name!("sfx_cha_velma_holdpose_grab"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    };
    if sink_world {
        w.events.push(ev);
    } else {
        w.fighter_mut(o).events.push(ev);
    }
}

/// A PickUpBuffs buff with its OnEndPlay bound.
fn pick_up(w: &mut World, id: ActorId, f: FighterId, b: BuffRef) {
    let s = st_mut(w, id);
    s.watch.push(b);
    s.pick_ups.push(PickUp { f, b });
}

fn unbind(w: &mut World, id: ActorId, f: Option<FighterId>) {
    let Some(f) = f else { return };
    let s = st_mut(w, id);
    if let Some(i) = s.bound.iter().position(|x| *x == f) {
        s.bound.remove(i);
    }
}

/// @8270 (JumpOnCollision's EndOverlap, RemoveJumpOnAlly): ButtonPressed unbound from JumpOnAlly, the prompt off,
/// BPI_IsCarrying(false).
fn drop_jump_on(w: &mut World, id: ActorId) {
    let j = st(w, id).jump_on_ally;
    unbind(w, id, j);
    st_mut(w, id).jump_on_ui = false;
    let o = owner_of(w, id);
    is_carrying(w, o, false);
}

/// RemoveJumpOnAlly @19000.
fn remove_jump_on_ally(w: &mut World, id: ActorId) {
    if valid(w, st(w, id).jump_on_ally) {
        drop_jump_on(w, id);
    }
}

/// CarryBuffDestroyed @18407.
fn carry_buff_destroyed(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    update_carry_number(w, id, false, false);
    {
        let s = st_mut(w, id);
        s.carried_ally = None;
        s.carried_enemy = None;
    }
    is_carrying(w, o, false);
    if let Some(slot) = active_instance(w, o) {
        slot_jump_to_section(w, o, slot, name!("RunEnd")); // NotifyChangeSection(RunEnd, null): the active montage
    }
    let s = st_mut(w, id);
    s.section = name!("RunEnd");
    if !s.ending {
        kill(w, id);
    }
}

/// The OnEndPlay -> CarryBuffDestroyed binding of the buffs that ended since the last look.
fn watch_buffs(w: &mut World, id: ActorId) {
    let mut i = 0;
    while i < st(w, id).watch.len() {
        let b = st(w, id).watch[i];
        let on = st(w, id).pick_ups.iter().find(|x| x.b == b).map(|x| x.f);
        if on.is_some_and(|f| live(w, f, b)) {
            i += 1;
            continue;
        }
        st_mut(w, id).watch.remove(i);
        carry_buff_destroyed(w, id);
    }
}

/// A PickUpBuffs buff's EndPlay, for the hook that ends it: CarryBuffDestroyed at once rather than at the actor's next
/// step. `by`: the buff's Instigator (the Velma).
pub fn carry_buff_ended(w: &mut World, by: FighterId, b: BuffRef) {
    let Some(ActorRef::Actor(id)) = velma_of(w, by).and_then(|v| v.carry_run) else { return };
    if !matches!(w.actors.actors.get(id).map(|a| &a.state), Some(WorldActorState::VelmaCarryRun(_))) {
        return;
    }
    let s = st(w, id);
    if is_dead(w, id) && !s.ending {
        return;
    }
    let Some(i) = s.watch.iter().position(|x| *x == b) else { return };
    st_mut(w, id).watch.remove(i);
    carry_buff_destroyed(w, id);
}

/// Fall @18708: the owner's NotifyChangeSection(Fall, Velma_Special_Pickup_HoldPose_Montage) (JumpToSectionLogic: that
/// montage when it plays, else the active one), BPI_IsCarrying(false). Its callers (the destructible branches) are not
/// ported.
pub fn fall(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    let on_pose = w
        .fighter(o)
        .montages
        .montage
        .as_ref()
        .is_some_and(|m| m.montage.name == HOLD_POSE && !m.ended && !m.terminated);
    if on_pose {
        nav_jump(w, o, MontageSlot::Main, name!("Fall"));
    } else if let Some(slot) = active_instance(w, o) {
        slot_jump_to_section(w, o, slot, name!("Fall"));
    }
    st_mut(w, id).section = name!("Fall");
    is_carrying(w, o, false);
}

/// `runSpawn`.
fn run_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let ad = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let _ = spawn_place(w, f, &np);
    let p = &ad.p;
    let base = HitPacket::from_props(&w.names, obj(p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None);
    let fi = w.fighter(f);
    let s = VelmaCarryRun {
        hit: base.clone(),
        base,
        attack_id: r.attack_id,
        x: fi.body.x,
        y: actor_y(w, f),
        turn_gate: p.get("TurnGate").and_then(Value::as_bool).unwrap_or(true),
        pct: num_or(p, "Percent Charged", 0.0),
        team: fi.team.team,
        base_max_accel: max_accel(w, f),
        kb_seq: fi.attackers.knocked_back_count,
        stun_was: fi.buffs.buff_stunned,
        stun_age: fi.buffs.buff_stun_age,
        ..VelmaCarryRun::default()
    };
    is_carrying(w, f, false); // @1876
    (WorldActorState::VelmaCarryRun(s), false)
}

/// The listed actor is VelmaState.carryRun.
fn run_spawned(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    if velma_of(w, o).is_some() {
        velma_w(w, o).carry_run = Some(ActorRef::Actor(id));
    }
}

/// `runActivate`: ActivateObject -> SetMovementVariableMulticast @1354.
fn run_activate(w: &mut World, id: ActorId) {
    if is_dead(w, id) {
        return;
    }
    let o = owner_of(w, id);
    let arg = min(w.fighter(o).charge.charge_pct, 1.0);
    st_mut(w, id).can_hit = true;
    check_to_hit(w, id);
    if is_dead(w, id) {
        return; // @117
    }
    {
        let s = st_mut(w, id);
        s.jump_gate = true;
        s.can_jump = true;
        s.cruise = true;
    }
    w.fighter_mut(o).air.air_control = 1.0;
    let accel = num_or(actor_p(w, ActorRef::Actor(id)), "MaxAcceleration", 0.0);
    set_max_accel(w, o, accel);
    st_mut(w, id).pct = min(arg, 1.0);
    spawn_buff(w, o, CARRY_SPEED_N, 0.0, Some(0), Some(o), true, Some(o));
    let (x, y) = (st(w, id).x, st(w, id).y);
    sound(w, o, name!("sfx_cha_jake_foley_horse_footstep"), x, y);
}

/// HitstunHandler.HitStunStart or DamageManager.KnockedBack of the owner since the last step (see the header).
fn interrupted(w: &mut World, id: ActorId) -> bool {
    let o = owner_of(w, id);
    let (kbc, stunned, age) = {
        let fi = w.fighter(o);
        (fi.attackers.knocked_back_count, fi.buffs.buff_stunned, fi.buffs.buff_stun_age)
    };
    let s = st_mut(w, id);
    let stun_new = stunned && (!s.stun_was || age <= s.stun_age);
    let kb = kbc != s.kb_seq;
    s.kb_seq = kbc;
    s.stun_was = stunned;
    s.stun_age = age;
    kb || stun_new
}

/// `runStep`.
fn run_step(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    follow(w, id);
    watch_buffs(w, id);
    if is_dead(w, id) {
        return;
    }
    if interrupted(w, id) {
        kill(w, id); // HitStunned / KnockedBack -> K2_DestroyActor
        return;
    }
    let s = st(w, id);
    if s.cruise && w.fighter(o).life.dead <= 0.0 && s.turn_gate && (grounded(w, o) || is_falling(w, o)) {
        let fc = facing(w, o);
        let m = &mut w.fighter_mut(o).air.move_in;
        *m = max(-1.0, min(1.0, *m + if fc < 0.0 { -1.0 } else { 1.0 })); // AddMovementInput @1107
        let left = fc < 0.0;
        let s = st_mut(w, id);
        if s.facing_left != left {
            s.facing_left = left;
            s.turn_gate = false;
            s.can_hit = false;
            s.turn_wait = latent_frames(0.2); // RetriggerableDelay: a pending one starts over
        }
    }
    // The latent RetriggerableDelays, behind the tick.
    let s = st_mut(w, id);
    if s.turn_wait > 0.0 {
        s.turn_wait -= 1.0;
        if s.turn_wait == 0.0 {
            s.turn_gate = true; // @15
            s.hit_wait = latent_frames(0.1);
        }
    }
    let s = st_mut(w, id);
    if s.hit_wait > 0.0 {
        s.hit_wait -= 1.0;
        if s.hit_wait == 0.0 && s.turn_gate {
            s.can_hit = true; // @81
            check_to_hit(w, id);
        }
    }
}

fn touches(w: &World, id: ActorId, sh: &Sphere, v: FighterId) -> bool {
    if w.fighter(v).life.dead > 0.0 || no_collision(w, v) {
        return false;
    }
    let k = w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0); // the spheres under the root, at its scale
    let s = st(w, id);
    let side = if facing(w, owner_of(w, id)) < 0.0 { -1.0 } else { 1.0 };
    sphere_hits(w, s.x + sh.x * k * side, s.y + sh.y * k, sh.r * k, v)
}

/// A minion other than Steven's (the BP_FighterCharacter cast and BPI_GetFighterCharacter 14) is passed over.
fn npc_passes(w: &World, v: FighterId) -> bool {
    let fi = w.fighter(v);
    !fi.life.not_player_character || w.names.str(fi.ch) == crate::chars::steven::KEY
}

/// The hit of a fighter (CarryInt >= 1, @13123-14008 / @16461-17106).
fn damage(w: &mut World, id: ActorId, vic: FighterId) {
    let o = owner_of(w, id);
    let time = w.fighter(o).clock.time;
    let vid = w.fighter(vic).id;
    let fc = facing(w, o);
    let (dm, km) = {
        let p = actor_p(w, ActorRef::Actor(id));
        (num_or(p, "Damage Multiplier", 1.0), num_or(p, "Knock Back Multiplier", 1.0))
    };
    let s = st_mut(w, id);
    s.hit_ids.push(vid);
    s.hit_times.push(time);
    let t = min(s.pct, 1.0);
    let bkb = s.base.base_knock_back.unwrap_or(0.0);
    let dmg = s.base.damage.unwrap_or(0.0);
    s.hit.damage = Some(lerp(dmg, dmg * dm, t));
    s.hit.base_knock_back = Some(lerp(bkb, bkb * km, t));
    s.hit.knock_back_direction_vector =
        Some(DataVec { x: Some(if fc < 0.0 { -1.0 } else { 1.0 }), y: Some(1.0), z: None });
    let hit = s.hit.clone();
    let me = ActorRef::Actor(id);
    let d = dealer(w, me);
    let pct = charge_pct(w, me);
    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
    if apply_projectile_hit(w, &d, vic, &hit, &[], at, None, &[], pct) {
        actor_hit_enemy(w, &d, vic);
    }
}

/// The capture (CarryInt < 1). `from_check`: CheckToHit (RemoveJumpOnAlly after the validity test) or the BeginOverlap
/// (ahead of it).
fn capture(w: &mut World, id: ActorId, vic: FighterId, from_check: bool) {
    {
        let s = st_mut(w, id);
        s.jump_on_ally = Some(vic);
        s.carried_enemy = Some(vic);
    }
    let o = owner_of(w, id);
    let b = spawn_buff(w, vic, ATTACH_TO_ALLY_N, 0.0, Some(0), Some(o), true, Some(vic));
    if !from_check {
        if st(w, id).ending {
            if let Some(b) = b {
                end_buff(w, vic, b); // @17584
            }
            return;
        }
        remove_jump_on_ally(w, id); // @17621
    }
    let Some(b) = b else { return };
    if !live(w, vic, b) {
        return;
    }
    w.fighter_mut(o).holds.holding_enemy = true;
    pick_up(w, id, vic, b);
    // capture.rs's part of the enemy's capture: the Buff_VelmaAttachToAlly is on the victim, spawned above.
    start_capture(w, o, vic, ActorRef::Actor(id));
    if from_check {
        remove_jump_on_ally(w, id); // @15073
    }
    update_carry_number(w, id, true, true);
    is_carrying(w, o, true); // @15177 / @17929
}

/// CheckToHit @10260: every fighter the HitBox overlaps now.
fn check_to_hit(w: &mut World, id: ActorId) {
    follow(w, id);
    let o = owner_of(w, id);
    for v in w.all_ids() {
        if is_dead(w, id) && !st(w, id).ending {
            return;
        }
        if v == o || !touches(w, id, &HITBOX, v) || !npc_passes(w, v) || invulnerable(w, v) {
            continue;
        }
        let s = st(w, id);
        if w.fighter(v).team.team == s.team || s.hit_ids.contains(&w.fighter(v).id) {
            continue;
        }
        if s.carry_int >= 1 {
            if s.carried_enemy != Some(v) {
                damage(w, id, v);
            }
        } else {
            capture(w, id, v, true);
        }
    }
}

/// The HitBox's BeginOverlap @15416, for a fighter that came in.
fn hit_box_begin(w: &mut World, id: ActorId, v: FighterId) {
    let s = st(w, id);
    if !npc_passes(w, v) || invulnerable(w, v) || !s.can_hit {
        return;
    }
    if w.fighter(v).team.team == s.team || s.hit_ids.contains(&w.fighter(v).id) {
        return;
    }
    if s.carry_int >= 1 {
        damage(w, id, v);
    } else {
        capture(w, id, v, false);
    }
}

/// JumpOnCollision's BeginOverlap @6224.
fn jump_on_begin(w: &mut World, id: ActorId, v: FighterId) {
    if valid(w, st(w, id).carried_enemy) {
        return;
    }
    if team_of(w, v) != st(w, id).team {
        return;
    }
    let s = st_mut(w, id);
    s.jump_on_ally = Some(v);
    s.jump_on_ui = true;
    if !s.bound.contains(&v) {
        s.bound.push(v);
    }
    if !s.cruise {
        let o = owner_of(w, id);
        is_carrying(w, o, true); // @6795
    }
}

/// JumpOnCollision's EndOverlap @8147.
fn jump_on_end(w: &mut World, id: ActorId, v: FighterId) {
    if st(w, id).jump_on_ally == Some(v) {
        drop_jump_on(w, id);
    }
}

/// `runHits`: the fighters each sphere touches, kept from the last test; the begin and end edges handed on.
fn run_hits(w: &mut World, id: ActorId) {
    follow(w, id);
    let o = owner_of(w, id);
    let list = w.all_ids();
    for &v in &list {
        if is_dead(w, id) {
            return;
        }
        if v == o {
            continue;
        }
        let was = st(w, id).inside_hit.iter().position(|x| *x == w.fighter(v).id);
        if !touches(w, id, &HITBOX, v) {
            if let Some(i) = was {
                st_mut(w, id).inside_hit.remove(i);
            }
            continue;
        }
        if was.is_some() {
            continue;
        }
        {
            let vid = w.fighter(v).id;
            st_mut(w, id).inside_hit.push(vid);
        }
        hit_box_begin(w, id, v);
    }
    for &v in &list {
        if is_dead(w, id) {
            return;
        }
        if v == o {
            continue;
        }
        let was = st(w, id).inside_jump.iter().position(|x| *x == w.fighter(v).id);
        let now = touches(w, id, &JUMPON, v);
        if now == was.is_some() {
            continue;
        }
        if now {
            let vid = w.fighter(v).id;
            st_mut(w, id).inside_jump.push(vid);
            jump_on_begin(w, id, v);
        } else if let Some(i) = was {
            st_mut(w, id).inside_jump.remove(i);
            jump_on_end(w, id, v);
        }
    }
}

/// ButtonPressed @6834 (a bound fighter's InputTriggered; no World in a FighterSignal).
fn button_pressed(w: &mut World, id: ActorId, kind: Real) {
    if valid(w, st(w, id).carried_enemy) || kind != 0.0 {
        return;
    }
    let o = owner_of(w, id);
    let ally = st(w, id).jump_on_ally;
    st_mut(w, id).carried_ally = ally;
    let kind_n = name!("VelmaCarryTempHealth");
    add_temp_health_pack(w, o, kind_n, 4.0, true, 3.0);
    let Some(ally) = ally else { return }; // the calls on None do nothing: no buff
    add_temp_health_pack(w, ally, kind_n, 4.0, true, 3.0);
    let Some(b) = spawn_buff(w, ally, ATTACH_TO_ALLY_N, 0.0, Some(0), Some(o), true, Some(ally)) else { return };
    if !live(w, ally, b) {
        return;
    }
    set_holding_ally(w, o, true);
    spawn_buff(w, ally, ARMOR_PACK, 3.0, Some(1), Some(o), true, Some(ally));
    spawn_buff(w, o, ARMOR_PACK, 3.0, Some(1), Some(o), true, Some(o));
    pick_up(w, id, ally, b);
    update_carry_number(w, id, true, false);
    is_carrying(w, o, true); // @7866
    unbind(w, id, Some(ally));
    st_mut(w, id).jump_on_ui = false;
}

/// The run's fighterSignal: a bound fighter's InputTriggered -> `button_pressed`.
#[allow(clippy::too_many_arguments, reason = "the hook's signature")]
fn run_fighter_signal(
    w: &mut World,
    a: ActorId,
    f: FighterId,
    sig: FighterSignal,
    flag: bool,
    num: Real,
    name: Name,
    other: Option<FighterId>,
) {
    let _ = (flag, name, other);
    if sig == FighterSignal::InputTriggered && !is_dead(w, a) && st(w, a).bound.contains(&f) {
        button_pressed(w, a, num);
    }
}

/// `runEnd`: ReceiveEndPlay @18912 -> @9705.
fn run_end(w: &mut World, id: ActorId) {
    let o = owner_of(w, id);
    st_mut(w, id).ending = true;
    // (1) PickUpBuffs @2617.
    let mut i = 0;
    while i < st(w, id).pick_ups.len() {
        let PickUp { f, b } = st(w, id).pick_ups[i];
        i += 1;
        if !live(w, f, b) || buff(w, b).is_none_or(|x| x.cls != ATTACH_TO_ALLY_N) {
            continue;
        }
        let (ally, enemy) = (st(w, id).carried_ally, st(w, id).carried_enemy);
        if let (true, Some(ally)) = (valid(w, ally), ally) {
            let side = if w.fighter(ally).team.team != w.fighter(o).team.team {
                0.0
            } else if facing(w, o) < 0.0 {
                -1000.0
            } else {
                1000.0
            }; // UE Y
            let m = &mut w.fighter_mut(ally).moves;
            // LaunchCharacter((0, Y, 2500), false, true)
            m.impulse = Some(Impulse { vx: -side, vy: 2500.0, xy: false, z: true, kind: 0, d: None });
            m.hit_launch = true; // a direct LaunchCharacter
        } else if let (true, Some(enemy)) = (valid(w, enemy), enemy) {
            let me = ActorRef::Actor(id);
            let d = dealer(w, me);
            let pct = charge_pct(w, me);
            let (ex, ey) = (w.fighter(enemy).body.x, actor_y(w, enemy));
            apply_projectile_hit(w, &d, enemy, &release_packet(), &[], V2 { x: ex, y: ey }, None, &[], pct);
            sound(w, enemy, name!("sfx_cha_velma_holdpose_hit"), ex, ey);
        }
        end_buff(w, f, b); // DestroyOnServer @3745: its OnEndPlay -> CarryBuffDestroyed
        if let Some(k) = st(w, id).watch.iter().position(|x| *x == b) {
            st_mut(w, id).watch.remove(k);
            carry_buff_destroyed(w, id);
        }
    }
    // (2) @9930.
    set_holding_ally(w, o, false);
    w.fighter_mut(o).holds.holding_enemy = false;
    w.fighter_mut(o).air.air_control = AIR_CONTROL; // BaseAirControl: BaseFighter's 0.9 (Pawn_Velma sets no other)
    let base = st(w, id).base_max_accel;
    set_max_accel(w, o, base);
    let (x, y) = (st(w, id).x, st(w, id).y);
    sound(w, o, name!("mix_stop_all_fade_eighthofsecond"), x, y);
    if let Some(sp) = buff_of(w, o, CARRY_SPEED_N) {
        end_buff(w, o, sp);
    }
    // (3) @9854 InputUI.Exit.
    st_mut(w, id).jump_on_ui = false;
    // (4) @9725.
    update_carry_number(w, id, false, true);
    is_carrying(w, o, false);
    st_mut(w, id).bound.clear();
    if velma_of(w, o).is_some_and(|v| v.carry_run == Some(ActorRef::Actor(id))) {
        velma_w(w, o).carry_run = None;
    }
}

/// UserConstructionScript, ahead of BeginPlay (SpawnBuffActor's UpdatePlayerStatus reads the result).
fn speed_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, d: Option<Real>) {
    let _ = (from, d);
    let class_speed = inst_view(w, b).num("SpeedMod").unwrap_or(1.0);
    let t = min(w.fighter(f).charge.charge_pct, 1.0);
    let mut v = mvs_data::value::Props::new();
    v.insert("SpeedMod".into(), Value::Num(f64::from(lerp(1.0, class_speed, t))));
    set_buff_values(w, b, &v);
    set_cmc_friction(w, f, 10.0, 10000.0, 10000.0);
}

fn speed_end(w: &mut World, f: FighterId, b: BuffRef) {
    let _ = b;
    set_cmc_friction(w, f, GROUND_FRICTION, 0.0, BRAKING_FRICTION_FACTOR);
}

/// This file's registry entries: VelmaCarryRun_Actor_C, Buff_Velma_Carry_Speed_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        CARRY_RUN,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::VelmaCarryRunActor),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(run_spawn),
                spawned: Some(run_spawned),
                activate: Some(run_activate),
                step: Some(run_step),
                hits: Some(run_hits),
                fighter_signal: Some(run_fighter_signal),
                end: Some(run_end),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.buffs.enter(
        CARRY_SPEED,
        BuffEntry::new(
            BuffClass::BuffVelmaCarrySpeed,
            BuffHooks { begin: Some(speed_begin), end: Some(speed_end), ..BuffHooks::default() },
        ),
    )
}
