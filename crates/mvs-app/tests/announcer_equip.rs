//! The Collection's announcer equip line (menu_audio.rs `AnnouncerEquip`) on
//! the dev store's packs and the offline mixer: each pack's line is posted through the menus' sink and is heard, a
//! second equip while it speaks is dropped, and the scope is let go a second after its end. Skips with a printed
//! line while the store or its pack list is missing.

use std::sync::Arc;

use mvs_app::audio::{Index, base_events, hud_named};
use mvs_app::menu_audio::{AnnouncerEquip, MenuAudio};
use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, Note, QUANTUM, RATE};
use mvs_ui::menu::MenuSink;

#[test]
fn every_pack_speaks_its_equip_line() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let Ok(pack) = store.kind(mvs_data::KIND) else {
        eprintln!("note: no gamedata kind; nothing checked");
        return;
    };
    if !pack.has(mvs_data::path::ANNOUNCER_PACKS) {
        eprintln!("note: the store holds no announcer pack list; nothing checked");
        return;
    }
    let packs: mvs_data::AnnouncerPacks = mvs_data::entry(&pack, mvs_data::path::ANNOUNCER_PACKS).unwrap();
    let store = Arc::new(store);
    // (the program's base scope, as at its start, and the output resumed)
    let mut audio = Some(Audio::new(store.clone(), Backend::Offline).expect("the offline audio"));
    {
        let a = audio.as_mut().unwrap();
        let known = Index::new(a.init().events.keys().cloned());
        let hud = hud_named(a.store(), &known);
        let base = Call::Scope { name: "base".into(), keep: base_events(&known, &hud), lazy: Vec::new() };
        a.call(&Line { tick: 0, call: base }).expect("the base scope");
        a.call(&Line { tick: 0, call: Call::Resume }).expect("the resume");
    }
    for p in &packs.packs {
        let mut equip = AnnouncerEquip::default();
        // (the chooser's press: the sink leaves the pack for the frame's end)
        let mut sink = MenuAudio { audio: &mut audio, tick: 0, window: None, equip: Some(&mut equip) };
        sink.announcer_equipped(&p.path);
        let length =
            |ev: &str| audio.as_ref().and_then(|a| a.init().events.get(ev).and_then(|e| e.dur[1])).unwrap_or(0.0);
        let calls = equip.take_calls(&store, length, 0.0);
        let ev = match &calls[..] {
            [Call::Scope { name, keep, .. }, Call::Post { ev, owner: Some(-1), .. }] => {
                assert_eq!(name, "announcerEquip");
                assert_eq!(keep, std::slice::from_ref(ev));
                ev.clone()
            }
            other => panic!("{}: {other:?}", p.path),
        };
        let len = length(&ev);
        assert!(len > 0.5, "{}: {ev} lasts {len} s", p.path);
        let a = audio.as_mut().unwrap();
        for c in calls {
            a.call(&Line { tick: 0, call: c }).unwrap();
        }
        // a second of the mix: the line is heard, and nothing it needs is missing
        for _ in 0..(RATE as usize / QUANTUM) {
            a.pump(0, QUANTUM);
        }
        let peak = a.take_output().iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 1e-3, "{}: {ev} is silent (peak {peak})", p.path);
        let warns: Vec<_> = a.take_notes().into_iter().filter(|n| matches!(n, Note::Warn { .. })).collect();
        assert!(warns.is_empty(), "{}: {ev}: {warns:?}", p.path);
        // another equip while it speaks: no line; the scope let go a second after its end
        equip.ask(&p.path);
        assert!(equip.take_calls(&store, |_| 1.0, len * 0.5).is_empty(), "{}", p.path);
        assert!(equip.take_calls(&store, |_| 1.0, len + 0.5).is_empty(), "{}", p.path);
        let calls = equip.take_calls(&store, |_| 1.0, len + 1.0);
        assert!(
            matches!(&calls[..], [Call::Scope { name, keep, lazy }] if name == "announcerEquip" && keep.is_empty() && lazy.is_empty()),
            "{}: {calls:?}",
            p.path
        );
    }
}
