//! The ship as the record writes it (the World's `ships`).

use mvs_sim::actors::lists::{Ship, ShipPiece, ShipSensor};
use mvs_trace::{ActorRecord, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(ShipSensor { "dx" = dx, "dy" = dy, "hw" = hw, "hh" = hh });
plain_struct!(ShipPiece {
    "name" = name, "dx" = dx, "dy" = dy, "hw" = hw, "hh" = hh, "sensor" = sensor, "count" = count, "on" = on,
    "trace" = trace, "nav" = nav, "item" = item,
});

macro_rules! ship_keys {
    ($mode:ident $($a:tt)*) => {
        plain_keys!($mode $($a)*
            "age" = age, "appearTick" = appear_tick, "deathSoundTick" = death_sound_tick, "destroyTick" = destroy_tick,
            "dx" = dx, "dy" = dy, "meshDy" = mesh_dy, "pieces" = pieces, "stopTick" = stop_tick, "team" = team,
            "edgeAt" ~ edge_at,
        )
    };
}

/// A ship's record (family `ships`).
pub fn ship_put(a: &Ship, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "ships".to_owned(),
        id: f64::from(a.seq),
        class: x.str(a.cls).to_owned(),
        owner: f64::from(x.fid(a.owner)),
        x: Some(f64::from(a.x)),
        y: Some(f64::from(a.y)),
        vx: Some(f64::from(a.vx)),
        vy: Some(f64::from(a.vy)),
        dead: a.dead,
        ..ActorRecord::default()
    };
    let mut o: Vec<(String, Value)> = Vec::new();
    ship_keys!(put a, x, o;);
    // (the class's data: static)
    o.push(("p".into(), static_object()));
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// [`ship_put`]'s inverse.
pub fn ship_load(a: &mut Ship, r: &ActorRecord, l: &mut Lx) {
    a.dead = r.dead;
    opt_load(&mut a.x, r.x, "x", l);
    opt_load(&mut a.y, r.y, "y", l);
    opt_load(&mut a.vx, r.vx, "vx", l);
    opt_load(&mut a.vy, r.vy, "vy", l);
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (a ship has one)"),
    });
    let Some(ob) = l.at("rest", |l| crate::conv::as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        ship_keys!(load a, ob, l;);
        let mut known = ship_keys!(names).to_vec();
        known.push("p");
        crate::conv::check_keys(&ob, &known, l);
    });
}

/// A new ship from its record (`None`: not made).
pub fn ship_make(r: &ActorRecord, l: &mut Lx) -> Option<Ship> {
    let owner = l.fighter(r.owner)?;
    let mut a = Ship {
        cls: l.name(&r.class),
        seq: crate::conv::int(r.id, l)?,
        x: 0.0,
        y: 0.0,
        vx: 0.0,
        vy: 0.0,
        dx: 0.0,
        dy: 0.0,
        age: 0,
        dead: false,
        pieces: Vec::new(),
        mesh_dy: 0.0,
        team: 0,
        appear_tick: 0,
        death_sound_tick: 0,
        stop_tick: 0,
        destroy_tick: -1,
        edge_at: None,
        owner,
    };
    ship_load(&mut a, r, l);
    Some(a)
}
