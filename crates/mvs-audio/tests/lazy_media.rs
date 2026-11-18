//! A scope's lazy events against the developer's store (`mvs_store::dev_store_dir`): a post of one decodes its media
//! and plays them, where the scope's load held only its banks. A run without a store says so and passes.

use std::sync::Arc;

use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, Note, QUANTUM};

/// The system voice lines of a character: lazy in a match's scope (mvs-app audio/scopes.rs lazy_vo).
fn lazy_lines(a: &Audio) -> Vec<String> {
    let mut v: Vec<String> =
        a.init().events.keys().filter(|e| e.starts_with("vo_shaggy_sys_")).take(3).cloned().collect();
    v.sort();
    v
}

#[test]
fn a_lazy_post_plays() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no store: nothing checked");
        return;
    };
    let mut a = Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio");
    let lines = lazy_lines(&a);
    assert!(!lines.is_empty(), "the store has no vo_shaggy_sys_ event");
    let call = |tick, call| Line { tick, call };
    a.call(&call(0, Call::Scope { name: "match".into(), keep: Vec::new(), lazy: lines.clone() })).unwrap();
    for (i, ev) in lines.iter().enumerate() {
        let tick = 1 + 60 * u32::try_from(i).unwrap();
        a.call(&call(tick, Call::Post { ev: ev.clone(), owner: Some(-1), key: None, stop_key: None, at: None }))
            .unwrap();
        a.pump(tick, 48 * QUANTUM);
    }
    let out = a.take_output();
    let notes = a.take_notes();
    let missing: Vec<u64> = notes
        .iter()
        .filter_map(|n| match n {
            Note::Warn { what, id } if what.contains("medium") || what.contains("late") => Some(*id),
            _ => None,
        })
        .collect();
    assert!(missing.is_empty(), "media not played: {missing:?} ({notes:?})");
    assert!(out.iter().any(|s| s.abs() > 1e-4), "the lazy lines made no sound");
}
