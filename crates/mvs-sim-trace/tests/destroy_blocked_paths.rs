//! BPI_SetDestroyEffectsBlocked on an Item (`combat::elem::set_destroy_blocked`, `world::items::destroy_item`): a
//! blocked item's EndPlay plays no DeathVFX and no 'Ak Death Location'; an unblocked one does. On ffa-items-b's World,
//! where items lie on the stage (no match blocks an item's effects).

use mvs_sim::actors::{ActorRef, ItemId};
use mvs_sim::combat::elem::set_destroy_blocked;
use mvs_sim::events::Event;
use mvs_sim::world::items::destroy_item;
use mvs_sim_trace::harness;

/// Where the search for two live items starts; the World steps on with the recorded inputs until it has them.
const FROM: usize = 600;

fn deaths(h: &harness::Harness) -> usize {
    h.w.events.iter().filter(|e| matches!(e, Event::ProjectileDeath { .. })).count()
}

#[test]
fn a_blocked_item_dies_without_its_death_effects() {
    let Some(mut h) = harness::at("ffa-items-b", FROM) else { return };
    let live = |h: &harness::Harness| -> Vec<ItemId> {
        h.w.actors.items.iter().filter(|(_, i)| !i.dead).map(|(id, _)| id).collect()
    };
    let mut ids = live(&h);
    for _ in 0..6000 {
        if ids.len() >= 2 {
            break;
        }
        h.step_recorded();
        ids = live(&h);
    }
    assert!(ids.len() >= 2, "two live items from frame {FROM} on, found {}", ids.len());
    h.w.events.clear();
    destroy_item(&mut h.w, ids[0]);
    assert_eq!(deaths(&h), 1, "an unblocked item plays its DeathVFX");
    h.w.events.clear();
    set_destroy_blocked(&mut h.w, ActorRef::Item(ids[1]), true, true);
    set_destroy_blocked(&mut h.w, ActorRef::Item(ids[1]), false, true);
    let it = h.w.actors.items.get(ids[1]).expect("the item");
    assert!(it.destroy_fx_blocked && it.destroy_spawn_blocked, "the item keeps both flags");
    destroy_item(&mut h.w, ids[1]);
    assert_eq!(deaths(&h), 0, "a blocked item plays no DeathVFX");
    assert!(!h.w.events.iter().any(|e| matches!(e, Event::Sound { .. })), "and no 'Ak Death Location'");
}
