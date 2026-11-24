//! MouseTrap_SpawnedActor: Tom's mouse trap ([`MouseTrap`], a
//! SpawnedActorParent child with no ProjectileMovement, a DealerActor): the ground probe and its ride, the trigger
//! volume, the snap and its hit, the cheese bait (cheese.rs add_cheese_stack). The ground actor's ObjectDestroyed
//! binding goes through `world::wall_bp` (`bindGroundDestroyed`, `wallDelegateFns`).
//!
//! The graph, by entry (offsets of its ubergraph):
//! - ReceiveBeginPlay @1368: the parent's (the blast-box loop), TriggerVolume off; Delay(SetDelay 0.5) @1689 -> @71:
//!   TriggerGate, TriggerVolume QueryOnly; the ground probe @1748-2341 (BoxTraceSingle from Z - 50 to Z - 150, half
//!   (100, 100, 0), ProjectileGroundTrace) and K2_AttachToActor(HitActor) @2341: the trap rides it; Delay(12) @2686
//!   -> @121: GroundDestroyed() and sfx_cha_tomandjerry_trap_death @135.
//! - TriggerVolume's begin overlap @3894: another team -> the hit gate @4328 (BaitType 0 or 1: the snap, the Cheese
//!   hidden, Delay(TriggerTime) @4568 -> @15 Detonate); the same team's Buff_Cheese holder -> CheeseVisualsServer
//!   @4893, AddStack(1) @4907, HitBoxDamage 5 / 1200 / 15 @4948-5012.
//! - ReceiveAnyDamage @5410: MulticastDamageTaken, Detonate. Detonate @5045: HitBoxOn?, Delay(0.15) @5080 -> @30 off,
//!   Delay(0.4) @5169 -> @56 K2_DestroyActor.
//
// Deviation (trap a): the TriggerVolume's overlaps are taken in the trap's tick, at the places the frame's moves
// left, as one overlap per actor: the bodies of Pawn, PhysicsBody and ProjectileDestructible that answer its
// WorldDynamic with Overlap (`sphere_overlap_actors` with that responder). The build's begin overlaps come inside the
// other actor's move. Another actor's Comp_Actor_Destructible invulnerability is taken as none for a spawned actor
// (no InvulnerabilityCount in the port).
// Deviation (trap b): the ride is the hit body's own move of the frame (`TraceHit::floor`: a moving stage platform, a
// stage solid, a spawned box), added at the head of the trap's step; the build's weld moves the actor inside its
// parent's move. A HitActor with no floor of its own in the port is warned and not ridden. Unverified (trap b): the
// attach parent's own destroy is taken as the stock detach with KeepWorld: the trap stays where it is.
// Unverified (trap c): the latent Delays count from the call's age (World.actors kinds tick behind the spawn loop: a
// Delay of BeginPlay counts the spawn frame); a second Detonate while a Delay of it is pending leaves that Delay as
// it is (UE's Delay node ignores a call while its action is pending).
// Deviation (trap d): the Detonate's hit box meets fighters through `touches` (their hurt shapes and root
// capsule); its begin overlaps with projectiles, items and other actors (HitBoxHandler @7466 / @7502:
// pfgDoDamage on them) are `hit_box_actor_hits` behind the fighter pass: at the
// trap's tick, in the play plane, and a hit there is no HitEnemyPlayer.
// Deviation (trap e): the trap's ProjectileCollision (r 32, ProjectileDestructible) is met by a fighter's
// DestroyProjectiles hit box (`hurt_box`), on every hit channel (the world reads a HurtBox component for the channel;
// the trap has none); an enemy projectile's hit box over it is not tested.
// Unverified (trap f): the ground probe runs in the `spawned` hook (the trap's id is known there), after the actor is
// listed; the probe ignores the trap itself either way.

