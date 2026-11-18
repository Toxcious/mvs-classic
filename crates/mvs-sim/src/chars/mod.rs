//! The characters' own code: per character its fighter state, its hooks, its actors, buffs,
//! conditions and notifies, entered into the registry by its `register`.

pub mod arya;
pub mod batman;
pub mod black_adam;
pub mod bugs;
pub mod creature;
pub mod finn;
pub mod garnet;
pub mod gizmo;
pub mod harley;
pub mod iron_giant;
pub mod jake;
pub mod lebron;
pub mod marvin;
pub mod morty;
pub mod rick;
pub mod shaggy;
pub mod shared;
pub mod steven;
pub mod stripe;
pub mod superman;
pub mod taz;
pub mod tom_and_jerry;
pub mod velma;
pub mod wonder_woman;

use crate::State;
use crate::registry::{Registry, RegistryError};

/// A fighter's character state (`Fighter.cs`, made by its `CharHooks.state`), a variant per character (Velma's police
/// minion a variant of its own).
#[derive(State, Debug, Default)]
#[allow(
    clippy::large_enum_variant,
    reason = "held inline in the fighter: a rollback save copies it with no allocation"
)]
pub enum CharState {
    /// A character with no state of its own (`cs` null).
    #[default]
    None,
    Shaggy(shaggy::ShaggyState),
    Batman(batman::BatmanState),
    Superman(superman::SupermanState),
    Lebron(lebron::LebronState),
    Arya(arya::AryaState),
    Creature(creature::ReindogState),
    BlackAdam(black_adam::BlackAdamState),
    WonderWoman(wonder_woman::WonderWomanState),
    Harley(harley::HarleyState),
    Morty(morty::MortyState),
    IronGiant(iron_giant::GiantState),
    Steven(steven::StevenState),
    Garnet(garnet::GarnetState),
    Taz(taz::TazState),
    Bugs(bugs::BugsState),
    Rick(rick::RickState),
    Finn(finn::FinnState),
    Stripe(stripe::StripeState),
    Jake(jake::JakeState),
    TomAndJerry(tom_and_jerry::TomAndJerryState),
    Velma(velma::VelmaState),
    VelmaPolice(velma::VelmaPoliceState),
    Marvin(marvin::MarvinState),
    Gizmo(gizmo::GizmoState),
}

/// Every character module's entries, in the character registry's order: the notifies Gizmo and Stripe share (its first
/// import), Shaggy, Morty, Iron Giant, Wonder Woman, Harley Quinn, Batman, Superman, LeBron, Arya, Reindog, Black Adam,
/// Bugs Bunny, Rick, Taz, Steven, Garnet, Finn, Jake, Tom and Jerry, Velma, Marvin, Stripe, Gizmo.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    shared::gizmo_stripe_notifies::register(r)?;
    shaggy::register(r)?;
    morty::register(r)?;
    iron_giant::register(r)?;
    wonder_woman::register(r)?;
    harley::register(r)?;
    batman::register(r)?;
    superman::register(r)?;
    lebron::register(r)?;
    arya::register(r)?;
    creature::register(r)?;
    black_adam::register(r)?;
    bugs::register(r)?;
    rick::register(r)?;
    taz::register(r)?;
    steven::register(r)?;
    garnet::register(r)?;
    finn::register(r)?;
    jake::register(r)?;
    tom_and_jerry::register(r)?;
    velma::register(r)?;
    marvin::register(r)?;
    stripe::register(r)?;
    gizmo::register(r)?;
    Ok(())
}
