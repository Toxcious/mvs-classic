//! Jake's horse: JakeHorse_Actor3_C ([`JakeHorse`], a DealerActor facing as its
//! owner: the runner, its hit, the jump), JakeHorsePlatform_C ([`JakeHorsePlatform`], a SpawnedPlatformParent child:
//! `plat` on the world actor, `world::spawned_platform`); joined to hitStunStart (the live horse destroyed). Its state
//! on the pawn: `JakeState::horse_actor`. Buff_AttachToAllyHorse_C is not entered (the ride is dormant).
//!
//! JakeHorse_Actor3 (a SpawnedActorParent child; DestroyOnExitedArena? false, Die With Instigator), attached to Jake's
//! root with no socket: the actor stands at Jake's actor location + (0, 0, -140) and turns with him. Offsets are
//! JakeHorse_Actor3's ubergraph's:
//! - ReceiveBeginPlay @7216: AddInvisibility on the owner @7522; BaseMaxAceleration = the owner's MaxAcceleration @7747;
//!   HitstunHandler.HitStunStart -> HitStunned (K2_DestroyActor @13253) and InputTriggered -> InputeTriggered bound
//!   @7912 / @7998; the facing, the armor mesh @8418 when the owner has armor packs, ArmorBreak bound to its removal
//!   @16595 (sampled in the step).
//! - ActivateObject @14565 -> SetMovementVariableMulticast @14652: CanHit, CheckToHit() (the hit on every fighter
//!   overlapping the Chest capsule), JumpGate, CanJump; CruseControlActive, the owner's AirControl = 1.0 and
//!   MaxAcceleration = GalopSpeed 3000.
//! - ReceiveTick @10237 (every pawn offline; cruise on; the owner valid): (1) MovementMode Walking or Falling:
//!   AddMovementInput((0, -1, 0), FacingLeft ? -1 : 1, force) on the owner; (2) the turn @10716-11499; (3) @11828
//!   bCanWalkOffLedges; (4) @11504 FacingLeft against OldFacing: the owner turns; (5) @12306 JumpGate = IsWalking while
//!   CanJump, and Audio_Turn (@17603).
//! - InputeTriggered @15336 (type 2, the jump; JumpGate and cruise): the owner's velocity clamped to GalopSpeed,
//!   NetImpulse((0, its Y, JumpVelocity 1800), XY and Z override, 0), then Delay(CalculateJumpStopTime(...)) ->
//!   NetImpulse((0, 0, 1800 x 0.3), Z override, 0) @27.
//! - ReceiveActorBeginOverlap @4795 / CheckToHit @19703 -> @4591: the hit on a fighter the Chest touches.
//! - ReceiveEndPlay @13268: CruseControlActive = false; RemoveInvisibility; Comp_WeaponSwap.UnhidEquippedWeapon;
//!   AirControl = BaseAirControl; MaxAcceleration = BaseMaxAceleration; bCanWalkOffLedges = true;
//!   mix_stop_all_fade_eighthofsecond.
//!
//! The ally ride is dormant in this build (JumpOnCollision is NoCollision and nothing turns it on): `rider` stays none
//! and `jump_on_ui` false.
//!
//! JakeHorsePlatform (a SpawnedPlatformParent child, ReceiveBeginPlay @807 after the parent's): the Instigator and the
//! platform ignore each other (IgnoreActorWhenMoving both ways @837 / @974: not undone by the parent's
//! BPI_IgnorePlatformCollision); with authority the first JakeHorse_Actor3 of the same Instigator @773-646: Collision
//! ignores it and the platform is attached to it, KeepWorld: it keeps its offset from the horse and follows it.
// Deviation: the Chest capsule (the hit shape: object type HitBox; half height 140, radius 80 on the Chest bone of
// Jake_Horse at scale 0.8, pitched 89.3 degrees: along the horse) is placed from the skeleton's reference pose (the
// Chest bone at component (80.06, 0, 209.58)): centre (1.12 forward, 159.73 up) from the horse root, a segment of 2 x
// 48 along x and radius 64, tested as five spheres along it against the fighters' hurt shapes; the gallop's animation
// of the bone, the 12 cm depth offset and the 0.7 degree tilt are not taken.
// Deviation: the overlap's begin is the port's (a fighter that was not touched at the last test and is now); the test
// runs in the world's actor hit pass (`hits`), not at the owner's move; only fighters are tested (the Chest's overlaps
// with an item's, a projectile's or a destructible's shapes are not ported, nor HitBoxHandler.HitNonPlayer ->
// CanTurn @6701). A fighter of Jake's team is skipped.
// Deviation: its EndPlay runs at the world's end-of-frame actor sweep, not inside the HitStunStart broadcast.
// Unverified: the Delays (TurnGate's 0.8 s, the jump stop) count their start frame, as the port's other latent Delays.
// Unverified: AddMovementInput's stock ConsumeInputVector / ScaleInputAcceleration (not read in the exe).

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::DataVec;
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::consts as C;
use crate::events::Event;
use crate::fighter::anim::{add_invisibility, remove_invisibility};
use crate::fighter::common::{actor_data, latent_frames, sound_at_for, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::moves::{is_falling, max_accel, net_impulse, set_can_walk_off_ledges, set_max_accel};
use crate::fighter::{FighterId, actor_y, facing, grounded, invulnerable, set_facing};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::FighterSignal;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::{
    SpawnedPlatform, ignore_platform_collision, ignore_wallbounce_collision, plat_mut, platform_end, platform_step,
    scale_platform,
};
use crate::world::spawning::request_props;

use super::state::{jake_mut, jake_of};
use super::weapons::unhid_equipped_weapon;
use crate::actors::carry_run::{calculate_jump_stop_time, clamp_vector_size};

/// JakeHorse_Actor3's class.
pub const HORSE: &str = "JakeHorse_Actor3_C";

/// JakeHorsePlatform's class.
pub const HORSE_PLATFORM: &str = "JakeHorsePlatform_C";

/// The probe spheres along the Chest segment, as multiples of the half segment.
const PROBE_AT: [Real; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];

/// `JakeHorse` (JakeHorse_Actor3_C). The fields the view reads: `x`, `y`, `vx`, `vy`, `facing_left`, `falling`,
/// `armor`, `rider`, `jump_on_ui`.
#[derive(State, Debug, Default)]
pub struct JakeHorse {
    pub attack_id: i32,
    /// HitBoxDamage.
    pub hit: HitPacket,
    /// The notify's translation Z, relative to the owner's actor location.
    pub up: Real,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// FacingLeft.
    pub facing_left: bool,
    /// OldFacing.
    pub old_facing: bool,
    pub falling: bool,
    /// AddedArmorMesh there.
    pub armor: bool,
    /// The dormant ride: always none.
    pub rider: Option<FighterId>,
    /// The same: always false.
    pub jump_on_ui: bool,
    /// CruseControlActive.
    pub cruise: bool,
    /// CanHit.
    pub can_hit: bool,
    /// JumpGate.
    pub jump_gate: bool,
    /// CanJump.
    pub can_jump: bool,
    /// TurnGate.
    pub turn_gate: bool,
    /// Ticks left of Delay(0.8) -> TurnGate, 0: none.
    pub turn_wait: i32,
    /// Ticks left of the jump stop's Delay, 0: none.
    pub jump_wait: i32,
    /// Audio_Turn's DoOnce pair: the side (-1 left, 1 right) of the last full-speed tick, 0: none yet.
    pub sound_side: i32,
    /// BaseMaxAceleration.
    pub base_max_accel: Real,
    /// The fighters (ids) the Chest touched at the last test.
    pub inside: Vec<i32>,
    /// ActorsHitCooldown's keys (fighter ids) and their GetGameTimeInSeconds.
    pub hit_ids: Vec<i32>,
    pub hit_times: Vec<Real>,
}

/// `JakeHorsePlatform` (JakeHorsePlatform_C; its platform is the world actor's `plat`).
#[derive(State, Debug, Default)]
pub struct JakeHorsePlatform {
    pub x: Real,
    pub y: Real,
    /// The attach parent (the horse), none for none.
    pub horse: Option<ActorRef>,
    /// The horse as it ended once it left `World.actors`: the platform's `horse` still holds the object (the
    /// record writes a gone actor whole there).
    pub horse_gone: Option<JakeHorse>,
    /// Its `id` (the world actor's `seq`).
    pub horse_seq: i32,
    /// The offset kept from it (KeepWorld).
    pub dx: Real,
    pub dy: Real,
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

/// The JakeHorse_Actor3_C's place (its `x`, `y`).
pub fn horse_place(_w: &World, _a: &WorldActor, s: &JakeHorse) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeHorsePlatform_C's place (its `x`, `y`).
pub fn platform_place(_w: &World, _a: &WorldActor, s: &JakeHorsePlatform) -> Option<(Real, Real)> {
    Some((s.x, s.y))
}

/// The JakeHorse_Actor3_C as a dealer (`DealerActor`): its place, its owner's facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &JakeHorse) -> (Real, Real, Real, i32) {
    (s.x, s.y, facing(w, a.owner), s.attack_id)
}

/// The JakeHorse_Actor3_C's hit packet (`hit`).
pub fn hit(s: &JakeHorse) -> &HitPacket {
    &s.hit
}

fn horse_of(w: &World, id: ActorId) -> Option<&JakeHorse> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::JakeHorse(s) => Some(s),
        _ => None,
    }
}

