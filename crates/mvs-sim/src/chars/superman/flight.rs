//! Superman's flights: the notify state AnimNotify_SupermanFlight, Superman_AnimBP's
//! update of the flight inputs (CharHooks.anim_update) and its ModifyBone_6 turn over the clip's shapes
//! (CharHooks.pose), and the actors SupermanFlightHit_C and SupermanGrabListener_C ([`FlightActor`]: the grab, the
//! carry and the throws); its hooks hit_enemy and knocked_back.
//!
//! The notify state: a notify state of Superman_Flying_Montage (0.3433-2.8937) alone.
//! Begin: Superman_AnimBP.AngleToVelocityAlpha = 1 @121, DirectionSoft = Direction @166, FlyAngle = Direction @237;
//! the Base_Fighter's BlockLandReset = true @429, SetCapsuleHalfHeight(150, true) @462, SetCapsuleRadius(120, true)
//! @522. End: AngleToVelocityAlpha = 0 @121; the Pawn_Superman's BlockLandReset = false @287,
//! CharacterMovement.SetMovementMode(Falling) @320, SetCapsuleHalfHeight(SupermanCapsuleHight) @382,
//! SetCapsuleRadius(SupermanCapsuleRadius) @468 (the sizes his BeginPlay saved: `SupermanState.cap_half` /
//! `cap_radius`). The End runs wherever the montage's notify states end: its own time, a stop (a knockback hit, a
//! ring-out's spawn). SetMovementMode(Falling) as AnimNotify_SetFlyingMoveMode's End has it: Flying, the knockback
//! mode and Walking all become Falling, and a floor he stood on gives its velocity.
//!
//! The actors: SpawnedActorParent children with no shape of their own that the montage's Spawner attaches to him.
//! Each binds, at BeginPlay, his Comp_Pawn_HitBoxHandler.HitEnemyPlayer and his DamageManager.KnockedBack.
//!   HitEnemyPlayer (his own hit went through: CharHooks.hit_enemy), behind SupermanNotHoldingAllyCondition (true):
//!     FlightHit: his BPI_GetPlayerAttackCount in his LandedAttackIDs, else nothing; a victim with
//!       Buff_StevenBubbleEnemy: the hit listener unbound, Set Holding Enemy(true), no buff; else a non-NPC fighter:
//!       HitPlayer = the victim, SpawnBuffActor(Buff_HeldByEnemy, the victim, him, stacks 0, 1.0 s); a valid buff:
//!       SpawnedBuff, its OnDestroyed bound, the hit listener unbound, Set Holding Enemy(true).
//!     GrabListener: HitPlayer = the victim first, whatever follows; the bubble: nothing (the listener stays, no
//!       holding); then the attack-count test, the buff and the rest as above.
//!   ActivateObject (the montage's ActivateSpawned): SpawnedBuff invalid or being destroyed: his
//!     Comp_Pawn_MontageNavigation.NotifyChangeSection(End, null), K2_DestroyActor. Else OnDestroyed unbound, the
//!     buff destroyed, and the actor's own handler's pfgDoDamage(null, HitPlayer) with a packet built there:
//!     FlightHit its FLIGHT packet, then K2_DestroyActor; GrabListener, for each true row of its
//!     ActivationConditions (the loop has no break, but HitOnlyOneFighter lets the first packet alone through; the
//!     montage's four ActivateSpawned set one row each: 0 Throw_U, 1 Throw_D, 2 Throw_B, 3 Throw_F), the row's throw
//!     packet and K2_DestroyActor; with no true row the buff is gone and the actor stays.
//!   OnGrappleBuffDestroyed (the buff ended on its own: its 1.0 s, a break, the victim's ring-out):
//!     NotifyChangeSection(End, null), K2_DestroyActor.
//!   KnockedBack (his): K2_DestroyActor, no packet.
//!   ReceiveEndPlay: a valid SpawnedBuff unbound and destroyed; both delegates unbound; Set Holding Enemy(false).
//! The held state is the shared model's (`combat/holds.rs`: Buff_HeldByEnemy on his FlightGrabSocket).
//! Model: the packet goes through [`apply_projectile_hit`] as every spawned actor's (no hit pause for him: the class
//! has no ApplyHitPauseToOwner; the vector's X to his facing side, the attached actor's); the actor's AttackID is
//! his attack count at the spawn.
// Deviation: OnDestroyed is found by the actor's step (after every fighter's tick of the frame the buff ended in),
// not inside the buff's destroy; an actor ended by the Spawner's notify end or with him runs its EndPlay in the
// world's end pass of that frame (his holding flag and the buff last until then).
// Unverified: the bubble branches (Buff_StevenBubbleEnemy, Steven's) are written from the graph. A SpawnBuffActor
// that returns no new actor (blocked, or stacked on a live hold) is taken as invalid. ForceKnockback of the packets is
// read by nothing in the port's spawned-actor hit path (HitReaction is: the victim's rumble,
// [`apply_projectile_hit`]); HitOnlyOneFighter is kept by dealing one packet per actor.

