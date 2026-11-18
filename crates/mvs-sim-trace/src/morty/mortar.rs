//! The mortar aim as the record writes it (the World's `mortars`).

use mvs_sim::actors::lists::MortarAim;
use mvs_trace::{ActorRecord, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! mortar_keys {
    ($mode:ident $($a:tt)*) => {
        plain_keys!($mode $($a)*
            "active" = active, "age" = age, "attackId" = attack_id, "attackType" = attack_type, "blastAt" = blast_at,
            "blastBounds" = blast_bounds, "boomIn" = boom_in, "boomX" = boom_x, "boomY" = boom_y, "dz" = dz,
            "facing" = facing, "facingLeft" = facing_left, "hit" = hit, "hurtR" = hurt_r, "hurtSeg" = hurt_seg,
            "hurtUp" = hurt_up, "offset" = offset, "overlappedBy" = overlapped_by, "charge" ~ charge, "elem" ~ elem,
            "lifetimePause" ~ lifetime_pause, "dodge" ~ dodge,
        )
    };
}

/// A mortar aim's record (family `mortars`).
pub fn mortar_put(a: &MortarAim, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "mortars".to_owned(),
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
    mortar_keys!(put a, x, o;);
    // (the class's data: static)
    o.push(("p".into(), static_object()));
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// A mortar aim gone from its list as the oracle writes the object whole (a projectile's `hitActors` still holds it):
/// the record's keys and its `rest` in one object, its owner by reference.
// The load: [`mortar_whole_load`].
pub fn mortar_whole(a: &MortarAim, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = vec![
        ("cls".into(), Value::Str(x.str(a.cls).to_owned())),
        ("dead".into(), Value::Bool(a.dead)),
        ("id".into(), Value::Num(f64::from(a.seq))),
        ("owner".into(), a.owner.to_value(x)),
        ("p".into(), static_object()),
        ("vx".into(), Value::Num(0.0)),
        ("vy".into(), Value::Num(0.0)),
        ("x".into(), Value::Num(f64::from(a.x))),
        ("y".into(), Value::Num(f64::from(a.y))),
    ];
    mortar_keys!(put a, x, o;);
    Value::Obj(mvs_trace::Obj::new(o))
}

/// [`mortar_whole`]'s inverse: the gone mortar aim a projectile's hit list names. It is made once per frame (by its
/// id), kept in `Lx::gone_mortars` for the World's `gone_mortars`, and named by an id of no live slot of the arena.
pub fn mortar_whole_load(o: &mvs_trace::Obj, l: &mut Lx) -> Option<mvs_sim::actors::MortarId> {
    let Some(Value::Num(id)) = o.get("id") else {
        l.miss("a gone mortar aim with no id");
        return None;
    };
    let seq = crate::conv::int(*id, l)?;
    if let Some((_, k, _)) = l.gone_mortars.iter().find(|(s, _, _)| *s == seq) {
        return Some(*k);
    }
    let owner = match o.get("owner") {
        Some(Value::Obj(r)) => match r.get("id") {
            Some(Value::Num(n)) => *n,
            _ => -1.0,
        },
        _ => -1.0,
    };
    let num = |k: &str| match o.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let Some(Value::Str(class)) = o.get("cls") else {
        l.miss("a gone mortar aim with no class");
        return None;
    };
    const OWN: [&str; 8] = ["cls", "dead", "id", "owner", "vx", "vy", "x", "y"];
    let rest = o.entries().iter().filter(|(k, _)| !OWN.contains(&k.as_str())).cloned().collect();
    let r = ActorRecord {
        family: "mortars".to_owned(),
        id: *id,
        class: class.clone(),
        owner,
        x: num("x"),
        y: num("y"),
        vx: num("vx"),
        vy: num("vy"),
        dead: matches!(o.get("dead"), Some(Value::Bool(true))),
        rest: Value::Obj(mvs_trace::Obj::new(rest)),
        ..ActorRecord::default()
    };
    let a = mortar_make(&r, l)?;
    // (an id of no slot: past the arena's, never handed out)
    let k = mvs_sim::actors::MortarId::new(u32::MAX - u32::try_from(l.gone_mortars.len()).ok()?, 0);
    l.gone_mortars.push((seq, k, a));
    Some(k)
}

/// [`mortar_put`]'s inverse.
pub fn mortar_load(a: &mut MortarAim, r: &ActorRecord, l: &mut Lx) {
    a.dead = r.dead;
    opt_load(&mut a.x, r.x, "x", l);
    opt_load(&mut a.y, r.y, "y", l);
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (a mortar aim has one)"),
    });
    let Some(ob) = l.at("rest", |l| crate::conv::as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        mortar_keys!(load a, ob, l;);
        let mut known = mortar_keys!(names).to_vec();
        known.push("p");
        crate::conv::check_keys(&ob, &known, l);
    });
}

/// A new mortar aim from its record (`None`: not made).
pub fn mortar_make(r: &ActorRecord, l: &mut Lx) -> Option<MortarAim> {
    let owner = l.fighter(r.owner)?;
    let mut a = MortarAim {
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
        blast_at: 0.0,
        blast_bounds: mvs_sim::world::blast::new_blast_bounds(),
        hit: Default::default(),
        charge: None,
        facing_left: false,
        offset: 0.0,
        active: false,
        boom_x: 0.0,
        boom_y: 0.0,
        boom_in: -1,
        dz: 0.0,
        hurt_r: 0.0,
        hurt_seg: 0.0,
        hurt_up: 0.0,
        attack_type: 0,
        overlapped_by: Default::default(),
        owner,
        attack_id: 0,
    };
    mortar_load(&mut a, r, l);
    Some(a)
}
