//! The knockback of a hit: BaseFighter's Calculate Knock Back distance, the launch tiers, hitpause and hitstun
//! times, and the directional influence on the launch.

#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated tests are kept for their NaN result")]

use super::{HitParams, KnockResult};
use crate::Real;
use crate::consts::{
    DELTA_SECONDS, HIT_PAUSE_MULT, HITSTUN_CONV, HITSTUN_EXP, HITSTUN_FLOOR, KB_DOWN_MOD, KB_EXP_CURVE, KB_MULT,
    KB_SPEED_CAP, KB_UP_MOD,
};
use crate::math::ue;
use crate::math::{max, min};

/// Calculate Knock Back. `d` is the victim's damage after this hit.
pub fn knockback_distance(p: &HitParams, d: Real) -> Real {
    let start = p.scaling_start_dmg;
    let mut scaled = 0.0;
    if d >= start {
        let e = d - start;
        scaled = (e * e * KB_EXP_CURVE + 1.0)
            * ((p.damage / 10.0 + (p.damage * e) / 20.0) * 1.6 + 18.0)
            * p.knock_back_scaling;
    }
    let dist = ((scaled + p.base_knock_back) * 200.0) / (p.weight + 100.0) * p.charge_kb_mult;
    min(max(dist, 0.0), KB_SPEED_CAP)
}

/// Damage tiers on the launch velocity (path A branch). The tier check fits x1.20-1.225 at 175 (moderate
/// confidence).
pub fn damage_tier(d: Real) -> Real {
    if d >= 175.0 {
        1.25
    } else if d >= 150.0 {
        1.125
    } else if d >= 115.0 {
        1.075
    } else {
        1.0
    }
}

/// Hitpause scale on post-hit damage (Blueprint table).
pub fn hitpause_scale(d: Real) -> Real {
    if d <= 50.0 {
        1.0
    } else if d <= 90.0 {
        1.2
    } else if d <= 110.0 {
        1.3
    } else {
        1.5
    }
}

/// KBFrictionHitStun (the same function): MinHitStunClamp when KB > 0 and Mult <= 0, else HitStun.
pub fn knockback_friction_stun(kb: Real, mult: Real) -> Real {
    if kb > 0.0 && mult <= 0.0 { HITSTUN_FLOOR } else { knockback_to_hitstun(kb, mult) }
}

/// Comp_Pawn_HitstunHandler ConvertKnockBackToHitstun: HitStun = FClamp(KB <= 0 ? Mult : KB^FallOffExp x Conv x
/// Mult, Mult == 0 ? 0 : MinHitStunClamp, 99999).
pub fn knockback_to_hitstun(kb: Real, mult: Real) -> Real {
    let h = if kb <= 0.0 { mult } else { ue::powf(kb, HITSTUN_EXP) * HITSTUN_CONV * mult };
    min(max(h, if mult == 0.0 { 0.0 } else { HITSTUN_FLOOR }), 99999.0)
}

/// Ticks of the timer manager a timer of Rate `t` takes to fire once started (FTimerManager::Tick 142e45520): the
/// manager's clock adds the world's DeltaSeconds per tick and a timer fires on a strict InternalTime > ExpireTime,
/// so floor(Rate / dt) + 1 counted ticks, the Rate a float32. No time: no timer (infinity).
pub fn timer_ticks(t: Option<Real>) -> Real {
    let t = t.unwrap_or(0.0);
    if t == 0.0 || t.is_nan() {
        return Real::INFINITY;
    }
    let rate = t;
    // The loop walks k up from 1 to the first k with k x dt > rate. k x dt (f64) grows with k, and every k up to
    // floor(rate / dt) - 2 has k x dt at least about 2 dt below rate, far more than the rounding of the product or
    // of the quotient, so none of them passes: starting at floor(rate / dt) - 1 (at least 1, the start for a
    // rate below 2 dt or not above 0) finds the same k in at most three steps.
    // (k in doubles: in float32 a rate in the millions of seconds (Buff_ArmorPack's 9999999 s) has k above
    // 2^24, where `k += 1.0` changes nothing and the walk never ends)
    let dt = f64::from(DELTA_SECONDS);
    let mut k = crate::math::dbl::max((f64::from(rate) / dt).floor() - 1.0, 1.0);
    while !(k * dt > f64::from(rate)) {
        k += 1.0;
    }
    crate::math::narrow(k)
}

/// The same for the HitPauseTimer, the HitstunTimer and the True Hitstun Timer, all set by K2_SetTimerDelegate
/// with a float of seconds (Comp_HitPause SetHitPauseTime, HitstunHandler @900 / @1630-1812), 0 for no timer.
pub fn timer_count(sec: Real) -> i32 {
    if sec > 0.0 { ticks_i32(timer_ticks(Some(sec))) } else { 0 }
}

