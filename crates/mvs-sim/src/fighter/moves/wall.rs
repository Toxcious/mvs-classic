//! Wall slide, wall fatigue and wall jump.

use super::*;
use crate::consts::{
    DELTA_SECONDS, FAST_FALL_JUMP_BOOST, GRAVITY_SCALE, GRAVITY_Z, JUMP_MIN_TIME, TRACE_OFFSET_01, WALL_FATIGUE_TIME,
    WALL_JUMP_PUSH, WALL_JUMP_TRACE_REACH, WALL_SLIDE_SLOW_MULT, WALL_SLIDE_TERMINAL, WALL_SLIDE_TRACE_HALF,
    WALL_SLIDE_TRACE_REACH, WALL_SLIDE_TRACE_Z, WALL_STALL_MULT_MAX, WALL_STALL_TIME, WALL_STALL_TOUCH_CAP,
};
use crate::events::{Event, NotifyRef, PopKind};
use crate::fighter::body::{Bounds, blocks_wall_jump, cap_half, cap_radius, def, plat};
use crate::fighter::match_side::{move_fx, sfx, tutorial_event_of};
use crate::input::Btn;
use crate::math::ue;
use crate::math::{max, min, sign};
use crate::registry::signals::OwnerSignal;
use mvs_collision::Ground;

/// `allowWallPlay`: BaseFighter AllowWallPlay?: the pawn CDO's until BPI_AllowWallPlay writes it.
pub fn allow_wall_play(w: &World, f: FighterId) -> bool {
    if let Some(v) = w.fighter(f).wall.allow_wall_play {
        return v;
    }
    let d = &w.data.chars[w.names.str(w.fighter(f).ch)].fighter;
    d.pawn.get("AllowWallPlay?").or_else(|| d.base.get("AllowWallPlay?")).and_then(|v| v.as_bool()) == Some(true)
}

/// `bpiAllowWallPlay`.
pub fn bpi_allow_wall_play(w: &mut World, f: FighterId, allow: bool) {
    w.fighter_mut(f).wall.allow_wall_play = Some(allow);
}

/// The attacking flag's notify has AllowWallSliding set.
fn allows_wall_sliding(w: &World, f: FighterId, n: &NotifyRef) -> bool {
    crate::fighter::anim::montages::any_notify_in(&w.data, w, f, *n)
        .and_then(|x| x.p.get("AllowWallSliding"))
        .is_some_and(|v| match v {
            mvs_data::value::Value::Bool(b) => *b,
            mvs_data::value::Value::Num(n) => *n != 0.0,
            mvs_data::value::Value::Null => false,
            _ => true,
        })
}

/// `wallSlideCheck`: Tick > HandleWallSlideCheck (after the fast fall handler). BoxTrace channel 2 from loc + (0, 0,
/// 85.5) forward r + 40, half Z 60.
// Unverified: the forward vector is the facing after this frame's controller tick.
pub fn wall_slide_check(w: &mut World, f: FighterId, flags: &[NotifyRef]) {
    let fi = w.fighter(f);
    // In the air GetMovementEnabled false goes the same way (@79428), and WallSlideOnServer tests it again (@126571).
    if fi.body.on_floor || fi.life.dead > 0.0 || !super::movement_enabled(w, f) {
        if fi.wall.wall_sliding {
            wall_slide_off(w, f);
        }
        return;
    }
    // WallAttachThruATKFlag comes from the attacking flag's AllowWallSliding.
    let atk_ok = flags.is_empty() || flags.iter().any(|n| allows_wall_sliding(w, f, n));
    let allow = fi.clock.tick >= fi.wall.no_wall_slide_until && allow_wall_play(w, f); // @79776
    // @79936: the sign of GetLastMovementInputVector's Y against the forward vector's.
    let lmi = fi.air.last_move_in;
    let input_ok = (lmi != 0.0 && sign(lmi) == fi.body.facing_dir)
        || crate::fighter::anim::has_active(w, f, crate::name!("AnimNotify_OverrideWallSlideInputCheck_C"), -1.0);
    let vx = fi.body.vx;
    let skate_ok = !fi.moves.wall_skate || (vx.abs() > 0.0 && vx.abs() < 200.0);
    if !atk_ok || !allow || !input_ok || fi.body.vy > 0.0 || !skate_ok || !wall_trace(w, f) {
        if w.fighter(f).wall.wall_sliding {
            wall_slide_off(w, f);
        }
        return;
    }
    if !w.fighter(f).wall.wall_sliding {
        wall_slide_on(w, f);
    }
    set_wall_slide_velocity(w, f);
}

