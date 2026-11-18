//! The shared classes' effects: what a
//! buff or a spawned class any character can carry draws, one module each, made for every match after the
//! characters' (fx::chars::CharFx). Shaggy's shockwave, the first of them, is in his module (chars/shaggy.rs).

pub(super) mod actor_vfx;
pub(super) mod air_superiority;
pub(super) mod ally_dash;
pub(super) mod arcade;
pub(super) mod barrier;
#[cfg(test)]
mod buff_tests;
pub(super) mod disguise;
pub(super) mod educated;
pub(super) mod electric;
pub(super) mod elemental;
pub(super) mod firewall;
pub(super) mod heart_skin;
pub(super) mod ice;
pub(super) mod m010_pickups;
pub(super) mod minion;
pub(super) mod polymorph;
pub(super) mod projectile_block;
pub(super) mod speed;
pub(super) mod tutorial;

#[cfg(test)]
mod tests;
