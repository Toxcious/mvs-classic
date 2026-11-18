//! The CharacterMovement step (walking, falling, flying), the stage collision and landing.
//!
//! The mode physics of a tick runs under anim root motion (RootMotionParams.bHasRootMotion, CMC +0xa90; set around
//! the move by `physics` and `paused_move`). The exe's rules (stock functions, the fighter component overrides
//! PhysFlying and PhysCustom alone):
//! - ApplyRootMotionToVelocity (14291cb50), once per substep of every mode after its own velocity step: Velocity =
//!   the root-motion velocity, Z kept from Velocity only while Falling.
//! - PhysWalking: CalcVelocity skipped; the move along the floor, FindFloor and the ledge / CheckFall / StartFalling
//!   block as for any walker; the velocity is not taken back from the move.
//! - PhysFalling: the lateral CalcVelocity skipped, NewFallVelocity (gravity) runs, then XY from the root motion;
//!   the move is the midpoint of the velocity before and after. The root track's Z does nothing.
//! - FMC PhysFlying: CalcVelocity skipped, the move is Velocity x dt, the velocity is not taken back from it.
//! - PhysKnockback (the custom mode): its velocity block skipped; not Falling, so the root Z is taken too.
//!
//! The root-motion flag and velocity are not module values (`RM`, `rootVel`) here; they are the
//! [`Rm`] argument of the calls that read them (`None`: no root motion).
// Unverified: MoveAlongFloor dropping the delta's Z is stock.

use super::*;
use crate::combat::buffs::{BuffThruAtkK, buff_signal, buff_thru_atk};
use crate::combat::hit::{hitstun_landed, juggle_landed, phys_knockback, try_bounce};
use crate::consts::{
    BRAKE_TO_STOP_VELOCITY, BRAKING_FRICTION_FACTOR, DASH_IMPULSE, DT, GRAVITY_Z, HITPAUSE_DILATION, MAX_DEPENETRATION,
    MAX_SIM_STEP, SUBSTEPS,
};
use crate::events::NotifyRef;
use crate::fighter::FrameMode;
use crate::fighter::body::{
    self, Bounds, FIND_FLOOR_DIST, Plat, PlatformRef, cap_half, cap_radius, capsule_half, clamp_floor, floor_holder,
    floor_top, ignores_platform, impart_base, ledge_corner, plat, rebase, rest_gap, set_grounded, span_at, span_near,
    stand_y_of,
};
use crate::math::ue;
use crate::math::{max, min, sign};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_collision::Ground;
use mvs_collision::capsule::capsule_sweep;

/// The root motion of the tick: its velocity (`rootVel`), `None` without root motion (`RM` false).
pub type Rm = Option<(Real, Real)>;

/// `capsuleOut` with its `OUT_N`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn cap_out(
    left: Real,
    right: Real,
    bottom: Real,
    top: Real,
    x: Real,
    y: Real,
    r: Real,
    hs: Real,
) -> (Real, Real, Real) {
    crate::fighter::common::capsule_out_n(left, right, bottom, top, x, y, r, hs)
}

/// PullBackHit's share of a move of length `dist`: Clamp(0.1, 0.1 / Dist, 1 / Dist) + 0.001.
fn pull_back(dist: Real) -> Real {
    min(max(0.1, 0.1 / dist), 1.0 / dist) + 0.001
}

/// `rootWalkDelta`: a PhysWalking substep under root motion: the move by the velocity as it is, in float32 like
/// `walk_delta`.
fn root_walk_delta(w: &World, f: FighterId, x: Real, h: Real) -> Real {
    let x0 = x;
    (x0 + (w.fighter(f).body.vx * h)) - x
}

/// `physRootKnockback`: PhysKnockback under root motion: the root-motion velocity whole; the floor sweep as a
/// fall's.
fn phys_root_knockback(w: &mut World, f: FighterId, dt: Real, rv: (Real, Real)) {
    let h = dt / SUBSTEPS;
    let mut s = 0.0;
    while s < SUBSTEPS {
        let fx = w.fighter_mut(f);
        fx.body.vx = rv.0;
        fx.body.vy = rv.1;
        let (x, y, vx, vy) = (fx.body.x, fx.body.y, fx.body.vx, fx.body.vy);
        if fall_move_rm(w, f, x, y, vx * h, vy * h, h, s, false, (SUBSTEPS - s - 1.0) * h, Some(rv)) {
            break;
        }
        s += 1.0;
    }
}

/// `dashDisabled`: BaseFighter DisableDash: the pawn CDO's until BPI_DisableDash writes it.
pub fn dash_disabled(w: &World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    if let Some(v) = fi.moves.disable_dash {
        return v;
    }
    let d = &w.data.chars[w.names.str(fi.ch)].fighter;
    d.pawn.get("DisableDash").or_else(|| d.base.get("DisableDash")).and_then(|v| v.as_bool()) == Some(true)
}

/// `bpiDisableDash`.
pub fn bpi_disable_dash(w: &mut World, f: FighterId, disable: bool) {
    w.fighter_mut(f).moves.disable_dash = Some(disable);
}

/// `recallMove`: the MoveAlongPath update's K2_SetActorLocation (sweep, nothing to hit
/// with the collision off).
pub fn recall_move(w: &mut World, f: FighterId, x: Real, actor_y: Real) {
    let d = cap_half(w, f) - capsule_half(w, f);
    let fx = w.fighter_mut(f);
    fx.body.x = x;
    fx.body.y = actor_y + d;
}

/// `teleportMove`: C019AimPunch ActivateObject's K2_SetActorLocation (no sweep) with the
/// capsule's collision on: the CMC's next move starts in penetration and ResolvePenetration moves it out.
// Deviation: resolved here at once, with the capsule taken as its bounding box in the play plane (the shortest of
// the four ways out of the block); each walking box in turn (the build's one MTD against the overlapping bodies is
// not read).
pub fn teleport_move(w: &mut World, f: FighterId, x: Real, actor_y: Real) {
    recall_move(w, f, x, actor_y);
    let r = cap_radius(w, f);
    let hh = capsule_half(w, f);
    let (mut ax, mut ay) = (x, actor_y);
    let list = body::floor_list(w, f);
    for g in &list {
        let gb = Bounds::of(g);
        let up = gb.top - (ay - hh);
        let down = ay + hh - gb.bottom;
        let left = ax + r - gb.left;
        let right = gb.right - (ax - r);
        let d = min(min(min(up, down), left), right);
        if d <= 0.0 {
            continue;
        }
        let k = d + 0.125;
        let fx = w.fighter_mut(f);
        if d == up {
            fx.body.y += k;
            ay += k;
        } else if d == down {
            fx.body.y -= k;
            ay -= k;
        } else if d == left {
            fx.body.x -= k;
            ax -= k;
        } else {
            fx.body.x += k;
            ax += k;
        }
    }
    body::floor_list_done(w, list);
}

/// `sweepMove`: K2_SetActorLocation with bSweep on a fighter whose capsule collides: the root capsule swept to the
/// target, stopped at the first blocking hit and pulled back.
// Deviation: a 2D capsule against boxes; the start's normal is the port's least way out, not PhysX's MTD; a capsule
// that starts in touch is blocked only when the move goes into the body; a ship's pieces are not swept.
pub fn sweep_move(w: &mut World, f: FighterId, x: Real, actor_y: Real) {
    let r = cap_radius(w, f);
    let hh = capsule_half(w, f);
    let ax = w.fighter(f).body.x;
    let ay = body::actor_y(w, f);
    let (dx, dy) = (x - ax, actor_y - ay);
    let dist = ue::size2d([dx, dy]);
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(dist > 4e-4): a NaN returns")]
    if !(dist > 4e-4) {
        return; // MinMovementDistSq of a sweep: (4 x KINDA_SMALL_NUMBER)^2
    }
    let mut t: Real = 1.0;
    let mut meet = |g: &Ground| {
        let (d, nx, ny) = cap_out(
            crate::math::narrow(g.left),
            crate::math::narrow(g.right),
            crate::math::narrow(g.bottom),
            crate::math::narrow(g.top),
            ax,
            ay,
            r,
            hh - r,
        );
        if d >= 0.0 {
            if nx * dx + ny * dy <= 0.0 {
                t = 0.0;
            }
            return;
        }
        let Some(k) = capsule_sweep(ax, ay, x, actor_y, r, hh, g) else {
            return;
        };
        if k >= f64::from(t) {
            return;
        }
        // in touch at the start: a hit only for a move into the body
        if k == 0.0
            && capsule_sweep(ax + (dx / dist) * 0.01, ay + (dy / dist) * 0.01, x, actor_y, r, hh, g) != Some(0.0)
        {
            return;
        }
        t = crate::math::narrow(k);
    };
    for (g, _) in body::floor_boxes(w, f) {
        meet(&g);
    }
    for &i in &w.fighter(f).platforms.stage_platforms {
        if !ignores_platform(w, f, PlatformRef::Stage(i)) {
            meet(&plat(w, PlatformRef::Stage(i)).ground());
        }
    }
    if t < 1.0 {
        t = min(max(t - (min(max(0.1, 0.1 / dist), 1.0 / dist) + 0.001), 0.0), 1.0);
    }
    recall_move(w, f, ax + dx * t, ay + dy * t);
}

/// `settle`: a capsule a root location written with no sweep may have left in a blocker:
/// the movement component's next sweeping move resolves it (`resolve_resize`).
pub fn settle(w: &mut World, f: FighterId) {
    w.fighter_mut(f).capsule.cap_resized = true;
}

/// `setCapsuleSize`: UCapsuleComponent::SetCapsuleSize (exe 142946110): HalfHeight = max(0, NewHalfHeight,
/// NewRadius), Radius = max(0, NewRadius), unscaled; no transform write, no sweep.
pub fn set_capsule_size(w: &mut World, f: FighterId, radius: Real, half: Real) {
    let h = max(max(0.0, half), radius);
    let r = max(0.0, radius);
    let c = &mut w.fighter_mut(f).capsule;
    if h == c.cap_half_u && r == c.cap_radius_u {
        return;
    }
    c.cap_half_u = h;
    c.cap_radius_u = r;
    c.cap_resized = true;
}

