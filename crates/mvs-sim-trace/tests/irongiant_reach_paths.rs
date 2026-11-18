//! A fighter's Comp_ItemManager list (ReachableItems) that still names a destroyed item: Iron Giant's BlockPickUp
//! keeps TrackReachableItems from taking the item out at its destroy (as the oracle's list keeps the object), so the
//! item manager's tick sets the icon of the gone item's kept copy (`Actors::gone_items`) instead of a stale id.

use mvs_sim::actors::ActorRef;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::items::{destroy_item, spawn_and_equip_item};
use mvs_sim_trace::harness;

fn player(c: &str) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), ..MatchPlayer::default() }
}

#[test]
fn a_destroyed_item_a_blocked_list_names_is_kept() {
    let setup = MatchSetup { players: vec![player("irongiant"), player("shaggy")], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let (g, s) = (h.fighter(0), h.fighter(1));
    for _ in 0..5 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let cls = h.w.lit("Item_C017_Scrap_C");
    let it = spawn_and_equip_item(&mut h.w, s, cls).expect("a scrap spawns");
    h.w.actors.reach[0].items.push(ActorRef::Item(it));
    h.w.fighter_mut(g).items.block_pick_up = true;
    destroy_item(&mut h.w, it);
    for _ in 0..3 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    assert!(h.w.actors.items.get(it).is_none(), "the item left the arena");
    assert!(h.w.actors.reach[0].items.contains(&ActorRef::Item(it)), "the blocked list keeps it");
    let kept = h.w.actors.gone_items.iter().find(|(k, _)| *k == it).expect("its copy is kept");
    assert!(kept.1.input_ui, "the tick set its icon");
    let _ = format!("{:?}", h.record());
}
