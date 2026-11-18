//! The Batman tutorial (TutorialActor_Batman_Fundementals), which no trace records (the oracle has no Batman class):
//! its three minigames and three demos stepped on a fresh World of the tutorial level.

use std::sync::Arc;

use mvs_sim::actors::{ActorId, ActorRef, ItemId};
use mvs_sim::ai::bt::BotController;
use mvs_sim::fighter::FighterId;
use mvs_sim::input::{Dir, InputState, NO_INPUT};
use mvs_sim::modes::tutorial::advanced::batman::{BatarangMinigame, DestroyMinigame};
use mvs_sim::modes::tutorial::demo::DemoState;
use mvs_sim::modes::tutorial::minigame::MinigameState;
use mvs_sim::modes::tutorial::run;
use mvs_sim::setup::{MatchMode, MatchPlayer, MatchSetup};
use mvs_sim::world::World;

const ACTOR: &str = "TutorialActor_Batman_Fundementals";

/// A fresh World of the Batman tutorial on the tutorial level, the data from the dev store; `None` (noted) without
/// a store or its tutorial data.
fn batman() -> Option<World> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    let cast: Vec<String> = ["batman", "harley", "wonderwoman"].iter().map(|&k| k.to_owned()).collect();
    let setup = MatchSetup {
        players: vec![MatchPlayer { bot: false, team: Some(0), ..MatchPlayer::default() }],
        mode: Some(MatchMode::Tutorial { actor: ACTOR.to_owned(), cast }),
        ..MatchSetup::default()
    };
    let keys = mvs_sim_trace::keys_of(&setup);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let data = mvs_data::GameData::load(&store, &refs, "trophyroom_tutorial").ok()?;
    if !data.tutorials.contains_key(ACTOR) {
        eprintln!("note: the store has no {ACTOR}; nothing checked");
        return None;
    }
    let trees = Arc::new(data.bot_trees.clone());
    let mut w = World::new(Arc::new(data), 7, &setup).expect("binds");
    w.bot_trees = Some(trees);
    Some(w)
}

fn steps(w: &mut World, n: usize) {
    for _ in 0..n {
        w.step(&[NO_INPUT]);
    }
}

/// The World at step `step`'s concept card (its demo running), the earlier steps skipped.
fn at_step(step: i32) -> Option<World> {
    at_step_after(step, 300)
}

/// [`at_step`], `wait` frames into the card.
fn at_step_after(step: i32, wait: usize) -> Option<World> {
    let mut w = batman()?;
    steps(&mut w, 30);
    run::intro_complete(&mut w);
    while run::step_index(&w) < step {
        steps(&mut w, 90);
        run::skip_step(&mut w);
    }
    assert_eq!(run::step_index(&w), step);
    steps(&mut w, wait);
    Some(w)
}

/// The step's card started: the player has the step's pawn and the tasks' minigames.
fn started(w: &mut World) {
    run::start_pressed(w);
    steps(w, 30);
}

fn managed(w: &World) -> FighterId {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.managed).expect("a managed pawn")
}

fn counter(w: &World, step: usize, task: usize) -> i32 {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).map_or(0, |a| a.counters[step][task])
}

fn batarangs(w: &World) -> Vec<ItemId> {
    let cls = w.names.get("Item_BatarangV2_C").expect("the class name");
    w.actors.items.iter().filter(|(_, i)| !i.dead && i.cls == cls).map(|(id, _)| id).collect()
}

fn demo_pad_y(w: &World) -> Option<f32> {
    w.tutorial.as_ref()?.demos.last().map(|d| d.pad.y)
}

fn demo_state(w: &World) -> Option<&DemoState> {
    w.tutorial.as_ref()?.demos.last().map(|d| &d.state)
}

fn batarang_minigame(w: &World) -> Option<&BatarangMinigame> {
    let a = w.tutorial.as_ref()?.actor.as_ref()?;
    a.minigames.iter().find_map(|m| match &m.state {
        MinigameState::BatmanBatarang(s) => Some(s),
        _ => None,
    })
}

fn destroy_minigame(w: &World) -> Option<&DestroyMinigame> {
    let a = w.tutorial.as_ref()?.actor.as_ref()?;
    a.minigames.iter().find_map(|m| match &m.state {
        MinigameState::BatmanDestroy(s) => Some(s),
        _ => None,
    })
}

