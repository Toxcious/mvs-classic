//! The spawn place of an actor spawned with collision handling AdjustIfPossibleButAlwaysSpawn (2): Comp_Spawner
//! CompSpawnActor's BeginDeferredActorSpawnFromClass(..., 2, Owner) @9054 / @9531. UWorld::SpawnActor 142b4cbd0
//! keeps the override at SpawnCollisionHandlingMethod; AActor::PostActorConstruction 142804390, after the
//! components are initialized and before BeginPlay, runs FindTeleportSpot 142b41200 for case 2 and teleports the
//! actor there when it says true; on false the actor stays where it was spawned. Comp_Spawner's pool branch
//! (InitFromPool @3399: K2_SetActorTransform, teleport, no sweep) does not come here. Nor does an attached spawn: on
//! CompSpawnActor's attach branch (K2_AttachToActor / K2_AttachToComponent, then @10448 K2_SetActorTransform or
//! @10515 K2_SetActorRelativeTransform, both teleports after FinishSpawningActor) that transform replaces the place
//! FindTeleportSpot gave, so a caller with an attached spawn does not call it.
//!
//! FindTeleportSpot, for the shape of the movement component's UpdatedPrimitive (the root: a sphere, a capsule or a
//! box; every SpawnedActorParent child's root is its Capsule):
//! - EncroachingBlockingGeometry with adjustment 142b3cce0: the boxes the shape shrunk by p.EncroachEpsilon (0.15)
//!   overlaps; none: the place is kept. The adjustment is the sum over them of the MTD of the full shape (out of
//!   the box, Direction x Distance; no + 0.125).
//! - An adjustment with every |c| <= 1e-4: false, the place kept.
//! - |Adjust.Z| > 1e-4: the place + (0, Adjust.Z) when the shrunk shape is free there.
//! - Then the place + (Adjust.Y, 0), then + (Adjust.Y, Adjust.Z); none free: the place kept.
//!
//! The caller passes the boxes that block the root's object channel and that the root does not ignore (the
//! overlap runs with the root's own responses, then keeps a component that blocks the root's channel).
//! In the play plane the MTD of a sphere or a capsule out of a box is `capsule_mtd`'s (Projectile's start in
//! penetration; for a capsule PhysX's MTD of a capsule in a box).
//!
//! Deviation: while spawning the build tries 8 permutations of the XY adjustment (one once the actor has begun
//! play); their table is not read, and the ones that swap X and Y would move the actor off the play plane: only
//! the adjustment as it is is tried here. The box's depth (UE X) is taken as covering the shape, so the MTD has no
//! X part. A failed MTD (zero adjustment) and the retry of a distance <= 1e-8 with the shrunk shape do not occur
//! for a sphere against a box and are not modelled.
//! Unverified (capsule, box): the shrunk shape is taken as GetCollisionShape(-0.15) does it in UE 4.26: a
//! capsule's radius and half height each less 0.15, a box's three half extents each less 0.15 (none below 0).
//! Unverified (capsule): as stock 4.26's FCollisionShape, a capsule whenever r < hh (else a sphere), its axis half
//! length GetCapsuleAxisHalfLength = max(hh - r, 1), for the shrunk overlap and the MTD alike.
//! Unverified (box): every Comp_Spawner spawn has yaw +-90 (CompSpawnActor's transform), so the box's local X runs
//! along the facing and its play-plane side is BoxExtent.X (`box_shape`'s `along_facing`), its depth BoxExtent.Y;
//! a yaw of 0 / 180 swaps them. The box's MTD is the separating-axis one (the least way out along the stage box's
//! two axes and the root box's own two, in that order, a tie to the first): the build's PhysX box-box MTD is not
//! read.

