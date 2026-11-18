//! A projectile's movement as UProjectileMovementComponent runs it: the bounce passes
//! with their impacts, deflection and slide, the plain hit move, and the swept capsule move against the platform
//! boxes with its way out of a penetration.
//!
//! UProjectileMovementComponent with bShouldBounce, one frame (TickComponent with HandleBlockingHit, HandleImpact,
//! ComputeBounceResult, HandleDeflection and HandleSliding; the frame is one sub-step, MaxSimulationTimeStep
//! 0.05 s). Each pass moves the rest of the frame along ComputeMoveDelta. Without a hit the velocity becomes
//! ComputeVelocity over the pass and the sliding state ends. With a blocking hit the velocity is the one after the
//! part of the pass before the hit (the old one for a hit at time <= 1e-4), the impact bounces it (bounce_impact),
//! the deflection may turn the bounce into a slide (below), and the time after the hit goes back to the frame's
//! rest; MaxSimulationIterations 4 passes, the first impact does not count (BounceAdditionalIterations 1).
//!
//! HandleDeflection: the hit is a repeat when the last pass's hit time was < 1 and this one's is <= 1e-4. The
//! projectile slides when a repeat has the same normal (dot >= 0.999845) or when the bounced velocity's direction is
//! within 0.01 of the surface (a zero velocity counts). A repeat on a surface at 90 degrees or less to the last one
//! keeps only the velocity along both surfaces' common line, which is the depth axis here: none. Otherwise the
//! velocity loses its part along the normal and HandleSliding moves the time after the hit along the surface at
//! once, before gravity is added again: a second hit there is an impact of its own; without one the velocity takes
//! the pass's acceleration, and when that points into the surface only its part along the surface stays and a
//! friction of min(the part into the surface x Friction, the speed) is taken off the speed. PreviousHitTime and
//! PreviousHitNormal are read after the slide's own move (the hit result is shared): 1 and zero after a slide
//! without a second hit.
//!
//! Read in the exe and stock 4.26: TickComponent 1429a0e00, HandleImpact 142993c20, HandleBlockingHit 1429933e0,
//! HandleDeflection 142993540, HandleSliding 142993dd0. HandleBlockingHit ends the frame's loop when the hit's
//! events destroyed the owner (the charge shot's last bounce), when the impact stopped the component, or for a hit
//! that starts in penetration. The pass's move and the slide's are SafeMoveUpdatedComponent (safe_move); a hit that
//! still starts in penetration runs HandleImpact (the bounce and OnProjectileBounce) and ends the frame's loop, with
//! the time left dropped.
//!
//! The OnProjectileBounce handlers run inside HandleImpact (bounce_impact), so inside this loop: the velocity one
//! writes is the member the deflection reads and the next pass starts from. The platforms are read again for each
//! pass and behind each impact: a handler may have written the response. A moving platform carries nothing here.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the build's NaN-keeping negated tests

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::actors::projectile::{Blockers, Projectile, class_p, compute_velocity, fly_on, stop_aim, stop_simulating};
use crate::fighter::traces::solids::PolyBlock;
use crate::math;
use crate::world::World;
use mvs_collision::Ground;
use mvs_collision::capsule_mtd::capsule_mtd;

// Deviation: the contact is found by 10 u samples and bisection against the boxes and the normal of a swept hit is
// the capsule centre's direction from the box (from a polygon solid's nearest point).

/// LimitVelocity (142996e60): with GetMaxSpeed() > 0 the velocity's GetClampedToMaxSize(MaxSpeed), inline (the
/// InvSqrt scale; zero for a max below 1e-4); the play plane's (x, y) is UE (Y, Z).
pub fn limit_velocity(vx: Real, vy: Real, max_speed: Real) -> (Real, Real) {
    if !(max_speed > 0.0) {
        return (vx, vy);
    }
    let v = math::ue::clamped_to_max_size([0.0, vx, vy], max_speed);
    (v[1], v[2])
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a projectile in its move")
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a projectile in its move")
}

/// ComputeVelocity over `dt` from (vx, vy), its result left in avx / avy.
fn cv(w: &mut World, id: ProjectileId, vx: Real, vy: Real, dt: Real) {
    let (ax, ay) = compute_velocity(w, pr(w, id), vx, vy, dt);
    let p = pm(w, id);
    p.avx = ax;
    p.avy = ay;
}