fn horse_mut(w: &mut World, id: ActorId) -> Option<&mut JakeHorse> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JakeHorse(s) => Some(s),
        _ => None,
    }
}

fn platform_mut(w: &mut World, id: ActorId) -> Option<&mut JakeHorsePlatform> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::JakeHorsePlatform(s) => Some(s),
        _ => None,
    }
}

/// `JakeHorse.follow`: the owner's place, the actor's own up, and the owner's velocity.
fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let (ox, oy, ovx, ovy) = (w.fighter(o).body.x, actor_y(w, o), w.fighter(o).body.vx, w.fighter(o).body.vy);
    let Some(s) = horse_mut(w, id) else { return };
    s.x = ox;
    s.y = oy + s.up;
    s.vx = ovx;
    s.vy = ovy;
}

/// The kind's `spawn` (`horseSpawn`): the state around the class data, the owner's AddInvisibility, the facing, the
/// armor mesh.
fn horse_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let (tx, ty) = (t.and_then(|t| num(t, "X")).unwrap_or(0.0), t.and_then(|t| num(t, "Y")).unwrap_or(0.0));
    let socket = text(&np, "Socket").filter(|s| !s.is_empty() && *s != "None");
    if !flag(&np, "AttachToActor") || flag(&np, "WorldTransforms") || socket.is_some() || tx != 0.0 || ty != 0.0 {
        warn_text(
            w,
            &format!(
                "{HORSE}: a spawn that is not attached to the owner (no socket) with a Z translation alone is not ported"
            ),
        );
    }
    let mut s = JakeHorse {
        attack_id: r.attack_id,
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None),
        up: t.and_then(|t| num(t, "Z")).unwrap_or(0.0),
        base_max_accel: max_accel(w, f),
        ..JakeHorse::default()
    };
    s.x = w.fighter(f).body.x;
    s.y = actor_y(w, f) + s.up;
    s.vx = w.fighter(f).body.vx;
    s.vy = w.fighter(f).body.vy;
    // ReceiveBeginPlay: AddInvisibility on the owner, the facing, the armor mesh.
    add_invisibility(w, f);
    s.facing_left = facing(w, f) < 0.0;
    s.old_facing = s.facing_left;
    s.armor = !w.fighter(f).armor.armor_packs.is_empty();
    (WorldActorState::JakeHorse(s), false)
}

