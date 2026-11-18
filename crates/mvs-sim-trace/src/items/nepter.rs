//! ItemHelper_Nepter as the record writes it.

use mvs_sim::items::nepter::Nepter;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A ItemHelper_Nepter_C as the record writes it.
pub fn nepter_put(s: &Nepter, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    plain_keys!(put s, x, o; "aimActors" = aim_actors, "anim" = anim, "bindTick" = bind_tick, "blastAt" = blast_at, "blastBounds" = blast_bounds, "born" = born, "depth" = depth, "facingLeft" = facing_left, "inside" = inside, "isJumping" = is_jumping, "jumpCount" = jump_count, "lastThrowTime" = last_throw_time, "lifeTick" = life_tick, "ovenLight" = oven_light, "radius" = radius, "scaleY" = scale_y, "team" = team, "throwAt" = throw_at, "ticking" = ticking, "tredAlpha" = tred_alpha, "wallSlide" = wall_slide);
    // (`p`: the class's parameters, static data)
    o.push(("p".into(), crate::actors::static_object()));
}

/// [`nepter_put`]'s inverse; the keys it knows go into `known`.
pub fn nepter_load(s: &mut Nepter, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    if let Some(v) = r.x {
        s.x = mvs_sim::math::narrow(v);
    }
    if let Some(v) = r.y {
        s.y = mvs_sim::math::narrow(v);
    }
    plain_keys!(load s, ob, l; "aimActors" = aim_actors, "anim" = anim, "bindTick" = bind_tick, "blastAt" = blast_at, "blastBounds" = blast_bounds, "born" = born, "depth" = depth, "facingLeft" = facing_left, "inside" = inside, "isJumping" = is_jumping, "jumpCount" = jump_count, "lastThrowTime" = last_throw_time, "lifeTick" = life_tick, "ovenLight" = oven_light, "radius" = radius, "scaleY" = scale_y, "team" = team, "throwAt" = throw_at, "ticking" = ticking, "tredAlpha" = tred_alpha, "wallSlide" = wall_slide);
    known.push("p");
    known.extend(plain_keys!(names "aimActors" = aim_actors, "anim" = anim, "bindTick" = bind_tick, "blastAt" = blast_at, "blastBounds" = blast_bounds, "born" = born, "depth" = depth, "facingLeft" = facing_left, "inside" = inside, "isJumping" = is_jumping, "jumpCount" = jump_count, "lastThrowTime" = last_throw_time, "lifeTick" = life_tick, "ovenLight" = oven_light, "radius" = radius, "scaleY" = scale_y, "team" = team, "throwAt" = throw_at, "ticking" = ticking, "tredAlpha" = tred_alpha, "wallSlide" = wall_slide));
}
