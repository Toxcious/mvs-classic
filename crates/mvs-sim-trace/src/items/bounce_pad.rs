//! Item_BouncePad_C and the pad it spawns, BouncePad_SpawnedActor_C as the record writes it.

use mvs_sim::items::bounce_pad::{BouncePad, BouncePadItem};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// A BouncePadItem as the record writes it.
pub fn bounce_pad_item_put(s: &BouncePadItem, x: &Wx, o: &mut Vec<(String, Value)>) {
    plain_keys!(put s, x, o; "canDeploy" = can_deploy, "deploying" = deploying, "deployed" = deployed);
}

/// [`bounce_pad_item_put`]'s inverse; the keys it knows go into `known`.
pub fn bounce_pad_item_load(s: &mut BouncePadItem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    plain_keys!(load s, ob, l; "canDeploy" = can_deploy, "deploying" = deploying, "deployed" = deployed);
    known.extend(plain_keys!(names "canDeploy" = can_deploy, "deploying" = deploying, "deployed" = deployed));
}

/// A BouncePad as the record writes it.
pub fn bounce_pad_put(s: &BouncePad, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    plain_keys!(put s, x, o; "age" = age, "bounceAt" = bounce_at, "detectHx" = detect_hx, "detectHy" = detect_hy, "detectY" = detect_y, "hx" = hx, "hy" = hy, "inside" = inside, "lifeAt" = life_at, "linDamp" = lin_damp);
    // (`p`: the class's parameters: the data's, written whole where small, as the oracle writes a static)
    o.push(("p".into(), s.p.as_deref().map_or_else(crate::actors::static_object, crate::parts::static_put)));
}

/// [`bounce_pad_put`]'s inverse; the keys it knows go into `known`.
pub fn bounce_pad_load(s: &mut BouncePad, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    if let Some(v) = r.x {
        s.x = mvs_sim::math::narrow(v);
    }
    if let Some(v) = r.y {
        s.y = mvs_sim::math::narrow(v);
    }
    if let Some(v) = r.vx {
        s.vx = mvs_sim::math::narrow(v);
    }
    if let Some(v) = r.vy {
        s.vy = mvs_sim::math::narrow(v);
    }
    plain_keys!(load s, ob, l; "age" = age, "bounceAt" = bounce_at, "detectHx" = detect_hx, "detectHy" = detect_hy, "detectY" = detect_y, "hx" = hx, "hy" = hy, "inside" = inside, "lifeAt" = life_at, "linDamp" = lin_damp);
    s.p = match ob.get("p") {
        Some(v @ Value::Obj(o)) if o.get("static").is_none() => match crate::parts::data_load(v) {
            mvs_data::value::Value::Obj(o) => Some(std::sync::Arc::new(o)),
            _ => None,
        },
        _ => None,
    };
    known.push("p");
    known.extend(plain_keys!(names "age" = age, "bounceAt" = bounce_at, "detectHx" = detect_hx, "detectHy" = detect_hy, "detectY" = detect_y, "hx" = hx, "hy" = hy, "inside" = inside, "lifeAt" = life_at, "linDamp" = lin_damp));
}
