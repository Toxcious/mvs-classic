//! The items of the shared item pools a character's code drops: those of Bugs Bunny's ACME box
//! (ItemDispenser_Bugs, `chars::bugs::dispenser`) and the FFA cars (Item_CarWheel_C and the cars it spawns). The
//! stage's items are `actors::item`'s.

pub mod armor_token;
pub mod banana_guard;
pub mod bounce_pad;
pub mod car;
pub mod dynamite;
pub mod heal;
pub mod helper;
pub mod meeseeks;
pub mod mystery;
pub mod nepter;
pub mod plain;
pub mod proximity_mine;
pub mod safe;
pub mod snowball;
pub mod snowball_head;

use crate::registry::{Registry, RegistryError};

/// The items' entries, in registration order.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    dynamite::register(r)?;
    helper::register(r)?;
    banana_guard::register(r)?;
    meeseeks::register(r)?;
    nepter::register(r)?;
    bounce_pad::register(r)?;
    proximity_mine::register(r)?;
    snowball::register(r)?;
    snowball_head::register(r)?;
    car::register(r)?;
    plain::register(r)?;
    heal::register(r)?;
    armor_token::register(r)?;
    mystery::register(r)?;
    safe::register(r)?;
    Ok(())
}