/// `setCapsuleHalfHeight`.
pub fn set_capsule_half_height(w: &mut World, f: FighterId, half: Real) {
    let r = w.fighter(f).capsule.cap_radius_u;
    set_capsule_size(w, f, r, half);
}

/// `setCapsuleRadius`.
pub fn set_capsule_radius(w: &mut World, f: FighterId, radius: Real) {
    let h = w.fighter(f).capsule.cap_half_u;
    set_capsule_size(w, f, radius, h);
}

/// `resolveResize`: the movement component's first sweeping move after a change that left
/// the capsule where it may overlap a blocker.
// Deviation: taken ahead of the move on his own base: within the reach and held at his x he is put at his walking
// height there, further over it he starts falling at once; a higher floor under him is not looked for.
pub fn resolve_resize(w: &mut World, f: FighterId) {
    if body::no_collision(w, f) {
        return;
    }
    w.fighter_mut(f).capsule.cap_resized = false;
    if w.fighter(f).body.on_floor {
        let fi = w.fighter(f);
        let (x, base) = (fi.body.x, fi.platforms.base);
        if fi.body.y - cap_half(w, f) - floor_top(w, f) <= FIND_FLOOR_DIST && floor_holder(w, f, base, x).is_some() {
            rebase(w, f);
            let sy = stand_y_of(w, f);
            w.fighter_mut(f).body.y = sy;
        } else {
            set_grounded(w, f, false);
            w.fighter_mut(f).body.vy = 0.0;
            if let Some(b) = base {
                impart_base(w, f, b);
            }
        }
    }
    depenetrate(w, f);
}

/// `depenetrate`: SafeMoveUpdatedComponent's resolve of a move that starts in penetration (142977610), against every
/// body the root capsule's sweep blocks on.
// Deviation: each body in turn, from where the one before left the capsule; the normal and the depth are the port's
// 2D least way out, not PhysX's MTD; the resolve's overlap test at the new place and its fallback sweeps are left
// out; a ship is met by its run of pieces nearest the capsule.
pub fn depenetrate(w: &mut World, f: FighterId) {
    let r = cap_radius(w, f);
    let hs = capsule_half(w, f) - r;
    let (x0, y0) = (w.fighter(f).body.x, w.fighter(f).body.y);
    let list = body::floor_list(w, f);
    for g in &list {
        push_out(
            w,
            f,
            crate::math::narrow(g.left),
            crate::math::narrow(g.right),
            crate::math::narrow(g.bottom),
            crate::math::narrow(g.top),
            r,
            hs,
        );
    }
    body::floor_list_done(w, list);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        if p.stage_solid || p.dead || ignores_platform(w, f, pr) {
            continue;
        }
        let x = w.fighter(f).body.x;
        if let Some(s) = span_near(w, &p, x, r) {
            push_out(w, f, s.left, s.right, p.bottom, p.top, r, hs);
        }
    }
    let mut k = 0;
    while k < w.fighter(f).platforms.stage_platforms.len() {
        let i = w.fighter(f).platforms.stage_platforms[k];
        k += 1;
        if !ignores_platform(w, f, PlatformRef::Stage(i)) {
            let p = plat(w, PlatformRef::Stage(i));
            push_out(w, f, p.left, p.right, p.bottom, p.top, r, hs);
        }
    }
    let fx = w.fighter_mut(f);
    let (dx, dy) = (fx.body.x - x0, fx.body.y - y0);
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    fx.body.prev_x += dx;
    fx.body.prev_y += dy;
    body::shift_move_start(w, f, dx, dy);
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn push_out(w: &mut World, f: FighterId, left: Real, right: Real, bottom: Real, top: Real, r: Real, hs: Real) {
    let (x, ay) = (w.fighter(f).body.x, body::actor_y(w, f));
    let (d, nx, ny) = cap_out(left, right, bottom, top, x, ay, r, hs);
    if d < 0.0 {
        return;
    }
    let k = min(d + 0.125, MAX_DEPENETRATION);
    let fx = w.fighter_mut(f);
    fx.body.x += nx * k;
    fx.body.y += ny * k;
}

/// `physFlying`: FighterMovementComponent PhysFlying (FUN_1410d7ca0). Under anim root
/// motion (`root`) CalcVelocity is skipped and the velocity is not taken back from the move.
pub fn phys_flying(w: &mut World, f: FighterId, dt: Real, root: bool) {
    let inp = w.fighter_mut(f).moves.fly_input.take();
    if !root {
        flying_velocity(w, f, dt, inp);
    }
    if w.fighter(f).capsule.cap_resized {
        // The first move after a SetCapsuleSize: none for a zero delta, else the penetration resolve first.
        let b = &w.fighter(f).body;
        if b.vx * dt == 0.0 && b.vy * dt == 0.0 {
            return;
        }
        resolve_resize(w, f);
    }
    let fx = w.fighter_mut(f);
    let (x0, y0) = (fx.body.x, fx.body.y);
    fx.body.x += fx.body.vx * dt;
    fx.body.y += fx.body.vy * dt;
    // Flying slides on the block itself (top + half height), without the walking hover: SafeMoveUpdatedComponent,
    // then on a hit StepUp or HandleImpact + SlideAlongSurface; no FindFloor; Velocity = displacement x (1 / dt).
    // Unverified: the rounded slide is a model (StepUp's stock body is not read; the block's lower corners are not
    // rounded). Every walking box, World.ground first, each from the move's start with what the boxes before it
    // left (Deviation: the build's one sweep meets the first of them).
    let ground = w.stage.ground;
    let sy = stand_y_of(w, f) - rest_gap(w, f);
    fly_slide(w, f, &ground, sy, x0, y0, dt, root);
    let mut k = 0;
    while k < w.fighter(f).platforms.platforms.len() {
        let pr = w.fighter(f).platforms.platforms[k];
        k += 1;
        let p = plat(w, pr);
        if !(p.stage_solid && !p.dead && !ignores_platform(w, f, pr)) {
            continue;
        }
        let q = p.ground();
        let sy = q.top + f64::from(cap_half(w, f)) + f64::from(rest_gap(w, f)) - f64::from(rest_gap(w, f));
        fly_slide(w, f, &q, crate::math::narrow(sy), x0, y0, dt, root);
    }
}

/// `flySlide`: physFlying's slide against one box g, whose top plane for the capsule's centre is sy.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn fly_slide(w: &mut World, f: FighterId, g: &Ground, sy: Real, x0: Real, y0: Real, dt: Real, root: bool) {
    let half = cap_half(w, f);
    let gb = Bounds::of(g);
    let low = gb.bottom - half;
    let r = cap_radius(w, f);
    let (x, y) = (w.fighter(f).body.x, w.fighter(f).body.y);
    if !(y < sy && y > low && x > gb.left - r && x < gb.right + r) {
        return;
    }
    let mx = x - x0;
    let my = y - y0;
    let dist = ue::size2d([mx, my]);
    let back = pull_back(dist);
    let straight = half - r; // the cap's centre under the capsule's
    let over = y0 >= sy - 1.0 || y0 - straight >= gb.top;
    if over && x >= gb.left && x <= gb.right {
        let fx = w.fighter_mut(f);
        fx.body.y = if y0 >= sy { y0 + my * max((sy - y0) / my - back, 0.0) } else { sy };
        if !root {
            fx.body.vy = (fx.body.y - y0) * (1.0 / dt);
        }
    } else if over {
        let ex = if x < gb.left { gb.left } else { gb.right };
        let cx = x - ex;
        let cy = y - straight - gb.top;
        let d = ue::size2d([cx, cy]);
        // over the top plane: the cap's sphere against the edge; under it: the straight side against the face
        if (if cy > 0.0 { d < r } else { cx.abs() < r }) && ledge_corner(w, f, g, ex) {
            let fx = w.fighter_mut(f);
            if cy > 0.0 {
                fx.body.x = ex + (cx / d) * r;
                fx.body.y = gb.top + straight + (cy / d) * r;
            } else {
                fx.body.x = if ex == gb.left { ex - r } else { ex + r };
            }
            if !root {
                fx.body.vx = (fx.body.x - x0) * (1.0 / dt);
                fx.body.vy = (fx.body.y - y0) * (1.0 / dt);
            }
        }
    } else if y0 <= low {
        let fx = w.fighter_mut(f);
        fx.body.y = y0 + my * max((low - y0) / my - back, 0.0);
        if !root {
            fx.body.vy = (fx.body.y - y0) * (1.0 / dt);
        }
    } else {
        let lw = x < (gb.left + gb.right) / 2.0;
        let wall = if lw { gb.left - r } else { gb.right + r };
        let out = if lw { x0 <= wall } else { x0 >= wall }; // started outside the face
        let fx = w.fighter_mut(f);
        fx.body.x = if out && mx != 0.0 { x0 + mx * max((wall - x0) / mx - back, 0.0) } else { wall };
        if !root {
            fx.body.vx = (fx.body.x - x0) * (1.0 / dt);
        }
    }
}