fn press(attack: bool, x: f32, y: f32) -> InputState {
    InputState { attack, x, y, atk_dir: Some(Dir::Neutral), ..NO_INPUT }
}

/// The player's neutral attack pressed for a frame, then up to `n` frames for a thrown batarang: its id.
fn throw(w: &mut World, n: usize) -> Option<ItemId> {
    let before = batarangs(w);
    w.step(&[press(true, 0.0, 0.0)]);
    for _ in 0..n {
        if let Some(&it) = batarangs(w).iter().find(|it| !before.contains(it)) {
            return Some(it);
        }
        w.step(&[NO_INPUT]);
    }
    None
}

/// The item put at (x, y), its last place there as well.
fn put_item(w: &mut World, it: ItemId, x: f32, y: f32) {
    let i = w.actors.items.get_mut(it).expect("a live item");
    (i.x, i.y, i.prev_x, i.prev_y) = (x, y, x, y);
}

/// The place of a live target's HurtBox centre (its place, 60 above).
fn target_at(w: &World, t: ActorId) -> Option<(f32, f32)> {
    let (x, y, _) = mvs_sim::actors::world_actor::actor_place(w, ActorRef::Actor(t))?;
    Some((x, y + 120.0))
}

#[test]
fn the_batarang_demo_throws_every_three_seconds() {
    let Some(mut w) = at_step(0) else { return };
    assert!(matches!(demo_state(&w), Some(DemoState::BatmanBatarang(_))), "step 0's demo controller");
    let mut thrown = 0;
    let mut seen = Vec::new();
    for _ in 0..400 {
        w.step(&[NO_INPUT]);
        for it in batarangs(&w) {
            if !seen.contains(&it) {
                seen.push(it);
                thrown += 1;
            }
        }
    }
    assert!(thrown >= 2, "the demo pawn throws a batarang every 3 s: {thrown} in 400 frames");
}

#[test]
fn the_batarang_minigame_cuts_the_moveset_and_six_target_hits_finish_the_step() {
    let Some(mut w) = at_step(0) else { return };
    started(&mut w);
    let p = managed(&w);
    let t = &w.fighter(p).tutorial;
    assert_eq!(t.disabled_input_types[0], 1023 & !(1 << 4), "the normal attack: neutral only");
    assert_eq!((t.disabled_input_types[1], t.disabled_input_types[3]), (1023, 1023), "special and evade cut");
    assert_eq!(t.move_disabled, 1, "movement cut");
    let s = batarang_minigame(&w).expect("the Batarang minigame");
    assert_eq!((s.stage, s.targets.len()), (0, 1), "TargetPractice0: one target");
    // A special press does nothing: no batarang, no move.
    let x0 = w.fighter(p).body.x;
    for _ in 0..30 {
        w.step(&[InputState { special: true, x: 1.0, ..NO_INPUT }]);
        w.step(&[NO_INPUT]);
    }
    assert_eq!(w.fighter(p).body.x, x0, "no walk, no special move");
    let mut hits = 0;
    let mut stages = Vec::new();
    for _ in 0..40 {
        if run::step_index(&w) != 0 {
            break;
        }
        let Some(s) = batarang_minigame(&w) else { break };
        stages.push(s.stage);
        let live: Vec<ActorId> = s.targets.iter().copied().filter(|t| !s.hit_targets.contains(t)).collect();
        let Some(&t) = live.first() else {
            steps(&mut w, 70);
            continue;
        };
        steps(&mut w, 20);
        let Some(it) = throw(&mut w, 20) else { panic!("no batarang thrown") };
        let at = place_of(&w, p);
        let Some((tx, ty)) = target_at(&w, t) else { continue };
        put_item(&mut w, it, tx, ty);
        w.step(&[NO_INPUT]);
        assert!(w.actors.items.get(it).is_none_or(|i| i.dead), "the hitting batarang is destroyed");
        assert_eq!(place_of(&w, p), at, "the throw put the fighter back where he started");
        hits += 1;
        steps(&mut w, 70);
    }
    assert_eq!(hits, 6, "six targets over the four stages: {stages:?}");
    assert!(stages.contains(&3), "stage 3 reached");
    steps(&mut w, 200);
    assert_eq!(run::step_index(&w), 1, "task 0.0 (72 x 6) done: step 1");
}

