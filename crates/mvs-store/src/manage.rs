//! What the import does to a store folder besides writing packs: the manifest read and replaced, what is stale (one
//! rule for the import and for a launcher asking before it offers Play), and the files no manifest names removed.
//!
//! Switching: a kind is written to `<name>.pack.tmp`, flushed, reopened and checked, renamed to `<name>.pack`
//! ([`crate::writer::PackWriter::finish`]); then the manifest is replaced ([`write_manifest`]: `store.json.tmp`
//! flushed, renamed over `store.json`); then the files no manifest names are removed ([`clean_store`]). A run that
//! dies leaves the previous manifest and everything it names untouched; its leftovers go on the next run.

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use crate::{FOOT, FOOTER, HEAD, Manifest, Result, STORE_FORMAT, StoreError};

/// The manifest's file name.
pub const MANIFEST: &str = "store.json";
/// The store's import lock's file name (held by the importer while it runs).
pub const LOCK: &str = "import.lock";

/// Whether a file name in a store folder is one of the store's own layout besides the manifest and the lock: packs
/// and catalogs (and their temp files while written), the manifest's temp file and a dead run's lock (`STORE_FILE`).
pub fn is_layout_file(name: &str) -> bool {
    let lower_alnum = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase());
    let name_run = |s: &str, ext: &str, head: &dyn Fn(&str) -> bool| -> bool {
        let s = s.strip_suffix(".tmp").unwrap_or(s);
        let Some(s) = s.strip_suffix(ext) else { return false };
        let Some((h, run)) = s.rsplit_once('-') else { return false };
        head(h) && lower_alnum(run)
    };
    // <kind>@<version>-<run>.pack
    let pack = |h: &str| {
        h.split_once('@').is_some_and(|(k, v)| {
            !k.is_empty()
                && k.bytes().all(|c| c.is_ascii_lowercase())
                && !v.is_empty()
                && v.bytes().all(|c| c.is_ascii_digit())
        })
    };
    if name_run(name, ".pack", &pack) || name_run(name, ".json", &|h| h == "catalog") || name == "store.json.tmp" {
        return true;
    }
    // import.lock.<hex>.dead
    name.strip_prefix("import.lock.")
        .and_then(|s| s.strip_suffix(".dead"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
}

/// The manifest of `dir`, `None` when there is none; an error when `store.json` does not read as a manifest of this
/// format (the message names the file, never its text).
pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let file = dir.join(MANIFEST);
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StoreError::Io(file, e)),
    };
    let bad = |why: &str| StoreError::Format(file.clone(), format!("{why}; run the import again"));
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| bad("not JSON"))?;
    let format = v.get("format").and_then(|f| f.as_u64()).ok_or_else(|| bad("not a store manifest"))?;
    if format != u64::from(STORE_FORMAT) {
        return Err(bad(&format!("format {format}, this code reads {STORE_FORMAT}")));
    }
    // (from the text again: a Value's map would sort the kinds, which keep the manifest's order)
    serde_json::from_str(&text).map(Some).map_err(|_| bad("not a store manifest"))
}

/// Waits between rename tries on Windows (a scanner or indexer holding the file for a moment): about 1 s in all.
const RENAME_WAITS: [u64; 5] = [50, 100, 200, 300, 350];

/// Renames `from` over `to`, trying again a few times while Windows refuses it (access denied, sharing violation).
pub fn rename_retry(from: &Path, to: &Path) -> Result<()> {
    rename_retried(from, to).map_err(|e| StoreError::Io(to.to_path_buf(), e))
}