/// `wallTrace`: the stage block, then the spawned actors' boxes of the frame that block channel 2.
// Unverified: a box is met where the frame's head placed it.
pub fn wall_trace(w: &World, f: FighterId) -> bool {
    let z0 = w.fighter(f).body.y + cap_half(w, f) * WALL_SLIDE_TRACE_Z - WALL_SLIDE_TRACE_HALF;
    let z1 = z0 + 2.0 * WALL_SLIDE_TRACE_HALF;
    if wall_box_hit(w, f, &w.stage.ground, z0, z1) {
        return true;
    }
    w.fighter(f).platforms.platforms.iter().any(|&pr| {
        let p = plat(w, pr);
        !p.dead && blocks_wall_jump(w, &p) == Some(true) && wall_box_hit(w, f, &p.ground(), z0, z1)
    })
}

fn wall_box_hit(w: &World, f: FighterId, g: &Ground, z0: Real, z1: Real) -> bool {
    let gb = Bounds::of(g);
    if z1 <= gb.bottom || z0 >= gb.top {
        return false;
    }
    let b = &w.fighter(f).body;
    if b.x > gb.left && b.x < gb.right {
        return true; // initial overlap
    }
    let d = ((if b.facing_dir > 0.0 { gb.left } else { gb.right }) - b.x) * b.facing_dir;
    d >= 0.0 && d <= cap_radius(w, f) + WALL_SLIDE_TRACE_REACH
}

/// `wallSlideOn`: WallSlideOnServer / Multicast / Client.
pub fn wall_slide_on(w: &mut World, f: FighterId) {
    tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::LedgeSlide); // @126870, ahead of WallSlideOnMulticast
    lateral_friction_event(w, f, true, false); // AirControlGate = true, DashFallGate = false, CheckLateralFriction
    let fi = w.fighter_mut(f);
    if !fi.wall.wall_capped {
        // ResetJump(0)
        fi.jump.jumps = 0;
        fi.air.air_evades = 0;
        fi.air.air_specials = 0;
        fi.fast_fall.ff_triggered = false;
    }
    // StopAnimMontage(null), BaseFighter @137607.
    crate::fighter::anim::stop_anim_montage(w, f);
    w.fighter_mut(f).wall.wall_sliding = true;
    crate::fighter::signal_actors(w, f, OwnerSignal::WallSliding, true); // OnIsWallSliding @135937
    move_fx(w, f, crate::events::MoveFxKind::WallOn); // OnRep IsWallSliding -> SFX_Wall_Enter
    let fi = w.fighter_mut(f);
    fi.wall.wall_touches += 1;
    if crate::real(fi.wall.wall_touches) > WALL_STALL_TOUCH_CAP {
        set_wall_capped(w, f);
    }
    if w.fighter(f).wall.wall_capped {
        crate::fighter::control::update_evade_values(w, f); // IncrementEvadeCount
        let fi = w.fighter_mut(f);
        if fi.wall.wall_fatigue < 0 {
            fi.wall.wall_fatigue = 0;
            fi.wall.wall_fatigue_at = f64::from(WALL_FATIGUE_TIME);
        }
    } else if w.fighter(f).wall.wall_stall < 0 {
        w.fighter_mut(f).wall.wall_stall = 0;
    }
    w.fighter_mut(f).wall.wall_timer_pending = true; // K2_SetTimerDelegate or K2_UnPauseTimerHandle
}

