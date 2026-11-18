//! The dash to the marker: ChargeToGauntlet @6469-7764 on the shared
//! Buff_SafeMoveToPosition (`pawn::charge_to_gauntlet`: BPI_ChargeToGauntlet), AnimNotify_GarnetMoveToGauntlets (End
//! destroys her own MoveToPosition), AnimNotify_GarnetRotateTowardsGauntlets, GarnetAtMarker's predicate (pawn.rs enters
//! the condition: [`garnet_at_marker`]). No spawned class and no pawn hook: the move's Delay and its tick are the ally
//! route's (`fighter::match_side` safe_move_begin / end_move_buff).
//! What the montage does around it is data, played by shared code (Garnet_Attack_Special_MoveToMarker_Montage4 /
//! _Montage3): the hand hit that gives Buff_HeldByEnemyGarnet (hold.rs), the jump to Arrive on GarnetAtMarker_C,
//! Arrive's AnimNotify_ApplyBuffNearbyPlayer (Buff_Caught_C on allies within 700) and its LaunchSelf (0, 500, 1000), the
//! Throw section's LaunchSelf (0, 10, 1000). Flying's exit: nothing in her graphs sets the mode back; a launch with a Z
//! does (native): Arrive's and the Throw's both.

use super::pawn::{charge_to_gauntlet, get_marker_location};
use super::state::{garnet_of, garnet_w};
use crate::Real;
use crate::events::NotifyRef;
use crate::fighter::control::pawn_prop;
use crate::fighter::match_side::{end_move_buff, safe_move_begin};
use crate::fighter::moves::set_flying_mode;
use crate::fighter::{FighterId, actor_y, facing, set_facing};
use crate::math::ue::size2d;
use crate::registry::props::flag;
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;

/// BPI_ChargeToGauntlet(Charging) @6469-7764. True: ChargingToMarker = true and LocationBeforeMove = her actor location
/// (every instance), then with authority: CheckHasBuff(Buff_MoveToPosition_C) (a child counts: her own
/// Buff_SafeMoveToPosition, an ally route's move) -> return. Else SetMovementMode(5 Flying) @6746 and
/// SpawnBuffActor(Buff_SafeMoveToPosition_C, self, self, 0, 0.0) @6786. IdleGauntlet valid and not being destroyed:
/// MulticastBeginMove(the marker's location, true, true, Vector_Distance(her, marker) /
/// DashToGauntletsSpeedPerSecond); else MulticastBeginMove(MarkerLocationBeforeDestroy, ...) @7696. No clamp of the time
/// (Divide_FloatFloat: 0 for a divisor of 0). False: ChargingToMarker = false, BPI_SetAimDirection(0, true, false)
/// @7764.
// Deviation: the buff has no buff-list entry (its StateInfo breaks and blocks nothing). IsActorBeingDestroyed is not
// kept: a marker the port holds and has not ended counts as valid and not being destroyed, so the second branch's
// IsValid-true case (a marker being destroyed: the distance to it, the move to the saved location) is never taken.
pub fn charge_to_gauntlet_body(w: &mut World, f: FighterId, charging: bool) {
    if !charging {
        garnet_w(w, f).charging_to_marker = false;
        super::pawn::set_aim_direction(w, f, 0.0, true, false);
        return;
    }
    let (x, ay) = (w.fighter(f).body.x, actor_y(w, f));
    let g = garnet_w(w, f);
    g.charging_to_marker = true;
    g.location_before_move_x = x;
    g.location_before_move_y = ay;
    if w.fighter(f).team.ally.buff > 0.0 {
        return;
    }
    set_flying_mode(w, f, true);
    let g = super::state::garnet(w, f);
    let marker = g.idle_gauntlet.and_then(|id| w.actors.projectiles.get(id)).filter(|m| !m.dead).map(|m| (m.x, m.y));
    let (tx, ty) = marker.unwrap_or((g.pawn.marker_location_before_destroy_x, g.pawn.marker_location_before_destroy_y));
    let dist = size2d([tx - x, ty - ay]);
    let speed = pawn_prop(w, f, "DashToGauntletsSpeedPerSecond").unwrap_or(0.0);
    safe_move_begin(w, f, tx, ty, if speed == 0.0 { 0.0 } else { dist / speed }, true, None);
}

/// GarnetAtMarker Check: BPI_IsMovingToMarker (ChargingToMarker; false for a failed BPI_Garnet cast) AND NOT
/// BPI_CheckIfHasBuff(Buff_MoveToPosition_C) (her own Buff_SafeMoveToPosition is one: `team.ally.buff`, live from
/// MulticastBeginMove to the end of the last re-aimed move). pawn.rs enters it.
pub fn garnet_at_marker(w: &World, f: FighterId) -> bool {
    garnet_of(w, f).is_some_and(|g| g.charging_to_marker) && w.fighter(f).team.ally.buff <= 0.0
}

/// AnimNotify_GarnetMoveToGauntlets Begin, on an owner that implements BPI_Garnet: BPI_ChargeToGauntlet(true).
fn move_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        charge_to_gauntlet(w, f, true);
    }
}

/// End: the cast's BPI_ChargeToGauntlet(false), then (whatever the cast gave) with authority and a BaseFighter owner:
/// BPI_CheckIfHasBuff(Buff_MoveToPosition_C), its Instigator the owner -> K2_DestroyActor (EndPlay: the force move
/// removed by its id; the mode stays Flying, native). Every port Buff_MoveToPosition has its owner as Instigator.
fn move_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        charge_to_gauntlet(w, f, false);
    }
    if w.fighter(f).team.ally.buff > 0.0 {
        end_move_buff(w, f);
    }
}

/// AnimNotify_GarnetRotateTowardsGauntlets Begin, on a BPI_Garnet owner (no authority test):
/// BPI_GetMarkerLocation(FacePositionBeforeWarp) (Success not read); a location within 1e-4 of the owner's -> nothing.
/// Else the marker's UE Y below the owner's (the sim's +x side) while facing left -> SetRotationLogic(false) (face
/// right); else (at or above) while facing right -> SetRotationLogic(true). So she turns to face it. FacePositionBeforeWarp:
/// the instance's (CDO false; M3's instance true).
fn rotate_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_none() {
        return;
    }
    let before = flag(crate::fighter::match_side::notify_props(w, f, n), "FacePositionBeforeWarp");
    let (_, mx, my) = get_marker_location(w, f, before);
    let (x, ay) = (w.fighter(f).body.x, actor_y(w, f));
    if (mx - x).abs() <= 1e-4 && (my - ay).abs() <= 1e-4 {
        return;
    }
    let face: Real = facing(w, f);
    if mx > x {
        if face < 0.0 {
            set_facing(w, f, 1.0);
        }
    } else if face > 0.0 {
        set_facing(w, f, -1.0);
    }
}

/// This file's registry entries (AnimNotify_GarnetMoveToGauntlets_C, AnimNotify_GarnetRotateTowardsGauntlets_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_GarnetMoveToGauntlets_C",
        NotifyHandler { begin: Some(move_begin), end: Some(move_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_GarnetRotateTowardsGauntlets_C",
        NotifyHandler { begin: Some(rotate_begin), ..NotifyHandler::default() },
    )
}
