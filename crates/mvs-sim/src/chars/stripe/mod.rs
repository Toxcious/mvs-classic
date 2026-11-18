//! Stripe (content stripe, Pawn_C023B): his state, the pawn and C023B_AnimBP's sim part,
//! the gun and its target tracker, the saw, his MoveCommand, the bomb, the dash grab, skateboard, chainsaw and
//! boombox, and the perks. His attacks themselves are data played by the shared code. The notifies he shares with
//! Gizmo are `chars::shared::gizmo_stripe_notifies`, Buff_TransferDebuffs `combat::transfer_debuffs`.
//!
//! `mod.rs` is this file (the state, matchReset's new state, readsRecentHitActor); its `enter` / `joinHooks` chain
//! is the joined hooks composed here: pawn.rs's `pawnHooks`, then the joined files' in the character registry's order
//! (gun, saw, bomb, dash). A hook two files hold is composed in this file. Each file enters its own classes in that
//! order.

pub mod bomb;
pub mod dash;
pub mod gun;
pub mod move_command;
pub mod pawn;
pub mod perks;
pub mod saw;
pub mod state;

pub use state::{StripeState, stripe, stripe_mut, stripe_of, stripe_w};

use crate::Real;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "stripe";

/// `state: () => new StripeState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Stripe(StripeState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance).
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Stripe(StripeState::default());
}

/// `timerTick`: gun.rs's (the target timer), then saw.rs's, then bomb.rs's (the bomb timer), the joinHooks order.
fn timer_tick(w: &mut World, f: FighterId) {
    gun::timer_tick(w, f);
    saw::timer_tick(w, f);
    bomb::timer_tick(w, f);
}

/// `hitEnemy`: gun.rs's, then bomb.rs's, then dash.rs's.
fn hit_enemy(w: &mut World, f: FighterId, victim: FighterId) {
    gun::hit_enemy(w, f, victim);
    bomb::hit_enemy(w, f, victim);
    dash::hit_enemy(w, f, victim);
}

/// `animUpdate`: pawn.rs's, then gun.rs's (the aim).
fn anim_update(w: &mut World, f: FighterId, dt: Real, first: bool) {
    pawn::anim_update(w, f, dt, first);
    gun::anim_update(w, f, dt, first);
}

/// `death`: pawn.rs's, then bomb.rs's (ResetOnDeath).
fn death(w: &mut World, f: FighterId) {
    pawn::death(w, f);
    bomb::death(w, f);
}

/// Stripe's entries in the character registry's order: his hooks (mod.rs's and pawn.rs's, then the joined files':
/// gun, bomb), then gun, saw, move command, bomb, dash, perks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        reads_recent_hit_actor: true,
        timer_tick: Some(timer_tick),
        hit_enemy: Some(hit_enemy),
        anim_update: Some(anim_update),
        death: Some(death),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    gun::hooks(&mut h);
    bomb::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    gun::register(r)?;
    saw::register(r)?;
    move_command::register(r)?;
    bomb::register(r)?;
    dash::register(r)?;
    perks::register(r)?;
    Ok(())
}
