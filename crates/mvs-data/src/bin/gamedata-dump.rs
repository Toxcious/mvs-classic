//! The Rust half of the gamedata schema check: decodes each entry of the store's `gamedata` kind into its typed
//! record, writes the record's canonical dump and compares it with the oracle's dump of the same entry (its import's
//! `GAMEDATA_DUMP=<dir>` writes those).
//!
//!   gamedata-dump [--store <dir>] [--ts <dir>] [--out <dir>] [--load <stage> <key>...] [<path part>...]
//!
//! --store  the store (default: mvs_store::dev_store_dir)
//! --ts     the oracle's dumps' folder: each entry's dump is compared with `<dir>/<path's segments joined by _>.txt`
//! --out    writes each entry's dump there under the same name
//! --load   times GameData::load of the stage and the keys, and prints the bytes read
//! Entries whose path holds one of the parts are taken (all without one). Exits 1 on a mismatch or a failed entry.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

fn dump_name(path: &str) -> String {
    let end = path.rfind('.').unwrap_or(path.len());
    path[1..end].replace('/', "_") + ".txt"
}

fn main() -> ExitCode {
    let mut store = None;
    let mut ts = None;
    let mut out = None;
    let mut load: Option<Vec<String>> = None;
    let mut parts = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" => store = args.next().map(PathBuf::from),
            "--ts" => ts = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "--load" => load = Some(args.by_ref().collect()),
            _ => parts.push(a),
        }
    }
    let dir = store.unwrap_or_else(mvs_store::dev_store_dir);
    let store = match mvs_store::Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(l) = load {
        let Some((stage, keys)) = l.split_first() else {
            eprintln!("--load <stage> <key>...");
            return ExitCode::FAILURE;
        };
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let pack = store.kind(mvs_data::KIND).expect("the gamedata kind");
        let t = Instant::now();
        match mvs_data::GameData::load(&store, &keys, stage) {
            Ok(gd) => {
                let ms = t.elapsed().as_secs_f64() * 1e3;
                let mut paths: Vec<String> = gd.chars.keys().map(|k| mvs_data::path::char(k)).collect();
                paths.push(mvs_data::path::stage(stage));
                use mvs_data::path as p;
                paths.extend(
                    [p::INDEX, p::ITEMS, p::PERKS, p::RING_OUTS, p::ANNOUNCER, p::ARCADE, p::MUTATORS, p::BOT_TREES]
                        .map(String::from),
                );
                for path in &paths {
                    let r = pack.record(path).expect("an entry");
                    println!("{path}: {} bytes ({} stored)", pack.bytes(path).map(|b| b.len()).unwrap_or(0), r.length);
                }
                println!("GameData::load {stage} {:?}: {ms:.1} ms ({} characters)", keys, gd.chars.len());
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let pack = match store.kind(mvs_data::KIND) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(o) = &out {
        std::fs::create_dir_all(o).expect("the out folder");
    }
    let mut bad = 0;
    let mut n = 0;
    let paths: Vec<String> = pack
        .paths()
        .filter(|p| parts.is_empty() || parts.iter().any(|x| p.contains(x.as_str())))
        .map(String::from)
        .collect();
    for path in paths {
        n += 1;
        let t = Instant::now();
        let bytes = match pack.bytes(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("FAIL {path}: {e}");
                bad += 1;
                continue;
            }
        };
        let d = match mvs_data::dump_entry(&path, &bytes) {
            Ok(d) => d,
            Err(e) => {
                println!("FAIL {path}: {e}");
                bad += 1;
                continue;
            }
        };
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let name = dump_name(&path);
        if let Some(o) = &out {
            std::fs::write(o.join(&name), &d).expect("a dump written");
        }
        let Some(ts) = &ts else {
            println!("ok   {path} ({} bytes, {ms:.1} ms)", bytes.len());
            continue;
        };
        let want = match std::fs::read_to_string(ts.join(&name)) {
            Ok(w) => w,
            Err(e) => {
                println!("FAIL {path}: no oracle dump {name}: {e}");
                bad += 1;
                continue;
            }
        };
        if want == d {
            println!("same {path} ({} lines, {ms:.1} ms)", d.lines().count());
            continue;
        }
        bad += 1;
        let (mut a, mut b) = (want.lines(), d.lines());
        let mut line = 0;
        loop {
            line += 1;
            match (a.next(), b.next()) {
                (Some(x), Some(y)) if x == y => continue,
                (x, y) => {
                    println!("DIFF {path} line {line}: oracle {x:?}, Rust {y:?}");
                    break;
                }
            }
        }
    }
    println!("{n} entries, {bad} failed");
    if bad == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
