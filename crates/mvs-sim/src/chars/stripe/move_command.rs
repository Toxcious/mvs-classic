//! AnimNotify_C023B_MoveCommand: AnimNotify_MoveCommand's copy with a
//! target pick (gun.rs's `find_target`), a notify state of its own (no parent but AnimNotifyState: its Begin, End and
//! Tick are copies of AnimNotify_MoveCommand's with a target pick).
//! Uses: C023B_Special_DashGrab_Montage and C023B_Special_DashGrab_Ground_Montage at 0.216-0.389 s (queued; MoveVector
//! (0, 600), StopOnEnd false, ClampSpeedOnEnd with (-800, -800)-(800, 800), X / Y stick influence 600 / 400).

use super::gun::{find_target, loc};
use crate::Real;
use crate::events::NotifyRef;
use crate::fighter::anim::notify_data;
use crate::fighter::common::warn_text;
use crate::fighter::moves::{
    MoveCtl, MoveEnd, V2, add_move_cmd, adjust_stick_influence, evade_move_vector, evade_play_rate,
};
use crate::fighter::{FighterId, actor_y, facing, grounded};
use crate::math::max;
use crate::math::ue::{SMALL_NUMBER, safe_normal, size2d};
use crate::registry::notifies::move_command_end;
use crate::registry::props::{flag, flag_or, num, num_or, obj};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// `p[k] ?? ZERO2` of a vector member; a member without `X` or `Y` reads `NaN` there, as JavaScript's `undefined`.
fn v2(p: &Props, k: &str) -> V2 {
    match obj(p, k) {
        None => V2 { x: 0.0, y: 0.0 },
        Some(o) => V2 { x: num(o, "X").unwrap_or(Real::NAN), y: num(o, "Y").unwrap_or(Real::NAN) },
    }
}

/// Received_NotifyBegin: OnlyPerformOnGround (default false) needs IsMovingOnGround @153; BPI_GetEvadeData @288 (its
/// EvadeAnimPlayRate), BPI_ApplyMoveBuffsToVector(MoveVector) @482 on every run (it clears ApplyCancelEvadeBuffs), the
/// new movement system (@825, the port's). Locally controlled: the move axes @1154 and BPI_IsFacingLeft @1330. Then
/// the TargetTracker's FindTarget @1469 (HasTarget, Target), and:
/// - Start = (0, Y, Z) of the owner's location @1824, End = (0, Target.Y, FMax(Z + 150, Target.Z)) @2240, Dir =
///   GetDirectionUnitVector(Start, End) @2291 ((End - Start) safe-normalised);
/// - the target vector (Facing Left ? Dir.Y : -Dir.Y, Dir.Z) x VSize2D(MoveVector) @2420-2563 (the facing's forward
///   component, the vector's length the raw MoveVector's);
/// - used when HasTarget AND GetDotProductTo(Target) >= -0.1 @2202 (the owner's forward . the unit offset, 3D: the
///   depth is 0 for both), else AdjustStickInfluence(Facing Left, IsEvade ? the buffed vector : MoveVector, the move
///   axes) @2793, the parent's own rule;
/// - BPI_AddMoveCommand(self, IsEvade ? TotalDuration / EvadeAnimPlayRate : TotalDuration, that, XYOverride,
///   ZOverride, ControlSpeed, ControlAngleMax / Min, ControlSpeedDecay, StopOnEnd, BreakAttach, WallSkate,
///   ControlSpeedFriction, DisablePredictiveHitboxes, ClampSpeedOnEnd, SpeedClampMin / Max) @2852: no
///   BlockDownMovement (the parent's has it).
///
/// Returns false. Received_NotifyTick: the parent's corner check behind CornerCheck (default false, set in no use):
/// refused here.
fn begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    if flag(p, "OnlyPerformOnGround") && !grounded(w, f) {
        return;
    }
    if flag(p, "CornerCheck") {
        warn_text(w, "AnimNotify_C023B_MoveCommand_C: CornerCheck set (its NotifyTick is not ported)");
    }
    let mv0 = v2(p, "MoveVector");
    let is_evade = flag(p, "IsEvade");
    let bv = evade_move_vector(w, f, mv0);
    let er = if is_evade { evade_play_rate(w, f) } else { 1.0 };
    let t = find_target(w, f);
    let mut mv: Option<V2> = None;
    if t.is_some() {
        let (tx, ty) = loc(w, t);
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        let fc = facing(w, f);
        // GetDotProductTo: forward (the facing along x) . (Target - owner) safe-normalised.
        let (ox, oz) = ((tx - fx), (ty - fy));
        let [_, nx, _] = safe_normal([0.0, ox, oz], SMALL_NUMBER);
        let dot = fc * nx;
        if dot >= -0.1 {
            let (ex, ez) = ((tx - fx), (max(fy + 150.0, ty) - fy));
            // GetDirectionUnitVector(Start, End), VSize2D(MoveVector)
            let [_, dx, dz] = safe_normal([0.0, ex, ez], SMALL_NUMBER);
            let len = size2d([mv0.x, mv0.y]);
            mv = Some(V2 { x: (fc * dx * len), y: (dz * len) });
        }
    }
    let mv = mv.unwrap_or_else(|| {
        if is_evade {
            let mut q = p.clone();
            let mut o = Props::new();
            o.insert("X".into(), Value::Num(f64::from(bv.x)));
            o.insert("Y".into(), Value::Num(f64::from(bv.y)));
            q.insert("MoveVector".into(), Value::Obj(o));
            adjust_stick_influence(w, f, &q)
        } else {
            adjust_stick_influence(w, f, p)
        }
    });
    let total = if er == 1.0 { nt.dur } else { f64::from(crate::math::narrow(nt.dur / f64::from(er))) };
    let end = MoveEnd {
        stop: flag_or(p, "StopOnEnd", true),
        clamp: flag_or(p, "ClampSpeedOnEnd", false),
        min: v2(p, "SpeedClampMin"),
        max: v2(p, "SpeedClampMax"),
    };
    let ctl = MoveCtl {
        speed: num_or(p, "ControlSpeed", 0.0),
        max: num_or(p, "ControlAngleMax", 0.0),
        min: num_or(p, "ControlAngleMin", 0.0),
        friction: num_or(p, "ControlSpeedFriction", 0.0),
        decay: num_or(p, "ControlSpeedDecay", 0.0),
    };
    let (xy, z) = (flag_or(p, "XYOverride", true), flag_or(p, "ZOverride", true));
    add_move_cmd(w, f, idx, crate::math::narrow(total), mv, xy, z, &end, false, flag_or(p, "WallSkate", false), &ctl);
}

/// This file's registry entries (AnimNotify_C023B_MoveCommand_C): the parent class's End (BPI_CompleteMoveCommand,
/// AnimNotify_C023B_MoveCommand Received_NotifyEnd @121 is the same call, with its deferral of a stopped montage's
/// queued End).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_C023B_MoveCommand_C",
        NotifyHandler { begin: Some(begin), end: Some(move_command_end), ..NotifyHandler::default() },
    )
}
