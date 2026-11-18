//! The pre-match music (menu_audio.rs `MenuMusic::level_switch`, the PreMatch level's SetupCharSelect
//! PF_Post_Global_Switch(mus-mus_menu_chaselect) @18136) on the dev store and the offline mixer: after the title's and
//! the main menu's music, the level's switch is heard and finds every medium it needs. Skips with a printed line while
//! the store is missing.

use std::sync::Arc;

use mvs_app::menu_audio::{MenuAudio, MenuMusic};
use mvs_audio::{Audio, Backend, Note, QUANTUM, RATE};
use mvs_ui::menu::MenuSink;

/// `secs` of the mix at the menus' 60 frames a second: the output's peak and the warnings.
fn run(audio: &mut Option<Audio>, m: &mut MenuMusic, secs: f64) -> (f32, Vec<Note>) {
    let a = audio.as_mut().unwrap();
    let quanta = (secs * f64::from(RATE) / QUANTUM as f64) as usize;
    let mut peak = 0f32;
    let mut warns = Vec::new();
    for q in 0..quanta {
        let tick = m.tick + ((q * QUANTUM) as f64 * 60.0 / f64::from(RATE)) as u32;
        a.pump(tick, QUANTUM);
        peak = a.take_output().iter().fold(peak, |p, s| p.max(s.abs()));
        warns.extend(a.take_notes().into_iter().filter(|n| matches!(n, Note::Warn { .. })));
    }
    m.tick += (secs * 60.0) as u32;
    (peak, warns)
}

#[test]
fn the_prematch_level_switches_to_its_music() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let mut audio = Some(Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio"));
    if !audio.as_ref().unwrap().has("mus_play") {
        eprintln!("note: the store has no mus_play; nothing checked");
        return;
    }
    let mut m = MenuMusic::default();
    m.preload(&mut audio);
    m.title(&mut audio);
    run(&mut audio, &mut m, 1.0);
    m.menu(&mut audio);
    m.play_mus_ost(&mut audio);
    let (_, warns) = run(&mut audio, &mut m, 2.0);
    assert!(warns.is_empty(), "the main menu's music: {warns:?}");
    m.level_switch(&mut audio, "mus_menu_chaselect");
    let (peak, warns) = run(&mut audio, &mut m, 6.0);
    assert!(warns.is_empty(), "mus_menu_chaselect: {warns:?}");
    assert!(peak > 1e-3, "mus_menu_chaselect is silent (peak {peak})");
    // (a second visit holds nothing again)
    m.level_switch(&mut audio, "mus_menu_chaselect");
    let (_, warns) = run(&mut audio, &mut m, 1.0);
    assert!(warns.is_empty(), "the second switch: {warns:?}");
    // (the switch without its media held is heard missing: what the check above rests on)
    MenuAudio { audio: &mut audio, tick: m.tick, window: None, equip: None }
        .set_switch("mus", "mus_menu_chaselect_ranked");
    let (_, warns) = run(&mut audio, &mut m, 6.0);
    assert!(!warns.is_empty(), "a switch to media not held warns");
}

/// A rematch's pre-match after a match: the match's end stops every voice (MatchAudio::end's StopAll) but not the
/// music, so StartMusic is not asked again and the PreMatch level's switch is heard.
#[test]
fn the_prematch_music_after_a_match_end() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let mut audio = Some(Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio"));
    let mut m = MenuMusic::default();
    m.preload(&mut audio);
    m.menu(&mut audio);
    m.play_mus_ost(&mut audio);
    run(&mut audio, &mut m, 1.0);
    m.level_switch(&mut audio, "mus_menu_chaselect");
    run(&mut audio, &mut m, 2.0);
    let stop = mvs_audio::tape::post::Call::StopAll { fade: 0.1, keep: None };
    audio.as_mut().unwrap().call(&mvs_audio::tape::post::Line { tick: m.tick, call: stop }).unwrap();
    run(&mut audio, &mut m, 0.5);
    let (peak, _) = run(&mut audio, &mut m, 2.5);
    // (the music's clips are the music engine's voices, not the posts StopAll stops: MenuMusic keeps playing)
    assert!(peak > 1e-3, "the music plays on through the stop (peak {peak})");
    m.level_switch(&mut audio, "mus_menu_chaselect");
    let (peak, warns) = run(&mut audio, &mut m, 6.0);
    assert!(warns.is_empty(), "{warns:?}");
    assert!(peak > 1e-3, "the rematch's mus_menu_chaselect is silent (peak {peak})");
}
