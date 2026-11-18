//! `minionRollback`: Velma's police car, a minion with a controller of its own, over
//! a rollback every tick. One World runs straight; the other goes back to its snapshot every tick and steps `lag`
//! predicted ticks past it on the bot's held input (the predicted line: the car's controller holds its pad, no tree
//! runs). The two must hash the same after every tick, and the car must drive (its controller's path is the
//! confirmed line's, not frozen by the predicted steps).

mod common;

use mvs_sim::ai::bt::BtBot;
use mvs_sim::chars::velma::police::call_police;
use mvs_sim::chars::velma::velma;
use mvs_sim::input::{NO_INPUT, held};
use mvs_sim::setup::MatchPlayer;
use mvs_sim::snapshot::SnapshotRing;
use mvs_sim::state::hash_of;
use mvs_sim::{InputState, World};

const CALL: usize = 300;
const TICKS: usize = 700;

/// The inputs of a step: a bot's slot its thought, the rest empty.
fn think(w: &mut World, bots: &mut [Option<BtBot>]) -> Vec<InputState> {
    (0..bots.len())
        .map(|i| {
            let me = w.players[i];
            bots[i].as_mut().map_or(NO_INPUT, |b| b.think(w, me))
        })
        .collect()
}

fn minion_rollback(lag: usize) {
    let Some(data) = common::data() else { return };
    let mut setup = common::setup(&common::versus(2, true));
    setup.players[0] = MatchPlayer { bot: false, character: Some("velma".to_owned()), ..setup.players[0].clone() };
    setup.players[1] = MatchPlayer { bot: true, character: Some("shaggy".to_owned()), ..setup.players[1].clone() };
    let mk = || {
        let w = common::world(&data, &setup);
        let b = common::bots(&w);
        (w, b)
    };
    let (mut rw, mut rb) = mk();
    let (mut w, mut b) = mk();
    let mut ring = SnapshotRing::new(1, &w);
    ring.save(0, &w);
    let (mut at, mut moved) = (None::<(f64, f64)>, 0.0f64);
    for t in 0..TICKS {
        if t == CALL {
            let v = rw.players[0];
            call_police(&mut rw, v);
        }
        let ins = think(&mut rw, &mut rb);
        rw.step(&ins);
        assert!(ring.load(0, &mut w));
        if t == CALL {
            let v = w.players[0];
            call_police(&mut w, v);
        }
        let last = think(&mut w, &mut b);
        w.step(&last);
        rw.events.clear();
        w.events.clear();
        assert_eq!(hash_of(&rw), hash_of(&w), "lag {lag}, tick {}: the World going back every tick differs", t + 1);
        let v = w.players[0];
        if let Some(m) = velma(&w, v).police {
            let body = &w.fighter(m).body;
            let (x, y) = (body.x, body.y);
            let a = *at.get_or_insert((f64::from(x), f64::from(y)));
            moved = moved.max((f64::from(x) - a.0).hypot(f64::from(y) - a.1));
        }
        ring.save(0, &w);
        w.predicted = true;
        for p in t + 1..=t + lag {
            if p == CALL {
                let v = w.players[0];
                call_police(&mut w, v);
            }
            let ins: Vec<InputState> =
                (0..b.len()).map(|i| if b[i].is_some() { held(&last[i]) } else { NO_INPUT }).collect();
            w.step(&ins);
            w.events.clear();
        }
        w.predicted = false;
    }
    assert!(at.is_some(), "lag {lag}: no police car");
    assert!(moved >= 50.0, "lag {lag}: the car moved {moved:.0} from its spawn");
    println!("ok   rollback, Velma's police car with {lag} predicted ticks: {TICKS} ticks, the car moved {moved:.0}");
}

#[test]
fn police_car_over_rollbacks_lag_2() {
    minion_rollback(2);
}

#[test]
fn police_car_over_rollbacks_lag_8() {
    minion_rollback(8);
}