/// A timer's tick count as an integer (kept as a number; a finite count fits).
fn ticks_i32(k: Real) -> i32 {
    crate::math::to_int32(k)
}

/// The True Hitstun Timer of a knockback distance (AddDecayedHitStun @1462-1812: KBFrictionHitStun through
/// ApplyOutsideHitstunClamp, whose clamp is 0 = none, with no HitStunDecayMod): the timer ticks it takes and its
/// Rate in float seconds.
pub fn kb_stun_times(dist: Real, mult: Real) -> KbStunTimesOut {
    let sec = knockback_friction_stun(dist, mult);
    KbStunTimesOut { kb_stun_frames: crate::real(timer_count(sec)), kb_stun_sec: sec }
}

/// What `kbStunTimes` gives.
#[derive(Clone, Debug)]
pub struct KbStunTimesOut {
    pub kb_stun_frames: Real,
    pub kb_stun_sec: Real,
}

/// Calculate Knock Back's direction (@2205-2330, @4835-4960): Normal2D(LocalKnockbackDirection) (no `== 1` shortcut),
/// as (0, X, Y) through Normal(.., 0.0001). The launch is it x (LocalTotalDistance x KnockbackMultiplier).
pub fn knockback_dir(x: Real, y: Real) -> (Real, Real) {
    let [nx, ny] = ue::safe_normal2d([x, y]);
    let n = ue::normal([0.0, nx, ny], ue::KINDA_SMALL_NUMBER);
    (n[1], n[2])
}

/// `computeKnock`.
pub fn compute_knock(p: &HitParams, d: Real) -> KnockResult {
    let dist = knockback_distance(p, d);
    let speed = dist * KB_MULT;
    // AnimNotify_Hitframe HitframeHitboxesOn: HitPauseTime + TotalChargedTime x ChargeHitPauseMult (seconds).
    // DamageAndLaunch scales it by the post-hit damage table and the victim's HitPauseMultiplier.
    let hit_pause_time = p.hit_pause_time + p.total_charged_time * p.charge_hit_pause_mult;
    let hp = hit_pause_time * hitpause_scale(d) * HIT_PAUSE_MULT;
    // AddDecayedHitStun: HitStun x HitStunDecayMod, no OutsideHitstunClamp (0) and no escape buffer (CanUseEscape
    // is false in the CDO and never set). hitstun_frames here is undecayed; the hit applies the HitStunDecayMod
    // (combo decay) to the HitstunTimer only.
    let hitstun = knockback_to_hitstun(dist, p.hit_stun_mult);
    let kb = kb_stun_times(dist, p.hit_stun_mult);
    KnockResult {
        dist,
        speed,
        tier: damage_tier(d),
        hit_pause_time,
        hitpause_frames: timer_count(hp),
        hitpause_sec: hp,
        hitstun,
        hitstun_frames: timer_count(hitstun),
        kb_stun_frames: ticks_i32(kb.kb_stun_frames),
        kb_stun_sec: kb.kb_stun_sec,
    }
}

/// ApplyKnockback at the end of hitpause: upward launches x1.07, downward x0.875 (before DI).
pub fn vertical_launch_mod(vy: Real) -> Real {
    vy * if vy >= 0.0 { KB_UP_MOD } else { KB_DOWN_MOD }
}

/// UE knockback / stick angle in degrees: up 360, sim right 270, left 90, down 180. CalculateDI @475 / @734
/// DegAtan2 (FMath::Atan2 x 180 / pi in one multiply), + 180 @521 / @780.
pub fn di_angle(x: Real, y: Real) -> Real {
    ue::deg_atan2(x + 0.0, -y) + 180.0
}

/// Comp_Pawn_DamageManager cumulative DI: 15 (near-vertical launches) or 10.5 degrees, growing with the combo
/// counter from the third hit (4 steps up to 25.5). CalculateCumulativeDI @689: the base degree x the victim's
/// DirectionalInfluenceMultiplier (the misc movement fold; 1 without a fold buff).
pub fn cumulative_di(kb: Real, combo: Real, mult: Real) -> Real {
    let vert = kb <= 15.0 || kb >= 345.0;
    let base = if mult == 1.0 { if vert { 15.0 } else { 10.5 } } else { mult * if vert { 15.0 } else { 10.5 } };
    let max_a: Real = if vert { 10.5 } else { 15.0 };
    let steps = min(max(combo - 2.0, 0.0), 4.0);
    base + if steps != 0.0 && !steps.is_nan() { ue::powf(ue::powf(max_a, 1.0 / 4.0), steps) } else { 0.0 }
}

