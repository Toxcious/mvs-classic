//! Tom and Jerry's mouse trap as the record writes it. The actor stands at its place
//! (the record's x and y, vx and vy 0); `p` is the class's static data.

use mvs_sim::chars::tom_and_jerry::trap::MouseTrap;
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};

macro_rules! trap_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "audioOnce" = audio_once, "aura" = aura, "bait" = bait,
            "blastAt" = blast_at, "blastBounds" = blast_bounds, "cheese" = cheese, "destroyAt" = destroy_at,
            "detonateAt" = detonate_at, "ended" = ended, "facing" = facing, "floor" = floor, "gateAt" = gate_at,
            "hasHit" = has_hit, "hit" = hit, "hitOffAt" = hit_off_at, "hitOn" = hit_on, "hitOnce" = hit_once,
            "hitR" = hit_r, "hitSet" = hit_set, "hurtR" = hurt_r, "inside" = inside,
            "invulnerable" = invulnerable, "lifeAt" = life_at, "sprung" = sprung, "triggerGate" = trigger_gate,
            "triggerR" = trigger_r,
        )
    };
}

/// A MouseTrap_SpawnedActor_C as the record writes it.
pub fn trap_put(s: &MouseTrap, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    trap_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`trap_put`]'s inverse; the keys it knows go into `known`.
pub fn trap_load(s: &mut MouseTrap, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        trap_keys!(plain_keys load s, ob, l;);
    });
    known.extend(trap_keys!(plain_keys names));
}