/// `bounceMove`.
pub fn bounce_move(
    w: &mut World,
    id: ProjectileId,
    g0: &Ground,
    platforms: &Blockers,
    ovx: Real,
    ovy: Real,
    host: bool,
) {
    {
        let p = pm(w, id);
        p.vx = ovx;
        p.vy = ovy;
    }
    let mut remaining: Real = 1.0 / 60.0;
    let mut impacts = 0;
    let mut it = 0;
    while remaining >= 1e-6 && it < 4 && !pr(w, id).stopped {
        it += 1;
        let mut ps = platforms.of(pr(w, id));
        let tick = remaining;
        remaining = 0.0;
        let (ox, oy) = {
            let p = pr(w, id);
            (p.vx, p.vy)
        };
        cv(w, id, ox, oy, tick);
        let p = pm(w, id);
        let dx = ox * tick + (p.avx - ox) * 0.5 * tick;
        let dy = oy * tick + (p.avy - oy) * 0.5 * tick;
        p.vx = p.avx;
        p.vy = p.avy;
        turn(p, ox, oy);
        if !safe_move(p, g0, ps, dx, dy) {
            p.prev_hit_time = 1.0;
            p.sliding = false;
            continue;
        }
        let (ht, nx, ny, pen) = (p.hit_t, p.hit_nx, p.hit_ny, p.hit_pen);
        // The end velocity of the part before the hit is only taken when the hit's own events left the velocity as
        // it was before the pass (TickComponent 1429a0e00: Velocity == OldVelocity): the projectile's own (a charge
        // shot's OnComponentHit), then the stage solid's actor's (a Tree Fort tree's bounce).
        let dispatched = !p.no_dispatch;
        let own = p.charge_shot && dispatched && component_hit(w, id, ox, oy);
        let other = dispatched && solid_dispatch(w, id);
        if own || other {
            // kept: UpdateProjectileVelocity's, the tree's
        } else if ht > 1e-4 {
            cv(w, id, ox, oy, tick * ht);
            let p = pm(w, id);
            p.vx = p.avx;
            p.vy = p.avy;
        } else {
            let p = pm(w, id);
            p.vx = ox;
            p.vy = oy;
        }
        impacts += 1;
        let mut sub = tick * (1.0 - ht);
        bounce_impact(w, id, nx, ny, host);
        // HandleBlockingHit 1429933e0: Abort for a destroyed owner, a stopped component or a start-penetrating hit
        // (dead: a hit the handler's hit box made at the impact place destroyed it).
        let p = pr(w, id);
        if p.stopped || p.ended || p.dead || pen {
            break;
        }
        ps = platforms.of(p);
        // HandleDeflection
        let p = pm(w, id);
        let repeat = p.prev_hit_time < 1.0 && ht <= 1e-4;
        let along = p.prev_nx * nx + p.prev_ny * ny;
        // GetSafeNormal(Velocity) . N (the C++ tolerance 1e-8; a zero velocity gives 0).
        let u = math::ue::safe_normal([0.0, p.vx, p.vy], math::ue::SMALL_NUMBER);
        p.sliding = (repeat && along >= 0.999845) || u[1] * nx + u[2] * ny <= 0.01;
        if p.sliding {
            if repeat && along <= 0.0 {
                p.vx = 0.0;
                p.vy = 0.0;
            } else {
                let vn = p.vx * nx + p.vy * ny;
                p.vx -= vn * nx;
                p.vy -= vn * ny;
            }
            // Both branches end with the stop test and the slide of the time left.
            if under_stop_speed(p) {
                stop_simulating(p);
                break;
            }
            if sub > 1e-4 {
                sub = slide_move(w, id, g0, ps, nx, ny, sub, host);
            }
            let p = pr(w, id);
            if sub < 0.0 || p.stopped || p.ended {
                break;
            }
        }
        let p = pm(w, id);
        p.prev_hit_time = p.hit_t;
        p.prev_nx = p.hit_nx;
        p.prev_ny = p.hit_ny;
        if sub >= 1e-6 {
            remaining += sub;
            if impacts <= 1 {
                it -= 1;
            }
        }
    }
}

