//! The registry's entries, one type per table. An entry is an object of
//! optional callbacks; here it is a struct of `Option<fn>` with the same names, bound once from the table and copied
//! out before the call (a hook takes `&mut World`). A hook that takes the `Record<string, any>` of a notify or a
//! data row takes the [`NotifyRef`] or the id of what it reads instead; one that takes a hit takes the [`HitPacket`].
//!
//! Every hook a table has is named; its argument types hold ids for object references.

#![allow(
    clippy::type_complexity,
    reason = "a hook table: each hook's signature reads clearest in place, beside its name"
)]

use crate::Real;
use crate::actors::actor_montage::MontageEvent;
use crate::actors::follower::{FollowerMeshVis, FollowerShape};
use crate::actors::item::ItemExt;
use crate::actors::projectile::ProjectileExt;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, ItemId, ProjectileId, StageActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{ExtraHurt, WorldSphere};
use crate::combat::{BuffClass, BuffRef, CondClass};
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::TraceBody;
use crate::names::Name;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg, FighterSignal, OwnerSignal};
use crate::world::World;
use crate::world::stage_actors::StageHurt;
use mvs_data::value::Props;

/// `Condition`: `(f) => boolean`.
pub type CondFn = fn(&mut World, FighterId) -> bool;

/// A `CONDITIONS` or `COND_CLASSES` entry: the two are kept in one namespace (a name in both is registered
/// twice), so they are one table here.
#[derive(Clone, Copy, Debug)]
pub enum Condition {
    Fn(CondFn),
    /// A class with state per fighter (`CondClass`): the state is the fighter's, the behaviour a `match` in
    /// `combat::conditions`.
    Class(CondClass),
}

/// `NotifyHandler`: a fighter notify's hooks, and those of a mesh actor and a stage actor that play the montage.
#[derive(Clone, Copy, Debug, Default)]
pub struct NotifyHandler {
    /// A fighter's begin and end take the notify's key (a montage instance's notify index, below 0 one that is no
    /// notify's: KEY_STRIDE).
    pub begin: Option<fn(&mut World, FighterId, NotifyRef, i32)>,
    pub end: Option<fn(&mut World, FighterId, NotifyRef, i32)>,
    pub instant: Option<fn(&mut World, FighterId, NotifyRef)>,
    /// Entered with `fighterNotifies`.
    pub strict: bool,
    pub actor_begin: Option<fn(&mut World, ActorRef, NotifyRef, u32)>,
    pub actor_end: Option<fn(&mut World, ActorRef, NotifyRef, u32)>,
    pub actor_instant: Option<fn(&mut World, ActorRef, NotifyRef)>,
    pub stage_begin: Option<fn(&mut World, u32, NotifyRef, u32)>,
    pub stage_end: Option<fn(&mut World, u32, NotifyRef, u32)>,
    pub stage_instant: Option<fn(&mut World, u32, NotifyRef)>,
}

/// An `ACTORS` entry (`ActorKind`): the class, the flags the World reads, and its family's hooks.
#[derive(Clone, Copy, Debug)]
pub struct ActorEntry {
    pub class: ActorClass,
    /// `parents`: the classes Comp_Spawner's FilterArray finds it as.
    pub parents: &'static [&'static str],
    pub foreign_owner: bool,
    pub ownerless: bool,
    pub keep_at_lab_reset: bool,
    pub spawned_meet_bodies: bool,
    pub calls_parent_hit_anything: bool,
    /// `elemental`: what of the actor's Comp_Elemental takes part (its box, its overlap, its sphere).
    pub elemental: Option<ElementalParts>,
    pub hooks: ActorHooks,
    /// `carry`: Buff_ProjectileThroughTunnel on an actor of the class.
    pub carry: Option<CarryHooks>,
    /// `mesh`: its skeletal mesh's montages (a world actor and a projectile alike).
    pub mesh: Option<MeshHooks>,
    /// `hazard`: BPI_EnvironmentalHazard (a world actor and a projectile alike).
    pub hazard: Option<HazardHooks>,
    /// `follower`: BPI_Follower and its getters.
    pub follower: Option<FollowerHooks>,
}

