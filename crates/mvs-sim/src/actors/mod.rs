//! The spawned actors: projectiles, items and world actors,
//! each family in its own arena of the World. A family's shared fields are a struct ([`Projectile`], [`Item`],
//! [`WorldActor`]); what a subclass adds is a variant of the family's state enum ([`ProjectileExt`],
//! [`ItemExt`], [`WorldActorState`]); which Blueprint class it is, a variant of [`classes`]. Behaviour is the class's
//! registry entry (fn pointers bound once) and `match`es on the state, never a trait object.

pub mod actor_montage;
pub mod barrier;
pub mod carry_run;
pub mod classes;
pub mod firewall;
pub mod follower;
pub mod homing;
pub mod item;
pub mod item_bounce;
pub mod lists;
pub mod predict_path;
pub mod projectile;
pub mod projectile_move;
pub mod shield_actor;
pub mod smoke_bomb;
pub mod spawn_area;
pub mod spline;
pub mod tether_shape;
pub mod tunnel_carry;
pub mod world_actor;

pub use classes::{ItemClass, ProjectileClass, StageActorClass, WorldActorClass};
pub use item::{Item, ItemExt};
pub use projectile::{Projectile, ProjectileExt};
pub use world_actor::{WorldActor, WorldActorState};

use crate::State;
use crate::fighter::FighterId;
use crate::ids::Id;

pub type ProjectileId = Id<Projectile>;
pub type ItemId = Id<Item>;
pub type ActorId = Id<WorldActor>;
pub type MarkerId = Id<lists::Marker>;
pub type ShipId = Id<lists::Ship>;
pub type MortarId = Id<lists::MortarAim>;
pub type AimPunchId = Id<lists::AimPunch>;
pub type AcceleratorId = Id<lists::Accelerator>;

/// A spawned actor's class, by family: what an `ACTORS` name resolves to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ActorClass {
    Projectile(ProjectileClass),
    Item(ItemClass),
    World(WorldActorClass),
}

impl ActorClass {
    pub const fn name(self) -> &'static str {
        match self {
            ActorClass::Projectile(c) => c.name(),
            ActorClass::Item(c) => c.name(),
            ActorClass::World(c) => c.name(),
        }
    }
}

/// What a `HitActor` / `object` reference points at in the match: a fighter, a spawned actor
/// of a family, a stage actor (by its place in the stage's list), a buff actor (a buff entry whose own
/// Comp_Pawn_HitBoxHandler deals a hit: a dealer actor with a buff actor).
#[derive(State, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ActorRef {
    Fighter(FighterId),
    Projectile(ProjectileId),
    Item(ItemId),
    Actor(ActorId),
    Marker(MarkerId),
    Ship(ShipId),
    Mortar(MortarId),
    AimPunch(AimPunchId),
    Accelerator(AcceleratorId),
    Stage(u32),
    Buff(crate::combat::buffs::BuffRef),
}