use super::state::{superman_of, superman_w};
use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::spawn_buff;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::holds::end_hold_buff;
use crate::combat::{DataEnum, DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::anim::box_pose::turn_boxes;
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::slot_jump_to_section;
use crate::fighter::anim::update::anim_direction;
use crate::fighter::body::impart_base;
use crate::fighter::common::actor_data;
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::moves::{set_capsule_half_height, set_capsule_radius};
use crate::fighter::{FighterId, actor_y, facing, grounded, invulnerable, set_grounded};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::arr;
use crate::registry::{CharHooks, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

/// The capsule half height and radius the flight notify sets (SetCapsuleHalfHeight @462, SetCapsuleRadius @522).
pub const FLIGHT_HALF: Real = 150.0;
pub const FLIGHT_RADIUS: Real = 120.0;

const FLIGHT_HIT: &str = "SupermanFlightHit_C";
const GRAB_LISTENER: &str = "SupermanGrabListener_C";

// ---- AnimNotify_SupermanFlight

fn flight_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let s = superman_w(w, f);
    s.angle_to_velocity_alpha = 1.0;
    s.direction_soft = s.direction;
    s.fly_angle = s.direction;
    w.fighter_mut(f).crouch.block_land_reset = true;
    set_capsule_half_height(w, f, FLIGHT_HALF);
    set_capsule_radius(w, f, FLIGHT_RADIUS);
}

fn flight_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let (half, radius) = {
        let s = superman_w(w, f);
        s.angle_to_velocity_alpha = 0.0;
        (s.cap_half, s.cap_radius)
    };
    w.fighter_mut(f).crouch.block_land_reset = false;
    let b = if grounded(w, f) { w.fighter(f).platforms.base } else { None };
    let fi = w.fighter_mut(f);
    fi.moves.flying = false;
    fi.hit.kb_mode = false;
    set_grounded(w, f, false);
    if let Some(b) = b {
        impart_base(w, f, b);
    }
    set_capsule_half_height(w, f, half);
    set_capsule_radius(w, f, radius);
}

// ---- Superman_AnimBP's flight inputs

