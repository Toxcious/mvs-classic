//! The port's own Chaos Wheel rule: the match plays one window plain, then at each window's
//! start one mutator of the set-up's pool comes on for every fighter and the one before it ends. Not in the build: no
//! line here ports a Blueprint. Every effect is one of the build's mutators, spawned and ended as a buff by the
//! build's own code (`world::mutators`, `combat::buffs`); only when one starts and ends is the rule's.
//!
//! The rule's state is [`WheelState`] on `World::wheel`, made with the World when the set-up has a wheel
//! (`MatchSetup::wheel`). Its draws come from a stream of its own, made from the match seed, so they move no other
//! draw of the match and every machine rolls the same.

use crate::State;
use crate::world::World;

/// The rule's state. Its fields are the sim track's.
#[derive(State, Debug, Default)]
pub struct WheelState {}

/// What the app and the HUD read of the rule each frame (`status`). The indexes are into the set-up's pool
/// (`MatchSetup::wheel`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelStatus {
    /// The mutator that is on, or None in the plain first window.
    pub current: Option<usize>,
    /// The next mutator, from the tick it is drawn (the set-up's `warn` ahead of the change) to the change.
    pub next: Option<usize>,
    /// Ticks to the next change; None before the match starts and once it is over.
    pub ticks_left: Option<u32>,
    /// A window's length in ticks.
    pub window_ticks: u32,
}

/// The rule as the app and the HUD read it, or None in a World with no wheel.
pub fn status(w: &World) -> Option<WheelStatus> {
    w.wheel.as_ref()?;
    crate::unported!("wheel::status", w)
}