use crate::Real;
use crate::actors::ActorRef;
use crate::fighter::FighterId;
use crate::fighter::moves::V2;
use crate::math::{max, min};
use crate::name;
use crate::world::World;
use crate::world::spawning::{CapsuleShapeC, FindTeleportSpotAtOut, SpawnShape, SpawnSpotA, SpawnSpotR};
use crate::world::stage_query::{ShapeRef, Shapes};
use mvs_collision::Ground;
use mvs_collision::capsule_mtd::{Mtd, capsule_mtd};
use mvs_data::value::{Props, Value};

/// p.EncroachEpsilon, CVar default 143cd70e4.
const ENCROACH_EPSILON: Real = 0.15;
/// KINDA_SMALL_NUMBER, 143b05c98.
const KINDA_SMALL_NUMBER: Real = 1e-4;
/// [`KINDA_SMALL_NUMBER`] in double, at the collision's and the anim's double calls.
const KINDA_SMALL_NUMBER_F64: f64 = 1e-4;

/// The field `k` of `p` when it is a number.
pub(crate) fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

/// The object `k` of `p`.
pub(crate) fn obj<'a>(p: &'a Props, k: &str) -> Option<&'a Props> {
    p.get(k).and_then(Value::as_obj)
}

/// A JavaScript truthy read of the field `k` (an absent field, null, false, 0, NaN and "" are false).
pub(crate) fn truthy(p: &Props, k: &str) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// A CapsuleComponent template's shape (the actor data's `components.<name>`), at a uniform world scale: the
/// constructor's 22 / 44 where the template chain leaves a field unset (as Projectile and Item take it), the half
/// height never below the radius (UCapsuleComponent keeps CapsuleHalfHeight >= CapsuleRadius); hh == r is a
/// sphere. (`ux`, `uy`): the up axis (the defaults 0, 1).
pub fn capsule_shape(c: Option<CapsuleShapeC>, scale: Real, ux: Real, uy: Real) -> SpawnShape {
    let (cr, ch) = c.map_or((None, None), |c| (c.capsule_radius, c.capsule_half_height));
    let r = cr.unwrap_or(22.0) * scale;
    let hh = max(ch.unwrap_or(44.0) * scale, r);
    if r < hh { SpawnShape::Capsule { r, hh, ux, uy } } else { SpawnShape::Sphere { r } }
}

/// The `CapsuleShapeC` of a CapsuleComponent template's fields.
pub fn capsule_c(c: &Props) -> CapsuleShapeC {
    CapsuleShapeC { capsule_radius: num(c, "CapsuleRadius"), capsule_half_height: num(c, "CapsuleHalfHeight") }
}

/// A BoxComponent template's shape, at a uniform world scale (the constructor's 32 where an extent is unset): its
/// play-plane side is BoxExtent.X when the actor's yaw is +-90 (`along_facing`, every Comp_Spawner spawn: local X
/// runs along the facing), BoxExtent.Y at yaw 0 / 180; its up side BoxExtent.Z.
pub fn box_shape(c: Option<&Props>, scale: Real, ux: Real, uy: Real, along_facing: bool) -> SpawnShape {
    let e = c.and_then(|c| obj(c, "BoxExtent"));
    let side = e.and_then(|e| num(e, if along_facing { "X" } else { "Y" }));
    let up = e.and_then(|e| num(e, "Z"));
    SpawnShape::Box { hx: side.unwrap_or(32.0) * scale, hz: up.unwrap_or(32.0) * scale, ux, uy }
}

/// A capsule's axis half length (GetCapsuleAxisHalfLength, at least 1).
fn axis_half(r: Real, hh: Real) -> Real {
    max(hh - r, 1.0)
}

/// Squared distance from (x, y) to the box (0 inside).
fn dist2(b: &Ground, x: Real, y: Real) -> Real {
    // the box is the collision's (double): the distance in double.
    let (x, y) = (f64::from(x), f64::from(y));
    let dx = if x < b.left {
        b.left - x
    } else if x > b.right {
        x - b.right
    } else {
        0.0
    };
    let dy = if y < b.bottom {
        b.bottom - y
    } else if y > b.top {
        y - b.top
    } else {
        0.0
    };
    crate::math::narrow(dx * dx + dy * dy)
}