/// `flyingVelocity`: PhysFlying's CalcVelocity (stock 1429217d0): Friction = 0.5 x FluidFriction, fluid,
/// GetMaxBrakingDeceleration = BrakingDecelerationFlying, MaxSpeed = MaxFlySpeed x the analog modifier. The CMC's
/// float routines: ScaleInputAcceleration (1429427e0: the input clamped to size 1 by InvSqrt, then x
/// MaxAcceleration), ComputeAnalogInputModifier (142926790: sqrtss(sum) x (1 / MaxAcceleration), clamped to 0..1),
/// IsExceedingMaxSpeed (142970eb0), ApplyVelocityBraking (14291de00: GetSafeNormal(V) x -B), the over-speed rescale and
/// the direction change by GetSafeNormal, the input acceleration clamped by GetClampedToMaxSize.
fn flying_velocity(w: &mut World, f: FighterId, dt: Real, inp: Option<[Real; 2]>) {
    let fi = w.fighter(f);
    let ix = inp.map_or(0.0, |i| i[0]) + fi.air.move_in;
    let iy = inp.map_or(0.0, |i| i[1]);
    // ScaleInputAcceleration: GetClampedToMaxSize(1) folded (no `== 1` test), then x GetMaxAcceleration.
    let s2 = ix * ix + iy * iy;
    let (ix, iy) = if s2 > 1.0 {
        let k = ue::inv_sqrt(s2);
        (ix * k, iy * k)
    } else {
        (ix, iy)
    };
    let a = max_accel(w, f); // GetMaxAcceleration outside Falling: CMC MaxAcceleration
    let ax = ix * a;
    let ay = iy * a;
    // ComputeAnalogInputModifier.
    let sa = ax * ax + ay * ay;
    let analog = if sa > 0.0 && a > ue::SMALL_NUMBER {
        let v = sa.sqrt() * (1.0 / a);
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the exe's !(v >= 0): a NaN gives 0")]
        if !(v >= 0.0) {
            0.0
        } else if v < 1.0 {
            v
        } else {
            1.0
        }
    } else {
        0.0
    };
    let ff = 0.5 * fi.moves.fluid_friction;
    let fly = fi.moves.fly.as_ref();
    let b = fly.map_or(0.0, |s| s.braking_decel_flying);
    let zero_acc = ax == 0.0 && ay == 0.0;
    let mx = fly.map_or_else(|| crate::math::narrow(body::def(w, f).max_fly_speed), |s| s.max_fly_speed) * analog;
    let fx = w.fighter_mut(f);
    let (vx0, vy0) = (fx.body.vx, fx.body.vy);
    let over = ue::is_exceeding_max_speed([0.0, vx0, vy0], mx);
    if zero_acc || over {
        let fr = ff * BRAKING_FRICTION_FACTOR;
        if (vx0 != 0.0 || vy0 != 0.0) && (fr != 0.0 || b != 0.0) {
            let (rx, ry) = if b != 0.0 {
                let n = ue::safe_normal([0.0, vx0, vy0], ue::SMALL_NUMBER);
                (n[1] * -b, n[2] * -b)
            } else {
                (0.0, 0.0)
            };
            let nx = vx0 + (-fr * vx0 + rx) * dt;
            let ny = vy0 + (-fr * vy0 + ry) * dt;
            let n2 = nx * nx + ny * ny;
            let stop = nx * vx0 + ny * vy0 <= 0.0
                || n2 <= 1e-4
                || (b != 0.0 && n2 <= BRAKE_TO_STOP_VELOCITY * BRAKE_TO_STOP_VELOCITY);
            fx.body.vx = if stop { 0.0 } else { nx };
            fx.body.vy = if stop { 0.0 } else { ny };
        }
        if over && fx.body.vx * fx.body.vx + fx.body.vy * fx.body.vy < mx * mx && ax * vx0 + ay * vy0 > 0.0 {
            let n = ue::safe_normal([0.0, vx0, vy0], ue::SMALL_NUMBER);
            fx.body.vx = n[1] * mx;
            fx.body.vy = n[2] * mx;
        }
    } else {
        let sp = ue::size2d([fx.body.vx, fx.body.vy]);
        let n = ue::safe_normal([0.0, ax, ay], ue::SMALL_NUMBER);
        let k = min(dt * ff, 1.0);
        fx.body.vx -= (fx.body.vx - n[1] * sp) * k;
        fx.body.vy -= (fx.body.vy - n[2] * sp) * k;
    }
    let fl = 1.0 - min(ff * dt, 1.0);
    fx.body.vx *= fl;
    fx.body.vy *= fl;
    if !zero_acc {
        let lim = if ue::is_exceeding_max_speed([0.0, fx.body.vx, fx.body.vy], mx) {
            ue::size2d([fx.body.vx, fx.body.vy])
        } else {
            mx
        };
        fx.body.vx += ax * dt;
        fx.body.vy += ay * dt;
        let v = ue::clamped_to_max_size([0.0, fx.body.vx, fx.body.vy], lim);
        fx.body.vx = v[1];
        fx.body.vy = v[2];
    }
}

/// A notify's properties (its montage's record in the fighter's data, else in the montages its second meshes play:
/// a Disguise copy's states are the owner's).
fn notify_p<'a>(w: &'a World, f: FighterId, n: &NotifyRef) -> Option<&'a mvs_data::value::Props> {
    crate::fighter::anim::montages::any_notify_in(&w.data, w, f, *n).map(|x| &x.p)
}

/// A flag of the attacking flags' notifies is set (truthy).
pub(super) fn flags_any(w: &World, f: FighterId, flags: &[NotifyRef], key: &str) -> bool {
    flags.iter().any(|n| notify_p(w, f, n).and_then(|p| p.get(key)).is_some_and(truthy))
}

/// JavaScript truthiness of a data value.
pub(super) fn truthy(v: &mvs_data::value::Value) -> bool {
    use mvs_data::value::Value;
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        _ => true,
    }
}

/// `pawnMoveInput`: BaseFighter HandleMovementInput (pawn ReceiveTick, old movement path;
/// reads MoveXAxisValue every frame).
pub fn pawn_move_input(w: &mut World, own: FighterId) {
    let f = own;
    let inp = w.fighter(f).controls.inp;
    let flags = crate::fighter::control::attacking_flags(w, f);
    let attacking = crate::fighter::is_attacking(w, f);
    // The ThruATK flags are counters raised by the attacking flag states' Allow values and by a buff's own writes.
    let ground_ctl =
        !attacking || flags_any(w, f, &flags, "AllowGroundControl") || buff_thru_atk(w, f, BuffThruAtkK::Ground);
    let air_ctl = !attacking || flags_any(w, f, &flags, "AllowAirControl") || buff_thru_atk(w, f, BuffThruAtkK::Air);
    // While attacking the turn also waits out RotationLockedFromRightStick (a forced press's 0.25 s window).
    let rot_thru = flags_any(w, f, &flags, "AllowRotationChange")
        || buff_thru_atk(w, f, BuffThruAtkK::Rotation)
        || w.fighter(f).controls.rot_thru_atk_count.unwrap_or(0) > 0;
    let can_turn =
        !attacking || (rot_thru && crate::real(w.fighter(f).clock.tick) >= w.fighter(f).controls.rot_lock_until);
    // MoveX handler @114813-114909 / @115390: GetIsInAir, not IsFalling.
    let air = crate::fighter::in_air(w, f);
    let ctl = if air { air_ctl } else { ground_ctl };
    let stunned = crate::fighter::in_hitstun(w, f);
    // HandleMovementInput runs only with GetMovementEnabled (@139577-139747): disabled, no movement input, no crouch
    // reset, dash or turn.
    // Unverified: AddMovementInput is spent each frame, so the input left is none (the CMC's consume, not read).
    if !super::movement_enabled(w, f) {
        w.fighter_mut(f).air.move_in = 0.0;
        return;
    }
    w.fighter_mut(f).air.move_in = if !stunned && ctl && inp.x.abs() > 0.4 { sign(inp.x) } else { 0.0 };
    // In hitstun, and while attacking without the ground / air control flag, MoveX runs SetCrouching(false).
    if stunned || !ctl {
        set_crouching(w, f, false);
    }
    if stunned {
        return;
    }
    // Dash (same event, ahead of the movement input and the turn).
    if ctl {
        let dir = sign(inp.x);
        // Deviation: the first stick movement stops the intro montage with its own BlendOut (in the build nothing
        // stops it, so a fighter of the second intro call slides in its intro pose for up to 1.35 s).
        if dir != 0.0 && w.fighter(f).anim.plain_is_intro {
            let blend = w.fighter(f).montages.dash_montage.as_ref().map(|m| m.montage);
            if let Some(m) = blend {
                let d = &w.data.chars[w.names.str(w.fighter(f).ch)].fighter;
                let bo = d.montages.get(w.names.str(m.name)).map_or(0.0, |r| r.blend_out);
                crate::fighter::anim::stop_dash_montage(w, f, bo);
            }
        }
        let fi = w.fighter(f);
        if !air
            && dir != 0.0
            && dir != fi.air.last_direction
            && !fi.evade.evading
            && !attacking
            && !fi.moves.cmd_jumped
            && !dash_disabled(w, f)
        {
            dash_dance(w, f, false);
        }
        w.fighter_mut(f).air.last_direction = dir;
    }
    // Turn (RotateInAnyMovementMode is true): any non-zero MoveX turns the fighter to the stick.
    if inp.x != 0.0 && can_turn {
        crate::fighter::set_facing(w, f, if inp.x < 0.0 { -1.0 } else { 1.0 });
    }
    // Crouch block (MoveX, after the movement input and the turn).
    if ctl {
        let d = &w.data.chars[w.names.str(w.fighter(f).ch)].fighter;
        let ccw = d.pawn.get("CanCrouchWalk").is_some_and(truthy);
        let fi = w.fighter(f);
        let crouch = ((ccw && inp.y < -0.25) || fi.crouch.pushing_down)
            && !air
            && !fi.platforms.dropping_down
            && !attacking
            && (ccw || inp.x.abs() < 0.5)
            && !fi.evade.evading;
        if crouch != fi.crouch.crouching {
            set_crouching(w, f, crouch);
        }
    }
}

/// `setCrouching`: isCrouching, then Crouch + SFX_Crouch_Enter + CrouchChange(true), or
/// UnCrouch + SFX_Crouch_Exit + CrouchChange(false).
pub fn set_crouching(w: &mut World, f: FighterId, c: bool) {
    if w.fighter(f).crouch.crouch_locked {
        return; // @133322
    }
    w.fighter_mut(f).crouch.crouching = c;
    let fx = w.fighter(f).crouch.crouch_fx;
    if c && !fx {
        let s = body::def(w, f).sfx.crouch_enter.clone();
        let ev = w.names.get(&s);
        crate::fighter::match_side::sfx(w, f, ev);
        w.fighter_mut(f).crouch.crouch_fx = true;
    } else if !c && fx {
        let s = body::def(w, f).sfx.crouch_exit.clone();
        let ev = w.names.get(&s);
        crate::fighter::match_side::sfx(w, f, ev);
        w.fighter_mut(f).crouch.crouch_fx = false;
    }
    if !w.fighter(f).buffs.buffs.is_empty() {
        buff_signal(w, f, BuffSignal::CrouchChange, BuffSignalArg { crouched: Some(c), ..Default::default() });
    }
    if c {
        crate::fighter::match_side::tutorial_event_of(w, f, crate::fighter::match_side::TutorialEvent::Crouch); // @2504, behind CrouchChange(true)
    }
}

