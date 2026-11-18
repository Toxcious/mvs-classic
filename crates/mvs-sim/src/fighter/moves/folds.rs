//! Comp_State_Manager CalculateBuffValues' jump, evade, misc movement and default speed folds and their Apply
//! functions, with BuffParentActor's per-buff getters (GetJumpMod, GetEvadeMod, GetMiscMovementMods: with CanStack
//! each field is base + CurrentStackAmount x per stack).

use super::*;
use crate::combat::buffs::{inst_def_of, mod_type, move_mod, normalize_multiplier_floats};
use crate::consts::{
    AIR_CONTROL, AIR_CONTROL_BOOST_THRESHOLD, EVADE_BASE_INVULN, EVADE_RECHARGE_TIME, JUMP_STOP_MULT, MAX_ACCEL_AIR,
};
use crate::math::to_int32;

/// `Folds`: the values off their base (`None` at the base; the whole `None` while every value
/// is). The values belong to the pawn: a respawn keeps them, World.reset drops them. AirControlBoostVelocityThreshold
/// grows with each UpdateAirControl under a multiplier other than 1 and keeps what it grew to until World.reset.
#[derive(State, Debug, Default)]
pub struct Folds {
    /// MaxJumpNumber.
    pub max_jumps: Option<Real>,
    /// MaxJumpHeight.
    pub jump_h: Option<Real>,
    /// MaxDoubleJumpHeight.
    pub djump_h: Option<Real>,
    /// MaxWallJumpHeight.
    pub wjump_h: Option<Real>,
    /// JumpVelocity.
    pub jump_vel: Option<Real>,
    /// JumpStopVelocityMultiplier.
    pub jump_stop: Option<Real>,
    /// VelocityFromFastFall.
    pub ff_speed: Option<Real>,
    /// EvadeAnimMontagePlayRateModifier.
    pub evade_rate: Option<Real>,
    /// BaseEvadeInvulnerability.
    pub evade_invuln: Option<Real>,
    /// EvadeInvulnerabilityRechargeTime (see [`evade_recharge`]).
    pub evade_recharge: Option<Real>,
    /// BounceMultiplier.
    pub bounce: Option<Real>,
    /// AirControlMultiplier.
    pub air_ctl: Option<Real>,
    /// DirectionalInfluenceMultiplier.
    pub di: Option<Real>,
    /// FallingAccelerationMultiplier.
    pub fall_accel: Option<Real>,
    /// Comp_State_Manager DefaultMovementSpeed (CDO 1.0; read by the haste visuals alone).
    pub default_speed: Option<Real>,
    /// CMC AirControlBoostVelocityThreshold (CDO 25).
    pub boost_thr: Option<Real>,
}

impl Folds {
    fn is_empty(&self) -> bool {
        [
            self.max_jumps,
            self.jump_h,
            self.djump_h,
            self.wjump_h,
            self.jump_vel,
            self.jump_stop,
            self.ff_speed,
            self.evade_rate,
            self.evade_invuln,
            self.evade_recharge,
            self.bounce,
            self.air_ctl,
            self.di,
            self.fall_accel,
            self.default_speed,
            self.boost_thr,
        ]
        .iter()
        .all(Option::is_none)
    }
}

/// A field of [`Folds`].
type Key = fn(&mut Folds) -> &mut Option<Real>;

fn near1(v: Real) -> bool {
    (v - 1.0).abs() <= 1e-6
}

/// A fold of the fighter's (`None`: at its base).
fn fold(w: &World, f: FighterId, k: fn(&Folds) -> Option<Real>) -> Option<Real> {
    w.fighter(f).moves.folds.as_ref().and_then(k)
}

/// The pawn's class default of a key (`f.data.pawn[k] ?? f.data.base[k]`), a number.
fn cdo(w: &World, f: FighterId, k: &str) -> Option<Real> {
    let d = &w.data.chars[w.names.str(w.fighter(f).ch)].fighter;
    d.pawn.get(k).or_else(|| d.base.get(k)).and_then(|v| v.as_f64()).map(crate::math::narrow)
}

// ---- readers: the fighter's current value

/// `maxJumps`.
pub fn max_jumps(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.max_jumps).unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).max_jumps))
}

/// `jumpHeight`.
pub fn jump_height(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.jump_h).unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).max_jump_height))
}

/// `doubleJumpHeight`.
pub fn double_jump_height(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.djump_h)
        .unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).max_double_jump_height))
}

