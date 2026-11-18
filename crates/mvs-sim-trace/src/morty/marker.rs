//! The marker as the record writes it (the World's `markers`).

use mvs_sim::actors::lists::{Marker, Pt2, ReparamEntry, SplinePt};
use mvs_trace::{ActorRecord, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(Pt2 { "x" = x, "y" = y });
plain_struct!(SplinePt { "x" = x, "y" = y, "tx" = tx, "ty" = ty, "auto" ~ auto });
plain_struct!(ReparamEntry { "d" = d, "key" = key });

macro_rules! marker_keys {
    ($mode:ident $($a:tt)*) => {
        plain_keys!($mode $($a)*
            "active" = active, "age" = age, "attackId" = attack_id, "autoTick" = auto_tick, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "canAdd" = can_add, "capRadius" = cap_radius, "currentDamage" = current_damage,
            "dropX" = drop_x, "dropY" = drop_y, "facing" = facing, "focusX" = focus_x, "focusY" = focus_y, "hit" = hit,
            "len" = len, "lifeTicks" = life_ticks, "meshes" = meshes, "moveEnd" = move_end, "moveStart" = move_start,
            "moving" = moving, "pts" = pts, "saved" = saved, "splineAt" = spline_at, "table" = table, "tlPos" = tl_pos,
            "tlRate" = tl_rate, "trailEnd" = trail_end, "charge" ~ charge, "elem" ~ elem,
            "lifetimePause" ~ lifetime_pause, "dodge" ~ dodge,
        )
    };
}

/// A marker's record (family `markers`).
pub fn marker_put(a: &Marker, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "markers".to_owned(),
        id: f64::from(a.seq),
        class: x.str(a.cls).to_owned(),
        owner: f64::from(x.fid(a.owner)),
        x: Some(f64::from(a.x)),
        y: Some(f64::from(a.y)),
        // (readonly 0)
        vx: Some(0.0),
        vy: Some(0.0),
        dead: a.dead,
        ..ActorRecord::default()
    };
    let mut o: Vec<(String, Value)> = Vec::new();
    marker_keys!(put a, x, o;);
    // (the class's data: static)
    o.push(("p".into(), static_object()));
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// [`marker_put`]'s inverse.
pub fn marker_load(a: &mut Marker, r: &ActorRecord, l: &mut Lx) {
    a.dead = r.dead;
    opt_load(&mut a.x, r.x, "x", l);
    opt_load(&mut a.y, r.y, "y", l);
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (a marker has one)"),
    });
    let Some(ob) = l.at("rest", |l| crate::conv::as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        marker_keys!(load a, ob, l;);
        let mut known = marker_keys!(names).to_vec();
        known.push("p");
        crate::conv::check_keys(&ob, &known, l);
    });
}

/// A new marker from its record (`None`: not made).
pub fn marker_make(r: &ActorRecord, l: &mut Lx) -> Option<Marker> {
    let owner = l.fighter(r.owner)?;
    let mut a = Marker {
        lifetime_pause: None,
        dodge: None,
        cls: l.name(&r.class),
        elem: None,
        seq: crate::conv::int(r.id, l)?,
        x: 0.0,
        y: 0.0,
        facing: 1.0,
        age: 0,
        dead: false,
        blast_bounds: mvs_sim::world::blast::new_blast_bounds(),
        blast_at: 0.0,
        hit: Default::default(),
        charge: None,
        pts: Vec::new(),
        current_damage: 0.0,
        cap_radius: 0.0,
        active: false,
        moving: false,
        focus_x: 0.0,
        focus_y: 0.0,
        move_start: Pt2::default(),
        move_end: Pt2::default(),
        can_add: false,
        drop_x: 0.0,
        drop_y: 0.0,
        spline_at: 0.0,
        auto_tick: 0,
        life_ticks: 0,
        tl_pos: 0.0,
        tl_rate: 0.0,
        len: 0.0,
        table: Vec::new(),
        saved: Vec::new(),
        meshes: 0,
        trail_end: Pt2::default(),
        owner,
        attack_id: 0,
    };
    marker_load(&mut a, r, l);
    Some(a)
}