/// `physics`.
pub fn physics(w: &mut World, f: FighterId) {
    apply_impulse(w, f);
    let dt = DT;
    let force = force_move_prepare(w, f, dt);
    let flags = crate::fighter::control::attacking_flags(w, f);
    let attacking = crate::fighter::is_attacking(w, f);
    if w.fighter(f).air.was_attacking && !attacking {
        check_lateral_friction(w, f); // AttackEnd
    }
    w.fighter_mut(f).air.was_attacking = attacking;
    jump_timers(w, f, attacking);
    fast_fall_tick(w, f, attacking, true);
    let pvy = w.fighter(f).platforms.pawn_vy;
    body::handle_platform_collision(w, f, pvy);
    // The double tap's drop (DownPressCount): the platform under the fighter is ignored from this move on.
    let fi = w.fighter(f);
    if fi.body.on_floor
        && let Some(b) = fi.platforms.base
        && ignores_platform(w, f, b)
    {
        w.fighter_mut(f).platforms.walk_drop = true;
        set_grounded(w, f, false);
    }
    let drop = w.fighter(f).platforms.walk_drop;
    // PerformMovement's anim root motion step (1429385a0, after HandlePendingLaunch).
    let mut rm: Rm = None;
    if crate::fighter::anim::root_montage(w, f).is_some() {
        crate::fighter::tick::cmc_pose_tick(w, f, 1.0);
        let fi = w.fighter(f);
        let g = fi.body.on_floor;
        let falling = !g && !drop && !fi.hit.kb_mode && !fi.moves.flying;
        if crate::fighter::anim::consume_root(w, f, dt, g || drop, falling, false) {
            let b = &w.fighter(f).body;
            rm = Some((b.vx, b.vy));
        }
    }
    mode_physics(w, f, dt, force, &flags, drop, rm);
}

/// `noCollisionMove`: no collision, so no floor (Falling) and nothing to hit.
pub fn no_collision_move(w: &mut World, f: FighterId, dt: Real) {
    set_grounded(w, f, false);
    let fi = w.fighter(f);
    let mut vy = fi.body.vy - GRAVITY_Z * fi.moves.gravity_scale * dt;
    vy = cap_fall(w, f, vy);
    let fx = w.fighter_mut(f);
    fx.body.x += fx.body.vx * dt;
    fx.body.y += 0.5 * (fx.body.vy + vy) * dt;
    fx.body.vy = vy;
}

/// `modePhysics`: StartNewPhysics and what follows it in `physics`.
fn mode_physics(
    w: &mut World,
    f: FighterId,
    dt: Real,
    force: Option<(Real, Real)>,
    flags: &[NotifyRef],
    drop: bool,
    rm: Rm,
) {
    if w.fighter(f).capsule.cap_resized && !w.fighter(f).moves.flying && !body::no_collision(w, f) {
        resolve_resize(w, f);
    }
    if body::no_collision(w, f) {
        no_collision_move(w, f, dt);
        return;
    }
    if let Some((fvx, fvy)) = force {
        // Unverified: an override-velocity source next to anim root motion is not read; the source moves here.
        if rm.is_some() {
            crate::fighter::common::warn_once(w, crate::name!("force move under anim root motion"));
        }
        // MOVE_Flying: PhysFlying with the override velocity, one move a frame, no floor and no landing.
        if w.fighter(f).moves.flying {
            let fx = w.fighter_mut(f);
            fx.body.vx = fvx;
            fx.body.vy = fvy;
            phys_flying(w, f, dt, true);
            return;
        }
        phys_force_move(w, f, dt, fvx, fvy);
        return;
    }
    wall_slide_check(w, f, flags);

    // The CMC's acceleration is the previous pawn tick's movement input.
    let mx = w.fighter(f).air.move_in;
    // PhysWalking: 2 ticks of 1/120 s, each CalcVelocity then a move by its own velocity; each substep ends with
    // FindFloor; without a walkable floor CheckFall -> StartFalling with the time left.
    let mut walk_dx = 0.0;
    let mut lost_at: Real = -1.0;
    let mut lost_dx = 0.0;
    let mut lost_d: Real = 0.0;
    let mut lost_v = 0.0;
    if w.fighter(f).body.on_floor || drop {
        let b = w.fighter(f).platforms.base;
        let mut s = 0.0;
        while s < SUBSTEPS {
            let x = w.fighter(f).body.x + walk_dx;
            let d = if rm.is_some() {
                root_walk_delta(w, f, x, dt / SUBSTEPS)
            } else {
                walk_delta(w, f, x, dt / SUBSTEPS, false)
            };
            walk_dx += d;
            if s == SUBSTEPS - 1.0 {
                body::set_walk_delta(w, f, d); // bZeroDelta of the floor check after the move
            }
            if lost_at < 0.0 {
                let wx = w.fighter(f).body.x + walk_dx;
                if drop || off_floor(w, f, b, wx) {
                    lost_at = s;
                    lost_dx = walk_dx;
                    lost_d = d;
                    lost_v = w.fighter(f).body.vx;
                }
            }
            s += 1.0;
        }
    }

    if w.fighter(f).moves.flying {
        phys_flying(w, f, dt, rm.is_some());
        return;
    }
    w.fighter_mut(f).moves.fly_input = None; // consumed by this tick
    // bCanWalkOffLedges false: the substep that left the floor takes PhysWalking's ledge branch (`hang`).
    // Deviation: GetLedgeMove's side step, the retry's own CalcVelocity and a second crossing in it are not modelled:
    // the port goes straight to the failed revert.
    let grounded = w.fighter(f).body.on_floor;
    let ledge = !drop && grounded && lost_at >= 0.0 && !can_walk_off_ledges(w, f);
    let hang = ledge && lost_d.abs() >= 1e-4;
    w.fighter_mut(f).body.walk_reverted = false;
    if drop || (grounded && lost_at >= 0.0 && (if ledge { !hang } else { lost_at < SUBSTEPS - 1.0 })) {
        // StartFalling inside PhysWalking. Off a ledge the fighter keeps the ground jump and takes a moving base's
        // velocity, as in collide.
        let fi = w.fighter(f);
        let b = if fi.body.on_floor { fi.platforms.base } else { fi.platforms.walk_base };
        let fx = w.fighter_mut(f);
        fx.body.vx = lost_v;
        fx.body.vy = 0.0;
        if let Some(b) = b {
            impart_base(w, f, b);
        }
        set_grounded(w, f, false);
        if lost_d.abs() >= 1e-4 {
            w.fighter_mut(f).body.x += lost_dx;
            phys_falling_rm(w, f, dt / SUBSTEPS, lost_at + 1.0, mx, rm);
        }
    } else if hang {
        let fx = w.fighter_mut(f);
        fx.body.x += lost_dx - lost_d; // RevertMove to the start of the substep that left the floor
        fx.body.vx = 0.0;
    } else if grounded {
        w.fighter_mut(f).body.x += walk_dx;
    } else if w.fighter(f).hit.kb_mode {
        // Still in the knockback mode after the lockout: the stick's acceleration feeds PhysKnockback.
        match rm {
            Some(rv) => phys_root_knockback(w, f, dt, rv),
            None => {
                phys_knockback(w, f, dt, mx, false);
            }
        }
    } else {
        // Stock PhysFalling, 2 substeps of 1/120 s, accelerating with the previous pawn tick's input.
        phys_falling_rm(w, f, dt / SUBSTEPS, 0.0, mx, rm);
    }
    collide(w, f, false, None);
    // A walker under root motion keeps the root-motion velocity whatever the move made of it.
    if let Some(rv) = rm
        && w.fighter(f).body.on_floor
        && !hang
        && !w.fighter(f).body.walk_reverted
    {
        w.fighter_mut(f).body.vx = rv.0;
    }
}

/// `physFalling`: PhysFalling's substeps from index s0 on, each of h.
pub fn phys_falling(w: &mut World, f: FighterId, h: Real, s0: Real, mx: Real) {
    phys_falling_rm(w, f, h, s0, mx, None);
}

fn phys_falling_rm(w: &mut World, f: FighterId, h: Real, s0: Real, mx: Real, rm: Rm) {
    let ax = falling_lateral_accel(w, f, mx);
    let g = GRAVITY_Z * w.fighter(f).moves.gravity_scale;
    let mut s = s0;
    while s < SUBSTEPS {
        let b = &w.fighter(f).body;
        let (x0, y0, vx0, vy0) = (b.x, b.y, b.vx, b.vy);
        if rm.is_none() {
            falling_lateral(w, f, h, ax);
        }
        let mut vy1 = vy0 - g * h; // NewFallVelocity
        vy1 = cap_fall(w, f, vy1);
        let fx = w.fighter_mut(f);
        fx.body.vy = vy1;
        if let Some(rv) = rm {
            fx.body.vx = rv.0;
        }
        let vx = fx.body.vx;
        // midpoint
        if fall_move_rm(
            w,
            f,
            x0,
            y0,
            0.5 * (vx0 + vx) * h,
            0.5 * (vy0 + vy1) * h,
            h,
            s,
            false,
            (SUBSTEPS - s - 1.0) * h,
            rm,
        ) {
            break;
        }
        s += 1.0;
    }
}

/// `pausedMove`: the CMC's tick under the hitpause dilation (one tick of DT x 0.005).
pub fn paused_move(w: &mut World, f: FighterId, dt: Real) {
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    if w.fighter(f).capsule.cap_resized && !w.fighter(f).moves.flying {
        resolve_resize(w, f);
    }
    if !crate::fighter::in_hitstun(w, f) && crate::fighter::anim::root_montage(w, f).is_some() {
        crate::fighter::tick::cmc_pose_tick(w, f, HITPAUSE_DILATION);
        let fi = w.fighter(f);
        let g = fi.body.on_floor;
        let falling = !g && !fi.hit.kb_mode && !fi.moves.flying;
        let rm = if crate::fighter::anim::consume_root(w, f, dt, g, falling, true) {
            let b = &w.fighter(f).body;
            Some((b.vx, b.vy))
        } else {
            None
        };
        paused_mode_move(w, f, dt, rm);
        return;
    }
    paused_mode_move(w, f, dt, None);
}

