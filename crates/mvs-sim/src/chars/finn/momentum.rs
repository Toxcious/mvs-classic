//! Comp_FinnMomentumCharge: StartMomentum, the tick, EndMomentum, the high
//! five's charge, the run meter. Its variables are `FinnState::momentum` (state.rs).
//!
//! Comp_FinnMomentumCharge (offsets of its ubergraph), the pawn's component, whole: StartMomentum @193 -> @62-192
//! (NewMomentumStarted), the tick @1156-1747 (StoredMomentum, the run meter) and the high-five charge @1771-2221,
//! EndMomentum @225 (the charge component's DamageMultiplier @619, KnockBackMultiplier @888, PercentCharged @1106),
//! ResetHighFive @2343, OnFighterReset @2245; AnimNotify_Finn_Momentum and AnimNotify_FinnMomentumMoveCommand
//! (BPI_GetEvadeData, MomentumAmount scaling @423, ApplyEvadeBuffsToMoveVector @889, AddMoveCommandNotify @962).
//!
//! Class defaults (Default__Comp_FinnMomentumCharge_C; Pawn_Finn's component template sets none): DamageMult 1.0,
//! KnockBackMult 1.0, HighFiveMaxCharge 4.5, PrimaryComponentTick.bCanEverTick true; the rest 0 / false (state.rs
//! `FinnMomentum`).
//!
//! The data: AnimNotify_Finn_Momentum on Finn_Base_Attack (Max Momentum 0.8, DamageMult 1.8, KnockBackMult 1.3),
//! Finn_Combo_01_Attack (0.8, 2, 1), Finn_F_Attack (0.8, 2, 1.25), all Queued from 0.0001 to 0.85, and Finn_D_Attack
//! (0.9, 1, 1; BranchingPoint, -0.0001 to 0.89). AnimNotify_FinnMomentumMoveCommand on Finn_D_Attack (MoveVector
//! (1000, 0) to MaxMomentumMoveVector (1200, 0), BranchingPoint) and Finn_F_Attack ((100, 200) to (100, 550), Speed
//! Clamp on End, min (-50, 0), max (50, 500), Queued).
//! The order against the charge (settled by the data): each of the four montages has its own AnimNotify_ChargingAttack
//! (Max multipliers all 1, MontageSectionOveride Attack, no OnlyEndOnNotifyEnd), ending at 0.85: its begin and its
//! release (UpdateChargeValues, from the multipliers it set) come before EndMomentum, its end writes nothing. So
//! EndMomentum's values are the component's from 0.85 on, and the hits after it read them through GetChargeValue
//! (Comp_Pawn_ChargeAttack @99-263: the stored values while AttackMontage, the montage's own, is the active one). Their
//! reset is the shared rule (`fighter::charge`).

use super::state::{finn, finn_of, finn_w};
use crate::Real;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::anim::notify_data;
use crate::fighter::moves::{MoveCtl, MoveEnd, V2, add_move_cmd, evade_move_vector, evade_play_rate};
use crate::fighter::{FighterId, grounded};
use crate::math::ue::size2d;
use crate::math::{max, min};
use crate::registry::notifies::move_command_end;
use crate::registry::props::{flag, flag_or, num, num_or, obj};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Props;

/// FMath::Lerp(A, B, Alpha) = A + Alpha x (B - A), in float32.
fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + (t * (b - a))
}