/// `wallJumpHeight`.
pub fn wall_jump_height(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.wjump_h)
        .unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).max_wall_jump_height))
}

/// `jumpVelocity`.
pub fn jump_velocity(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.jump_vel).unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).jump_velocity))
}

/// `jumpStopMult`.
pub fn jump_stop_mult(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.jump_stop).unwrap_or(JUMP_STOP_MULT)
}

/// `fastFallSpeed`.
pub fn fast_fall_speed(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.ff_speed).unwrap_or_else(|| crate::math::narrow(crate::fighter::body::def(w, f).fast_fall_speed))
}

/// `evadePlayRate`: GetEvadeAnimMontagePlayRate (BaseFighter @0) = AnimMontagePlayRate (1 on every pawn) x
/// EvadeAnimMontagePlayRateModifier.
pub fn evade_play_rate(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.evade_rate).unwrap_or(1.0)
}

/// `evadeBaseInvuln`.
pub fn evade_base_invuln(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.evade_invuln).unwrap_or(EVADE_BASE_INVULN)
}

/// `evadeRecharge`: EvadeInvulnerabilityRechargeTime: the fold's, else the crouch's (Pawn_C019) or the pawn's base.
pub fn evade_recharge(w: &World, f: FighterId) -> Real {
    if let Some(v) = fold(w, f, |s| s.evade_recharge) {
        return v;
    }
    if w.fighter(f).evade.evade_crouch_rate {
        return crate::math::narrow(
            crate::fighter::body::def(w, f).crouch_evade_recharge.expect("a crouch evade rate without its value"),
        );
    }
    evade_recharge_base(w, f)
}

fn evade_recharge_base(w: &World, f: FighterId) -> Real {
    let d = &w.data.chars[w.names.str(w.fighter(f).ch)].fighter;
    crate::math::narrow(
        d.pawn
            .get("EvadeInvulnerabilityRechargeTime")
            .and_then(|v| v.as_f64())
            .unwrap_or(f64::from(EVADE_RECHARGE_TIME)),
    )
}

/// `bounceMult`.
pub fn bounce_mult(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.bounce).unwrap_or(1.0)
}

/// `airControlMult`.
pub fn air_control_mult(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.air_ctl).unwrap_or(1.0)
}

/// `diMult`.
pub fn di_mult(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.di).unwrap_or(1.0)
}

/// `maxAccelAir`: FMC MaxAccelerationAir (GetMaxAcceleration while Falling).
pub fn max_accel_air(w: &World, f: FighterId) -> Real {
    match fold(w, f, |s| s.fall_accel) {
        None => MAX_ACCEL_AIR,
        Some(a) => a * MAX_ACCEL_AIR,
    }
}

/// `airBoostThreshold`.
pub fn air_boost_threshold(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.boost_thr).unwrap_or(AIR_CONTROL_BOOST_THRESHOLD)
}

/// `defaultMovementSpeed`.
pub fn default_movement_speed(w: &World, f: FighterId) -> Real {
    fold(w, f, |s| s.default_speed).unwrap_or(1.0)
}

/// `put`: a value at its base drops the key (and the folds with their last key).
fn put(w: &mut World, f: FighterId, k: Key, v: Real, base: Real) {
    let m = &mut w.fighter_mut(f).moves;
    if v == base {
        if let Some(s) = &mut m.folds {
            *k(s) = None;
            if s.is_empty() {
                m.folds = None;
            }
        }
        return;
    }
    *k(m.folds.get_or_insert_with(Folds::default)) = Some(v);
}

/// `growBoostThreshold`: UpdateAirControl's threshold step: AirControlBoostVelocityThreshold = AirControlMultiplier x
/// its current value.
pub fn grow_boost_threshold(w: &mut World, f: FighterId, mult: Real) {
    if mult == 1.0 {
        return;
    }
    let v = mult * air_boost_threshold(w, f);
    put(w, f, |s| &mut s.boost_thr, v, AIR_CONTROL_BOOST_THRESHOLD);
}

/// `foldsReset`: World.reset, a new pawn's class defaults.
pub fn folds_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.folds = None;
}

/// `crouchEvadeRate`'s fold clear: `delete f.folds.evadeRecharge`, and `f.folds = undefined`
/// once it holds nothing.
pub fn clear_evade_recharge_fold(w: &mut World, f: FighterId) {
    let m = &mut w.fighter_mut(f).moves;
    if let Some(s) = m.folds.as_mut() {
        s.evade_recharge = None;
        if s.is_empty() {
            m.folds = None;
        }
    }
}

