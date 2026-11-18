//! Iron Giant (C017; content irongiant, Pawn_C017): his state, the hover and its
//! fuel, the rage and weapon mode (Comp_C017_Hover_Rage), the ally turret a teammate climbs into
//! (Comp_C017_Ally_Turret and its reticle and bomb), his actors (the eye laser's bolts, the cannon ball, the stomp
//! wave, the down thruster, the bomb, the missiles, the scrap and the art), the weapon mode aimers and the cannon's
//! burst, the plasma shot's hold, the grab's hold (Buff_HeldBy_C017), the wall, C017_AnimBP's layers under the shapes
//! and his perks (AllyBarrierGrayHealth, GrayHealthThornsStack, FastWeaponMode, ScorchedEarth). His attacks themselves
//! are data played by the shared code.
//!
//! `mod.rs` is pawn.rs (the hooks, conditions, notifies), hold.rs (Buff_HeldBy_C017), plasma.rs (firePlasma,
//! Buff_HeldByPlasmaBall), perks.rs (C017_TempHealth_DamageReflect_Passive) and cannon_aim.rs (fireCannon, fireDisc,
//! burstHits and the aimers' entries). Each file enters its own classes. The bot injections come with the bots'
//! crate.

pub mod actor_state;
pub mod actors;
pub mod cannon_aim;
pub mod hold;
pub mod hover_rage;
pub mod pawn;
pub mod perks;
pub mod plasma;
pub mod pose;
pub mod split;
pub mod state;
pub mod turret;
pub mod turret_actors;
pub mod wall;

pub use state::{
    GiantState, bind_giant_crouch, giant, giant_blocking, giant_crouched, giant_gliding, giant_hover_fast_fall_block,
    giant_land_hover, giant_mut, giant_of, giant_w, hover_rage_own_hit, unbind_giant_crouch,
};

use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The roster key.
pub const KEY: &str = "irongiant";

/// `state: () => new GiantState()`.
fn new_state(_w: &World, _f: FighterId) -> CharState {
    CharState::IronGiant(GiantState::default())
}

/// Iron Giant's entries: his hooks, then each file's classes in registration order (`mod.rs` imports
/// `actors.rs`, which imports `perks.rs`, then `turret.rs`, which imports `turret_actors.rs`; its own body enters the
/// conditions, notifies, the grab's hold, the two buffs and the aimers' entries; the registry imports
/// `wall.rs` after it).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let mut h = CharHooks { state: Some(new_state), ..CharHooks::default() };
    pawn::hooks(&mut h);
    hover_rage::hooks(&mut h);
    turret::hooks(&mut h);
    pose::hooks(&mut h);
    split::hooks(&mut h);
    r.chars.enter(KEY, h)?;
    perks::register(r)?;
    actors::register(r)?;
    turret_actors::register(r)?;
    turret::register(r)?;
    pawn::register(r)?;
    hold::register(r)?;
    plasma::register(r)?;
    cannon_aim::register(r)?;
    wall::register(r)?;
    Ok(())
}
