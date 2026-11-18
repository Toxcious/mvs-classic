//! Garnet's aim: SetAimDirection @9777 and its AimResetTimeline, SetGarnetAimFlag
//! @12250, SetReadAimFromMovement @10019, the Tick's aim @13946-14522 and the charging-to-marker aim @13705-13920, and
//! Garnet_AnimBP's slot inputs.
//! - BPI_SetAimDirection(Direction, Reset, Fast) @9777: Reset -> AimDirectionBeforeReset = AimDirection @9791,
//!   AimResetTimeline.PlayFromStart @9818; else AimResetTimeline.Stop @9856, then AimDirection = Lerp(AimDirection,
//!   Direction, Fast ? 0.375 : 0.1) @9889-9991.
//! - AimResetTimeline (TimelineLength 0.5, CurveFloat_1 linear (0, 0) (0.5, 1); its Update @19931 -> @10062:
//!   AimDirection = Lerp(AimDirectionBeforeReset, 0, Alpha); Finished @19936: nothing).
//! - BPI_SetReadAimFromMovement(On, IsHorizontalMovement) @10019, then @3857: off -> SetAimDirection(0, true, false).
//! - Garnet_AnimBP's Update @2032-2446: AimDirection = the pawn's BPI_GetAimDirection @2173 -> AimDirectionInverted =
//!   -AimDirection @1164; IsOnGround = Conv_BoolToFloat(IsMovingOnGround) @2341-2446: the slot graph's three inputs.

use super::state::{garnet, garnet_w};
use crate::Real;
use crate::consts::{DELTA_SECONDS, HITPAUSE_DILATION};
use crate::fighter::FighterId;
use crate::fighter::tick::FrameMode;
use crate::math::ue::{KINDA_SMALL_NUMBER, normal};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::world::World;

const LERP_FAST: Real = 0.375;
const LERP_SLOW: Real = 0.1_f32 as Real;
/// AimResetTimeline_Template.TimelineLength (the curve's last key at 0.5 too).
const RESET_LENGTH: Real = 0.5;

/// UKismetMathLibrary::Lerp: A + Alpha x (B - A), float32.
fn lerp(a: Real, b: Real, alpha: Real) -> Real {
    a + (alpha * (b - a))
}

/// AimResetTimeline's Update @10062 at the current position: CurveFloat_1, linear from (0, 0) to (0.5, 1).
fn reset_update(w: &mut World, f: FighterId) {
    let a = &mut garnet_w(w, f).aim;
    let alpha = a.reset_pos / RESET_LENGTH;
    a.aim_direction = lerp(a.aim_direction_before_reset, 0.0, alpha);
}

/// BPI_SetAimDirection @9777.
pub fn set_aim_direction(w: &mut World, f: FighterId, direction: Real, reset: bool, fast: bool) {
    let a = &mut garnet_w(w, f).aim;
    if reset {
        a.aim_direction_before_reset = a.aim_direction; // @9791
        // PlayFromStart @9818: SetPlaybackPosition(0) fires the update (alpha 0: the aim it saved), then Play. The
        // update inside the call is UE 4.26 FTimeline::PlayFromStart.
        a.reset_pos = 0.0;
        reset_update(w, f);
        garnet_w(w, f).aim.reset_on = true;
        return;
    }
    a.reset_on = false; // AimResetTimeline.Stop @9856
    a.aim_direction = lerp(a.aim_direction, direction, if fast { LERP_FAST } else { LERP_SLOW });
}

/// BPI_SetReadAimFromMovement @10019 -> @3857.
pub fn set_read_aim_from_movement(w: &mut World, f: FighterId, on: bool, horizontal: bool) {
    let a = &mut garnet_w(w, f).aim;
    a.set_aim_value_from_movement = on;
    a.read_aim_as_horizontal_movement = horizontal;
    if !on {
        set_aim_direction(w, f, 0.0, true, false); // @3872
    }
}

/// BPI_SetGarnetAimFlag @12250.
pub fn set_garnet_aim_flag(w: &mut World, f: FighterId, on: bool) {
    garnet_w(w, f).aim.aim_flag = on;
}

/// Normal(V, 0.0001) of V = (0, -sx, sy) in sim terms. Returns (|UE Y|, UE Z).
fn safe_normal(sx: Real, sy: Real) -> (Real, Real) {
    let n = normal([0.0, sx, sy], KINDA_SMALL_NUMBER);
    (n[1].abs(), n[2])
}

