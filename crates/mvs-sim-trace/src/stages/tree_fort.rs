//! Tree Fort's state as the record writes it (`stages/tree-fort/`): the entry's own state (`StageRun.s`, `{ tick }`)
//! and a tree's own keys (`TreePillar` beside the `StageActor` ones).

use mvs_sim::stages::tree_fort::pillar::{BounceFx, PillarWiggle};
use mvs_sim::stages::tree_fort::{Pillar, TreeFortOwn};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(TreeFortOwn { "tick" = tick });
plain_struct!(PillarWiggle { "bone" = bone, "dx" = dx, "dy" = dy, "dz" = dz, "t" = t });
plain_struct!(BounceFx { "x" = x, "y" = y, "z" = z, "pitch" = pitch });

/// `StageRun.s` of Tree Fort.
pub fn own_put(s: &TreeFortOwn, x: &Wx) -> Value {
    s.to_value(x)
}

pub fn own_load(s: &mut TreeFortOwn, v: &Value, l: &mut Lx) {
    s.load_value(v, l);
}

/// A `TreePillar`'s own keys (beside the `StageActor` ones), pushed on `o`.
pub fn pillar_put(a: &Pillar, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put a, x, o;
        "hp" = hp, "startHp" = start_hp, "damageStage" = damage_stage, "isRight" = is_right, "doDmg" = do_dmg,
        "dyingAt" = dying_at, "hitAt" = hit_at, "damageAt" = damage_at, "bounceAt" = bounce_at,
        "bounceFx" = bounce_fx, "wiggles" = wiggles, "bones" = bones, "limitActors" = limit_actors,
        "limitTimers" = limit_timers, "limitTime" = limit_time,
    );
}

/// Loads them from `ob`, each key read pushed on `known`.
pub fn pillar_load(a: &mut Pillar, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load a, ob, l;
        "hp" = hp, "startHp" = start_hp, "damageStage" = damage_stage, "isRight" = is_right, "doDmg" = do_dmg,
        "dyingAt" = dying_at, "hitAt" = hit_at, "damageAt" = damage_at, "bounceAt" = bounce_at,
        "bounceFx" = bounce_fx, "wiggles" = wiggles, "bones" = bones, "limitActors" = limit_actors,
        "limitTimers" = limit_timers, "limitTime" = limit_time,
    );
    known.extend(plain_keys!(names
        "hp" = hp, "startHp" = start_hp, "damageStage" = damage_stage, "isRight" = is_right, "doDmg" = do_dmg,
        "dyingAt" = dying_at, "hitAt" = hit_at, "damageAt" = damage_at, "bounceAt" = bounce_at,
        "bounceFx" = bounce_fx, "wiggles" = wiggles, "bones" = bones, "limitActors" = limit_actors,
        "limitTimers" = limit_timers, "limitTime" = limit_time,
    ));
}
