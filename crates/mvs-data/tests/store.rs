//! Every entry of the store's gamedata kind decodes into its typed record. With `GAMEDATA_TS_DUMPS=<dir>` (the
//! oracle import's `GAMEDATA_DUMP` folder), each record's canonical dump must equal the oracle's dump of its entry.
//! Passes with a note where the store has no gamedata kind.

use mvs_data::{GameData, KIND, dump_entry};

#[test]
fn every_entry_decodes() {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("note: no store at {} (set CONTENT_STORE): not checked", dir.display());
        return;
    };
    let Ok(pack) = store.kind(KIND) else {
        eprintln!("note: the store at {} has no {KIND} kind (run the import): not checked", dir.display());
        return;
    };
    let ts = std::env::var_os("GAMEDATA_TS_DUMPS").map(std::path::PathBuf::from);
    let mut bad = Vec::new();
    for path in pack.paths() {
        let bytes = pack.bytes(path).unwrap();
        match dump_entry(path, &bytes) {
            Err(e) => bad.push(format!("{path}: {e}")),
            Ok(d) => {
                if let Some(ts) = &ts {
                    let end = path.rfind('.').unwrap();
                    let name = path[1..end].replace('/', "_") + ".txt";
                    match std::fs::read_to_string(ts.join(&name)) {
                        Ok(w) if w == d => {}
                        Ok(_) => bad.push(format!("{path}: its dump differs from {name}")),
                        Err(e) => bad.push(format!("{path}: {name}: {e}")),
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    let gd = GameData::load(&store, &["shaggy"], "trainingroomsmall").unwrap();
    assert_eq!(gd.default_char().key, gd.default_character);
    assert_eq!(gd.stage.name, "Map_TrainingRoomSmall");
    // The bindings' defaults and the rumble table (UForceFeedbackEffect GetDuration: the largest last key time).
    assert!(gd.input_defaults().rows["Default-P0"]["Jump"].iter().any(|k| k == "SpaceBar"));
    assert!(!gd.rumble.is_empty());
    for (name, fx) in &gd.rumble {
        let last = fx.channels.iter().filter_map(|c| c.keys.last()).fold(0f64, |m, k| if k.0 > m { k.0 } else { m });
        assert_eq!(fx.duration, last, "{name}");
        assert!(gd.rumble(name).is_some());
    }
    assert!(gd.rumble("no such effect").is_none());
}

/// The store's announcer packs (the native import's): every pack gives each alias the port posts exactly one row, each
/// row's event is one the build's banks hold, the default pack is the sim's, and a path no pack has, or none, is the
/// default (LoadStoredAnnouncerPackAssetData @390 / @414). Passes with a note where the store holds no pack list.
#[test]
fn announcer_packs() {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("note: no store at {} (set CONTENT_STORE): not checked", dir.display());
        return;
    };
    let Ok(pack) = store.kind(KIND) else {
        eprintln!("note: the store at {} has no {KIND} kind: not checked", dir.display());
        return;
    };
    if !pack.has(mvs_data::path::ANNOUNCER_PACKS) {
        eprintln!("note: the store at {} holds no announcer pack list (the oracle's): not checked", dir.display());
        return;
    }
    let packs: mvs_data::AnnouncerPacks = mvs_data::entry(&pack, mvs_data::path::ANNOUNCER_PACKS).unwrap();
    let default: mvs_data::AnnouncerPack = mvs_data::entry(&pack, mvs_data::path::ANNOUNCER).unwrap();
    let init = mvs_data::sound::load_init(&store).unwrap();
    const POSTED: [&str; 7] = [
        "announcer_timeout",
        "announcer_matchpoint_blue",
        "announcer_matchpoint_red",
        "announcer_victory",
        "announcer_defeat",
        "announcer_characterselect_intro",
        "announcer_characterselect_outro",
    ];
    assert!(packs.packs.len() > 1, "{} packs", packs.packs.len());
    for p in &packs.packs {
        for alias in POSTED {
            let n = p.rows.iter().filter(|r| r.0.eq_ignore_ascii_case(alias)).count();
            assert_eq!(n, 1, "{}: {n} rows of {alias}", p.path);
            assert!(p.rows.iter().any(|r| r.0.eq_ignore_ascii_case(alias) && r.1.is_empty()), "{}: {alias}", p.path);
        }
        for r in &p.rows {
            assert!(init.events.contains_key(&r.3.to_lowercase()), "{}: {} is no event of the banks", p.path, r.3);
        }
        let got = mvs_data::stored_announcer_pack(&store, &p.path).unwrap();
        assert_eq!(got.path, p.path);
        // (the package alone, any case, names it too)
        let pkg = p.path.split('.').next().unwrap().to_uppercase();
        assert_eq!(mvs_data::stored_announcer_pack(&store, &pkg).unwrap().path, p.path);
    }
    let in_list = packs.get(&default.path).expect("the default pack is listed");
    assert_eq!(format!("{:?}", in_list.rows), format!("{:?}", default.rows));
    assert_eq!(mvs_data::stored_announcer_pack(&store, "").unwrap().path, default.path);
    assert_eq!(mvs_data::stored_announcer_pack(&store, "/Game/No/Such.Such").unwrap().path, default.path);
}
