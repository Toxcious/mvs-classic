//! The port's own big matches (more than four players): a Versus 3v3 and 4v4 and an eight-player Free For All of bots,
//! on m010 and the small training room, each at a start of its own (`world::starts`), played to their
//! end; the Free For All's teams one per player; the ally lists' order (`fighter::common::allies`); the starts clear of each other on the stages whose own extra starts stand on others. On the dev store;
//! a tree without one says so and passes.

use mvs_sim::ai::bt::BtBot;
use mvs_sim::fighter::common::{first_ally, last_ally};
use mvs_sim::setup::{MatchPlayer, MatchSetup, Rules, TeamStyle, player_team, team_count};
use mvs_sim::world::World;
use mvs_sim::world::starts::{MIN_START_GAP, big_match_starts_ok, sorted_index};
use std::sync::Arc;

const STAGES: [&str; 2] = ["m010", "trainingroomsmall"];
/// Characters whose moves read their allies (Bugs' dispenser, Black Adam's bolt, Velma's and the Creature's homing,
/// Shaggy's ally range, Taz's ally in front, Steven's shared shield, Jake's swallow), then four more.
const CAST: [&str; 8] = ["bugs", "blackadam", "velma", "creature", "shaggy", "taz", "steven", "jake"];

fn world(stage: &str, style: TeamStyle, n: usize) -> Option<(World, Arc<mvs_data::BotTrees>, MatchSetup)> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let cast = &CAST[..n];
    let data = Arc::new(mvs_data::GameData::load(&store, cast, stage).ok()?);
    let player = |c: &str| MatchPlayer {
        bot: true,
        difficulty: Some(3.0),
        character: Some(c.to_owned()),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: style,
        players: cast.iter().map(|c| player(c)).collect(),
        rules: Some(Rules { knockouts_to_win: 4.0, duration: 90.0, stocks: false }),
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data.clone(), 57, &setup).ok()?;
    let trees = Arc::new(data.bot_trees.clone());
    w.bot_trees = Some(trees.clone());
    Some((w, trees, setup))
}

/// Plays the match with Hard bots to its end; panics when it does not end in `limit` ticks.
fn play(mut w: World, trees: &Arc<mvs_data::BotTrees>, limit: u32) -> World {
    let n = w.players.len();
    let mut bots: Vec<BtBot> = (0..n).map(|i| BtBot::match_bot(70 + i32::try_from(i).unwrap(), trees, 3)).collect();
    for _ in 0..limit {
        let players = w.players.clone();
        let ins: Vec<_> = bots.iter_mut().zip(players).map(|(b, f)| b.think(&mut w, f)).collect();
        w.step(&ins);
        w.events.clear();
        if w.flow.match_over {
            return w;
        }
    }
    panic!("the match did not end in {limit} ticks");
}

fn check(stage: &str, style: TeamStyle, n: usize) {
    let Some((w, trees, setup)) = world(stage, style, n) else {
        eprintln!("note: no dev store (or no {stage} in it); nothing checked");
        return;
    };
    starts_apart(&w, &setup, stage, style, n);
    let teams: Vec<u8> = (0..n).map(|i| player_team(&setup, i)).collect();
    if style == TeamStyle::FreeForAll {
        assert_eq!(usize::from(team_count(&setup)), n);
        assert_eq!(teams, (0..n).map(|i| u8::try_from(i).unwrap()).collect::<Vec<_>>(), "a team per player");
    }
    let w = play(w, &trees, 90 * 60 + 3000);
    let hurt = w.players.iter().filter(|&&f| w.fighter(f).stats.damage > 0.0).count();
    assert!(hurt > 0, "{stage} {style:?} {n}: nobody was hit");
}

