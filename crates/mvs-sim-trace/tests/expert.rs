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
