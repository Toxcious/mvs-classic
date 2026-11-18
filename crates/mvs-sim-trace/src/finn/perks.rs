//! Finn's perks' actor and buff extensions as the record writes them.

use mvs_sim::chars::finn::perks::{GemBoostBlast, GemVars, SaleVars};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

macro_rules! blast_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds,
            "facing" = facing, "hit" = hit, "hitOn" = hit_on, "hitSet" = hit_set, "offAt" = off_at, "r" = r,
        )
    };
}

/// A FinnGemBoostBlast_C as the record writes it (vx and vy readonly 0).
pub fn blast_put(s: &GemBoostBlast, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    blast_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`blast_put`]'s inverse; the keys it knows go into `known`.
pub fn blast_load(s: &mut GemBoostBlast, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        blast_keys!(plain_keys load s, ob, l;);
    });
    known.extend(blast_keys!(plain_keys names));
}

plain_struct!(GemVars { "lastHitAttackId" = last_hit_attack_id, "nextGemFree" = next_gem_free });

/// Buff_Finn_GemOnCharge_C's extension as the oracle writes it (`{ lastHitAttackId, nextGemFree }`).
pub fn gem_on_charge_put(e: &GemVars, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`gem_on_charge_put`]'s inverse.
pub fn gem_on_charge_load(e: &mut GemVars, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

plain_struct!(SaleVars {
    "activated" = activated, "saleActive" = sale_active, "team" = team, "loser" = loser, "koWait" = ko_wait,
    "saleWait" = sale_wait,
});

/// Buff_Finn_GoingOutOfBusiness_C's extension as the oracle writes it.
pub fn sale_put(e: &SaleVars, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`sale_put`]'s inverse.
pub fn sale_load(e: &mut SaleVars, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
