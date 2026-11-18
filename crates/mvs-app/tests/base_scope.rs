//! The base scope's event list against the oracle's two `base` scope lines of `hud.tape` (the first the code's
//! posts, the second with the HUD bundle's names), parsed. Skips with a printed line while the tape or the store is
//! missing (`MVS_AUDIO_TAPES` names the tapes' directory).

use std::path::PathBuf;
use std::sync::Arc;

use mvs_app::audio::{Index, base_events, hud_named};
use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend};

fn tapes() -> PathBuf {
    std::env::var_os("MVS_AUDIO_TAPES")
        .map_or_else(|| mvs_sim_trace::run::default_traces().join("../audio-tapes"), PathBuf::from)
}

#[test]
fn base_scope_equals_the_page_tapes() {
    let path = tapes().join("hud.tape");
    let Ok(tape) = mvs_audio::tape::read::<Line>(&path) else {
        eprintln!("note: no tape {}; nothing checked", path.display());
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let a = Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio");
    let known = Index::new(a.init().events.keys().cloned());
    let page: Vec<&Vec<String>> = tape
        .iter()
        .filter_map(|l| match &l.call {
            Call::Scope { name, keep, .. } if name == "base" => Some(keep),
            _ => None,
        })
        .collect();
    assert_eq!(page.len(), 2, "the oracle tape's base scope lines");
    // The port's list comes from the Rust sources' posts (src/audio/posted.rs, the `posted` test), the oracle's from
    // the oracle's list: an event only the oracle holds is a post the port does not make (printed), one only the port
    // holds is a
    // post only the port makes; the lists must not have the latter.
    let hud = hud_named(a.store(), &known);
    for (mine, theirs, what) in
        [(base_events(&known, &[]), page[0], "codeBase"), (base_events(&known, &hud), page[1], "with the HUD's names")]
    {
        let only_port: Vec<_> = mine.iter().filter(|e| !theirs.contains(e)).collect();
        let only_page: Vec<_> = theirs.iter().filter(|e| !mine.contains(e)).collect();
        eprintln!("{what}: only the oracle holds {only_page:?}");
        assert!(only_port.is_empty(), "{what}: only the port holds {only_port:?}");
    }
}
