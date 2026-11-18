//! Harley Quinn's bomb as the record writes them: its own keys and place. The record's
//! x, y, vx and vy are the bomb's own fields.

use mvs_sim::chars::harley::bomb::Bomb;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! bomb_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "ally" = ally, "armed" = armed, "attachR" = attach_r, "attackId" = attack_id,
            "blastAt" = blast_at, "blastBounds" = blast_bounds, "carrier" = carrier, "carrierFacing" = carrier_facing,
            "exitR" = exit_r, "facing" = facing, "flipOnAttach" = flip_on_attach, "front" = front,
            "gravAt" = grav_at, "hit" = hit, "inArea" = in_area, "inside" = inside, "leftArena" = left_arena,
            "pollAt" = poll_at, "pollFor" = poll_for, "pvx" = pvx, "pvy" = pvy, "radius" = radius, "relY" = rel_y,
            "spin" = spin, "state" = state, "noCollision" ~ no_collision,
        )
    };
}

/// The bomb's own keys and place.
pub fn bomb_put(s: &Bomb, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(f64::from(s.vx));
    r.vy = Some(f64::from(s.vy));
    bomb_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`bomb_put`]'s inverse.
pub fn bomb_load(s: &mut Bomb, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    opt_load(&mut s.vx, r.vx, "vx", l);
    opt_load(&mut s.vy, r.vy, "vy", l);
    l.at("rest", |l| {
        bomb_keys!(plain_keys load s, ob, l;);
    });
    known.extend(bomb_keys!(plain_keys names));
}
