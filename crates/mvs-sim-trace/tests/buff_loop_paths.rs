//! BuffParentActor's loop sound (`combat::buffs` parent_loop_begin / parent_loop_end) on the paths the matches may
//! not take: a CanOverride buff swept by a new one of its class (the old one's destroy stop ahead of the new loop),
//! a removal (SFX_Loop_Stop cut with the loop, then the destroy's stop) and the respawn's end (`fire_end`). On a fresh
//! World of Morty (Buff_HealOverTime) and Wonder Woman (Buff_NullifyNextHit); without a store it checks nothing.

use mvs_sim::combat::buffs::{BuffRef, buff_of, fire_end, remove_buff, spawn_buff};
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn pair(a: &str, b: &str) -> Option<Harness> {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player(a, 0), player(b, 1)], ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    for _ in 0..10 {
        h.step(&NONE);
    }
    Some(h)
}

fn spawn(w: &mut World, f: FighterId, cls: &str) -> BuffRef {
    let c = w.lit(cls);
    spawn_buff(w, f, c, 0.0, None, Some(f), false, Some(f)).unwrap_or_else(|| panic!("{cls} spawns"))
}

/// The fighter's sound events since the last call, as text: `start ev key`, `stop ev key fade cut`, `sound ev`.
fn sounds(w: &mut World, f: FighterId) -> Vec<String> {
    let evs = std::mem::take(&mut w.fighter_mut(f).events);
    let s = |n| w.names.str(n).to_owned();
    evs.iter()
        .filter_map(|e| match *e {
            Event::SoundStart { ev, key, .. } => Some(format!("start {} {}{}", s(ev), s(key.tag), key.n)),
            Event::SoundStop { key, fade, ev, cut, .. } => {
                Some(format!("stop {} {}{} {fade:?} {cut}", ev.map_or_else(String::new, s), s(key.tag), key.n))
            }
            Event::Sound { ev, .. } => Some(format!("sound {}", s(ev))),
            _ => None,
        })
        .collect()
}

#[test]
fn a_swept_buff_stops_its_loop_before_the_new_one_starts() {
    let Some(mut h) = pair("morty", "shaggy") else { return };
    let f = h.fighter(0);
    let id = h.w.fighter(f).id;
    let w = &mut h.w;
    sounds(w, f);
    spawn(w, f, "Buff_HealOverTime_C");
    let key = format!("Buff_HealOverTime_C{id}");
    assert!(sounds(w, f).contains(&format!("start sfx_shared_combat_heal {key}")));
    // CanOverride: the live one's EndPlay (no SFX_Loop_Stop or _End: the destroy's stop alone), then the new loop.
    spawn(w, f, "Buff_HealOverTime_C");
    let got: Vec<String> = sounds(w, f).into_iter().filter(|s| s.contains(&key)).collect();
    assert_eq!(got, [format!("stop  {key} Some(0.0) false"), format!("start sfx_shared_combat_heal {key}")]);
}

#[test]
fn a_removed_or_respawned_buff_cuts_its_loop() {
    let Some(mut h) = pair("wonderwoman", "shaggy") else { return };
    let f = h.fighter(0);
    let id = h.w.fighter(f).id;
    let w = &mut h.w;
    let key = format!("Buff_NullifyNextHit_C{id}");
    let ends = [
        format!("stop sfx_cha_wonderwoman_shieldbuff_loop_stop {key} None true"),
        format!("stop  {key} Some(0.0) false"),
    ];
    sounds(w, f);
    spawn(w, f, "Buff_NullifyNextHit_C");
    assert!(sounds(w, f).contains(&format!("start sfx_cha_wonderwoman_shieldbuff_loop {key}")));
    let cls = w.lit("Buff_NullifyNextHit_C");
    assert!(remove_buff(w, f, Some(cls), None));
    let got: Vec<String> = sounds(w, f).into_iter().filter(|s| s.contains(&key)).collect();
    assert_eq!(got, ends);
    // The respawn's end of a live buff (match_side's call).
    let b = spawn(w, f, "Buff_NullifyNextHit_C");
    sounds(w, f);
    assert_eq!(buff_of(w, f, cls), Some(b));
    fire_end(w, f, b);
    let got: Vec<String> = sounds(w, f).into_iter().filter(|s| s.contains(&key)).collect();
    assert_eq!(got, ends);
}