/// HandleSliding, the time left after a hit moved along the surface
/// (normal nx, ny). Returns the time still left, or -1 when its own hit ends the frame's loop (HandleBlockingHit's
/// Abort).
#[allow(clippy::too_many_arguments, reason = "the signature the call site fills")]
pub fn slide_move(
    w: &mut World,
    id: ProjectileId,
    g0: &Ground,
    ps: Shapes,
    nx: Real,
    ny: Real,
    sub: Real,
    host: bool,
) -> Real {
    let p = pm(w, id);
    let (mx, my) = (p.vx * sub, p.vy * sub);
    if safe_move(p, g0, ps, mx, my) {
        let (ht, pen) = (p.hit_t, p.hit_pen);
        let dispatched = !p.no_dispatch;
        if p.charge_shot && dispatched {
            let (vx, vy) = (p.vx, p.vy);
            component_hit(w, id, vx, vy);
        }
        if dispatched {
            solid_dispatch(w, id);
        }
        let p = pr(w, id);
        let (hnx, hny) = (p.hit_nx, p.hit_ny);
        bounce_impact(w, id, hnx, hny, host);
        let p = pr(w, id);
        return if p.stopped || p.ended || p.dead || pen { -1.0 } else { sub * (1.0 - ht) };
    }
    let (vx, vy) = (p.vx, p.vy);
    cv(w, id, vx, vy, sub);
    let p = pm(w, id);
    let fx = p.avx - p.vx;
    let fy = p.avy - p.vy;
    let f_n = fx * nx + fy * ny;
    if !(f_n >= 0.0) {
        // HandleSliding 142993dd0: T = (dv - N dn) + V; the friction min(-(dn Friction), |T|) taken off along
        // T.GetSafeNormal() (tolerance 1e-8, the == 1 shortcut).
        let wx = (fx - nx * f_n) + p.vx;
        let wy = (fy - ny * f_n) + p.vy;
        let ws = math::ue::size2d([wx, wy]);
        let fm = -(f_n * p.friction);
        let fr = if fm <= ws { fm } else { ws };
        let u = math::ue::safe_normal([0.0, wx, wy], math::ue::SMALL_NUMBER);
        p.vx = wx - u[1] * fr;
        p.vy = wy - u[2] * fr;
    } else {
        p.vx = p.avx;
        p.vy = p.avy;
    }
    if under_stop_speed(p) {
        stop_simulating(p);
    }
    0.0
}

/// HandleImpact with bShouldBounce. ComputeBounceResult, for a
/// velocity that does not point away from the surface: the part along the normal is turned around times
/// Bounciness and the part along the surface is scaled by 1 - Friction; while sliding (or with
/// bBounceAngleAffectsFriction) the friction is first scaled by clamp(normal speed / surface speed,
/// MinFrictionFraction, 1). Then OnProjectileBounce (whatever the velocity), LimitVelocity and the stop test.
/// HandleImpact 142993c20: OldVelocity = Velocity; Velocity = ComputeBounceResult;
/// OnProjectileBounce.Broadcast(Hit, OldVelocity), synchronous; Velocity = LimitVelocity(Velocity), read from the
/// member again, so a handler's write is kept and only clamped; then the stop test on it. The handlers here: the
/// classes' this file still holds (BounceEvent, OnBounce), then the class's kind (`ProjectileHooks::bounce_impact`)
/// with the hit's normal and OldVelocity. A handler that destroys the actor ends the move (ended).
pub fn bounce_impact(w: &mut World, id: ProjectileId, nx: Real, ny: Real, host: bool) {
    let bounce_max = math_or(class_p(w, pr(w, id)), "BounceMaxCount", 0.0);
    let p = pm(w, id);
    let ivx = p.vx;
    let ivy = p.vy;
    if !p.bounced {
        p.impact_vx = p.vx;
        p.impact_vy = p.vy;
        p.impact_x = p.x;
        p.impact_y = p.y;
    }
    let vn = p.vx * nx + p.vy * ny;
    if !(vn > 0.0) {
        // ComputeBounceResult 14298c4e0: P = N (-d), T = P + V, the result (k T) + (B P); the friction fraction is a
        // true divide by sqrtss(|T|^2), a NaN or one below MinFrictionFraction taking the minimum.
        let (px, py) = (nx * -vn, ny * -vn);
        let tx = px + p.vx;
        let ty = py + p.vy;
        let mut fr = p.friction;
        if p.angle_friction || p.sliding {
            let q = -(vn / math::ue::size2d([tx, ty]));
            fr *= if !(q >= p.min_friction) {
                p.min_friction
            } else if q < 1.0 {
                q
            } else {
                1.0
            };
        }
        let k = 1.0 - fr;
        let keep = if !(k >= 0.0) {
            0.0
        } else if k < 1.0 {
            k
        } else {
            1.0
        };
        let b = if p.bounce > 0.0 { p.bounce } else { 0.0 };
        p.vx = keep * tx + b * px;
        p.vy = keep * ty + b * py;
    }
    p.bounced = true;
    // C019BlasterChargeProjectile BounceEvent (bound to OnProjectileBounce, ubergraph 5557): BounceCount + 1 >
    // BounceMaxCount -> K2_DestroyActor; HandleBlockingHit then aborts the frame's loop (the owner is pending kill).
    if p.charge_shot {
        p.bounce_count += 1;
        if crate::real(p.bounce_count) > bounce_max {
            p.ended = true;
        }
    }
    if p.die_on_bounce {
        p.ended = true;
    }
    if host
        && let Some(f) = crate::actors::projectile::hooks_of(w, id).and_then(|h| h.bounce_impact)
        && f(w, id, nx, ny, ivx, ivy)
    {
        pm(w, id).ended = true;
    }
    let p = pm(w, id);
    (p.vx, p.vy) = limit_velocity(p.vx, p.vy, p.max_speed);
    if under_stop_speed(p) {
        stop_simulating(p);
    }
}