#[test]
fn the_wonder_woman_bag_of_stage_3_takes_no_damage() {
    let Some(mut w) = at_step(0) else { return };
    started(&mut w);
    for _ in 0..40 {
        let Some(s) = batarang_minigame(&w) else { break };
        if let Some(b) = s.bag {
            assert!(w.fighter(b).invuln.damage_disabled, "DamageDisabled");
            return;
        }
        let Some(&t) = s.targets.iter().find(|t| !s.hit_targets.contains(t)) else {
            steps(&mut w, 70);
            continue;
        };
        steps(&mut w, 20);
        let Some(it) = throw(&mut w, 20) else { panic!("no batarang thrown") };
        let Some((tx, ty)) = target_at(&w, t) else { continue };
        put_item(&mut w, it, tx, ty);
        steps(&mut w, 90);
    }
    panic!("stage 3 not reached");
}

fn place_of(w: &World, f: FighterId) -> (f32, f32) {
    (w.fighter(f).body.x, mvs_sim::fighter::actor_y(w, f))
}

#[test]
fn the_retrieve_steps_count_bag_hits_and_idle_pickups() {
    let Some(mut w) = at_step(1) else { return };
    assert!(matches!(demo_state(&w), Some(DemoState::BatmanRetrieve(_))), "step 1's demo controller");
    let req: Vec<i32> = run::actor_data(&w).expect("the data").steps[1].tasks.iter().map(|t| t.req).collect();
    assert_eq!(req.len(), 2, "two tasks: 72 and 73");
    started(&mut w);
    let p = managed(&w);
    assert_eq!(w.fighter(p).tutorial.disabled_input_types[1], 1023, "special cut");
    assert_eq!(w.fighter(p).tutorial.move_disabled, 0, "movement kept");
    let bag = w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.bag).expect("the Harley bag");
    // (the RecoverOnly bag walks off this level, which has no waypoint for it: held still here)
    run::possess_bot(&mut w, bag, BotController::Idle);
    // 72: a thrown batarang on the Harley bag.
    for n in 1..=req[0] {
        let it = throw_again(&mut w).expect("a batarang");
        let at = place_of(&w, bag);
        put_item(&mut w, it, at.0, at.1 + 120.0);
        steps(&mut w, 2);
        assert_eq!(counter(&w, 1, 0), n, "a batarang hit on the bag");
    }
    // 73: a batarang steered down to lie idle, walked over.
    for _ in 0..req[1] {
        if counter(&w, 1, 1) >= req[1] {
            break;
        }
        let before = counter(&w, 1, 1);
        let it = throw_again(&mut w).expect("a batarang");
        for k in 0..600 {
            let Some(i) = w.actors.items.get(it).filter(|i| !i.dead) else { break };
            let dir = if i.x > w.fighter(p).body.x { 1.0 } else { -1.0 };
            let input = if k < 90 { press(false, 0.0, -1.0) } else { press(false, dir, 0.0) };
            w.step(&[input]);
        }
        assert!(counter(&w, 1, 1) > before, "an idle batarang walked over counts");
    }
    assert_eq!(counter(&w, 1, 1), req[1]);
    steps(&mut w, 300);
    assert_eq!(run::step_index(&w), 2, "both tasks done: step 2");
}

/// The neutral attack pressed every 30 frames (the batarang back from its last throw) until a new batarang flies.
fn throw_again(w: &mut World) -> Option<ItemId> {
    (0..40).find_map(|_| throw(w, 30))
}

#[test]
fn the_retrieve_demo_steers_its_batarang_down_and_walks_to_it() {
    let Some(mut w) = at_step_after(1, 0) else { return };
    let (mut down, mut walked, mut back) = (false, false, false);
    let (mut x0, mut f) = (None, None);
    for _ in 0..900 {
        w.step(&[NO_INPUT]);
        if f.is_none()
            && let Some(DemoState::BatmanRetrieve(s)) = demo_state(&w)
        {
            f = s.fighter;
            x0 = f.map(|f| w.fighter(f).body.x);
        }
        let (Some(f), Some(x0)) = (f, x0) else { continue };
        down |= demo_pad_y(&w) == Some(-1.0) && w.fighter(f).controls.inp.y == -1.0;
        walked |= w.fighter(f).body.x > x0 + 100.0;
        back |= walked && w.fighter(f).body.x == x0;
    }
    assert!(f.is_some(), "step 1's demo controller has its pawn");
    assert!(down, "SetMoveAxisY(-1.0) on the thrown batarang");
    assert!(walked, "SetMoveAxisX(1.0): the pawn walks to it");
    assert!(back, "teleported back after the batarang is gone");
}

