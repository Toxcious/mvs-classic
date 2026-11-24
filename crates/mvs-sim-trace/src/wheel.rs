//! The port's own Chaos Wheel rule in the record: the set-up's `wheel` (`{pool, seconds, warn}`) and the World's
//! `rest` key `wheel` (`mvs_sim::world::wheel::WheelState`), present only in a World whose set-up has a wheel. The
//! oracle has no wheel: these keys are the Rust writer's alone, and a wheel trace's baseline is the Rust side's own
//! record.

use mvs_sim::setup::Wheel;
use mvs_sim::world::World;
use mvs_sim::world::wheel::{WheelSaved, WheelState};
use mvs_trace::{Obj, Value};

use crate::conv::{Plain, plain_struct};
use crate::cx::{Lx, Wx};

plain_struct!(WheelSaved { "air" = air, "gravity" = gravity, "held" = held });

plain_struct!(WheelState {
    "also" = also,
    "alsoNext" = also_next,
    "bag" = bag,
    "changeAt" = change_at,
    "current" = current,
    "from" = from,
    "items" = items,
    "next" = next,
    "over" = over,
    "recent" = recent,
    "rng" = rng,
    "saved" = saved,
});

/// A set-up's `wheel` as a header holds it.
pub fn setup_of(v: &serde_json::Value) -> Result<Wheel, String> {
    let m: mvs_trace::WheelSpec = serde_json::from_value(v.clone()).map_err(|e| format!("wheel: {e}"))?;
    let chaos = match m.chaos.as_deref() {
        None => mvs_mutators::ChaosRate::Normal,
        Some(k) => mvs_mutators::ChaosRate::from_key(k).ok_or_else(|| format!("wheel: chaos {k}"))?,
    };
    let (seconds, warn) = (mvs_sim::math::narrow(m.seconds), mvs_sim::math::narrow(m.warn));
    Ok(Wheel { pool: m.pool, seconds, warn, chaos, two: m.two })
}

/// [`setup_of`]'s inverse.
pub fn setup_put(w: &Wheel) -> serde_json::Value {
    let chaos = (w.chaos != mvs_mutators::ChaosRate::Normal).then(|| w.chaos.key().to_owned());
    let (seconds, warn) = (f64::from(w.seconds), f64::from(w.warn));
    let spec = mvs_trace::WheelSpec { pool: w.pool.clone(), seconds, warn, chaos, two: w.two };
    serde_json::to_value(spec).expect("a wheel writes")
}

/// The World's rest key of the rule (none without a wheel).
pub fn rest_put(w: &World, x: &Wx, o: &mut Vec<(&'static str, Value)>) {
    if let Some(st) = &w.wheel {
        o.push(("wheel", st.to_value(x)));
    }
}

/// [`rest_put`]'s inverse: the rule's state where the World has a wheel.
pub fn rest_load(w: &mut World, ob: &Obj, l: &mut Lx) {
    let v = ob.get("wheel");
    if v.is_some() != w.wheel.is_some() {
        l.miss(format_args!("the wheel in the record {}, in the World {}", v.is_some(), w.wheel.is_some()));
    }
    if let (Some(v), Some(st)) = (v, w.wheel.as_mut()) {
        l.at("wheel", |l| st.load_value(v, l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wheel_reads_back() {
        let mut w = Wheel::of(vec!["Mutator_Giant".to_owned()]);
        assert_eq!(setup_of(&setup_put(&w)), Ok(w.clone()));
        w.chaos = mvs_mutators::ChaosRate::Rare;
        w.two = true;
        assert_eq!(setup_of(&setup_put(&w)), Ok(w));
    }
}