/// Every extra start and respawn of the match (team-sorted index 4 on) stands [`MIN_START_GAP`] or more from the
/// others of its kind (the stage's own four may stand nearer each other: TreeHouse's Respawn1 and 3 are 75 apart).
fn starts_apart(w: &World, setup: &MatchSetup, stage: &str, style: TeamStyle, n: usize) {
    let teams: Vec<i32> = (0..n).map(|i| i32::from(player_team(setup, i))).collect();
    let sorted: Vec<usize> = (0..n).map(|i| sorted_index(setup, &teams, i)).collect();
    assert!(big_match_starts_ok(&w.data.stage, style), "{stage}: a start of a big {style:?} match has no place");
    for (kind, list) in [("spawn", &w.stage.spawns), ("respawn", &w.stage.respawns)] {
        assert_eq!(list.len(), n);
        for i in 0..n {
            for j in (i + 1..n).filter(|&j| sorted[i].max(sorted[j]) >= 4) {
                let (a, b) = (list[i], list[j]);
                let d = (f64::from(a.x) - f64::from(b.x)).hypot(f64::from(a.y) - f64::from(b.y));
                assert!(d >= MIN_START_GAP, "{stage} {style:?} {n}: {kind}s of players {i} and {j} are {d} apart");
            }
        }
    }
}

/// The stages whose own Player5..8 stand on (or by) another start: Batcave's Player5 on Player3, TreeHouse's Player7
/// on Player3, TreeHouseSmall's Player7 / 8 on Player5 / 6, M009 small's Player5 80 from Player1.
#[test]
fn the_starts_of_a_big_match_stand_apart() {
    for stage in ["batcave", "treehouse", "treehousesmall", "m009_small"] {
        for (style, n) in [(TeamStyle::TwoVsTwo, 6), (TeamStyle::TwoVsTwo, 8), (TeamStyle::FreeForAll, 8)] {
            let Some((w, _, setup)) = world(stage, style, n) else {
                eprintln!("note: no dev store (or no {stage} in it); nothing checked");
                return;
            };
            starts_apart(&w, &setup, stage, style, n);
        }
    }
}

#[test]
fn a_3v3_plays_to_its_end() {
    for stage in STAGES {
        check(stage, TeamStyle::TwoVsTwo, 6);
    }
}

#[test]
fn a_4v4_plays_to_its_end() {
    for stage in STAGES {
        check(stage, TeamStyle::TwoVsTwo, 8);
    }
}

#[test]
fn an_eight_player_free_for_all_plays_to_its_end() {
    for stage in STAGES {
        check(stage, TeamStyle::FreeForAll, 8);
    }
}

#[test]
fn a_respawned_ally_comes_last_in_the_ally_lists() {
    let Some((mut w, ..)) = world(STAGES[0], TeamStyle::TwoVsTwo, 6) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    // (team 0: slots 0, 2, 4)
    let p = w.players.clone();
    let ends = |w: &World, f| (first_ally(w, f), last_ally(w, f));
    assert_eq!(ends(&w, p[0]), (Some(p[2]), Some(p[4])), "the first fill in player order");
    assert_eq!(ends(&w, p[4]), (Some(p[0]), Some(p[2])));
    let idle = vec![mvs_sim::input::InputState::default(); 6];
    // Slot 2 knocked out: out of the lists until its respawn, then last.
    w.fighter_mut(p[2]).life.dead = 3.0;
    assert_eq!(ends(&w, p[0]), (Some(p[4]), Some(p[4])), "a knocked-out ally leaves the list");
    for _ in 0..3 {
        w.step(&idle);
        w.events.clear();
    }
    assert_eq!(w.fighter(p[2]).life.dead, 0.0, "respawned");
    assert_eq!(ends(&w, p[0]), (Some(p[4]), Some(p[2])), "the respawned ally comes last");
    assert_eq!(ends(&w, p[4]), (Some(p[0]), Some(p[2])));
}