/// Superman_AnimBP's update of the flight inputs (BlueprintUpdateAnimation, ubergraph @7259-7894, every update with a
/// valid Fighter):
///   Direction = GetDirection() (the base's: CalculateDirection of the fighter's velocity, negated facing left);
///   DirectionSoft = HorizontalSpeed <= 10 ? -90 : FInterpTo_Constant(DirectionSoft, Direction, DeltaTimeX, 180);
///   FlyAngle = IsMovingOnGround ? Direction : FInterpTo(FlyAngle, Direction, DeltaTimeX, 6);
///   FlyLean = HorizontalSpeed <= 10 ? 0 : Direction - FlyAngle.
/// HorizontalSpeed (@2648, earlier in the same update) is the speed along the play plane's horizontal, |vx|.
/// FMath::FInterpConstantTo / FInterpTo: the target when (Target - Current)^2 < 1e-8; else Current + Clamp(Dist,
/// -180 DT, 180 DT), and Current + Dist x Clamp(DT x 6, 0, 1). DeltaTimeX carries the pause's dilation.
fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    if !pre {
        return;
    }
    let (vx, vy) = {
        let b = &w.fighter(f).body;
        (b.vx, b.vy)
    };
    let dir = anim_direction(w, f, vx, vy);
    let on_floor = grounded(w, f);
    let slow = vx.abs() <= 10.0;
    let dt = DELTA_SECONDS * dilation;
    let s = superman_w(w, f);
    s.direction = dir;
    if slow {
        s.direction_soft = -90.0;
    } else {
        let d = dir - s.direction_soft;
        if d * d < 1e-8 {
            s.direction_soft = dir;
        } else {
            let step = 180.0 * dt;
            s.direction_soft += min(max(d, -step), step);
        }
    }
    if on_floor {
        s.fly_angle = dir;
    } else {
        let d = dir - s.fly_angle;
        s.fly_angle = if d * d < 1e-8 { dir } else { s.fly_angle + (d * min(max(dt * 6.0, 0.0), 1.0)) };
    }
    s.fly_lean = if slow { 0.0 } else { dir - s.fly_angle };
}

/// The pitch in degrees ModifyBone_6 gives the bone fml_un_COM, `None` while its Alpha (AngleToVelocityAlpha) is 0.
/// AnimGraph: the node (BoneToModify fml_un_COM, RotationMode BMM_Replace, component space) sits over the montage
/// slots; its handler @5549-6102: Rotation = MakeRotator(0, Pitch, 0), Alpha = AngleToVelocityAlpha,
///   Pitch = ((Velocity.Y >= 0 && FacingLeft) || (!FacingLeft && Velocity.Y <= 0)) ? DirectionSoft + 90
///                                                                                 : DirectionSoft - 90
/// with the owning actor's velocity of that evaluation (UE Y = -x: the velocity toward the facing, or none). Level
/// flight toward the facing is DirectionSoft -90: no turn.
pub fn flight_pitch(w: &World, f: FighterId) -> Option<Real> {
    let s = superman_of(w, f)?;
    if s.angle_to_velocity_alpha != 1.0 {
        return None;
    }
    let vx = w.fighter(f).body.vx;
    let to_facing = if facing(w, f) < 0.0 { vx <= 0.0 } else { vx >= 0.0 };
    Some(if to_facing { s.direction_soft + 90.0 } else { s.direction_soft - 90.0 })
}

/// His `pose`: the turn of ModifyBone_6 over the clip's shapes, as [`turn_boxes`] turns Base_AnimBP's hit rotation
/// (the same bone, mode and space: what rides the bone turns about the bone's point in the play plane). True while the
/// flight notify is on: the base's hit rotation is not laid over it (a knockback hit stops the montage and the notify
/// with it).
// Unverified: the bone's own component rotation in the flight clip is taken as the identity (as in the fly clips the
// hit rotation was read on); ApplyAdditive of SupermanFlightAdditive_BlendSpace (X = FlyLean, -35..35) over the
// turned pose is not laid on the shapes (the box tracks carry clips' shapes, no additive pose): fly_lean is kept for
// the drawing; ModifyBone_4 (AimOn, the aim punch's look-at) is not this file's.
fn pose(w: &World, f: FighterId, s: &mut [f32]) -> bool {
    let Some(pitch) = flight_pitch(w, f) else { return false };
    if pitch != 0.0 {
        turn_boxes(w, f, s, pitch);
    }
    true
}

// ---- SupermanFlightHit and SupermanGrabListener