// ---- the per-buff getters (BuffParentActor)

/// A buff the folds read: its class and stack count.
#[derive(Clone, Copy)]
struct B {
    stacks: Real,
    serial: u32,
}

/// `stackedMods`: GetJumpMod @14-1328 / GetEvadeMod @14-1219 / GetMiscMovementMods @14-618 / GetMiscCombatMods
/// @14-781: one numeric field, base + CurrentStackAmount x per stack with CanStack; a field the class's data leaves
/// out is the parent's default (multipliers 1, the rest 0).
fn stacked_mod(w: &World, f: FighterId, b: B, key: &str, per_key: &str, k: &str) -> Real {
    let d = inst_def_of(w, BuffRef { fighter: f, serial: b.serial });
    let d = d.as_deref();
    let get =
        |o: &str| d.and_then(|d| d.get(o)).and_then(|v| v.as_obj()).and_then(|o| o.get(k)).and_then(|v| v.as_f64());
    let v0 = get(key).unwrap_or(if k.starts_with("Mult") { 1.0 } else { 0.0 });
    let stack = d
        .and_then(|d| d.get("CanStack"))
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_f64().is_some_and(|n| n != 0.0));
    if !stack {
        return crate::math::narrow(v0);
    }
    let p = get(per_key).unwrap_or(0.0);
    if k == "JumpCountMod" || k == "FlatArmorMaxHitsMod" {
        return crate::real(crate::math::dbl::to_int32(v0 + f64::from(b.stacks * crate::math::narrow(p))));
    }
    crate::math::narrow(v0 + f64::from(b.stacks * crate::math::narrow(p)))
}

fn jump_mod(w: &World, f: FighterId, b: B, k: &str) -> Real {
    stacked_mod(w, f, b, "JumpMods", "JumpModsPerkStack", k)
}

fn evade_mod(w: &World, f: FighterId, b: B, k: &str) -> Real {
    stacked_mod(w, f, b, "EvadeMods", "EvadeModsPerStack", k)
}

fn misc_movement_mod(w: &World, f: FighterId, b: B, k: &str) -> Real {
    stacked_mod(w, f, b, "MiscMovementMods", "MiscMovementModsPerStack", k)
}

fn misc_combat_mod(w: &World, f: FighterId, b: B, k: &str) -> Real {
    stacked_mod(w, f, b, "MiscCombatMods", "MiscCombatModsPerStack", k)
}

/// `eachBuff`: the buffs in CalculateBuffValues' order: the new one first, then the owner's live buffs.
// A new buff its own begin already removed is left out, as in `calc_order` (combat/buffs.rs).
fn each_buff(w: &World, f: FighterId, first: Option<&BuffRef>) -> Vec<B> {
    let bs = &w.fighter(f).buffs.buffs;
    let mut out = Vec::new();
    let first = first.map(|r| r.serial);
    if let Some(b) = first.and_then(|s| bs.iter().find(|b| b.serial == s)) {
        out.push(B { stacks: b.stacks, serial: b.serial });
    }
    for b in bs {
        if Some(b.serial) != first && b.frames > 0.0 {
            out.push(B { stacks: b.stacks, serial: b.serial });
        }
    }
    out
}

/// `eachLive`: the live buffs in the Children map's order.
fn each_live(w: &World, f: FighterId) -> Vec<B> {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .filter(|b| b.frames > 0.0)
        .map(|b| B { stacks: b.stacks, serial: b.serial })
        .collect()
}

/// The multipliers join by NormalizeMultiplierFloats unless NearlyEqual(v, 1, 1e-6).
fn mul_join(acc: Real, v: Real) -> Real {
    if near1(v) { acc } else { normalize_multiplier_floats(acc, v) }
}

/// The flats join only when > 0.
fn flat_join(acc: Real, v: Real) -> Real {
    if v > 0.0 { acc + v } else { acc }
}

/// `build`: base x mult (unless ~1) + flat (when > 0), float32.
fn build(base: Real, mult: Real, flat: Real) -> Real {
    let mut v = base;
    if !near1(mult) {
        v = base * mult;
    }
    if flat > 0.0 {
        v += flat;
    }
    v
}