/// DamageManager CalculateDI: the launch turns by a fixed DI angle (x KBI) toward the stick; speed is unchanged.
/// `grounded` = IsWalking at the end of hitpause. Grounded hits only steer when both the launch and the stick are
/// in [75, 295] (not up); airborne hits keep a near-straight-down launch unless it is a wall bounce.
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn calculate_di(
    vx: Real,
    vy: Real,
    sx: Real,
    sy: Real,
    grounded: bool,
    wall_bounce: bool,
    kbi: Real,
    combo: Real,
    di_mult: Real,
) -> (Real, Real) {
    if sx == 0.0 && sy == 0.0 {
        return (vx, vy);
    }
    let kb = di_angle(vx, vy);
    let stick = di_angle(sx, sy);
    let in_r = |a: Real| (75.0..=295.0).contains(&a);
    let sel0 = if in_r(kb) && in_r(stick) { kb } else { stick };
    let sel1 = if (165.0..=195.0).contains(&kb) && !wall_bounce { kb } else { stick };
    let target = if grounded { sel0 } else { sel1 };
    let d = kb - target;
    let p1 = if d.abs() < (d - 360.0).abs() { d } else { d - 360.0 };
    let pick = if p1.abs() < (d + 360.0).abs() { p1 } else { d + 360.0 };
    if pick == 0.0 {
        return (vx, vy);
    }
    let di = cumulative_di(kb, combo, di_mult);
    // @1890 RotateAngleAxis(KnockbackLaunchVelocity, angle, (1, 0, 0)): the node's axis is a unit vector already
    // (GetSafeNormal returns it as it is), then FVector::RotateAngleAxis (FMath::SinCos of the angle x pi / 180).
    let ang = if pick < 0.0 { -di } else { di } * kbi;
    let r = ue::rotate_angle_axis([0.0, -vx, vy], ang, [1.0, 0.0, 0.0]);
    (-r[1], r[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rate of millions of seconds (Buff_ArmorPack_C's 9999999 s in the Armor tutorial) ends the walk: in float32 a
    /// frame count above 2^24 does not grow by 1.
    #[test]
    fn timer_ticks_of_a_huge_rate_ends() {
        let k = timer_ticks(Some(9_999_999.0));
        assert!(k > 5.0e8 && k < 7.0e8, "{k}");
    }

    fn params() -> HitParams {
        HitParams {
            damage: 12.0,
            base_knock_back: 900.0,
            knock_back_scaling: 1.1,
            scaling_start_dmg: 0.0,
            hit_pause_time: 0.08,
            hit_stun_mult: 1.0,
            charge_kb_mult: 1.0,
            charge_hit_pause_mult: 0.0,
            total_charged_time: 0.0,
            weight: 100.0,
        }
    }

    #[test]
    fn timer_ticks_counts_strictly_past_the_rate() {
        assert_eq!(timer_ticks(None), Real::INFINITY);
        assert_eq!(timer_ticks(Some(0.0)), Real::INFINITY);
        for sec in [0.001, 0.01, 1.0 / 60.0, 0.05, 0.3, 0.75, 2.5, 30.0] {
            let rate = sec;
            let mut k = 1.0;
            while !(crate::consts::clock(k) > f64::from(rate)) {
                k += 1.0;
            }
            assert_eq!(timer_ticks(Some(sec)), k, "{sec}");
        }
        assert_eq!(timer_count(0.0), 0);
        assert_eq!(timer_count(0.01), 1);
    }

    #[test]
    fn hitstun_clamps() {
        assert_eq!(knockback_to_hitstun(0.0, 0.0), 0.0);
        assert_eq!(knockback_to_hitstun(1.0, 1.0), HITSTUN_FLOOR);
        assert_eq!(knockback_friction_stun(10.0, 0.0), HITSTUN_FLOOR);
        assert_eq!(knockback_to_hitstun(-5.0, 0.7), 0.7);
    }

    #[test]
    fn compute_knock_parts_agree() {
        let p = params();
        let k = compute_knock(&p, 40.0);
        assert_eq!(k.dist, knockback_distance(&p, 40.0));
        assert_eq!(k.speed, k.dist * KB_MULT);
        assert_eq!(k.tier, 1.0);
        assert_eq!(k.hitpause_sec, 0.08);
        assert_eq!(k.hitstun_frames, timer_count(k.hitstun));
    }

    #[test]
    fn di_without_stick_keeps_the_launch() {
        assert_eq!(calculate_di(100.0, 200.0, 0.0, 0.0, false, false, 1.0, 0.0, 1.0), (100.0, 200.0));
        let (x, y) = calculate_di(0.0, 1000.0, 1.0, 0.0, false, false, 1.0, 0.0, 1.0);
        // the launch (0, 1000) turned by the DI stick: 15 degrees of 1000 (258.8, 965.9), its square sum 1e6 + 1/16
        assert_eq!((x, y), (258.81903, 965.92584));
    }
}
