//! The views' posing on the dev store: the reference pose skins to identity, a clip's sample through the rig is
//! mvs-anim's in the view's space, and a Shaggy match poses tick after tick. Without the dev store each test says so
//! and passes.

use std::sync::Arc;

use mvs_anim::clip::AnimClip;
use mvs_anim::montage::PROXY_NONE;
use mvs_anim::pose::BonePose;
use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_view::anim::{Clips, Pose, Rig, sample_clip};
use mvs_view::math;
use mvs_view::mesh::{MeshInfo, SkeletonEntry};
use mvs_view::skins;
use mvs_view::view::CharView;
use mvs_view::{FighterPose, ViewPoser};

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

fn shaggy_world(store: &mvs_store::Store) -> World {
    let data = Arc::new(GameData::load(store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], ..MatchSetup::default() };
    World::new(data, 7, &setup).expect("binds")
}

fn body_and_clips(store: &mvs_store::Store, w: &World) -> (MeshInfo, Clips) {
    let cd = &w.data.chars["shaggy"];
    let skin = skins::skin_of(&cd.fighter, None).expect("the default skin");
    let body = MeshInfo::load(store, &skin.mesh).expect("the main mesh");
    let clips =
        Clips::load(store, cd.fighter.paths.clips.iter().map(|(k, v)| (k.as_str(), v.as_str()))).expect("clips");
    (body, clips)
}

#[test]
fn the_reference_pose_skins_to_identity() {
    let Some(store) = store() else { return };
    let w = shaggy_world(&store);
    let (body, clips) = body_and_clips(&store, &w);
    let rig = Rig::bind(&clips, body.bones().to_vec());
    let view = CharView::new(&body, rig, 1.0, 0.0);
    let mut cs = Vec::new();
    view.bones_cs(&mut cs);
    assert_eq!(cs.len(), body.inv_ref.len());
    let mut worst = 0.0f64;
    for (k, m) in cs.iter().enumerate() {
        let skin = math::mul(&math::to_ue(m), &body.inv_ref[k]);
        for (i, v) in skin.iter().enumerate() {
            let id = if i % 5 == 0 { 1.0 } else { 0.0 };
            // (the translation column in cm, the 3x3 unitless)
            worst = worst.max((v - id).abs());
        }
    }
    eprintln!("largest difference from identity: {worst:e}");
    assert!(worst < 1e-3, "the reference pose's skinning is identity within 0.001 ({worst})");
}

#[test]
fn a_clip_sample_through_the_rig_is_mvs_anims() {
    let Some(store) = store() else { return };
    let w = shaggy_world(&store);
    let (body, mut clips) = body_and_clips(&store, &w);
    let names = body.bones().to_vec();
    let rig = Rig::bind(&clips, names.clone());
    let c = clips.clips.iter().position(|c| !c.additive).expect("a full clip");
    let path = clips.clips[c].anim.path.clone();
    let entry: SkeletonEntry =
        serde_json::from_slice(&store.kind("skeleton").unwrap().bytes(&clips.clips[c].anim.skeleton).unwrap()).unwrap();
    let bytes = store.kind("animation").unwrap().bytes(&path).unwrap();
    let mut own = AnimClip::new(&bytes, &path).unwrap();
    let n = entry.bones.len().max(own.bone_end);
    let mut ref_ = BonePose::new(n);
    for i in 0..entry.bones.len() {
        ref_.t[i * 3..i * 3 + 3].copy_from_slice(&entry.pose[i * 10..i * 10 + 3]);
        ref_.r[i * 4..i * 4 + 4].copy_from_slice(&entry.pose[i * 10 + 3..i * 10 + 7]);
        ref_.s[i * 3..i * 3 + 3].copy_from_slice(&entry.pose[i * 10 + 7..i * 10 + 10]);
    }
    for i in entry.bones.len()..n {
        ref_.r[i * 4 + 3] = 1.0;
        ref_.s[i * 3..i * 3 + 3].fill(1.0);
    }
    let mut out = BonePose::new(n);
    let tracked: Vec<usize> = (0..own.tracks).map(|i| own.track_bone(i) as usize).collect();
    let mut pose = Pose::new(names.len());
    let mut checked = 0;
    for time in [0.0, 0.1, 0.25] {
        own.sample(time, &ref_, &mut out, false, false);
        sample_clip(&mut clips, c, &rig, &body.rest, time, &mut pose, -1);
        for (i, name) in names.iter().enumerate() {
            let Some(s) = entry.bone_index(name).filter(|s| tracked.contains(s)) else { continue };
            let cm = |v: f32| (f64::from(v) * f64::from(0.01f32)) as f32;
            assert_eq!(
                pose.t[i * 3..i * 3 + 3],
                [cm(out.t[s * 3]), cm(out.t[s * 3 + 2]), cm(out.t[s * 3 + 1])],
                "{name} t at {time}"
            );
            assert_eq!(
                pose.r[i * 4..i * 4 + 4],
                [out.r[s * 4], out.r[s * 4 + 2], out.r[s * 4 + 1], -out.r[s * 4 + 3]],
                "{name} r at {time}"
            );
            checked += 1;
        }
    }
    eprintln!("{path}: {checked} bone samples checked");
    assert!(checked > 0);
}