/// `p[k] ?? or` for a number.
fn math_or(p: &mvs_data::value::Props, k: &str, or: Real) -> Real {
    crate::math::narrow(p.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::from(or)))
}

/// C019BlasterChargeProjectile's Capsule OnComponentHit
/// (ubergraph 5721 -> 1629), dispatched by the blocked move before the component handles the hit:
/// StopAimMoveMulticast and UpdateProjectileVelocity(unit(TravelStart -> location) x TravelSpeed), so each bounce
/// turns around the direction from the travel start, not the incoming velocity. True when that left a velocity
/// other than (ovx, ovy).
// Unverified: the dispatch inside the move is UPrimitiveComponent::MoveComponentImpl's from the 4.26 source.
pub fn component_hit(w: &mut World, id: ProjectileId, ovx: Real, ovy: Real) -> bool {
    stop_aim(pm(w, id));
    fly_on(w, id);
    let p = pr(w, id);
    p.vx != ovx || p.vy != ovy
}

/// The blocking hit's half that the stage solid's actor hears (UPrimitiveComponent::DispatchBlockingHit 142961f50:
/// the mover's own first, then the other actor's, unless its component is pending kill): the solid the move met
/// ([`HitShape`]: a polygon, or a box looked up by its place) and the move's hit normal
/// (`world::stage::stage_solid_hit_by`). True when its graph wrote the velocity.
pub fn solid_dispatch(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let solid = match p.hit_shape {
        HitShape::Poly(s) => Some(s),
        HitShape::Box(g) => crate::fighter::traces::solids::solid_of_box(w, &g),
        HitShape::None | HitShape::Block => None,
    };
    let Some(s) = solid else { return false };
    let (vx, vy, nx, ny) = (p.vx, p.vy, p.hit_nx, p.hit_ny);
    crate::world::stage::stage_solid_hit_by(w, crate::actors::ActorRef::Projectile(id), Some(s), nx, ny);
    let p = pr(w, id);
    p.vx != vx || p.vy != vy
}

/// The environment kill's blocking hit (the root's OnComponentHit -> DestroySpawnedActor, `projectile::step`): the
/// shape the capsule meets at (x, y), dispatched to the stage solid's actor (as [`solid_dispatch`]) though the
/// projectile is pending kill. The hit normal: a sweep that starts inside the shape (the capsule where it stands, at
/// (x, y)) takes the way out (the MTD, as `move_comp`'s start in penetration); else from the shape's nearest point
/// to where the capsule stops.
pub fn env_hit(w: &mut World, id: ProjectileId, g0: &Ground, ps: Shapes, x: Real, y: Real) {
    let p = pr(w, id);
    let Some(met) = blocker(p, g0, ps, x, y, 0, 0.0) else { return };
    let (nx, ny) = if p.x == x && p.y == y {
        let r = p.cap_r * p.scale;
        let h = (p.cap_h - p.cap_r) * p.scale;
        let [ux, uy] = math::ue::turn_by_pitch(p.pitch, [0.0, 1.0]);
        let ux = ux * axis_side(p);
        let m = match met {
            Met::Box(g) => capsule_mtd(&g, x, y, r, h, ux, uy),
            Met::Poly(i) => ps.polys[i].mtd(x, y, r, h, ux, uy),
        };
        (crate::math::narrow(m.nx), crate::math::narrow(m.ny))
    } else {
        let (qx, qy) = met_closest(ps, met, p.x, p.y);
        let (nx, ny) = (p.x - qx, p.y - qy);
        let h = math::ue::size2d([nx, ny]);
        let nl = if h == 0.0 || h.is_nan() { 1.0 } else { h };
        (nx / nl, ny / nl)
    };
    let solid = match shape_of(ps, met, met == Met::Box(*g0)) {
        HitShape::Poly(s) => Some(s),
        HitShape::Box(g) => crate::fighter::traces::solids::solid_of_box(w, &g),
        HitShape::None | HitShape::Block => None,
    };
    if solid.is_some() {
        crate::world::stage::stage_solid_hit_by(w, crate::actors::ActorRef::Projectile(id), solid, nx, ny);
    }
}

