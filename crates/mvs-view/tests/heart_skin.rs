//! The heart skins' opacity scalar reaches the drawn materials (heart_skin.rs, ViewPoser::place's place_heart): on
//! the dev store, a match of each skin run right for half a second and then left standing, the scalar on every drawn
//! material of the skin's heart material read from the presented FighterPose. Without the dev store the test says so
//! and passes.

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_view::{FighterPose, ViewPoser};

const MAX: &str = "Skin BaseColor Opacity Max";

fn world(store: &mvs_store::Store, key: &str, skin: &str) -> World {
    let data = Arc::new(GameData::load(store, &[key], "trainingroomsmall").expect("the gamedata kind loads"));
    let p = MatchPlayer { character: Some(key.into()), skin: Some(skin.into()), ..MatchPlayer::default() };
    let q = MatchPlayer { character: Some(key.into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![p, q], ..MatchSetup::default() };
    World::new(data, 7, &setup).expect("binds")
}

/// Per drawn mesh of fighter 0 that carries the scalar: (mesh, slot, value).
fn scalars(f: &FighterPose) -> Vec<(String, usize, f32)> {
    let mut out = Vec::new();
    for m in &f.meshes {
        for (k, mat) in m.materials.iter().enumerate() {
            if let Some((_, v)) = mat.scalars.iter().find(|(n, _)| &**n == MAX) {
                out.push((m.mesh.to_string(), k, *v));
            }
        }
    }
    out
}

/// The values of the scalar on fighter 0's drawn materials standing (tick 50), running (tick 85) and standing again
/// (tick 200); the body's slot 0 (the heart material) included in each.
fn run(key: &str, skin: &str) -> Option<[Vec<(String, usize, f32)>; 3]> {
    let store = match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("no dev store: skipped");
            return None;
        }
    };
    let mut w = world(&store, key, skin);
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let mut seen: [Vec<(String, usize, f32)>; 3] = Default::default();
    for t in 0..=200 {
        let x = if (60..90).contains(&t) { 1.0 } else { 0.0 };
        w.step(&[InputState { x, ..InputState::default() }, InputState::default()]);
        poser.place(&w);
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        w.events.clear();
        let at = match t {
            50 => 0,
            85 => 1,
            200 => 2,
            _ => continue,
        };
        let f = fighters.iter().find(|f| f.fighter == 0).expect("fighter 0 drawn");
        seen[at] = scalars(f);
        eprintln!("{key} {skin} tick {t}: {:?}", seen[at]);
    }
    Some(seen)
}

fn check(key: &str, skin: &str) {
    let Some([stand, moving, again]) = run(key, skin) else { return };
    assert!(!stand.is_empty(), "{skin}: no drawn material carries the scalar");
    let body = |l: &[(String, usize, f32)]| l.first().map(|x| x.2);
    let (max, min) = (body(&stand).expect("standing"), body(&moving).expect("moving"));
    assert!(max > min, "{skin}: the scalar is {max} standing and {min} running");
    assert!(moving.iter().all(|x| x.2 == min), "{skin}: a material keeps another value running: {moving:?}");
    assert!(stand.iter().chain(&again).all(|x| x.2 == max), "{skin}: standing {stand:?}, again {again:?}");
}

#[test]
fn shaggys_heart_skin_hides_the_hearts_running() {
    check("shaggy", "C002_S05");
}

#[test]
fn velmas_heart_skin_hides_the_hearts_running() {
    check("velma", "C014_S05");
}
