//! Wonder Woman (content wonderwoman, Pawn_WonderWomanV3): her state and meter, her
//! pawn's hooks (the bracer charge, the hair, the shield walk and the lasso aim slots), her conditions and notifies,
//! the bracer clash, the bracer shield and its Buff_NullifyNextHit, the lasso and its perks (LassoGrapple,
//! LassoTipper), her shields (WW_ShieldActor and its up special child, on the shared `actors::shield_actor`) and
//! her other perks (FastFallSlam, ProjectileBlock and its evade shield). Her attacks themselves are data played by
//! the shared code.
//!
//! `actors.rs` is split in two files: actors.rs (the clash, the bracer shield, the lasso, Buff_NullifyNextHit,
//! LassoOutCondition) and shield.rs (the two WW_ShieldActor entries and WWShieldBlockedProjectile). Each file enters
//! its own classes. The bot injections come with the bots' crate.

pub mod actors;
pub mod pawn;
pub mod perks;
pub mod perks_lasso;
pub mod shield;
pub mod state;

pub use state::WonderWomanState;

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "wonderwoman";

/// `state: () => new WonderWomanState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::WonderWoman(WonderWomanState::default())
}

/// Wonder Woman's entries: her hooks, then each file's classes (`mod.rs` imports `perks.rs` and `perks_lasso.rs`
/// ahead of its own entries; then the registry imports `actors.rs`).
// Deviation: the two WW_ShieldActor entries are entered after WW_BracerShield_C (shield.rs), not between the clash and
// the bracer shield as `actors.rs` lists them.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks {
        state: Some(new_state),
        slot_mix: Some(state::slot_mix),
        slot_mix_mut: Some(state::slot_mix_mut),
        ..CharHooks::default()
    };
    pawn::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    perks_lasso::register(r)?;
    pawn::register(r)?;
    actors::register(r)?;
    shield::register(r)?;
    Ok(())
}
