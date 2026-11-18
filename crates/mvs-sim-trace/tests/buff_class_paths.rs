//! The buff tests by class take a child class (BuffParentActor.IsBuffOfClass @152-228: `combat::buffs`
//! is_buff_of_class, through has_buff, buff_stacks, remove_buff and remove_buff_stack): LeBron's
//! Buff_IgnitedBasketballPass_C is a child of Buff_BasketballPass_C, so Base_BehaviorTree's TargetHasBuff
//! (Buff_BasketballPass_C) holds on it. The parents are set here as the gamedata's BuffSupers entry gives them (the
//! oracle's store has none); without a store it checks nothing.

use std::sync::Arc;

use mvs_sim::combat::buffs::{
    buff, buff_stacks, has_buff, is_buff_of_class, remove_buff, remove_buff_stack, spawn_buff,
};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];
const PASS: &str = "Buff_BasketballPass_C";
const IGNITED: &str = "Buff_IgnitedBasketballPass_C";

/// LeBron and Shaggy, the ignited pass's parents in the data.
fn lebron() -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player("lebron", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    let supers = &mut Arc::make_mut(&mut h.w.data).buff_supers;
    supers.insert(IGNITED.to_owned(), vec![PASS.to_owned(), "BuffParentActor_C".to_owned()]);
    supers.insert(PASS.to_owned(), vec!["BuffParentActor_C".to_owned()]);
    for _ in 0..10 {
        h.step(&NONE);
    }
    Some(h)
}

#[test]
fn a_class_is_of_itself_and_its_parents_only() {
    let Some(h) = lebron() else { return };
    let w = &h.w;
    assert!(is_buff_of_class(w, IGNITED, IGNITED));
    assert!(is_buff_of_class(w, IGNITED, PASS));
    assert!(is_buff_of_class(w, IGNITED, "BuffParentActor_C"));
    assert!(!is_buff_of_class(w, PASS, IGNITED));
    assert!(!is_buff_of_class(w, "Buff_Educated_C", PASS));
}

#[test]
fn a_child_buff_answers_for_its_parent() {
    let Some(mut h) = lebron() else { return };
    let f = h.fighter(1);
    let w = &mut h.w;
    let (pass, ignited) = (w.lit(PASS), w.lit(IGNITED));
    assert!(!has_buff(w, f, pass));
    let b = spawn_buff(w, f, ignited, 5.0, Some(2), Some(f), false, Some(f)).expect("the ignited pass spawns");
    assert!(has_buff(w, f, pass), "CheckHasBuff(Buff_BasketballPass_C) holds on its child");
    assert!(has_buff(w, f, ignited));
    let before = buff(w, b).expect("live").stacks;
    assert_eq!(buff_stacks(w, f, PASS), before);
    // RemoveBuffStack of the parent class takes the child's stacks
    remove_buff_stack(w, f, pass, 1);
    assert!(buff(w, b).is_none_or(|x| x.stacks < before || x.frames <= 0.0), "a stack of the child removed");
}

#[test]
fn removing_the_parent_class_destroys_the_child() {
    let Some(mut h) = lebron() else { return };
    let f = h.fighter(1);
    let w = &mut h.w;
    let (pass, ignited) = (w.lit(PASS), w.lit(IGNITED));
    spawn_buff(w, f, ignited, 5.0, Some(1), Some(f), false, Some(f)).expect("the ignited pass spawns");
    assert!(remove_buff(w, f, Some(pass), None), "RemoveBuff(Buff_BasketballPass_C) destroys its child");
    assert!(!has_buff(w, f, ignited));
}

#[test]
fn a_parent_buff_does_not_answer_for_its_child() {
    let Some(mut h) = lebron() else { return };
    let f = h.fighter(1);
    let w = &mut h.w;
    let (pass, ignited) = (w.lit(PASS), w.lit(IGNITED));
    spawn_buff(w, f, pass, 5.0, Some(1), Some(f), false, Some(f)).expect("the pass spawns");
    assert!(has_buff(w, f, pass));
    assert!(!has_buff(w, f, ignited));
    assert_eq!(buff_stacks(w, f, IGNITED), 0.0);
    assert!(!remove_buff(w, f, Some(ignited), None));
    assert!(has_buff(w, f, pass));
}