/// `setWallSlideVelocity`: the slide velocity (pawn tick, old movement path).
pub fn set_wall_slide_velocity(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    if !fi.wall.wall_capped {
        // K2_GetTimerElapsedTimeHandle (142e3dd80): the deltas counted so far.
        let e = crate::real(fi.wall.wall_stall) * DELTA_SECONDS;
        if fi.wall.wall_stall >= 0 && e > 5.0 {
            fi.wall.wall_mult = 1.0 + (WALL_STALL_MULT_MAX - 1.0) * ((e - 5.0) / 5.0);
        }
        if fi.wall.wall_touches > 6 {
            let t = crate::real(fi.wall.wall_touches - 6);
            fi.wall.wall_mult = max(fi.wall.wall_mult, 1.0 + (WALL_STALL_MULT_MAX - 1.0) * (t / 5.0));
        }
    }
    let slow = if fi.controls.inp.y >= 0.5 { WALL_SLIDE_SLOW_MULT } else { 1.0 }; // BaseFighter @131955
    let m = fi.wall.wall_mult;
    let v = if m > 1.0 { WALL_SLIDE_TERMINAL * m } else { WALL_SLIDE_TERMINAL * slow };
    set_terminal(w, f, false, v, false);
}

/// `wallSlideOff`: terminal reset; both timers pause; NetGravity(BFDefaultGravityScale)
/// without touching GravityModifierCounter (HandleWallSlideCheck @79570-79608).
pub fn wall_slide_off(w: &mut World, f: FighterId) {
    set_terminal(w, f, true, 0.0, false);
    w.fighter_mut(f).wall.wall_sliding = false;
    crate::fighter::signal_actors(w, f, OwnerSignal::WallSliding, false); // OnIsWallSliding @136085
    w.fighter_mut(f).moves.net_gravity = Some(GRAVITY_SCALE);
    move_fx(w, f, crate::events::MoveFxKind::WallOff); // SFX_Wall_Exit
}

/// `tickWallTimers`: WallStallTimer (10 s) and WallFatigueTimer (0.5 s looping) for the
/// previous frame's FTimerManager::Tick; a timer set or unpaused in the pawn tick is pending that frame.
pub fn tick_wall_timers(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    if !fi.wall.wall_sliding {
        return;
    }
    if fi.wall.wall_timer_pending {
        fi.wall.wall_timer_pending = false;
        return;
    }
    if fi.wall.wall_stall >= 0 {
        fi.wall.wall_stall += 1;
        if crate::consts::clock(crate::real(fi.wall.wall_stall)) > f64::from(WALL_STALL_TIME) {
            set_wall_capped(w, f);
        }
    }
    let fi = w.fighter_mut(f);
    if fi.wall.wall_fatigue >= 0 {
        fi.wall.wall_fatigue += 1;
        if crate::consts::clock(crate::real(fi.wall.wall_fatigue)) > fi.wall.wall_fatigue_at {
            fi.wall.wall_fatigue_at += f64::from(WALL_FATIGUE_TIME);
            crate::fighter::control::update_evade_values(w, f);
        }
    }
}

/// `setWallCapped`: SetWallStallPenaltyCapped (runs once); WallFatigueFX posts ui_ability_wallfatigue and pops
/// "WALL FATIGUE".
pub fn set_wall_capped(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    if fi.wall.wall_capped {
        return;
    }
    fi.wall.wall_capped = true;
    fi.wall.wall_mult = WALL_STALL_MULT_MAX;
    fi.wall.wall_stall = -1;
    sfx(w, f, Some(crate::name!("ui_ability_wallfatigue"))); // SFX_WALL_FATIGUE
    let fm = w.fighter_mut(f); // f.events: flushed with the fighter's other events
    let id = fm.id;
    fm.events.push(Event::PopText { fighter: id, kind: PopKind::WallFatigue, stacks: 0.0, gain: None, params: None });
}

/// `resetWallFatigue`: ResetWallFatigue (landing, ResetFighterLogic).
pub fn reset_wall_fatigue(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.wall.wall_stall = -1;
    fi.wall.wall_fatigue = -1;
    fi.wall.wall_touches = 0;
    fi.wall.wall_mult = 1.0;
    fi.wall.wall_capped = false;
}

