//! The port's own Dodgeball rule: a barrier down the middle of the stage keeps each team on its half, a fighter's
//! own attacks spare an opponent, and the items the rule drops on both halves hit when thrown across. Not in the
//! build: no line here ports a Blueprint. The items, their throw and their hits are the build's own.
//!
//! The rule's state is [`DodgeballState`] on `World::dodgeball`, made with the World when the set-up has the rule
//! (`MatchSetup::dodgeball`). Its draws come from a stream of its own, made from the match seed, so they move no
//! other draw of the match and every machine draws the same.

use crate::Real;
use crate::State;
use crate::fighter::FighterId;
use crate::math;
use crate::setup::MatchSetup;
use crate::world::World;

/// The rule's state. Its fields are the sim track's.
#[derive(State, Debug, Default)]
pub struct DodgeballState {
    /// The rule's own random stream (`rng::Rng`'s state).
    pub rng: i32,
}

/// The rule's state for a World of `setup` with `seed`, None without the rule. The stream's seed is the World's own
/// `seed` through constants no other stream of the match uses (`World::rng`: 31337 and 7; a fighter's: 7919 and
/// 104729; the Chaos Wheel's: 69069 and 1013), so its draws move no other.
pub fn new_state(setup: &MatchSetup, seed: i32) -> Option<DodgeballState> {
    setup.dodgeball?;
    let rng = math::dbl::to_int32(f64::from(seed) * 48271.0 + 2053.0);
    Some(DodgeballState { rng })
}

/// What a fighter of `team` deals does not reach `vic`: the rule is on and `vic` is on another team. The one dealer
/// that reaches an opponent under the rule is an item the stage dropped, thrown by a fighter, and the test that
/// picks its victim does not ask.
pub(crate) fn spares_team(w: &World, team: i32, vic: FighterId) -> bool {
    w.setup.dodgeball.is_some() && w.fighter(vic).team.team != team
}

/// [`spares_team`] by the dealing fighter.
pub(crate) fn spares(w: &World, owner: FighterId, vic: FighterId) -> bool {
    w.setup.dodgeball.is_some() && w.fighter(vic).team.team != w.fighter(owner).team.team
}

/// The barrier's box in the play plane as left, right, bottom and top, for the drawing side. None without the rule
/// or its barrier.
pub fn barrier(w: &World) -> Option<[Real; 4]> {
    let _ = w;
    None
}
