//! Marvin's flags as the record writes them.
//!
//! The oracle `floor` is a Platform, the Ground or null; `wall` a placed stage actor by reference, `wallDelegate` its
//! delegate's name. The Ground as a floor is written whole (the World's ground).

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::marvin::flag::C018Flag;
use mvs_sim::world::wall_bp::WallDelegate;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! flag_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "activated" = activated, "activeCharging" = active_charging, "enemies" = enemies,
            "flagVisible" = flag_visible, "floor" = floor, "floorActor" = floor_actor, "hitOffAt" = hit_off_at,
            "isMaxSize" = is_max_size, "kbBound" = kb_bound, "kbSeq" = kb_seq, "looping" = looping,
            "overlapOn" = overlap_on, "over" = over, "pulseAt" = pulse_at, "queued" = queued,
            "sceneScale" = scene_scale,
        )
    };
}

/// a flag of Marvin as the record writes it.
pub fn flag_put(s: &C018Flag, x: &Wx, o: &mut Vec<(String, Value)>) {
    flag_keys!(plain_keys put s, x, o;);
    if s.has_floor
        && s.floor.is_none()
        && let Some(e) = o.iter_mut().find(|e| e.0 == "floor")
    {
        // The Ground as the floor: written whole, as the oracle writes the object.
        e.1 = x.w.stage.ground.to_value(x);
    }
    o.push(("wall".into(), s.wall.map_or(Value::Null, |a| a.to_value(x))));
    let d = s.wall_delegate.map(|d| match d {
        WallDelegate::ObjectDestroyed => "ObjectDestroyed",
        WallDelegate::StructureDestroyed => "StructureDestroyed",
    });
    o.push(("wallDelegate".into(), d.map_or(Value::Null, |d| Value::Str(d.to_owned()))));
}

/// [`flag_put`]'s inverse; the keys it knows go into `known`.
pub fn flag_load(s: &mut C018Flag, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    // The Ground as the floor is an object of its four edges: the flag rides World.ground (`floor` None, attached).
    let on_ground =
        matches!(ob.get("floor"), Some(Value::Obj(f)) if f.get("actor").is_none() && f.get("ref").is_none());
    if on_ground {
        let entries: Vec<(String, Value)> =
            ob.entries().iter().map(|(k, v)| (k.clone(), if k == "floor" { Value::Null } else { v.clone() })).collect();
        let copy = Obj::new(entries);
        flag_keys!(plain_keys load s, copy, l;);
        s.has_floor = true;
    } else {
        flag_keys!(plain_keys load s, ob, l;);
        s.has_floor = s.floor.is_some();
    }
    known.extend(flag_keys!(plain_keys names));
    known.extend(["wall", "wallDelegate"]);
    s.wall = match ob.get("wall") {
        None | Some(Value::Null) => None,
        Some(v) => l.at("wall", |l| ActorRef::from_value(v, l)),
    };
    s.wall_delegate = match ob.get("wallDelegate") {
        Some(Value::Str(d)) if d == "ObjectDestroyed" => Some(WallDelegate::ObjectDestroyed),
        Some(Value::Str(d)) if d == "StructureDestroyed" => Some(WallDelegate::StructureDestroyed),
        None | Some(Value::Null) => None,
        Some(v) => {
            l.miss(format_args!("wallDelegate {v:?}"));
            None
        }
    };
}

// `Plain` is named for the macros' expansions.
const _: fn() = || {
    fn plain<T: Plain>() {}
    plain::<mvs_sim::Real>();
};
