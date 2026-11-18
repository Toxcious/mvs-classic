//! The shapes two tether projectiles share: Reindog's TetherProjectile_C
//! (`chars::creature::tether`) and Steven's StevenTetherStunProjectile_C. Both graphs read the instigator's mesh
//! socket Gem, test the fighters' root capsules against the PlayerAttachCollision sphere and against the Cylinder
//! mesh laid from the Gem to the actor (each class's module has its Unverified notes for the shapes).

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::Projectile;
use crate::fighter::anim::{put_pose, sampled_boxes};
use crate::fighter::common::{actor_def, warn_text};
use crate::fighter::{FighterId, actor_y, cap_half, cap_radius, facing, mesh_wx, mesh_wy};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::props::{num, obj};
use crate::world::World;

/// `gemOf`: the owner's mesh socket Gem in the play plane (the mesh's own place when the data lacks it, warned once
/// naming `cls`). Creature's box tracks carry Creature_AnimBP's ApplyAdditive of Creature_Heart_Pulse on
/// fml_un_C_gem (+13.287 cm on the parent's Z: the boxes' POSE_ADDITIVE), and the sampled boxes ModifyBone_1's spin.
pub fn gem_of(w: &World, f: FighterId, cls: Name) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let at = w.fixed.samplers.get(&ch).and_then(|b| {
        let si = *b.by_name.get(&name!("socket:Gem"))?;
        b.offsets.get(usize::try_from(si).ok()?).copied()
    });
    if let Some(o) = at
        && let Some(pose) = sampled_boxes(w, f)
    {
        let o = usize::try_from(o).expect("a box offset");
        let r = (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]));
        put_pose(w, pose);
        return r;
    }
    let msg = format!("{}: no Gem socket on {}, the beam starts at the mesh", w.names.str(cls), w.names.str(ch));
    warn_text(w, &msg);
    (mesh_wx(w, f), mesh_wy(w, f))
}

/// `sphereHits`: the sphere (x, y, r) meets fighter `v`'s root capsule.
pub fn sphere_hits(w: &World, x: Real, y: Real, r: Real, v: FighterId) -> bool {
    let cr = cap_radius(w, v);
    let h = max(cap_half(w, v) - cr, 0.0);
    let dy = max((y - actor_y(w, v)).abs() - h, 0.0);
    let dx = x - w.fighter(v).body.x;
    dx * dx + dy * dy <= (r + cr) * (r + cr)
}

/// `rectHits`: the Cylinder laid from (gx, gy) along the unit (ux, uy) for `len`, half width `hw`, meets fighter
/// `v`'s root capsule: the distance from the capsule's axis to the rectangle [0, len] x [-hw, hw], in the
/// rectangle's frame, no more than the capsule's radius.
#[allow(clippy::too_many_arguments, reason = "the function's parameters")]
pub fn rect_hits(w: &World, gx: Real, gy: Real, ux: Real, uy: Real, len: Real, hw: Real, v: FighterId) -> bool {
    let cr = cap_radius(w, v);
    let h = max(cap_half(w, v) - cr, 0.0);
    // the capsule axis's ends in the rectangle's frame
    let ay = actor_y(w, v);
    let (dx, dy_a, dy_b) = (w.fighter(v).body.x - gx, ay - h - gy, ay + h - gy);
    let d = seg_box_dist(
        dx * ux + dy_a * uy,
        -dx * uy + dy_a * ux,
        dx * ux + dy_b * uy,
        -dx * uy + dy_b * ux,
        [0.0, len, -hw, hw],
    );
    d <= cr
}

/// `pointBoxDist`: (px, py) to the box [x0, x1] x [y0, y1].
fn point_box_dist(px: Real, py: Real, b: [Real; 4]) -> Real {
    let [x0, x1, y0, y1] = b;
    let dx = max(max(x0 - px, 0.0), px - x1);
    let dy = max(max(y0 - py, 0.0), py - y1);
    (dx * dx + dy * dy).sqrt()
}

/// `pointSegDist`: (px, py) to the segment a -> b.
fn point_seg_dist(px: Real, py: Real, ax: Real, ay: Real, bx: Real, by: Real) -> Real {
    let (ex, ey) = (bx - ax, by - ay);
    let l2 = ex * ex + ey * ey;
    let t = if l2 > 0.0 { min(max(((px - ax) * ex + (py - ay) * ey) / l2, 0.0), 1.0) } else { 0.0 };
    let (dx, dy) = (ax + ex * t - px, ay + ey * t - py);
    (dx * dx + dy * dy).sqrt()
}

/// One Liang-Barsky clip of the segment's span [t0, t1].
fn clip(p: Real, q: Real, t: &mut (Real, Real)) -> bool {
    if p == 0.0 {
        return q >= 0.0;
    }
    let r = q / p;
    if p < 0.0 {
        if r > t.1 {
            return false;
        }
        if r > t.0 {
            t.0 = r;
        }
    } else {
        if r < t.0 {
            return false;
        }
        if r < t.1 {
            t.1 = r;
        }
    }
    true
}

/// `segBoxDist`: a segment and an axis-aligned box [x0, x1] x [y0, y1]: 0 when the segment crosses it
/// (Liang-Barsky), else the least of the endpoints' distances to the box and the corners' to the segment (two convex
/// sets apart meet at a vertex).
fn seg_box_dist(ax: Real, ay: Real, bx: Real, by: Real, b: [Real; 4]) -> Real {
    let [x0, x1, y0, y1] = b;
    let mut t = (0.0, 1.0);
    let (dx, dy) = (bx - ax, by - ay);
    if clip(-dx, ax - x0, &mut t)
        && clip(dx, x1 - ax, &mut t)
        && clip(-dy, ay - y0, &mut t)
        && clip(dy, y1 - ay, &mut t)
        && t.0 <= t.1
    {
        return 0.0;
    }
    let ds = [
        point_box_dist(bx, by, b),
        point_seg_dist(x0, y0, ax, ay, bx, by),
        point_seg_dist(x1, y0, ax, ay, bx, by),
        point_seg_dist(x0, y1, ax, ay, bx, by),
        point_seg_dist(x1, y1, ax, ay, bx, by),
    ];
    ds.into_iter().fold(point_box_dist(ax, ay, b), min)
}

/// `attachRadius`: the PlayerAttachCollision's radius: SphereRadius x the component's scale (its parents' are 1).
pub fn attach_radius(w: &World, p: &Projectile) -> Real {
    let owner = p.owner.expect("a tether has an owner");
    let c = actor_def(w, ActorRef::Fighter(owner), p.cls).and_then(|d| obj(&d.components, "PlayerAttachCollision"));
    let r = c.and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0);
    r * c.and_then(|c| obj(c, "RelativeScale3D")).and_then(|s| num(s, "X")).unwrap_or(1.0)
}
