//! ItemDispenser_Bugs_C (an ItemDispenserActorParent child, ability 4, the ACME
//! box): the actor Pawn_BugsBunny's BeginPlay equips through its Comp_Spawner (CompSpawnActor attached to him, socket
//! None, world transforms, identity: Pawn_BugsBunny @65), [`Dispenser`], a world actor whose items are
//! `crate::items`'. Only its Capsule and ProjectileMovement; Die With Instigator and DestroyOnExitedArena? are false,
//! so it lives as long as he does.
//! BugsBunny_Nav_Crouch_Loop's two AnimNotify_ActivateSpawned states (the `_C_0` Begin, the `_C_1` End) reach it
//! through the world's activation pass (`activate`): SpawnedActorParent ActivateObject @8831 is a bare return and the
//! dispenser never reads ActivationConditions, so both calls run ActivateObject @1873:
//!   - the owner's Comp_Actor_Destructible GetAllies (the other fighters of his team not knocked out, in the sim's
//!     order); nonempty: GetClosestAlly, which answers Allies[0]'s location whatever the distances (its loop compares
//!     Allies[0] with itself, @434 / @633); empty: his own location (@2346);
//!   - MulticastTriggerAbility(location) @3214: DisableCD skips the checks (false in the CDO: not ported);
//!     TriggerAbilityCheck(4) (its failure gives the cooldown feedback), TriggerAbilityActual(4) (true only with the
//!     game mode's cooldowns on: Lab instant cooldowns give no item, by the graph); then the gate @38: closed ->
//!     nothing; else closed, SpawnItem, Delay 5 (@107) opens it (@15).
//! SpawnItem @187: Array_Random(ItemSpawnList); N = Normal(location - his, 1e-4); facing left N, else N x (1, -1, 1);
//! rotated 15 degrees about -X or, with its Y > 0, +X; speed Lerp(400, 1250, clamp(distance / 2000, 0, 1));
//! CompSpawnActor(class, not attached, transform (0, 0, 100) relative, that direction and speed) as a notify's item
//! spawn (`comp_spawn_item` through the same rule as the notify's spawn place). With no ally the direction is zero: the
//! item drops at his place. BPI_IncrementCoopActions (the player state's stat) is not ported.
//! The Capsule's BeginOverlap trigger (@2976: an ally entering it) is dead: SetActive, which turns the collision on,
//! has no caller.

#![allow(clippy::doc_lazy_continuation, reason = "comment blocks kept as written")]

use crate::Real;
use crate::State;
use crate::actors::{ActorId, ActorRef, WorldActorClass, WorldActorState};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, first_ally, latent_frames};
use crate::fighter::control::{data_name, game_mode_cooldown_disabled, trigger_ability, trigger_ability_check};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::{actor_y, facing};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, rotate_angle_axis};
use crate::math::{max, min, to_int32};
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::comp_spawn_item;
use mvs_data::value::Value;

/// `DISPENSER`.
pub const DISPENSER: &str = "ItemDispenser_Bugs_C";
/// The ability the dispenser triggers.
const ABILITY: i32 = 4;

/// `Dispenser` (ItemDispenser_Bugs_C): `closed`, the gate's IsClosed; `reopen`, the frames left of the Delay(5) that
/// opens it, 0: none running.
#[derive(State, Debug, Default)]
pub struct Dispenser {
    pub closed: bool,
    pub reopen: Real,
}

fn state(w: &mut World, id: ActorId) -> &mut Dispenser {
    match &mut w.actors.actors.get_mut(id).expect("a live actor").state {
        WorldActorState::Dispenser(d) => d,
        s => panic!("ItemDispenser_Bugs_C without its state: {s:?}"),
    }
}

