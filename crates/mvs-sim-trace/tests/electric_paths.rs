//! Buff_Low_Damage_Electric (`combat::electric`), the data-only child of Buff_Damage_Electric: its imported class
//! default carries the parent's whole Damage struct with Damage 0.5, and the entry runs the parent's pulses at that
//! damage. On a fresh World of Wonder Woman (whose lasso perk gives it) and Shaggy; without a store it checks nothing.

use mvs_sim::actors::ActorRef;
use mvs_sim::combat::buffs::{buff_of, spawn_buff};
use mvs_sim::fighter::common::buff_def;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn pair() -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player("wonderwoman", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    Some(h)
}

#[test]
fn the_child_default_holds_the_parents_damage_struct_at_half() {
    let Some(h) = pair() else { return };
    let ww = h.fighter(0);
    let cls = h.w.lit("Buff_Low_Damage_Electric_C");
    let d = buff_def(&h.w, ActorRef::Fighter(ww), cls).expect("Wonder Woman's data holds Buff_Low_Damage_Electric_C");
    let dmg = d.get("Damage").and_then(|v| v.as_obj()).expect("a Damage struct");
    let num = |k: &str| dmg.get(k).and_then(mvs_data::value::Value::as_f64);
    assert_eq!(num("Damage"), Some(0.5), "the child's own value");
    // the parent's members, written only in its default
    assert_eq!(dmg.get("ArmorPierce").and_then(mvs_data::value::Value::as_bool), Some(true));
    assert_eq!(num("BaseKnockBack"), Some(0.0));
    assert_eq!(num("HitStunMult"), Some(0.0));
    assert!(dmg.len() > 5, "the whole struct, not the child's one member: {:?}", dmg.keys().collect::<Vec<_>>());
    assert_eq!(d.get("DamageIntervalTimer").and_then(mvs_data::value::Value::as_f64), Some(0.8));
    assert_eq!(d.get("ChainLightningRange").and_then(mvs_data::value::Value::as_f64), Some(500.0));
}

#[test]
fn the_child_pulses_at_half_damage() {
    let Some(mut h) = pair() else { return };
    let (ww, s) = (h.fighter(0), h.fighter(1));
    let cls = h.w.lit("Buff_Low_Damage_Electric_C");
    let before = h.w.fighter(s).stats.damage;
    spawn_buff(&mut h.w, s, cls, 5.0, Some(0), Some(ww), true, Some(s)).expect("the buff spawns");
    // the BeginPlay's Trigger runs in the frame's timer pass
    h.step(&NONE);
    let first = h.w.fighter(s).stats.damage;
    assert!((first - before - 0.5).abs() < 1e-6, "one pulse of 0.5: {before} -> {first}");
    // the looping timer: the next Trigger 0.8 s (48 frames) after the spawn's frame (the spawn here is made between
    // two steps, so it counts from the frame before the first step)
    let mut n = 0;
    while h.w.fighter(s).stats.damage == first && n < 60 {
        h.step(&NONE);
        n += 1;
    }
    let second = h.w.fighter(s).stats.damage;
    assert!(buff_of(&h.w, s, cls).is_some(), "the buff lives");
    assert!((47..=48).contains(&n), "the second pulse after {n} steps");
    assert!((second - first - 0.5).abs() < 1e-6, "the second pulse: {first} -> {second}");
}
