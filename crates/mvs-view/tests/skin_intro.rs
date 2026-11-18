//! The skins' own match intros (skin_intro.rs) on the dev store: which skins have one, and a match of four such skins
//! drawn through the intro with the intros' sounds. Without the dev store each test says so and passes.

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup, TeamStyle};
use mvs_view::skin_intro::IntroMute;
use mvs_view::{FighterPose, ViewPoser, skins};

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

#[test]
fn the_skins_with_an_intro_of_their_own() {
    let Some(store) = store() else { return };
    let mut found = Vec::new();
    for key in &mvs_trace::record::CharRecord::VARIANTS[1..] {
        let Ok(data) = GameData::load(&store, &[key], "trainingroomsmall") else { continue };
        let Some(cd) = data.chars.get(*key) else { continue };
        let d = &cd.fighter;
        for name in d.skins.iter().flat_map(|m| m.keys()) {
            let skin = skins::skin_of(d, Some(name)).expect("the skin");
            if IntroMute::of(d, &skin).is_some() {
                let own = skin.intro.as_deref().unwrap_or("");
                let look = d.skins.as_ref().and_then(|m| m.get(name));
                let m = look.and_then(|l| l.montages.as_ref()).and_then(|m| m.get(own)).or_else(|| d.montages.get(own));
                let classes: Vec<&str> = m.iter().flat_map(|m| &m.notifies).map(|n| n.cls.as_str()).collect();
                found.push(format!("{key} {name} {own} {classes:?}"));
            }
        }
    }
    eprintln!("{}", found.join("\n"));
    assert!(found.iter().any(|s| s.starts_with("velma C014_S03 ")), "Velma's witch skin has an intro of its own");
    // (Jake_S01's is Pawn_Jake's: no IntroMontageOverride, IntroAnimation set by the pawn's graph) with its clip
    assert!(
        found.iter().any(|s| s.starts_with("jake Jake_S01 S01_Emote_Spawn_Montage ")),
        "Jake_S01's intro is S01_Emote_Spawn_Montage"
    );
    let data = GameData::load(&store, &["jake"], "trainingroomsmall").expect("the gamedata");
    assert!(data.chars["jake"].fighter.paths.clips.contains_key("S01_Emote_Spawn"), "its clip among Jake's");
}

/// Velma's witch skin, Morty's C019_S01 (its intro's SetSubmeshVisibility state), Gizmo's C023A_S01 and Stripe's
/// C023B_S03 (their intros' sounds) through the intro of a free-for-all.
#[test]
fn four_skin_intros_draw_and_sound() {
    let Some(store) = store() else { return };
    let picks = [("velma", "C014_S03"), ("morty", "C019_S01"), ("gizmo", "C023A_S01"), ("stripe", "C023B_S03")];
    let keys: Vec<&str> = picks.iter().map(|p| p.0).collect();
    let data = Arc::new(GameData::load(&store, &keys, "trainingroomsmall").expect("the gamedata"));
    let players = picks
        .iter()
        .map(|(k, s)| MatchPlayer { character: Some((*k).into()), skin: Some((*s).into()), ..MatchPlayer::default() })
        .collect();
    let setup = MatchSetup { team_style: TeamStyle::FreeForAll, players, flow: true, ..MatchSetup::default() };
    let mut w = World::new(data, 1, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let (mut sounds, mut heard) = (Vec::new(), Vec::new());
    for t in 0..600 {
        w.step(&[InputState::default(); 4]);
        poser.place(&w);
        poser.intro_sounds(&mut sounds);
        heard.extend(sounds.drain(..).map(|(id, ev)| (t, id, ev)));
        poser.present(&w, 1.0, &mut fighters, &mut actors);
        for f in &fighters {
            assert!(
                f.meshes.iter().all(|m| m.to_world.iter().chain(m.bones.iter().flatten()).all(|v| v.is_finite())),
                "tick {t}"
            );
        }
        w.events.clear();
    }
    for (t, id, ev) in &heard {
        eprintln!("t{t} f{id} {ev}");
    }
    // (each of the two intros' Ak_OneShot and Ak_SurfaceEffect notifies once, on its own fighter)
    for id in [2, 3] {
        assert_eq!(heard.iter().filter(|h| h.1 == id).count(), 6, "fighter {id}'s intro sounds");
    }
    assert!(heard.iter().all(|h| h.1 >= 2), "no intro sounds on the skins without any");
}
