//! Finn's side special's aimer as the record writes it.

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::finn::aimer::FinnAimer;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys};
use crate::cx::{Lx, Wx};

macro_rules! aimer_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "dx" = dx, "dy" = dy, "aimR" = aim_r, "thrownR" = thrown_r, "target" = target, "lock" = lock,
            "widget" = widget, "closestDistance" = closest_distance, "inFront" = in_front, "foundCoin" = found_coin,
            "teleportX" = teleport_x, "teleportY" = teleport_y, "lastHit" = last_hit, "lastHitTime" = last_hit_time,
            "bound" = bound, "overlapping" = overlapping, "wait01" = wait01, "wait075" = wait075,
        )
    };
}

/// A Finn_Dash_AimerExperiment_C as the record writes it: its place and own keys.
pub fn aimer_put(s: &FinnAimer, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    aimer_keys!(plain_keys put s, x, o;);
    let gone = s
        .teleport_actor
        .and_then(|g| x.w.actors.gone_items.iter().find(|(k, _)| *k == g))
        .filter(|(g, _)| x.w.actors.items.get(*g).is_none());
    let t = match gone {
        Some((_, i)) => crate::actors::item_whole(i, x),
        None => s.teleport_actor.map(ActorRef::Item).to_value(x),
    };
    o.push(("teleportActor".into(), t));
    let targeted: Vec<ActorRef> = s.targeted.iter().map(|c| ActorRef::Projectile(*c)).collect();
    o.push(("targeted".into(), targeted.to_value(x)));
    o.push(("p".into(), static_object()));
}

/// [`aimer_put`]'s inverse; the keys it knows go into `known`.
pub fn aimer_load(s: &mut FinnAimer, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        aimer_keys!(plain_keys load s, ob, l;);
        match ob.get("teleportActor") {
            Some(v) => l.at("teleportActor", |l| {
                let whole = match v {
                    Value::Obj(o) if o.get("ref").is_none() => crate::actors::item_whole_load(o, l),
                    _ => None,
                };
                let mut a: Option<ActorRef> = whole.map(ActorRef::Item);
                if whole.is_none() {
                    a.load_value(v, l);
                }
                s.teleport_actor = a.and_then(|a| if let ActorRef::Item(i) = a { Some(i) } else { None });
            }),
            None => l.miss("key teleportActor absent"),
        }
        match ob.get("targeted") {
            Some(v) => l.at("targeted", |l| {
                let mut a: Vec<ActorRef> = Vec::new();
                a.load_value(v, l);
                s.targeted = a
                    .into_iter()
                    .filter_map(|a| if let ActorRef::Projectile(c) = a { Some(c) } else { None })
                    .collect();
            }),
            None => l.miss("key targeted absent"),
        }
    });
    known.extend(aimer_keys!(plain_keys names));
    known.extend(["teleportActor", "targeted"]);
}