/// The rest of the spawn once the World holds the actor: the live horse on the pawn (`JakeState::horse_actor`).
fn horse_spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    if let Some(j) = jake_mut(w, f) {
        j.horse_actor = Some(ActorRef::Actor(id));
    }
}

/// SetMovementVariableMulticast @14652.
fn horse_activate(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    if let Some(s) = horse_mut(w, id) {
        s.can_hit = true;
    }
    check_to_hit(w, id);
    let gs = num(actor_p(w, ActorRef::Actor(id)), "GalopSpeed").unwrap_or(0.0);
    let Some(s) = horse_mut(w, id) else { return };
    s.jump_gate = true;
    s.can_jump = true;
    s.cruise = true;
    let (x, y) = (s.x, s.y);
    w.fighter_mut(o).air.air_control = 1.0;
    set_max_accel(w, o, gs);
    sound_at_for(w, Some(o), name!("sfx_cha_jake_foley_horse_footstep"), x, y);
}

fn horse_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    follow(w, id);
    let Some(s) = horse_of(w, id) else { return };
    if s.cruise {
        let mx = w.fighter(o).controls.inp.x;
        let vy = -w.fighter(o).body.vx; // UE Y is sim -x
        // (1) AddMovementInput, forced, toward the owner's facing, and (2) the turn: both behind the Walking /
        // Falling test @10390-10578 (any other movement mode pops the flow to @11828, past the turn).
        if grounded(w, o) || is_falling(w, o) {
            let side = if facing(w, o) < 0.0 { -1.0 } else { 1.0 };
            let mi = &mut w.fighter_mut(o).air.move_in;
            *mi = max(-1.0, min(1.0, *mi + side));
            let s = horse_of(w, id).expect("the horse");
            let against = if s.facing_left { mx > 0.0 } else { mx < 0.0 };
            if mx.abs() > 0.4 && (s.turn_gate || vy.abs() >= 1800.0 || vy == 0.0) && against {
                let s = horse_mut(w, id).expect("the horse");
                s.facing_left = mx <= 0.0;
                s.turn_gate = false;
                if s.turn_wait == 0 {
                    s.turn_wait = to_int32(latent_frames(0.8)); // a pending Delay ignores a second call
                }
            }
        }
        // (3) bCanWalkOffLedges.
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(mx < 0) / !(VY >= 0): NaN takes the else side")]
        let walk_off = (mx < 0.0 && vy >= 0.0) || (!(mx < 0.0) && !(vy >= 0.0));
        set_can_walk_off_ledges(w, o, walk_off);
        // (4) the owner's rotation.
        let s = horse_mut(w, id).expect("the horse");
        if s.facing_left != s.old_facing {
            s.old_facing = s.facing_left;
            let v = if s.facing_left { -1.0 } else { 1.0 };
            set_facing(w, o, v); // OwningFighter.SetRotation(FacingLeft)
        }
    }
    // (5) the AnimBP's inputs, JumpGate and Audio_Turn.
    let walk = grounded(w, o);
    let (ovx, ovy) = (w.fighter(o).body.vx, w.fighter(o).body.vy);
    let s = horse_mut(w, id).expect("the horse");
    if s.can_jump {
        s.jump_gate = walk;
    }
    s.falling = !walk;
    let mut chirp = None;
    if walk && ovy == 0.0 && ovx.abs() >= 1800.0 {
        let side = if s.facing_left { -1 } else { 1 };
        if s.sound_side == -side {
            chirp = Some((s.x, s.y));
        }
        s.sound_side = side;
    }
    if let Some((x, y)) = chirp {
        sound_at_for(w, Some(o), name!("sfx_cha_jake_foley_horse_turn"), x, y);
    }
    // The latent Delays, behind the tick (AActor::Tick processes them after ReceiveTick).
    let s = horse_mut(w, id).expect("the horse");
    let mut turn_gate = false;
    if s.turn_wait > 0 {
        s.turn_wait -= 1;
        turn_gate = s.turn_wait == 0;
    }
    if turn_gate {
        s.turn_gate = true;
    }
    let mut stop = false;
    if s.jump_wait > 0 {
        s.jump_wait -= 1;
        stop = s.jump_wait == 0;
    }
    if stop {
        let p = actor_p(w, ActorRef::Actor(id));
        let v = num(p, "JumpVelocity").unwrap_or(0.0) * num(p, "JumpStopVelocityMultiplier").unwrap_or(0.0);
        net_impulse(w, o, 0.0, v, false, true, 0.0, false, 0.0);
    }
    // ArmorBreak @16595: the armor mesh goes once the owner has no pack left.
    let none_left = w.fighter(o).armor.armor_packs.is_empty();
    let s = horse_mut(w, id).expect("the horse");
    if s.armor && none_left {
        s.armor = false;
    }
}