/// `rename_retry` with the OS error as it came (the program's other files: the stats file, the settings).
pub fn rename_retried(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut i = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33)) => {
                // (32: ERROR_SHARING_VIOLATION, 33: ERROR_LOCK_VIOLATION, which libuv maps to EBUSY: `renameRetry` retries it)
                let Some(&ms) = RENAME_WAITS.get(i) else { return Err(e) };
                std::thread::sleep(Duration::from_millis(ms));
                i += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Writes a file and flushes it to disk before returning (what is renamed into place must be whole after a crash).
pub fn write_synced(file: &Path, data: &[u8]) -> Result<()> {
    let io = |e| StoreError::Io(file.to_path_buf(), e);
    let mut f = File::create(file).map_err(io)?;
    f.write_all(data).map_err(io)?;
    f.sync_all().map_err(io)
}

/// Writes `dir`'s `store.json` (`store.json.tmp` flushed, then renamed over the
/// manifest), as JSON.stringify(m, null, 1) lays it out.
pub fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    let mut text = Vec::new();
    let mut ser =
        serde_json::Serializer::with_formatter(&mut text, serde_json::ser::PrettyFormatter::with_indent(b" "));
    serde::Serialize::serialize(m, &mut ser).map_err(|e| StoreError::Format(dir.join(MANIFEST), e.to_string()))?;
    text.push(b'\n');
    let tmp = dir.join(format!("{MANIFEST}.tmp"));
    write_synced(&tmp, &text)?;
    rename_retry(&tmp, &dir.join(MANIFEST))
}

/// Removes what the manifest does not name from the store folder: packs and catalogs of other runs, and the temp
/// files of a run that died. Only names of the layout are touched ([`is_layout_file`]); a file still open elsewhere
/// (a reader on an old pack, on Windows) stays for the next run. `writing`: the names of the packs this run is still
/// writing (its kinds overlap: one kind's switch does not remove another's pack in progress), asked per name after
/// the folder is read. Returns the names removed. Run under the store's lock.
pub fn clean_store(dir: &Path, m: Option<&Manifest>, writing: &dyn Fn(&str) -> bool) -> Result<Vec<String>> {
    let mut keep: Vec<&str> = vec![MANIFEST, LOCK];
    if let Some(m) = m {
        keep.extend(m.catalog.as_deref());
        keep.extend(m.kinds.values().map(|k| k.file.as_str()));
    }
    let mut removed = Vec::new();
    let Ok(names) = std::fs::read_dir(dir) else { return Ok(removed) };
    for e in names.flatten() {
        let Ok(n) = e.file_name().into_string() else { continue };
        if keep.contains(&n.as_str()) || !is_layout_file(&n) || writing(&n) {
            continue;
        }
        match std::fs::remove_file(e.path()) {
            Ok(()) => removed.push(n),
            Err(err) if err.kind() == ErrorKind::NotFound || err.kind() == ErrorKind::PermissionDenied => {}
            Err(err) if err.raw_os_error() == Some(32) => {}
            Err(err) => return Err(StoreError::Io(e.path(), err)),
        }
    }
    removed.sort();
    Ok(removed)
}

/// Whether a file is a finished pack: its head and footer are there, the footer naming the index right before it
/// (the index itself is read on open).
pub fn pack_complete(file: &Path) -> bool {
    let check = || -> std::io::Result<bool> {
        let mut f = File::open(file)?;
        let size = f.metadata()?.len();
        if size < HEAD.len() as u64 + FOOTER {
            return Ok(false);
        }
        let mut h = [0u8; 8];
        f.read_exact(&mut h)?;
        let mut foot = [0u8; FOOTER as usize];
        f.seek(SeekFrom::Start(size - FOOTER))?;
        f.read_exact(&mut foot)?;
        let at = f64::from_le_bytes(foot[0..8].try_into().unwrap_or_default());
        let len = u32::from_le_bytes(foot[8..12].try_into().unwrap_or_default());
        Ok(&h == HEAD && &foot[16..24] == FOOT && at + f64::from(len) == (size - FOOTER) as f64)
    };
    check().unwrap_or(false)
}

