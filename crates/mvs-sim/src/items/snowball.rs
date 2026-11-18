//! Item_Snowball_C and its child Item_SuperSnowball_C: ItemActorParent children of
//! the random items pool. Durability 1, CanEnemyDestroy, HitBoxDamage 3 / 500 / 5 with DirectionalKnockback and
//! VelocityInsteadOfFacing, ThrowHitBox 62; On Hit Buffs Buff_SnowballHead_C x1 (the super: x4, then Buff_Ice_C x15 for
//! 5 s), dealt by the thrown hit from the data to a fighter, and to a spawned HitActor of another team that survives
//! the hit as well (pfgDoDamage @19011 has no fighter cast: `combat::projectile_buffs`; the head there is
//! `snowball_head`'s actor entry).
//! - HitEnemyPlayer @143, the class's own with no parent call: with authority the Instigator's IncQuestIntValue of
//!   Quest_Event_HitWithSnowballs0-4 (no match state). So the enemy hit spends nothing: no Durability - 1, no hit
//!   box off, no ItemThrowHit, no bounce (`actors::item::item_spend`). The ball ends by SpawnedActorParent's
//!   OnHitAnything all the same (DestroyedOnHit true in its data: Delay(0.033) -> DestroySpawnedActor).
//!   Item_SuperSnowball's HitEnemyPlayer @10 calls its parent's (the same).
//! - ReceiveBeginPlay @1413: SnowballMesh's Team 1.0; SetTeamColor @1100 (the owner's BPI_GetColorValue): the view's.
//! - ReceiveDestroyed @10: sfx_shared_item_snowball_impact at the Capsule.
//!
//! The blocking hit, ReceiveAnyDamage and HitNonPlayer are the parent's (Durability 1: the ball is gone at its
//! first). ReceiveDestroyed is every end of the item (`Item::end`): CanBePooled is false with no writer, so
//! ReleaseToPool destroys the actor. Both spawn with no owner from the world's random items: nothing above reads it
//! but the sound's poster.

use crate::State;
use crate::actors::item::ItemExt;
use crate::actors::{ActorClass, ItemClass, ItemId};
use crate::events::Event;
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;

/// `Snowball extends Item` (`spend` is the class's override: `item_spend`).
#[derive(State, Debug, Default)]
pub struct Snowball {}

fn sub() -> ItemExt {
    ItemExt::Snowball(Snowball::default())
}

/// `end`: the sound at the Capsule, posted for the owner (none: -1).
fn end(w: &mut World, it: ItemId) {
    if reset_pass(w) {
        return; // the Lab reset's end pass
    }
    let i = w.actors.items.get(it).expect("a live item");
    let (x, y, owner) = (i.x, i.y, i.owner);
    let fighter = owner.map_or(-1, |f| w.fighter(f).id);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_shared_item_snowball_impact"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// This file's registry entries (Item_Snowball_C, Item_SuperSnowball_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for class in [ItemClass::ItemSnowball, ItemClass::ItemSuperSnowball] {
        let hooks = ItemHooks { sub: Some(sub), end: Some(end), ..ItemHooks::default() };
        r.actors.enter(
            class.name(),
            ActorEntry { ownerless: true, ..ActorEntry::new(ActorClass::Item(class), ActorHooks::Item(hooks)) },
        )?;
    }
    Ok(())
}