/// IsVelocityUnderSimulationThreshold, speed^2 <
/// BounceVelocityStopSimulatingThreshold^2 (never with 0).
pub fn under_stop_speed(p: &Projectile) -> bool {
    p.vx * p.vx + p.vy * p.vy < p.stop_speed * p.stop_speed
}

/// The move of a component without bounce for a class whose graph has
/// its own ReceiveHit: MoveUpdatedComponent with MOVECOMP_NeverIgnoreBlockingOverlaps (flag 4), so a blocker the
/// capsule starts in is a hit at time 0 too. A blocking hit dispatches ReceiveHit inside MoveComponentImpl, with the
/// capsule already at the pulled-back hit and before the velocity update, HandleBlockingHit and HandleImpact (the
/// kind's `receive_hit`). When the handler stopped the component or destroyed the actor TickComponent returns
/// there; otherwise HandleImpact, without bounce, is StopSimulating.
// Deviation: the sweep is the port's sampled one (move_comp) and the capsule's start-penetrating normal the 2D mtd's.
#[allow(clippy::too_many_arguments, reason = "the signature the call site fills")]
pub fn hit_move(w: &mut World, id: ProjectileId, g0: &Ground, ps: Shapes, dx: Real, dy: Real, host: bool) {
    if !move_comp(pm(w, id), g0, ps, dx, dy, true) {
        return;
    }
    if host && let Some(r) = crate::actors::projectile::hooks_of(w, id).and_then(|h| h.receive_hit) {
        let p = pr(w, id);
        let (nx, ny, t, pen) = (p.hit_nx, p.hit_ny, p.hit_t, p.hit_pen);
        if (r.hit)(w, id, nx, ny, t, pen) {
            pm(w, id).ended = true;
            solid_dispatch(w, id); // the other actor's half runs on a mover pending kill
            return;
        }
    }
    solid_dispatch(w, id);
    let p = pm(w, id);
    if p.stopped || p.dead {
        return;
    }
    stop_simulating(p);
}

/// UMovementComponent::SafeMoveUpdatedComponent (142977610), the move
/// of a bouncing component's pass and of its slide. The move is first made with MOVECOMP_NeverIgnoreBlockingOverlaps
/// | DisableBlockingOverlapDispatch, so any blocker the capsule starts in stops it at time 0 without a hit event.
/// Then GetPenetrationAdjustment (142969830): Normal x ((PenetrationDepth > 0 ? it : 0.125) + 0.125),
/// ResolvePenetration, and when that moved the capsule the move is made again with the component's own flags (none).
/// A push-out that fails leaves the first hit, which TickComponent bounces off and then aborts on (bounce_move).
pub fn safe_move(p: &mut Projectile, g0: &Ground, ps: Shapes, dx: Real, dy: Real) -> bool {
    let hit = move_comp(p, g0, ps, dx, dy, true);
    p.no_dispatch = false;
    if !hit || !p.hit_pen {
        return hit;
    }
    let (nx, ny, depth) = (p.hit_nx, p.hit_ny, p.hit_depth);
    let k = (if depth > 0.0 { depth } else { 0.125 }) + 0.125;
    if resolve_penetration(p, g0, ps, nx * k, ny * k, dx, dy) {
        return move_comp(p, g0, ps, dx, dy, false);
    }
    p.hit_t = 0.0;
    p.hit_nx = nx;
    p.hit_ny = ny;
    p.hit_pen = true;
    p.hit_depth = depth;
    p.no_dispatch = true;
    true
}