/// `FlightActor` (SupermanFlightHit_C and SupermanGrabListener_C): the class's own fields beside
/// [`crate::actors::WorldActor`]'s. The actor is where its owner is (AttachToActor), with no velocity.
#[derive(State, Debug, Default)]
pub struct FlightActor {
    /// The AttackID: his attack count at the spawn.
    pub attack_id: i32,
    /// Its handler is on his HitEnemyPlayer.
    pub bound: bool,
    /// HitPlayer.
    pub hit_player: Option<FighterId>,
    /// The fighter whose hold buff is SpawnedBuff, while its OnDestroyed is bound.
    pub held: Option<FighterId>,
    /// ReceiveEndPlay ran.
    pub ended: bool,
    /// ActivationConditions (the class's defaults; the grab listener's four rows).
    pub conds: Vec<bool>,
}

/// The FlightHit packet (`FLIGHT_PACKET`): the class's own HitBoxDamage default.
fn flight_packet() -> HitPacket {
    HitPacket {
        damage: Some(8.0),
        base_knock_back: Some(1100.0),
        knock_back_scaling: Some(21.0),
        knock_back_direction_vector: Some(DataVec { x: Some(1.0), y: Some(1.0), z: None }),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.1),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_superman_hit_punch")),
        on_hit_effect_override: Some(name!("PS_ShaggyRageImpact")),
        force_knockback: Some(true),
        hit_only_one_fighter: Some(true),
        armor_pierce: Some(false),
        attack_decay_type: Some(DataEnum::Value(0)),
        play_on_hit_effects: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// GrabListener's packet per ActivationConditions row: BaseKnockBack = X == 0 ? (Y > 0 ? 1064 : 525) : 1333.
fn throw_packet(reaction: i32, x: Real, y: Real) -> HitPacket {
    HitPacket {
        damage: Some(7.0),
        base_knock_back: Some(if x == 0.0 { if y > 0.0 { 1064.0 } else { 525.0 } } else { 1333.0 }),
        knock_back_scaling: Some(18.0),
        knock_back_direction_vector: Some(DataVec { x: Some(x), y: Some(y), z: None }),
        hit_reaction: Some(DataEnum::Value(reaction)),
        hit_pause_time: Some(0.0),
        hit_stun_mult: Some(1.0),
        force_knockback: Some(true),
        hit_only_one_fighter: Some(true),
        armor_pierce: Some(false),
        attack_decay_type: Some(DataEnum::Value(0)),
        play_on_hit_effects: Some(true),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// `THROWS`: rows 0 Throw_U, 1 Throw_D, 2 Throw_B, 3 Throw_F.
fn throws() -> [HitPacket; 4] {
    [throw_packet(4, 0.0, 1.0), throw_packet(3, 0.0, -1.0), throw_packet(2, -1.0, 0.2), throw_packet(2, 1.0, 0.2)]
}

fn state(w: &World, id: ActorId) -> Option<&FlightActor> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::FlightActor(s) => Some(s),
        _ => None,
    }
}

fn state_mut(w: &mut World, id: ActorId) -> Option<&mut FlightActor> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::FlightActor(s) => Some(s),
        _ => None,
    }
}

/// SpawnedBuff is valid and not being destroyed: the victim's hold is still the one this actor spawned.
fn buff_live(w: &World, id: ActorId) -> bool {
    let (Some(a), Some(s)) = (w.actors.actors.get(id), state(w, id)) else { return false };
    let Some(v) = s.held else { return false };
    w.fighter(v)
        .holds
        .hold
        .as_ref()
        .is_some_and(|h| h.cls == BuffClass::Data(name!("Buff_HeldByEnemy_C")) && h.by == a.owner)
}

/// ReceiveEndPlay, once. Not in the Lab reset's pass (an old pawn there).
fn end_play(w: &mut World, id: ActorId) {
    let Some(s) = state(w, id) else { return };
    if s.ended {
        return;
    }
    let (owner, held) = (w.actors.actors.get(id).expect("a live actor").owner, s.held);
    state_mut(w, id).expect("a flight actor").ended = true;
    if !reset_pass(w) {
        if buff_live(w, id)
            && let Some(v) = held
        {
            end_hold_buff(w, v);
        }
        w.fighter_mut(owner).holds.holding_enemy = false;
    }
    let s = state_mut(w, id).expect("a flight actor");
    s.held = None;
    s.bound = false;
}

