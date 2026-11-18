//! The bouncing flight of an ItemActorParent child whose ProjectileMovement has bShouldBounce
//! (BouncingItem): Finn's gem (FinnDashThrownTarget, `chars::finn::gem`) and
//! Stripe's dropped bomb (C023B_DroppedBomb, `chars::stripe::bomb`). The class's OnProjectileBounce handlers are its
//! `bounced`, a handler that destroys it sets `bounce_stop`, its Platform response is `plat_ignored`: dispatched here
//! by the item's extension. [`Item::step`]'s flight comes here for an item whose extension holds a [`Bounce`].

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the build's NaN-keeping negated tests

use crate::Real;
use crate::State;
use crate::actors::item::Item;
use crate::actors::item::ItemBox;
use crate::actors::item::ItemDef;
use crate::actors::item::ItemExt;
use crate::actors::item::num;
use crate::actors::projectile_move::limit_velocity;
use crate::consts::{DT, GRAVITY_Z};
use crate::fighter::moves::V2;
use crate::math::max;
use crate::math::ue;
use mvs_collision::capsule_mtd::Mtd;

/// `BouncingItem`'s own fields: the flight's HandleDeflection memory (the last pass's hit time and normal, and the
/// slide), whether the last sweep started inside a box, and the ProjectileMovement's Bounciness and Friction.
#[derive(State, Debug)]
pub struct Bounce {
    pub prev_hit_t: Real,
    pub prev_nx: Real,
    pub prev_ny: Real,
    pub sliding: bool,
    /// The last sweep started inside a box.
    pub start_pen: bool,
    pub bounciness: Real,
    pub friction: Real,
}

impl Default for Bounce {
    /// A class whose data has no ProjectileMovement template (an empty one is used).
    fn default() -> Self {
        Bounce {
            prev_hit_t: 1.0,
            prev_nx: 0.0,
            prev_ny: 0.0,
            sliding: false,
            start_pen: false,
            bounciness: 0.6,
            friction: 0.2,
        }
    }
}

impl Bounce {
    /// The constructor's part: Bounciness and Friction of the class's ProjectileMovement template.
    pub fn new(a: ItemDef) -> Self {
        let pm = a.comp("ProjectileMovement");
        Bounce {
            bounciness: pm.and_then(|c| num(c, "Bounciness")).unwrap_or(0.6),
            friction: pm.and_then(|c| num(c, "Friction")).unwrap_or(0.2),
            ..Bounce::default()
        }
    }
}

/// The item's [`Bounce`], `None` for an item that does not bounce.
pub fn bounce_of(it: &Item) -> Option<&Bounce> {
    match &it.ext {
        ItemExt::Gem(g) => Some(&g.bounce),
        ItemExt::DroppedBomb(d) => Some(&d.bounce),
        _ => None,
    }
}

fn bs(it: &Item) -> &Bounce {
    bounce_of(it).expect("a bouncing item")
}

fn bm(it: &mut Item) -> &mut Bounce {
    match &mut it.ext {
        ItemExt::Gem(g) => &mut g.bounce,
        ItemExt::DroppedBomb(d) => &mut d.bounce,
        _ => panic!("item {:?}: not a bouncing item", it.class),
    }
}

