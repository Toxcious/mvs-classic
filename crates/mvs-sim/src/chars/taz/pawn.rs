//! Pawn_C015's BPI_C015 functions: the eat level, the eaten fighters, the eaten
//! projectile and the knock-out's reset.

use super::{taz, taz_of, taz_w};
use crate::Real;
use crate::fighter::FighterId;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `updateEatLevel` (UpdateEatLevelServer(Delta, Add, UpdateOwner) @10850-11085; a reliable server RPC, the port is the
/// authority): EatLevel = FClamp(Add ? EatLevel + Delta : EatLevel - Delta, 0, 1) @10942-11044; with UpdateOwner,
/// SetEatLevelOwner(EatLevel) @4320 (the HUD reads eatLevel); then FFloor(3 x EatLevel) against EatWeightAdditions and,
/// unequal, SetWeightServer @4549, whose body is empty in this build: nothing is kept for it. Called by eat.rs,
/// projectile_eat.rs and tasty.rs.
pub fn update_eat_level(w: &mut World, f: FighterId, delta: Real, add: bool, update_owner: bool) {
    let _ = update_owner;
    let s = taz_w(w, f);
    s.eat_level = min(max(if add { s.eat_level + delta } else { s.eat_level - delta }, 0.0), 1.0);
}

/// `adjustEatValue` (BPI_AdjustEatValue(Delta, Add) @14600 -> @13001): on authority UpdateEatLevelServer(Delta, Add,
/// true) @13031. Its one caller is C015Boot.
pub fn adjust_eat_value(w: &mut World, f: FighterId, delta: Real, add: bool) {
    update_eat_level(w, f, delta, add, true);
}

/// `onKnockedOut` (OnKnockedOutServer @16645, bound at BeginPlay @10150-10471 to the pawn's own OnExitedArena, on
/// authority): UpdateEatLevelServer(1.0, false, true): the meter empties at a ring-out.
pub fn on_knocked_out(w: &mut World, f: FighterId) {
    update_eat_level(w, f, 1.0, false, true);
}

/// `eatMeterValue` (BPI_GetEatMeterValue): EatLevel. A fighter without BPI_C015: `None` (the callers' failed cast).
pub fn eat_meter_value(w: &World, f: FighterId) -> Option<Real> {
    taz_of(w, f).map(|s| s.eat_level)
}

/// `hasEatenFighters` (BPI_HasEatenFighters): Map_Keys(EatenFighters).Length > 0.
pub fn has_eaten_fighters(w: &World, f: FighterId) -> bool {
    !taz(w, f).eaten_fighters.is_empty()
}

/// `isEatingProjectile` (BPI_IsEatingProjectile): ProjectilesBeingEaten > 0 (the counter is projectile_eat.rs's).
pub fn is_eating_projectile(w: &World, f: FighterId) -> bool {
    taz(w, f).projectiles_being_eaten > 0.0
}

/// `hasEatenAlly` (BPI_HasEatenAlly): HitAlly (tornado.rs's).
pub fn has_eaten_ally(w: &World, f: FighterId) -> bool {
    taz(w, f).hit_ally
}

/// `storedProjectileClass` (BPI_GetStoredProjectile's StoredProjectileClass): StoredProjectile, or C015Trash_C when it
/// is None (@0-68). The stored class itself is projectile_eat.rs's.
pub fn stored_projectile_class(w: &World, f: FighterId) -> Name {
    let c = taz(w, f).stored_projectile;
    if c == Name::default() { name!("C015Trash_C") } else { c }
}