/// InputeTriggered @15336: InputType 2 (Jump) and JumpGate and CruseControlActive.
fn input_triggered(w: &mut World, id: ActorId, kind: Real) {
    let Some(s) = horse_of(w, id) else { return };
    if !(kind == 2.0 && s.jump_gate && s.cruise) {
        return;
    }
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let (x, y) = (s.x, s.y);
    let p = actor_p(w, ActorRef::Actor(id));
    let gs = num(p, "GalopSpeed").unwrap_or(0.0);
    let jv = num(p, "JumpVelocity").unwrap_or(0.0);
    let max_h = num(p, "MaxJumpHeight").unwrap_or(0.0);
    let mult = num(p, "JumpStopVelocityMultiplier").unwrap_or(0.0);
    horse_mut(w, id).expect("the horse").jump_gate = false;
    // GetVelocity's (Y, Z) is (-vx, vy); ClampVectorSize(-GalopSpeed, GalopSpeed); NetImpulse((0, Y, JumpVelocity)).
    let (vx, vy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    let (cx, _) = clamp_vector_size(-vx, vy, -gs, gs);
    net_impulse(w, f, -cx, jv, true, true, 0.0, false, 0.0);
    // (no World in a FighterSignal: the sound goes on the fighter's own events, not the world's)
    let fid = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_jake_foley_horse_jump"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    // The velocity read again (the impulse waits for the next move): the same clamp.
    let (cx, _) = clamp_vector_size(-vx, vy, -gs, gs);
    let h = min(max((cx * 0.2).abs(), 50.0), max_h);
    let t = calculate_jump_stop_time(jv, h, mult, C::GRAVITY_SCALE);
    let s = horse_mut(w, id).expect("the horse");
    if s.jump_wait == 0 {
        s.jump_wait = to_int32(latent_frames(t));
    }
}

/// InputTriggered of its owner (InputeTriggered @7998).
#[allow(clippy::too_many_arguments)]
fn horse_signal(
    w: &mut World,
    a: ActorId,
    f: FighterId,
    sig: FighterSignal,
    _flag: bool,
    num: Real,
    _n: Name,
    _by: Option<FighterId>,
) {
    let Some(x) = w.actors.actors.get(a) else { return };
    if sig == FighterSignal::InputTriggered && f == x.owner && !x.dead {
        input_triggered(w, a, num);
    }
}

/// The Chest capsule's probe spheres where the horse stands (see the Deviation above).
fn probe(w: &World, id: ActorId) -> [WorldSphere; 5] {
    let chest_fwd: Real = (80.057 - 78.65888) * 0.8;
    let chest_up: Real = (209.5801 - 9.919802) * 0.8;
    let chest_r: Real = 80.0 * 0.8;
    let chest_seg: Real = (140.0 - 80.0) * 0.8;
    let a = w.actors.actors.get(id).expect("a live actor id");
    let Some(s) = horse_of(w, id) else { return Default::default() };
    let side = if facing(w, a.owner) < 0.0 { -1.0 } else { 1.0 };
    let k = a.size_scale.unwrap_or(1.0); // the Chest under the mesh under the root: at the root's scale
    let (cx, cy) = (s.x + chest_fwd * side * k, s.y + chest_up * k);
    PROBE_AT.map(|at| WorldSphere {
        x: cx + at * chest_seg * k,
        y: cy,
        z: Some(0.0),
        r: chest_r * k,
        ..WorldSphere::default()
    })
}

/// The cooldown test and the damage of one fighter (@5216-5579 / @3026-4011).
fn try_hit(w: &mut World, id: ActorId, vic: FighterId, at: &WorldSphere) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let now = w.fighter(o).clock.time;
    let vid = w.fighter(vic).id;
    let cooldown = num(actor_p(w, me), "ActorHitCooldownTime").unwrap_or(0.0);
    let Some(s) = horse_mut(w, id) else { return };
    let k = s.hit_ids.iter().position(|&i| i == vid);
    if let Some(k) = k {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(f32(now - t) >= cooldown): NaN refuses")]
        if !((now - s.hit_times[k]) >= cooldown) {
            return;
        }
        s.hit_times[k] = now;
    } else {
        s.hit_ids.push(vid);
        s.hit_times.push(now);
    }
    let d = dealer(w, me);
    if projectile_blocks(w, &d, vic) {
        return;
    }
    let (attack_id, bkb, base) = {
        let s = horse_of(w, id).expect("the horse");
        (s.attack_id, s.hit.base_knock_back.unwrap_or(0.0), s.hit.clone())
    };
    if invulnerable(w, vic) {
        let tick = w.flow.tick;
        attack_dodged(w, me, vic, attack_id, bkb, tick, None, false);
        return;
    }
    dodge_over(w, me, vic);
    // KnockBackDirectionVector by the owner's BPI_IsFacingLeft; IgnoreFacing: the world direction.
    let left = facing(w, o) < 0.0;
    let mut p = base;
    p.knock_back_direction_vector = Some(DataVec { x: Some(if left { 1.0 } else { -1.0 }), y: Some(1.0), z: None });
    let dir = V2 { x: if left { -1.0 } else { 1.0 }, y: 1.0 };
    let d = dealer(w, me);
    let charge = charge_pct(w, me);
    if apply_projectile_hit(w, &d, vic, &p, &[], V2 { x: at.x, y: at.y }, Some(dir), &[], charge) {
        actor_hit_enemy(w, &d, vic);
    }
}