/// Squared distance from the segment (ax, ay)-(bx, by) to the box (0 when it runs through it).
fn seg_dist2(b: &Ground, ax: Real, ay: Real, bx: Real, by: Real) -> Real {
    let ex = bx - ax;
    let ey = by - ay;
    let mut t0: Real = 0.0;
    let mut t1: Real = 1.0;
    let mut through = true;
    if ex == 0.0 {
        through = ax >= crate::math::narrow(b.left) && ax <= crate::math::narrow(b.right);
    } else {
        let p = (b.left - f64::from(ax)) / f64::from(ex);
        let q = (b.right - f64::from(ax)) / f64::from(ex);
        t0 = max(t0, min(crate::math::narrow(p), crate::math::narrow(q)));
        t1 = min(t1, max(crate::math::narrow(p), crate::math::narrow(q)));
    }
    if ey == 0.0 {
        through = through && ay >= crate::math::narrow(b.bottom) && ay <= crate::math::narrow(b.top);
    } else {
        let p = (b.bottom - f64::from(ay)) / f64::from(ey);
        let q = (b.top - f64::from(ay)) / f64::from(ey);
        t0 = max(t0, min(crate::math::narrow(p), crate::math::narrow(q)));
        t1 = min(t1, max(crate::math::narrow(p), crate::math::narrow(q)));
    }
    if through && t0 <= t1 {
        return 0.0;
    }
    let mut d2 = min(dist2(b, ax, ay), dist2(b, bx, by));
    let e2 = ex * ex + ey * ey;
    if e2 > 0.0 {
        for i in 0..4 {
            let cx = if i & 1 != 0 { b.right } else { b.left };
            let cy = if i & 2 != 0 { b.top } else { b.bottom };
            let t = min(
                max(
                    crate::math::narrow(
                        ((cx - f64::from(ax)) * f64::from(ex) + (cy - f64::from(ay)) * f64::from(ey)) / f64::from(e2),
                    ),
                    0.0,
                ),
                1.0,
            );
            let qx = ax + ex * t - crate::math::narrow(cx);
            let qy = ay + ey * t - crate::math::narrow(cy);
            d2 = min(d2, qx * qx + qy * qy);
        }
    }
    d2
}

/// The box root (centre (x, y), half extents `hx` across and `hz` along (ux, uy)) overlaps the stage box: the
/// separating-axis test on the stage box's two axes and, when the root box is turned, its own two.
#[allow(clippy::too_many_arguments, reason = "a box, a place and the root box's shape")]
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "a NaN separates: the negated tests are kept")]
fn box_overlaps(b: &Ground, x: Real, y: Real, hx: Real, hz: Real, ux: Real, uy: Real) -> bool {
    let aux = ux.abs();
    let auy = uy.abs();
    let bw = (b.right - b.left) * 0.5;
    let bh = (b.top - b.bottom) * 0.5;
    let dx = x - crate::math::narrow((b.left + b.right) * 0.5);
    let dy = y - crate::math::narrow((b.bottom + b.top) * 0.5);
    if !(dx.abs() < auy * hx + aux * hz + crate::math::narrow(bw)) {
        return false;
    }
    if !(dy.abs() < aux * hx + auy * hz + crate::math::narrow(bh)) {
        return false;
    }
    if aux > KINDA_SMALL_NUMBER && auy > KINDA_SMALL_NUMBER {
        if !((ux * dx + uy * dy).abs() < hz + aux * crate::math::narrow(bw) + auy * crate::math::narrow(bh)) {
            return false;
        }
        if !((uy * dx - ux * dy).abs() < hx + auy * crate::math::narrow(bw) + aux * crate::math::narrow(bh)) {
            return false;
        }
    }
    true
}