/// FClamp(NormalizeToRange(Stored, 0, Max), 0, 1) (@274-372, again @399 / @668 / @937): NormalizeToRange with Min == Max
/// gives Value < Min ? 0 : 1.
#[allow(clippy::manual_clamp, reason = "FClamp as written: a NaN passes through")]
fn fraction(stored: Real, max: Real) -> Real {
    let v = if max == 0.0 { if stored < 0.0 { 0.0 } else { 1.0 } } else { stored / max };
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// UpdateChargeUI(Amount) -> UI_FinnCharge UpdateRunChargeProgressBar (@102: the bar takes the value only when it is
/// above 0; HandleBarSwitch(true) when the high-five meter is the one shown): `FinnMomentum::meter` keeps the last value
/// the bar took, which the view reads. UI only. UpdateHighFiveUI has no gate: the view reads HighFivePercent itself.
fn update_charge_ui(w: &mut World, f: FighterId, amount: Real) {
    if amount > 0.0 {
        finn_w(w, f).momentum.meter = amount;
    }
}

/// StartMomentum(MaxMomentum, DamageMult, KnockBackMult) @193: MaxStoredMomentum, then @62: StoredMomentum 0,
/// MomentumAmount 0, DamageMult, KnockBackMult, CalculateMomentum true, broadcast NewMomentumStarted (@173: Pawn_Finn's
/// TurnOnRunUi @8829 -> UpdateRunUI(true) -> UI_FinnCharge HandleBarSwitch @7465-7508: `FinnState::run_ui`).
// Deviation: Pawn_Finn binds NewMomentumStarted / MomentumEnd in BeginPlay only for a locally controlled player's pawn
// (@15-543, with the widget, retried by a Delay 0.1 @544 -> 599); here every Finn sets run_ui (a bot's too). UI only:
// the view shows the meter for the local player's Finn alone.
pub fn start_momentum(w: &mut World, f: FighterId, max: Real, dmg: Real, kb: Real) {
    let s = finn_w(w, f);
    let m = &mut s.momentum;
    m.max = max;
    m.stored = 0.0;
    m.amount = 0.0;
    m.dmg_mult = dmg;
    m.kb_mult = kb;
    m.calculating = true;
    s.run_ui = true; // NewMomentumStarted -> TurnOnRunUi
}

/// EndMomentum @225: broadcast MomentumEnd (TurnOffRunUI @8845), CalculateMomentum false, UpdateChargeUI(-1) (the bar
/// keeps its value), MomentumAmount = the fraction @372; the charge component's DamageMultiplier = Lerp(1, DamageMult,
/// fraction) @619, KnockBackMultiplier = Lerp(1, KnockBackMult, fraction) @888, PercentCharged = fraction @1106.
pub fn end_momentum(w: &mut World, f: FighterId) {
    let s = finn_w(w, f);
    s.run_ui = false; // MomentumEnd -> TurnOffRunUI
    s.momentum.calculating = false;
    update_charge_ui(w, f, -1.0);
    let m = &finn(w, f).momentum;
    let a = fraction(m.stored, m.max);
    let (dm, km) = (m.dmg_mult, m.kb_mult);
    finn_w(w, f).momentum.amount = a;
    let c = &mut w.fighter_mut(f).charge;
    c.charge_dmg_mult = lerp(1.0, dm, a);
    c.charge_mult = lerp(1.0, km, a);
    c.charge_pct = a;
}

/// The component's ReceiveTick(DeltaSeconds) @2338 -> @1156 (the owner's CharacterMovement):
///   CalculateMomentum and not StoredMomentum >= Max and VSize(Velocity) > 0: StoredMomentum = FMin(dt + Stored, Max),
///   UpdateChargeUI(Max == 0 ? -1 : FClamp(Stored / Max, 0.01, 0.99)) @1747;
///   then always (@1771): IsMovingOnGround and VSize(Velocity) > 50: HighFiveCharge = FMin(dt + HighFiveCharge,
///   HighFiveMaxCharge), HighFivePercent = HighFiveCharge / HighFiveMaxCharge, UpdateHighFiveUI @2221.
/// dt: the frame's DeltaSeconds times the hit pause's dilation (CharHooks.component_tick), float32.
// Unverified: the component's tick is placed in the port's component_tick (the fighter's during-physics tick); the
// build's tick group and its order against the CharacterMovement's tick are not read: the velocity is the one of that
// moment.
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test: a NaN stored momentum goes on as there")]
pub fn momentum_tick(w: &mut World, f: FighterId, dilation: Real) {
    let dt = DELTA_SECONDS * dilation;
    let (vx, vy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    let m = &finn(w, f).momentum;
    if m.calculating && !(m.stored >= m.max) && (vx != 0.0 || vy != 0.0) {
        let (stored, mx) = (min(dt + m.stored, m.max), m.max);
        finn_w(w, f).momentum.stored = stored;
        update_charge_ui(w, f, if mx == 0.0 { -1.0 } else { min(0.99, max(0.01, stored / mx)) });
    }
    // IsMovingOnGround: the sim's grounded (Walking).
    if grounded(w, f) && size2d([vx, vy]) > 50.0 {
        let m = &mut finn_w(w, f).momentum;
        m.high_five = min(dt + m.high_five, m.high_five_max);
        m.high_five_pct = m.high_five / m.high_five_max;
    }
}

/// ResetHighFive @2343: HighFiveCharge 0, then @15: HighFivePercent 0, UpdateHighFiveUI.
pub fn reset_high_five(w: &mut World, f: FighterId) {
    let m = &mut finn_w(w, f).momentum;
    m.high_five = 0.0;
    m.high_five_pct = 0.0;
}

/// OnFighterReset @2245 (BPI_Cleanup @7797): HighFiveCharge, StoredMomentum, HighFivePercent 0, UpdateHighFiveUI. The
/// rest (MaxStoredMomentum, MomentumAmount, the multipliers, CalculateMomentum) is left as it is.
pub fn on_fighter_reset(w: &mut World, f: FighterId) {
    let m = &mut finn_w(w, f).momentum;
    m.high_five = 0.0;
    m.stored = 0.0;
    m.high_five_pct = 0.0;
}

/// AnimNotify_Finn_Momentum: Received_NotifyBegin StartMomentum(Max Momentum, DamageMult, KnockBackMult) on the owner's
/// Comp_FinnMomentumCharge; Received_NotifyEnd EndMomentum. Class defaults (Default__AnimNotify_Finn_Momentum_C):
/// DamageMult 1, KnockBackMult 1, Max Momentum 0.
/// The owner without the component (Creature's disguise playing Finn's montage): GetComponentByClass gives None and the
/// call on it does nothing.
// Deviation: a Queued end after a late stop is not held for the next dispatch (the fighter's deferred ends do not list
// this class): it runs at the stop, as a BranchingPoint end does.
fn momentum_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if finn_of(w, f).is_none() {
        return;
    }
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    start_momentum(w, f, num_or(p, "Max Momentum", 0.0), num_or(p, "DamageMult", 1.0), num_or(p, "KnockBackMult", 1.0));
}

fn momentum_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if finn_of(w, f).is_some() {
        end_momentum(w, f);
    }
}

