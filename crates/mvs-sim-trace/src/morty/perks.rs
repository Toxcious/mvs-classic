//! Morty's perks as the record writes them: Buff_C019_Cook's extension `{ own }`.

use mvs_sim::chars::morty::perks::Cook;
use mvs_trace::Value;

use crate::conv::{Plain, obj};
use crate::cx::{Lx, Wx};

/// Buff_C019_Cook's extension as the oracle writes it.
pub fn cook_put(e: &Cook, x: &Wx) -> Value {
    obj(vec![("own", e.own.to_value(x))])
}

/// [`cook_put`]'s inverse.
pub fn cook_load(e: &mut Cook, v: &Value, l: &mut Lx) {
    let Some(o) = crate::conv::as_obj(v, l) else { return };
    match o.get("own") {
        Some(v) => l.at("own", |l| e.own.load_value(v, l)),
        None => l.miss("key own absent"),
    }
    crate::conv::check_keys(o, &["own"], l);
}
