//! Base_AnimBP's carried pose (`fighter/anim/update.rs` set_being_carried) on a path no trace match runs: Velma's
//! capture of a team mate shows his anim set's CarriedAbove, looped, in place of the movement machine, and the release
//! takes it off. Without a store the test checks nothing and says so.

use mvs_sim::actors::ActorRef;
use mvs_sim::chars::velma::capture::{knocked_back, start_capture};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn player(c: &str, bot: bool, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), bot, team: Some(team), ..MatchPlayer::default() }
}

#[test]
fn a_captured_ally_shows_the_carried_pose_until_released() {
    let setup =
        MatchSetup { players: vec![player("velma", false, 0), player("shaggy", true, 0)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        h.step(&NONE);
    }
    let (v, s) = (h.fighter(0), h.fighter(1));
    let clip = h.w.char_data(s).fighter.anim_set.get("CarriedAbove").and_then(|v| v.as_str());
    let clip = clip.expect("a CarriedAbove clip").to_owned();
    assert!(h.w.fighter(s).anim.carried.is_none());
    start_capture(&mut h.w, v, s, ActorRef::Fighter(v)).expect("the capture lives");
    let c = h.w.fighter(s).anim.carried.expect("the carried pose on");
    assert!(c.above, "CarryInt 1");
    h.step(&NONE);
    h.step(&NONE);
    let an = &h.w.fighter(s).anim;
    assert_eq!(h.w.names.str(an.anim), clip, "the carried clip in place of the machine's");
    assert!(an.anim_time > 0.0, "its player runs");
    knocked_back(&mut h.w, v, 0.0, 0.0, 0.0, None);
    assert!(h.w.fighter(s).anim.carried.is_none(), "the release takes the pose off");
    h.step(&NONE);
    assert_ne!(h.w.names.str(h.w.fighter(s).anim.anim), clip);
}
