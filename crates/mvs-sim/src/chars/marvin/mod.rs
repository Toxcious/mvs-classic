//! Marvin (content marvin, Pawn_C018): his state, the pawn and C018_AnimBP's sim part
//! (pawn, aim), the UFO and its rockets, the bubble, the blasters, the disintegration ray, the flag and its homing
//! mark, the swap, the reversal and the perks. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs's `pawnHooks` (aim.rs's animUpdate among them), then perks.rs's `afterSpawnedActor`, the
//! only other file that joins one. Each file enters its own classes in the character registry's order.

pub mod aim;
pub mod blaster;
pub mod bubble;
pub mod flag;
pub mod homing;
pub mod pawn;
pub mod perks;
pub mod ray;
pub mod reverse;
pub mod state;
pub mod swap;
pub mod ufo;

pub use state::{MarvinState, marvin, marvin_mut, marvin_of, marvin_w};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "marvin";

/// `state: () => new MarvinState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Marvin(MarvinState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance: C018_AnimBP's variables at
/// their defaults).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Marvin(MarvinState::default());
}

/// Marvin's entries in the character registry's order: his hooks (mod.rs's and pawn.rs's, then perks.rs's
/// afterSpawnedActor), then pawn, ufo, bubble, blaster, ray, flag, homing, swap, reverse, perks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        tick: Some(pawn::tick),
        timer_tick: Some(pawn::timer_tick),
        anim_update: Some(aim::anim_update),
        death: Some(pawn::death),
        after_spawned_actor: Some(perks::after_spawned_actor),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    ufo::register(r)?;
    bubble::register(r)?;
    blaster::register(r)?;
    ray::register(r)?;
    flag::register(r)?;
    homing::register(r)?;
    swap::register(r)?;
    reverse::register(r)?;
    perks::register(r)?;
    Ok(())
}
