//! Jake (content jake, Pawn_Jake): his state, the pawn and Jake_AnimBP's sim part, the
//! stretch, the house, the horse, the swallow (Comp_JakeEat and its buffs), the enemy and ally spits, the forward-air
//! aim punch, the weapons (Comp_WeaponSwap) and the perks. His attacks themselves are data played by the shared code.
//!
//! `mod.rs` is this file (the state, matchReset's new state); its `enter` / `joinHooks` chain is the joined hooks
//! composed here: pawn.rs's `pawnHooks`, then the joined files' in the character registry's order (stretch, house,
//! horse,
//! eat, eat-buffs, spit, ally-spit, aim, weapons, perks). A hook two files hold is composed in this file. Each file
//! enters its own classes in that order. bots.rs is not ported.

pub mod aim;
pub mod ally_spit;
pub mod anim;
pub mod bots;
pub mod eat;
pub mod eat_buffs;
pub mod horse;
pub mod house;
pub mod pawn;
pub mod perks;
pub mod spit;
pub mod state;
pub mod stretch;
pub mod weapons;

pub use state::{JakeState, jake, jake_mut, jake_of, jake_w};

use crate::chars::CharState;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "jake";

/// `state: () => new JakeState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::Jake(JakeState::default())
}

/// `matchReset`: a match reset spawns a new pawn, so a new state (and a new anim instance: Jake_AnimBP's variables at
/// their defaults, the slot blend at INDEX_NONE); then eat.rs's (Comp_JakeEat's BeginPlay), which finds it.
fn match_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).cs = CharState::Jake(JakeState::default());
    eat::match_reset(w, f);
}

/// `chargeEnd`: pawn.rs's (ReachCheck), then aim.rs's.
fn charge_end(w: &mut World, f: FighterId, n: NotifyRef) {
    pawn::charge_end(w, f, n);
    aim::charge_end(w, f, n);
}

/// `hitStunStart`: house.rs's, then horse.rs's (each destroys its live actor).
fn hit_stun_start(w: &mut World, f: FighterId) {
    house::hit_stun_start(w, f);
    horse::hit_stun_start(w, f);
}

/// Jake's entries in the character registry's order: his hooks (mod.rs's and pawn.rs's, then the joined files': eat,
/// spit), then pawn, stretch, house, horse, eat-buffs, spit, ally-spit, aim, weapons, perks (eat.rs enters no class).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        match_reset: Some(match_reset),
        charge_end: Some(charge_end),
        hit_stun_start: Some(hit_stun_start),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    eat::hooks(&mut h);
    spit::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    pawn::register(r)?;
    stretch::register(r)?;
    house::register(r)?;
    horse::register(r)?;
    eat_buffs::register(r)?;
    spit::register(r)?;
    ally_spit::register(r)?;
    aim::register(r)?;
    weapons::register(r)?;
    perks::register(r)?;
    Ok(())
}