impl ActorEntry {
    /// An entry of `class` with `hooks` and every flag off.
    pub const fn new(class: ActorClass, hooks: ActorHooks) -> Self {
        ActorEntry {
            class,
            parents: &[],
            foreign_owner: false,
            ownerless: false,
            keep_at_lab_reset: false,
            spawned_meet_bodies: false,
            calls_parent_hit_anything: false,
            elemental: None,
            hooks,
            carry: None,
            mesh: None,
            hazard: None,
            follower: None,
        }
    }
}

/// `ActorKind.elemental`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ElementalParts {
    pub box_: bool,
    pub overlap: bool,
    pub sphere: bool,
}

/// `MeshHooks`: an actor's montages played with a PlayMontageCallbackProxy, and the
/// AnimNotify_Hitframe of its own Comp_Pawn_HitBoxHandler. A stage actor is `ActorRef::Stage`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshHooks {
    /// `montageEvent`: the event and the montage.
    pub montage_event: Option<fn(&mut World, ActorRef, MontageEvent, Name)>,
    /// `notifyBegin` / `notifyEnd`: the notify's name and the montage.
    pub notify_begin: Option<fn(&mut World, ActorRef, Name, Name)>,
    pub notify_end: Option<fn(&mut World, ActorRef, Name, Name)>,
    /// `hitBoxesOn`: the tags, the packet the notify builds, its scale (X, Y, Z), the notify's index.
    pub hit_boxes_on: Option<fn(&mut World, ActorRef, &[Name], &HitPacket, [Real; 3], u32)>,
    pub hit_boxes_off: Option<fn(&mut World, ActorRef, &[Name], u32)>,
}

/// `CarryHooks`: what Buff_ProjectileThroughTunnel does to an actor; the `u32` is the
/// carry's buff on the actor's Comp_State_Manager.
#[derive(Clone, Copy, Debug)]
pub struct CarryHooks {
    pub moving: fn(&World, ActorRef) -> bool,
    pub team: fn(&World, ActorRef) -> i32,
    /// `where`: K2_GetActorLocation.
    pub at: fn(&World, ActorRef) -> V2,
    pub place: fn(&mut World, ActorRef, Real, Real),
    pub velocity: fn(&World, ActorRef) -> V2,
    pub set_velocity: fn(&mut World, ActorRef, Real, Real),
    pub collision: fn(&mut World, ActorRef, u32, bool),
    pub pause_life: fn(&mut World, ActorRef, u32, bool),
    pub tick: Option<fn(&mut World, ActorRef, u32)>,
}

/// `ActorKind.hazard`: does a component of it touch the sphere (x, y, r) now; its BPI Clean.
#[derive(Clone, Copy, Debug)]
pub struct HazardHooks {
    pub overlaps: fn(&World, ActorRef, Real, Real, Real) -> bool,
    pub clean: fn(&mut World, ActorRef) -> bool,
}

/// `FollowerHooks`.
#[derive(Clone, Copy, Debug)]
pub struct FollowerHooks {
    /// The class has a Comp_Follower.
    pub comp: bool,
    pub mesh: fn(&mut World, ActorRef, FollowerMeshVis),
    pub is_attacking: fn(&World, ActorRef) -> bool,
    pub is_flying: fn(&World, ActorRef) -> bool,
    pub is_falling: fn(&World, ActorRef) -> bool,
    /// Its overlap shapes now, into the list (cleared by the caller).
    pub shapes: fn(&World, ActorRef, &mut Vec<FollowerShape>),
}

/// `ReflectHooks`: an item's or a projectile's overrides of SpawnedActorParent's
/// Reflected / MulticastReflect.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReflectHooks {
    pub reflected: Option<fn(&mut World, ActorRef)>,
    /// The new owner, ShouldPause.
    pub multicast: Option<fn(&mut World, ActorRef, FighterId, bool)>,
    pub multicast_pause: Option<fn(&World, ActorRef, FighterId, bool) -> bool>,
    pub can_be_reflected: Option<fn(&World, ActorRef) -> bool>,
    /// The HitBoxDamage variable BPI_Reflected scales, for a class whose graph keeps it apart from its boxes' packet.
    pub hit_box_damage: Option<fn(&mut World, ActorRef) -> &mut HitPacket>,
}