/// The entry's data's flag `k` is set (truthy): `instDef(f, b)`, its own values over its class's.
fn def_flag(w: &World, f: FighterId, serial: u32, k: &str) -> bool {
    inst_def_of(w, BuffRef { fighter: f, serial }).as_deref().and_then(|d| d.get(k)).is_some_and(|v| match v {
        mvs_data::value::Value::Bool(b) => *b,
        mvs_data::value::Value::Num(n) => *n != 0.0,
        mvs_data::value::Value::Null => false,
        _ => true,
    })
}

/// The class data's `EvadeMods.ApplyOnlyAfterHitCancel` is set.
fn hit_cancel_only(w: &World, f: FighterId, serial: u32) -> bool {
    inst_def_of(w, BuffRef { fighter: f, serial })
        .as_deref()
        .and_then(|d| d.get("EvadeMods"))
        .and_then(|v| v.as_obj())
        .and_then(|o| o.get("ApplyOnlyAfterHitCancel"))
        .and_then(|v| v.as_bool())
        == Some(true)
}

/// `recalcMoveFolds`: CalculateBuffValues' default speed (@2134-2614, @11651-11714), jump (@2989-5321) and evade
/// (@5358-6911) parts, then SetDefaultSpeed, ApplyJumpBuffs and ApplyEvadeBuffs.
// Deviation: the buff order is the port's (appended); the build's Children map reuses freed slots LIFO, so after a
// buff ends a later one can fold before an older one. Only the float32 rounding of the multiplier sums can differ.
pub fn recalc_move_folds(w: &mut World, f: FighterId, first: Option<&BuffRef>) {
    let mut dflt: Real = 1.0;
    let (mut cnt, mut fh, mut mh, mut fs, mut ms, mut fst, mut mst, mut fff, mut mff) =
        (0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0);
    let (mut fes, mut mes, mut fei, mut mei, mut fer, mut mer) = (0.0, 1.0, 0.0, 1.0, 0.0, 1.0);
    for b in each_buff(w, f, first) {
        if def_flag(w, f, b.serial, "AdjustsDefaultSpeed") {
            // The buff's GetMoveMod by its SpeedModType: Multiply, Add, Override, DoNothing.
            let d = inst_def_of(w, BuffRef { fighter: f, serial: b.serial })
                .expect("a buff with AdjustsDefaultSpeed has its data");
            let d = &*d;
            let t = mod_type(d.get("SpeedModType"));
            let mm = move_mod(d, to_int32(b.stacks));
            if t == 1.0 {
                dflt *= mm;
            } else if t == 2.0 {
                dflt += mm;
            } else if t == 3.0 {
                dflt = mm;
            }
        }
        let j = |k: &str| jump_mod(w, f, b, k);
        if j("JumpCountMod") != 0.0 {
            cnt += j("JumpCountMod");
        }
        fh = flat_join(fh, j("FlatJumpHeightMod"));
        fs = flat_join(fs, j("FlatJumpSpeedMod"));
        fst = flat_join(fst, j("FlatJumpStopVelocityMod"));
        fff = flat_join(fff, j("FlatFastFallMod"));
        mh = mul_join(mh, j("MultJumpHeightMod"));
        ms = mul_join(ms, j("MultJumpSpeedMod"));
        mst = mul_join(mst, j("MultJumpStopVelocityMod"));
        mff = mul_join(mff, j("MultFastFallMod"));
        let e = |k: &str| evade_mod(w, f, b, k);
        fes = flat_join(fes, e("FlatEvadeSpeedMod"));
        fei = flat_join(fei, e("FlatEvadeBaseInvulnerabilityMod"));
        fer = flat_join(fer, e("FlatEvadeRegenerationTimeMod"));
        mes = mul_join(mes, e("MultEvadeSpeedMod"));
        mei = mul_join(mei, e("MultEvadeBaseInvulnerabilityMod"));
        mer = mul_join(mer, e("MultEvadeRegenerationTimeMod"));
    }
    // SetDefaultSpeed: only a value other than DefaultMovementSpeed is written.
    if dflt != default_movement_speed(w, f) {
        put(w, f, |s| &mut s.default_speed, dflt, 1.0);
    }
    // ApplyJumpBuffs: the multipliers, then MaxJumpNumber + JumpCountMod (always), then the flats.
    let ch = &w.data.chars[w.names.str(w.fighter(f).base_ch)].def;
    let (jh, djh, wjh, jv, ffs, mj) = (
        ch.max_jump_height,
        ch.max_double_jump_height,
        ch.max_wall_jump_height,
        ch.jump_velocity,
        ch.fast_fall_speed,
        ch.max_jumps,
    );
    put(w, f, |s| &mut s.jump_h, build(crate::math::narrow(jh), mh, fh), crate::math::narrow(jh));
    put(w, f, |s| &mut s.djump_h, build(crate::math::narrow(djh), mh, fh), crate::math::narrow(djh));
    put(w, f, |s| &mut s.wjump_h, build(crate::math::narrow(wjh), mh, fh), crate::math::narrow(wjh));
    put(w, f, |s| &mut s.jump_vel, build(crate::math::narrow(jv), ms, fs), crate::math::narrow(jv));
    put(w, f, |s| &mut s.jump_stop, build(JUMP_STOP_MULT, mst, fst), JUMP_STOP_MULT);
    put(w, f, |s| &mut s.ff_speed, build(crate::math::narrow(ffs), mff, fff), crate::math::narrow(ffs));
    put(
        w,
        f,
        |s| &mut s.max_jumps,
        crate::real(crate::math::dbl::to_int32(mj + f64::from(cnt))),
        crate::math::narrow(mj),
    );
    // ApplyEvadeBuffs: base x mult + flat.
    let rate = build(cdo(w, f, "EvadeAnimMontagePlayRateModifier").unwrap_or(1.0), mes, fes);
    let inv = build(EVADE_BASE_INVULN, mei, fei);
    let rbase = evade_recharge_base(w, f);
    let rech = build(rbase, mer, fer);
    if rate == evade_play_rate(w, f) && inv == evade_base_invuln(w, f) && rech == evade_recharge(w, f) {
        return;
    }
    put(w, f, |s| &mut s.evade_rate, rate, 1.0);
    put(w, f, |s| &mut s.evade_invuln, inv, EVADE_BASE_INVULN);
    // Morty crouched: the base written over the crouch's 0.5 is kept until K2_OnEndCrouch.
    if w.fighter(f).evade.evade_crouch_rate {
        w.fighter_mut(f).moves.folds.get_or_insert_with(Folds::default).evade_recharge = Some(rech);
    } else {
        put(w, f, |s| &mut s.evade_recharge, rech, rbase);
    }
    // ReplicateEvadeValues -> BPI_ResetEvadeRegenTimer.
    // Unverified: the new timer's first period is counted from the next frame.
    let fi = w.fighter_mut(f);
    fi.evade.evade_regen_start = crate::real(fi.clock.tick) + 1.0;
}

