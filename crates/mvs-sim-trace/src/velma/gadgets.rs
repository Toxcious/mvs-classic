//! Velma's gadgets as the record writes them: its classes' `put` and `load`.
//!
//! The grenade's `floor` is the attach parent's platform in the oracle (an object); no match has one and the sim's
//! floor is a reference of its own, so a floor that is set is not written and a floor in a record is a miss.

use mvs_sim::chars::velma::gadgets::{SlowExt, SlowToStunGrenade, VelmaBook, VelmaBurst, VelmaSassBubble, VelmaVial};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(SlowExt { "trap" = trap, "from" = from, "start" = start, "n" = n, "rate" = rate });

/// A VelmaVialAttack_C as the record writes it: the place and the body's velocity typed, the spin in `rest`.
pub fn vial_put(s: &VelmaVial, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    let _ = x;
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    plain_keys!(put s, x, o; "spin" = spin,);
    o.push(("p".into(), static_object()));
}

/// [`vial_put`]'s inverse; the keys it knows go into `known`.
pub fn vial_load(s: &mut VelmaVial, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "spin" = spin,);
    });
    known.extend(plain_keys!(names "spin" = spin,));
    known.push("p");
}

/// A SlowtoStunGrenade_C as the record writes it: the place typed (no velocity), the rest in `rest`.
pub fn grenade_put(s: &SlowToStunGrenade, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    plain_keys!(put s, x, o; "age" = age, "inside" = inside, "lifeAt" = life_at, "blockedDestroy" ~ blocked_destroy,);
    if s.floor.is_some() {
        x.unmapped("a SlowtoStunGrenade_C riding a floor");
    }
    o.push(("floor".into(), Value::Null));
    o.push(("p".into(), static_object()));
}

/// [`grenade_put`]'s inverse; the keys it knows go into `known`.
pub fn grenade_load(s: &mut SlowToStunGrenade, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "age" = age, "inside" = inside, "lifeAt" = life_at, "blockedDestroy" ~ blocked_destroy,);
        match ob.get("floor") {
            None | Some(Value::Null) => s.floor = None,
            Some(_) => l.miss("a grenade's floor object (no match has one)"),
        }
    });
    known.extend(
        plain_keys!(names "age" = age, "inside" = inside, "lifeAt" = life_at, "blockedDestroy" ~ blocked_destroy,),
    );
    known.extend(["floor", "p"]);
}

/// Buff_SlowIntoStun_C's extension as the oracle writes it.
pub fn slow_put(e: &SlowExt, x: &Wx) -> Value {
    use crate::conv::Plain;
    e.to_value(x)
}

/// [`slow_put`]'s inverse.
pub fn slow_load(e: &mut SlowExt, v: &Value, l: &mut Lx) {
    use crate::conv::Plain;
    e.load_value(v, l);
}

macro_rules! book_keys {
    ($mac:ident $($pre:tt)*) => { $mac!($($pre)* "homeMag" = home_mag, "homeTo" = home_to,) };
}

/// A VelmaBookProjectile_C's own fields as the record writes them.
pub fn book_put(s: &VelmaBook, x: &Wx, o: &mut Vec<(String, Value)>) {
    book_keys!(plain_keys put s, x, o;);
}

/// [`book_put`]'s inverse; the keys it knows go into `known`.
pub fn book_load(s: &mut VelmaBook, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        book_keys!(plain_keys load s, ob, l;);
    });
    known.extend(book_keys!(plain_keys names));
}

/// A Velma_SPAIR_CameraFlash_C / Velma_AirSideSp_BookBlast_C's own fields as the record writes them.
pub fn burst_put(s: &VelmaBurst, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "onAt" = on_at,);
}

/// [`burst_put`]'s inverse; the keys it knows go into `known`.
pub fn burst_load(s: &mut VelmaBurst, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l; "onAt" = on_at,);
    });
    known.extend(plain_keys!(names "onAt" = on_at,));
}

macro_rules! bubble_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "boxFwd" = box_fwd, "boxGone" = box_gone, "boxX" = box_x, "boxY" = box_y, "boxZ" = box_z,
            "growthLen" = growth_len, "offAt" = off_at, "psScale" = ps_scale,)
    };
}

/// A Velma_SassBubblev2_C's own fields as the record writes them.
pub fn bubble_put(s: &VelmaSassBubble, x: &Wx, o: &mut Vec<(String, Value)>) {
    bubble_keys!(plain_keys put s, x, o;);
}

/// [`bubble_put`]'s inverse; the keys it knows go into `known`.
pub fn bubble_load(s: &mut VelmaSassBubble, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        bubble_keys!(plain_keys load s, ob, l;);
    });
    known.extend(bubble_keys!(plain_keys names));
}
