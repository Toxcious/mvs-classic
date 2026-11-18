//! Homing moves: UProjectileMovementComponent's homing as a graph outside the actor's own
//! class writes it (bIsHomingProjectile, HomingAccelerationMagnitude, HomingTargetComponent: a buff's,
//! `combat/elem.rs` setHoming), for a Projectile and an Item's ProjectileMovement alike. The target is an actor's
//! root component: a fighter's CapsuleComponent (its centre, actorY) or a spawned actor's root (its location).
//!
//! ComputeAcceleration adds ComputeHomingAcceleration while bIsHomingProjectile and HomingTargetComponent.IsValid():
//! (the target component's location - the updated component's location).GetSafeNormal() x the magnitude.
// `!(a < b)` is kept as written, true for a NaN: partial_cmp would hide that.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::projectile::BuffHome;
use crate::fighter::moves::V2;
use crate::math;
use crate::world::World;

/// The acceleration of one move from (x, y); `None` for none (an invalid
/// target; a zero vector gives zero).
// Unverified: a spawned actor target is invalid once it is dead in the port (destroyed); one the build releases to
// its pool is not destroyed there and would stay a valid target (not read). A fighter target is valid until it is
// a minion destroyed (dead = infinity); a player's pawn stays (its ring-out and the match end keep it).
// Unverified: the destroyed minion's capsule taken to be invalid at once (IsValid on a component pending kill).
pub fn home_toward(w: &World, x: Real, y: Real, h: &BuffHome) -> Option<V2> {
    let (tx, ty) = match h.to {
        ActorRef::Fighter(f) => {
            let fi = w.fighter(f);
            if fi.life.is_minion && fi.life.dead == Real::INFINITY {
                return None;
            }
            (fi.body.x, crate::fighter::actor_y(w, f))
        }
        a => {
            let (ax, ay, dead) = crate::actors::world_actor::actor_place(w, a)?;
            if dead {
                return None;
            }
            (ax, ay)
        }
    };
    let dx = tx - x;
    let dy = ty - y;
    // ComputeHomingAcceleration 14298c730: GetSafeNormal (1e-8, the == 1 shortcut) x the magnitude.
    let u = math::ue::safe_normal([0.0, dx, dy], math::ue::SMALL_NUMBER);
    Some(V2 { x: u[1] * h.mag, y: u[2] * h.mag })
}

/// The buff's homing target.
pub fn buff_home(to: ActorRef, mag: Real) -> BuffHome {
    BuffHome { to, mag }
}
