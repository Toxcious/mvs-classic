//! Finn's gem as the record writes it.

use mvs_sim::chars::finn::gem::Gem;
use mvs_trace::{Obj, Value};

use crate::conv::plain_keys;
use crate::cx::{Lx, Wx};
use crate::parts::{bounce_load, bounce_put};

macro_rules! gem_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "isTargeted" = is_targeted, "weakThrow" = weak_throw, "fadeAt" = fade_at, "born" = born, "fade" = fade,
            "fading" = fading, "fadeHidden" = fade_hidden, "blinkClosed" = blink_closed, "fadePaused" = fade_paused,
            "boost" = boost, "boostSet" = boost_set, "boostNext" = boost_next, "instigator" = instigator,
            "platOn" = plat_on, "noPick" = no_pick, "warpAt" = warp_at, "bounceSounds" = bounce_sounds,
            "bounceKill" = bounce_kill,
        )
    };
}

/// A FinnDashThrownTarget_C as the record writes it: its own keys and those of the bouncing item it extends.
pub fn gem_put(s: &Gem, x: &Wx, o: &mut Vec<(String, Value)>) {
    bounce_put(&s.bounce, x, o);
    gem_keys!(plain_keys put s, x, o;);
}

/// [`gem_put`]'s inverse; the keys it knows go into `known`.
pub fn gem_load(s: &mut Gem, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    bounce_load(&mut s.bounce, ob, l, known);
    gem_keys!(plain_keys load s, ob, l;);
    known.extend(gem_keys!(plain_keys names));
}
