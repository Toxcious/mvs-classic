//! Finn's shop and its buffs' extensions as the record writes them.

use mvs_sim::chars::finn::shop::{BootsExt, BunsExt, FinnShop};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::{Plain, plain_keys, plain_struct};
use crate::cx::{Lx, Wx};

macro_rules! shop_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "affordable" = affordable, "age" = age, "attachIn" = attach_in, "attached" = attached,
            "blastAt" = blast_at, "blastBounds" = blast_bounds, "bootsStacks" = boots_stacks, "dx" = dx, "dy" = dy,
            "gooseLeft" = goose_left, "leftTimer" = left_timer, "onFireSale" = on_fire_sale, "prices" = prices,
            "rightTimer" = right_timer, "selection" = selection, "visible" = visible,
        )
    };
}

/// A Finn_Shop_C as the record writes it (its place is the record's x and y).
pub fn shop_put(s: &FinnShop, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    shop_keys!(plain_keys put s, x, o;);
    o.push(("p".into(), static_object()));
}

/// [`shop_put`]'s inverse; the keys it knows go into `known`.
pub fn shop_load(s: &mut FinnShop, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        shop_keys!(plain_keys load s, ob, l;);
    });
    known.extend(shop_keys!(plain_keys names));
}

plain_struct!(BootsExt { "delay" = delay, "life" = life });

/// Finn_Shop_Boots_Buff_C's extension as the oracle writes it (`{ delay, life }`).
pub fn boots_put(e: &BootsExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`boots_put`]'s inverse.
pub fn boots_load(e: &mut BootsExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}

plain_struct!(BunsExt {
    "allies" = allies, "applied" = applied, "bound" = bound, "charging" = charging, "counts" = counts,
    "delay" = delay,
});

/// Finn_Shop_ArmorBuns_Buff_C's extension as the oracle writes it. Unverified: `bound` holds the Buff_ProjectileBlock
/// entries; the oracle writes each whole (a BuffEntry has no reference here), which no rule writes: a bound buff is
/// named unmapped.
pub fn buns_put(e: &BunsExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`buns_put`]'s inverse.
pub fn buns_load(e: &mut BunsExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
