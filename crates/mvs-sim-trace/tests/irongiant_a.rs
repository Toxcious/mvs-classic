//! Paths of Iron Giant's hover rage that no match runs, on a World loaded from `irongiant-combat` (both fighters are
//! Giants there). With no baseline trace the tests check nothing and say so.

use mvs_sim::chars::iron_giant::{hover_rage, state};
use mvs_sim_trace::harness;

fn giants() -> Option<harness::Harness> {
    harness::at("irongiant-combat", 400)
}

/// Rage as the hover rage holds it, in float32.
fn rage(h: &harness::Harness, n: usize) -> f64 {
    f64::from(state::giant(&h.w, h.fighter(n)).hover_rage.as_ref().expect("the component").rage)
}

#[test]
fn an_own_hit_adds_eight_tenths_of_the_damage_and_an_ally_hit_all_of_it() {
    let Some(mut h) = giants() else { return };
    let f = h.fighter(0);
    hover_rage::reset_rage(&mut h.w, f, false);
    assert_eq!(rage(&h, 0), 0.0);
    hover_rage::own_hit(&mut h.w, f, 5.0);
    assert_eq!(rage(&h, 0), 4.0);
    hover_rage::ally_hit(&mut h.w, f, 6.0);
    assert_eq!(rage(&h, 0), 10.0);
    hover_rage::ally_death(&mut h.w, f);
    assert_eq!(rage(&h, 0), 45.0);
}

#[test]
fn a_full_meter_sets_max_rage_and_a_reset_from_respawn_keeps_the_respawn_rage() {
    let Some(mut h) = giants() else { return };
    let f = h.fighter(1);
    hover_rage::reset_rage(&mut h.w, f, false);
    hover_rage::own_hit(&mut h.w, f, 200.0);
    assert_eq!(rage(&h, 1), 100.0);
    assert!(state::giant(&h.w, f).is_max_rage);
    hover_rage::set_respawn_rage(&mut h.w, f, 25.0);
    hover_rage::reset_rage(&mut h.w, f, true);
    assert_eq!(rage(&h, 1), 25.0);
    hover_rage::reset_rage(&mut h.w, f, false);
    assert_eq!(rage(&h, 1), 0.0);
}

#[test]
fn fuel_goes_down_by_the_percent_of_the_maximum_and_stops_at_zero() {
    let Some(mut h) = giants() else { return };
    let f = h.fighter(0);
    hover_rage::owner_reset(&mut h.w, f);
    let fuel = |h: &harness::Harness| state::giant(&h.w, f).hover_rage.as_ref().expect("the component").fuel;
    assert_eq!(fuel(&h), 100.0);
    hover_rage::use_fuel(&mut h.w, f, 0.25);
    assert_eq!(fuel(&h), 75.0);
    hover_rage::use_fuel(&mut h.w, f, 5.0);
    assert_eq!(fuel(&h), 0.0);
}
