//! `FlightActor` (SupermanFlightHit_C, SupermanGrabListener_C).
//!
//! The actor is where its owner is (the oracle `x` / `y` are getters of the owner's place) and has no velocity (`vx` /
//! `vy` are readonly 0): the record's place columns are written from the owner and not loaded.

use mvs_sim::chars::superman::flight::FlightActor;
use mvs_sim::fighter::FighterId;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::static_object;
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

/// The class's own keys: `attackId`, `bound`, `hitPlayer`, `held`, `ended`, `conds` and `p` (the class's parameters,
/// static data written by its kind alone).
pub fn put(s: &FlightActor, owner: FighterId, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    // The actor is where its owner is.
    r.x = Some(f64::from(x.w.fighter(owner).body.x));
    r.y = Some(f64::from(mvs_sim::fighter::actor_y(x.w, owner)));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    plain_keys!(put s, x, o;
        "attackId" = attack_id, "bound" = bound, "hitPlayer" = hit_player, "held" = held, "ended" = ended, "conds" = conds,
    );
    o.push(("p".into(), static_object()));
}

/// [`put`]'s inverse (the place columns are the owner's: nothing to load).
pub fn load(s: &mut FlightActor, _r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    l.at("rest", |l| {
        plain_keys!(load s, ob, l;
            "attackId" = attack_id, "bound" = bound, "hitPlayer" = hit_player, "held" = held, "ended" = ended, "conds" = conds,
        );
    });
    known.extend(plain_keys!(names
        "attackId" = attack_id, "bound" = bound, "hitPlayer" = hit_player, "held" = held, "ended" = ended, "conds" = conds,
    ));
}
