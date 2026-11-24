//! The port's own Dodgeball rule in the record: the set-up's `dodgeball` (`{barrier}`) and the World's `rest` key
//! `dodgeball` (`mvs_sim::world::dodgeball::DodgeballState`), present only in a World whose set-up has the rule. Its
//! barrier is a run-time solid, written in `rest.runtimeSolids` (`tutorial::rest_put`). The oracle has no Dodgeball:
//! these keys are the Rust writer's alone, and a Dodgeball trace's baseline is the Rust side's own record.

use mvs_sim::setup::Dodgeball;
use mvs_sim::world::World;
use mvs_sim::world::dodgeball::DodgeballState;
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(DodgeballState { "dropAt" = drop_at, "from" = from, "over" = over, "rng" = rng });

/// A set-up's `dodgeball` as a header holds it.
pub fn setup_of(v: &serde_json::Value) -> Result<Dodgeball, String> {
    let m: mvs_trace::DodgeballSpec = serde_json::from_value(v.clone()).map_err(|e| format!("dodgeball: {e}"))?;
    Ok(Dodgeball { barrier: m.barrier })
}

/// [`setup_of`]'s inverse.
pub fn setup_put(d: &Dodgeball) -> serde_json::Value {
    serde_json::to_value(mvs_trace::DodgeballSpec { barrier: d.barrier }).expect("a dodgeball writes")
}

/// The World's rest key of the rule (none without it).
pub fn rest_put(w: &World, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    if let Some(st) = &w.dodgeball {
        o.push(("dodgeball", st.to_value(x)));
    }
}

/// [`rest_put`]'s inverse: the rule's state where the World has the rule.
pub fn rest_load(w: &mut World, ob: &Obj, l: &mut Lx) {
    let v = ob.get("dodgeball");
    if v.is_some() != w.dodgeball.is_some() {
        l.miss(format_args!("the dodgeball in the record {}, in the World {}", v.is_some(), w.dodgeball.is_some()));
    }
    if let (Some(v), Some(st)) = (v, w.dodgeball.as_mut()) {
        l.at("dodgeball", |l| st.load_value(v, l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dodgeball_reads_back() {
        for d in [Dodgeball::PLAY, Dodgeball { barrier: false }] {
            assert_eq!(setup_of(&setup_put(&d)), Ok(d));
        }
    }
}
