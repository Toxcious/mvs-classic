//! The boxes bind against the oracle: per character key of the oracle store's gamedata kind, `bind_boxes` over the
//! oracle fighter kind's `data` (the oracle bind's input) gives the entry's `boxes` and `boxData` exactly (the bits of
//! every number, key order, the float32 bytes), and its bone boxes the entry's actors' `boneBox` tracks. The paks and
//! the stores are the developer's, so it runs only when asked:
//! `cargo test -p mvs-import --profile trace boxes::check -- --ignored --nocapture` with `MVS_PAKS` (the paks),
//! `MVS_BOXES_FIGHTER` (a store with the oracle's fighter kind holding `data`) and `MVS_BOXES_ORACLE` (a store with
//! the oracle's gamedata kind); `MVS_BOXES_KEYS` (comma separated) picks keys.

use std::path::PathBuf;
use std::sync::Arc;

use mvs_data::value::{decode_entry, encode, from_value};
use mvs_data::{BoxIndex, FighterData, Value};
use mvs_pak::{BuildPaths, pak_files_in, setup};
use mvs_store::Store;
use mvs_uasset::PackageCache;
use mvs_uasset::package::FileSource;

use super::bind_boxes;
use crate::catalog::scan_catalog;
use crate::kind::{Cx, Shared};
use crate::reads::StoreReads;

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
}

/// The CLI's way to a context over the developer's paks, run `f` with it.
fn with_cx<T>(f: impl FnOnce(&Cx<'_>) -> T) -> T {
    let dir = env("MVS_PAKS");
    let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).unwrap();
    let files = pak_files_in(&dir).unwrap();
    let set = Arc::new(mvs_pak::PakSet::open(&files, &key).unwrap());
    let paths = Arc::new(BuildPaths::from_paks(&set).unwrap());
    let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), 1 << 30);
    let (catalog, _) = scan_catalog(&set, &paths, &|_| {});
    let shared = Shared::default();
    let reads = StoreReads::none();
    let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
    f(&cx)
}

/// The first place two trees differ (a number by its bits), `None` none.
fn diff(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) if x.to_bits() == y.to_bits() => None,
        (Value::Num(_), Value::Num(_)) => Some(format!("{at}: {a:?} / {b:?}")),
        (Value::Obj(x), Value::Obj(y)) => {
            let (kx, ky): (Vec<&String>, Vec<&String>) = (x.keys().collect(), y.keys().collect());
            if kx != ky {
                return Some(format!("{at}: keys {kx:?} / {ky:?}"));
            }
            x.iter().find_map(|(k, v)| diff(v, &y[k], &format!("{at}.{k}")))
        }
        (Value::Arr(x), Value::Arr(y)) => {
            if x.len() != y.len() {
                return Some(format!("{at}: length {} / {}", x.len(), y.len()));
            }
            x.iter().zip(y).enumerate().find_map(|(i, (v, w))| diff(v, w, &format!("{at}[{i}]")))
        }
        (Value::F32s(x), Value::F32s(y)) => f32_diff(x, y, at),
        _ if a == b => None,
        _ => Some(format!("{at}: {a:?} / {b:?}")),
    }
}

fn f32_diff(x: &[f32], y: &[f32], at: &str) -> Option<String> {
    if x.len() != y.len() {
        return Some(format!("{at}: {} floats / {}", x.len(), y.len()));
    }
    let bad: Vec<usize> = (0..x.len()).filter(|i| x[*i].to_bits() != y[*i].to_bits()).collect();
    let first = *bad.first()?;
    Some(format!("{at}: {} floats differ, first [{first}] {} / {}", bad.len(), x[first], y[first]))
}

/// A tree as the gamedata entry writes it.
fn tree<T: serde::Serialize>(v: &T) -> Value {
    decode_entry(&encode(v).unwrap()).unwrap()
}

/// One key: `Ok` or the first difference.
fn check_key(cx: &Cx<'_>, key: &str, fighter: &Store, oracle: &Value) -> Result<(), String> {
    let fk = fighter.kind("fighter").map_err(|e| e.to_string())?;
    let tail = format!("/{key}.{key}").to_lowercase();
    let path = fk.paths().find(|p| p.to_lowercase().ends_with(&tail)).ok_or("no fighter entry")?.to_owned();
    // (straight from the bytes: a serde_json::Value on the way would sort the bags' keys)
    #[derive(serde::Deserialize)]
    struct Entry {
        data: FighterData,
    }
    let entry: Entry =
        serde_json::from_slice(&fk.bytes(&path).map_err(|e| e.to_string())?).map_err(|e| format!("data: {e}"))?;
    let fd = entry.data;
    let (mut index, data) = bind_boxes(cx, key, &fd).map_err(|e| format!("bind: {e}"))?;
    let bone_boxes = index.bone_boxes.take();
    let ob = oracle.as_obj().ok_or("no entry")?;
    let theirs: BoxIndex = from_value(ob["boxes"].clone()).map_err(|e| format!("oracle boxes: {e}"))?;
    if let Some(d) = diff(&tree(&index), &tree(&theirs), "boxes") {
        return Err(d);
    }
    let Value::F32s(their_data) = &ob["boxData"] else { return Err("oracle boxData: no float32 section".into()) };
    if let Some(d) = f32_diff(&data.0, their_data, "boxData") {
        return Err(d);
    }
    // the bone boxes, which layBoneBoxes laid on the fighter data's actors' components
    let actors = ob["fighter"].as_obj().and_then(|f| f.get("actors")).and_then(Value::as_obj);
    let mut n = 0;
    for (cls, comps) in bone_boxes.iter().flatten() {
        for (comp, track) in comps {
            let theirs = actors
                .and_then(|a| a.get(cls))
                .and_then(Value::as_obj)
                .and_then(|a| a.get("components"))
                .and_then(Value::as_obj)
                .and_then(|c| c.get(comp))
                .and_then(Value::as_obj)
                .and_then(|c| c.get("boneBox"))
                .and_then(Value::as_str);
            if theirs != Some(track.as_str()) {
                return Err(format!("boneBoxes {cls}.{comp}: differ ({} / {:?})", track.len(), theirs.map(str::len)));
            }
            n += 1;
        }
    }
    eprintln!("{key}: equal ({} anims, {} floats, {n} bone boxes)", index.anims.len(), data.0.len());
    Ok(())
}

#[test]
#[ignore = "needs the paks and the oracle's stores"]
fn boxes_equal_oracle() {
    let fighter = Store::open(env("MVS_BOXES_FIGHTER")).unwrap();
    let store = Store::open(env("MVS_BOXES_ORACLE")).unwrap();
    let gd = store.kind("gamedata").unwrap();
    let pick: Option<Vec<String>> =
        std::env::var("MVS_BOXES_KEYS").ok().map(|k| k.split(',').map(str::to_owned).collect());
    let keys: Vec<(String, String)> = gd
        .paths()
        .filter_map(|p| Some((p.strip_prefix("/GameData/Char/")?.split('.').next()?.to_owned(), p.to_owned())))
        .filter(|(k, _)| pick.as_ref().is_none_or(|l| l.contains(k)))
        .collect();
    let bad: Vec<String> = with_cx(|cx| {
        keys.iter()
            .filter_map(|(k, p)| {
                let oracle = decode_entry(&gd.bytes(p).unwrap()).unwrap();
                check_key(cx, k, &fighter, &oracle).err().map(|e| format!("{k}: {e}"))
            })
            .collect()
    });
    eprintln!("{} keys, {} differ", keys.len(), bad.len());
    for b in &bad {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}