/// The least move out of the stage box for the box root (the order of the axes as `box_overlaps`', a tie to the
/// first: +Y side, -Y side, top, bottom, then the root's own up and across axes).
#[allow(clippy::too_many_arguments, reason = "a box, a place and the root box's shape")]
fn box_mtd(b: &Ground, x: Real, y: Real, hx: Real, hz: Real, ux: Real, uy: Real) -> Mtd {
    let aux = ux.abs();
    let auy = uy.abs();
    let ex = auy * hx + aux * hz;
    let ey = aux * hx + auy * hz;
    // the depths in float, as the build holds the box
    let mut pen = crate::math::narrow(b.right) - (x - ex);
    let (mut nx, mut ny) = (1.0, 0.0);
    let mut q = x + ex - crate::math::narrow(b.left);
    if q < pen {
        pen = q;
        nx = -1.0;
    }
    q = crate::math::narrow(b.top) - (y - ey);
    if q < pen {
        pen = q;
        (nx, ny) = (0.0, 1.0);
    }
    q = y + ey - crate::math::narrow(b.bottom);
    if q < pen {
        pen = q;
        (nx, ny) = (0.0, -1.0);
    }
    if aux > KINDA_SMALL_NUMBER && auy > KINDA_SMALL_NUMBER {
        let bw = (b.right - b.left) * 0.5;
        let bh = (b.top - b.bottom) * 0.5;
        let cx = (b.left + b.right) * 0.5;
        let cy = (b.bottom + b.top) * 0.5;
        // along the up axis (ux, uy)
        let mut s = ux * x + uy * y;
        let mut mid = ux * crate::math::narrow(cx) + uy * crate::math::narrow(cy);
        let mut ext = aux * crate::math::narrow(bw) + auy * crate::math::narrow(bh);
        q = mid + ext - (s - hz);
        if q < pen {
            pen = q;
            (nx, ny) = (ux, uy);
        }
        q = s + hz - (mid - ext);
        if q < pen {
            pen = q;
            (nx, ny) = (-ux, -uy);
        }
        // across it (uy, -ux)
        s = uy * x - ux * y;
        mid = uy * crate::math::narrow(cx) - ux * crate::math::narrow(cy);
        ext = auy * crate::math::narrow(bw) + aux * crate::math::narrow(bh);
        q = mid + ext - (s - hx);
        if q < pen {
            pen = q;
            (nx, ny) = (uy, -ux);
        }
        q = s + hx - (mid - ext);
        if q < pen {
            pen = q;
            (nx, ny) = (-uy, ux);
        }
    }
    Mtd { nx: f64::from(nx), ny: f64::from(ny), depth: f64::from(pen) }
}

/// The shape shrunk by p.EncroachEpsilon at (x, y) overlaps the box.
fn overlaps(b: &Ground, x: Real, y: Real, s: &SpawnShape) -> bool {
    match *s {
        SpawnShape::Sphere { r } => {
            let rs = r - ENCROACH_EPSILON;
            dist2(b, x, y) < rs * rs
        }
        SpawnShape::Capsule { r, hh, ux, uy } => {
            let rs = max(r - ENCROACH_EPSILON, 0.0);
            let h = axis_half(rs, max(hh - ENCROACH_EPSILON, 0.0));
            seg_dist2(b, x - ux * h, y - uy * h, x + ux * h, y + uy * h) < rs * rs
        }
        SpawnShape::Box { hx, hz, ux, uy } => {
            box_overlaps(b, x, y, max(hx - ENCROACH_EPSILON, 0.0), max(hz - ENCROACH_EPSILON, 0.0), ux, uy)
        }
    }
}

