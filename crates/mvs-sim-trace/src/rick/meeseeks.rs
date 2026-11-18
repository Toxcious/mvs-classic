//! Rick's Meeseeks as the record writes it: its own keys (`activated` is the projectile's).
//! `dirZero` is declared, not initialised: present only once set.

use mvs_sim::chars::rick::meeseeks::MeeseeksSummon;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! summon_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "mode" = mode, "conds" = conds, "facingLeft" = facing_left, "handlerLeft" = handler_left, "dir" = dir,
            "dirZero" ~ dir_zero, "running" = running, "golfing" = golfing, "destroyOnEnd" = destroy_on_end,
            "bvx" = bvx, "bvy" = bvy, "bspin" = bspin, "turnClosed" = turn_closed, "turnAt" = turn_at,
            "endAt" = end_at, "frameOn" = frame_on, "frameIdx" = frame_idx, "frameTags" = frame_tags,
            "frameScale" = frame_scale, "shakeAt" = shake_at, "pendingDmg" = pending_dmg,
            "anyDamageAt" = any_damage_at, "anyDamageN" = any_damage_n, "inst" = inst, "overF" = over_f,
            "overA" = over_a, "boxA" = box_a, "bindF" = bind_f, "bindKb" = bind_kb,
        )
    };
}

/// A Meeseeks_SpawnedActor_C's own keys.
pub fn summon_put(s: &MeeseeksSummon, x: &Wx, o: &mut Vec<(String, Value)>) {
    o.push(("anim".into(), crate::parts::anim_put(&s.anim, x)));
    summon_keys!(plain_keys put s, x, o;);
}

/// [`summon_put`]'s inverse; the keys it knows go into `known`.
pub fn summon_load(s: &mut MeeseeksSummon, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    match ob.get("anim") {
        Some(v) => l.at("anim", |l| crate::parts::anim_load(&mut s.anim, v, l)),
        None => l.miss("key anim absent"),
    }
    summon_keys!(plain_keys load s, ob, l;);
    known.extend(summon_keys!(plain_keys names));
    known.push("anim");
}
