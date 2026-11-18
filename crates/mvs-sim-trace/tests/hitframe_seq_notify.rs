//! A hit-reaction sequence's AnimNotify_Hitframe reaches the sim: the importer keeps the notify of the sequence
//! (`CharDef.seq_notifies`) and the sequence state's begin runs `hitframe_begin` (the hit on `montages.active_hits`
//! under the sequence's key, below 0), its end takes it off.

use mvs_sim::fighter::anim::{seq_base, seq_states_tick};
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim_trace::harness;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

#[test]
fn a_hit_reaction_sequences_hitframe_begins_in_the_sim() {
    let player =
        |c: &str, team| MatchPlayer { character: Some(c.to_owned()), team: Some(team), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player("shaggy", 0), player("shaggy", 1)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    for _ in 0..10 {
        h.step(&NONE);
    }
    let f = h.fighter(0);

    // the store's sequence data: a Hitframe state notify on an HR_ sequence
    let seqs = &h.w.data.chars.get("shaggy").expect("shaggy").def.seq_notifies;
    let found = seqs.iter().find_map(|(name, list)| {
        if !name.contains("HR_") {
            return None;
        }
        let i = list.iter().position(|n| n.cls == "AnimNotify_Hitframe_C" && n.state)?;
        Some((name.clone(), i))
    });
    let (name, i) = found.expect("an HR_ sequence of Shaggy's holds an AnimNotify_Hitframe in the store");
    eprintln!("hitframe notify {i} of {name}");

    // the sim: the state listed by the layer's update begins, then ends
    let anim = h.w.lit(&name);
    let key = seq_base(&h.w, f, anim) + i32::try_from(i).expect("an index");
    let idx = -1 - key;
    assert!(!h.w.fighter(f).montages.active_hits.contains_key(&idx));
    seq_states_tick(&mut h.w, f, &[key]);
    assert!(h.w.fighter(f).montages.active_hits.contains_key(&idx), "hitframe_begin put the hit on the list");
    seq_states_tick(&mut h.w, f, &[]);
    assert!(!h.w.fighter(f).montages.active_hits.contains_key(&idx), "hitframe_end took it off");
}