/// `spawnItem`: SpawnItem @187 for the target at (rx, ry).
fn spawn_item(w: &mut World, owner: FighterId, rx: Real, ry: Real) {
    let list: Vec<String> = actor_def(w, ActorRef::Fighter(owner), data_name(w, DISPENSER))
        .and_then(|a| a.p.get("ItemSpawnList"))
        .and_then(|v| match v {
            Value::Arr(a) => Some(a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()),
            _ => None,
        })
        .unwrap_or_default();
    if list.is_empty() {
        return;
    }
    // Array_Random @462: the world's stream
    let i = usize::try_from(to_int32(w.random_int(crate::real(u32::try_from(list.len()).expect("a short list")))))
        .expect("a list index");
    let cls = data_name(w, &list[i]);
    let (fx, fy, face) = (w.fighter(owner).body.x, actor_y(w, owner), facing(w, owner));
    // In UE axes: Y = -x, Z = y.
    let (uy, uz) = (-(rx - fx), ry - fy);
    let ss = uy * uy + uz * uz;
    let [_, ny, nz] = normal([0.0, uy, uz], KINDA_SMALL_NUMBER); // @752
    let sy = if face < 0.0 { ny } else { -ny };
    let sz = nz;
    let sg = if sy > 0.0 { 1.0 } else { -1.0 }; // +X axis with Y > 0, -X otherwise: both tilt it up
    // RotateAngleAxis(v, 15, (+-1, 0, 0)) @1077 / @1132 (a unit axis: the node's GetSafeNormal keeps it)
    let [_, dy, dz] = rotate_angle_axis([0.0, sy, sz], 15.0, [sg, 0.0, 0.0]);
    let t = min(max(ss.sqrt() / 2000.0, 0.0), 1.0);
    let speed = 400.0 + (1250.0 - 400.0) * t;
    comp_spawn_item(w, cls, owner, fx, fy, dy * speed * face, dz * speed, false, None);
}

/// `trigger`: MulticastTriggerAbility(location).
fn trigger(w: &mut World, id: ActorId, owner: FighterId, x: Real, y: Real) {
    if !trigger_ability_check(w, owner, ABILITY) {
        return;
    }
    // TriggerAbilityActual's Triggered (Comp_Pawn_AbilityManager): its body runs only with the cooldowns on.
    let triggered = !game_mode_cooldown_disabled(w, owner);
    trigger_ability(w, owner, ABILITY);
    if !triggered || state(w, id).closed {
        return;
    }
    state(w, id).closed = true;
    spawn_item(w, owner, x, y);
    state(w, id).reopen = latent_frames(5.0);
}

/// The pawn's request (`pawn.rs` BeginPlay) names the class alone; its fields are not read.
// Deviation: the port's respawn keeps the Fighter and runs its BeginPlay again, where the build's new pawn has a new
// Comp_Spawner (the old pawn's dispenser goes with the old pawn's teardown): an older live dispenser of the same
// owner is dropped here so one stays.
fn spawn(w: &mut World, f: FighterId, _r: &SpawnRequest) -> (WorldActorState, bool) {
    let cls = crate::name!("ItemDispenser_Bugs_C");
    let ids: Vec<ActorId> = w.actors.actors.ids().collect();
    for id in ids {
        if let Some(x) = w.actors.actors.get_mut(id)
            && x.cls == cls
            && x.owner == f
        {
            x.dead = true;
        }
    }
    (WorldActorState::Dispenser(Dispenser::default()), false)
}

/// `activate`: the closest ally's place, else the owner's own. ItemDispenser_Bugs GetClosestAlly reads
/// `Array_Get(Allies, 0)` at @5, @434 and @633, never its loop's index: always the first of the owner's GetAllies
/// (`common::first_ally`), not the closest; kept as the build has it.
fn activate(w: &mut World, id: ActorId) {
    let owner = w.actors.actors.get(id).expect("a live actor").owner;
    let ally = first_ally(w, owner);
    let at = ally.unwrap_or(owner);
    let (x, y) = (w.fighter(at).body.x, actor_y(w, at));
    trigger(w, id, owner, x, y);
}

/// The Delay(5)'s countdown.
fn step(w: &mut World, id: ActorId) {
    let d = state(w, id);
    if d.reopen > 0.0 {
        d.reopen -= 1.0;
        if d.reopen == 0.0 {
            d.closed = false;
        }
    }
}

/// Its only body, the Capsule, is NoCollision (a Custom profile with no ObjectType; SetActive, which would turn it on,
/// has no caller): no trace or overlap meets it.
fn bodies(_w: &World, _id: ActorId, _out: &mut Vec<crate::fighter::traces::TraceBody>) {}

/// This file's registry entries (ItemDispenser_Bugs_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        // no shape and nothing drawn: its items spawn at the owner
        size: Some(SizeHooks::default()),
        spawn: Some(spawn),
        activate: Some(activate),
        step: Some(step),
        bodies: Some(bodies),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        DISPENSER,
        ActorEntry::new(crate::actors::ActorClass::World(WorldActorClass::ItemDispenserBugs), ActorHooks::World(hooks)),
    )
}