#[test]
fn shaggys_sections_are_his_slots() {
    let Some(store) = store() else { return };
    let w = shaggy_world(&store);
    let cd = &w.data.chars["shaggy"];
    let skin = skins::skin_of(&cd.fighter, None).unwrap();
    let paths = std::iter::once(skin.mesh.clone()).chain(skin.parts.iter().filter_map(|p| p.path.clone()));
    for p in paths {
        let m = MeshInfo::load(&store, &p).unwrap();
        let slots: Vec<usize> = m.sections.iter().map(|s| s.slot).collect();
        assert_eq!(slots, (0..m.slots.len()).collect::<Vec<_>>(), "{p}: a section per slot, in order");
    }
}

/// Player 0's inputs at tick `t`: a walk, attacks, specials and a jump.
fn inputs(t: i32) -> InputState {
    InputState {
        x: if (60..90).contains(&t) { 1.0 } else { 0.0 },
        attack: t % 40 == 20,
        special: t % 97 == 50,
        jump: t == 150,
        ..InputState::default()
    }
}

#[test]
fn two_steps_between_frames_pose_as_one_step_frames() {
    let Some(store) = store() else { return };
    let (mut wa, mut wb) = (shaggy_world(&store), shaggy_world(&store));
    let mut a = ViewPoser::new(&store, &wa).expect("the views");
    let mut b = ViewPoser::new(&store, &wb).expect("the views");
    let (mut fa, mut fb, mut actors) = (Vec::<FighterPose>::new(), Vec::<FighterPose>::new(), Vec::new());
    let mut compared = 0;
    for t in 0..300 {
        for (w, p) in [(&mut wa, &mut a), (&mut wb, &mut b)] {
            w.step(&[inputs(t), InputState::default()]);
            p.place(w);
            w.events.clear();
        }
        // a presents every tick, b every second tick
        a.present(&wa, 0.5, &mut fa, &mut actors);
        if t % 2 == 1 {
            b.present(&wb, 0.5, &mut fb, &mut actors);
            assert_eq!(fa, fb, "tick {t}");
            compared += usize::from(!fa.is_empty());
        }
    }
    assert!(compared > 0, "no fighter drawn");
}

#[test]
fn a_shaggy_match_poses_each_tick() {
    let Some(store) = store() else { return };
    let mut w = shaggy_world(&store);
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    for t in 0..300 {
        w.step(&[inputs(t), InputState::default()]);
        poser.place(&w);
        for a in [0.25f32, 1.0] {
            poser.present(&w, a, &mut fighters, &mut actors);
            assert_eq!(fighters.len(), 2, "tick {t}");
            for f in &fighters {
                let body = &f.meshes[0];
                assert!(!body.bones.is_empty());
                assert!(
                    f.meshes.iter().all(|m| m.to_world.iter().chain(m.bones.iter().flatten()).all(|v| v.is_finite())),
                    "tick {t}"
                );
            }
        }
        w.events.clear();
    }
    eprintln!("{} meshes drawn for fighter 0", fighters[0].meshes.len());
}

/// A big match's eight fighters each take their place in the priority stack (a body past the build's four once
/// kept no index, or a stale one).
#[test]
fn eight_fighters_each_take_a_priority_index() {
    let Some(store) = store() else { return };
    let data = Arc::new(GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player; 8], ..MatchSetup::default() };
    let mut w = World::new(data, 7, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    // (setupMaterials runs once its one-second Delay is over)
    for _ in 0..90 {
        w.step(&[InputState::default(); 8]);
        poser.place(&w);
        w.events.clear();
    }
    poser.present(&w, 1.0, &mut fighters, &mut actors);
    assert_eq!(fighters.len(), 8);
    let mut at: Vec<u8> = fighters.iter().map(|f| f.priority.expect("an index in the stack")).collect();
    at.sort_unstable();
    assert_eq!(at, (0..8).collect::<Vec<u8>>());
}

