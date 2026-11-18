//! Gizmo's popcorn as the record writes it (the hide box is `barrier.rs`'s): the keys the
//! class adds to a projectile. `trigBox` and `rootBox` are the extra dealing boxes' objects (`PopcornTriggerBox`,
//! `PopcornRootBox`): each holds its popcorn (`pr`, written as the reference) and its own sets; the boxes' `list` is
//! the view `extraHitBoxes` returns, not written.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::chars::gizmo::popcorn::{Popcorn, RootBox};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, as_obj, check_keys, obj, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! popcorn_head {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "hidden" = hidden, "growScale" = grow_scale, "growPos" = grow_pos, "hitGate" = hit_gate,
            "exploded" = exploded, "pops" = pops, "popStart" = pop_start, "gateStart" = gate_start,
            "bumpAt" = bump_at, "lastAttacker" = last_attacker, "homingTarget" = homing_target,
            "shotHit" = shot_hit, "shotFire" = shot_fire, "shotPos" = shot_pos, "shotX" = shot_x,
            "shotY" = shot_y, "shotNx" = shot_nx, "shotNy" = shot_ny, "boxX" = box_x, "boxY" = box_y,
        )
    };
}

/// The popcorn's own reference (a box's `pr`): the projectile whose extension is `s`.
fn self_ref(s: &Popcorn, x: &Wx) -> Value {
    for (id, p) in x.w.actors.projectiles.iter() {
        if matches!(&p.ext, ProjectileExt::Popcorn(c) if std::ptr::eq(c, s)) {
            return ActorRef::Projectile(id).to_value(x);
        }
    }
    x.unmapped("a popcorn's own box of a projectile no list holds");
    Value::Null
}

fn root_box_put(s: &Popcorn, r: &RootBox, x: &Wx) -> Value {
    obj(vec![
        ("hitSet", r.hit_set.to_value(x)),
        ("allyOver", r.ally_over.to_value(x)),
        ("pr", self_ref(s, x)),
        ("hit", r.hit.to_value(x)),
    ])
}

fn root_box_load(r: &mut RootBox, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    plain_keys!(load r, ob, l; "hitSet" = hit_set, "allyOver" = ally_over, "hit" = hit);
    check_keys(ob, &["hitSet", "allyOver", "pr", "hit"], l);
}

/// A C023A_PopCorn_C's own keys.
pub fn popcorn_put(s: &Popcorn, x: &Wx, o: &mut Vec<(String, Value)>) {
    popcorn_head!(plain_keys put s, x, o;);
    let root = match &s.root_box {
        Some(r) => root_box_put(s, r, x),
        None => Value::Null,
    };
    o.push(("rootBox".to_owned(), root));
    o.push(("trigBox".to_owned(), obj(vec![("allyOver", s.trig_ally_over.to_value(x)), ("pr", self_ref(s, x))])));
    plain_keys!(put s, x, o; "inTrigger" = in_trigger);
}

/// [`popcorn_put`]'s inverse; the keys it knows go into `known`.
pub fn popcorn_load(s: &mut Popcorn, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    popcorn_head!(plain_keys load s, ob, l;);
    s.root_box = match ob.get("rootBox") {
        None => {
            l.miss("key rootBox absent");
            None
        }
        Some(Value::Null) => None,
        Some(v) => l.at("rootBox", |l| {
            let mut r = RootBox::default();
            root_box_load(&mut r, v, l);
            Some(r)
        }),
    };
    match ob.get("trigBox") {
        Some(v) => l.at("trigBox", |l| {
            let Some(t) = as_obj(v, l) else { return };
            match t.get("allyOver") {
                Some(a) => l.at("allyOver", |l| s.trig_ally_over.load_value(a, l)),
                None => l.miss("key allyOver absent"),
            }
            check_keys(t, &["allyOver", "pr"], l);
        }),
        None => l.miss("key trigBox absent"),
    }
    plain_keys!(load s, ob, l; "inTrigger" = in_trigger);
    known.extend(popcorn_head!(plain_keys names));
    known.extend(["rootBox", "trigBox", "inTrigger"]);
}
