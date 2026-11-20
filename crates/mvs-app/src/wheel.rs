//! (the port's own) The Chaos Wheel rule on the app's side (`mvs_sim::world::wheel` is the rule): which matches the
//! command line's `--wheel` gives it to, and the HUD's card of the rule's status.

use mvs_protocol::lobby_rules::{LOBBY_MUTATORS, WHEEL_MUTATORS};
use mvs_sim::setup::{MatchSetup, Wheel};
use mvs_sim::world::World;
use mvs_sim::world::wheel::WheelStatus;
use mvs_ui::hud::WheelHud;

use crate::app::MatchKind;

const TICKS_PER_SECOND: f64 = 60.0;

/// `--wheel`: the matches the switch gives the rule to: the ones played on this machine alone with the build's own
/// rules (the command line's match, a Lab, Local Play). A replay runs its trace's set-up, an online match the
/// host's, and a tutorial, an arcade round and a Waves run are scripted.
pub fn switched(kind: &MatchKind) -> bool {
    matches!(kind, MatchKind::Default | MatchKind::Lab { .. } | MatchKind::LocalPlay)
}

/// A custom game's rule over the whole pool.
pub fn whole_pool() -> Wheel {
    Wheel::of(WHEEL_MUTATORS.iter().map(|&k| k.to_owned()).collect())
}

/// The HUD's card of the rule's status: the pool's mutators by their place in the menus' names.
pub fn hud_of(setup: &MatchSetup, s: &WheelStatus) -> WheelHud {
    let named = |i: Option<usize>| {
        let key = setup.wheel.as_ref()?.pool.get(i?)?;
        LOBBY_MUTATORS.iter().position(|k| k == key)
    };
    WheelHud {
        current: named(s.current),
        next: named(s.next),
        seconds: s.ticks_left.map(|t| f64::from(t) / TICKS_PER_SECOND),
    }
}

/// The card of `world`'s rule, or None in a match with no wheel.
pub fn hud(world: &World) -> Option<WheelHud> {
    mvs_sim::world::wheel::status(world).map(|s| hud_of(&world.setup, &s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_names_the_pool_by_the_menus_order() {
        let setup = MatchSetup { wheel: Some(whole_pool()), ..MatchSetup::default() };
        let s = WheelStatus { current: Some(0), next: Some(12), ticks_left: Some(90), window_ticks: 1200 };
        let h = hud_of(&setup, &s);
        assert_eq!(h.current.map(|i| LOBBY_MUTATORS[i]), Some("Mutator_Giant"));
        assert_eq!(h.next.map(|i| LOBBY_MUTATORS[i]), Some("Mutator_RandomItemSpawns"));
        assert_eq!(h.seconds, Some(1.5));
        let plain = WheelStatus { current: None, next: None, ticks_left: None, window_ticks: 1200 };
        assert_eq!(hud_of(&setup, &plain), WheelHud::default());
    }

    #[test]
    fn every_pool_mutator_has_a_menu_name() {
        for k in WHEEL_MUTATORS {
            assert!(LOBBY_MUTATORS.contains(&k), "{k}");
        }
    }

    #[test]
    fn the_switch_leaves_scripted_and_given_matches_alone() {
        assert!(switched(&MatchKind::Default) && switched(&MatchKind::LocalPlay));
        assert!(switched(&MatchKind::Lab { carry: None }));
        assert!(!switched(&MatchKind::Tutorial) && !switched(&MatchKind::Arcade));
        assert!(!switched(&MatchKind::Replay { name: String::new() }));
    }
}