fn paused_mode_move(w: &mut World, f: FighterId, dt: Real, rm: Rm) {
    if w.fighter(f).moves.flying {
        phys_flying(w, f, dt, rm.is_some());
        return;
    }
    if crate::fighter::in_hitstun(w, f) {
        if w.fighter(f).body.on_floor {
            let x = w.fighter(f).body.x;
            let d = walk_delta(w, f, x, dt, true);
            ledge_step(w, f, d);
            return;
        }
        if w.fighter(f).hit.kb_mode {
            phys_knockback(w, f, dt, 0.0, true);
        } else {
            let fi = w.fighter(f);
            let vy0 = fi.body.vy;
            let mut vy1 = vy0 - GRAVITY_Z * fi.moves.gravity_scale * dt;
            vy1 = cap_fall(w, f, vy1);
            let fx = w.fighter_mut(f);
            fx.body.vy = vy1;
            let (x, y, vx) = (fx.body.x, fx.body.y, fx.body.vx);
            fall_move(w, f, x, y, vx * dt, 0.5 * (vy0 + vy1) * dt, dt, SUBSTEPS - 1.0, true, 0.0);
        }
        // A floor hit inside the pause lands (BFBounce skips a fighter in hitpause); so does a landing on the
        // block's edge (2-4), and an edge hit that slid on (5-7) tests nothing. The hit's box still gets its
        // OnComponentHit, the normal of the code as collide takes it.
        let sw = w.fighter(f).body.floor_swept;
        if sw >= 2.0 {
            w.fighter_mut(f).body.floor_swept = 0.0;
            if sw <= 4.0 {
                hitstun_landed(w, f);
            }
            let nx = if sw == 3.0 || sw == 6.0 {
                1.0
            } else if sw == 4.0 || sw == 7.0 {
                -1.0
            } else {
                0.0
            };
            let sp = w.fighter(f).platforms.swept_hit.flatten();
            solid_hit(w, f, sp, nx, if nx == 0.0 { 1.0 } else { 0.0 });
        }
        return;
    }
    if w.fighter(f).body.on_floor {
        let x = w.fighter(f).body.x;
        let d = if rm.is_some() { root_walk_delta(w, f, x, dt) } else { walk_delta(w, f, x, dt, false) };
        ledge_step(w, f, d);
        return;
    }
    if rm.is_some() && w.fighter(f).hit.kb_mode {
        let b = &w.fighter(f).body;
        let (x, y, vx, vy) = (b.x, b.y, b.vx, b.vy);
        fall_move_rm(w, f, x, y, vx * dt, vy * dt, dt, SUBSTEPS - 1.0, false, 0.0, rm);
        return;
    }
    let b = &w.fighter(f).body;
    let (x0, y0, vx0, vy0) = (b.x, b.y, b.vx, b.vy);
    if rm.is_none() {
        let mi = w.fighter(f).air.move_in;
        let a = falling_lateral_accel(w, f, mi);
        falling_lateral(w, f, dt, a);
    }
    let mut vy1 = vy0 - GRAVITY_Z * w.fighter(f).moves.gravity_scale * dt;
    vy1 = cap_fall(w, f, vy1);
    let fx = w.fighter_mut(f);
    fx.body.vy = vy1;
    if let Some(rv) = rm {
        fx.body.vx = rv.0;
    }
    let vx = fx.body.vx;
    fall_move_rm(w, f, x0, y0, 0.5 * (vx0 + vx) * dt, 0.5 * (vy0 + vy1) * dt, dt, SUBSTEPS - 1.0, false, 0.0, rm);
}

/// `fallingLateralAccel`: GetFallingLateralAcceleration (MaxAccelerationAir x AirControl, boosted to at most 1
/// below AirControlBoostVelocityThreshold, clamped to GetMaxAcceleration).
pub fn falling_lateral_accel(w: &World, f: FighterId, x: Real) -> Real {
    let fi = w.fighter(f);
    let mut ctl = fi.air.air_control;
    if ctl != 0.0 && fi.body.vx.abs() < air_boost_threshold(w, f) {
        ctl = min(1.0, fi.air.air_boost_mult * ctl);
    }
    let a = max_accel_air(w, f); // FallingAccelerationMultiplier x BaseAirAcceleration
    max(-a, min(a, x * a * ctl))
}

/// `fallingLateral`: PhysFalling's lateral step: CalcVelocity with Friction = FallingLateralFriction, MaxSpeed =
/// MaxWalkSpeed and ApplyVelocityBraking.
pub fn falling_lateral(w: &mut World, f: FighterId, h: Real, aa: Real) {
    let ff = w.fighter(f).air.fall_lat_friction;
    let b = braking_decel_falling(w, f);
    let v = w.fighter(f).body.vx;
    let mx = crate::fighter::max_walk(w, f);
    let over = v * v > mx * mx * 1.01;
    let mut nv = v;
    if aa == 0.0 || over {
        let fr = ff * braking_friction_factor(w, f);
        if (fr != 0.0 || b != 0.0) && nv != 0.0 {
            nv -= (fr * nv + b * sign(nv)) * h;
            if nv * v <= 0.0 || (b != 0.0 && nv.abs() <= BRAKE_TO_STOP_VELOCITY) {
                nv = 0.0;
            }
        }
        if over && nv * nv < mx * mx && aa * v > 0.0 {
            nv = sign(v) * mx;
        }
    } else {
        let dir = sign(aa);
        nv -= (nv - dir * nv.abs()) * min(h * ff, 1.0);
    }
    if aa != 0.0 {
        let lim = if nv * nv > mx * mx * 1.01 { nv.abs() } else { mx }; // MaxInputSpeed
        nv += aa * h;
        if nv.abs() > lim {
            nv = sign(nv) * lim;
        }
    }
    w.fighter_mut(f).body.vx = nv;
}

/// `floorSweep`: PhysFalling's sweep against the floor block's top: the hit time after PullBackHit, or -1 for no
/// hit. The pass-through platforms are swept the same way and met first (`swept_base`); the spawned pieces too.
// Unverified: a capsule that starts up to 10 into the Box hits at time 0 and is put on the top; the engine's
// depenetration is not read.
// Deviation: a piece that rose into the capsule is met on the flat top alone.
pub fn floor_sweep(w: &mut World, f: FighterId, x0: Real, y0: Real, dx: Real, dy: Real) -> Real {
    w.fighter_mut(f).platforms.swept_base = None;
    if dy >= 0.0 {
        return -1.0;
    }
    let half = cap_half(w, f);
    let mut swept: Option<Plat> = None;
    let mut raw: Real = -1.0;
    for &i in &w.fighter(f).platforms.stage_platforms {
        let pr = PlatformRef::Stage(i);
        let p = plat(w, pr);
        if ignores_platform(w, f, pr) || swept.is_some_and(|s| p.top < s.top) {
            continue;
        }
        let py = p.top + half;
        if y0 < py - 10.0 || y0 + dy > py {
            continue;
        }
        let t = max(0.0, (y0 - py) / -dy);
        let xh = x0 + dx * t;
        if xh < p.left || xh > p.right {
            continue;
        }
        swept = Some(p);
        raw = t;
    }
    if swept.is_none() {
        let sy = stand_y_of(w, f) - rest_gap(w, f);
        if y0 >= sy - 1.0 && y0 + dy <= sy {
            let t = max(0.0, (y0 - sy) / -dy);
            let xh = x0 + dx * t;
            let g = &w.stage.ground;
            if xh >= crate::math::narrow(g.left) && xh <= crate::math::narrow(g.right) {
                raw = t;
            }
        }
    }
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if p.dead || ignores_platform(w, f, pr) {
            continue;
        }
        let py = p.top + half;
        if y0 < py - p.dy - 1.0 {
            continue;
        }
        let ys = if y0 < py { py + 0.125 } else { y0 };
        if ys + dy > py {
            continue;
        }
        let t = (ys - py) / -dy;
        if (raw >= 0.0 && t >= raw) || span_at(w, &p, x0 + dx * t).is_none() {
            continue;
        }
        swept = Some(p);
        raw = t;
    }
    w.fighter_mut(f).platforms.swept_base = swept.map(|p| p.r);
    if raw < 0.0 {
        return -1.0;
    }
    let dist = ue::size2d([dx, dy]);
    min(max(raw - pull_back(dist), 0.0), 1.0)
}

