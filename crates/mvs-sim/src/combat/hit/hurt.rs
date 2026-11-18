//! A fighter's hurt shapes against spheres and boxes, the one entry of every overlap test against a fighter:
//! `boxes.rs` spheresHitHurt on its own set (as points where CharHooks.normalHitboxesOff holds:
//! Pawn_Arya's CopyCat montages), then the clone shapes of a Disguise's second mesh while a copy plays
//! (`combat::disguise`; not the Mesh's children).

use crate::Real;
use crate::combat::boxes::{self, BoxSampler, WorldSphere};
use crate::combat::disguise;
use crate::fighter::FighterId;
use crate::fighter::{extra_hurt, mesh_wx, mesh_wy};
use crate::world::World;
use std::sync::Arc;

/// The victim's box tables, its hurt shapes' attach order and whether its hitboxes are off (points).
fn sampler(w: &World, vic: FighterId) -> (Arc<crate::world::Fixed>, bool) {
    let off = w.fighter(vic).hooks.char.normal_hitboxes_off.is_some_and(|h| h(w, vic));
    (w.fixed.clone(), off)
}

fn boxes_of<'a>(fx: &'a crate::world::Fixed, w: &World, vic: FighterId) -> &'a BoxSampler {
    let ch = w.fighter(vic).ch;
    fx.samplers.get(&ch).unwrap_or_else(|| panic!("no box sampler for {}", w.names.str(ch)))
}

/// `hurtHit`.
pub fn hurt_hit(w: &mut World, vic: FighterId, sp: &[WorldSphere], vp: &[f32]) -> Option<WorldSphere> {
    let (fx, points) = sampler(w, vic);
    let b = boxes_of(&fx, w, vic);
    let (x, y, extra) = (mesh_wx(w, vic), mesh_wy(w, vic), extra_hurt(w, vic));
    let v = w.fighter(vic);
    let hit_fn = if points { boxes::spheres_hit_hurt_points } else { boxes::spheres_hit_hurt };
    let hit = hit_fn(b, sp, vp, x, y, v.body.facing_dir, extra.as_ref(), Some(&v.hit.hurt_off));
    if hit.is_some() || v.shapes.disguise.is_none() {
        return hit;
    }
    disguise::clone_hurt_hit(w, vic, sp)
}

/// `hurtRank`: the place of the first hurt shape of the victim a hit sphere touches, for the overlap update's
/// ranking: its own Mesh's shapes in attach order (as points with its hitboxes off, the same places), the root
/// capsule's last, then the clone shapes of a Disguise behind them all (the second mesh is attached behind the Mesh's
/// own children); -1 for none.
pub fn hurt_rank(w: &mut World, vic: FighterId, s: &WorldSphere, vp: &[f32]) -> Real {
    let (fx, points) = sampler(w, vic);
    let b = boxes_of(&fx, w, vic);
    let (x, y, extra) = (mesh_wx(w, vic), mesh_wy(w, vic), extra_hurt(w, vic));
    let v = w.fighter(vic);
    let rank_fn = if points { boxes::first_hurt_points } else { boxes::first_hurt };
    let r = rank_fn(b, s, vp, x, y, v.body.facing_dir, extra.as_ref(), Some(&v.hit.hurt_off));
    if r >= 0.0 || v.shapes.disguise.is_none() {
        return r;
    }
    let n = b.hurt.len() as Real;
    let k = disguise::clone_first_hurt(w, vic, s);
    if k >= 0.0 { n + 1.0 + k } else { -1.0 }
}

/// `boxHurtHit`: the same fighter against a box hitbox (upright in the play plane, extruded along the depth: an
/// actor's Box component as a hit shape), by the same three rules.
pub fn box_hurt_hit(w: &mut World, vic: FighterId, cx: Real, cy: Real, hx: Real, hy: Real, vp: &[f32]) -> bool {
    let (fx, points) = sampler(w, vic);
    let b = boxes_of(&fx, w, vic);
    let (x, y, extra) = (mesh_wx(w, vic), mesh_wy(w, vic), extra_hurt(w, vic));
    let v = w.fighter(vic);
    let box_fn = if points { boxes::box_hits_hurt_points } else { boxes::box_hits_hurt };
    let hit = box_fn(b, cx, cy, hx, hy, vp, x, y, v.body.facing_dir, extra.as_ref(), Some(&v.hit.hurt_off));
    if hit || v.shapes.disguise.is_none() {
        return hit;
    }
    disguise::clone_box_hits_hurt(w, vic, cx, cy, hx, hy)
}

/// `hurtTouched`: every hurt shape of the fighter a sphere touches, for a caller that keeps the begin of an overlap
/// per pair of components: `out` is emptied and takes the places `hurt_rank` gives (its own shapes in attach order,
/// the root capsule's at the hurt count, a Disguise's clones from the hurt count + 1 on). Returns the count.
pub fn hurt_touched(w: &mut World, vic: FighterId, s: &WorldSphere, vp: &[f32], out: &mut Vec<Real>) -> usize {
    let (fx, points) = sampler(w, vic);
    let b = boxes_of(&fx, w, vic);
    let (x, y, extra) = (mesh_wx(w, vic), mesh_wy(w, vic), extra_hurt(w, vic));
    let v = w.fighter(vic);
    let touch_fn = if points { boxes::hurts_touched_points } else { boxes::hurts_touched };
    touch_fn(b, s, vp, x, y, v.body.facing_dir, extra.as_ref(), Some(&v.hit.hurt_off), out);
    if v.shapes.disguise.is_some() {
        let base = b.hurt.len() as Real + 1.0;
        disguise::clone_hurts_touched(w, vic, s, base, out);
    }
    out.len()
}