/// `ActorKind.size`: BPI_Scale's work on the actor.
#[derive(Clone, Copy, Debug, Default)]
pub struct SizeHooks {
    /// The new scale and the old.
    pub scale: Option<fn(&mut World, ActorId, Real, Real)>,
    pub capsule_half: Option<fn(&World, ActorId) -> Real>,
    pub move_z: Option<fn(&mut World, ActorId, Real)>,
}

/// An `ActorKind.damageBox` row: a component of the actor that deals its hits, and where it is now.
#[derive(Clone, Copy, Debug)]
pub struct DamageBox {
    pub comp: &'static str,
    pub at: fn(&mut World, ActorId) -> Option<WorldSphere>,
}

/// The family part of an [`ActorEntry`]: `projectile` (`ProjectileKind`), `item` (`ItemKind`), or the world actor
/// hooks of `ActorKind` itself.
#[derive(Clone, Copy, Debug)]
pub enum ActorHooks {
    Projectile(ProjectileHooks),
    Item(ItemHooks),
    World(WorldActorHooks),
}

/// `ActorKind`'s hooks for a world actor.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldActorHooks {
    /// BPI_GetProjectileAttackType of a dealer of World.actors (none: Normal).
    pub attack_type: Option<fn(&World, ActorId) -> i32>,
    /// BPI_CanProjectileClank of a dealer of World.actors (none: false).
    pub can_clank: Option<fn(&World, ActorId) -> bool>,
    /// `spawn(w, f, r, a)`: the class's state made from the request (its actor data is the owner's `actor_def` of
    /// `r.cls`), and whether the parent's BeginPlay already destroyed it (`dead`); the World makes the actor around it.
    pub spawn: Option<fn(&mut World, FighterId, &SpawnRequest) -> (WorldActorState, bool)>,
    /// The rest of the kind's `spawn` that needs the actor's id: run once the World has listed it (not for one
    /// `spawn` made dead), ahead of `add_actor` (its number, the lists, the parent's BeginPlay part).
    pub spawned: Option<fn(&mut World, ActorId)>,
    /// `deferred`: the class made at a deferred spawn by the owner (the class's name, the place); the actor made.
    pub deferred: Option<fn(&mut World, FighterId, Name, Real, Real) -> ActorId>,
    /// `pawnBody`: its capsule as a pawn's hurt body.
    pub pawn_body: Option<fn(&World, ActorId) -> Option<ExtraHurt>>,
    pub hurt_box: Option<fn(&World, ActorId) -> Option<WorldSphere>>,
    /// `destructible`: its Comp_Actor_Destructible values now.
    pub destructible: Option<fn(&World, ActorId) -> Option<&Props>>,
    /// `hitShapes`: its hit shapes now, into the list; false for none (null).
    pub hit_shapes: Option<fn(&World, ActorId, &mut Vec<WorldSphere>) -> bool>,
    pub size: Option<SizeHooks>,
    pub damage_box: &'static [DamageBox],
    /// `hitBoxOn`: a projectile's box met one of its components (the component's name, the box).
    pub hit_box_on: Option<fn(&mut World, ActorId, ProjectileId, Name, &WorldSphere)>,
    /// `bodies`: its trace bodies, pushed to the list.
    pub bodies: Option<fn(&World, ActorId, &mut Vec<TraceBody>)>,
    pub begin_play: Option<fn(&mut World, ActorId)>,
    pub activate: Option<fn(&mut World, ActorId)>,
    pub reverse_move: Option<fn(&mut World, ActorId)>,
    pub set_condition: Option<fn(&mut World, ActorId, i32, bool)>,
    pub step: Option<fn(&mut World, ActorId)>,
    pub late: Option<fn(&mut World, ActorId)>,
    pub hits: Option<fn(&mut World, ActorId)>,
    pub any_damage: Option<fn(&mut World, ActorId, Real, ActorRef)>,
    pub destroy: Option<fn(&mut World, ActorId)>,
    pub spawned_moved: Option<fn(&mut World, ActorId, ActorRef)>,
    pub blocked: Option<fn(&mut World, ActorId, ActorRef)>,
    /// (the actor, the index of the box the capsule hit, the fighter)
    pub wall_bounce_multiplier: Option<fn(&mut World, ActorId, u32, FighterId) -> Real>,
    pub end: Option<fn(&mut World, ActorId)>,
    pub jump_pressed: Option<fn(&mut World, ActorId, FighterId)>,
    pub giant_crouch_changed: Option<fn(&mut World, ActorId, FighterId, bool)>,
    pub projectile_moved: Option<fn(&mut World, ActorId, ProjectileId)>,
    pub ignore_platform_collision: Option<fn(&mut World, ActorId, FighterId, bool, Real)>,
    pub ignore_wallbounce_collision: Option<fn(&mut World, ActorId, FighterId, bool)>,
    pub on_hit_anything: Option<fn(&mut World, ActorId, ActorRef)>,
    pub owner_signal: Option<fn(&mut World, ActorId, FighterId, OwnerSignal, bool)>,
    pub fighter_signal: Option<fn(&mut World, ActorId, FighterId, FighterSignal, bool, Real, Name, Option<FighterId>)>,
}

