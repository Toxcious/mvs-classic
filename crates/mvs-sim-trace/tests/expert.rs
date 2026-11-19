//! The Expert bot (`mvs_sim::ai::expert`): its look-ahead writes nothing to the real World, and a clone of an Expert
//! bot plans as the original does. On the dev store; a tree without one says so and passes.

use mvs_sim::ai::bt::BtBot;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle};
use mvs_sim::state::hash_of;
use mvs_sim::world::World;
use std::sync::Arc;

/// A 1v1 Shaggy mirror with the flow (intro and countdown), bot slots at `levels`, and its bot trees.
fn world(levels: [f32; 2]) -> Option<(World, Arc<mvs_data::BotTrees>)> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let data = Arc::new(mvs_data::GameData::load(&store, &["shaggy"], mvs_sim_trace::TRACE_STAGE).ok()?);
    let player = |i: usize| MatchPlayer {
        bot: true,
        difficulty: Some(levels[i]),
        character: Some("shaggy".to_owned()),
        team: Some(u8::try_from(i).expect("two slots")),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: vec![player(0), player(1)],
        rules: Some(Rules { knockouts_to_win: 3.0, duration: 420.0, stocks: false }),
        flow: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data.clone(), 77, &setup).ok()?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    Some((w, trees))
}

/// Both bots think, then the World steps on their inputs.
fn tick(w: &mut World, bots: &mut [BtBot]) -> Vec<InputState> {
    let ins: Vec<InputState> = bots.iter_mut().enumerate().map(|(i, b)| b.think(w, w.players[i])).collect();
    w.step(&ins);
    w.events.clear();
    ins
}

/// An Expert bot's think leaves the World as a Hard bot of the same seed leaves it: the look-ahead works on its own
/// copies. Both Worlds step on the Hard bot's inputs, so the two trees see the same match.
#[test]
fn the_look_ahead_writes_nothing_to_the_world() {
    let Some((mut wa, trees)) = world([4.0, 3.0]) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let mut wb = wa.clone();
    let mut expert = BtBot::match_bot(5, &trees, 4);
    let mut hard = BtBot::match_bot(5, &trees, 3);
    let mut other = [BtBot::match_bot(6, &trees, 3), BtBot::match_bot(6, &trees, 3)];
    let mut differed = 0;
    for t in 0..900 {
        let (a0, b0) = (wa.players[0], wb.players[0]);
        let e = expert.think(&mut wa, a0);
        let h = hard.think(&mut wb, b0);
        differed += usize::from(e != h);
        let (a1, b1) = (wa.players[1], wb.players[1]);
        let oa = other[0].think(&mut wa, a1);
        let ob = other[1].think(&mut wb, b1);
        assert_eq!(oa, ob, "tick {t}: the other bot's input");
        assert_eq!(hash_of(&wa), hash_of(&wb), "tick {t}: the World after the thinks");
        wa.step(&[h, oa]);
        wb.step(&[h, ob]);
        wa.events.clear();
        wb.events.clear();
    }
    assert!(differed > 0, "the Expert never gave another input than the tree's");
}

/// A clone of an Expert bot and of its World, taken mid-match, plays on exactly as the original.
#[test]
fn a_cloned_expert_plans_as_the_original() {
    let Some((mut w, trees)) = world([4.0, 3.0]) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let mut bots = vec![BtBot::match_bot(5, &trees, 4), BtBot::match_bot(6, &trees, 3)];
    for _ in 0..500 {
        tick(&mut w, &mut bots);
    }
    let mut w2 = w.clone();
    let mut bots2 = bots.clone();
    for t in 0..600 {
        let a = tick(&mut w, &mut bots);
        let b = tick(&mut w2, &mut bots2);
        assert_eq!(a, b, "tick {t}: the inputs");
        assert_eq!(hash_of(&w), hash_of(&w2), "tick {t}: the World");
    }
}

/// A network-led Expert match played twice gives the same inputs and the same World every tick, and a bot taking the
/// teacher's samples plays as one that does not.
#[test]
fn a_network_led_match_is_the_same_twice() {
    use mvs_sim::ai::expert::{Lead, net};
    let Some((w0, trees)) = world([4.0, 4.0]) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let bots = |teach: u32| {
        let mut b = vec![BtBot::match_bot(5, &trees, 4), BtBot::match_bot(6, &trees, 4)];
        let e = b[0].expert.as_mut().expect("an Expert");
        e.set_lead(Lead::Net(net::built_in()));
        e.teach(teach);
        b
    };
    let (mut wa, mut wb) = (w0.clone(), w0);
    let (mut ba, mut bb) = (bots(0), bots(3));
    let mut samples = 0;
    for t in 0..1500 {
        let a = tick(&mut wa, &mut ba);
        let b = tick(&mut wb, &mut bb);
        samples += bb[0].expert.as_mut().expect("an Expert").take_samples().len();
        assert_eq!(a, b, "tick {t}: the inputs");
        assert_eq!(hash_of(&wa), hash_of(&wb), "tick {t}: the World");
    }
    assert!(samples > 0, "the teacher took no sample");
}

/// The built-in network reads the Expert's input and scores its whole action list.
#[test]
fn the_built_in_network_fits_the_action_list() {
    use mvs_sim::ai::expert::{ACTIONS, FEATURES, net};
    let n = net::built_in();
    assert_eq!((n.inputs(), n.outputs()), (FEATURES, ACTIONS));
}

/// A whole Expert match with its decisions on workers gives the same inputs and the same World every tick as the
/// inline match: a decision is taken back at the think the inline path finishes it, whatever the worker's pace. One
/// worker for both bots (a decision waits for the other's), then two; a clone taken mid-match, with a decision on a
/// worker, plays on as the original.
#[test]
fn the_workers_give_the_inline_match() {
    use mvs_sim::ai::expert::Workers;
    let Some((w0, trees)) = world([4.0, 4.0]) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    for threads in [1, 2] {
        let workers = Arc::new(Workers::new(threads));
        let (mut wa, mut wb) = (w0.clone(), w0.clone());
        let mut ba = vec![BtBot::match_bot(5, &trees, 4), BtBot::match_bot(6, &trees, 4)];
        let mut bb = ba.clone();
        for b in &mut bb {
            b.set_workers(Some(&workers));
        }
        let mut cloned: Option<(World, Vec<BtBot>)> = None;
        let mut t = 0;
        while !wa.flow.match_over && t < 30_000 {
            let a = tick(&mut wa, &mut ba);
            let b = tick(&mut wb, &mut bb);
            assert_eq!(a, b, "{threads} workers, tick {t}: the inputs");
            assert_eq!(hash_of(&wa), hash_of(&wb), "{threads} workers, tick {t}: the World");
            if let Some((wc, bc)) = &mut cloned {
                let c = tick(wc, bc);
                assert_eq!(a, c, "{threads} workers, tick {t}: the clone's inputs");
            } else if t == 700 {
                cloned = Some((wb.clone(), bb.clone()));
            }
            t += 1;
        }
        assert!(wa.flow.match_over, "the match ended by tick {t}");
        eprintln!("{threads} workers: the same over {t} ticks");
    }
}
