//! Comp_Actor_VO's VO_TagContainer (`fighter/match_side.rs` set_attack_vo_tags / set_help_vo_tags /
//! vo_tags_latent): the tag a hit puts in, the 1 s RetriggerableDelay that takes it out, and the pick it moves.

use mvs_sim::fighter::match_side;
use mvs_sim_trace::harness;

/// bot-ffa-a: Superman (fighter 2) hits Bugs at frame 4494; his VO post at frame 4504 then searches Bugs' enemy tag
/// alone (no SetAllyOpponentTags), the first frame this match moves from the oracle's.
#[test]
fn a_hit_puts_the_victims_enemy_tag_in_for_one_second() {
    let Some(mut h) = harness::at("bot-ffa-a", 4490) else { return };
    let sup = h.fighter(2);
    assert!(h.w.fighter(sup).vo.vo_tags.is_empty());
    let mut hit_at = None;
    for _ in 0..80 {
        h.step_recorded();
        let vo = &h.w.fighter(sup).vo;
        if hit_at.is_none() && !vo.vo_tags.is_empty() {
            hit_at = Some(h.frame_index());
            eprintln!("frame {}: tags {:?}", h.frame_index(), vo.vo_tags);
            let d = &h.w.data.chars.get("bugs").expect("bugs").def;
            assert_eq!(vo.vo_tags, vec![d.vo_tag_enemy.clone()]);
        }
        if let Some(at) = hit_at
            && vo.vo_tags.is_empty()
        {
            eprintln!("frame {}: tags out, {} frames after the hit", h.frame_index(), h.frame_index() - at);
            // FDelayAction 1.0 on fround(1/60) steps, the hit's frame counted.
            assert_eq!(h.frame_index() - at, 60 - 1);
            return;
        }
    }
    panic!("the tag never came in or never went out (hit at {hit_at:?})");
}

/// No trace match is known to reach SetHelpVOTags with a VO post inside its second: the help tag goes in, a second
/// call retriggers the delay, and the "friend" tag goes out 1 s after the last.
#[test]
fn a_hit_on_an_ally_puts_its_friend_tag_in_and_retriggers() {
    let Some(mut h) = harness::at("bot-ffa-a", 100) else { return };
    let (a, b) = (h.fighter(0), h.fighter(1));
    match_side::set_help_vo_tags(&mut h.w, a, b);
    let tag = h.w.fighter(a).vo.vo_tags.clone();
    eprintln!("help tag {tag:?}");
    assert_eq!(tag.len(), 1);
    for _ in 0..30 {
        h.step_recorded();
    }
    match_side::set_help_vo_tags(&mut h.w, a, b);
    assert_eq!(h.w.fighter(a).vo.vo_tags, tag, "AddUnique: no second copy");
    let mut n = 0;
    while !h.w.fighter(a).vo.vo_tags.is_empty() {
        h.step_recorded();
        n += 1;
        assert!(n < 120, "the friend tag never went out");
    }
    // Set outside a step here, so the first count is the next step's: 60 steps.
    assert_eq!(n, 60);
}