/// Evil Morty (C019_S02) poses through his EvilMorty slot (chars/morty.rs EvilMortyPose; its view once panicked at the
/// match's load): the skin's Axe_Swing2 sequence loads, and the Trident part (C019_S02_Weapon_SkelMesh) is drawn only
/// inside the track's SetSubmeshVisibility window.
#[test]
fn evil_morty_poses_through_his_slot() {
    let Some(store) = store() else { return };
    let data = Arc::new(GameData::load(&store, &["morty"], "trainingroomsmall").expect("the gamedata kind loads"));
    let m = &data.chars["morty"].fighter.montages["C019_Attack_Air_Axe_Swing2_Montage"];
    if m.skin_slot_notifies.is_empty() {
        eprintln!("the store keeps no EvilMorty notifies (an older import): skipped");
        return;
    }
    assert!(data.chars["morty"].fighter.paths.clips.contains_key("C019_S02_Attack_Air_Axe_Swing2"), "the S02 clip");
    let p =
        |skin: &str| MatchPlayer { character: Some("morty".into()), skin: Some(skin.into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![p("C019_S02"), p("")], ..MatchSetup::default() };
    let mut w = World::new(data, 7, &setup).expect("binds");
    let mut poser = ViewPoser::new(&store, &w).expect("the views");
    let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
    let me = w.pawns.as_ref().unwrap_or(&w.players)[0];
    let mut trident = |w: &mut World, t: i32| -> bool {
        w.step(&[InputState::default(), InputState::default()]);
        poser.place(w);
        poser.present(w, 1.0, &mut fighters, &mut actors);
        assert_eq!(fighters.len(), 2, "tick {t}");
        w.events.clear();
        fighters[0].meshes.iter().any(|m| m.mesh.contains("C019_S02_Weapon"))
    };
    for t in 0..30 {
        assert!(!trident(&mut w, t), "no Trident before the swing, tick {t}");
    }
    let swing = w.lit("C019_Attack_Air_Axe_Swing2_Montage");
    start_montage(&mut w, me, swing, &PROXY_NONE, None);
    let shown: Vec<bool> = (30..60).map(|t| trident(&mut w, t)).collect();
    assert!(shown.iter().any(|&s| s), "the Trident in the swing's window");
    for t in 60..200 {
        trident(&mut w, t);
    }
    assert!(!trident(&mut w, 200), "no Trident once the swing is over");
}

/// Jake the Starchild (Jake_S02) draws his own eat sequences (chars/jake.rs): in an eat hold the OriginsEatOverride
/// machine's Jake_S02_Eat_Attack_* clips, and in the eat montages their Origins tracks', so his pose differs
/// from the one a store without those clips gives (the default machine's and DefaultSlot's clips standing in), and is
/// the same outside them. Skipped on a store whose Jake clips lack them (an older import).
#[test]
fn starchild_eats_with_his_own_clips() {
    let Some(store) = store() else { return };
    let data = GameData::load(&store, &["jake"], "trainingroomsmall").expect("the gamedata kind loads");
    if !data.chars["jake"].fighter.paths.clips.contains_key("Jake_S02_Eat_Attack_Idle") {
        eprintln!("the store's Jake clips lack the Jake_S02 eat sequences (an older import): skipped");
        return;
    }
    let mut bare = data.clone();
    let cd = Arc::make_mut(bare.chars.get_mut("jake").expect("jake"));
    cd.fighter.paths.clips.retain(|k, _| !k.starts_with("Jake_S02_Eat_Attack_"));
    let p =
        |skin: &str| MatchPlayer { character: Some("jake".into()), skin: Some(skin.into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![p("Jake_S02"), p("")], ..MatchSetup::default() };
    let mut worlds = [Arc::new(data), Arc::new(bare)].map(|d| World::new(d, 7, &setup).expect("binds"));
    let mut posers = worlds.each_ref().map(|w| ViewPoser::new(&store, w).expect("the views"));
    let me = worlds[0].pawns.as_ref().unwrap_or(&worlds[0].players)[0];
    // fighter 0's main mesh bones in each world after one tick (`eat`: EatOverrideBool on through the tick)
    let mut step = |worlds: &mut [World; 2], eat: bool| -> [Vec<[f32; 12]>; 2] {
        [0, 1].map(|k| {
            let w = &mut worlds[k];
            mvs_sim::chars::jake::state::jake_w(w, me).anim.eat_override = eat;
            w.step(&[InputState::default(), InputState::default()]);
            let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
            posers[k].place(w);
            posers[k].present(w, 1.0, &mut fighters, &mut actors);
            w.events.clear();
            let body = fighters[0].meshes.iter().find(|m| m.mesh.contains("C005_S02_SkelMesh")).expect("the S02 body");
            body.bones.clone()
        })
    };
    for t in 0..30 {
        let [a, b] = step(&mut worlds, false);
        assert_eq!(a, b, "the same pose before the eat, tick {t}");
    }
    let held: Vec<bool> = (30..60).map(|_| step(&mut worlds, true)).map(|[a, b]| a != b).collect();
    assert!(held[5..].iter().all(|&d| d), "the eat hold draws the OriginsEatOverride clips: {held:?}");
    for _ in 60..120 {
        step(&mut worlds, false);
    }
    let [a, b] = step(&mut worlds, false);
    assert_eq!(a, b, "the same pose once the hold is over");
    // the OriginsSkin track, the OriginsSpit* tracks, the OriginsSpit track
    for name in ["Jake_Eat_Attack_Montage", "Jake_Eat_Attack_Spit_Montage", "Jake_Eat_Attack_Spit_Ally_Montage"] {
        for w in &mut worlds {
            let m = w.lit(name);
            start_montage(w, me, m, &PROXY_NONE, None);
        }
        let shown: Vec<bool> = (0..20).map(|_| step(&mut worlds, false)).map(|[a, b]| a != b).collect();
        assert!(shown[5..].iter().all(|&d| d), "{name} draws its Origins tracks: {shown:?}");
        for _ in 0..150 {
            step(&mut worlds, false);
        }
        let [a, b] = step(&mut worlds, false);
        assert_eq!(a, b, "the same pose once {name} is over");
    }
}
