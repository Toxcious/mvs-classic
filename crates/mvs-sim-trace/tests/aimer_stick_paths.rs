//! ProjectileAimer's right stick (`fighter/match_side.rs` `tick_aimer`, AimInfo.UseRightStickToAim): no trace record
//! carries `rsy` and none a non-zero `rsx`, so no match runs this path. A fresh World, a montage with an aimer played
//! with the stick held, the aimer's arrow read. Without a store the tests check nothing and say so.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, bot: bool) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, ..MatchPlayer::default() }
}

/// The arrow (UE Y, Z) of the fighter's aimer after `ticks` ticks of `montage` with `held` as his input.
fn aimed(who: &str, montage: &str, held: InputState, ticks: usize) -> Option<(f32, f32)> {
    let setup = MatchSetup { players: vec![player(who, false), player("shaggy", true)], ..MatchSetup::default() };
    let mut h: Harness = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&[NO_INPUT, NO_INPUT]);
    }
    let f = h.fighter(0);
    let m = h.w.lit(montage);
    start_montage(&mut h.w, f, m, &PROXY_NONE, None);
    let mut last = None;
    for _ in 0..ticks {
        h.step(&[held, NO_INPUT]);
        last = h.w.fighter(f).items.aimer.as_ref().map(|a| (a.ay, a.az)).or(last);
    }
    Some(last.expect("the montage made an aimer"))
}

/// The right stick turns the arrow when the notify asks for it; a right stick with `rsx = -rsy` (the sum exactly 0)
/// falls back to the move axes, so it matches the same move axes with no right stick.
fn stick_rules(who: &str, montage: &str, ticks: usize) {
    let none = aimed(who, montage, NO_INPUT, ticks);
    let Some(none) = none else { return };
    let up = aimed(who, montage, InputState { rsx: Some(0.0), rsy: Some(1.0), ..NO_INPUT }, ticks).unwrap();
    let down = aimed(who, montage, InputState { rsx: Some(0.0), rsy: Some(-1.0), ..NO_INPUT }, ticks).unwrap();
    assert_ne!(up, none, "{montage}: the right stick's Y turned the arrow");
    assert_ne!(up, down, "{montage}: up and down turn it apart");
    let moved = aimed(who, montage, InputState { y: 1.0, ..NO_INPUT }, ticks).unwrap();
    assert_ne!(moved, none, "{montage}: the move axes turn the arrow");
    let both = InputState { y: 1.0, rsx: Some(0.5), rsy: Some(-0.5), ..NO_INPUT };
    assert_eq!(aimed(who, montage, both, ticks).unwrap(), moved, "{montage}: a stick pair summing to 0 falls back");
    let off = InputState { y: 1.0, rsx: Some(0.0), rsy: Some(0.0), ..NO_INPUT };
    assert_eq!(aimed(who, montage, off, ticks).unwrap(), moved, "{montage}: a centred stick falls back");
}

/// The held-item forward throw: the notify's own 'Use Right Stick to Aim' (class default true).
#[test]
fn the_item_throw_aims_with_the_right_stick() {
    stick_rules("arya", "Arya_Item_ThrowForward_Ground_Montage", 20);
}

/// A Spawner notify whose AimInfo sets UseRightStickToAim: Taz's ground neutral.
#[test]
fn taz_ground_neutral_aims_with_the_right_stick() {
    stick_rules("taz", "C015_Neutral_Normal_Ground_Montage", 20);
}