/// What an import has to do to a store folder ([`staleness`]).
#[derive(Clone, Debug, Default)]
pub struct Staleness {
    /// The manifest, `None` when there is none or it does not read (`unreadable` then says why).
    pub manifest: Option<Manifest>,
    pub unreadable: Option<String>,
    /// The store holds another build's import (its files stay until the new build's import switches in).
    pub new_build: bool,
    /// The catalog needs scanning.
    pub catalog: bool,
    /// The kinds of those asked that need importing, in the order asked.
    pub kinds: Vec<String>,
}

/// A kind an import asks about: its id, its version, and the kinds it is built from out of the store (the importer's
/// `Kind::reads`: id and the version read).
#[derive(Clone, Copy, Debug)]
pub struct Asked<'a> {
    pub id: &'a str,
    pub version: u64,
    pub reads: &'a [(&'a str, u64)],
}

/// What an import of `build_id` building `kinds` has to do in `dir`: a kind is stale when forced, when the store is
/// for another build or has no manifest, when its record is missing, of another version or partial, when its pack is
/// missing or unfinished, when a kind it reads is not the pack it was built from (rebuilt since, by any run; or the
/// record keeps none: one written before records kept them), or when a kind it reads is stale among those asked
/// (rebuilt by this run first); the catalog when forced, for another build, missing, or of another `catalog_version`
/// (the importer's).
pub fn staleness(dir: &Path, build_id: &str, catalog_version: u32, kinds: &[Asked<'_>], force: bool) -> Staleness {
    let (manifest, unreadable) = match read_manifest(dir) {
        Ok(m) => (m, None),
        Err(e) => (None, Some(e.to_string())),
    };
    let same = manifest.as_ref().filter(|m| m.build_id == build_id);
    let catalog = force
        || same.is_none_or(|m| {
            m.catalog_version != Some(catalog_version) || m.catalog.as_ref().is_none_or(|c| !dir.join(c).is_file())
        });
    let mut stale: Vec<bool> = kinds
        .iter()
        .map(|k| {
            let Some(m) = same else { return true };
            let Some(r) = m.kinds.get(k.id) else { return true };
            force
                || r.version != k.version
                || r.partial
                || !pack_complete(&dir.join(&r.file))
                || k.reads.iter().any(|(read, _)| r.reads.get(*read) != m.kinds.get(*read).map(|x| &x.file))
        })
        .collect();
    // closed over the reads: a reader of a kind this run rebuilds is rebuilt after it (the runner's waves)
    loop {
        let mut changed = false;
        for (i, k) in kinds.iter().enumerate() {
            if !stale[i] && k.reads.iter().any(|(read, _)| kinds.iter().zip(&stale).any(|(o, s)| *s && o.id == *read)) {
                stale[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let kinds = kinds.iter().zip(&stale).filter(|(_, s)| **s).map(|(k, _)| k.id.to_string()).collect();
    let new_build = manifest.as_ref().is_some_and(|m| m.build_id != build_id);
    Staleness { manifest, unreadable, new_build, catalog, kinds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KindRecord;

    /// A target another program holds open without sharing (a sharing violation, error 32) is renamed over once it
    /// lets go.
    #[cfg(windows)]
    #[test]
    fn rename_waits_for_a_held_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("mvs-rename-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("a.part"), dir.join("a"));
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&to).unwrap();
        let e = std::fs::rename(&from, &to).unwrap_err();
        assert!(e.kind() == ErrorKind::PermissionDenied || e.raw_os_error() == Some(32), "{e:?}");
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            drop(held);
        });
        rename_retried(&from, &to).unwrap();
        t.join().unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
    use crate::writer::PackWriter;
    use indexmap::IndexMap;

    #[test]
    fn layout_names() {
        for n in [
            "texture@2-mvw30m9z141e98.pack",
            "texture@2-mvw30m9z141e98.pack.tmp",
            "catalog-mvw30m9z141e98.json",
            "catalog-abc.json.tmp",
            "store.json.tmp",
            "import.lock.0f3a.dead",
        ] {
            assert!(is_layout_file(n), "{n}");
        }
        for n in [
            "store.json",
            "import.lock",
            "Texture@2-a.pack",
            "texture@-a.pack",
            "texture@2-.pack",
            "texture@2-A.pack",
            "catalog-.json",
            "notes.txt",
            "import.lock.xyz.dead",
            "import.lock.0f3a.tmp",
        ] {
            assert!(!is_layout_file(n), "{n}");
        }
    }

    fn ask(id: &str, version: u64) -> Asked<'_> {
        Asked { id, version, reads: &[] }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mvs-store-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn manifest_round_trip_staleness_and_clean() {
        let dir = scratch("manage");
        let mut w = PackWriter::create(&dir.join("skeleton@1-r1.pack")).unwrap();
        w.add("/Game/A.A", "application/json", b"{}").unwrap();
        w.finish().unwrap();
        assert!(pack_complete(&dir.join("skeleton@1-r1.pack")));
        let rec = |file: &str, version| KindRecord {
            version,
            file: file.into(),
            entries: 1,
            bytes: 1,
            failed: 0,
            partial: false,
            reads: IndexMap::new(),
        };
        let mut kinds = IndexMap::new();
        kinds.insert("skeleton".to_string(), rec("skeleton@1-r1.pack", 1));
        kinds.insert("mesh".to_string(), rec("mesh@2-r1.pack", 2));
        let m = Manifest { format: 2, build_id: "b".into(), catalog: None, catalog_version: None, kinds };
        write_manifest(&dir, &m).unwrap();
        let text = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
        assert!(text.starts_with(
            "{\n \"format\": 2,\n \"buildId\": \"b\",\n \"kinds\": {\n  \"skeleton\": {\n   \"version\": 1,"
        ));
        assert!(text.ends_with("}\n"));
        let back = read_manifest(&dir).unwrap().unwrap();
        assert_eq!(back.kinds.keys().collect::<Vec<_>>(), ["skeleton", "mesh"]);

        // mesh's pack is missing, texture has no record, skeleton is current; the catalog was never written
        let st = staleness(&dir, "b", 1, &[ask("skeleton", 1), ask("mesh", 2), ask("texture", 2)], false);
        assert_eq!(st.kinds, ["mesh", "texture"]);
        assert!(st.catalog && !st.new_build);
        assert_eq!(staleness(&dir, "b", 1, &[ask("skeleton", 2)], false).kinds, ["skeleton"]);
        assert_eq!(staleness(&dir, "b", 1, &[ask("skeleton", 1)], true).kinds, ["skeleton"]);
        let other = staleness(&dir, "c", 1, &[ask("skeleton", 1)], false);
        assert!(other.new_build && other.kinds == ["skeleton"]);

        std::fs::write(dir.join("skeleton@1-old.pack"), b"x").unwrap();
        std::fs::write(dir.join("mesh@2-r2.pack.tmp"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let removed = clean_store(&dir, Some(&m), &|n| n == "mesh@2-r2.pack.tmp").unwrap();
        assert_eq!(removed, ["skeleton@1-old.pack"]);
        assert!(dir.join("notes.txt").is_file() && dir.join("skeleton@1-r1.pack").is_file());

        std::fs::write(dir.join(MANIFEST), b"{\"format\": 1}").unwrap();
        assert!(read_manifest(&dir).is_err());
        let st = staleness(&dir, "b", 1, &[ask("skeleton", 1)], false);
        assert!(st.unreadable.is_some() && st.manifest.is_none() && !st.new_build);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A kind built from another kind's pack is stale once that pack is not the store's (rebuilt by a run that did not
    /// ask for the reader), when its record keeps no reads (written before records kept them), and when the kind it
    /// reads is stale among those asked; never for a read it does not have.
    #[test]
    fn readers_go_stale_with_what_they_read() {
        let dir = scratch("reads");
        let pack = |name: &str| {
            let mut w = PackWriter::create(&dir.join(name)).unwrap();
            w.add("/Game/A.A", "application/json", b"{}").unwrap();
            w.finish().unwrap();
        };
        for p in ["gamedata@1-r1.pack", "fighter@1-r1.pack", "material@1-r1.pack", "texture@1-r1.pack"] {
            pack(p);
        }
        let rec = |file: &str, reads: &[(&str, &str)]| KindRecord {
            version: 1,
            file: file.into(),
            entries: 1,
            bytes: 1,
            failed: 0,
            partial: false,
            reads: reads.iter().map(|(k, f)| (k.to_string(), f.to_string())).collect(),
        };
        let manifest = |gamedata: &str, fighter: KindRecord| {
            let mut kinds = IndexMap::new();
            kinds.insert("gamedata".to_string(), rec(gamedata, &[]));
            kinds.insert("fighter".to_string(), fighter);
            kinds.insert("material".to_string(), rec("material@1-r1.pack", &[("fighter", "fighter@1-r1.pack")]));
            kinds.insert("texture".to_string(), rec("texture@1-r1.pack", &[]));
            Manifest { format: 2, build_id: "b".into(), catalog: None, catalog_version: None, kinds }
        };
        let asked = [
            Asked { id: "gamedata", version: 1, reads: &[] },
            Asked { id: "fighter", version: 1, reads: &[("gamedata", 1)] },
            Asked { id: "material", version: 1, reads: &[("fighter", 1)] },
            Asked { id: "texture", version: 1, reads: &[] },
        ];
        let stale = |m: &Manifest, asked: &[Asked<'_>]| {
            write_manifest(&dir, m).unwrap();
            staleness(&dir, "b", 1, asked, false).kinds
        };
        // every reader built from the packs the store names: current
        let current = manifest("gamedata@1-r1.pack", rec("fighter@1-r1.pack", &[("gamedata", "gamedata@1-r1.pack")]));
        assert!(stale(&current, &asked).is_empty());
        // gamedata rebuilt by an earlier run (forced, or alone): its reader, and that one's reader, are stale
        pack("gamedata@1-r2.pack");
        let rebuilt = manifest("gamedata@1-r2.pack", rec("fighter@1-r1.pack", &[("gamedata", "gamedata@1-r1.pack")]));
        assert_eq!(stale(&rebuilt, &asked), ["fighter", "material"]);
        // asked alone, the reader is stale on its own record; its reader not asked is not named
        assert_eq!(stale(&rebuilt, &asked[1..2]), ["fighter"]);
        // a record without reads (an older store): stale where the kind has reads
        assert_eq!(
            stale(&manifest("gamedata@1-r1.pack", rec("fighter@1-r1.pack", &[])), &asked),
            ["fighter", "material"]
        );
        // gamedata stale in this run (its pack unfinished): its readers rebuild after it
        std::fs::write(dir.join("gamedata@1-r1.pack"), b"x").unwrap();
        assert_eq!(stale(&current, &asked), ["gamedata", "fighter", "material"]);
        // a stored record with its reads reads back the same; an older one's reads are empty
        write_manifest(&dir, &current).unwrap();
        let back = read_manifest(&dir).unwrap().unwrap();
        assert_eq!(back.kinds["fighter"].reads["gamedata"], "gamedata@1-r1.pack");
        assert!(!std::fs::read_to_string(dir.join(MANIFEST)).unwrap().contains("\"reads\": {}"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A pack whose target a scanner holds for a moment (opened without sharing): the rename is refused, then allowed,
    /// and `finish` goes through it.
    #[cfg(windows)]
    #[test]
    fn rename_refused_once_then_allowed() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch("rename");
        let target = dir.join("skeleton@1-r1.pack");
        std::fs::write(&target, b"old").unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
        let mut w = PackWriter::create(&target).unwrap();
        w.add("/Game/A.A", "application/json", b"{}").unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            drop(held);
        });
        w.finish().unwrap();
        release.join().unwrap();
        assert!(pack_complete(&target));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