/// `fallMove`: substep s's move by (dx, dy) from (x0, y0). A floor hit runs ProcessLanded
/// and hands the substep's unused time and the substeps left to PhysWalking. Returns true on a hit. `rest`: its
/// default is `(SUBSTEPS - s - 1) * h`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn fall_move(
    w: &mut World,
    f: FighterId,
    x0: Real,
    y0: Real,
    dx: Real,
    dy: Real,
    h: Real,
    s: Real,
    hitstun: bool,
    rest: Real,
) -> bool {
    fall_move_rm(w, f, x0, y0, dx, dy, h, s, hitstun, rest, None)
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn fall_move_rm(
    w: &mut World,
    f: FighterId,
    x0: Real,
    y0: Real,
    dx: Real,
    dy: Real,
    h: Real,
    s: Real,
    hitstun: bool,
    rest: Real,
    rm: Rm,
) -> bool {
    let t = floor_sweep(w, f, x0, y0, dx, dy);
    if let Some(e) = edge_sweep(w, f, x0, y0, dx, dy) {
        let dist = ue::size2d([dx, dy]);
        let te = min(max(e.k - pull_back(dist), 0.0), 1.0);
        if t < 0.0 || te < t {
            return edge_hit(w, f, x0, y0, dx, dy, &e, te, h, s, hitstun, rest, rm);
        }
    }
    if t < 0.0 {
        let fx = w.fighter_mut(f);
        fx.body.x = x0 + dx;
        fx.body.y = y0 + dy;
        return false;
    }
    let fx = w.fighter_mut(f);
    fx.body.x = x0 + dx * t;
    if let Some(b) = fx.platforms.swept_base {
        fx.platforms.base = Some(b);
    }
    fx.platforms.swept_base = None;
    let sy = stand_y_of(w, f);
    let fx = w.fighter_mut(f);
    fx.body.y = sy;
    fx.body.floor_swept = if hitstun { 2.0 } else { 1.0 };
    fx.platforms.swept_hit = Some(fx.platforms.base);
    if !hitstun && !(bounce_free(w, f) && try_bounce(w, f, 0.0, 1.0, true, 1.0)) {
        land(w, f);
    }
    if !hitstun {
        let b = w.fighter(f).platforms.base;
        solid_hit(w, f, b, 0.0, 1.0); // the box's OnComponentHit (in hitstun: collide, after the bounce test)
    }
    w.fighter_mut(f).body.vy = 0.0;
    walk_for_rm(w, f, h * (1.0 - t) + rest, s + 1.0, !hitstun, rm);
    true
}

/// The first edge contact of a move (`EDGE_BEST`, `EDGE_OUT`, `EDGE_BOX`, `EDGE_BASE`).
#[derive(Clone, Copy)]
struct EdgeContact {
    /// The raw time (0..1, before the pull-back).
    k: Real,
    /// The edge's outward side (+1 right, -1 left).
    out: Real,
    g: Ground,
    /// The base a landing on it takes (`None`: World.ground).
    base: Option<PlatformRef>,
}

/// `edgeSweep`: the stage block's top edges: the first contact of the capsule's lower hemisphere with a box's top
/// corner, over the walking boxes' ledge corners, the pass-through platforms and the spawned boxes.
// Deviation: a ship is swept by one run of pieces alone, the first from the left within reach of the move's start; a
// run's inner gaps are no corners.
fn edge_sweep(w: &World, f: FighterId, x0: Real, y0: Real, dx: Real, dy: Real) -> Option<EdgeContact> {
    let r = cap_radius(w, f);
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    let mut best: Option<EdgeContact> = None;
    let ground = w.stage.ground;
    edge_corners(w, f, &ground, None, true, x0, y0, dx, dy, &mut best);
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if p.stage_solid && !p.dead && !ignores_platform(w, f, pr) {
            edge_corners(w, f, &p.ground(), Some(pr), true, x0, y0, dx, dy, &mut best);
        }
    }
    for &i in &w.fighter(f).platforms.stage_platforms {
        let pr = PlatformRef::Stage(i);
        if !ignores_platform(w, f, pr) {
            edge_corners(w, f, &plat(w, pr).ground(), Some(pr), false, x0, y0, dx, dy, &mut best);
        }
    }
    for &pr in &w.fighter(f).platforms.platforms {
        let p = plat(w, pr);
        if p.dead || p.stage_solid || p.poly || ignores_platform(w, f, pr) {
            continue;
        }
        let Some(s) = span_near(w, &p, x0, r + dx.abs()) else { continue };
        let run = Ground {
            left: f64::from(s.left),
            right: f64::from(s.right),
            top: f64::from(p.top),
            bottom: f64::from(p.bottom),
        };
        edge_corners(w, f, &run, Some(pr), false, x0, y0, dx, dy, &mut best);
    }
    best
}

/// `edgeCorners`: the two top corners of box g against the move; ledge: only a corner no other walking box holds.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn edge_corners(
    w: &World,
    f: FighterId,
    g: &Ground,
    base: Option<PlatformRef>,
    ledge: bool,
    x0: Real,
    y0: Real,
    dx: Real,
    dy: Real,
    best: &mut Option<EdgeContact>,
) {
    let r = cap_radius(w, f);
    let qa = dx * dx + dy * dy;
    let gb = Bounds::of(g);
    let ey = gb.top + cap_half(w, f) - r;
    for out in [-1.0, 1.0] {
        let ex = if out > 0.0 { gb.right } else { gb.left };
        let ox = x0 - ex;
        let oy = y0 - ey;
        let qb = 2.0 * (ox * dx + oy * dy);
        let qc = ox * ox + oy * oy - r * r;
        if qb >= 0.0 || qc < 0.0 {
            continue;
        }
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            continue;
        }
        let k = (-qb - disc.sqrt()) / (2.0 * qa);
        if k > 1.0 || best.is_some_and(|b| k >= b.k) {
            continue;
        }
        if (x0 + dx * k - ex) * out <= 0.0 || y0 + dy * k <= ey {
            continue;
        }
        if ledge && !ledge_corner(w, f, g, ex) {
            continue;
        }
        *best = Some(EdgeContact { k, out, g: *g, base });
    }
}

/// `edgeHit`: a falling or knockback move's hit on the block's top edge at raw time e (te after PullBackHit), read
/// from PhysFalling 1429390b0 / PhysKnockback 1410d8100.
// Unverified: the slide's own rules are from the 4.26 source, not read in the exe (HandleSlopeBoosting, Velocity =
// slide / remaining time); the slide's own hit is not modelled.
// Deviation: ProcessLanded leaves the capsule resting on the corner and the next walking tick lifts it to the flat
// height; here he stands flat at once. The build defers every Hit event of the move to the end of PerformMovement;
// the port tests one hit per move, the last one fall_move met.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn edge_hit(
    w: &mut World,
    f: FighterId,
    x0: Real,
    y0: Real,
    dx: Real,
    dy: Real,
    e: &EdgeContact,
    te: Real,
    h: Real,
    s: Real,
    hitstun: bool,
    rest: Real,
    rm: Rm,
) -> bool {
    let (g, base, out) = (e.g, e.base, e.out);
    let r = cap_radius(w, f);
    let straight = cap_half(w, f) - r; // the hemisphere's centre over the capsule's bottom
    let gb = Bounds::of(&g);
    let ex = if out > 0.0 { gb.right } else { gb.left };
    let cx = x0 + dx * e.k - ex;
    let cy = y0 + dy * e.k - (gb.top + straight);
    let nx = cx / r; // Hit.Normal
    let ny = cy / r;
    let d = cx.abs();
    let top = dy <= dx * out; // FindBoxOpposingNormal: the face with the least dot with the delta
    let (bnx, bny) = if top { (0.0, 1.0) } else { (out, 0.0) }; // ImpactNormal
    let tol = max(r - 0.15, 0.1501);
    let b2 = r - 0.1 * (capsule_half(w, f) - r); // ComputeFloorDist's ShrinkHeight
    let lands = d < tol && (top || (ny > 1e-4 && d * d < r * r - b2 * b2));
    let fx = w.fighter_mut(f);
    fx.body.x = x0 + dx * te;
    fx.body.y = y0 + dy * te;
    fx.platforms.swept_hit = Some(base);
    if lands {
        fx.platforms.base = base; // the box whose edge he lands on (None: World.ground)
        let sy = stand_y_of(w, f);
        let fx = w.fighter_mut(f);
        fx.body.y = sy;
        fx.body.floor_swept = if hitstun {
            if top {
                2.0
            } else if out > 0.0 {
                3.0
            } else {
                4.0
            }
        } else {
            1.0
        };
        if !hitstun {
            if top {
                if !(bounce_free(w, f) && try_bounce(w, f, bnx, bny, true, 1.0)) {
                    land(w, f);
                }
                let b = w.fighter(f).platforms.base;
                solid_hit(w, f, b, bnx, bny);
            } else {
                land(w, f);
                let b = bounce_free(w, f) && try_bounce(w, f, bnx, bny, true, 1.0); // the wall test after the landing
                solid_hit(w, f, base, bnx, bny);
                if b {
                    return true;
                }
            }
        }
        w.fighter_mut(f).body.vy = 0.0;
        walk_for_rm(w, f, h * (1.0 - te) + rest, s + 1.0, !hitstun, rm);
        return true;
    }
    let k = 1.0 - te;
    let mut sx = dx * k;
    let mut sy = dy * k;
    let dn = sx * nx + sy * ny;
    sx -= dn * nx;
    sy -= dn * ny;
    let zl = dy * k;
    if sy > 0.0 && sy - zl > 1e-4 {
        if zl > 0.0 {
            let u = zl / sy;
            sx *= u;
            sy *= u;
        } else {
            sx = 0.0;
            sy = 0.0;
        }
    }
    let rem = h * k;
    let fx = w.fighter_mut(f);
    if rem > 1e-4 {
        if rm.is_none() {
            fx.body.vx = sx / rem;
        }
        fx.body.vy = sy / rem;
    }
    if rem > 1e-4 && sx * dx + sy * dy > 0.0 {
        fx.body.x += sx;
        fx.body.y += sy;
    }
    if hitstun {
        fx.body.floor_swept = if top {
            5.0
        } else if out > 0.0 {
            6.0
        } else {
            7.0
        };
    } else {
        if bounce_free(w, f) {
            try_bounce(w, f, bnx, bny, false, 1.0);
        }
        solid_hit(w, f, base, bnx, bny);
    }
    false
}

/// `stageSolidHit(f, p, ox, oy)` on a base: a stage solid's box or polygon alone has a hit owner.
fn solid_hit(w: &mut World, f: FighterId, p: Option<PlatformRef>, ox: Real, oy: Real) {
    match p {
        Some(PlatformRef::Solid(i)) => crate::world::stage::stage_solid_box_hit(w, f, i, ox, oy),
        Some(PlatformRef::Poly(i)) => crate::world::stage::stage_solid_hit(w, f, Some(i), ox, oy),
        _ => {}
    }
}

/// `offFloor`: the walker leaves his floor at x on base b.
pub fn off_floor(w: &World, f: FighterId, b: Option<PlatformRef>, x: Real) -> bool {
    floor_holder(w, f, b, x).is_none()
}

/// `ledgeStep`: a grounded move by d that has no walk loop of its own: with
/// bCanWalkOffLedges false, a delta that leaves the floor takes the ledge branch (RevertMove(bFailMove), Velocity 0);
/// returns true then.
pub fn ledge_step(w: &mut World, f: FighterId, d: Real) -> bool {
    let x = w.fighter(f).body.x + d;
    if !can_walk_off_ledges(w, f) && d.abs() >= 1e-4 {
        let b = w.fighter(f).platforms.base;
        if off_floor(w, f, b, x) {
            w.fighter_mut(f).body.vx = 0.0;
            return true;
        }
    }
    w.fighter_mut(f).body.x = x;
    false
}

/// `walkFor`: StartNewPhysics(remainingTime, Iterations) into PhysWalking after a landing.
// Deviation: the hitstun landing's remainder still leaves the fall to the next frame (collide), and with
// bCanWalkOffLedges false there it is clamped at the frame's end (collide), not reverted in the tick.
pub fn walk_for(w: &mut World, f: FighterId, rem: Real, iterations: Real, fall: bool) {
    walk_for_rm(w, f, rem, iterations, fall, None);
}