/// `BouncingItem.fly`: the bouncing item's flight in place of `Item::fly` (the same parameters and answer: blocked).
/// UProjectileMovementComponent with bShouldBounce (its Bounciness and Friction; BounceVelocityStopSimulatingThreshold
/// 0, as both classes have it), one frame: the projectile's bounce move's passes (HandleBlockingHit, HandleImpact,
/// HandleDeflection, HandleSliding; MaxSimulationIterations 4) against the item's boxes. The threshold never stops
/// it, so it never leaves the movement: lying, it takes an impact a frame from the gravity half-step. With HitBoxOn?
/// a blocking hit is ItemActorParent's ReceiveHit (FinnDashThrownTarget's read, its @4579): its tail deactivates the
/// ProjectileMovement (@5148) inside the move's dispatch, HandleBlockingHit aborts and no OnProjectileBounce fires;
/// true hands it to the step's spend. Without it the hit bounces: the class's OnProjectileBounce handlers
/// ([`bounced`]). The Platform channel: [`plat_ignored`]. The homing acceleration and the contacts list are the base
/// flight's: this one has neither.
/// Deviation: the contact is found by 10 u samples and bisection, its normal the deepest sample's (Item's fly's).
pub fn fly(
    it: &mut Item,
    b: &ItemBox,
    platforms: &[ItemBox],
    contacts: &mut Vec<super::item::ItemContact>,
    accel: Option<V2>,
) -> bool {
    let _ = (contacts, accel);
    let mut remaining = DT;
    let mut impacts = 0;
    let mut n_it = 0;
    while remaining >= 1e-6 && n_it < 4 {
        n_it += 1;
        let tick = remaining;
        remaining = 0.0;
        let (ox, oy) = (it.vx, it.vy);
        velocity(it, ox, oy, tick);
        let dx = ox * tick + (it.vx - ox) * 0.5 * tick;
        let dy = oy * tick + (it.vy - oy) * 0.5 * tick;
        let (ht, hit) = sweep(it, b, platforms, dx, dy);
        if ht < 0.0 {
            let s = bm(it);
            s.prev_hit_t = 1.0;
            s.sliding = false;
            continue;
        }
        if it.hit_on {
            return true;
        }
        let (nx, ny, pen) = (hit[1], hit[2], bs(it).start_pen);
        if ht > 1e-4 {
            velocity(it, ox, oy, tick * ht);
        } else {
            it.vx = ox;
            it.vy = oy;
        }
        impacts += 1;
        let mut sub = tick * (1.0 - ht);
        impact(it, nx, ny);
        if bounce_stop(it) || pen {
            break;
        }
        let s = bs(it);
        let repeat = s.prev_hit_t < 1.0 && ht <= 1e-4;
        let along = s.prev_nx * nx + s.prev_ny * ny;
        // HandleDeflection: GetSafeNormal(Velocity) . N (tolerance 1e-8; a zero velocity gives 0).
        let u = ue::safe_normal([0.0, it.vx, it.vy], ue::SMALL_NUMBER);
        let sliding = (repeat && along >= 0.999845) || u[1] * nx + u[2] * ny <= 0.01;
        bm(it).sliding = sliding;
        let (mut last_t, mut last_nx, mut last_ny) = (ht, nx, ny);
        if sliding {
            if repeat && along <= 0.0 {
                it.vx = 0.0;
                it.vy = 0.0;
            } else {
                let vn = it.vx * nx + it.vy * ny;
                it.vx -= vn * nx;
                it.vy -= vn * ny;
            }
            if sub > 1e-4 {
                // HandleSliding: the time left moved along the surface; a hit there is an impact of its own.
                let (vx, vy) = (it.vx, it.vy);
                let (st, h2) = sweep(it, b, platforms, vx * sub, vy * sub);
                if st >= 0.0 {
                    last_t = st;
                    last_nx = h2[1];
                    last_ny = h2[2];
                    let spen = bs(it).start_pen;
                    impact(it, h2[1], h2[2]);
                    if bounce_stop(it) || spen {
                        break;
                    }
                    sub *= 1.0 - st;
                } else {
                    // The pass's acceleration: into the surface, only its part along it stays, less a friction of
                    // min(the part into it x Friction, the speed).
                    last_t = 1.0;
                    last_nx = 0.0;
                    last_ny = 0.0;
                    let (vx0, vy0) = (it.vx, it.vy);
                    velocity(it, vx0, vy0, sub);
                    let (fx, fy) = (it.vx - vx0, it.vy - vy0);
                    let fnn = fx * nx + fy * ny;
                    if !(fnn >= 0.0) {
                        // HandleSliding 142993dd0: T = (dv - N dn) + V, less min(-(dn Friction), |T|) along
                        // T.GetSafeNormal() (tolerance 1e-8, the == 1 shortcut).
                        let (wx, wy) = ((fx - nx * fnn) + vx0, (fy - ny * fnn) + vy0);
                        let ws = ue::size2d([wx, wy]);
                        let fm = -(fnn * bs(it).friction);
                        let fr = if fm <= ws { fm } else { ws };
                        let u = ue::safe_normal([0.0, wx, wy], ue::SMALL_NUMBER);
                        it.vx = wx - u[1] * fr;
                        it.vy = wy - u[2] * fr;
                    }
                    sub = 0.0;
                }
            }
        }
        let s = bm(it);
        s.prev_hit_t = last_t;
        s.prev_nx = last_nx;
        s.prev_ny = last_ny;
        if sub >= 1e-6 {
            remaining += sub;
            if impacts <= 1 {
                n_it -= 1;
            }
        }
    }
    false
}