fn mul1(base: Real, m: Real) -> Real {
    if m == 1.0 { base } else { base * m }
}

/// `recalcMiscMovement`: CalculateBuffValues' misc movement part (@7226-8363), then ApplyMiscMovementBuffs, its
/// setter and BPI_ResetAirControl.
pub fn recalc_misc_movement(w: &mut World, f: FighterId, first: Option<&BuffRef>) {
    let (mut di, mut wb, mut ac, mut fa) = (1.0, 1.0, 1.0, 1.0);
    for b in each_buff(w, f, first) {
        let m = |k: &str| misc_movement_mod(w, f, b, k);
        di = mul_join(di, m("MultDirectionalInfluenceDegree"));
        wb = mul_join(wb, m("MultWallBounceVelocity"));
        ac = mul_join(ac, m("MultAirControlMod"));
        fa = mul_join(fa, m("MultFallingAcceleration"));
    }
    let nb = mul1(cdo(w, f, "BounceMultiplier").unwrap_or(1.0), wb);
    let nac = mul1(cdo(w, f, "AirControlMultiplier").unwrap_or(1.0), ac);
    let ndi = mul1(cdo(w, f, "DirectionalInfluenceMultiplier").unwrap_or(1.0), di);
    let nfa = mul1(cdo(w, f, "FallingAccelerationMultiplier").unwrap_or(1.0), fa);
    if nb == bounce_mult(w, f)
        && nac == air_control_mult(w, f)
        && ndi == di_mult(w, f)
        && nfa == fold(w, f, |s| s.fall_accel).unwrap_or(1.0)
    {
        return;
    }
    put(w, f, |s| &mut s.bounce, nb, 1.0);
    put(w, f, |s| &mut s.air_ctl, nac, 1.0);
    put(w, f, |s| &mut s.di, ndi, 1.0);
    put(w, f, |s| &mut s.fall_accel, nfa, 1.0);
    update_air_control(w, f, AIR_CONTROL); // BPI_ResetAirControl: UpdateAirControl(BaseAirControl)
}

// ---- read at use: evade distance, projectile speed, armor

