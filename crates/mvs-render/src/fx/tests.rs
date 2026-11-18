//! Fx over a Shaggy match on the dev store: the step's events start their systems, the sandwich carries its
//! component, a ring-out's effect spawns. Without the dev store each test says so and passes.

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::{Event, World};
use mvs_view::{FighterPose, ViewPoser};

use super::Fx;

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

/// The lab-picture trace match as a Lab set-up (no rules: the poser draws a
/// match with rules only after its start): World seed 1, Shaggy 0's down special (the sandwich) at tick 480.
fn lab_world(store: &mvs_store::Store) -> World {
    let data = Arc::new(GameData::load(store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![player.clone(), player],
        rules: None,
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    World::new(data, 1, &setup).expect("binds")
}

fn inputs(t: i32) -> InputState {
    if t == 480 { InputState { y: -1.0, special: true, ..InputState::default() } } else { InputState::default() }
}

#[test]
fn the_lab_sandwich_carries_its_effects() {
    let Some(store) = store() else { return };
    let mut w = lab_world(&store);
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let (mut moves, mut with_projectile, mut carried) = (0, 0, 0);
    for t in 0..600 {
        w.step(&[inputs(t), InputState::default()]);
        poser.place(&w);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        fx.step(&w, &fighters, &[]).expect("the step");
        moves += w.events.iter().filter(|e| matches!(e, Event::MoveFx { .. })).count();
        if w.actors.projectiles.iter().any(|(_, p)| !p.dead) {
            with_projectile += 1;
            if fx.projectiles.values().any(|p| p.h.is_some_and(|h| fx.player.alive(h))) {
                carried += 1;
            }
        }
        w.events.clear();
    }
    eprintln!("move fx {moves}, ticks with a projectile {with_projectile} ({carried} carried)");
    assert!(fx.pawns.values().all(|p| p.body.as_ref().is_some_and(|b| !b.bones.is_empty())));
    // a hand socket sits within reach of the mesh origin, its rows a rotation
    for id in [0, 1] {
        let f = fx.socket_follow(&w, id, Some("R_Hand"), [0.0; 3], &mvs_vfx::math::IDENTITY, [1.0; 3]);
        let a = fx.place(&w, &f).expect("a place");
        let o = fx.pawns[&id].body.as_ref().expect("a body").loc;
        let d = ((a.loc[0] - o[0]).powi(2) + (a.loc[1] - o[1]).powi(2) + (a.loc[2] - o[2]).powi(2)).sqrt();
        assert!(d > 10.0 && d < 300.0, "R_Hand of {id} at {d} cm from the mesh");
        let r = a.rot;
        let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
            + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
        assert!((det - 1.0).abs() < 1e-3, "R_Hand rows of {id}: det {det}");
    }
    assert!(with_projectile > 0, "the down special throws the sandwich");
    assert_eq!(with_projectile, carried, "every live projectile carries its component");
}
#[test]
fn a_ring_out_spawns_its_entry() {
    let Some(store) = store() else { return };
    let mut w = lab_world(&store);
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    w.step(&[InputState::default(), InputState::default()]);
    poser.place(&w);
    poser.present(&w, 1.0, &mut fighters, &mut actors);
    w.events.clear();
    let ko = Event::KoVfx {
        fighter: 1,
        asset: w.lit("ROV_Default"),
        hold: 0.01,
        p: [-3000.0, 200.0],
        tl: [2500.0, 1500.0],
        tr: [-2500.0, 1500.0],
        bl: [2500.0, -800.0],
        center: [0.0, 300.0],
    };
    w.events.push(ko);
    fx.step(&w, &fighters, &[]).expect("the step");
    assert_eq!(fx.ko_anchors.len(), 1);
    assert!(fx.ko_anchors[0].hold <= 1.0 / 60.0, "held one tick at most (ROV_Default)");
}

/// Every stage's Emitter actors spawn on the particle player, with the stage's
/// surfaces, and live after a tick: none has a module the player has no port for (begin_level would skip it).
#[test]
fn level_emitters_spawn() {
    let Some(store) = store() else { return };
    let keys: Vec<String> = store
        .kind("stagedraw")
        .expect("the stagedraw kind")
        .paths()
        .filter_map(|p| p.strip_prefix("/StageDraw/")?.split('.').next().map(str::to_string))
        .collect();
    let (mut bad, mut skipped, mut live) = (Vec::new(), Vec::new(), 0);
    for key in &keys {
        let emitters = crate::scenery::level_emitters(&store, key).expect("the scenery entry");
        let surfaces = super::surfaces::level_surfaces(&store, key).expect("the stage data");
        for e in &emitters {
            let mut fx = Fx::new(&store, 1).expect("the particles kind");
            if let Some(m) = fx.player.unported(&e.system) {
                skipped.push(format!("{key} {} ({m})", e.system));
            }
            fx.begin_level(std::slice::from_ref(e), surfaces.clone()).expect("spawned");
            fx.player.tick(1.0 / 60.0);
            match (fx.level_live(), skipped.last().is_some_and(|s| s.contains(&e.system))) {
                (1, false) => live += 1,
                (0, true) => {}
                (n, _) => bad.push(format!("{key} {}: {n} live", e.system)),
            }
        }
    }
    eprintln!("{live} level emitters live, skipped: {skipped:?}");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(skipped.is_empty(), "level systems not spawned: {skipped:?}");
}

/// A Lab fighter change keeps the level: the Emitter actors' systems and those a
/// scenery block spawned on the level (spawn_level) go on into the next match's effects on the same player, its
/// one-shot systems end, and a system the next match spawns takes no id a block holds.
#[test]
fn a_kept_level_keeps_its_systems_ids() {
    let Some(store) = store() else { return };
    // (the first stage with Emitter actors)
    let (key, emitters) = store
        .kind("stagedraw")
        .expect("the stagedraw kind")
        .paths()
        .filter_map(|p| p.strip_prefix("/StageDraw/")?.split('.').next().map(str::to_string))
        .map(|k| {
            let e = crate::scenery::level_emitters(&store, &k).expect("the scenery entry");
            (k, e)
        })
        .find(|(_, e)| !e.is_empty())
        .expect("a stage with Emitter actors");
    let key = key.as_str();
    let system = emitters[0].system.clone();
    let mut fx = Fx::new(&store, 1).expect("the particles kind");
    fx.begin_level(&emitters, super::surfaces::level_surfaces(&store, key).expect("the stage data")).expect("spawned");
    let at =
        mvs_vfx::Attachment { loc: [0.0; 3], rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], scale: None };
    let block = fx.player.spawn_level(&system, &at, false, &[], 0, false).expect("a block's system");
    let one_shot = fx.player.spawn(&system, [0.0; 3], [0.0; 3], [1.0; 3], &[]).expect("a match's system");
    fx.player.tick(1.0 / 60.0);
    let level = fx.level_live();
    assert_eq!(level, emitters.len());
    let kept = fx.keep_level(&store).expect("the placeholder");
    assert_eq!(fx.level_live(), 0, "the match left holds no level system");
    let mut next = Fx::resume_level(&store, 2, kept).expect("the next match's effects");
    assert_eq!(next.level_live(), level);
    assert!(next.player.alive(block), "the block's id stays");
    assert!(!next.player.alive(one_shot), "the match's systems ended");
    let fresh: Vec<_> = (0..4).filter_map(|_| next.player.spawn(&system, [0.0; 3], [0.0; 3], [1.0; 3], &[])).collect();
    assert_eq!(fresh.len(), 4);
    assert!(!fresh.contains(&block) && !fresh.contains(&one_shot), "a new system aliases no old id");
    next.player.kill(fresh[0]);
    assert!(next.player.alive(block));
}

/// A heart skin's petals (shared/heart_skin.rs): Velma in C014_S05 standing has her two hand systems, and a walk
/// ends them; the Witch skin's sparkles (C014_S03) are on her while she lives.
#[test]
fn heart_petals_and_witch_sparkles_draw() {
    let Some(store) = store() else { return };
    let data = Arc::new(GameData::load(&store, &["velma"], "trainingroomsmall").expect("the gamedata kind loads"));
    let skin =
        |s: &str| MatchPlayer { character: Some("velma".into()), skin: Some(s.into()), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![skin("C014_S05"), skin("C014_S03")],
        rules: None,
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data, 1, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let (mut standing, mut walking, mut witch) = (0, 0, false);
    for t in 0..520 {
        let walk = t >= 500;
        let x = if walk { 1.0 } else { 0.0 };
        w.step(&[InputState { x, ..InputState::default() }, InputState::default()]);
        poser.place(&w);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        fx.step(&w, &fighters, &[]).expect("the step");
        w.events.clear();
        let live = fx.chars.iter().find_map(|c| match c {
            super::chars::CharFx::HeartSkin(h) => Some(h.live(&fx.player)),
            _ => None,
        });
        if t == 150 {
            standing = live.expect("the module");
            witch = fx.chars.iter().any(|c| matches!(c, super::chars::CharFx::Velma(v) if v.witch_live(1, &fx.player)));
        }
        if t == 519 {
            walking = live.expect("the module");
        }
    }
    assert!(fighters.iter().find(|f| f.fighter == 0).is_some_and(|f| f.heart), "the hearts skin is a heart pawn");
    assert!(fighters.iter().find(|f| f.fighter == 1).is_some_and(|f| !f.heart), "the witch skin is not");
    assert_eq!(standing, 2, "the two hand petals while standing");
    assert_eq!(walking, 0, "none while moving");
    assert!(witch, "the witch skin's sparkles");
}

/// Velma's SlowtoStunGrenade decal goes with the grenade when its place moves (a moving platform carries it).
#[test]
fn the_grenade_decal_follows_the_grenade() {
    use mvs_sim::actors::world_actor::WorldActorState;
    use mvs_sim::chars::velma::gadgets::spawn_grenade;
    let Some(store) = store() else { return };
    let data = Arc::new(GameData::load(&store, &["velma"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("velma".into()), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![player.clone(), player],
        rules: None,
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data, 1, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let mut step = |w: &mut World, fx: &mut Fx| {
        w.step(&[InputState::default(), InputState::default()]);
        poser.place(w);
        poser.present(w, 1.0, &mut fighters, &mut actors);
        fx.step(w, &fighters, &[]).expect("the step");
        w.events.clear();
    };
    for _ in 0..10 {
        step(&mut w, &mut fx);
    }
    let v = w.players[0];
    let (x, y) = (w.fighter(v).body.x, w.fighter(v).body.y);
    spawn_grenade(&mut w, v, x, y);
    step(&mut w, &mut fx);
    let decal = |fx: &Fx| {
        fx.chars.iter().find_map(|c| match c {
            super::chars::CharFx::Velma(v) => v.grenade_decal_loc(),
            _ => None,
        })
    };
    let Some(before) = decal(&fx) else {
        eprintln!("no grenade decal (no Decal component in the store's class data): skipped");
        return;
    };
    let ids: Vec<_> = w.actors.actors.ids().collect();
    for id in ids {
        if let Some(WorldActorState::SlowToStunGrenade(g)) = w.actors.actors.get_mut(id).map(|a| &mut a.state) {
            g.x += 100.0;
            g.y += 30.0;
        }
    }
    step(&mut w, &mut fx);
    let after = decal(&fx).expect("the decal");
    assert!((after[1] - before[1] + 100.0).abs() < 1e-3, "UE Y is the sim's -x: {before:?} -> {after:?}");
    assert!((after[2] - before[2] - 30.0).abs() < 1e-3, "UE Z the sim's y: {before:?} -> {after:?}");
}

/// The Iron Giant S04 skin's three electricity components are drawn on him; another skin's stay hidden (none made).
#[test]
fn iron_giant_s04_electricity_draws() {
    let Some(store) = store() else { return };
    let data = Arc::new(GameData::load(&store, &["irongiant"], "trainingroomsmall").expect("the gamedata kind loads"));
    let skin =
        |s: &str| MatchPlayer { character: Some("irongiant".into()), skin: Some(s.into()), ..MatchPlayer::default() };
    let setup = MatchSetup {
        players: vec![skin("C017_S04"), skin("C017_S01")],
        rules: None,
        flow: true,
        allow_hazards: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(data, 1, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let mut fx = Fx::new(&store, 1).expect("the player");
    if ["PS_C017_Electricity_Shoulders", "PS_C017_Electricity_Head"]
        .iter()
        .any(|ps| fx.player.emitter_count(ps).is_none())
    {
        eprintln!("no electricity template in the store: skipped");
        return;
    }
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    for _ in 0..30 {
        w.step(&[InputState::default(), InputState::default()]);
        poser.place(&w);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        fx.step(&w, &fighters, &[]).expect("the step");
        w.events.clear();
    }
    let live = |id| {
        fx.chars.iter().find_map(|c| match c {
            super::chars::CharFx::IronGiant(g) => Some(g.electricity_live(id, &fx.player)),
            _ => None,
        })
    };
    assert_eq!(live(0), Some(3), "the S04 Giant's shoulders and head");
    assert_eq!(live(1), Some(0), "the S01 Giant's none");
}