/// UMovementComponent::ResolvePenetrationImpl (142977160)
/// for the adjustment (ax, ay) of a hit whose move was (dx, dy). An overlap test of the capsule grown by 0.1
/// (PenetrationOverlapCheckInflation) at the adjusted place: free -> the capsule is put there without a sweep.
/// Otherwise a sweep along the adjustment; if that did not move and started in penetration, one along the
/// adjustment plus that hit's own (unless the two are equal or cancel); if still not moved, one along the
/// adjustment plus the move and, when the move points along the adjustment, one along the move alone. True when the
/// capsule moved.
// Not ported: these sweeps' own hit events (a charge shot's OnComponentHit on the way out) are left out.
#[allow(clippy::too_many_arguments, reason = "the signature the call site fills")]
pub fn resolve_penetration(
    p: &mut Projectile,
    g0: &Ground,
    ps: Shapes,
    ax: Real,
    ay: Real,
    dx: Real,
    dy: Real,
) -> bool {
    if ax == 0.0 && ay == 0.0 {
        return false;
    }
    if blocker(p, g0, ps, p.x + ax, p.y + ay, 0, 0.1).is_none() {
        p.x += ax;
        p.y += ay;
        return true;
    }
    let mut moved = sweep_out(p, g0, ps, ax, ay);
    if !moved && p.hit_pen {
        let k = (if p.hit_depth > 0.0 { p.hit_depth } else { 0.125 }) + 0.125;
        let sx = p.hit_nx * k;
        let sy = p.hit_ny * k;
        if (sx != ax || sy != ay) && (ax + sx != 0.0 || ay + sy != 0.0) {
            moved = sweep_out(p, g0, ps, ax + sx, ay + sy);
        }
    }
    if !moved && (dx != 0.0 || dy != 0.0) {
        moved = sweep_out(p, g0, ps, ax + dx, ay + dy);
        if !moved && dx * ax + dy * ay > 0.0 {
            moved = sweep_out(p, g0, ps, dx, dy);
        }
    }
    moved
}

/// MoveUpdatedComponent's result there, whether the capsule moved
/// (true for a move too short to make).
pub fn sweep_out(p: &mut Projectile, g0: &Ground, ps: Shapes, dx: Real, dy: Real) -> bool {
    let (x, y) = (p.x, p.y);
    move_comp(p, g0, ps, dx, dy, false);
    p.x != x || p.y != y || dx * dx + dy * dy <= 16e-8
}

/// One swept move of the root capsule
/// (UPrimitiveComponent::MoveComponentImpl 142971aa0). A move of 4e-4 u or less is not made. Without a blocking hit
/// the capsule takes the whole move and the result is false. With one the hit time is pulled back (PullBackHit,
/// inlined at 142972119: 0.1 u for a move under 1 u, a tenth of the move up to 10 u, 1 u beyond, plus a thousandth
/// of the move; not under 0), the capsule goes to that time and hit_t / hit_nx / hit_ny hold the hit. A blocker the
/// capsule starts in is a hit at time 0 (bStartPenetrating: hit_pen, with the way out as the normal and hit_depth)
/// and nothing moves; of several the one whose normal is most against the move is taken (1429721f0).
/// ShouldIgnoreHitResult (14297c130) drops such a hit when the move's direction has a positive part along its
/// normal (InitialOverlapTolerance 0), unless `never` (MOVECOMP_NeverIgnoreBlockingOverlaps).
// Unverified: the normal and depth of a start in penetration are the 2D capsule's least way out of the box (mtd);
// the build's come from PhysX (the sweep's MTD, UE 4.26 ConvertOverlappedShapeToImpactHit), not read.
pub fn move_comp(p: &mut Projectile, g0: &Ground, ps: Shapes, dx: Real, dy: Real, never: bool) -> bool {
    p.hit_t = 1.0;
    p.hit_nx = 0.0;
    p.hit_ny = 0.0;
    p.hit_pen = false;
    p.hit_depth = 0.0;
    p.hit_shape = HitShape::None;
    let len = math::ue::size2d([dx, dy]);
    if len <= 4e-4 {
        return false;
    }
    let (x0, y0) = (p.x, p.y);
    let mut skip: i32 = 0;
    let mut best: Real = 3.4e38;
    // (the floor, the boxes, then the polygons: one index each, as `blocker`'s skip bits)
    for i in 0..=ps.boxes.len() + ps.polys.len() {
        let met = if i == 0 {
            Met::Box(*g0)
        } else if i <= ps.boxes.len() {
            Met::Box(ps.boxes[i - 1])
        } else {
            Met::Poly(i - 1 - ps.boxes.len())
        };
        if !met_hits(p, ps, met, x0, y0, 0.0) {
            continue;
        }
        let m = met_mtd(p, ps, met, x0, y0);
        let d = m.0 * dx + m.1 * dy;
        if !never && d > 0.0 {
            if i < 31 {
                skip |= 1 << i;
            }
            continue;
        }
        if d < best {
            best = d;
            p.hit_pen = true;
            p.hit_nx = m.0;
            p.hit_ny = m.1;
            p.hit_depth = m.2;
            p.hit_shape = shape_of(ps, met, i == 0);
        }
    }
    if p.hit_pen {
        p.hit_t = 0.0;
        return true;
    }
    let n = math::max(1.0, (len / 10.0).ceil());
    let mut hit: Real = -1.0;
    let mut i: Real = 1.0;
    while i <= n && hit < 0.0 {
        if blocker(p, g0, ps, x0 + (dx * i) / n, y0 + (dy * i) / n, skip, 0.0).is_some() {
            hit = i;
        }
        i += 1.0;
    }
    if hit < 0.0 {
        p.x += dx;
        p.y += dy;
        return false;
    }
    let mut lo = (hit - 1.0) / n;
    let mut hi = hit / n;
    for _ in 0..12 {
        let m = (lo + hi) / 2.0;
        if blocker(p, g0, ps, x0 + dx * m, y0 + dy * m, skip, 0.0).is_some() {
            hi = m;
        } else {
            lo = m;
        }
    }
    let met = blocker(p, g0, ps, x0 + dx * hi, y0 + dy * hi, skip, 0.0).unwrap_or(Met::Box(*g0));
    let cx = x0 + dx * lo;
    let cy = y0 + dy * lo;
    let (qx, qy) = met_closest(ps, met, cx, cy);
    let mut nx = cx - qx;
    let mut ny = cy - qy;
    let h = math::ue::size2d([nx, ny]);
    // `|| 1`: a zero (or NaN) length is 1.
    let nl = if h == 0.0 || h.is_nan() { 1.0 } else { h };
    nx /= nl;
    ny /= nl;
    let back = math::min(math::max(0.1, 0.1 / len), 1.0 / len) + 0.001;
    let t = math::max(lo - back, 0.0);
    p.x = x0 + dx * t;
    p.y = y0 + dy * t;
    p.hit_t = t;
    p.hit_nx = nx;
    p.hit_ny = ny;
    p.hit_shape = shape_of(ps, met, met == Met::Box(*g0));
    true
}