/// K2_DestroyActor on the actor.
fn destroy(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
    end_play(w, id);
}

/// Comp_Pawn_MontageNavigation.NotifyChangeSection(End, null) on his active montage instance.
fn jump_to_end(w: &mut World, owner: FighterId) {
    if let Some(m) = active_instance(w, owner) {
        slot_jump_to_section(w, owner, m, name!("End"));
    }
}

fn bubble(w: &World, vic: FighterId) -> bool {
    w.fighter(vic).buffs.buffs.iter().any(|b| b.class == BuffClass::BuffStevenBubbleEnemy && b.frames > 0.0)
}

fn on_hit_enemy(w: &mut World, id: ActorId, vic: FighterId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, listener) = (a.owner, a.cls == name!("SupermanGrabListener_C"));
    if listener {
        state_mut(w, id).expect("a flight actor").hit_player = Some(vic);
        if bubble(w, vic) {
            return;
        }
    }
    let d = &w.fighter(f).decay;
    if !d.landed_attack_ids.contains(&d.attack_count) {
        return;
    }
    if !listener && bubble(w, vic) {
        state_mut(w, id).expect("a flight actor").bound = false;
        w.fighter_mut(f).holds.holding_enemy = true;
        return;
    }
    // A NotPlayerCharacter of character 14 (the Watermelon, Steven's minion) goes on to the buff as a player does
    // (FlightHit ==14 @3571 / @3775 -> @3192; GrabListener @1396 / @1596 -> @993). The other not-player branches are
    // not written (no such pawn in the roster): FlightHit @3178 -> @3571 (the listener unbound, Set Holding
    // Enemy(true), no buff), GrabListener @979 -> @1396 (nothing).
    if !listener {
        state_mut(w, id).expect("a flight actor").hit_player = Some(vic);
    }
    let b = spawn_buff(w, vic, name!("Buff_HeldByEnemy_C"), 1.0, Some(0), Some(f), true, None);
    if b.is_none() || w.fighter(vic).holds.hold.as_ref().map(|h| h.by) != Some(f) {
        return;
    }
    let s = state_mut(w, id).expect("a flight actor");
    s.held = Some(vic);
    s.bound = false;
    w.fighter_mut(f).holds.holding_enemy = true;
}

/// The actor's own pfgDoDamage(null, HitPlayer) with `packet`.
fn deal(w: &mut World, id: ActorId, packet: &HitPacket) {
    let me = ActorRef::Actor(id);
    let Some(s) = state(w, id) else { return };
    let (Some(v), attack_id) = (s.hit_player, s.attack_id) else { return };
    if w.fighter(v).life.dead > 0.0 {
        return;
    }
    if projectile_blocks(w, &dealer(w, me), v) {
        return;
    }
    if invulnerable(w, v) {
        let tick = w.flow.tick;
        attack_dodged(w, me, v, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
        return;
    }
    dodge_over(w, me, v);
    let at = V2 { x: w.fighter(v).body.x, y: actor_y(w, v) };
    let d = dealer(w, me);
    if apply_projectile_hit(w, &d, v, packet, &[], at, None, &[], crate::combat::elem::charge_pct(w, me)) {
        actor_hit_enemy(w, &d, v);
    }
}

fn activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let (owner, cls) = (a.owner, a.cls);
    if !buff_live(w, id) {
        jump_to_end(w, owner);
        destroy(w, id);
        return;
    }
    let s = state_mut(w, id).expect("a flight actor");
    let v = s.held.take().expect("a live buff has its fighter"); // OnDestroyed unbound
    end_hold_buff(w, v);
    if cls == name!("SupermanFlightHit_C") {
        deal(w, id, &flight_packet());
        destroy(w, id);
        return;
    }
    let throws = throws();
    let conds = state(w, id).expect("a flight actor").conds.clone();
    for (i, &on) in conds.iter().enumerate() {
        if !on || i >= throws.len() {
            continue;
        }
        deal(w, id, &throws[i]);
        destroy(w, id);
        // The loop goes on in the build, and a second true row's pfgDoDamage is refused: the packets set
        // HitOnlyOneFighter and the actor's handler has hit a fighter (HasHitAFighter @3236), the actor is destroyed.
        break;
    }
}