use indexmap::IndexSet;

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p, spawned_dead};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_pct;
use crate::combat::hits::{
    HitShape, actor_hit_enemy, apply_projectile_hit, dealer, hit_box_actor_hits, projectile_blocks,
};
use crate::combat::phases::alive;
use crate::combat::spawned_hits::{class_rows, hit_box_types};
use crate::events::{Event, SoundKey};
use crate::fighter::body::{PlatformRef, plat};
use crate::fighter::common::{actor_class_def, actor_data, latent_frames, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_DYNAMIC, TRACE_PROJECTILE_GROUND, TraceBody,
    body_info, box_trace_single, sphere_overlap_actors,
};
use crate::fighter::{FighterId, facing, invulnerable};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{all_fighters, flag, num, num_or, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::{spawn_place, spawn_spot};
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, request_props};
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};

/// The trap's class.
pub const TRAP: &str = "MouseTrap_SpawnedActor_C";

const HIT_OFF: Real = 0.15; // Delay @5080
const DESTROY: Real = 0.4; // Delay @5169
const LIFE: Real = 12.0; // Delay @2686
const GROUND_DESTROYED: &str = "MouseTrap_SpawnedActor_C.GroundDestroyed";

/// `MouseTrap` (MouseTrap_SpawnedActor_C): the class's fields under their names. Ages are in ticks, a pending
/// Delay's is the age it fires at (-1 for none).
#[derive(State, Debug)]
pub struct MouseTrap {
    /// HitBoxDamage (the cheese bait writes three of its fields).
    pub hit: HitPacket,
    pub x: Real,
    pub y: Real,
    /// The owner's facing at the spawn.
    pub facing: Real,
    pub attack_id: i32,
    /// CheckBlastBoxTimer: the next check's time.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// Ticks lived, the spawn frame's included.
    pub age: Real,
    /// The Cheese component is visible from the spawn; hidden at the hit gate @4534.
    pub cheese: bool,
    pub sprung: bool,
    /// PS_TomJerryMouseTrapAura alive (Detonate destroys it @5135).
    pub aura: bool,
    /// TriggerGate (TriggerVolume's collision on with it).
    pub trigger_gate: bool,
    pub has_hit: bool,
    /// BaitType.
    pub bait: i32,
    /// Audio Trigger's do-once.
    pub audio_once: bool,
    /// The hit gate's do-once.
    pub hit_once: bool,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// HitEachActorOnlyOnce, by fighter id (cleared when the hit box goes off).
    pub hit_set: IndexSet<i32>,
    /// The actors that are no fighter the HitBox overlaps (`hit_box_actor_hits`).
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track (cleared with the box).
    pub once: IndexSet<ActorRef>,
    /// The object types the HitBox's template answers (`hit_box_types`).
    pub types: Vec<Real>,
    /// OverlappingInvulnerableFighters.
    pub invulnerable: Vec<ActorRef>,
    /// The actors overlapping the TriggerVolume (a fighter's id, 1000 + an actor's seq).
    pub inside: Vec<i32>,
    pub gate_at: Real,
    pub life_at: Real,
    pub detonate_at: Real,
    pub hit_off_at: Real,
    pub destroy_at: Real,
    pub ended: bool,
    /// The attach parent's floor it rides (the BeginPlay probe's `TraceHit::floor`), `None` for none.
    pub floor: Option<PlatformRef>,
    /// The spheres' radii at scale 1 (the root's scale is taken at the read).
    pub trigger_r: Real,
    pub hit_r: Real,
    pub hurt_r: Real,
}

impl Default for MouseTrap {
    /// The field initial values.
    fn default() -> Self {
        MouseTrap {
            hit: HitPacket::default(),
            x: 0.0,
            y: 0.0,
            facing: 1.0,
            attack_id: 0,
            blast_at: 0.0,
            blast_bounds: new_blast_bounds(),
            age: 0.0,
            cheese: true,
            sprung: false,
            aura: true,
            trigger_gate: false,
            has_hit: false,
            bait: 0,
            audio_once: false,
            hit_once: false,
            hit_on: false,
            hit_set: IndexSet::new(),
            over: IndexSet::new(),
            once: IndexSet::new(),
            types: Vec::new(),
            invulnerable: Vec::new(),
            inside: Vec::new(),
            gate_at: -1.0,
            life_at: -1.0,
            detonate_at: -1.0,
            hit_off_at: -1.0,
            destroy_at: -1.0,
            ended: false,
            floor: None,
            trigger_r: 32.0,
            hit_r: 32.0,
            hurt_r: 32.0,
        }
    }
}

