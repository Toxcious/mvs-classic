//! The portal as the record writes it (the World's `accelerators`).

use mvs_sim::actors::lists::Accelerator;
use mvs_trace::ActorRecord;

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};
use mvs_trace::Value;

/// A portal's record (family `accelerators`).
pub fn accelerator_put(a: &Accelerator, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "accelerators".to_owned(),
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
        "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "endTick" = end_tick, "facing" = facing,
        "inside" = inside, "doom" ~ doom,
    );
    // (the class's data: static)
    o.push(("comps".into(), static_object()));
    o.push(("p".into(), static_object()));
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// [`accelerator_put`]'s inverse.
pub fn accelerator_load(a: &mut Accelerator, r: &ActorRecord, l: &mut Lx) {
    a.dead = r.dead;
    opt_load(&mut a.x, r.x, "x", l);
    opt_load(&mut a.y, r.y, "y", l);
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (a portal has one)"),
    });
    let Some(ob) = l.at("rest", |l| crate::conv::as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        plain_keys!(load a, ob, l;
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "endTick" = end_tick, "facing" = facing,
            "inside" = inside, "doom" ~ doom,
        );
        let mut known = plain_keys!(names
            "age" = age, "blastAt" = blast_at, "blastBounds" = blast_bounds, "endTick" = end_tick, "facing" = facing,
            "inside" = inside, "doom" ~ doom,
        )
        .to_vec();
        known.extend(["comps", "p"]);
        crate::conv::check_keys(&ob, &known, l);
    });
}

/// A new portal from its record (`None`: not made).
pub fn accelerator_make(r: &ActorRecord, l: &mut Lx) -> Option<Accelerator> {
    let owner = l.fighter(r.owner)?;
    let mut a = Accelerator {
        cls: l.name(&r.class),
        seq: crate::conv::int(r.id, l)?,
        x: 0.0,
        y: 0.0,
        facing: 1.0,
        age: 0,
        dead: false,
        blast_at: 0.0,
        blast_bounds: mvs_sim::world::blast::new_blast_bounds(),
        inside: Default::default(),
        doom: None,
        end_tick: 0,
        owner,
    };
    accelerator_load(&mut a, r, l);
    Some(a)
}