/// The fighters the Chest touches, `inside` kept; each one that came in (every one touched, with `all`) is hit when
/// `hit`.
fn overlap_pass(w: &mut World, id: ActorId, hit: bool, all: bool) {
    follow(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let spheres = probe(w, id);
    for vic in match_fighters(w) {
        if vic == o {
            continue;
        }
        let vid = w.fighter(vic).id;
        let was = horse_of(w, id).and_then(|s| s.inside.iter().position(|&i| i == vid));
        let now = alive(w, vic)
            && match victim_pose(w, vic) {
                Some(vp) => {
                    let m = hurt_hit(w, vic, &spheres, &vp).is_some();
                    give_vbuf(w, vp);
                    m
                }
                None => false,
            };
        let Some(s) = horse_mut(w, id) else { return };
        if !now {
            if let Some(k) = was {
                s.inside.remove(k);
            }
            continue;
        }
        if was.is_none() {
            s.inside.push(vid);
        }
        if !hit || (!all && was.is_some()) {
            continue;
        }
        if w.fighter(vic).team.team == w.fighter(o).team.team || no_hit(w, vic) {
            continue;
        }
        try_hit(w, id, vic, &spheres[2]);
    }
}

/// CheckToHit: every fighter overlapping now.
fn check_to_hit(w: &mut World, id: ActorId) {
    overlap_pass(w, id, true, true);
}

/// ReceiveActorBeginOverlap, CanHit on.
fn horse_hits(w: &mut World, id: ActorId) {
    let can = horse_of(w, id).is_some_and(|s| s.can_hit);
    overlap_pass(w, id, can, false);
}

fn horse_end(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let o = a.owner;
    let Some(s) = horse_mut(w, id) else { return };
    s.cruise = false;
    let (base, x, y) = (s.base_max_accel, s.x, s.y);
    remove_invisibility(w, o);
    unhid_equipped_weapon(w, o);
    w.fighter_mut(o).air.air_control = C::AIR_CONTROL; // BaseAirControl: BaseFighter's 0.9 (Pawn_Jake has no other)
    set_max_accel(w, o, base);
    set_can_walk_off_ledges(w, o, true);
    sound_at_for(w, Some(o), name!("mix_stop_all_fade_eighthofsecond"), x, y);
    if let Some(j) = jake_mut(w, o)
        && j.horse_actor == Some(ActorRef::Actor(id))
    {
        j.horse_actor = None;
    }
    // The platforms attached to it keep the object.
    let ended = horse_of(w, id).cloned();
    let seq = w.actors.actors.get(id).map_or(-1, |a| a.seq);
    let ids: Vec<ActorId> = w.actors.actors.ids().collect();
    for pid in ids {
        if let Some(s) = platform_mut(w, pid)
            && s.horse == Some(ActorRef::Actor(id))
        {
            s.horse_gone = ended.clone();
            s.horse_seq = seq;
        }
    }
}

/// horse.rs's part of the joined hitStunStart (HitStunned @13253: the live horse's `dead`), after house.rs's.
pub fn hit_stun_start(w: &mut World, f: FighterId) {
    let Some(ActorRef::Actor(id)) = jake_of(w, f).and_then(|j| j.horse_actor) else { return };
    if horse_of(w, id).is_some()
        && let Some(a) = w.actors.actors.get_mut(id)
        && !a.dead
    {
        a.dead = true;
    }
}

// ------------------------------------------------------------------ JakeHorsePlatform

/// `JakeHorsePlatform.place`: the platform at the horse's place plus the kept offset (a live horse), its Collision there.
fn platform_follow(w: &mut World, id: ActorId) {
    let Some(s) = (match &w.actors.actors.get(id).map(|a| &a.state) {
        Some(WorldActorState::JakeHorsePlatform(s)) => Some(s),
        _ => None,
    }) else {
        return;
    };
    let (horse, dx, dy) = (s.horse, s.dx, s.dy);
    let at = horse
        .and_then(|h| match h {
            ActorRef::Actor(h) => w.actors.actors.get(h),
            _ => None,
        })
        .filter(|a| !a.dead)
        .and_then(|a| match &a.state {
            WorldActorState::JakeHorse(h) => Some((h.x, h.y)),
            _ => None,
        });
    let Some(s) = platform_mut(w, id) else { return };
    if let Some((hx, hy)) = at {
        s.x = hx + dx;
        s.y = hy + dy;
    }
    let (x, y) = (s.x, s.y);
    if let Some(c) = plat_mut(w, id).and_then(SpawnedPlatform::collision_part) {
        c.place(x, y);
    }
}

/// BPI_Scale: the platform's boxes by the ratio, then its place (Collision on the root with no scale of its own).
fn platform_scale(w: &mut World, id: ActorId, k: Real, old: Real) {
    scale_platform(w, id, k / old);
    platform_follow(w, id);
}

/// The kind's `spawn` (`platformSpawn`): the place of the notify, the parent's BeginPlay loop.
fn platform_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = request_props(w, f, r);
    let at = crate::world::spawn_spot::spawn_place(w, f, &np);
    let mut s = JakeHorsePlatform {
        x: at.x,
        y: at.y,
        blast_at: 0.1,
        blast_bounds: new_blast_bounds(),
        ..JakeHorsePlatform::default()
    };
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(f), r.cls).map(|d| &d.p);
    let force = p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox"));
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    let dead = blast_box_loop(&spawn_box(w), &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe);
    (WorldActorState::JakeHorsePlatform(s), dead)
}