fn walk_for_rm(w: &mut World, f: FighterId, mut rem: Real, iterations: Real, fall: bool, rm: Rm) {
    let mut it = iterations;
    while rem >= 1e-6 && it < 16.0 {
        it += 1.0;
        let tt = if rem > MAX_SIM_STEP && it < 16.0 { min(MAX_SIM_STEP, rem * 0.5) } else { rem };
        rem -= tt;
        let x = w.fighter(f).body.x;
        let d = if rm.is_some() { root_walk_delta(w, f, x, tt) } else { walk_delta(w, f, x, tt, false) };
        w.fighter_mut(f).body.x += d;
        if !fall || !w.fighter(f).body.on_floor {
            continue;
        }
        let b = w.fighter(f).platforms.base;
        let x = w.fighter(f).body.x;
        if !off_floor(w, f, b, x) {
            continue;
        }
        if !can_walk_off_ledges(w, f) && d.abs() >= 1e-4 {
            // The ledge branch as in `physics` (`hang`): RevertMove(bFailMove), Velocity 0, remainingTime 0.
            let fx = w.fighter_mut(f);
            fx.body.x -= d;
            fx.body.vx = 0.0;
            fx.body.walk_reverted = true;
            return;
        }
        set_grounded(w, f, false);
        w.fighter_mut(f).body.vy = 0.0; // walking off keeps the ground jump (collide)
        if let Some(b) = b {
            impart_base(w, f, b);
        }
        if d.abs() >= 1e-4 {
            fall_for_rm(w, f, rem, it, rm);
        }
        return;
    }
}

/// `fallFor`: PhysFalling over `rem` from iteration `it`.
pub fn fall_for(w: &mut World, f: FighterId, rem: Real, iterations: Real) {
    fall_for_rm(w, f, rem, iterations, None);
}

fn fall_for_rm(w: &mut World, f: FighterId, mut rem: Real, iterations: Real, rm: Rm) {
    let mi = w.fighter(f).air.move_in;
    let ax = falling_lateral_accel(w, f, mi);
    let g = GRAVITY_Z * w.fighter(f).moves.gravity_scale;
    let mut it = iterations;
    while rem >= 1e-6 && it < 16.0 {
        it += 1.0;
        let h = if rem > MAX_SIM_STEP && it < 16.0 { min(MAX_SIM_STEP, rem * 0.5) } else { rem };
        rem -= h;
        let b = &w.fighter(f).body;
        let (x0, y0, vx0, vy0) = (b.x, b.y, b.vx, b.vy);
        if rm.is_none() {
            falling_lateral(w, f, h, ax);
        }
        let mut vy1 = vy0 - g * h;
        vy1 = cap_fall(w, f, vy1);
        let fx = w.fighter_mut(f);
        fx.body.vy = vy1;
        if let Some(rv) = rm {
            fx.body.vx = rv.0;
        }
        let vx = fx.body.vx;
        if fall_move_rm(w, f, x0, y0, 0.5 * (vx0 + vx) * h, 0.5 * (vy0 + vy1) * h, h, it - 1.0, false, rem, rm) {
            break;
        }
    }
}

/// `walkDelta`: one PhysWalking substep from x over h: CalcVelocity (braking alone for
/// a stunned fighter), the move, and Velocity = (NewLocation - OldLocation) x (1 / timeTick), all float32. Returns
/// the move.
pub fn walk_delta(w: &mut World, f: FighterId, x: Real, h: Real, stunned: bool) -> Real {
    if stunned {
        brake(w, f, h);
    } else {
        walk_vel(w, f, h);
    }
    let tt = h;
    let x0 = x;
    let fx = w.fighter_mut(f);
    let x1 = x0 + (fx.body.vx * tt);
    fx.body.vx = (x1 - x0) * (1.0 / tt);
    x1 - x
}

/// `walkVel`: walking CalcVelocity over dt (stock PhysWalking 14293b840 -> CalcVelocity 1429217d0).
pub fn walk_vel(w: &mut World, f: FighterId, dt: Real) {
    let fi = w.fighter(f);
    let mx = if fi.capsule.cmc_crouched {
        body::def(w, f).max_walk_crouched
    } else {
        f64::from(crate::fighter::max_walk(w, f))
    };
    let v = calc_velocity(
        fi.body.vx,
        fi.air.move_in,
        crate::math::narrow(mx),
        ground_friction(w, f),
        max_accel(w, f),
        walk_braking_friction(w, f),
        braking_decel_walking(w, f),
        dt,
    );
    w.fighter_mut(f).body.vx = v;
}

/// `calcVelocity`: CalcVelocity's lateral part on a velocity v, for any CMC's values.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn calc_velocity(
    v: Real,
    mx: Real,
    max_v: Real,
    friction: Real,
    accel: Real,
    brake_friction: Real,
    brake_decel: Real,
    dt: Real,
) -> Real {
    let mut vx;
    let over = v * v > max_v * max_v * 1.01;
    if mx == 0.0 || over {
        vx = brake_velocity(v, brake_friction, brake_decel, dt);
        if over && vx * vx < max_v * max_v && mx * v > 0.0 {
            vx = sign(v) * max_v;
        }
    } else {
        vx = v - (v - sign(mx) * v.abs()) * min(dt * friction, 1.0);
    }
    if mx != 0.0 {
        let lim = if vx * vx > max_v * max_v * 1.01 { vx.abs() } else { max_v };
        let nv = vx + mx * accel * dt;
        vx = if nv.abs() > lim { sign(nv) * lim } else { nv };
    }
    vx
}

/// `brake`: walking braking: GroundFriction x BrakingFrictionFactor plus BrakingDecelerationWalking.
pub fn brake(w: &mut World, f: FighterId, dt: Real) {
    let v = brake_velocity(w.fighter(f).body.vx, walk_braking_friction(w, f), braking_decel_walking(w, f), dt);
    w.fighter_mut(f).body.vx = v;
}

/// `brakeVelocity`: ApplyVelocityBraking's (14291de00) one step on a velocity v (unchanged when its float32 is 0).
pub fn brake_velocity(vx: Real, fr: Real, b: Real, dt: Real) -> Real {
    let v = vx;
    if v == 0.0 {
        return vx;
    }
    let nv = v + ((-fr * v - b * sign(v)) * dt);
    if nv * v <= 0.0 || nv * nv <= BRAKE_TO_STOP_VELOCITY * BRAKE_TO_STOP_VELOCITY { 0.0 } else { nv }
}

/// `resetJumpCounts`: ResetJump(0).
pub fn reset_jump_counts(w: &mut World, f: FighterId) {
    let fx = w.fighter_mut(f);
    fx.jump.jumps = 0;
    fx.air.air_evades = 0;
    fx.air.air_specials = 0;
    fx.fast_fall.ff_triggered = false;
    crate::fighter::match_side::air_saturation_reset(w, f);
}

/// `resetAirOptions`: BaseFighter BPI_ResetAirOptions @107810-107963.
pub fn reset_air_options(w: &mut World, f: FighterId, jumps: bool, evades: bool, specials: bool) {
    let fx = w.fighter_mut(f);
    if jumps {
        fx.jump.jumps = 0;
    }
    if evades {
        fx.air.air_evades = 0;
    }
    if specials {
        fx.air.air_specials = 0;
    }
    fx.fast_fall.ff_triggered = false;
}

/// Whether the floor's actor is a SpawnedPlatformParent (its box answers `canPassThrough`).
fn base_passes_through(w: &World, f: FighterId) -> bool {
    match w.fighter(f).platforms.base {
        Some(PlatformRef::Spawned(a, i)) => crate::world::spawned_platform::spawned_has_pass_through(w, a, i),
        _ => false,
    }
}

/// `resetAir`: OnLanded and SetJumpData. silent: the respawn's (no OnIsJumping).
pub fn reset_air(w: &mut World, f: FighterId, keep_jump_delays: bool, silent: bool) {
    let fx = w.fighter_mut(f);
    fx.air.air_attack_available = true;
    fx.jump.jumps = 0;
    fx.air.air_evades = 0;
    fx.air.air_specials = 0;
    if !keep_jump_delays {
        fx.jump.jump_frame = -1;
        fx.jump.jump_cut_at = -1;
        fx.jump.quick_fall_at = -1;
    }
    if silent {
        fx.moves.cmd_jumped = false;
    } else {
        crate::fighter::set_jumping(w, f, false);
    }
    let fx = w.fighter_mut(f);
    fx.fast_fall.fast_falling = false;
    fx.fast_fall.ff_triggered = false; // SetJumpData
    fx.jump.ground_jump_tick = -1;
    crate::fighter::match_side::air_saturation_reset(w, f);
    // OnLanded (ubergraph 110894-110959, 113693-113738): ResetWallFatigue only when the floor's actor is not a
    // SpawnedPlatformParent.
    if !base_passes_through(w, f) {
        reset_wall_fatigue(w, f);
    }
}

/// `bounceFree`: CanBounceWithoutHitStun, and not on a paused frame.
pub fn bounce_free(w: &World, f: FighterId) -> bool {
    w.fighter(f).hit.can_bounce_no_hitstun && crate::fighter::mode(w, f) != FrameMode::Paused
}

