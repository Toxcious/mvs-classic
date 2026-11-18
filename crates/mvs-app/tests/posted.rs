//! `src/audio/posted.rs` against the Rust sources: the sound events the posting crates (mvs-app, mvs-audio,
//! mvs-render, mvs-sim, mvs-ui) name by a string literal holding an event name alone, each with the areas whose code
//! names it. An area is `menu` for the menus' code (mvs-ui menu/, mvs-app menus / menu_audio / online menus /
//! launcher, mvs-render menuscene), else the crate's name without `mvs-`. Comment lines, `#[cfg(test)]` modules to the
//! end of the file and posted.rs itself are skipped. The event names are the store's InitBank's (the dev store,
//! `CONTENT_STORE`); with no store it says so and passes. Fails when posted.rs is stale; `MVS_WRITE_POSTED=1` writes it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mvs_audio::{Audio, Backend};

const CRATES: [&str; 5] = ["mvs-app", "mvs-audio", "mvs-render", "mvs-sim", "mvs-ui"];

/// The `.rs` files under `dir`, recursively.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut ents: Vec<_> =
        std::fs::read_dir(dir).expect("a source folder reads").map(|e| e.expect("an entry").path()).collect();
    ents.sort();
    for p in &ents {
        if p.is_file() && p.extension().is_some_and(|x| x == "rs") {
            out.push(p.clone());
        }
    }
    for p in &ents {
        if p.is_dir() {
            rs_files(p, out);
        }
    }
}

fn area_of(krate: &str, rel: &str) -> String {
    let own = krate.trim_start_matches("mvs-").to_owned();
    let menu = match krate {
        "mvs-ui" => rel.starts_with("menu/"),
        "mvs-render" => rel.starts_with("menuscene/"),
        "mvs-app" => {
            rel == "menus.rs" || rel == "menu_audio.rs" || rel.starts_with("launcher") || rel == "online/menus.rs"
        }
        _ => false,
    };
    if menu { "menu".to_owned() } else { own }
}

/// Each `"<word>"` of a line (a word of ASCII letters, digits and `_`), left to right, a match's closing quote not
/// opening the next.
fn literals(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'"' {
                out.push(&line[i + 1..j]);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn text(found: &BTreeMap<String, BTreeSet<String>>) -> String {
    let rows: Vec<String> = found
        .iter()
        .map(|(e, areas)| {
            let a: Vec<String> = areas.iter().map(|a| format!("\"{a}\"")).collect();
            format!("    (\"{e}\", &[{}]),", a.join(", "))
        })
        .collect();
    format!(
        "//! The sound events the native crates post by a string literal, with the areas whose code names each: the base
//! scope's source (from the Rust sources). Written by mvs-app's
//! `posted` test (`MVS_WRITE_POSTED=1`), which fails when this file is stale. Not edited by hand.

/// Each event (lower case, sorted) and the areas whose code names it (sorted); the area `menu` is the menus' code,
/// whose events are the menu scope's.
pub const POSTED: &[(&str, &[&str])] = &[
{}
];
",
        rows.join("\n")
    )
}

#[test]
fn posted_is_current() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let a = Audio::new(Arc::new(store), Backend::Offline).expect("the offline audio");
    let events: BTreeSet<String> = a.init().events.keys().map(|e| e.to_lowercase()).collect();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for krate in CRATES {
        let src = crates.join(krate).join("src");
        let mut files = Vec::new();
        rs_files(&src, &mut files);
        for file in files {
            let rel = file.strip_prefix(&src).expect("under src").to_string_lossy().replace('\\', "/");
            if rel == "audio/posted.rs" {
                continue;
            }
            let area = area_of(krate, &rel);
            let body = std::fs::read_to_string(&file).expect("a source file reads");
            for line in body.split('\n') {
                let s = line.trim_start();
                if s.starts_with("#[cfg(test)]") {
                    break;
                }
                if s.starts_with("//") {
                    continue;
                }
                for lit in literals(line) {
                    let e = lit.to_lowercase();
                    if events.contains(&e) {
                        found.entry(e).or_default().insert(area.clone());
                    }
                }
            }
        }
    }
    let want = text(&found);
    let out = crates.join("mvs-app/src/audio/posted.rs");
    if std::env::var_os("MVS_WRITE_POSTED").is_some() {
        std::fs::write(&out, &want).expect("posted.rs writes");
        return;
    }
    let have = std::fs::read_to_string(&out).unwrap_or_default();
    assert!(have == want, "src/audio/posted.rs is stale: run this test with MVS_WRITE_POSTED=1");
}