/// The shape shrunk by p.EncroachEpsilon at (x, y) overlaps the polygon solid: a sphere or a capsule by its centre's
/// (segment's) distance to it, a box by the box round it in the play plane (the shape traces' overlap).
// Unverified: a polygon solid is met by its nearest point and the shape traces' convex sweep, not PhysX's convex
// overlap; a turned box root is taken as the box round it.
fn poly_overlaps(q: &crate::world::stage_query::PolyBlock, x: Real, y: Real, s: &SpawnShape) -> bool {
    match *s {
        SpawnShape::Sphere { r } => {
            let rs = r - ENCROACH_EPSILON;
            let (cx, cy) = q.closest(x, y);
            (cx - x) * (cx - x) + (cy - y) * (cy - y) < rs * rs
        }
        SpawnShape::Capsule { r, hh, ux, uy } => {
            let rs = max(r - ENCROACH_EPSILON, 0.0);
            let h = axis_half(rs, max(hh - ENCROACH_EPSILON, 0.0));
            q.seg_dist2(x - ux * h, y - uy * h, x + ux * h, y + uy * h) < rs * rs
        }
        SpawnShape::Box { hx, hz, ux, uy } => {
            let (hx, hz) = (max(hx - ENCROACH_EPSILON, 0.0), max(hz - ENCROACH_EPSILON, 0.0));
            let (aux, auy) = (ux.abs(), uy.abs());
            q.box_overlaps(x, y, auy * hx + aux * hz, aux * hx + auy * hz)
        }
    }
}

/// The MTD of the full shape out of the polygon solid (its own capsule MTD; a box root as the box round it in the
/// play plane, the shape traces' overlap at the start, as `poly_overlaps` tests it).
fn poly_mtd(q: &crate::world::stage_query::PolyBlock, x: Real, y: Real, s: &SpawnShape) -> Mtd {
    match *s {
        SpawnShape::Sphere { r } => q.mtd(x, y, r, 0.0, 0.0, 1.0),
        SpawnShape::Capsule { r, hh, ux, uy } => q.mtd(x, y, r, axis_half(r, hh), ux, uy),
        SpawnShape::Box { hx, hz, ux, uy } => {
            let (aux, auy) = (ux.abs(), uy.abs());
            let (nx, ny, d) = q.box_mtd(x, y, auy * hx + aux * hz, aux * hx + auy * hz).unwrap_or((0.0, 0.0, 0.0));
            Mtd { nx: f64::from(nx), ny: f64::from(ny), depth: f64::from(d) }
        }
    }
}

fn overlaps_shape(b: ShapeRef, x: Real, y: Real, s: &SpawnShape) -> bool {
    match b {
        ShapeRef::Box(g) => overlaps(g, x, y, s),
        ShapeRef::Poly(q) => poly_overlaps(q, x, y, s),
    }
}

fn encroaches(boxes: Shapes, x: Real, y: Real, s: &SpawnShape) -> bool {
    boxes.iter().any(|b| overlaps_shape(b, x, y, s))
}

/// The MTD of the full shape out of the box.
fn mtd(b: &Ground, x: Real, y: Real, s: &SpawnShape) -> Mtd {
    match *s {
        SpawnShape::Sphere { r } => capsule_mtd(b, x, y, r, 0.0, 0.0, 1.0),
        SpawnShape::Capsule { r, hh, ux, uy } => capsule_mtd(b, x, y, r, axis_half(r, hh), ux, uy),
        SpawnShape::Box { hx, hz, ux, uy } => box_mtd(b, x, y, hx, hz, ux, uy),
    }
}

/// The place an actor whose root has the shape `s`, spawned at (x, y), is moved to (the same place when it is not
/// moved), against the boxes (and the polygon solids) that block it.
pub fn find_teleport_spot_shape(boxes: Shapes, x: Real, y: Real, s: &SpawnShape) -> V2 {
    let at = find_teleport_spot_at(boxes, x, y, s);
    V2 { x: at.x, y: at.y }
}

