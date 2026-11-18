//! The reader against the build's paks: the developer's paks folder (`MVS_PAKS`, else `content/local.json`'s "paks"),
//! the key from the exe beside them. When the oracle's pak dump has been written (its reader's view of the
//! same paks: path count and hash, mounts, SHA-256 of a spread of entries and ranges) to `.scratch/pak-dump/` or the
//! folder `MVS_PAK_DUMP` names, each point is compared with it. A tree with no paks or no dump says so and passes.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mvs_pak::{BuildPaths, PakSet, pak_files_in, setup};
use sha2::{Digest, Sha256};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn paks_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MVS_PAKS") {
        return Some(PathBuf::from(d));
    }
    let text = std::fs::read_to_string(root().join("content/local.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let d = PathBuf::from(v.get("paks")?.as_str()?);
    d.is_dir().then_some(d)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

#[test]
fn reads_the_build_paks() {
    let Some(dir) = paks_dir() else {
        eprintln!("no paks folder (MVS_PAKS or content/local.json \"paks\"): skipped");
        return;
    };
    let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).unwrap();
    let files = pak_files_in(&dir).unwrap();
    let t0 = Instant::now();
    let set = PakSet::open(&files, &key).unwrap();
    let open = t0.elapsed();
    let paths: Vec<&str> = set.paths().collect();
    let list_hash = hex(&Sha256::digest(paths.join("\n").as_bytes()));
    eprintln!("open {:.0} ms, {} paths, list {}", open.as_secs_f64() * 1e3, paths.len(), &list_hash[..16]);
    for w in paths.windows(2) {
        assert_eq!(mvs_pak::cmp_utf16(w[0], w[1]), std::cmp::Ordering::Less, "{} {}", w[0], w[1]);
    }
    for p in paths.iter().step_by(997) {
        assert_eq!(set.entry(&p.to_uppercase()).map(|e| e.path.as_str()), Some(*p));
    }
    assert!(set.entry("no/such/file.uasset").is_none());
    let bp = BuildPaths::from_paks(&set).unwrap();

    let dump = std::env::var_os("MVS_PAK_DUMP").map_or_else(|| root().join(".scratch/pak-dump"), PathBuf::from);
    let Ok(text) = std::fs::read_to_string(dump.join("pak-dump.json")) else {
        eprintln!("no pak-dump.json in {} (the oracle's pak dump): the oracle comparison skipped", dump.display());
        return;
    };
    let ts: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(ts["count"].as_u64(), Some(paths.len() as u64), "path count");
    assert_eq!(ts["listHash"].as_str(), Some(list_hash.as_str()), "path list hash");
    assert_eq!(ts["project"].as_str(), Some(bp.project.as_str()));
    let mounts: Vec<(String, String)> = ts["mounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["root"].as_str().unwrap().to_string(), m["dir"].as_str().unwrap().to_string()))
        .collect();
    let ours: Vec<(String, String)> = bp.mounts.iter().map(|m| (m.root.clone(), m.dir.clone())).collect();
    assert_eq!(ours, mounts, "mounts");
    let mut n = 0;
    let mut bytes = 0;
    let t0 = Instant::now();
    for (p, want) in ts["entries"].as_object().unwrap() {
        let e = set.entry(p).unwrap();
        assert_eq!(e.compressed, want["comp"].as_bool().unwrap(), "{p}");
        assert_eq!(e.encrypted, want["enc"].as_bool().unwrap(), "{p}");
        assert_eq!(set.pak_files()[e.pak].file_name().unwrap().to_str(), want["pak"].as_str(), "{p}");
        let b = set.read(p).unwrap();
        assert_eq!(b.len() as u64, want["size"].as_u64().unwrap(), "{p}");
        assert_eq!(hex(&Sha256::digest(&b)), want["sha256"].as_str().unwrap(), "{p}");
        n += 1;
        bytes += b.len();
    }
    let read = t0.elapsed();
    for r in ts["ranges"].as_array().unwrap() {
        let p = r["path"].as_str().unwrap();
        let b = set.read_range(p, r["offset"].as_u64().unwrap(), r["length"].as_u64().unwrap()).unwrap();
        assert_eq!(hex(&Sha256::digest(&b)), r["sha256"].as_str().unwrap(), "{p} range");
    }
    eprintln!(
        "{n} entries ({} MB) and {} ranges equal to the oracle reader's ({:.0} ms)",
        bytes >> 20,
        ts["ranges"].as_array().unwrap().len(),
        read.as_secs_f64() * 1e3
    );
}

#[test]
fn build_list_is_whole() {
    let b = setup::build_list().unwrap();
    assert_eq!(b.paks.len(), 16);
    assert_eq!(b.files.len(), 2);
    assert_eq!(setup::build_id().unwrap(), b.id);
    let mut bad = b.clone();
    bad.paks[0].size += 1;
    assert!(setup::check_list(&bad).is_err());
    assert!(setup::check_table(&b.files, &b.files[0].path, "codebooks", b"x").is_err());
    assert!(setup::check_table(&b.files, &b.files[0].path, "nothing", b"x").is_err());
}
