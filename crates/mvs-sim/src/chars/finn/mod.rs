//! Finn (content finn, Pawn_Finn, EFighter 17): his state, the momentum component, the
//! coins and the wallet, the side special's aimer, the pawn and Finn_AnimBP's slot mix, the perks, the gem and the
//! apple, the shop, BMO, the high five and kick, the down attack's shockwaves. His attacks themselves are data played
//! by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs's `pawnHooks`, then the joined files' in the character registry's order (high_five, shop). A
//! hook two files hold is composed in this file. Each file enters its own classes in that order.

pub mod aimer;
pub mod bmo;
pub mod bots;
pub mod coins;
pub mod gem;
pub mod high_five;
pub mod momentum;
pub mod pawn;
pub mod perks;
pub mod shockwave;
pub mod shop;
pub mod state;

pub use state::{FinnState, finn, finn_mut, finn_of, finn_w};

use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::combat::HitPacket;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "finn";

/// `state: () => new FinnState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Finn(FinnState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state; pawn.rs's matchReset (its BeginPlay) runs after it
/// and finds it.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Finn(FinnState::default());
    pawn::match_reset(w, f);
}

/// `hitAnything`: pawn.rs's (the coins), then high_five.rs's, the joinHooks order.
fn hit_anything(w: &mut World, f: FighterId, a: ActorRef, hit: &HitPacket, boxed: bool) {
    pawn::hit_anything(w, f, a, hit, boxed);
    high_five::hit_anything(w, f, a, hit, boxed);
}

/// Finn's entries in the character registry's order: momentum, coins, aimer (pawn.rs imports them first), his hooks
/// (mod.rs's and pawn.rs's, then the joined files': high_five, shop), then perks, gem, shop, bmo, high five,
/// shockwave.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    momentum::register(r)?;
    coins::register(r)?;
    aimer::register(r)?;
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        hit_anything: Some(hit_anything),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    high_five::hooks(&mut h);
    shop::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    gem::register(r)?;
    shop::register(r)?;
    bmo::register(r)?;
    high_five::register(r)?;
    shockwave::register(r)?;
    Ok(())
}