/// The same with FindTeleportSpot's return: `ok` false where it says false (a nearly zero adjustment, no try
/// clear), the place then the one asked.
pub fn find_teleport_spot_at(boxes: Shapes, x: Real, y: Real, s: &SpawnShape) -> FindTeleportSpotAtOut {
    if !encroaches(boxes, x, y, s) {
        return FindTeleportSpotAtOut { x, y, ok: true };
    }
    let mut ax = 0.0;
    let mut ay = 0.0;
    for b in boxes.iter() {
        if !overlaps_shape(b, x, y, s) {
            continue;
        }
        let m = match b {
            ShapeRef::Box(g) => mtd(g, x, y, s),
            ShapeRef::Poly(q) => poly_mtd(q, x, y, s),
        };
        ax += m.nx * m.depth;
        ay += m.ny * m.depth;
    }
    if ax.abs() <= KINDA_SMALL_NUMBER_F64 && ay.abs() <= KINDA_SMALL_NUMBER_F64 {
        return FindTeleportSpotAtOut { x, y, ok: false };
    }
    if ay.abs() > KINDA_SMALL_NUMBER_F64 && !encroaches(boxes, x, y + crate::math::narrow(ay), s) {
        return FindTeleportSpotAtOut { x, y: y + crate::math::narrow(ay), ok: true };
    }
    if !encroaches(boxes, x + crate::math::narrow(ax), y, s) {
        return FindTeleportSpotAtOut { x: x + crate::math::narrow(ax), y, ok: true };
    }
    if !encroaches(boxes, x + crate::math::narrow(ax), y + crate::math::narrow(ay), s) {
        return FindTeleportSpotAtOut { x: x + crate::math::narrow(ax), y: y + crate::math::narrow(ay), ok: true };
    }
    FindTeleportSpotAtOut { x, y, ok: false }
}

/// The same for a sphere root of radius `r` (Batman's smoke grenade, Arya's kill pie).
pub fn find_teleport_spot(boxes: Shapes, x: Real, y: Real, r: Real) -> V2 {
    find_teleport_spot_shape(boxes, x, y, &SpawnShape::Sphere { r })
}

/// The place a `World.actors` spawn of Comp_Spawner (collision handling 2) has after PostActorConstruction's
/// FindTeleportSpot: (x, y) moved out of what the root Capsule blocks (`root_body_blockers`: the template's
/// profile; none, the shape constructor's OverlapAllDynamic: the platforms of `blockers(Dynamic)`), upright at the
/// template's scale. An attached spawn (AttachToActor) is put back by Comp_Spawner (@10335-10581): not moved.
/// For the classes whose construction script leaves the Capsule's collision as the template has it.
pub fn spawn_spot(w: &mut World, r: &SpawnSpotR, a: &SpawnSpotA, x: Real, y: Real) -> V2 {
    let Some(cap) = obj(&a.components, "Capsule") else { return V2 { x, y } };
    if truthy(&r.p, "AttachToActor") {
        return V2 { x, y };
    }
    let body = obj(cap, "BodyInstance");
    let Some(boxes) = super::spawning::root_body_blockers(w, body, r.cls, name!("spawn push-out")) else {
        return V2 { x, y };
    };
    let scale = obj(cap, "RelativeScale3D").and_then(|s| num(s, "X")).unwrap_or(1.0);
    find_teleport_spot_shape(boxes.view(), x, y, &capsule_shape(Some(capsule_c(cap)), scale, 0.0, 1.0))
}