/// What a blocking hit met: the stage block or a box (its place), or a polygon solid (its `StageState::solids`
/// index). The move keeps it for the hit's dispatch to the solid's actor ([`solid_dispatch`]).
#[derive(State, Copy, Debug, Default, PartialEq)]
pub enum HitShape {
    #[default]
    None,
    Block,
    Box(Ground),
    Poly(u32),
}

pub use crate::world::stage_query::{Met, Shapes};

/// The [`HitShape`] of a met shape (`block`: it is the stage block).
fn shape_of(ps: Shapes, met: Met, block: bool) -> HitShape {
    match met {
        Met::Box(_) if block => HitShape::Block,
        Met::Box(g) => HitShape::Box(g),
        Met::Poly(i) => HitShape::Poly(ps.polys[i].solid),
    }
}

/// The point of the shape nearest (x, y) ((x, y) inside it).
fn met_closest(ps: Shapes, met: Met, x: Real, y: Real) -> (Real, Real) {
    match met {
        Met::Box(g) => (
            math::max(crate::math::narrow(g.left), math::min(x, crate::math::narrow(g.right))),
            math::max(crate::math::narrow(g.bottom), math::min(y, crate::math::narrow(g.top))),
        ),
        Met::Poly(i) => ps.polys[i].closest(x, y),
    }
}

/// [`capsule_hits`] of a met shape.
fn met_hits(p: &Projectile, ps: Shapes, met: Met, x: Real, y: Real, infl: Real) -> bool {
    match met {
        Met::Box(g) => capsule_hits(p, &g, x, y, infl),
        Met::Poly(i) => capsule_hits_poly(p, &ps.polys[i], x, y, infl),
    }
}

/// [`mtd`] of a met shape: (nx, ny, depth); the projectile's mtd fields are left as `mtd` leaves them.
fn met_mtd(p: &mut Projectile, ps: Shapes, met: Met, x: Real, y: Real) -> (Real, Real, Real) {
    match met {
        Met::Box(g) => mtd(p, &g, x, y),
        Met::Poly(i) => {
            let r = p.cap_r * p.scale;
            let h = (p.cap_h - p.cap_r) * p.scale;
            let [ux, uy] = math::ue::turn_by_pitch(p.pitch, [0.0, 1.0]);
            let ux = ux * axis_side(p);
            let m = ps.polys[i].mtd(x, y, r, h, ux, uy);
            p.mtd_nx = crate::math::narrow(m.nx);
            p.mtd_ny = crate::math::narrow(m.ny);
            p.mtd_depth = crate::math::narrow(m.depth);
        }
    }
    (p.mtd_nx, p.mtd_ny, p.mtd_depth)
}

/// `Math.sign(pr.vx || pr.facing)`: the capsule axis's side.
fn axis_side(p: &Projectile) -> Real {
    let v = if p.vx == 0.0 || p.vx.is_nan() { p.facing } else { p.vx };
    math::sign(v)
}