/// `velocity`: ComputeVelocity over dt from (vx, vy): the gravity x ProjectileGravityScale, LimitVelocity at MaxSpeed.
fn velocity(it: &mut Item, vx: Real, vy: Real, dt: Real) {
    let nvy = vy - GRAVITY_Z * it.grav_scale * dt;
    (it.vx, it.vy) = limit_velocity(vx, nvy, it.max_speed);
}

/// `impact`: HandleImpact: ComputeBounceResult (the part along the normal turned around x Bounciness, the part along
/// the surface x (1 - Friction), the friction first scaled by clamp(normal / surface speed, 0, 1) while sliding), then
/// the OnProjectileBounce handlers with ImpactVelocity (the velocity before), then LimitVelocity.
fn impact(it: &mut Item, nx: Real, ny: Real) {
    let ivy = it.vy;
    let vn = it.vx * nx + it.vy * ny;
    if !(vn > 0.0) {
        // ComputeBounceResult 14298c4e0: P = N (-d), T = P + V, the result (k T) + (B P); the friction fraction a
        // true divide by sqrtss(|T|^2), a NaN or one below MinFrictionFraction (0) taking 0.
        let (px, py) = (nx * -vn, ny * -vn);
        let (tx, ty) = (px + it.vx, py + it.vy);
        let s = bs(it);
        let mut fr = s.friction;
        if s.sliding {
            let q = -(vn / ue::size2d([tx, ty]));
            fr *= if !(q >= 0.0) {
                0.0
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
        let b = if s.bounciness > 0.0 { s.bounciness } else { 0.0 };
        it.vx = keep * tx + b * px;
        it.vy = keep * ty + b * py;
    }
    bounced(it, ivy);
    (it.vx, it.vy) = limit_velocity(it.vx, it.vy, it.max_speed);
}

/// `sweep`: one sweep of (dx, dy) from the item's place: the hit time (0..1; the item moved to the contact) with the
/// hit ([depth, nx, ny]), or -1 (moved the whole way). SafeMoveUpdatedComponent (exe 1429a0e00, as the projectile's
/// safe move): a start inside a box is pushed out (`resolve_pen`) and the move made from there; one that cannot be
/// pushed out is a hit at 0 (`start_pen`).
fn sweep(it: &mut Item, b: &ItemBox, platforms: &[ItemBox], dx: Real, dy: Real) -> (Real, [Real; 3]) {
    let mut hit = [0.0, 0.0, 0.0];
    bm(it).start_pen = false;
    if blocked_at(it, b, platforms, it.x, it.y) && !resolve_pen(it, b, platforms, dx, dy) {
        normal_at(it, b, platforms, it.x, it.y, &mut hit);
        bm(it).start_pen = true;
        return (0.0, hit);
    }
    let (x0, y0) = (it.x, it.y);
    let n = max(1.0, (ue::size2d([dx, dy]) / 10.0).ceil());
    let mut i = 1.0;
    while i <= n {
        if !blocked_at(it, b, platforms, x0 + (dx * i) / n, y0 + (dy * i) / n) {
            i += 1.0;
            continue;
        }
        let mut lo = (i - 1.0) / n;
        let mut hi = i / n;
        for _ in 0..12 {
            let m = (lo + hi) / 2.0;
            if blocked_at(it, b, platforms, x0 + dx * m, y0 + dy * m) {
                hi = m;
            } else {
                lo = m;
            }
        }
        it.x = x0 + dx * lo;
        it.y = y0 + dy * lo;
        normal_at(it, b, platforms, x0 + dx * hi, y0 + dy * hi, &mut hit);
        return (lo, hit);
    }
    it.x += dx;
    it.y += dy;
    (-1.0, hit)
}

/// `resolvePen`: ResolvePenetrationImpl's order (142977160, the projectile's resolvePenetration) for the item: the
/// start hit's way out (`pen_mtd`) x (depth + 0.125) as the adjustment; free there -> the item is put there; else the
/// adjustment plus the adjusted place's own; else the adjustment plus the move. True when the item moved.
/// Deviation: the item has no swept capsule (Item's 9-sample test), so each try is a place test, not a sweep that
/// passes what it starts in; the 0.1 overlap inflation is left out. The projectile's safe move, resolve and move are
/// bound to its own swept root capsule and fields: the MTD (capsule_mtd) is the part shared.
fn resolve_pen(it: &mut Item, b: &ItemBox, platforms: &[ItemBox], dx: Real, dy: Real) -> bool {
    let (x, y) = (it.x, it.y);
    let pen = pen_mtd(it, b, platforms, x, y, dx, dy);
    let k = (if pen.depth > 0.0 { pen.depth } else { 0.125 }) + 0.125;
    let (ax, ay) = (pen.nx * k, pen.ny * k);
    let (mut tx, mut ty) = (x + crate::math::narrow(ax), y + crate::math::narrow(ay));
    if blocked_at(it, b, platforms, tx, ty) {
        let pen2 = pen_mtd(it, b, platforms, tx, ty, dx, dy);
        let k2 = (if pen2.depth > 0.0 { pen2.depth } else { 0.125 }) + 0.125;
        tx = x + crate::math::narrow(ax) + crate::math::narrow(pen2.nx * k2);
        ty = y + crate::math::narrow(ay) + crate::math::narrow(pen2.ny * k2);
        if blocked_at(it, b, platforms, tx, ty) {
            tx = x + crate::math::narrow(ax) + dx;
            ty = y + crate::math::narrow(ay) + dy;
            if blocked_at(it, b, platforms, tx, ty) {
                return false;
            }
        }
    }
    it.x = tx;
    it.y = ty;
    true
}

/// `penMtd`: the start-in-penetration hit at (x, y): of the boxes the item touches there, the way out (capsule_mtd)
/// whose normal is most against the move (dx, dy) (MoveComponentImpl 1429721f0, as the projectile's move).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn pen_mtd(it: &Item, b: &ItemBox, platforms: &[ItemBox], x: Real, y: Real, dx: Real, dy: Real) -> Mtd {
    let r = it.capsule_radius();
    let h = max(it.capsule_half_height() - r, 0.0);
    let [ux, uy] = crate::math::ue::turn_by_pitch(it.pitch, [0.0, 1.0]);
    let ux = ux * it.facing;
    let mut best = 3.4e38;
    let mut pen = Mtd { nx: 0.0, ny: 1.0, depth: 0.0 };
    for p in std::iter::once(b).chain(platforms.iter().filter(|p| !plat_ignored(it, p))) {
        if !it.touches(p, x, y) {
            continue;
        }
        let m = p.mtd(x, y, r, h, ux, uy);
        let d = m.nx * f64::from(dx) + m.ny * f64::from(dy);
        if d >= best {
            continue;
        }
        best = d;
        pen = m;
    }
    pen
}

/// `blockedAt`.
fn blocked_at(it: &Item, b: &ItemBox, platforms: &[ItemBox], x: Real, y: Real) -> bool {
    if it.no_collision == Some(true) {
        return false;
    }
    if it.touches(b, x, y) {
        return true;
    }
    platforms.iter().any(|p| !plat_ignored(it, p) && it.touches(p, x, y))
}

/// `normalAt`: into `hit` ([depth, nx, ny]) the deepest touching sample of the boxes the flight meets at (x, y).
fn normal_at(it: &Item, b: &ItemBox, platforms: &[ItemBox], x: Real, y: Real, hit: &mut [Real; 3]) {
    *hit = [-1.0, 0.0, 1.0];
    it.deepest(b, x, y, hit);
    for p in platforms {
        if !plat_ignored(it, p) {
            it.deepest(p, x, y, hit);
        }
    }
}

/// `bounced`: the class's OnProjectileBounce handlers, at HandleImpact's broadcast (ImpactVelocity.Z: `ivy`).
pub fn bounced(it: &mut Item, ivy: Real) {
    if matches!(it.ext, ItemExt::Gem(_)) {
        crate::chars::finn::gem::bounced(it, ivy);
    }
}

/// `bounceStop`: a handler destroyed the actor in this frame's flight, the move ends there.
pub fn bounce_stop(it: &Item) -> bool {
    match it.ext {
        ItemExt::Gem(_) => crate::chars::finn::gem::bounce_stop(it),
        _ => false,
    }
}

/// `platIgnored`: the class ignores the platform `p`. The base: the Platform channel (object type Platform) is
/// ignored while IsThrown (ItemActorParent's throw); a stage solid of object type WorldStatic blocks it all the same
/// (Item's blocked test's rule). A class with its own Platform response overrides it.
pub fn plat_ignored(it: &Item, p: &ItemBox) -> bool {
    match it.ext {
        ItemExt::Gem(_) => crate::chars::finn::gem::plat_ignored(it, p),
        _ => it.thrown && !p.is_static,
    }
}