/// `fighterTeleportTo`: fighter `f` teleported to (`x`, `actor_y`) through
/// FindTeleportSpot against its blocking boxes; false (the fighter stays) when no place is free, else the unswept
/// move and true.
///
/// AActor::TeleportTo of a fighter (K2_TeleportTo(location, rotation) on the pawn: MarkTeleportProjectile @4912), at
/// (x, actor_y) its actor location (the root CapsuleComponent's centre). With the capsule's collision on,
/// FindTeleportSpot (once begun play: the one XY adjustment) against what blocks the capsule: its walking boxes, the
/// spawned boxes (a ship's pieces, a SpawnedPlatformParent child's box, a projectile's box), the stage platforms it
/// does not ignore and the polygon stage solids that block it (`fighter_polys`).
// Unverified: a polygon solid is met by `poly_overlaps` / `poly_mtd` (the port's nearest point and least way out), not
// PhysX's convex overlap and MTD.
// Not ported: TeleportSucceeded (+0x500) and the movement component's teleport handling after the move.
// Deviation: the capsule is upright in the play plane; the other bodies of the Pawn profile's blocking channels are
// not gathered.
pub fn fighter_teleport_to(w: &mut World, f: FighterId, x: Real, actor_y: Real) -> bool {
    use crate::fighter::body::{self, PlatformRef, cap_radius, capsule_half, ignores_platform, plat, span_near};
    let (mut x, mut actor_y) = (x, actor_y);
    if !body::no_collision(w, f) {
        let mut boxes = body::floor_list(w, f);
        let mut k = 0;
        while k < w.fighter(f).platforms.platforms.len() {
            let pr = w.fighter(f).platforms.platforms[k];
            k += 1;
            let p = plat(w, pr);
            if p.stage_solid || p.dead || ignores_platform(w, f, pr) {
                continue;
            }
            // (a ship's run of pieces near the target)
            if let Some(s) = span_near(w, &p, x, cap_radius(w, f)) {
                boxes.push(Ground {
                    left: f64::from(s.left),
                    right: f64::from(s.right),
                    bottom: f64::from(p.bottom),
                    top: f64::from(p.top),
                });
            }
        }
        let mut k = 0;
        while k < w.fighter(f).platforms.stage_platforms.len() {
            let i = w.fighter(f).platforms.stage_platforms[k];
            k += 1;
            if !ignores_platform(w, f, PlatformRef::Stage(i)) {
                boxes.push(plat(w, PlatformRef::Stage(i)).ground());
            }
        }
        let (r, hh) = (cap_radius(w, f), capsule_half(w, f));
        let s = if r < hh { SpawnShape::Capsule { r, hh, ux: 0.0, uy: 1.0 } } else { SpawnShape::Sphere { r } };
        let mut polys = Vec::new();
        crate::fighter::traces::solids::fighter_polys(w, f, &mut polys);
        let at = find_teleport_spot_at(Shapes { boxes: &boxes, polys: &polys }, x, actor_y, &s);
        body::floor_list_done(w, boxes);
        if !at.ok {
            return false;
        }
        x = at.x;
        actor_y = at.y;
    }
    crate::fighter::moves::recall_move(w, f, x, actor_y);
    true
}

/// Comp_Spawner CompSpawnActor's place for a notify with a socket (as Projectile's constructor): the mesh socket
/// plus the notify's translation, Y along the owner's facing; with socket None the actor's location - (0, 0, 100).
pub fn spawn_place(w: &mut World, f: FighterId, np: &Props) -> V2 {
    let t = obj(np, "Transforms").and_then(|t| obj(t, "Translation"));
    let ty = t.and_then(|t| num(t, "Y")).unwrap_or(0.0);
    let tz = t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
    let socket = np.get("Socket").and_then(Value::as_str).filter(|s| *s != "None" && !s.is_empty());
    let facing = crate::fighter::facing(w, f);
    if let Some(socket) = socket {
        let ch = w.fighter(f).ch;
        let si =
            w.names.get(&format!("socket:{socket}")).and_then(|s| w.fixed.samplers.get(&ch)?.by_name.get(&s).copied());
        let o = si.and_then(|si| {
            let b = w.fixed.samplers.get(&ch)?;
            b.offsets.get(usize::try_from(si).ok()?).copied()
        });
        if let Some(o) = o
            && let Some(pose) = crate::fighter::anim::sampled_boxes(w, f)
        {
            let o = usize::try_from(o).expect("a box offset");
            let (fwd, up) = (crate::real(pose[o]), crate::real(pose[o + 1]));
            crate::fighter::anim::put_pose(w, pose);
            return V2 {
                x: crate::fighter::mesh_wx(w, f) + (fwd + ty) * facing,
                y: crate::fighter::mesh_wy(w, f) + up + tz,
            };
        }
        crate::fighter::common::warn_text(w, &format!("spawn socket missing: {socket}"));
    }
    let x = w.fighter(f).body.x;
    V2 { x: x + ty * facing, y: crate::fighter::actor_y(w, f) - 100.0 + tz }
}