/// `ProjectileKind`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProjectileHooks {
    /// The constructor's tail (the subclass's constructor body): the class's own state in `ext` and the base
    /// fields it writes from its class data, on the projectile `make_projectile` built, ahead of the list.
    pub construct: Option<fn(&World, &mut crate::actors::Projectile)>,
    /// `ActorKind.bodies`: its trace bodies, pushed to the list.
    pub bodies: Option<fn(&World, ActorRef, &mut Vec<TraceBody>)>,
    /// `make` / `makeWorld`: the class's own state (the subclass's fields) for a spawn by a fighter or by the
    /// world (the fighter looked up through).
    pub make: Option<fn(&World, FighterId, &SpawnRequest) -> ProjectileExt>,
    pub make_world: Option<fn(&World, FighterId, &SpawnRequest) -> ProjectileExt>,
    /// The options the subclass's `make` passes to the Projectile constructor (`{ tether }` of the tether
    /// family), for a spawn by a fighter; the spawn sets `by_fighter` after it.
    pub make_opts: Option<fn(&World, FighterId, &SpawnRequest) -> crate::actors::projectile::MakeOpts>,
    pub begin: Option<fn(&mut World, ProjectileId)>,
    pub receive_hit: Option<ProjectileReceiveHit>,
    pub damage_box: Option<fn(&World, ProjectileId) -> Option<WorldSphere>>,
    pub reflect: ReflectHooks,
    /// `step`: true ends the projectile's own step.
    pub step: Option<fn(&mut World, ProjectileId) -> bool>,
    /// The class's override of the parent's step (`Projectile.step`), called ahead of it on a living projectile:
    /// `Some` takes the tick, with why the projectile died; `None` runs the parent's step.
    pub step_override: Option<fn(&mut World, ProjectileId) -> Option<Option<crate::actors::projectile::StepEnd>>>,
    pub physics: Option<fn(&mut World, ProjectileId)>,
    pub own_tick: bool,
    pub bounce_impact: Option<fn(&mut World, ProjectileId, Real, Real, Real, Real) -> bool>,
    pub moved: Option<fn(&mut World, ProjectileId)>,
    pub split: Option<fn(&mut World, ProjectileId)>,
    pub overlap: Option<fn(&mut World, ProjectileId, FighterId)>,
    pub non_item_pickup: Option<fn(&mut World, ProjectileId, FighterId)>,
    pub pickup_label: bool,
    pub hitbox_begin_overlap: Option<fn(&mut World, ProjectileId, ActorRef)>,
    pub reflecting_projectiles: Option<fn(&World, ProjectileId) -> bool>,
    pub hit_enemy: Option<fn(&mut World, ProjectileId, FighterId)>,
    pub can_mark: Option<fn(&World, ProjectileId) -> bool>,
    pub hit_by: Option<fn(&mut World, ProjectileId, bool, Real, Real, Real, i32, Option<ActorRef>)>,
    pub move_command_completed: Option<fn(&mut World, ProjectileId, FighterId, bool)>,
    pub hit_non_player: Option<fn(&mut World, ProjectileId, ActorRef)>,
    /// `ActorKind.onHitAnything`: the class's own OnHitAnything, called by `combat::hits::hit_anything` on a living
    /// projectile once a hit went through, behind the parent's DestroyedOnHit part when the entry has
    /// `calls_parent_hit_anything` (no parent part without it).
    pub hit_anything: Option<fn(&mut World, ProjectileId, ActorRef)>,
    pub hit_ally: Option<fn(&mut World, ProjectileId, FighterId)>,
    pub blocked: Option<fn(&mut World, ProjectileId, ActorRef)>,
    pub activate: Option<fn(&mut World, ProjectileId)>,
    pub any_damage: Option<fn(&mut World, ProjectileId, Real, ActorRef) -> bool>,
    /// `fighterSignal`: a fighter's broadcasts (`FighterSignal`) on any fighter of the match, as
    /// `WorldActorHooks::fighter_signal` (C023A_Car_SpawnedActor's and the item cars' driver binds): the broadcasting
    /// fighter, the signal, flag, num, tag, by.
    pub fighter_signal:
        Option<fn(&mut World, ProjectileId, FighterId, FighterSignal, bool, Real, Name, Option<FighterId>)>,
    pub destroyed: Option<fn(&mut World, ProjectileId)>,
    pub spawns_at_death: Option<fn(&World, ProjectileId) -> bool>,
    /// `ActorKind.reverseMove`: the class's own BPI_ReverseMoveDirection (`combat::reverse`).
    pub reverse_move: Option<fn(&mut World, ProjectileId)>,
    pub end: Option<fn(&mut World, ProjectileId)>,
}