/// `collide`: stage block collision, landing and walk-off. Returns true on landing.
/// `kb_slide`: the frame's PhysKnockback move without stick, its substeps' end velocity Z x timeTick summed
/// ([`phys_knockback`]'s return), read by `collide_platforms`' start-penetrating side hit.
pub fn collide(w: &mut World, f: FighterId, hitstun: bool, kb_slide: Option<Real>) -> bool {
    let swept = w.fighter(f).body.floor_swept;
    w.fighter_mut(f).body.floor_swept = 0.0;
    // A hitstun move's hit: BFBounce's test along the hit's ImpactNormal, 2-4 a landing, 5-7 an edge hit that slid.
    if swept >= 2.0 {
        let nx = if swept == 3.0 || swept == 6.0 {
            1.0
        } else if swept == 4.0 || swept == 7.0 {
            -1.0
        } else {
            0.0
        };
        let sp = w.fighter(f).platforms.swept_hit.flatten();
        if swept == 3.0 || swept == 4.0 {
            // A side-face landing: ProcessLanded's landing first, then the Hit event's wall test.
            hitstun_landed(w, f);
            if hitstun {
                try_bounce(w, f, nx, 0.0, true, 1.0);
            }
            solid_hit(w, f, sp, nx, 0.0);
            return false; // landed here already
        }
        let bounced = hitstun && try_bounce(w, f, nx, if nx == 0.0 { 1.0 } else { 0.0 }, swept == 2.0, 1.0);
        solid_hit(w, f, sp, nx, if nx == 0.0 { 1.0 } else { 0.0 });
        if swept <= 4.0 {
            if !bounced {
                return true;
            }
            if !w.fighter(f).body.on_floor {
                w.fighter_mut(f).platforms.base = None; // off a platform's top
            }
            return false;
        }
        if bounced {
            return false;
        }
    }
    let g = w.stage.ground;
    let gb = Bounds::of(&g);
    let mut b = w.fighter(f).platforms.base;
    let sy = stand_y_of(w, f);
    let r = cap_radius(w, f);
    let rg = rest_gap(w, f);
    if w.fighter(f).body.on_floor {
        // A walker on another floor has the wall test against the stage block (without it he walks into
        // it and the next move's depenetration pops him out), as in the build's walk.
        if body::walk_out_of_ground(w, f) {
            b = None; // stepped onto the block
        }
        // on a polygon solid: the floor's height at his new place.
        // Deviation: the horizontal speed kept and the height taken at the frame's end.
        if let Some(br) = b {
            let p = plat(w, br);
            if body::walkable_at(w, &p, w.fighter(f).body.x, r) {
                let sy = stand_y_of(w, f);
                w.fighter_mut(f).body.y = sy;
            }
        }
        if !w.fighter(f).platforms.platforms.is_empty() {
            body::walk_out_of_platforms(w, f);
            crate::fighter::traces::walk_out_of_polys(w, f);
        }
        let x = w.fighter(f).body.x;
        if off_floor(w, f, b, x) {
            if !can_walk_off_ledges(w, f) {
                // What the tick's ledge branch did not revert: clamped at the edge (a deviation).
                let cx = clamp_floor(w, f, b, x);
                let fx = w.fighter_mut(f);
                fx.body.x = cx;
                fx.body.vx = 0.0;
            } else {
                set_grounded(w, f, false);
                w.fighter_mut(f).body.vy = 0.0; // walking off keeps the ground jump
                if let Some(b) = b {
                    impart_base(w, f, b); // off a moving base: its velocity
                }
            }
        } else {
            rebase(w, f);
        }
        return false;
    }
    let fi = w.fighter(f);
    let (x, y, prev_y) = (fi.body.x, fi.body.y, fi.body.prev_y);
    if swept == 0.0 && fi.body.vy <= 0.0 && y <= sy - rg && prev_y >= sy - 1.0 && x >= gb.left && x <= gb.right {
        w.fighter_mut(f).body.y = sy;
        if hitstun && try_bounce(w, f, 0.0, 1.0, true, 1.0) {
            return false;
        }
        if !hitstun && !(bounce_free(w, f) && try_bounce(w, f, 0.0, 1.0, true, 1.0)) {
            land(w, f);
        }
        return true;
    }
    let half = cap_half(w, f);
    let low = gb.bottom - half;
    let top = sy - rg; // the block's own top (the walking hover is above it)
    let ey = gb.top + half - r; // the lower hemisphere's centre on the block's top
    if y < top && y > low && x > gb.left - r && x < gb.right + r {
        let ex = if x > gb.right {
            gb.right
        } else if x < gb.left {
            gb.left
        } else {
            Real::NAN
        };
        #[allow(clippy::eq_op, reason = "ex === ex: a NaN test")]
        let has_ex = ex == ex;
        if y > ey && has_ex && prev_y > low {
            // Beside the top edge: a capsule some other move left inside the round is pushed out along the corner's
            // normal, with no hit. Unverified: the push is the port's, no sweep's resolve was read. Only at a ledge.
            let cx = x - ex;
            let cy = y - ey;
            let dd = ue::size2d([cx, cy]);
            if dd < r && dd > 0.0 && ledge_corner(w, f, &g, ex) {
                let fx = w.fighter_mut(f);
                fx.body.x = ex + (cx / dd) * r;
                fx.body.y = ey + (cy / dd) * r;
            }
        } else if prev_y <= low {
            w.fighter_mut(f).body.y = low;
            if (hitstun || bounce_free(w, f)) && try_bounce(w, f, 0.0, -1.0, true, 1.0) {
                return false;
            }
            let fx = w.fighter_mut(f);
            if fx.body.vy > 0.0 {
                fx.body.vy = 0.0;
            }
        } else {
            // the side of the block the capsule is on
            let side = if x < (gb.left + gb.right) / 2.0 { -1.0 } else { 1.0 };
            w.fighter_mut(f).body.x = if side < 0.0 { gb.left - r } else { gb.right + r };
            if (hitstun || bounce_free(w, f)) && try_bounce(w, f, side, 0.0, true, 1.0) {
                return false;
            }
            let fx = w.fighter_mut(f);
            if sign(fx.body.vx) == -side {
                fx.body.vx = 0.0;
            }
        }
    }
    let has_plats = !w.fighter(f).platforms.platforms.is_empty();
    if has_plats && body::collide_platforms(w, f, hitstun, kb_slide) {
        return true;
    }
    if has_plats && crate::fighter::traces::collide_polys(w, f, hitstun) {
        return true; // the stage's tilted solids: true on a landing
    }
    if !w.fighter(f).platforms.stage_platforms.is_empty() && body::collide_stage_platforms(w, f, hitstun) {
        return true;
    }
    false
}

/// `landedOnSolidGround`: OnLanded's OnLandedOnSolidGround broadcast @113707 (a floor
/// whose actor is not a SpawnedPlatformParent): the pawn's binding.
// Unverified: taken at the landing's head in land / hitstun_landed, not in the graph's order among the port's other
// OnLanded steps.
pub fn landed_on_solid_ground(w: &mut World, f: FighterId) {
    if !base_passes_through(w, f)
        && let Some(h) = w.fighter(f).hooks.char.landed_on_solid_ground
    {
        h(w, f);
    }
}

/// `bfLanded`: OnBFLanded @110711 to the buffs that bind it.
// Unverified: sent beside the port's other OnBFLanded binder and at the head of hitstun_landed (bind order not read).
pub fn bf_landed(w: &mut World, f: FighterId) {
    if !w.fighter(f).buffs.buffs.is_empty() {
        buff_signal(w, f, BuffSignal::BfLanded, BuffSignalArg::default());
    }
}

/// `land`: BaseFighter's landed event (ubergraph @110711-113738) with the movement mode
/// change the landing makes.
pub fn land(w: &mut World, f: FighterId) {
    // VFX_SFX_Land returns while CheckHitstunActive or dead (@0-97).
    let block = w.fighter(f).crouch.block_land_reset;
    if w.fighter(f).life.dead <= 0.0 && !crate::fighter::in_hitstun(w, f) && !block {
        crate::fighter::match_side::move_fx(w, f, crate::events::MoveFxKind::Land);
    }
    bf_landed(w, f);
    // Pawn_C017 LandLaunchHover (@19896), bound to OnBFLanded while CanAttackHover's notify runs.
    if crate::chars::iron_giant::giant_land_hover(w, f) {
        net_impulse(w, f, 0.0, 100.0, false, true, 0.0, false, 0.0);
    }
    if !w.fighter(f).hit.kb_mode {
        juggle_landed(w, f);
    }
    set_grounded(w, f, true);
    let fx = w.fighter_mut(f);
    fx.platforms.dropping_down = false; // OnLanded
    fx.platforms.evading_through_platform = false;
    fx.hit.kb_mode = false;
    fx.body.vy = 0.0;
    if fx.fast_fall.fast_falling {
        set_terminal(w, f, true, 0.0, false);
    }
    landed_on_solid_ground(w, f);
    if block {
        w.fighter_mut(f).fast_fall.fast_falling = false; // @128281
        if !base_passes_through(w, f) {
            reset_wall_fatigue(w, f); // as reset_air
        }
        w.fighter_mut(f).air.time_in_air = 0.0;
        fast_fall_landed(w, f);
        return;
    }
    reset_air(w, f, true, false);
    w.fighter_mut(f).air.time_in_air = 0.0;
    // LandMulticast has no caller in the build, so a landing leaves the gates as they are.
    land_loco(w, f);
    fast_fall_landed(w, f);
}

/// `fastFallLanded`: the Falling to Walking mode change with IsFastFalling (BaseFighter
/// @128202-128507): FastFallLanded(FastFallStartLocation.Z - K2_GetActorLocation().Z), float32.
// Deviation: the other ways a fighter goes to Walking with IsFastFalling (a mode set by a move command) are not sent.
pub fn fast_fall_landed(w: &mut World, f: FighterId) {
    let Some(y) = w.fighter(f).fast_fall.fast_fall_start_y else { return };
    fast_fall_end(w, f); // IsFastFalling = false @128281
    if !w.fighter(f).buffs.buffs.is_empty() {
        let h = y - body::actor_y(w, f);
        buff_signal(w, f, BuffSignal::FastFallLanded, BuffSignalArg { height: Some(h), ..Default::default() });
    }
}

/// `landLoco`: OnLanded's dash (BaseFighter ubergraph 112812-113675), behind GetMovementEnabled and GetInputEnabled
/// (@111223-111307).
pub fn land_loco(w: &mut World, f: FighterId) {
    let x = w.fighter(f).controls.inp.x;
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(Math.abs(x) >= 0.3): a NaN returns")]
    if !super::movement_enabled(w, f)
        || crate::fighter::in_hitstun(w, f)
        || crate::fighter::is_attacking(w, f)
        || w.fighter(f).evade.evading
        || !(x.abs() >= 0.3)
    {
        return;
    }
    let vx = w.fighter(f).body.vx;
    if vx.abs() < DASH_IMPULSE || sign(vx) == -sign(x) {
        dash_dance(w, f, true);
    }
}
