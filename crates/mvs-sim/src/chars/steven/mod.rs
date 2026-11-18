//! Steven Universe (content steven, Pawn_StevenUniverse): his state, the pawn (the glide
//! input, the up shield ammo), Steven_AnimBP's glide under the shapes, the shield charges and the ally's bubble shield,
//! the tether and the enemy's bubble, the barrier and the up special's platform, the heal and the Watermelon minion,
//! and the bubble charge. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs, glide.rs, shield.rs and watermelon.rs set or join theirs (one track's files, so the port
//! composes a hook two of them hold in this file). Each file enters its own classes.

pub mod barrier;
pub mod bots;
pub mod bubble;
pub mod glide;
pub mod pawn;
pub mod shield;
pub mod state;
pub mod tether;
pub mod watermelon;

pub use state::{StevenState, steven, steven_mut, steven_of, steven_w};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "steven";

/// `state: () => new StevenState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Steven(StevenState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (pawn.rs's BeginPlay and the joined files' resets
/// run after it and find it).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Steven(StevenState::default());
    pawn::begin_play(w, f);
}

/// `reset` (BPI_Cleanup): pawn.rs's, then shield.rs's (OnFighterReset after isGliding @8654), then watermelon.rs's
/// (@8644), the joinHooks order.
fn reset(w: &mut World, f: FighterId) {
    pawn::cleanup(w, f);
    shield::reset(w, f);
    watermelon::reset(w, f);
}

/// Steven's entries: his hooks (mod.rs's, then pawn.rs's and glide.rs's, then the joined files' in the registry's
/// order: shield, watermelon), then each file's classes in that order (shield, tether, barrier, watermelon, bubble).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        reset: Some(reset),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    glide::hooks(&mut h);
    shield::hooks(&mut h);
    watermelon::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    shield::register(r)?;
    tether::register(r)?;
    barrier::register(r)?;
    watermelon::register(r)?;
    bubble::register(r)?;
    Ok(())
}
