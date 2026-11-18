//! The aim punch as the record writes it (the World's `aimPunches`).

use mvs_sim::actors::lists::AimPunch;
use mvs_trace::{ActorRecord, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// An aim punch's record (family `aimPunches`).
pub fn aim_punch_put(a: &AimPunch, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "aimPunches".to_owned(),
        id: f64::from(a.seq),
        class: x.str(a.cls).to_owned(),
        owner: f64::from(x.fid(a.owner)),
        x: Some(f64::from(a.x)),
        y: Some(f64::from(a.y)),
        dead: a.dead,
        ..ActorRecord::default()
    };
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put a, x, o;
        "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "checkIn" = check_in, "prevX" = prev_x,
        "prevY" = prev_y, "serverX" = server_x, "serverY" = server_y,
    );
    o.push(("p".into(), static_object()));
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// [`aim_punch_put`]'s inverse.
pub fn aim_punch_load(a: &mut AimPunch, r: &ActorRecord, l: &mut Lx) {
    a.dead = r.dead;
    opt_load(&mut a.x, r.x, "x", l);
    opt_load(&mut a.y, r.y, "y", l);
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (an aim punch has one)"),
    });
    let Some(ob) = l.at("rest", |l| crate::conv::as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        plain_keys!(load a, ob, l;
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "checkIn" = check_in, "prevX" = prev_x,
            "prevY" = prev_y, "serverX" = server_x, "serverY" = server_y,
        );
        let mut known = plain_keys!(names
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "checkIn" = check_in, "prevX" = prev_x,
            "prevY" = prev_y, "serverX" = server_x, "serverY" = server_y,
        )
        .to_vec();
        known.push("p");
        crate::conv::check_keys(&ob, &known, l);
    });
}

/// A new aim punch from its record (`None`: not made).
pub fn aim_punch_make(r: &ActorRecord, l: &mut Lx) -> Option<AimPunch> {
    let owner = l.fighter(r.owner)?;
    let mut a = AimPunch {
        cls: l.name(&r.class),
        seq: crate::conv::int(r.id, l)?,
        x: 0.0,
        y: 0.0,
        prev_x: 0.0,
        prev_y: 0.0,
        server_x: 0.0,
        server_y: 0.0,
        age: 0,
        dead: false,
        blast_at: 0.0,
        blast_bounds: mvs_sim::world::blast::new_blast_bounds(),
        check_in: 0,
        owner,
    };
    aim_punch_load(&mut a, r, l);
    Some(a)
}