/// The Tick's aim @13946-14522.
pub fn aim_tick(w: &mut World, f: FighterId) {
    let a = &garnet(w, f).aim;
    let (movement, flag, horizontal) = (a.set_aim_value_from_movement, a.aim_flag, a.read_aim_as_horizontal_movement);
    if movement {
        // GetVelocity @13960, Normal @13992, (Abs(Y) - 0.5) x 2 @14080-14159 or Z by ReadAimAsHorizontalMovement @14201.
        let (vx, vy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
        let (abs_y, z) = safe_normal(vx, vy);
        let v = if horizontal { (abs_y - 0.5) * 2.0 } else { z };
        set_aim_direction(w, f, v, false, false); // @14256
    } else if flag {
        // BPI_GetProjectileAimDirection @14296 (Comp_Spawner.ProjectileAimVector), FClamp(2 x (Z - 0.5), -1, 1)
        // @14365-14449.
        let z = w.fighter(f).items.proj_aim.z;
        let v = min(max((z - 0.5) * 2.0, -1.0), 1.0);
        set_aim_direction(w, f, v, false, false); // @14496
    } else {
        set_aim_direction(w, f, 0.0, false, false); // @14522
    }
}

/// The Tick's charging-to-marker aim @13705-13920, given the marker's location from GetMarkerLocation(false) @13715
/// (pawn.rs): Normal(location - K2_GetActorLocation) @13786-13832, its Z, SetAimDirection(Z, false, true) @13920.
pub fn marker_aim(w: &mut World, f: FighterId, mx: Real, my: Real) {
    let (x, ay) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
    let (_, z) = safe_normal(mx - x, my - ay);
    set_aim_direction(w, f, z, false, true);
}

/// AimResetTimeline's tick (UTimelineComponent, TG_PrePhysics; FTimeline::TickTimeline: position + PlayRate x dt in
/// float32, finished only past the length, then the update at the length and Finished in the same tick), on the owner's
/// dilated delta (a hit pause slows it).
// Unverified: the timeline and the pawn's ReceiveTick are in one tick group with no prerequisite; the timeline is taken
// as ticking after it (the Batcave fans' choice). Every ReceiveTick calls SetAimDirection(.., false, ..), which stops
// the timeline, so under this order a reset (SetReadAimFromMovement(false), ChargeToGauntlet(false)) never moves the
// aim past its PlayFromStart update: the next tick's lerp to the Tick's target takes over.
pub fn reset_timeline_tick(w: &mut World, f: FighterId) {
    if !garnet(w, f).aim.reset_on {
        return;
    }
    let paused = crate::fighter::mode(w, f) == FrameMode::Paused;
    let dt = DELTA_SECONDS * if paused { HITPAUSE_DILATION } else { 1.0 };
    let p = garnet(w, f).aim.reset_pos + dt;
    if p > RESET_LENGTH {
        garnet_w(w, f).aim.reset_pos = RESET_LENGTH;
        reset_update(w, f);
        garnet_w(w, f).aim.reset_on = false; // Finished @19936: nothing more
        return;
    }
    garnet_w(w, f).aim.reset_pos = p;
    reset_update(w, f);
}

/// Garnet_AnimBP's slot graph inputs (`CharHooks.slot_input`; `fighter::anim::slot_mix::graph_slots` walks the data's
/// graph: root DefaultSlot -> JumpSlot -> TwoWayBlend_1 (AimDirectionInverted: TwoWayBlend_2 / AimDown) ->
/// TwoWayBlend_2 (AimDirection: AimCenter / AimUp); each aim slot's source TwoWayBlend (IsOnGround: InAir / OnGround)
/// -> the second DefaultSlot -> the linked layer). The AnimBP's Update writes the three at this pose update from the
/// pawn's aim and the movement mode (IsMovingOnGround: the walking mode, `grounded`).
pub fn slot_input(w: &World, f: FighterId, n: Name) -> Real {
    if n == name!("AimDirection") {
        garnet(w, f).aim.aim_direction
    } else if n == name!("AimDirectionInverted") {
        -garnet(w, f).aim.aim_direction
    } else if n == name!("IsOnGround") {
        if crate::fighter::grounded(w, f) { 1.0 } else { 0.0 }
    } else {
        Real::NAN
    }
}
