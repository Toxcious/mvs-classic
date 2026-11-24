//! (the port's own) The Dodgeball rule on the app's side (`mvs_sim::world::dodgeball` is the rule): which matches
//! the command line's `--dodgeball` gives it to, and the layer its barrier is drawn on.

use mvs_sim::setup::{MatchSetup, TeamStyle};

use crate::app::MatchKind;

/// The barrier's layer in the frame's draw list: under the HUD's widgets.
pub const BARRIER_LAYER: i32 = -1;

/// `--dodgeball`: the matches the switch gives the rule to: the ones `--wheel` takes (`crate::wheel::switched`),
/// in 1v1 and 2v2. The rule has no Free For All.
pub fn switched(kind: &MatchKind, setup: &MatchSetup) -> bool {
    crate::wheel::switched(kind) && setup.team_style != TeamStyle::FreeForAll
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_takes_teams_only() {
        let teams = MatchSetup { team_style: TeamStyle::TwoVsTwo, ..MatchSetup::default() };
        let ffa = MatchSetup { team_style: TeamStyle::FreeForAll, ..MatchSetup::default() };
        assert!(switched(&MatchKind::LocalPlay, &teams));
        assert!(!switched(&MatchKind::LocalPlay, &ffa));
        assert!(!switched(&MatchKind::Arcade, &teams));
    }
}
