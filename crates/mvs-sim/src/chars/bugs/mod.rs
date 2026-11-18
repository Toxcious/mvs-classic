//! Bugs Bunny (content bugs, Pawn_BugsBunny, EFighter 12): his state, the pawn (the glide
//! input, EndJump, BPI_Cleanup, BPI_BugsBunny), BugsBunny_AnimBP's glide list under the shapes and in the sim, the
//! perks (Lingering Love, Shockwave), the tunnels and their teleport, the safe, the rockets, the pie, the ACME box (whose
//! items are `crate::items`'), and the charm. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state and BeginPlay); its `hookJoiner` chain is the joined
//! hooks: each hook here has one file (pawn.rs, glide_box.rs, glide.rs, tunnel.rs), which sets it. Each file enters
//! its own classes, in the character registry's import order.

pub mod charm;
pub mod dispenser;
pub mod glide;
pub mod glide_box;
pub mod pawn;
pub mod perks;
pub mod pie;
pub mod rockets;
pub mod safe;
pub mod state;
pub mod tunnel;

pub use state::{BugsState, bugs, bugs_mut, bugs_of, bugs_w};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "bugs";

/// `state: () => new BugsState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Bugs(BugsState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state and its BeginPlay (pawn.rs: OriginalGravityScale,
/// the dispenser's CompSpawnActor).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Bugs(BugsState::default());
    pawn::begin_play(w, f);
}

/// Bugs Bunny's entries in the character registry's order: perks.rs's classes (mod.rs imports them first), his hooks
/// (mod.rs's, pawn.rs's, glide_box.rs's, then the joined files': tunnel, glide), then tunnel, safe, rockets, pie,
/// dispenser, charm.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    perks::register(r)?;
    let mut h = CharHooks { state: Some(new_state), match_reset: Some(match_reset), ..CharHooks::default() };
    pawn::hooks(&mut h);
    glide_box::hooks(&mut h);
    tunnel::hooks(&mut h);
    glide::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    tunnel::register(r)?;
    safe::register(r)?;
    rockets::register(r)?;
    pie::register(r)?;
    dispenser::register(r)?;
    charm::register(r)?;
    Ok(())
}
