//! The tether's classes as the record writes them: StevenTetherStunProjectile_C,
//! Buff_BubbleCount's and Buff_StevenBubbleEnemy's extensions.

use mvs_sim::chars::steven::tether::{BubbleCount, BubbleEnemy, StevenTether};
use mvs_trace::{Obj, Value};

use crate::conv::{check_keys, plain_keys};
use crate::cx::{Lx, Wx};

/// a StevenTetherStunProjectile_C as the record writes it: the keys it adds to its projectile's `rest`.
pub fn tether_put(s: &StevenTether, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o;
        "pinned" = pinned, "ally" = ally, "stopOnce" = stop_once, "endVfx" = end_vfx, "at03" = at03, "at1" = at1,
        "knockAt" = knock_at, "destroyAt" = destroy_at, "retryAt" = retry_at, "retryWho" = retry_who, "over" = over,
        "overlapped" = overlapped, "lastCyl" = last_cyl, "slope" = slope, "gemX" = gem_x, "gemY" = gem_y,
        "knocked" = knocked, "vfxOn" = vfx_on, "vfxTriggered" = vfx_triggered, "vfxAt" = vfx_at,);
}

/// [`tether_put`]'s inverse; the keys it knows go into `known`.
pub fn tether_load(s: &mut StevenTether, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l;
        "pinned" = pinned, "ally" = ally, "stopOnce" = stop_once, "endVfx" = end_vfx, "at03" = at03, "at1" = at1,
        "knockAt" = knock_at, "destroyAt" = destroy_at, "retryAt" = retry_at, "retryWho" = retry_who, "over" = over,
        "overlapped" = overlapped, "lastCyl" = last_cyl, "slope" = slope, "gemX" = gem_x, "gemY" = gem_y,
        "knocked" = knocked, "vfxOn" = vfx_on, "vfxTriggered" = vfx_triggered, "vfxAt" = vfx_at,);
    known.extend(plain_keys!(names
        "pinned" = pinned, "ally" = ally, "stopOnce" = stop_once, "endVfx" = end_vfx, "at03" = at03, "at1" = at1,
        "knockAt" = knock_at, "destroyAt" = destroy_at, "retryAt" = retry_at, "retryWho" = retry_who, "over" = over,
        "overlapped" = overlapped, "lastCyl" = last_cyl, "slope" = slope, "gemX" = gem_x, "gemY" = gem_y,
        "knocked" = knocked, "vfxOn" = vfx_on, "vfxTriggered" = vfx_triggered, "vfxAt" = vfx_at,));
}

/// Buff_BubbleCount_C's extension as the oracle writes it (`null` before it has one).
pub fn count_put(e: &BubbleCount, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put e, x, o; "from" = from,);
    Value::Obj(Obj::new(o))
}

/// [`count_put`]'s inverse.
pub fn count_load(e: &mut BubbleCount, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = BubbleCount::default(),
        Value::Obj(o) => {
            e.begun = true;
            plain_keys!(load e, o, l; "from" = from,);
            check_keys(o, plain_keys!(names "from" = from,), l);
        }
        _ => l.miss(format_args!("Buff_BubbleCount_C's extension is neither null nor an object")),
    }
}

/// Buff_StevenBubbleEnemy_C's extension as the oracle writes it (`null` before it has one).
pub fn bubble_enemy_put(e: &BubbleEnemy, x: &Wx) -> Value {
    if !e.begun {
        return Value::Null;
    }
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put e, x, o;
        "from" = from, "age" = age, "bindAt" = bind_at, "bound" = bound, "stopTick" = stop_tick, "kbAt" = kb_at,
        "closed" = closed, "openAt" = open_at, "moveAt" = move_at, "stopAt" = stop_at,);
    Value::Obj(Obj::new(o))
}

/// [`bubble_enemy_put`]'s inverse.
pub fn bubble_enemy_load(e: &mut BubbleEnemy, v: &Value, l: &mut Lx) {
    match v {
        Value::Null => *e = BubbleEnemy::default(),
        Value::Obj(o) => {
            e.begun = true;
            plain_keys!(load e, o, l;
                "from" = from, "age" = age, "bindAt" = bind_at, "bound" = bound, "stopTick" = stop_tick,
                "kbAt" = kb_at, "closed" = closed, "openAt" = open_at, "moveAt" = move_at, "stopAt" = stop_at,);
            check_keys(
                o,
                plain_keys!(names
                    "from" = from, "age" = age, "bindAt" = bind_at, "bound" = bound, "stopTick" = stop_tick,
                    "kbAt" = kb_at, "closed" = closed, "openAt" = open_at, "moveAt" = move_at, "stopAt" = stop_at,),
                l,
            );
        }
        _ => l.miss(format_args!("Buff_StevenBubbleEnemy_C's extension is neither null nor an object")),
    }
}
