//! (the port's own) The Chaos Wheel rule on the app's side (`mvs_sim::world::wheel` is the rule): which matches the
//! command line's `--wheel` gives it to, and the HUD's card of the rule's status.

use mvs_sim::setup::{MatchSetup, Wheel};
use mvs_sim::world::World;
use mvs_sim::world::wheel::WheelStatus;
use mvs_ui::hud::WheelHud;

use crate::app::MatchKind;

const TICKS_PER_SECOND: f64 = 60.0;

/// `--wheel`: the matches the switch gives the rule to: the ones played on this machine alone with the build's own
/// rules (the command line's match, a Lab, Local Play), and the command line's relay match (`--relay`), where both
/// players pass the switch. A replay runs its trace's set-up, the menus' online match the host's, and a tutorial, an
/// arcade round and a Waves run are scripted.
pub fn switched(kind: &MatchKind) -> bool {
    match kind {
        MatchKind::Default | MatchKind::Lab { .. } | MatchKind::LocalPlay => true,
        MatchKind::Online { net: Some(net) } => matches!(**net, crate::online::Online::Dev(_)),
        _ => false,
    }
}

/// A custom game's rule over the whole pool.
pub fn whole_pool() -> Wheel {
    Wheel::of(mvs_mutators::keys().map(str::to_owned).collect())
}

/// `--wheel`'s rule: the whole pool or `--wheel-pool`'s, with `--wheel-seconds`' window when it names one (the
/// warning a third of a window shorter than the rule's own).
pub fn switch_rule(seconds: Option<f64>, pool: Option<Vec<String>>) -> Wheel {
    let mut w = pool.map_or_else(whole_pool, Wheel::of);
    if let Some(s) = seconds {
        w.seconds = mvs_sim::math::narrow(s);
        w.warn = w.warn.min(mvs_sim::math::narrow(s / 3.0));
    }
    w
}

/// A pool entry by its place in the wheel's list (`mvs_mutators::ALL`).
fn lobby_index(key: &str) -> Option<usize> {
    mvs_mutators::find(key)
}

/// The HUD's card of the rule's status: the pool's mutators by their place in the wheel's list.
pub fn hud_of(setup: &MatchSetup, s: &WheelStatus) -> WheelHud {
    let named = |i: Option<usize>| lobby_index(setup.wheel.as_ref()?.pool.get(i?)?);
    // (the rule's draw ahead of a change, as `world::wheel` counts it: whole ticks, at least 1, at most a window)
    let warn = setup.wheel.as_ref().map_or(0.0, |w| f64::from(w.warn));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a clamped, rounded count")]
    let warn_ticks = ((warn * TICKS_PER_SECOND).round().max(1.0) as u32).min(s.window_ticks);
    WheelHud {
        current: named(s.current),
        next: named(s.next),
        also: named(s.also),
        also_next: named(s.also_next),
        ticks_left: s.ticks_left,
        window_ticks: s.window_ticks,
        warn_ticks,
    }
}

/// The HUD's pool of a set-up's wheel, by the wheel's list; None in a match with no wheel.
pub fn hud_pool(setup: &MatchSetup) -> Option<Vec<usize>> {
    Some(setup.wheel.as_ref()?.pool.iter().filter_map(|k| lobby_index(k)).collect())
}

/// The card of `world`'s rule, or None in a match with no wheel.
pub fn hud(world: &World) -> Option<WheelHud> {
    mvs_sim::world::wheel::status(world).map(|s| hud_of(&world.setup, &s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_names_the_pool_by_the_wheels_order() {
        let setup = MatchSetup { wheel: Some(whole_pool()), ..MatchSetup::default() };
        let s = WheelStatus {
            current: Some(0),
            next: Some(12),
            also: Some(1),
            also_next: None,
            ticks_left: Some(90),
            window_ticks: 1200,
        };
        let h = hud_of(&setup, &s);
        assert_eq!(h.current.map(|i| mvs_mutators::ALL[i].key), Some("Mutator_Giant"));
        assert_eq!(h.next.map(|i| mvs_mutators::ALL[i].key), Some("Mutator_RandomItemSpawns"));
        assert_eq!((h.also, h.also_next), (Some(1), None));
        assert_eq!((h.ticks_left, h.window_ticks, h.warn_ticks), (Some(90), 1200, 180));
        assert_eq!(hud_pool(&setup).map(|p| p.len()), Some(mvs_mutators::ALL.len()));
        assert_eq!(hud_pool(&MatchSetup::default()), None);
        // a window shorter than the warning: the draw comes at the window's start
        let short = MatchSetup { wheel: Some(switch_rule(Some(0.5), None)), ..MatchSetup::default() };
        let s = WheelStatus {
            current: None,
            next: None,
            also: None,
            also_next: None,
            ticks_left: Some(30),
            window_ticks: 30,
        };
        assert_eq!(hud_of(&short, &s).warn_ticks, 10);
        let plain = WheelStatus {
            current: None,
            next: None,
            also: None,
            also_next: None,
            ticks_left: None,
            window_ticks: 1200,
        };
        assert_eq!(hud_of(&setup, &plain), WheelHud { window_ticks: 1200, warn_ticks: 180, ..WheelHud::default() });
    }

    #[test]
    fn every_pool_mutator_has_a_name() {
        for k in mvs_mutators::keys() {
            assert!(!mvs_ui::menu::custom_text::wheel_name(k).is_empty(), "{k}");
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