/// A line trace's hit: the entry point's x and the normal's y.
#[derive(Clone, Copy)]
struct LineHit {
    x: Real,
    ny: Real,
    t: Real,
}

/// `traceBlock`: line trace (channel 2) against the stage block and the spawned boxes that block it; the first hit
/// along the line (the stage block's at an equal distance). A start inside a box is a hit at the start with normal
/// -dir (PxGeometryQuery::raycast; ConvertQueryImpactHit 1428f5e60).
pub fn trace_block(w: &World, f: FighterId, x0: Real, y0: Real, x1: Real, y1: Real) -> Option<CornerTraceOut> {
    let mut best = trace_box(&w.stage.ground, x0, y0, x1, y1);
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if p.dead || blocks_wall_jump(w, &p) != Some(true) {
            continue;
        }
        let h = trace_box(&p.ground(), x0, y0, x1, y1);
        if let Some(h) = h
            && best.is_none_or(|b| h.t < b.t)
        {
            best = Some(h);
        }
    }
    best.map(|b| CornerTraceOut { x: b.x, ny: b.ny })
}

fn trace_box(g: &Ground, x0: Real, y0: Real, x1: Real, y1: Real) -> Option<LineHit> {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let gb = Bounds::of(g);
    if x0 >= gb.left && x0 <= gb.right && y0 >= gb.bottom && y0 <= gb.top {
        return Some(LineHit { x: x0, ny: -dy / ue::size2d([dx, dy]), t: 0.0 });
    }
    let mut t0: Real = 0.0;
    let mut t1: Real = 1.0;
    let mut ny = 0.0;
    for (p, d, lo, hi, is_y) in [(x0, dx, gb.left, gb.right, false), (y0, dy, gb.bottom, gb.top, true)] {
        if d == 0.0 {
            if p <= lo || p >= hi {
                return None;
            }
            continue;
        }
        let mut a = (lo - p) / d;
        let mut b = (hi - p) / d;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t0 {
            t0 = a;
            ny = if is_y { if d > 0.0 { -1.0 } else { 1.0 } } else { 0.0 };
        }
        t1 = min(t1, b);
        if t0 > t1 {
            return None;
        }
    }
    Some(LineHit { x: x0 + dx * t0, ny, t: t0 })
}