fn state(w: &World, id: ActorId) -> &MouseTrap {
    match &w.actors.actors.get(id).expect("a live world actor").state {
        WorldActorState::MouseTrap(s) => s,
        _ => panic!("a MouseTrap_SpawnedActor_C state"),
    }
}

fn state_mut(w: &mut World, id: ActorId) -> &mut MouseTrap {
    match &mut w.actors.actors.get_mut(id).expect("a live world actor").state {
        WorldActorState::MouseTrap(s) => s,
        _ => panic!("a MouseTrap_SpawnedActor_C state"),
    }
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live world actor").owner
}

fn is_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

/// The root's scale (`k`).
fn scale(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

/// The MouseTrap_SpawnedActor_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &MouseTrap) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The MouseTrap_SpawnedActor_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &MouseTrap) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The MouseTrap_SpawnedActor_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &MouseTrap) -> &HitPacket {
    &s.hit
}

// ---------------------------------------------------------------- the delays

#[derive(Clone, Copy)]
enum Delay {
    Gate,
    Life,
    Detonate,
    HitOff,
    Destroy,
}

fn slot(s: &mut MouseTrap, d: Delay) -> &mut Real {
    match d {
        Delay::Gate => &mut s.gate_at,
        Delay::Life => &mut s.life_at,
        Delay::Detonate => &mut s.detonate_at,
        Delay::HitOff => &mut s.hit_off_at,
        Delay::Destroy => &mut s.destroy_at,
    }
}

/// A Delay node: a call while its action is pending does nothing.
fn delay(s: &mut MouseTrap, d: Delay, seconds: Real) {
    let age = s.age;
    let at = slot(s, d);
    if *at < 0.0 {
        *at = age + latent_frames(seconds);
    }
}

fn fires(s: &mut MouseTrap, d: Delay) -> bool {
    let age = s.age;
    let at = slot(s, d);
    if *at < 0.0 || age < *at {
        return false;
    }
    *at = -1.0;
    true
}

fn sound_on(w: &mut World, id: ActorId, ev: Name) {
    let own = owner_of(w, id);
    let seq = w.actors.actors.get(id).expect("a live world actor").seq;
    let (x, y) = {
        let s = state(w, id);
        (s.x, s.y)
    };
    let fighter = w.fighter(own).id;
    w.events.push(Event::Sound {
        fighter,
        ev,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: Some(SoundKey { tag: name!("ak"), n: seq }),
    });
}

/// The hit gate @4328 (a do-once): HasHit, and for BaitType 0 or 1 the snap, the Cheese hidden and the Delay before
/// Detonate.
fn hit_gate(w: &mut World, id: ActorId) {
    let trigger_time = num_or(actor_p(w, ActorRef::Actor(id)), "TriggerTime", 0.0);
    let t = state_mut(w, id);
    if t.hit_once {
        return;
    }
    t.hit_once = true;
    t.has_hit = true;
    if t.bait != 0 && t.bait != 1 {
        return;
    }
    t.sprung = true;
    t.cheese = false;
    delay(t, Delay::Detonate, trigger_time);
}

/// ActivateHitMulticast / the begin overlap's enemy branch @4314: Audio Trigger (its do-once), then the hit gate.
fn activate_hit(w: &mut World, id: ActorId) {
    if !state(w, id).audio_once {
        state_mut(w, id).audio_once = true;
        sound_on(w, id, name!("sfx_cha_tomandjerry_trap_activate"));
    }
    hit_gate(w, id);
}