/// `ItemKind`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemHooks {
    /// `make`: the item made for a spawn (the request, the place).
    pub make: Option<fn(&mut World, FighterId, &SpawnRequest, Real, Real) -> ItemId>,
    /// `sub`: the subclass; here the class's own state.
    pub sub: Option<fn() -> ItemExt>,
    pub reflect: ReflectHooks,
    pub begin: Option<fn(&mut World, ItemId) -> bool>,
    pub thrown: Option<fn(&mut World, ItemId)>,
    pub throw_hit: Option<fn(&mut World, ItemId, bool, bool, Real, Real)>,
    pub activate: Option<fn(&mut World, ItemId, FighterId)>,
    pub step: Option<fn(&mut World, ItemId) -> bool>,
    pub picked_up: Option<fn(&mut World, ItemId, FighterId) -> bool>,
    pub grabbed: Option<fn(&mut World, ItemId, FighterId)>,
    pub pick_up_override: Option<fn(&mut World, ItemId, FighterId) -> bool>,
    pub hit_enemy: Option<fn(&mut World, ItemId, FighterId) -> bool>,
    pub tick: Option<fn(&mut World, ItemId) -> bool>,
    pub timer: Option<fn(&mut World, ItemId) -> bool>,
    pub overlaps: Option<fn(&mut World, ItemId, Option<FighterId>) -> bool>,
    pub end: Option<fn(&mut World, ItemId)>,
    pub receive_hit: Option<fn(&mut World, ItemId, bool) -> bool>,
    /// OnComponentHit of one of the step's body contacts.
    pub component_hit: Option<fn(&mut World, ItemId, crate::actors::item::ItemContact) -> bool>,
    pub hit_non_player: Option<fn(&mut World, ItemId, bool) -> bool>,
    /// `ActorKind.onHitAnything`: as [`ProjectileHooks::hit_anything`], for an item.
    pub hit_anything: Option<fn(&mut World, ItemId, ActorRef)>,
}