/// The rest of the spawn once the World holds the actor: the SpawnedPlatform with the half extents (X, Z) of the
/// class's Collision, its place, ReceiveBeginPlay @807 (the Instigator ignored) and the first JakeHorse_Actor3 of the
/// same Instigator: attached, KeepWorld.
fn platform_spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let half = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), cls).and_then(|d| {
        let e = obj(obj(&d.components, "Collision")?, "BoxExtent")?;
        Some((num(e, "X").unwrap_or(0.0), num(e, "Z").unwrap_or(0.0)))
    });
    if half.is_none() {
        warn_text(w, &format!("{HORSE_PLATFORM}: no Collision box in the data"));
    }
    let pl = SpawnedPlatform::new(w, f, cls, half, None);
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    platform_follow(w, id);
    // ReceiveBeginPlay @807: the Instigator and the platform ignore each other (see `platform_ignore`).
    let fid = w.fighter(f).id;
    if let Some(c) = plat_mut(w, id).and_then(SpawnedPlatform::collision_part) {
        c.set_ignored(fid, true);
    }
    // The first JakeHorse_Actor3 of the same Instigator: attached, KeepWorld.
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let h = w.actors.actors.get(x).expect("a live actor id");
        if h.cls != name_of_horse(w) || h.dead || h.owner != f {
            continue;
        }
        follow(w, x);
        let (hx, hy) = horse_of(w, x).map_or((0.0, 0.0), |h| (h.x, h.y));
        if let Some(s) = platform_mut(w, id) {
            s.horse = Some(ActorRef::Actor(x));
            s.dx = s.x - hx;
            s.dy = s.y - hy;
        }
        break;
    }
}

