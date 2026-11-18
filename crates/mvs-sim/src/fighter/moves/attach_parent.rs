//! GetAttachParentActor of a fighter: the actor its root is attached to.
//! The port's attaches of a fighter's root: an item's carry (Fastball's PickUp, `BuffShapes::carried`), a hold
//! (Buff_HeldByEnemy and its children from InitiateGrapple to the buff's end, `Hold::attached`: the holder), a live
//! buff whose class has the `attach_parent` hook (the Buff_AttachToAlly family: its Instigator).

use crate::actors::ActorRef;
use crate::combat::BuffRef;
use crate::fighter::FighterId;
use crate::world::World;

/// `attachParentOf`: the fighter's attach parent, `None` for none.
// Unverified: an actor has one attach parent, the one of its last attach; a fighter under two of these at once is
// taken in the order above (no ported pair of them overlaps: a carried or held fighter is not given an attach buff by
// the ported classes).
pub fn attach_parent_of(w: &World, f: FighterId) -> Option<ActorRef> {
    let x = w.fighter(f);
    if let Some(c) = &x.shapes.carried {
        return Some(ActorRef::Item(c.item));
    }
    if let Some(h) = &x.holds.hold
        && h.attached
    {
        return Some(ActorRef::Fighter(h.by));
    }
    for b in &x.buffs.buffs {
        if b.frames <= 0.0 {
            continue;
        }
        let hook = crate::combat::buffs::kind_of(w, b.class).and_then(|k| k.hooks.attach_parent);
        if let Some(p) = hook.and_then(|h| h(w, f, BuffRef { fighter: f, serial: b.serial })) {
            return Some(ActorRef::Fighter(p));
        }
    }
    None
}