/// `ProjectileKind.receiveHit`: its own boxes its root meets besides the stage block (into the list, cleared first)
/// and a hit on one (the normal, the sweep's time, penetrating); true ends it.
#[derive(Clone, Copy, Debug)]
pub struct ProjectileReceiveHit {
    pub boxes: fn(&mut World, ProjectileId, &mut crate::world::stage_query::StageShapes),
    pub hit: fn(&mut World, ProjectileId, Real, Real, Real, bool) -> bool,
}

/// A `BUFFS` entry (`BuffKind`): the class, its flags, its hooks.
#[derive(Clone, Copy, Debug)]
pub struct BuffEntry {
    pub class: BuffClass,
    pub end_skips_parent: bool,
    pub block_fast_fall: bool,
    /// `thruAtk`: ground, air, rotation.
    pub thru_atk: Option<[bool; 3]>,
    pub active_attack: bool,
    pub damage_hit_by_ally: bool,
    pub hooks: BuffHooks,
    /// `actor`: the class runs on an actor's Comp_State_Manager too.
    pub actor: Option<ActorBuffHooks>,
}

impl BuffEntry {
    /// An entry of `class` with `hooks` and every flag off.
    pub const fn new(class: BuffClass, hooks: BuffHooks) -> Self {
        BuffEntry {
            class,
            end_skips_parent: false,
            block_fast_fall: false,
            thru_atk: None,
            active_attack: false,
            damage_hit_by_ally: false,
            hooks,
            actor: None,
        }
    }
}

/// `BuffKind`'s hooks. A hook returning `bool` returns `true` (handled / ended) as true.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuffHooks {
    pub begin: Option<fn(&mut World, FighterId, BuffRef, Option<FighterId>, Option<Real>)>,
    pub tick: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
    pub on: Option<fn(&mut World, FighterId, BuffRef, BuffSignal, &BuffSignalArg) -> bool>,
    pub take_any_damage: Option<fn(&mut World, FighterId, BuffRef, Real, Option<FighterId>) -> bool>,
    pub end: Option<fn(&mut World, FighterId, BuffRef)>,
    pub branch_group_blocked: Option<fn(&mut World, FighterId, BuffRef, NotifyRef, Name, Real)>,
    pub branch_group_end_blocked: Option<fn(&mut World, FighterId, BuffRef, NotifyRef)>,
    pub actor_collision_off: Option<fn(&World, FighterId, BuffRef) -> bool>,
    pub timer: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
    pub max_stacks: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
    pub stacks: Option<fn(&mut World, FighterId, BuffRef, bool) -> bool>,
    pub cleanse: Option<fn(&mut World, FighterId, BuffRef) -> bool>,
    pub status: Option<fn(&mut World, FighterId, BuffClass, Real, bool, bool)>,
    /// The victim, the owner, the dealer, the hit, the buff, IsValid(HitCollision).
    pub dealt_cond: Option<fn(&mut World, ActorRef, FighterId, FighterId, &HitPacket, BuffRef, bool) -> bool>,
    /// The owner, the buff, the attacker, boxed, the hit, the attacker's fighter.
    pub taken_cond:
        Option<fn(&mut World, FighterId, BuffRef, Option<ActorRef>, bool, &HitPacket, Option<FighterId>) -> bool>,
    pub attach_parent: Option<fn(&World, FighterId, BuffRef) -> Option<FighterId>>,
}