fn name_of_horse(w: &World) -> Name {
    w.names.get(HORSE).expect("the horse class is interned at bind")
}

fn platform_step_kind(w: &mut World, id: ActorId) {
    if let Some(s) = platform_mut(w, id) {
        s.age += 1;
    }
    platform_step(w, id);
    platform_follow(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let doe = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let WorldActorState::JakeHorsePlatform(s) = &mut a.state else { return };
    // SpawnedActorParent CheckBlastBoxExit on its 0.1 s looping timer (DestroyOnExitedArena?), as Projectile.expiry.
    if doe && crate::consts::clock(crate::real(s.age - 1)) > f64::from(s.blast_at) {
        s.blast_at += 0.1;
        if check_blast_box_exit(s.x, s.y, &s.blast_bounds) {
            a.dead = true;
        }
    }
}

/// The parent's BPI_IgnorePlatformCollision, but the Instigator stays ignored (its actor-level lists, @837 / @974).
fn platform_ignore(w: &mut World, id: ActorId, f: FighterId, ignore: bool, vy: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    if f == a.owner {
        let fid = w.fighter(f).id;
        if let Some(c) = plat_mut(w, id).and_then(SpawnedPlatform::collision_part) {
            c.set_ignored(fid, true);
        }
    } else {
        ignore_platform_collision(w, id, f, ignore, vy);
    }
}

fn platform_ignore_wallbounce(w: &mut World, id: ActorId, f: FighterId, ignore: bool) {
    ignore_wallbounce_collision(w, id, f, ignore);
}

/// This file's registry entries (JakeHorse_Actor3_C, JakeHorsePlatform_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        HORSE,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeHorseActor3),
            ActorHooks::World(WorldActorHooks {
                // the place probe reads the scale; the view draws at it
                size: Some(SizeHooks::default()),
                spawn: Some(horse_spawn),
                spawned: Some(horse_spawned),
                activate: Some(horse_activate),
                step: Some(horse_step),
                hits: Some(horse_hits),
                fighter_signal: Some(horse_signal),
                end: Some(horse_end),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        HORSE_PLATFORM,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::JakeHorsePlatform),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks { scale: Some(platform_scale), ..SizeHooks::default() }),
                spawn: Some(platform_spawn),
                spawned: Some(platform_spawned),
                step: Some(platform_step_kind),
                end: Some(platform_end),
                ignore_platform_collision: Some(platform_ignore),
                ignore_wallbounce_collision: Some(platform_ignore_wallbounce),
                ..WorldActorHooks::default()
            }),
        ),
    )
}