/// `k2TeleportTo`: K2_TeleportTo(location, the actor's rotation) of a spawned actor by
/// another class's graph (Comp_Pawn_Portal TeleportActor @543 / @686): AActor::TeleportTo (read in the exe, as
/// FindTeleportSpot): a primitive root with collision runs FindTeleportSpot at the place (once begun play: the one
/// adjustment as it is, which `find_teleport_spot_at` tries); false there leaves the actor where it was and returns false;
/// else an unswept move. Rotation and velocity are kept. True: moved.
/// - A Projectile: its root Capsule against what it blocks (`root_blockers`: the stage block and the platforms its
///   profile blocks; NoCollision or turned off: no test), at its scale; the move sets the previous place too.
/// - An Item out of a hand, or a World.actors entry with carry hooks (`CarryHooks.place`): placed.
///
/// Not ported: an Item's root and a World.actors entry's root are placed with no FindTeleportSpot test (their roots'
/// blocking boxes are not gathered here); a held Item (attached: TeleportTo moves the root off its socket until the
/// next attachment update) is refused with a warning.
pub fn k2_teleport_to(w: &mut World, a: ActorRef, x: Real, y: Real) -> bool {
    let k = crate::actors::world_actor::actor_entry(w, a).and_then(|e| e.carry);
    match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get(id) else { return false };
            let (cls, scale, dead, no_col) = (p.cls, p.scale, p.dead, p.no_collision);
            // The root's collision: a graph's SetCollisionEnabled where one wrote it, else the template's.
            let on = if p.cap_written { crate::actors::projectile::capsule_on(p) } else { !dead && !no_col };
            let (mut x, mut y) = (x, y);
            if on && let Some(boxes) = crate::world::spawning::root_blockers(w, id, name!("K2_TeleportTo spot test")) {
                let owner = ActorRef::Fighter(crate::fighter::common::lookup_owner(w, a));
                let cap = crate::fighter::common::actor_def(w, owner, cls)
                    .and_then(|d| obj(&d.components, "Capsule"))
                    .map(capsule_c);
                let at = find_teleport_spot_at(boxes.view(), x, y, &capsule_shape(cap, scale, 0.0, 1.0));
                if !at.ok {
                    return false;
                }
                x = at.x;
                y = at.y;
            }
            let place = match k {
                Some(k) => k.place,
                None => crate::actors::tunnel_carry::carry_hooks(w, a).expect("the shared Projectile hooks").place,
            };
            place(w, a, x, y);
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.prev_x = p.x;
                p.prev_y = p.y;
            }
            true
        }
        ActorRef::Item(id) => {
            let Some(i) = w.actors.items.get(id) else { return false };
            if i.holder >= 0 {
                let msg = format!("K2_TeleportTo of a held item not ported ({})", w.names.str(i.cls));
                crate::fighter::common::warn_text(w, &msg);
                return false;
            }
            match k {
                Some(k) => (k.place)(w, a, x, y),
                None => {
                    if let Some(i) = w.actors.items.get_mut(id) {
                        i.x = x;
                        i.y = y;
                    }
                }
            }
            true
        }
        _ => {
            if let Some(k) = k {
                (k.place)(w, a, x, y);
                return true;
            }
            let cls = crate::actors::world_actor::actor_cls(w, a).map_or("an actor", |c| w.names.str(c));
            let msg = format!("{cls} has no carry hooks (registry CarryHooks): K2_TeleportTo not ported");
            crate::fighter::common::warn_text(w, &msg);
            false
        }
    }
}
