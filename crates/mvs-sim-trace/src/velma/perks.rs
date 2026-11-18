//! Velma's perks as the record writes it: its classes' `put` and `load`.

use mvs_sim::chars::velma::perks::StudiedExt;
use mvs_trace::Value;

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(StudiedExt { "wait" = wait });

/// Buff_Studied_C's extension as the oracle writes it (`{ wait }`).
pub fn studied_put(e: &StudiedExt, x: &Wx) -> Value {
    e.to_value(x)
}

/// [`studied_put`]'s inverse.
pub fn studied_load(e: &mut StudiedExt, v: &Value, l: &mut Lx) {
    e.load_value(v, l);
}