/// Slot 0's buffs (class and stacks, sorted) at the start of a 3v3 whose team 0 (slots 0, 2, 4) has the perk on the
/// slots `with`.
fn perk_buffs(with: &[usize]) -> Option<Vec<(String, mvs_sim::Real)>> {
    const PERK: &str = "Perk_General_AirControl_Small";
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let cast = &CAST[..6];
    let data = Arc::new(mvs_data::GameData::load(&store, cast, STAGES[0]).ok()?);
    let setup = MatchSetup {
        team_style: TeamStyle::TwoVsTwo,
        players: cast
            .iter()
            .enumerate()
            .map(|(i, c)| MatchPlayer {
                bot: true,
                character: Some((*c).to_owned()),
                perks: if with.contains(&i) { vec![PERK.to_owned()] } else { Vec::new() },
                ..MatchPlayer::default()
            })
            .collect(),
        rules: Some(Rules { knockouts_to_win: 4.0, duration: 90.0, stocks: false }),
        ..MatchSetup::default()
    };
    let w = World::new(data, 57, &setup).ok()?;
    let f = w.players[0];
    let mut out: Vec<(String, mvs_sim::Real)> =
        w.fighter(f).buffs.buffs.iter().map(|b| (w.names.str(b.cls).to_owned(), b.stacks)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Some(out)
}

#[test]
fn a_perk_counts_twice_at_most_from_a_team() {
    let Some(none) = perk_buffs(&[]) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let one = perk_buffs(&[0]).expect("the world");
    let two = perk_buffs(&[0, 2]).expect("the world");
    let three = perk_buffs(&[0, 2, 4]).expect("the world");
    assert_ne!(none, one, "the perk gives slot 0 a buff");
    assert_ne!(one, two, "a teammate's copy stacks");
    assert_eq!(two, three, "a third copy on the team gives nothing more");
}

/// Every stage of the store holds every big match a lobby can make in both styles (`big_match_starts_ok` is the
/// lobby's filter: a safety net that leaves no stage out today): Free For All of 5 to 8, every 2v2 split of up to four
/// a team (3v2, 4v1, 3v3, 4v2, 4v3, 4v4). A stage that fails is named with the set and the first start that finds no
/// place. A stage without the style's four starts plays no match of that style at all (the tutorial's room: 1v1
/// starts only) and is listed apart.
#[test]
fn every_stage_holds_a_big_match() {
    use mvs_sim::world::starts::{StartKind, big_match_indexes, match_starts};
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no dev store; nothing checked");
        return;
    };
    let stages = mvs_data::stage_levels(&store).expect("the stage list");
    let (mut failed, mut no_style) = (Vec::new(), Vec::new());
    for (_, key) in &stages {
        let st: Arc<mvs_data::StageEntry> =
            mvs_data::shared(&store, &mvs_data::path::stage(key)).expect("the stage's data");
        for style in [TeamStyle::TwoVsTwo, TeamStyle::FreeForAll] {
            let tag = mvs_sim::setup::START_TAGS[style as usize];
            let four: Vec<usize> = (0..4).collect();
            if match_starts(&st.data, StartKind::Spawn, tag, &four).is_none()
                || match_starts(&st.data, StartKind::Respawn, tag, &four).is_none()
            {
                assert!(!big_match_starts_ok(&st.data, style));
                no_style.push(format!("{key} {style:?}"));
                continue;
            }
            let before = failed.len();
            for ns in big_match_indexes(style) {
                for kind in [StartKind::Spawn, StartKind::Respawn] {
                    if match_starts(&st.data, kind, tag, &ns).is_some() {
                        continue;
                    }
                    let first = (1..=ns.len()).find(|&k| match_starts(&st.data, kind, tag, &ns[..k]).is_none());
                    failed.push(format!(
                        "{key} {style:?} {ns:?} {kind:?}: index {:?} has no place",
                        first.map(|k| ns[k - 1])
                    ));
                }
            }
            assert_eq!(big_match_starts_ok(&st.data, style), failed.len() == before, "{key} {style:?}");
        }
    }
    eprintln!("big match starts: {} stages judged; without the style's own four: {no_style:?}", stages.len());
    assert!(failed.is_empty(), "stages that do not hold a big match:\n{}", failed.join("\n"));
}