/// `tryWallJump`: the jump press's wall jump: !Walking && AllowWallSliding? && AllowWallPlay?, then two line traces
/// from loc - fwd (r + 30); either hit with normal.Z > -0.3 wall jumps.
pub fn try_wall_jump(w: &mut World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    if fi.body.on_floor || fi.clock.tick < fi.wall.no_wall_slide_until || !allow_wall_play(w, f) {
        return false; // @61550
    }
    let r = cap_radius(w, f) + WALL_JUMP_TRACE_REACH;
    let fd = fi.body.facing_dir;
    let (x, y) = (fi.body.x, fi.body.y);
    let mut h1 = trace_block(w, f, x - fd * r, y, x + fd * r, y + cap_half(w, f));
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(ny > -0.3): a NaN fails")]
    if h1.as_ref().is_some_and(|h| !(h.ny > -0.3)) {
        h1 = None;
    }
    if h1.is_none() {
        let h2 = trace_block(w, f, x - fd * r, y, x + fd * r, y);
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(ny > -0.3): a NaN fails")]
        if h2.is_none_or(|h| !(h.ny > -0.3)) {
            return false;
        }
    }
    // EvadeWallJump: GetCurrentMontage is the montage of one of the Evades entries.
    let cur = active_montage_name(w, f);
    let evade_wall_jump = cur.is_some_and(|c| {
        let nav = nav_of(w, f);
        let c = w.names.str(c);
        nav.evades.as_ref().is_some_and(|es| es.iter().any(|e| e.attack_montage == c))
    });
    // PlayAnimMontage(WallJumpAnim, 1.0): Montage_Play stops the group's other instances with its BlendIn (0).
    crate::fighter::anim::stop_montage(w, f, Some(0.0), None);
    let wj = nav_of(w, f).wall_jump.clone();
    // (a data string is interned when the World binds its data, so `get` finds every montage and event name the data
    // holds; a name it lacks is none)
    let wj = wj.and_then(|s| w.names.get(&s));
    crate::fighter::anim::play_plain_montage(w, f, wj, 1.0, crate::fighter::anim::SinkKind::Dash, None);
    crate::fighter::control::add_recent_attack(w, f, Btn::Jump, None);
    // FastfallMult = FastFallTriggered || IsFastFalling || EvadeWallJump.
    let fi = w.fighter_mut(f);
    let ff = fi.fast_fall.ff_triggered || fi.fast_fall.fast_falling || evade_wall_jump;
    fi.fast_fall.ff_mult = ff;
    if ff {
        tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::FastFallJump); // FastFallJumpTutEvent(FastfallMult) @9313
    }
    w.fighter_mut(f).fast_fall.fast_falling = false;
    fast_fall_end(w, f); // IsFastFalling = false @9336
    w.fighter_mut(f).jump.quick_fall_at = -1; // BreakQuickFall
    // NetImpulse((0, sign(Y - hitY) x 600 if trace 1 hit, else 0, jump velocity), both overrides, type 2.
    let v = jump_velocity(w, f) * (if ff { 1.05 } else { 1.0 });
    let px = h1.map_or(0.0, |h| sign(x - h.x) * WALL_JUMP_PUSH);
    net_impulse(w, f, px, v, true, true, 2.0, false, 0.0);
    let fi = w.fighter_mut(f);
    fi.jump.jump_v = v;
    fi.jump.last_jump_time = fi.clock.time;
    crate::fighter::set_jumping(w, f, true);
    let fi = w.fighter_mut(f);
    fi.jump.jumps = if fi.wall.wall_capped { 2 } else { 1 };
    fi.air.air_attack_available = true;
    fi.jump.ground_jump_tick = fi.clock.tick; // DoubleJumpAvailable false, Delay(0.15)
    fi.jump.jump_frame = 0;
    fi.jump.jump_released = false;
    fi.jump.jump_stop_gate = true;
    // CalcJumpStopTime with MaxWallJumpHeight (x FastFallJumpBoostMult with FastfallMult).
    let h = wall_jump_height(w, f) * (if ff { FAST_FALL_JUMP_BOOST } else { 1.0 });
    let g = GRAVITY_Z * GRAVITY_SCALE;
    let vs = jump_stop_mult(w, f) * v;
    let t = (v - max(v * v - 2.0 * g * (h - (1.5 * vs * vs) / g), 0.0).sqrt()) / g;
    w.fighter_mut(f).jump.jump_cut_at = crate::math::to_int32(crate::fighter::common::latent_frames(t));
    if let Some(h) = w.fighter(f).hooks.char.end_jump {
        h(w, f); // a pawn's EndJump override
    }
    let fi = w.fighter_mut(f);
    fi.jump.jump_min_time = JUMP_MIN_TIME; // JumpFromGround: SingleJumpMinTime
    fi.jump.jump_start_time = fi.clock.time;
    fi.fast_fall.ff_triggered = false;
    tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::WallJump); // @10696
    // Wall Jump Multicast -> SFX_WallJump: SFX_Wall_Jump by surface trace, VO_Jump (no dust).
    let sd = &def(w, f).sfx;
    let (wj, vo) = (sd.wall_jump.clone(), sd.vo_jump.clone());
    // (the data's event names are interned at the bind: a name `get` misses is not in the data)
    if let Some(ev) = w.names.get(&wj) {
        let fi = w.fighter(f);
        let (y, x) = (fi.body.y, fi.body.x);
        crate::fighter::match_side::surface_sfx(w, f, ev, y - TRACE_OFFSET_01, false, None, Some(x));
    }
    let vo = vo.and_then(|s| w.names.get(&s));
    sfx(w, f, vo);
    true
}
