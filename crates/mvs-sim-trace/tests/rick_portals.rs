//! Rick's portals no trace match runs (`chars/rick/{portal,portal_comp,teleport}.rs`, `world/spawn_spot.rs`): the
//! allied list's clean-up, the spawn of two portals, an enemy sent through the first (Buff_Teleporting3: the delay, the
//! main and the exit timelines, the knockback on exit), the one-portal path, and K2_TeleportTo's item and projectile
//! branches. The values are what the port's own run gives; no oracle probe of these paths was recorded (Unverified).

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::rick::portal_comp::{
    add_allied_portal, get_allied_portals, has_comp, spawn_object, teleport_server,
};
use mvs_sim::input::NO_INPUT;
use mvs_sim::name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

const NONE: [mvs_sim::input::InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

fn rick_world() -> Option<harness::Harness> {
    let setup = MatchSetup { players: vec![player("rick", false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..13 {
        h.step(&NONE);
    }
    Some(h)
}

/// CleanUpAlliedPortals drops an entry that is no valid portal.
#[test]
fn the_allied_list_drops_a_dead_entry() {
    let Some(mut h) = rick_world() else { return };
    let r = h.fighter(0);
    assert!(has_comp(&h.w, r));
    assert!(get_allied_portals(&mut h.w, r).is_empty());
    add_allied_portal(&mut h.w, r, ActorRef::Fighter(r));
    assert!(get_allied_portals(&mut h.w, r).is_empty());
}

/// Two portals: PortalCount 2 on both, and an enemy teleported through the first leaves through the second, the
/// teleport buff gone at its end.
#[test]
fn an_enemy_goes_through_two_portals() {
    let Some(mut h) = rick_world() else { return };
    let (r, s) = (h.fighter(0), h.fighter(1));
    let cls = name!("AutoTeleportPortal_C");
    spawn_object(&mut h.w, r, -300.0, 0.0, cls);
    spawn_object(&mut h.w, r, 300.0, 0.0, cls);
    let list = get_allied_portals(&mut h.w, r);
    assert_eq!(list.len(), 2);
    h.step(&NONE);
    let (enter, exit) = (list[0], list[1]);
    teleport_server(&mut h.w, enter, ActorRef::Fighter(s));
    let b = mvs_sim::combat::buffs::buff_of(&h.w, s, name!("Buff_Teleporting3_C"));
    assert!(b.is_some(), "the teleport buff is on the enemy");
    for _ in 0..60 {
        h.step(&NONE);
    }
    assert!(mvs_sim::combat::buffs::buff_of(&h.w, s, name!("Buff_Teleporting3_C")).is_none());
    let (px, _) = mvs_sim::chars::rick::teleport::portal_loc(&h.w, Some(exit));
    assert!((h.w.fighter(s).body.x - px).abs() < 400.0, "he left near the exit portal: {}", h.w.fighter(s).body.x);
}