/// A live buff whose EvadeMods.ApplyOnlyAfterHitCancel is set.
fn hit_cancel_evade_buff(w: &World, f: FighterId) -> bool {
    each_live(w, f).iter().any(|b| hit_cancel_only(w, f, b.serial))
}

/// `noteAttackCancelled`: CurrentAttackHitCondition Check @401: Comp_Pawn_BranchManager.PreviousAttackCancelled, kept
/// only while the fighter has a buff that reads it.
// Unverified: a check made before such a buff was spawned is not seen by a hit-cancel evade after the spawn; the
// blend-out clear is taken on every montage blend-out of the pawn.
pub fn note_attack_cancelled(w: &mut World, f: FighterId, v: bool) -> bool {
    let keep = v && hit_cancel_evade_buff(w, f);
    w.fighter_mut(f).moves.prev_atk_cancelled = keep;
    v
}

/// `clearAttackCancelled`.
pub fn clear_attack_cancelled(w: &mut World, f: FighterId) {
    w.fighter_mut(f).moves.prev_atk_cancelled = false;
}

/// `branchEvade`: Trigger Current Attack Client's evade path (@7405-7617): ApplyCancelEvadeBuffs =
/// PreviousAttackCancelled && not a Neutral Evade.
pub fn branch_evade(w: &mut World, f: FighterId, neutral: bool) {
    let m = &mut w.fighter_mut(f).moves;
    m.cancel_evade_buffs = m.prev_atk_cancelled && !neutral;
}

/// `evadeMoveVector`: ApplyEvadeBuffsToMoveVector (CSM @0-1676).
pub fn evade_move_vector(w: &mut World, f: FighterId, mv: V2) -> V2 {
    let local = w.fighter(f).moves.cancel_evade_buffs;
    w.fighter_mut(f).moves.cancel_evade_buffs = false;
    let (mut ax, mut ay, mut hit) = (0.0, 0.0, false);
    for b in each_live(w, f) {
        if hit_cancel_only(w, f, b.serial) && !local {
            continue;
        }
        let flat = evade_mod(w, f, b, "FlatEvadeDistanceMod");
        if flat > 0.0 {
            ax += flat;
            ay += flat;
            hit = true;
        }
        let m = evade_mod(w, f, b, "MultEvadeDistanceMod");
        if !near1(m) {
            ax += (mv.x * m) - mv.x;
            ay += (mv.y * m) - mv.y;
            hit = true;
        }
    }
    if hit { V2 { x: (mv.x + ax), y: (mv.y + ay) } } else { mv }
}

/// `projectileSpeedBuffed`: ApplyProjectileSpeedBuffsToInputs (CSM @0-1568).
pub fn projectile_speed_buffed(w: &mut World, f: FighterId, v: Real) -> Real {
    let (mut add, mut hit) = (0.0, false);
    for b in each_live(w, f) {
        let m = misc_combat_mod(w, f, b, "MultProjectileSpeedMod");
        if near1(m) {
            continue;
        }
        add += (v * m) - v;
        hit = true;
    }
    if hit { v + add } else { v }
}

/// `armorPackBuffed`: ApplyArmorBuffsToArmorPack (CSM @0-2309). MultKnockbackThresholdMod
/// SETS the threshold sum to the armor amount sum so far plus its own delta (@1483-1834, a build bug kept).
pub fn armor_pack_buffed(
    w: &World,
    f: FighterId,
    a: &crate::combat::defense::ArmorPack,
) -> crate::combat::defense::ArmorPack {
    let (mut amt, mut hits, mut thr, mut hit) = (0.0, 0.0, 0.0, false);
    for b in each_live(w, f) {
        let m = misc_combat_mod(w, f, b, "MultArmorAmountMod");
        if !near1(m) {
            amt += (m * a.amount) - a.amount;
            hit = true;
        }
        let h = misc_combat_mod(w, f, b, "FlatArmorMaxHitsMod");
        if h > 0.0 {
            hits = crate::real(to_int32(h + hits));
            hit = true;
        }
        let k = misc_combat_mod(w, f, b, "MultKnockbackThresholdMod");
        if !near1(k) {
            thr = amt + ((k * a.kb_res) - a.kb_res);
            hit = true;
        }
    }
    let mut out = a.clone();
    if !hit {
        return out;
    }
    out.amount = a.amount + amt;
    out.max_hits = crate::real(to_int32(a.max_hits + hits));
    out.threshold = a.threshold + thr;
    out
}
