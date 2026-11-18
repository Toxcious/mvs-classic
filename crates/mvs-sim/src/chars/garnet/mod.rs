//! Garnet (content garnet, Pawn_Garnet): her state, the pawn (the idle and the flying
//! gauntlets, the aim, the ground probes), the side special's gauntlets and the marker, the dash to the marker, the up
//! special's bolt and electric zone, ground down's shockwave, the grab's hold (Buff_HeldByEnemyGarnet), the song
//! (Buff_GarnetStackingSinging) and the stop of a projectile inside it (Buff_GarnetStopProjectile), and the perk
//! Electric Groove. Her attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); pawn.rs sets its hooks (aim.rs's slot inputs among
//! them), and the joined timerTick is composed here: zone.rs's part, then sing.rs's (the character registry's order).
//! Each file enters its own classes.

pub mod aim;
pub mod bots;
pub mod dash;
pub mod gauntlets;
pub mod groove;
pub mod hold;
pub mod marker;
pub mod pawn;
pub mod shockwave;
pub mod sing;
pub mod state;
pub mod stop_projectile;
pub mod zone;

pub use state::{GarnetState, garnet, garnet_mut, garnet_of, garnet_w};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "garnet";

/// `state: () => new GarnetState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Garnet(GarnetState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (pawn.rs's BeginPlay runs after it and finds it).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Garnet(GarnetState::default());
}

/// The joined timerTick: zone.rs's part, then sing.rs's.
fn timer_tick(w: &mut World, f: FighterId) {
    zone::timer_tick(w, f);
    sing::timer_tick(w, f);
}

/// Garnet's entries: her hooks (mod.rs's, pawn.rs's, the joined timerTick), then each file's classes in
/// the character registry's order (pawn with shockwave, gauntlets, zone, marker, dash, hold, sing, stop projectile,
/// groove).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        timer_tick: Some(timer_tick),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    shockwave::register(r)?;
    pawn::register(r)?;
    gauntlets::register(r)?;
    zone::register(r)?;
    marker::register(r)?;
    dash::register(r)?;
    hold::register(r)?;
    sing::register(r)?;
    stop_projectile::register(r)?;
    groove::register(r)?;
    Ok(())
}
