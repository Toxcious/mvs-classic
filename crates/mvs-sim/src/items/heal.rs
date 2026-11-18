//! Item_SmallHeal_C and its data children Item_MedHeal_C and Item_LargeHeal_C:
//! an ItemActorParent child; UseOnPickup, HealAmount 10 (the children set their own HealAmount and
//! capsule, and take its entry).
//! ActivateOnPickup @523 -> @43 (authority): ApplyDamage(Instigator, -FMin(HealAmount, the Instigator's
//! Comp_Actor_Damage CurrentDamage), Instigator) @258; SpawnEmitterAttached(PS_Heal) on the Instigator's root component
//! @407; then HealVisualMulticast @10 -> @528 (SpawnEmitterAtLocation(PS_Heal) at the item) and ReleaseToPool @24.
//! ReceiveBeginPlay is the parent's alone. Ported for a spawn with no owner: the class reads its Instigator only in
//! ActivateOnPickup, where the picker is.

use crate::actors::{ActorClass, ItemClass, ItemId};
use crate::combat::buffs::apply_healing;
use crate::events::Event;
use crate::fighter::{FighterId, actor_y};
use crate::math::min;
use crate::name;
use crate::registry::entries::ItemHooks;
use crate::registry::props::num;
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;

/// `pickedUp`: true is the ReleaseToPool.
fn picked_up(w: &mut World, it: ItemId, f: FighterId) -> bool {
    let (cls, seq, x, y, heal) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.cls, i.seq, i.x, i.y, num(i.def(&w.data).p, "HealAmount").unwrap_or(0.0))
    };
    // UGameplayStatics::ApplyDamage does nothing with a damage of 0 (a fighter at 0 damage).
    let h = min(heal, w.fighter(f).stats.damage);
    if h != 0.0 {
        apply_healing(w, f, h);
    }
    // Deviation: the attached PS_Heal (@407) is an event at the picker's place; it does not follow the root.
    let (fid, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    let vfx = name!("PS_Heal");
    w.events.push(Event::ItemFx { cls, item: seq, fighter: fid, vfx, x: fx, y: fy, scale: 1.0 });
    w.events.push(Event::ItemFx { cls, item: seq, fighter: fid, vfx, x, y, scale: 1.0 }); // @556
    true
}

/// This file's registry entries: Item_SmallHeal_C, then the data children.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for class in [ItemClass::ItemSmallHeal, ItemClass::ItemMedHeal, ItemClass::ItemLargeHeal] {
        let hooks = ItemHooks { picked_up: Some(picked_up), ..ItemHooks::default() };
        r.actors.enter(
            class.name(),
            ActorEntry { ownerless: true, ..ActorEntry::new(ActorClass::Item(class), ActorHooks::Item(hooks)) },
        )?;
    }
    Ok(())
}