/// `ActorBuffKind`: a buff on an actor's Comp_State_Manager.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorBuffHooks {
    pub begin: Option<fn(&mut World, ActorRef, u32, Option<FighterId>) -> bool>,
    pub tick: Option<fn(&mut World, ActorRef, u32) -> bool>,
    /// The fighters and the actors the host's box or sphere overlaps.
    pub overlap_timer: Option<fn(&mut World, ActorRef, u32, &[FighterId], &[ActorRef])>,
    /// The signal and its argument (`HitByEnemy`'s fighter and knockback, `ActorBuffArg`).
    pub on: Option<fn(&mut World, ActorRef, u32, ActorBuffSignal, Option<&ActorBuffArg>) -> bool>,
    pub end: Option<fn(&mut World, ActorRef, u32)>,
    pub stacks: Option<fn(&mut World, ActorRef, u32, bool) -> bool>,
    /// `On Max Stacks`: AddStack at MaxStacks on an actor's entry; true when the hook destroyed it.
    pub max_stacks: Option<fn(&mut World, ActorRef, u32) -> bool>,
    pub hurt_sphere: Option<fn(&World, ActorRef, u32) -> Real>,
    /// `hitboxSphere`: the sphere it adds to itself (x, y, r), none for none.
    pub hitbox_sphere: Option<fn(&World, ActorRef, u32) -> Option<[Real; 3]>>,
    /// `values`: the variables its graph wrote over its class defaults.
    pub values: Option<fn(&World, ActorRef, u32) -> Option<Props>>,
    pub taken_cond: Option<fn(&World, ActorRef, u32, FighterId) -> bool>,
    /// The victim, the hit, IsValid(HitCollision).
    pub dealt_cond: Option<fn(&mut World, ActorRef, u32, ActorRef, &HitPacket, bool) -> bool>,
}

/// A `PICKUPS` entry (`PickupHooks`), keyed by a character.
#[derive(Clone, Copy, Debug)]
pub struct PickupEntry {
    pub picked_up: fn(&mut World, FighterId, ItemId),
    pub tick: Option<fn(&mut World, FighterId)>,
}

/// A `STAGE_HOOKS` entry (`StageHooks`), keyed by the map. The stage's state is the World's `StageState`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageEntry {
    pub create: Option<fn(&mut World)>,
    /// `tickers`: the level actors that tick now, by owner number, in registration order (kept in the stage's state).
    pub tickers: Option<fn(&World) -> &[i32]>,
    pub pre_tick: Option<fn(&mut World, i32)>,
    pub tick: Option<fn(&mut World)>,
    pub item_moved: Option<fn(&mut World, ItemId)>,
    pub timer: Option<fn(&mut World, i32, i32)>,
    pub ring_out: Option<fn(&mut World, FighterId)>,
    pub reset: Option<fn(&mut World)>,
}

/// A `STAGE_ACTORS` entry (`StageActorKind`), a stage actor by its place in the stage's list.
#[derive(Clone, Copy, Debug)]
pub struct StageActorEntry {
    pub class: StageActorClass,
    pub hurt_type: Option<i32>,
    pub friendly_fire: bool,
    pub can_take_damage: bool,
    pub hit_pause_multiplier: Option<Real>,
    pub surface: Option<i32>,
    pub create: Option<fn(&mut World, u32)>,
    pub any_damage: Option<fn(&mut World, u32, Real, ActorRef)>,
    /// The causer, the hit, the damage.
    pub hit_by_enemy: Option<fn(&mut World, u32, ActorRef, &HitPacket, Real)>,
    /// The causer, the hit, ForceKnockback.
    pub hit_by_ally: Option<fn(&mut World, u32, ActorRef, &HitPacket, bool)>,
    pub knocked_back: Option<fn(&mut World, u32, ActorRef, &HitPacket)>,
    /// `hurt`: its hurt shapes now, into the list; false for none.
    pub hurt: Option<fn(&World, u32, &mut Vec<StageHurt>) -> bool>,
    /// `sounds`: the sound events its hits play.
    pub sounds: &'static [&'static str],
    pub damage_disabled: Option<fn(&World, u32) -> bool>,
    pub invulnerable: Option<fn(&World, u32) -> bool>,
    pub current_damage: Option<fn(&World, u32) -> Real>,
    /// `victimPause`: the hit pause it takes (seconds).
    pub victim_pause: Option<fn(&mut World, u32, Real)>,
    pub mesh: Option<MeshHooks>,
    pub reset: Option<fn(&mut World, u32)>,
    /// `solidHit`: a blocking hit of a mover (a fighter, a projectile, an item) on one of its solids: (slot, the
    /// solid's data id, the mover, the hit's normal as its event gets it).
    pub solid_hit: Option<fn(&mut World, u32, u32, crate::actors::ActorRef, Real, Real)>,
}

