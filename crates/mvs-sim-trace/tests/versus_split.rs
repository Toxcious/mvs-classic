//! A custom lobby's Versus of uneven teams (`mvs_protocol::lobby_rules`: a 2v1 or 1v2 starts as the match's 2v2 with
//! three players, the larger team on the even slots): the World takes three players in the 2v2 style, each at a
//! spawn of its own, and plays on with its bots thinking; played to its end by ring-outs (on points and on stocks),
//! the side that rang out loses, the lone fighter's or the pair's. On the dev store; a tree without one says so and
//! passes.

use mvs_sim::ai::bt::BtBot;
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle, player_team};
use mvs_sim::world::World;
use std::sync::Arc;

/// The lobby's default map's stage (Sky Arena).
const STAGE: &str = "m011_largenoplat";
const CAST: [&str; 3] = ["shaggy", "batman", "wonderwoman"];

/// Three bots in the 2v2 style, the teams by slot (no explicit team, as a lobby's set-up), with the flow; on points
/// (4 ring-outs to win) or on stocks (4 lives).
fn world(stocks: bool) -> Option<(World, Arc<mvs_data::BotTrees>, MatchSetup)> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let data = Arc::new(mvs_data::GameData::load(&store, &CAST, STAGE).ok()?);
    let player = |c: &str| MatchPlayer {
        bot: true,
        difficulty: Some(3.0),
        character: Some(c.to_owned()),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: TeamStyle::TwoVsTwo,
        players: CAST.iter().map(|c| player(c)).collect(),
        rules: Some(Rules { knockouts_to_win: 4.0, duration: 420.0, stocks }),
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data.clone(), 31, &setup).ok()?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    Some((w, trees, setup))
}

#[test]
fn three_players_in_the_2v2_style_spawn_apart_and_play() {
    let Some((mut w, trees, setup)) = world(false) else {
        eprintln!("note: no dev store (or no {STAGE} in it); nothing checked");
        return;
    };
    assert_eq!(w.players.len(), 3);
    let teams: Vec<u8> = (0..3).map(|i| player_team(&setup, i)).collect();
    assert_eq!(teams, vec![0, 1, 0], "the teams by slot % 2");
    for (i, &f) in w.players.iter().enumerate() {
        assert_eq!(w.fighter(f).team.team, i32::from(teams[i]), "player {i}'s team");
    }
    assert_eq!(w.stage.spawns.len(), 3, "a spawn for each player");
    let at: Vec<(mvs_sim::Real, mvs_sim::Real)> = w.stage.spawns.iter().map(|s| (s.x, s.y)).collect();
    for i in 0..3 {
        for j in i + 1..3 {
            assert_ne!(at[i], at[j], "players {i} and {j} spawn at one place");
        }
    }
    let mut bots: Vec<BtBot> = (0..3).map(|i| BtBot::match_bot(40 + i, &trees, 2)).collect();
    for _ in 0..1800 {
        let players = w.players.clone();
        let ins: Vec<_> = bots.iter_mut().zip(players).map(|(b, f)| b.think(&mut w, f)).collect();
        w.step(&ins);
        w.events.clear();
    }
    // (the bots fought: the lone player or one of the pair took damage or was rung out)
    let hurt = w.players.iter().any(|&f| w.fighter(f).stats.damage > 0.0);
    assert!(hurt, "nobody was hit in 1800 ticks");
}

/// The match played with no input, the players `out` put past the outer blast box each frame they are alive (a
/// ring-out by the stage's own check), to its end: the winning team, None when it did not end in 20000 ticks.
fn play_out(stocks: bool, out: &[usize]) -> Option<Option<i32>> {
    let (mut w, ..) = world(stocks)?;
    let idle = vec![mvs_sim::input::InputState::default(); 3];
    for _ in 0..20000 {
        for &i in out {
            let f = w.players[i];
            let past = w.stage.blast_outer.right + 5000.0;
            let fr = w.fighter_mut(f);
            if fr.life.dead == 0.0 {
                fr.body.x = past;
            }
        }
        w.step(&idle);
        w.events.clear();
        if w.flow.match_over {
            return Some(Some(w.flow.winning_team));
        }
    }
    Some(None)
}

#[test]
fn the_side_rung_out_loses_the_three_player_match() {
    // (slot 1 is the lone fighter, team 1; slots 0 and 2 the pair, team 0)
    for stocks in [false, true] {
        let Some(lone_out) = play_out(stocks, &[1]) else {
            eprintln!("note: no dev store (or no {STAGE} in it); nothing checked");
            return;
        };
        assert_eq!(lone_out, Some(0), "stocks {stocks}: the lone fighter rung out, the pair wins");
        let pair_out = play_out(stocks, &[0, 2]).expect("the world");
        assert_eq!(pair_out, Some(1), "stocks {stocks}: the pair rung out, the lone fighter wins");
    }
}