/// `p[k] ?? ZERO` of a vector member; a member without `X` or `Y` reads NaN there, as JavaScript's `undefined`.
fn v2(p: &Props, k: &str) -> V2 {
    match obj(p, k) {
        None => V2 { x: 0.0, y: 0.0 },
        Some(o) => V2 { x: num(o, "X").unwrap_or(Real::NAN), y: num(o, "Y").unwrap_or(Real::NAN) },
    }
}

/// AnimNotify_FinnMomentumMoveCommand Received_NotifyBegin (owner cast to BPI_BaseFighter @42, the move command manager
/// valid @316): BPI_GetEvadeData @121 (the play rate: `evade_play_rate`), a = FClamp(MomentumAmount, 0, 1) @423, the
/// vector Lerp(MoveVector, MaxMomentumMoveVector, a) per axis @714 / @769, ApplyEvadeBuffsToMoveVector on it @889
/// (every run: it clears ApplyCancelEvadeBuffs), then AddMoveCommandNotify @962: IsEvade ? TotalDuration /
/// EvadeAnimPlayRate : TotalDuration, IsEvade ? the buffed vector : the lerped one, XYOverride, ZOverride, the Control*
/// values, StopOnEnd, BreakAttach, WallSkate, ControlSpeedFriction, DisablePredictiveHitboxes, Speed Clamp on End / Min /
/// Max. Unlike AnimNotify_MoveCommand: no OnlyPerformOnGround, no AdjustStickInfluence, no BlockDownMovement (false).
/// BreakAttach and DisablePredictiveHitboxes are taken as AnimNotify_MoveCommand's handler takes them (not read).
/// Class defaults (Default__AnimNotify_FinnMomentumMoveCommand_C): XYOverride, ZOverride, StopOnEnd, BreakAttach true;
/// the vectors (0, 0); the rest 0 / false.
/// Received_NotifyEnd: CompleteMoveCommand(self) on the manager (a Queued end after a late stop deferred as
/// AnimNotify_MoveCommand's). Received_NotifyTick: the corner check only with CornerCheck, which the class default and
/// both notifies of the data leave false: nothing.
// Unverified: the clamp members are read as 'Speed Clamp on End' / 'Speed Clamp Min' / 'Speed Clamp Max', the
// build's
// property names; the notify data (AnimNotify_MoveCommand's are ClampSpeedOnEnd / SpeedClampMin / SpeedClampMax) may name
// them differently, in which case Finn_F_Attack's clamp is read as off.
fn move_command_begin(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let nt = notify_data(w, &data, f, n);
    let p = &nt.p;
    // in [0, 1] already (EndMomentum's FClamp); an owner without the component (the disguise) reads None's: 0
    let a = finn_of(w, f).map_or(0.0, |s| s.momentum.amount);
    let (lo, hi) = (v2(p, "MoveVector"), v2(p, "MaxMomentumMoveVector"));
    let mv = V2 { x: lerp(lo.x, hi.x, a), y: lerp(lo.y, hi.y, a) };
    let bv = evade_move_vector(w, f, mv);
    let is_evade = flag(p, "IsEvade");
    let er = if is_evade { evade_play_rate(w, f) } else { 1.0 };
    let total = if er == 1.0 { nt.dur } else { f64::from(crate::math::narrow(nt.dur / f64::from(er))) };
    let end = MoveEnd {
        stop: flag_or(p, "StopOnEnd", true),
        clamp: flag_or(p, "Speed Clamp on End", false),
        min: v2(p, "Speed Clamp Min"),
        max: v2(p, "Speed Clamp Max"),
    };
    let ctl = MoveCtl {
        speed: num_or(p, "ControlSpeed", 0.0),
        max: num_or(p, "ControlAngleMax", 0.0),
        min: num_or(p, "ControlAngleMin", 0.0),
        friction: num_or(p, "ControlSpeedFriction", 0.0),
        decay: num_or(p, "ControlSpeedDecay", 0.0),
    };
    let (xy, z) = (flag_or(p, "XYOverride", true), flag_or(p, "ZOverride", true));
    let skate = flag_or(p, "WallSkate", false);
    add_move_cmd(
        w,
        f,
        idx,
        crate::math::narrow(total),
        if is_evade { bv } else { mv },
        xy,
        z,
        &end,
        false,
        skate,
        &ctl,
    );
}

/// This file's registry entries (AnimNotify_Finn_Momentum_C, AnimNotify_FinnMomentumMoveCommand_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_Finn_Momentum_C",
        NotifyHandler { begin: Some(momentum_begin), end: Some(momentum_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_FinnMomentumMoveCommand_C",
        NotifyHandler { begin: Some(move_command_begin), end: Some(move_command_end), ..NotifyHandler::default() },
    )?;
    Ok(())
}
