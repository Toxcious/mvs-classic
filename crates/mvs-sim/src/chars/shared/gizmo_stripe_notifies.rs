//! The anim notify classes Gizmo (C023A) and Stripe (C023B) share or copy from each other
//! AnimNotify_C023A_COM_Rotation /
//! AnimNotify_C023B_COM_Rotation, AnimNotify_C023A_DettachFromAlly. The pawns' sides are hooks (CharHooks
//! `set_com_angle`, `last_dash_target`, `ally_attach`), filled by the characters; a pawn without the hook takes the
//! graph's failed cast.
//!
//! AnimNotify_C023A_COM_Rotation / AnimNotify_C023B_COM_Rotation (58 lines each, the same graph with BPI_C023A
//! BPI_Set_COM_Angle against BPI_C023B BPI_C023B_Set_COM_Angle; both copies read BPI_C023A BPI_GetLastDashTarget for
//! UseBuffMoveInfo). Received_NotifyBegin:
//! - the owner as BP_FighterCharacter and IsLocallyControlled (@107, @167; every fighter of the port);
//! - UseBuffMoveInfo -> the owner's BPI_GetLastDashTarget @316 (no BPI_C023A: DashTarget null @1592);
//! - BPI_IsFacingLeft @483; the owner as the kit's interface @571 (none: return @1576, nothing set);
//! - GetDirectionUnitVector(owner location, DashTarget location) @792; the last entry of the owner's
//!   Comp_Actor_MoveCommandManager.MoveCommands @1100 (none: an out-of-range get, MoveDelta (0, 0, 0));
//! - SelectFloat(Angle, -Angle, IsFacingLeft) @1223; SelectVector(direction, MoveDelta, UseBuffMoveInfo) @1278;
//!   SelectFloat(DegAtan2(Y, Z) of it, that angle, SetBasedOnMovement) @1456;
//! - Set_COM_Angle(it, true, BlendTime, FMax(TotalDuration - BlendTime, 0)) @1511 on the pawn (its Server ->
//!   Multicast -> the AnimBP's).
//!
//! Received_NotifyEnd: the mesh's AnimInstance as the kit's interface -> Set_COM_Angle(0, false, 0, 0) @121.
//! The class defaults are all zero / false (the CDOs list no property).
//!
//! AnimNotify_C023A_DettachFromAlly (13 lines; on Gizmo's and Stripe's DashGrab montages): Received_NotifyBegin reads
//! the owner's Comp_C023A_AllyAttach IsAttached @93; true -> Attach to Actor(null, false) @222, return true; else
//! return false. Stripe has no such component (Pawn_C023B has no Comp_C023A_AllyAttach): IsAttached of none reads
//! false, so on him it does nothing and warns nothing.

use crate::Real;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::notify_data;
use crate::math::max;
use crate::math::ue::{SMALL_NUMBER, deg_atan2, safe_normal};
use crate::registry::props::{flag, num_or};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;

fn com_rotation_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    let hooks = w.fighter(f).hooks.char;
    let use_buff = flag(p, "UseBuffMoveInfo");
    let target = if use_buff { hooks.last_dash_target.and_then(|h| h(w, f)) } else { None }; // @316 / @1592
    let left = crate::fighter::facing(w, f) < 0.0; // BPI_IsFacingLeft @483
    let Some(set) = hooks.set_com_angle else { return }; // @636 -> @1576
    // The vector in UE axes (Y = -sim x, Z = sim y).
    let (mut vy, mut vz): (Real, Real) = (0.0, 0.0);
    let fx = w.fighter(f).body.x;
    let fy = crate::fighter::actor_y(w, f);
    if use_buff {
        // GetDirectionUnitVector(From owner, To DashTarget) @792: (To - From) safe-normalised.
        let (ty, tz) = target.map_or((0.0, 0.0), |t| (-t.x, t.actor_y.unwrap_or(t.y)));
        let dy = ty + fx;
        let dz = tz - fy;
        let n = safe_normal([0.0, dy, dz], SMALL_NUMBER);
        (vy, vz) = (n[1], n[2]);
    } else if let Some(c) = w.fighter(f).moves.move_cmds.last() {
        // MoveDelta of the last command (Comp_Actor_MoveCommandManager @6098: (0, GetMoveDirection, Y x mod)); the
        // port keeps MoveDelta / TotalMoveTime (bx, by: world, facing applied), the same direction.
        vy = -c.bx;
        vz = c.by;
    }
    let angle = num_or(p, "Angle", 0.0);
    let sel = if left { angle } else { -angle }; // @1223
    let a = if flag(p, "SetBasedOnMovement") { deg_atan2(vy, vz) } else { sel }; // DegAtan2(Y, Z) @1410, @1456
    let blend = num_or(p, "BlendTime", 0.0);
    set(w, f, a, true, blend, max(crate::math::narrow(nt.dur - f64::from(blend)), 0.0)); // @1511
}

fn com_rotation_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(set) = w.fighter(f).hooks.char.set_com_angle {
        set(w, f, 0.0, false, 0.0, 0.0); // @121
    }
}

fn detach_from_ally_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let Some(h) = w.fighter(f).hooks.char.ally_attach else { return };
    if let Some(c) = h(w, f) // GetComponentByClass @42
        && c.is_attached
    {
        (c.attach_to_actor)(w, f, None, false); // @93 -> @222
    }
}

/// This file's registry entries (the three notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let com =
        NotifyHandler { begin: Some(com_rotation_begin), end: Some(com_rotation_end), ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_C023A_COM_Rotation_C", com)?;
    r.notifies.enter("AnimNotify_C023B_COM_Rotation_C", com)?;
    r.notifies.enter(
        "AnimNotify_C023A_DettachFromAlly_C",
        NotifyHandler { begin: Some(detach_from_ally_begin), ..NotifyHandler::default() },
    )
}