/// Detonate @5045.
fn detonate(t: &mut MouseTrap) {
    t.hit_on = true;
    delay(t, Delay::HitOff, HIT_OFF);
    delay(t, Delay::Destroy, DESTROY);
    t.aura = false;
}

// ---------------------------------------------------------------- the trigger volume

/// A fighter's id, an actor's 1000 + its count (`keyOf`).
fn key_of(w: &World, o: ActorRef) -> i32 {
    match o {
        ActorRef::Fighter(f) => w.fighter(f).id,
        o => 1000 + crate::chars::shaggy::shockwave::actor_key(w, o).map_or(0, |(_, n)| n),
    }
}

fn team_of(w: &World, o: ActorRef) -> i32 {
    match o {
        ActorRef::Fighter(f) => w.fighter(f).team.team,
        o => owner_team(w, o),
    }
}

/// Comp_Actor_Destructible IsInvulnerable? (InvulnerabilityCount > 0): a fighter's; a spawned actor's is taken as none
/// (Deviation trap a).
fn invulnerable_now(w: &World, o: ActorRef) -> bool {
    match o {
        ActorRef::Fighter(f) => invulnerable(w, f),
        _ => false,
    }
}

/// The overlap's own test @3904-4018: a BPI_BaseFighter (a minion too) or a BPI_SpawnedActor.
fn is_trigger_actor(w: &World, me: ActorRef, o: ActorRef) -> bool {
    if let ActorRef::Fighter(f) = o {
        return w.fighter(f).life.dead == 0.0;
    }
    if o == me {
        return false;
    }
    let (Some(cls), Some(own)) =
        (crate::actors::world_actor::actor_cls(w, o), crate::actors::world_actor::actor_owner(w, o))
    else {
        return false;
    };
    if spawned_dead(w, o).is_none_or(|dead| dead) {
        return false;
    }
    actor_class_def(w, own, cls)
        .and_then(|d| d.ifaces.as_deref())
        .is_some_and(|i| format!(" {i} ").contains(" BPI_SpawnedActor_C "))
}

/// TriggerVolume's begin overlap @3894 with `o`.
fn begin_overlap(w: &mut World, id: ActorId, o: ActorRef) {
    if !state(w, id).trigger_gate {
        return;
    }
    let team = w.fighter(owner_of(w, id)).team.team;
    if team_of(w, o) != team {
        if !invulnerable_now(w, o) {
            activate_hit(w, id);
        } else if !state(w, id).invulnerable.contains(&o) {
            state_mut(w, id).invulnerable.push(o); // AddUnique @1194
        }
        return;
    }
    // @4627: the Comp_State_Manager cast (a spawned actor's holds no Buff_Cheese).
    let ActorRef::Fighter(f) = o else { return };
    if !super::cheese::has_cheese(w, f) || state(w, id).bait == 1 {
        return;
    }
    // CheeseVisualsServer -> AddCheeseVisual @5351: the Cheese shown, then @5224 BaitType 1 and the post at the location.
    let (x, y) = {
        let t = state_mut(w, id);
        t.cheese = true;
        t.bait = 1;
        (t.x, t.y)
    };
    let fighter = w.fighter(owner_of(w, id)).id;
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_tomandjerry_trap_cheese"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    super::cheese::add_cheese_stack(w, f, 1);
    let t = state_mut(w, id);
    t.hit.damage = Some(5.0); // @4948-5012
    t.hit.base_knock_back = Some(1200.0);
    t.hit.knock_back_scaling = Some(15.0);
}