/// The bag's first batarang hits Harley and her answer starts (On Harley Hit): her pad's (1, -1), which her DI and
/// fast fall read. Her knockback still carries her off the left edge here, so the demo ends at her respawn (the row
/// on it); her answer's end (the teleport back, CurrentDamage 0) is not reached.
#[test]
fn the_destroy_demo_bag_throws_and_harley_answers() {
    let Some(mut w) = at_step_after(2, 0) else { return };
    let (mut thrown, mut hurt, mut answered, mut reset, mut ids) = (false, false, false, false, None);
    for _ in 0..300 {
        w.step(&[NO_INPUT]);
        let Some(DemoState::BatmanDestroy(s)) = demo_state(&w) else { continue };
        let (Some(f), Some(bag)) = (s.fighter, s.bag) else { continue };
        ids = Some((f, bag));
        thrown |= !batarangs(&w).is_empty();
        hurt |= w.fighter(f).stats.damage > 0.0;
        let pad = w.tutorial.as_ref().and_then(|t| t.demos.last()).map(|d| d.pad.x);
        answered |= hurt && demo_pad_y(&w) == Some(-1.0) && pad == Some(1.0);
        reset |= answered && w.fighter(f).stats.damage == 0.0;
    }
    let (_, bag) = ids.expect("the demo's Harley and Batman bag");
    assert!(w.fighter(bag).abilities.disable_cd, "the bag's DisableCD");
    assert!(thrown, "the bag throws a batarang 0.5 s in");
    assert!(hurt, "the batarang hits Harley");
    assert!(answered, "On Harley Hit: SetMoveAxisY(-1.0), SetMoveAxisX(1.0)");
    assert!(!reset, "(the row: she rings out before her answer ends; this fails once she stays on)");
}

#[test]
fn the_destroy_minigame_counts_harleys_hits_on_idle_batarangs() {
    let Some(mut w) = at_step(2) else { return };
    assert!(matches!(demo_state(&w), Some(DemoState::BatmanDestroy(_))), "step 2's demo controller");
    started(&mut w);
    let p = managed(&w);
    let s = destroy_minigame(&w).expect("the Destroy minigame");
    let bag = s.bag.expect("the Batman bag");
    assert!(w.fighter(bag).invuln.damage_disabled && w.fighter(bag).abilities.disable_cd);
    let mut counted = 0;
    for _ in 0..12 {
        // a batarang the bag threw, at rest
        let mut rest = None;
        for _ in 0..400 {
            w.step(&[NO_INPUT]);
            rest = batarangs(&w).into_iter().find(|&it| w.actors.items.get(it).is_some_and(|i| !i.flying));
            if rest.is_some() {
                break;
            }
        }
        let Some(it) = rest else { panic!("no batarang came to rest") };
        let (ix, iy) = {
            let i = w.actors.items.get(it).expect("a live item");
            (i.x, i.y)
        };
        mvs_sim::world::spawn_spot::fighter_teleport_to(&mut w, p, ix - 60.0, iy);
        steps(&mut w, 2);
        w.step(&[press(true, 0.0, 0.0)]);
        steps(&mut w, 40);
        counted = counter(&w, 2, 0);
        if counted >= 3 || run::step_index(&w) != 2 {
            break;
        }
    }
    assert!(counted >= 3 || run::step_index(&w) == 3, "three hits on batarangs: {counted}");
    assert!(destroy_minigame(&w).is_none_or(|s| s.vo_done), "the bag's VO 140 once");
}

#[test]
fn step_3_has_no_task_and_its_skip_ends_the_tutorial() {
    let Some(mut w) = at_step(3) else { return };
    started(&mut w);
    steps(&mut w, 300);
    assert_eq!(run::step_index(&w), 3, "no task: the step stays");
    assert!(!run::over(&w));
    run::advance(&mut w);
    steps(&mut w, 600);
    assert!(run::over(&w), "the card's skip (AdvanceTutorial) ends the tutorial");
}
