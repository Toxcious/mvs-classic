//! The ally turret's actors as the record writes them: the pilot reticle and
//! the turret bomb. The reticle's `gone` is the oracle's own destroy flag (its `dead` a getter that reads the turret's
//! flag too: the record's dead); the bomb's vx / vy are readonly 0, its `sph` the HitBox sphere it last placed.

use mvs_sim::chars::iron_giant::turret_actors::{PilotReticle, TurretBomb};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, as_obj, check_keys, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! reticle_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "flip" = flip, "pilot" = pilot,
        )
    };
}

macro_rules! bomb_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "expandT" = expand_t, "exploding" = exploding, "facing" = facing,
            "hit" = hit, "move" = mv, "nextTick" = next_tick, "scale" = scale, "sphereR" = sphere_r, "t0" = t0,
            "travel" = travel, "tx" = tx, "ty" = ty,
        )
    };
}

/// The pilot reticle's own keys and place.
pub fn reticle_put(s: &PilotReticle, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    reticle_keys!(plain_keys put s, x, o;);
    o.push(("gone".into(), Value::Bool(r.dead && !s.off)));
    o.push(("p".into(), static_object()));
}

/// [`reticle_put`]'s inverse.
pub fn reticle_load(s: &mut PilotReticle, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        reticle_keys!(plain_keys load s, ob, l;);
        let mut gone = false;
        match ob.get("gone") {
            Some(v) => l.at("gone", |l| gone.load_value(v, l)),
            None => l.miss(format_args!("key gone absent")),
        }
        s.off = r.dead && !gone;
    });
    known.extend(reticle_keys!(plain_keys names));
    known.push("gone");
}

fn sph_put(s: &TurretBomb) -> Value {
    let n = |v: f64| Value::Num(v);
    Value::List(vec![Value::Obj(Obj::new(vec![
        ("r".to_owned(), n(f64::from(s.sph_r))),
        ("x".to_owned(), n(f64::from(s.sph_x))),
        ("y".to_owned(), n(f64::from(s.sph_y))),
        ("z".to_owned(), n(0.0)),
    ]))])
}

fn sph_load(s: &mut TurretBomb, v: &Value, l: &mut Lx) {
    let Value::List(a) = v else {
        l.miss(format_args!("sph is not a list"));
        return;
    };
    let [one] = a.as_slice() else {
        l.miss(format_args!("sph holds {} spheres, not one", a.len()));
        return;
    };
    let Some(o) = as_obj(one, l) else { return };
    for (k, f) in [("r", &mut s.sph_r), ("x", &mut s.sph_x), ("y", &mut s.sph_y)] {
        match o.get(k) {
            Some(v) => l.at(k, |l| f.load_value(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        }
    }
    check_keys(o, &["r", "x", "y", "z"], l);
}

/// The turret bomb's own keys and place.
pub fn bomb_put(s: &TurretBomb, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    bomb_keys!(plain_keys put s, x, o;);
    o.push(("sph".into(), sph_put(s)));
    o.push(("p".into(), static_object()));
}

/// [`bomb_put`]'s inverse.
pub fn bomb_load(s: &mut TurretBomb, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        bomb_keys!(plain_keys load s, ob, l;);
        match ob.get("sph") {
            Some(v) => l.at("sph", |l| sph_load(s, v, l)),
            None => l.miss(format_args!("key sph absent")),
        }
    });
    known.extend(bomb_keys!(plain_keys names));
    known.push("sph");
}