/// The TriggerVolume's overlap events of the frame (Deviation trap a): the end overlaps first, then the begins in the
/// order the query lists them.
fn overlaps(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (x, y, r) = {
        let s = state(w, id);
        (s.x, s.y, s.trigger_r * scale(w, id))
    };
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)];
    let listed = sphere_overlap_actors(w, x, y, r, &types, &[Some(me)], crate::real(OBJ_WORLD_DYNAMIC));
    let found: Vec<ActorRef> = listed.into_iter().filter(|&o| is_trigger_actor(w, me, o)).collect();
    let keys: Vec<i32> = found.iter().map(|&o| key_of(w, o)).collect();
    let mut i = state(w, id).inside.len();
    while i > 0 {
        i -= 1;
        let k = state(w, id).inside[i];
        if keys.contains(&k) {
            continue;
        }
        state_mut(w, id).inside.remove(i);
        let at = state(w, id).invulnerable.iter().position(|&o| key_of(w, o) == k); // RemoveItem @1307
        if let Some(at) = at {
            state_mut(w, id).invulnerable.remove(at);
        }
    }
    for (o, k) in found.into_iter().zip(keys) {
        if state(w, id).inside.contains(&k) {
            continue;
        }
        state_mut(w, id).inside.push(k);
        begin_overlap(w, id, o);
    }
}

// ---------------------------------------------------------------- the ground

/// ReceiveBeginPlay's ground probe, attach and bind @1748-2645 (see the head).
fn attach_trap(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (x, y) = {
        let s = state(w, id);
        (s.x, s.y)
    };
    let Some(hit) =
        box_trace_single(w, x, y - 50.0, x, y - 150.0, 100.0, 0.0, 100.0, TRACE_PROJECTILE_GROUND, &[], Some(me))
    else {
        return; // K2_AttachToActor(None): nothing; the cast fails
    };
    if let Some(fl) = hit.floor {
        state_mut(w, id).floor = Some(fl);
    } else if let Some(a) = hit.actor {
        let cls = crate::actors::world_actor::actor_cls(w, a).map_or("an actor", |c| w.names.str(c));
        warn_text(
            w,
            &format!(
                "{TRAP}: the ground probe met {cls} ({}): the ride on it is not ported",
                w.names.str(hit.component)
            ),
        );
    }
    bind_ground_destroyed(w, Some(&hit), me, GROUND_DESTROYED);
}

/// GroundDestroyed @5405 -> the hit gate @4328 (no Audio Trigger).
fn ground_destroyed(w: &mut World, a: ActorRef, _wall: WallActor) {
    if let ActorRef::Actor(id) = a
        && !is_dead(w, id)
    {
        hit_gate(w, id);
    }
}

// ---------------------------------------------------------------- the kind

fn step(w: &mut World, id: ActorId) {
    // The attach parent's move of the frame (Deviation trap b). The spawn frame's is already in the spawn place (the
    // owner's location after its base's move), so the first step does not ride.
    if let Some(fl) = state(w, id).floor {
        let p = plat(w, fl);
        let t = state_mut(w, id);
        if !p.dead && t.age > 0.0 {
            t.x += p.dx;
            t.y += p.dy;
        }
    }
    state_mut(w, id).age += 1.0;
    // ReceiveTick @201.
    if !state(w, id).has_hit {
        let list = state(w, id).invulnerable.clone();
        for o in list {
            if invulnerable_now(w, o) {
                continue;
            }
            activate_hit(w, id);
            state_mut(w, id).invulnerable.clear();
            break;
        }
    }
    // The latent actions, behind the tick (AActor::Tick processes them after ReceiveTick).
    if fires(state_mut(w, id), Delay::Gate) {
        state_mut(w, id).trigger_gate = true; // @71
    }
    if fires(state_mut(w, id), Delay::Life) {
        hit_gate(w, id); // @121 GroundDestroyed -> @5405 -> @4328
        sound_on(w, id, name!("sfx_cha_tomandjerry_trap_death")); // @135
    }
    if fires(state_mut(w, id), Delay::Detonate) {
        detonate(state_mut(w, id)); // @15
    }
    if fires(state_mut(w, id), Delay::HitOff) {
        let t = state_mut(w, id);
        t.hit_on = false; // @30
        t.hit_set.clear();
        t.over.clear();
        t.once.clear();
    }
    if fires(state_mut(w, id), Delay::Destroy) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true; // @56
        }
        return;
    }
    // SpawnedActorParent CheckBlastBoxTimer (0.1 s, looping, with DestroyOnExitedArena?).
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let t = state_mut(w, id);
    let frames = t.age - 1.0;
    if blast_timer_exit(doe, &mut t.blast_at, &t.blast_bounds, frames, t.x, t.y) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    if state(w, id).trigger_gate {
        overlaps(w, id);
    }
}

