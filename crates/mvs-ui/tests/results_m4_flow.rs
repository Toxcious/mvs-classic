//! The results screen's stat sheets and podium order from the PodiumData of trace matches: the World stepped from
//! the header of the Rust baseline's trace (`run::baseline`, the Rust sim's own match: the oracle's traces hold the
//! oracle sim's, which the float32 sim leaves on the same recorded inputs) to its end, `podium_data` read and checked
//! against the trace's last frame, each sheet's rows computed as UI_EOG_StatSheet PopulateStats does. m4-flow ends
//! its match through the flow (player 1 wins on falls, no ring-out credited, no damage); m5-combat has the combat
//! numbers (read at its last tick, the match not over). With no trace or no store the test passes with a note.

use mvs_sim::world::flow::podium_data;
use mvs_sim_trace::run;
use mvs_ui::results::{EndOfGame, StatRow, podium_order, stat_rows};

const KOS: &str = "CDAF30DF406FBD9BABE735A1DA7946A7";
const DAMAGE: &str = "E2F856D74A29018E6BEF12B5865F95A0";

/// The 1v1 EndOfGameData at the end of trace match `name` (local player 0), None with no trace or store.
fn end_of(name: &str) -> Option<(EndOfGame, bool)> {
    let dir = run::baseline().dir;
    let Some((_, path)) = run::traces(&dir, &[name.to_string()]).ok().and_then(|l| l.into_iter().next()) else {
        eprintln!("note: no {name} trace in {}; nothing checked", dir.display());
        return None;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
        return None;
    };
    let t = mvs_trace::Trace::read(&path).expect("a trace that reads");
    let (mut w, book) = mvs_sim_trace::world_for(&t.header, &store).expect("the World of the header");
    let mut last = None;
    for i in 0..t.len() {
        let frame = t.frame(i).expect("a frame");
        let inputs: Vec<_> = frame.inputs.iter().map(|r| mvs_sim_trace::input_of(r, &w, &book)).collect();
        w.step(&inputs);
        w.events.clear();
        last = Some(frame);
    }
    let players = podium_data(&w);
    // the stats PodiumData reads are the record's: a difference here is the sim's (sim-trace shows where)
    let last = last.expect("a trace with frames");
    for (p, f) in players.iter().zip(&last.world.fighters) {
        let recorded = (f.ring_outs, f.damage_dealt, f.kos);
        let read = (f64::from(p.ring_outs), f64::from(p.damage_dealt), f64::from(p.falls));
        assert_eq!(read, recorded, "{name} player {}: PodiumData against the last frame", p.player_index);
    }
    let eog =
        EndOfGame { team_style: 0, me: 0, names: vec!["P1".into(), "P2".into()], players, ..EndOfGame::default() };
    Some((eog, w.flow.match_over))
}

fn row(title: &'static str, value: &str, highlight: Option<i32>) -> StatRow {
    StatRow { title, value: value.to_string(), highlight }
}

#[test]
fn stat_sheets_of_m4_flow() {
    let Some((eog, over)) = end_of("m4-flow") else { return };
    assert!(over, "m4-flow ends its match");
    assert_eq!(eog.players.iter().map(|p| p.is_winner).collect::<Vec<_>>(), [false, true]);
    // the winner first
    assert_eq!(podium_order(&eog), [1, 0]);
    for i in 0..2 {
        assert_eq!(stat_rows(&eog, i), [row(KOS, "0", None), row(DAMAGE, "0", None)], "player {i}");
    }
}

#[test]
fn stat_sheets_of_m5_combat() {
    let Some((eog, _)) = end_of("m5-combat") else { return };
    // the Rust baseline's numbers (`sim-trace --record`; a baseline that moves m5-combat moves them): the KOs tie at 3,
    // both highlighted; player 0 has more damage (500 to 489); a higher stat is highlighted in its team's palette
    // index, 0 for your own team, 1 for the other
    assert_eq!(stat_rows(&eog, 0), [row(KOS, "3", Some(0)), row(DAMAGE, "500", Some(0))]);
    assert_eq!(stat_rows(&eog, 1), [row(KOS, "3", Some(1)), row(DAMAGE, "489", None)]);
    // no winner yet: your own record first
    assert_eq!(podium_order(&eog), [0, 1]);
}
