//! The bot matches' plumbing in the harness (`world_for`, `bots_for`): a match with a bot player (or whose spec asks
//! for them) has the bot trees on its World; a match without has none, and no bot controller. Reads main's traces
//! and the dev store; a tree without either says so and passes.

use mvs_sim_trace::{bots_for, run, world_for};
use mvs_trace::Trace;

fn header_of(name: &str) -> Option<(Trace, mvs_store::Store)> {
    let path = run::default_traces().join(format!("{name}.trace"));
    let trace = Trace::read(&path).ok()?;
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    Some((trace, store))
}

#[test]
fn a_bot_free_match_has_no_trees_and_no_bots() {
    let Some((t, store)) = header_of("m5-combat") else {
        eprintln!("note: no m5-combat trace or store; nothing checked");
        return;
    };
    let (w, _) = world_for(&t.header, &store).expect("the World");
    assert!(w.bot_trees.is_none());
    assert!(bots_for(&t.header.spec, &w).iter().all(Option::is_none));
}

#[test]
fn a_minion_match_has_the_trees_and_no_bots() {
    let Some((t, store)) = header_of("steven-melon-combat") else {
        eprintln!("note: no steven-melon-combat trace or store; nothing checked");
        return;
    };
    let (w, _) = world_for(&t.header, &store).expect("the World");
    assert!(w.bot_trees.is_some());
    assert!(bots_for(&t.header.spec, &w).iter().all(Option::is_none));
}

#[test]
fn a_bot_match_has_the_trees() {
    let Some((t, store)) = header_of("bot-lvl0") else {
        eprintln!("note: no bot-lvl0 trace or store; nothing checked");
        return;
    };
    let (w, _) = world_for(&t.header, &store).expect("the World");
    assert!(w.bot_trees.is_some());
    // (the controller itself is made by BtBot::new: its tree runner's reset is another track's, ai/bt.rs)
}