/// The HitBox (radius 200 at the root, HitBoxTags HitBox) while HitBoxOn?, against the enemies (Deviation trap d).
fn hits(w: &mut World, id: ActorId) {
    if is_dead(w, id) {
        return;
    }
    if !state(w, id).hit_on {
        state_mut(w, id).over.clear();
        return;
    }
    let me = ActorRef::Actor(id);
    let own = owner_of(w, id);
    let team = w.fighter(own).team.team;
    let (x, y, r) = {
        let s = state(w, id);
        (s.x, s.y, s.hit_r * scale(w, id))
    };
    let (attack_id, packet) = {
        let s = state(w, id);
        (s.attack_id, s.hit.clone())
    };
    for vic in all_fighters(w) {
        let vid = w.fighter(vic).id;
        if w.fighter(vic).team.team == team
            || !alive(w, vic)
            || crate::combat::phases::no_hit_by_team(w, team, vic)
            || state(w, id).hit_set.contains(&vid)
        {
            continue;
        }
        if !crate::chars::harley::actors::touches(w, own, vic, x, y, r, true) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if packet.hit_each_actor_only_once.unwrap_or(false) {
            state_mut(w, id).hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &packet, &rows, V2 { x, y }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    // The HitBox's begin overlaps with the actors that are no fighter (Deviation trap d).
    if is_dead(w, id) {
        return;
    }
    let s = state_mut(w, id);
    let types = s.types.clone();
    let (mut over, mut once) = (std::mem::take(&mut s.over), std::mem::take(&mut s.once));
    hit_box_actor_hits(w, me, &packet, &types, Some(HitShape::Sphere { x, y, r }), &mut over, &mut once);
    if let Some(a) = w.actors.actors.get_mut(id)
        && let WorldActorState::MouseTrap(s) = &mut a.state
    {
        s.over = over;
        s.once = once;
    }
}

/// ReceiveAnyDamage @5410 (a fighter's DestroyProjectiles hit on ProjectileCollision): MulticastDamageTaken @5439,
/// Detonate.
fn any_damage(w: &mut World, id: ActorId, dmg: Real, by: ActorRef) {
    let _ = (dmg, by);
    if is_dead(w, id) {
        return;
    }
    let t = state_mut(w, id);
    t.sprung = true;
    detonate(t);
}

fn hurt_box(w: &World, id: ActorId) -> Option<WorldSphere> {
    if is_dead(w, id) {
        return None;
    }
    let s = state(w, id);
    Some(WorldSphere { x: s.x, y: s.y, r: s.hurt_r * scale(w, id), ..WorldSphere::default() })
}

/// One sphere body at the root.
fn sphere_body(
    w: &World,
    id: ActorId,
    comp: Option<&mvs_data::value::Props>,
    r: Real,
    name: Name,
    out: &mut Vec<TraceBody>,
) {
    let Some(info) = body_info(comp, None) else { return };
    if !info.on || info.kind < 0 {
        return;
    }
    let s = state(w, id);
    out.push(TraceBody {
        ax: s.x,
        ay: s.y,
        bx: s.x,
        by: s.y,
        hx: 0.0,
        hy: 0.0,
        r,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name,
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// Its bodies for the traces: ProjectileCollision (ProjectileDestructible) always, TriggerVolume once the gate has
/// turned it on (QueryOnly @71), HitBox while HitBoxOn? (UpdateHitbox), each a sphere at the root.
fn bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let a = w.actors.actors.get(id).expect("a live world actor");
    if a.dead {
        return;
    }
    let Some(def) = actor_class_def(w, a.owner, a.cls) else { return };
    let (s, k) = (state(w, id), scale(w, id));
    sphere_body(w, id, obj(&def.components, "ProjectileCollision"), s.hurt_r * k, name!("ProjectileCollision"), out);
    if s.trigger_gate {
        sphere_body(w, id, obj(&def.components, "TriggerVolume"), s.trigger_r * k, name!("TriggerVolume"), out);
    }
    if s.hit_on {
        sphere_body(w, id, obj(&def.components, "HitBox"), s.hit_r * k, name!("HitBox"), out);
    }
}

/// The parent's EndPlay: no DeathVFX or death sound in the class data; the actor's posts stop (StopAll, no fade).
fn end(w: &mut World, id: ActorId) {
    if state(w, id).ended {
        return;
    }
    state_mut(w, id).ended = true;
    let own = owner_of(w, id);
    let seq = w.actors.actors.get(id).expect("a live world actor").seq;
    let fighter = w.fighter(own).id;
    w.events.push(Event::SoundStop {
        fighter,
        key: SoundKey { tag: name!("ak"), n: seq },
        fade: Some(0.0),
        ev: None,
        curve: None,
        cut: false,
    });
}

fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let ad = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let sp = spawn_place(w, f, &np);
    // PostActorConstruction's FindTeleportSpot.
    let at = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: np.clone() },
        &SpawnSpotA { components: ad.components.clone() },
        sp.x,
        sp.y,
    );
    let radius = |c: &str| obj(&ad.components, c).and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0);
    let empty = mvs_data::value::Props::new();
    let mut t = MouseTrap {
        hit: HitPacket::from_props(&w.names, obj(&ad.p, "HitBoxDamage").unwrap_or(&empty), None),
        x: at.x,
        y: at.y,
        facing: facing(w, f),
        attack_id: r.attack_id,
        trigger_r: radius("TriggerVolume"),
        hit_r: radius("HitBox"),
        hurt_r: radius("ProjectileCollision"),
        types: hit_box_types(obj(&ad.components, "HitBox")),
        ..MouseTrap::default()
    };
    // The parent's BeginPlay: the blast-box loop, then NumberOfActorsAllowed (the World's add, behind this call).
    let sb = spawn_box(w);
    let (mut x, mut y) = (t.x, t.y);
    let dead = blast_box_loop(
        &sb,
        &mut x,
        &mut y,
        None,
        flag(&ad.p, "ForceSpawnPositionInsideBlastBox"),
        &mut t.blast_bounds,
        flag(&ad.p, "DestroyOnExitedArena?"),
    );
    (t.x, t.y) = (x, y);
    // ReceiveBeginPlay's own part: the TriggerVolume off (trigger_gate false), the Delays.
    delay(&mut t, Delay::Gate, num_or(&ad.p, "SetDelay", 0.0));
    delay(&mut t, Delay::Life, LIFE);
    (WorldActorState::MouseTrap(t), dead)
}

/// The probe and the bind, once the actor is listed (Unverified trap f).
fn spawned(w: &mut World, id: ActorId) {
    attach_trap(w, id);
}

/// This file's registry entries (MouseTrap_SpawnedActor_C) and the GroundDestroyed delegate's function.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    wall_delegate_fns(r, &[(GROUND_DESTROYED, ground_destroyed)])?;
    let hooks = WorldActorHooks {
        // the spheres read k; the view draws the actor at it
        size: Some(SizeHooks::default()),
        spawn: Some(spawn),
        spawned: Some(spawned),
        step: Some(step),
        hits: Some(hits),
        any_damage: Some(any_damage),
        hurt_box: Some(hurt_box),
        bodies: Some(bodies),
        end: Some(end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        TRAP,
        ActorEntry::new(ActorClass::World(WorldActorClass::MouseTrapSpawnedActor), ActorHooks::World(hooks)),
    )?;
    Ok(())
}
