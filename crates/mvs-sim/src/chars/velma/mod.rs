//! Velma (content velma, Pawn_Velma): her state, the pawn and Velma_AnimBP's sim part, the
//! evidence, the police (her side and the `velmapolice` minion, which police.rs enters as a character of its own), the
//! heal beam, the mark, the carry run and the capture it starts, the gadgets, and the perks. Her attacks themselves are
//! data played by the shared code; her bot tasks are the bot port's.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs's `pawnHooks`, then the joined files' in the character registry's order (evidence, police,
//! beam, mark, carry, capture). Each file enters its own classes in that order, perks last.

pub mod beam;
pub mod bots;
pub mod capture;
pub mod carry;
pub mod evidence;
pub mod gadgets;
pub mod mark;
pub mod pawn;
pub mod perks;
pub mod police;
pub mod state;

pub use state::{
    VelmaPoliceState, VelmaState, police, police_mut, police_of, police_w, velma, velma_mut, velma_of, velma_w,
};

use crate::Real;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "velma";

/// `state: () => new VelmaState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Velma(VelmaState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance: Velma_AnimBP's variables at
/// their defaults); then pawn.rs's (its BeginPlay).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Velma(VelmaState::default());
    pawn::match_reset(w, f);
}

/// `timerTick`: evidence.rs's (the SpawnEvidence timer), then police.rs's (Delay 1 and 8), then beam.rs's (the
/// beams' timers), the joinHooks order.
fn timer_tick(w: &mut World, f: FighterId) {
    evidence::timer_tick(w, f);
    police::timer_tick(w, f);
    beam::timer_tick(w, f);
}

/// `worldTick`: capture.rs's (the held fighters follow her). mark.rs has none: the World is passed to its hooks.
fn world_tick(w: &mut World, f: FighterId) {
    capture::world_tick(w, f);
}

/// `knockedBack`: capture.rs's (a held ally lets go).
fn knocked_back(w: &mut World, f: FighterId, x: Real, y: Real, z: Real, by: Option<ActorRef>) {
    capture::knocked_back(w, f, x, y, z, by);
}

/// Velma's entries in the character registry's order: her hooks (mod.rs's and pawn.rs's, then the joined files'),
/// then pawn, evidence, police (with the `velmapolice` character), beam, mark, carry, capture, gadgets, perks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        timer_tick: Some(timer_tick),
        world_tick: Some(world_tick),
        knocked_back: Some(knocked_back),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    evidence::register(r)?;
    police::register(r)?;
    beam::register(r)?;
    mark::register(r)?;
    carry::register(r)?;
    capture::register(r)?;
    gadgets::register(r)?;
    perks::register(r)
}