/// The least move out of a box for this capsule (capsule_mtd), left in
/// mtd_nx / mtd_ny (its direction) and mtd_depth.
pub fn mtd(p: &mut Projectile, b: &Ground, x: Real, y: Real) {
    let r = p.cap_r * p.scale;
    let h = (p.cap_h - p.cap_r) * p.scale;
    let [ux, uy] = math::ue::turn_by_pitch(p.pitch, [0.0, 1.0]);
    let ux = ux * axis_side(p);
    let m = capsule_mtd(b, x, y, r, h, ux, uy);
    p.mtd_nx = crate::math::narrow(m.nx);
    p.mtd_ny = crate::math::narrow(m.ny);
    p.mtd_depth = crate::math::narrow(m.depth);
}

/// The first of the floor box, the platforms and the polygon solids the
/// root capsule (UE default radius 22, half-height 44, times the root scale, tilted by pitch) touches at (x, y), with
/// its radius grown by `infl`. `skip`: the shapes left out, as bits (1 the floor, 2 << i platform i, then the polygons
/// after the platforms; a sweep's passed blockers).
#[allow(clippy::too_many_arguments, reason = "the signature the call site fills")]
pub fn blocker(p: &Projectile, g: &Ground, ps: Shapes, x: Real, y: Real, skip: i32, infl: Real) -> Option<Met> {
    if skip & 1 == 0 && capsule_hits(p, g, x, y, infl) {
        return Some(Met::Box(*g));
    }
    for (i, b) in ps.boxes.iter().enumerate() {
        if i < 30 && skip & (2 << i) != 0 {
            continue;
        }
        if capsule_hits(p, b, x, y, infl) {
            return Some(Met::Box(*b));
        }
    }
    for (k, q) in ps.polys.iter().enumerate() {
        let i = ps.boxes.len() + 1 + k;
        if i < 31 && skip & (1 << i) != 0 {
            continue;
        }
        if capsule_hits_poly(p, q, x, y, infl) {
            return Some(Met::Poly(k));
        }
    }
    None
}

/// `capsuleHits`.
pub fn capsule_hits(p: &Projectile, b: &Ground, x: Real, y: Real, infl: Real) -> bool {
    let r = p.cap_r * p.scale + infl;
    let h = (p.cap_h - p.cap_r) * p.scale;
    let [ux, uy] = math::ue::turn_by_pitch(p.pitch, [0.0, 1.0]);
    let ux = ux * axis_side(p);
    for i in 0..=8 {
        let s = -1.0 + crate::real(i) / 4.0;
        let px = x + ux * h * s;
        let py = y + uy * h * s;
        let qx = math::max(crate::math::narrow(b.left), math::min(px, crate::math::narrow(b.right))) - px;
        let qy = math::max(crate::math::narrow(b.bottom), math::min(py, crate::math::narrow(b.top))) - py;
        if qx * qx + qy * qy <= r * r {
            return true;
        }
    }
    false
}

/// [`capsule_hits`] against a polygon solid: the same nine samples of the axis, each against the polygon's nearest
/// point.
pub fn capsule_hits_poly(p: &Projectile, q: &PolyBlock, x: Real, y: Real, infl: Real) -> bool {
    let r = p.cap_r * p.scale + infl;
    let h = (p.cap_h - p.cap_r) * p.scale;
    let [ux, uy] = math::ue::turn_by_pitch(p.pitch, [0.0, 1.0]);
    let ux = ux * axis_side(p);
    for i in 0..=8 {
        let s = -1.0 + crate::real(i) / 4.0;
        let px = x + ux * h * s;
        let py = y + uy * h * s;
        let (cx, cy) = q.closest(px, py);
        let (qx, qy) = (cx - px, cy - py);
        if qx * qx + qy * qy <= r * r {
            return true;
        }
    }
    false
}

/// The rotation a pass moves with (TickComponent 1429a0e00, read before
/// ComputeMoveDelta): with bRotationFollowsVelocity the orientation of the velocity the pass starts with, unless
/// each part is within 0.01 of zero (then the rotation stays). So the actor's rotation trails the velocity: after a
/// bounce, or a velocity written by the actor's own tick, it turns in the next pass. A velocity straight up or down
/// has yaw 0.
pub fn turn(p: &mut Projectile, vx: Real, vy: Real) {
    if !p.follows {
        return;
    }
    if vx.abs() <= 0.01 && vy.abs() <= 0.01 {
        return;
    }
    // Velocity.ToOrientationRotator(): FVector::Rotation of (0, |vx|, vy) for the pitch.
    p.pitch = math::ue::vector_rotation([0.0, vx.abs(), vy])[0];
    p.rot_x = if vx > 0.0 { 1.0 } else { -1.0 };
}
