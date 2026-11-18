//! Gizmo (content gizmo, Pawn_C023A): his state, the pawn and C023A_AnimBP's sim part,
//! the bow, the ally attach, the car, the music and piano, the popcorn and hide box, the umbrella and the perks. His
//! attacks themselves are data played by the shared code. The notifies he shares with Stripe are
//! `chars::shared::gizmo_stripe_notifies`, Buff_MoveToActor2 `combat::move_to_actor_2`'s; the item cars that share
//! his car's graph are `crate::items::car`.
//!
//! `mod.rs` is this file (the state, matchReset's new state, readsRecentHitActor); its `enter` / `joinHooks` chain
//! is the joined hooks composed here: pawn.rs's `pawnHooks` (the value hooks, set by `pawn::hooks`), then the joined
//! files' in the character registry's order (bow, attach, car, music, popcorn, umbrella, perks; of those only attach.rs
//! joins hooks). A hook two files hold is composed in this file. Each file enters its own classes in that order.

pub mod attach;
pub mod bots;
pub mod bow;
pub mod car;
pub mod music;
pub mod pawn;
pub mod perks;
pub mod popcorn;
pub mod state;
pub mod umbrella;

pub use state::{GizmoState, gizmo, gizmo_mut, gizmo_of, gizmo_w};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "gizmo";

/// `state: () => new GizmoState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Gizmo(GizmoState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance); then pawn.rs's (its
/// BeginPlay), which finds it.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Gizmo(GizmoState::default());
    pawn::match_reset(w, f);
}

/// `timerTick`: pawn.rs's (DrivingTurn's RetriggerableDelay), then attach.rs's (the Delay(0) and the AttachTimer), the
/// joinHooks order.
fn timer_tick(w: &mut World, f: FighterId) {
    pawn::timer_tick(w, f);
    attach::timer_tick(w, f);
}

/// Gizmo's entries in the character registry's order: his hooks (mod.rs's and pawn.rs's, then attach.rs's), then
/// pawn, bow, attach, car, music, popcorn, umbrella, perks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        // AnimNotify_C023A_MoveToActorHit (attach.rs)
        reads_recent_hit_actor: true,
        timer_tick: Some(timer_tick),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    attach::hooks(&mut h);
    bow::bind(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    bow::register(r)?;
    attach::register(r)?;
    car::register(r)?;
    music::register(r)?;
    popcorn::register(r)?;
    umbrella::register(r)?;
    perks::register(r)?;
    Ok(())
}
