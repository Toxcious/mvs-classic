//! Item_ArmorToken_C: an ItemActorParent child; UseOnPickup.
//! ActivateOnPickup @10: the Instigator's Comp_State_Manager.SpawnBuffActor(Buff_ArmorPack_C, Instigator, Instigator,
//! 0 stacks, 10 s, BreakIfBlocked true) @81, then K2_DestroyActor @165. The rest of its graph is the Spin timeline,
//! which turns the Item_Shield mesh alone (the view's). Ported for a spawn with no owner: the class reads its
//! Instigator only in ActivateOnPickup, where the picker is.

use crate::actors::{ActorClass, ItemClass, ItemId};
use crate::combat::buffs::spawn_buff;
use crate::fighter::FighterId;
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;

/// `pickedUp`: true destroys the item.
fn picked_up(w: &mut World, _it: ItemId, f: FighterId) -> bool {
    spawn_buff(w, f, name!("Buff_ArmorPack_C"), 10.0, Some(0), Some(f), true, Some(f));
    true
}

/// This file's registry entries (Item_ArmorToken_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks { picked_up: Some(picked_up), ..ItemHooks::default() };
    r.actors.enter(
        ItemClass::ItemArmorToken.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemArmorToken), ActorHooks::Item(hooks))
        },
    )
}
