//! `Polymorph` (`Fighter.polymorph`): the fighter's `rest.polymorph`. Its `from` (the data
//! that holds the mesh's montages) is a static object the record writes by its kind alone: loaded as a mark and
//! resolved with the lists (`resolve_from`).

use mvs_sim::combat::polymorph::polymorph_owner;
use mvs_sim::combat::shapes::Polymorph;
use mvs_sim::names::Name;
use mvs_sim::world::World;
use mvs_trace::Value;

use crate::conv::{Plain, as_obj, check_keys, obj};
use crate::cx::{Lx, Wx};

const KEYS: &[&str] = &[
    "cls",
    "from",
    "mesh",
    "anim",
    "scale",
    "head",
    "play",
    "damage",
    "evadeTimer",
    "showIn",
    "active",
    "crouched",
    "hitstunBound",
    "windows",
    "orig",
];

/// A key that must be there.
fn need<T: Plain>(o: &mvs_trace::Obj, k: &str, t: &mut T, l: &mut Lx) {
    match o.get(k) {
        Some(v) => l.at(k, |l| t.load_value(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    }
}

impl Plain for Polymorph {
    fn to_value(&self, x: &Wx) -> Value {
        let from = match self.from {
            Some(_) => obj(vec![("static", Value::Str("object".to_owned()))]),
            None => Value::Null,
        };
        obj(vec![
            ("cls", self.cls.to_value(x)),
            ("from", from),
            ("mesh", self.mesh.to_value(x)),
            ("anim", self.anim.to_value(x)),
            ("scale", self.scale.to_value(x)),
            ("head", self.head.to_value(x)),
            ("play", self.play.to_value(x)),
            ("damage", self.damage.to_value(x)),
            ("evadeTimer", self.evade_timer.to_value(x)),
            ("showIn", self.show_in.to_value(x)),
            ("active", Value::Bool(self.active)),
            ("crouched", Value::Bool(self.crouched)),
            ("hitstunBound", Value::Bool(self.hitstun_bound)),
            ("windows", self.windows.to_value(x)),
            ("orig", obj(vec![("xy", self.orig.0.to_value(x)), ("z", self.orig.1.to_value(x))])),
        ])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(o) = as_obj(v, l) else { return };
        need(o, "cls", &mut self.cls, l);
        match o.get("from") {
            Some(Value::Null) => self.from = None,
            // a mark, resolved with the lists (`resolve_from`)
            Some(Value::Obj(s)) if s.get("static").is_some() => self.from = Some(Name::NONE),
            Some(v) => l.miss(format_args!("from {v:?} is neither null nor a static")),
            None => l.miss("key from absent"),
        }
        need(o, "mesh", &mut self.mesh, l);
        need(o, "anim", &mut self.anim, l);
        need(o, "scale", &mut self.scale, l);
        need(o, "head", &mut self.head, l);
        need(o, "play", &mut self.play, l);
        need(o, "damage", &mut self.damage, l);
        need(o, "evadeTimer", &mut self.evade_timer, l);
        need(o, "showIn", &mut self.show_in, l);
        need(o, "active", &mut self.active, l);
        need(o, "crouched", &mut self.crouched, l);
        need(o, "hitstunBound", &mut self.hitstun_bound, l);
        need(o, "windows", &mut self.windows, l);
        match o.get("orig").map(|v| as_obj(v, l)) {
            Some(Some(p)) => {
                need(p, "xy", &mut self.orig.0, l);
                need(p, "z", &mut self.orig.1, l);
                check_keys(p, &["xy", "z"], l);
            }
            Some(None) => {}
            None => l.miss("key orig absent"),
        }
        check_keys(o, KEYS, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut p = Polymorph {
            cls: Name::NONE,
            from: None,
            mesh: Name::NONE,
            anim: Name::NONE,
            scale: 1.0,
            head: 1.0,
            play: None,
            damage: 0.0,
            evade_timer: -1,
            show_in: -1,
            active: false,
            crouched: false,
            hitstun_bound: false,
            windows: Vec::new(),
            orig: (1.0, 1.0),
        };
        p.load_value(v, l);
        Some(p)
    }
}

/// Each loaded polymorph's `from` mark to the data `polymorph_owner` finds for its class and mesh (the oracle's own
/// rule at the polymorph's begin), once the fighters are in.
pub fn resolve_from(w: &mut World) {
    let list: Vec<_> = w.fighters.iter().map(|(id, _)| id).collect();
    for f in list {
        let Some((cls, mesh)) =
            w.fighter(f).shapes.polymorph.as_ref().filter(|p| p.from.is_some()).map(|p| (p.cls, p.mesh))
        else {
            continue;
        };
        let mesh = w.names.str(mesh).to_owned();
        let from = polymorph_owner(w, cls, &mesh);
        if let Some(p) = w.fighter_mut(f).shapes.polymorph.as_mut() {
            p.from = from;
        }
    }
}
