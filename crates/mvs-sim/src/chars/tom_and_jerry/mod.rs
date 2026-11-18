//! Tom and Jerry (content tomandjerry, Pawn_TomAndJerry, EFighter 15): his state,
//! the pawn and Tom_and_Jerry_AnimBP's slot inputs and rocket arms, Jerry as his own actor and as an ally's follower,
//! the champagne and its corks, the tennis ball, the slingshot, the rockets, the fishing lure, the mouse trap and its
//! cheese, the dynamite. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs's `pawnHooks`, then the joined files' (champagne's tick, fishing's montageEnded). Each file
//! enters its own classes in registration order (below).

pub mod aim;
pub mod arms;
pub mod bots;
pub mod champagne;
pub mod cheese;
pub mod dynamite;
pub mod fishing;
pub mod follower;
pub mod jerry;
pub mod kicked;
pub mod pawn;
pub mod rocket;
pub mod slingshot;
pub mod state;
pub mod tennis;
pub mod trap;

pub use state::{TomAndJerryState, tj, tj_mut, tj_of, tj_w};

use crate::actors::projectile::ProjectileExt;
use crate::actors::{ActorId, ActorRef, WorldActorState};
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::names::Name;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "tomandjerry";

/// `state: () => new TomAndJerryState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::TomAndJerry(TomAndJerryState::default())
}

/// A projectile or an aimer holds world actor `id` as its Jerry or homing target: the actor stays in
/// `Actors::gone_actors` after its removal, as the object stays held (the record writes it whole).
pub fn names_actor(w: &World, id: ActorId) -> bool {
    let r = Some(ActorRef::Actor(id));
    w.actors.projectiles.iter().any(|(_, p)| match &p.ext {
        ProjectileExt::AimRocket(s) => s.jerry == r || s.home_target == r,
        ProjectileExt::TennisBall(s) => s.jerry == r || s.home_target == r,
        _ => false,
    }) || w.actors.actors.iter().any(|(_, a)| matches!(&a.state, WorldActorState::NoJerryAim(s) if s.jerry == r))
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance); pawn.rs's matchReset (its
/// BeginPlay) runs after it and finds it.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::TomAndJerry(TomAndJerryState::default());
    pawn::match_reset(w, f);
}

/// `tick`: pawn.rs's (the JerryPointer, the wall traces), then champagne.rs's, the joinHooks order.
fn tick(w: &mut World, f: FighterId) {
    pawn::tick(w, f);
    champagne::tick(w, f);
}

/// `montageEnded`: fishing.rs's alone.
fn montage_ended(w: &mut World, f: FighterId, m: Name) {
    fishing::montage_ended(w, f, m);
}

/// Tom and Jerry's entries in registration order: pawn.rs (its conditions and notifies; it imports
/// aim
/// and arms, which enter nothing), his hooks (mod.rs's and pawn.rs's, then the joined files': champagne, fishing),
/// then the registry's imports as they evaluate: jerry.rs first loads champagne, cheese, rocket and tennis
/// (follower enters nothing), then jerry itself, then slingshot, kicked, fishing, trap, dynamite.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    pawn::register(r)?;
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        tick: Some(tick),
        montage_ended: Some(montage_ended),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    champagne::register(r)?;
    cheese::register(r)?;
    rocket::register(r)?;
    tennis::register(r)?;
    jerry::register(r)?;
    slingshot::register(r)?;
    kicked::register(r)?;
    fishing::register(r)?;
    trap::register(r)?;
    dynamite::register(r)?;
    Ok(())
}