/// OnGrappleBuffDestroyed, found by the step: the buff ended on its own.
fn step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    if a.dead || state(w, id).is_none_or(|s| s.held.is_none()) || buff_live(w, id) {
        return;
    }
    state_mut(w, id).expect("a flight actor").held = None;
    jump_to_end(w, owner);
    destroy(w, id);
}

fn set_condition(w: &mut World, id: ActorId, n: i32, value: bool) {
    let Some(s) = state_mut(w, id) else { return };
    if let Ok(n) = usize::try_from(n)
        && n < s.conds.len()
    {
        s.conds[n] = value;
    }
}

/// The kind's `spawn`: the state from the request (`conds` from the class's ActivationConditions defaults, the
/// AttackID his attack count).
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let conds = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .map(|a| arr(&a.p, "ActivationConditions").iter().map(crate::registry::props::truthy).collect())
        .unwrap_or_default();
    (
        WorldActorState::FlightActor(FlightActor {
            attack_id: r.attack_id,
            bound: true,
            conds,
            ..FlightActor::default()
        }),
        false,
    )
}

fn entry(class: WorldActorClass) -> ActorEntry {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        // listeners: no shape of their own and nothing drawn
        size: Some(SizeHooks::default()),
        set_condition: Some(set_condition),
        activate: Some(activate),
        step: Some(step),
        end: Some(end_play),
        ..WorldActorHooks::default()
    };
    ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
}

/// `HOOKS.hitEnemy`: every live listener still bound.
fn hit_enemy(w: &mut World, f: FighterId, vic: FighterId) {
    let list = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        let ActorRef::Actor(id) = a else { continue };
        if w.actors.actors.get(id).is_some_and(|x| !x.dead) && state(w, id).is_some_and(|s| s.bound) {
            on_hit_enemy(w, id, vic);
        }
    }
}

/// `HOOKS.knockedBack`: K2_DestroyActor, no packet.
fn knocked_back(w: &mut World, f: FighterId, _vx: Real, _vy: Real, _a: Real, _by: Option<ActorRef>) {
    let list = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        let ActorRef::Actor(id) = a else { continue };
        if w.actors.actors.get(id).is_some_and(|x| !x.dead) && state(w, id).is_some() {
            destroy(w, id);
        }
    }
}

/// The hooks of this file in Superman's CharHooks (`flight.rs` HOOKS and POSE). `match_reset` is not set here:
/// `mod.rs` joins it behind the pawn's.
pub fn hooks(h: &mut CharHooks) {
    h.anim_update = Some(anim_update);
    h.pose = Some(pose);
    h.hit_enemy = Some(hit_enemy);
    h.knocked_back = Some(knocked_back);
}

/// `flight.rs` HOOKS.matchReset (a new pawn holds nobody; the world's reset runs the actors' EndPlay without its
/// fighter part). Called by `mod.rs`'s join behind the pawn's.
pub fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).holds.holding_enemy = false;
}

/// This file's registry entries (the notify, the two actor classes).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_SupermanFlight_C",
        NotifyHandler { begin: Some(flight_begin), end: Some(flight_end), ..NotifyHandler::default() },
    )?;
    r.actors.enter(FLIGHT_HIT, entry(WorldActorClass::SupermanFlightHit))?;
    r.actors.enter(GRAB_LISTENER, entry(WorldActorClass::SupermanGrabListener))
}
