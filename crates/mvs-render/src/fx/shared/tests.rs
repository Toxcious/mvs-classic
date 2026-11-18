//! The buff-driven shared modules over a Lab match on the dev store (`CONTENT_STORE`): the arcade bosses' aura and
//! eyes (an arcade round's setup, whose data holds their classes), Educated's end burst (Velma's data holds it).
//! Without the dev store, or without a system's template in it, each test says so and passes.

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::combat::buffs::spawn_buff;
use mvs_sim::input::InputState;
use mvs_sim::setup::{ArcadeRound, MatchPlayer, MatchSetup};
use mvs_view::{FighterPose, ViewPoser};

use crate::fx::Fx;

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

/// Two fighters of `char` in the small training room, no rules (fx/tests.rs lab_world); `arcade` the round's setup
/// (the players human: no round buff of its own).
fn lab_world(store: &mvs_store::Store, char: &str, arcade: Option<ArcadeRound>) -> World {
    let data = Arc::new(GameData::load(store, &[char], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some(char.into()), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![player.clone(), player],
        arcade,
        rules: None,
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    World::new(data, 1, &setup).expect("binds")
}

/// Steps `w` and the effects `n` ticks with no input.
fn run(w: &mut World, poser: &mut ViewPoser, fx: &mut Fx, n: usize) {
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    for _ in 0..n {
        w.step(&[InputState::default(), InputState::default()]);
        poser.place(w);
        poser.present(w, 1.0, &mut fighters, &mut actors);
        fx.step(w, &fighters, &[]).expect("the step");
        w.events.clear();
    }
}

/// Whether the store holds every one of `systems`' templates; a skip line when not.
fn templates(fx: &mut Fx, systems: &[&str]) -> bool {
    let missing: Vec<&str> = systems.iter().copied().filter(|ps| fx.player.emitter_count(ps).is_none()).collect();
    if !missing.is_empty() {
        eprintln!("no template for {missing:?} in the store: skipped");
    }
    missing.is_empty()
}

/// The live systems named `ps` on fighter `id`'s mesh.
fn on_mesh(fx: &Fx, id: i32, ps: &str) -> usize {
    fx.pawns[&id].mesh_psc.iter().filter(|(n, _, h)| n == ps && fx.player.alive(*h)).count()
}

#[test]
fn arcade_bosses_draw_their_aura_and_eyes() {
    let Some(store) = store() else { return };
    let mut w = lab_world(&store, "shaggy", Some(ArcadeRound { difficulty: 3, stage: 4 }));
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    if !templates(&mut fx, &["PS_ShaggyRageAuraBuff_Loop", "PS_RageEyes"]) {
        return;
    }
    run(&mut w, &mut poser, &mut fx, 10);
    let (a, b) = (w.players[0], w.players[1]);
    let (mid, nemesis) = (w.lit("Buff_Arcade_MidBoss_C"), w.lit("Buff_AXBotEnemyHardNemesis_C"));
    spawn_buff(&mut w, a, mid, 0.0, Some(1), Some(a), true, None).expect("the mid boss buff");
    spawn_buff(&mut w, b, nemesis, 0.0, Some(1), Some(b), true, None).expect("the nemesis buff");
    run(&mut w, &mut poser, &mut fx, 1);
    assert_eq!(on_mesh(&fx, 0, "PS_ShaggyRageAuraBuff_Loop"), 1, "the aura at once");
    assert_eq!(on_mesh(&fx, 1, "PS_RageEyes"), 0, "no eyes before the delay");
    run(&mut w, &mut poser, &mut fx, 30);
    assert_eq!(on_mesh(&fx, 1, "PS_RageEyes"), 2, "an eye a socket after the delay");
    run(&mut w, &mut poser, &mut fx, 60);
    assert_eq!(on_mesh(&fx, 0, "PS_ShaggyRageAuraBuff_Loop"), 1, "one aura, kept");
    assert_eq!(on_mesh(&fx, 1, "PS_RageEyes"), 2, "the eyes made once");
}

#[test]
fn educated_bursts_at_its_end() {
    let Some(store) = store() else { return };
    let mut w = lab_world(&store, "velma", None);
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    if !templates(&mut fx, &["PS_WonderWomanBracers_Cancel"]) {
        return;
    }
    run(&mut w, &mut poser, &mut fx, 10);
    let a = w.players[0];
    let educated = w.lit("Buff_Educated_C");
    spawn_buff(&mut w, a, educated, 0.25, Some(1), Some(a), true, None).expect("the buff");
    run(&mut w, &mut poser, &mut fx, 2);
    assert_eq!(on_mesh(&fx, 0, "PS_WonderWomanBracers_Cancel"), 0, "nothing while it is on");
    let mut seen = 0;
    for _ in 0..40 {
        run(&mut w, &mut poser, &mut fx, 1);
        seen = seen.max(on_mesh(&fx, 0, "PS_WonderWomanBracers_Cancel"));
    }
    assert_eq!(seen, 1, "one burst at its end");
}