/// `ACTORS.get(cls)?.bodies` of a projectile's or an item's class:
/// `ProjectileHooks::bodies`; `ItemHooks` have no `bodies` yet (`WorldActorHooks::bodies` is read
/// by the traces themselves); `None` for a class without the hook, a panic for one whose kind has it (not ported yet).
pub fn spawned_bodies_hook(w: &World, a: ActorRef) -> Option<fn(&World, ActorRef, &mut Vec<TraceBody>)> {
    // A projectile's own `bodies` hook (the item's and the other classes' wait on theirs).
    if let ActorRef::Projectile(_) = a
        && let Some(ActorHooks::Projectile(h)) = crate::actors::world_actor::actor_entry(w, a).map(|e| e.hooks)
        && let Some(b) = h.bodies
    {
        return Some(b);
    }
    let cls = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| p.cls),
        ActorRef::Item(id) => w.actors.items.get(id).map(|i| i.cls),
        _ => None,
    };
    // A class whose kind has no `bodies` has none here either; one that has them waits on the hook.
    if cls.is_some_and(|c| BODIES_CLASSES.contains(&w.names.str(c))) {
        crate::unported!("ActorKind.bodies of the registry", w, a)
    }
    None
}

/// The classes whose `ActorKind` has `bodies`.
const BODIES_CLASSES: &[&str] = &[
    "AutoTeleportPortal_C",
    "BatMobile_SpawnedActor_Child_C",
    "BugsBunnyNeutralSafe_C",
    "BugsBunnyRocketUpProjectile_C",
    "BugsBunnySafe_C",
    "BugsBunnyTunnelParent_C",
    "BugsBunny_SafeDrop_C",
    "C016ProjectileBarrier_C",
    "C018_PerkPlatform_C",
    "C018_Platform_C",
    "C019ScrubShield_C",
    "C020PortalAimer_C",
    "C021_Bolt_C",
    "C021_EarthSmashIn_C",
    "C021_ForceField_C",
    "C021_ForceField_Perk_C",
    "C023A_Car_SpawnedActor_C",
    "C023A_PopCorn_C",
    "C023A_ProjectileBarrier_C",
    "ExitPortalParent_C",
    "FartBomb_SpawnedActor_C",
    "Finn_Dash_AimerExperiment_C",
    "Finn_Shop_C",
    "FireStormHazard_C",
    "Firebolt_C",
    "Firebolt_Child_C",
    "GarnetElectricZone_C",
    "HarleyBomb_C",
    "HarleyJackInTheBox_C",
    "HealOverTimeZoneBP_C",
    "ItemDispenser_Bugs_C",
    "ItemHelper_Nepter_C",
    "JakeAimPunchEatListener_C",
    "JakeAimer_C",
    "Jerry_SpawnedActor_C",
    "Jerry_SpawnedActor_TempInvincibility_C",
    "LingeringLoveProjectile_C",
    "Meeseeks_SpawnedActor_C",
    "MouseTrap_SpawnedActor_C",
    "MysteryMachine_SpawnedCar_C",
    "Parent_Car_SpawnedActor_C",
    "PolymorphGrenade_C",
    "ProjectileBarrier_C",
    "ScoobySnackBox_C",
    "SeismicReactor_C",
    "StevenCollisionBlock_C",
    "Steven_ShieldActor_C",
    "Steven_ShieldActor_U_SpecialPlatform_C",
    "Steven_ShieldActor_Up_C",
    "TomAndJerry_SlingShot_Aim_02_C",
    "Velma_ProjectileShieldActor_C",
    "VisualOnlyPortal_C",
    "WW_BracerShield_C",
    "WW_EvadeProjectileBlock_C",
    "WW_ShieldActor_C",
    "WW_ShieldActor_U_Special_C",
];
