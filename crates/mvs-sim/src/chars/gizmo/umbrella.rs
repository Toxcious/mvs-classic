//! Gizmo's umbrella: Buff_C023A_ProjectileBlock and
//! Buff_C023A_AllyProjectileBlock, one kind (the fighter's projectile armor, a stack off per projectile hit). No pawn
//! hook.

use crate::Real;
use crate::combat::buffs::{buff_signal, remove_stack};
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::FighterId;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// `PROJECTILE_BLOCK`.
pub const PROJECTILE_BLOCK: &str = "Buff_C023A_ProjectileBlock_C";
/// `ALLY_PROJECTILE_BLOCK`.
pub const ALLY_PROJECTILE_BLOCK: &str = "Buff_C023A_AllyProjectileBlock_C";

// Buff_C023A_ProjectileBlock and Buff_C023A_AllyProjectileBlock, the same but for the VFX and a sound post (offsets of
// Buff_C023A_ProjectileBlock):
// - ReceiveBeginPlay: the parent's; the owner's Comp_Pawn_DamageManager valid -> HasProjectileArmor = true @908;
//   (authority) the DamageManager's HitByEnemy bound to OnHitByEnemy @1487.
// - OnHitByEnemy(Attacker) @1552: the attacker's Comp_Actor_Destructible valid and its IsProjectile @1693 ->
//   RemoveStack(1) @1725 and BPI_ConditionalStackUpdate(1) @1744. No IgnoreProjectileTypes test (Buff_ProjectileBlock's):
//   a Light or Transcendent projectile takes a stack too.
// - ReceiveEndPlay @1764: the parent's; the DamageManager valid -> HasProjectileArmor = false @1968 (a plain write).
// No reflect: neither buff writes one. What the umbrella does to a projectile is the shared projectile armor.

/// The umbrella's begin: projectile armor on.
fn umbrella_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    w.fighter_mut(f).invuln.projectile_armor = true;
}

/// The umbrella's on: the DamageManager's HitByEnemy: the attacker's Destructible IsProjectile (false for a fighter and
/// for an actor without the component).
fn umbrella_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitByEnemy || !a.by.is_some_and(|by| by.is_projectile) {
        return false;
    }
    remove_stack(w, f, b, 1);
    // BPI_ConditionalStackUpdate(1) on the actor RemoveStack may have ended (BuffSignal ConditionalBuffUpdate:
    // Finn_Shop_ArmorBuns_Buff alone binds it).
    buff_signal(w, f, BuffSignal::ConditionalBuffUpdate, BuffSignalArg { source: Some(b), ..BuffSignalArg::default() });
    false
}

/// The umbrella's end: projectile armor off.
fn umbrella_end(w: &mut World, f: FighterId, _b: BuffRef) {
    w.fighter_mut(f).invuln.projectile_armor = false;
}

/// The umbrella's hooks, both classes'.
fn umbrella() -> BuffHooks {
    BuffHooks { begin: Some(umbrella_begin), on: Some(umbrella_on), end: Some(umbrella_end), ..BuffHooks::default() }
}

/// This file's registry entries: the two umbrella buffs.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(PROJECTILE_BLOCK, BuffEntry::new(BuffClass::BuffC023AProjectileBlock, umbrella()))?;
    r.buffs.enter(ALLY_PROJECTILE_BLOCK, BuffEntry::new(BuffClass::BuffC023AAllyProjectileBlock, umbrella()))
}
