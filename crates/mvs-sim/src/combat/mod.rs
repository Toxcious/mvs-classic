//! Combat: the hits, the hit boxes and hurt boxes, knockback, hitstun, the buffs, the conditions, rage and combo
//! decay (the hit routes, the overlap tests, the spawned actors' hits, pfgDoDamage and
//! the hit and buff parts of the fighter). The fighter's combat state is in the groups below, held by
//! [`crate::Fighter`]; [`phases`] holds the combat calls of the World's step.

pub mod attach_ally;
pub mod boxes;
pub mod buff_class;
pub mod buffs;
pub mod conditions;
pub mod decay;
pub mod defense;
pub mod disguise;
pub mod dynamic_move;
pub mod electric;
pub mod elem;
pub mod flatten;
pub mod gauntlet;
pub mod grow_with_damage;
pub mod hit;
pub mod hits;
pub mod holds;
pub mod move_to_actor_2;
pub mod mutator_buffs;
pub mod packet;
pub mod perks;
pub mod phases;
pub mod polymorph;
pub mod pop_text;
pub mod reflect;
pub mod reverse;
pub mod shapes;
pub mod transfer_debuffs;

pub use boxes::BoxSampler;
pub use buff_class::BuffClass;
pub use buffs::{BuffDecay, BuffExt, BuffRef, Buffs, FighterBuff};
pub use conditions::{CondClass, CondState};
pub use decay::{Attackers, ComboDecay};
pub use defense::{Armor, Invuln, Rage};
pub use elem::{ChargeVals, ElemState};
pub use hit::{ActiveHit, HitReact, HitState};
pub use holds::Holds;
pub use packet::{DataEnum, DataVec, HitPacket, OnHitBuffRow, VacuumInfo};
pub use shapes::BuffShapes;

use crate::registry::{Registry, RegistryError};

/// The shared modules' entries (the registry's own tables and what the combat, actor and item modules enter),
/// one call per module.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    conditions::register(r)?;
    // Registration order: the elemental buffs ahead of the shared buffs' own entries.
    elem::register(r)?;
    crate::actors::smoke_bomb::register(r)?;
    crate::actors::firewall::register(r)?;
    crate::actors::barrier::register(r)?;
    // Registration order: the dynamic move, the move to an actor and the transfer of debuffs
    // ahead of its own entries.
    dynamic_move::register(r)?;
    move_to_actor_2::register(r)?;
    transfer_debuffs::register(r)?;
    buffs::register(r)?;
    electric::register(r)?;
    flatten::register(r)?;
    gauntlet::register(r)?;
    disguise::register(r)?;
    polymorph::register(r)?;
    // Registration order: the fighter mutators and the grow-with-damage mutator behind the arcade buffs.
    mutator_buffs::register(r)?;
    grow_with_damage::register(r)?;
    // AnimNotify_HoldChargeSet
    hits::register(r)?;
    perks::register(r)?;
    // SpawnedPlatformParent_C
    crate::world::spawned_platform::register(r)?;
    Ok(())
}

pub mod melee_actors;
pub mod spawned_hits;
