//! The store: what the import wrote, the one thing the running game reads. A folder with a manifest (`store.json`)
//! and one pack per kind; an entry is named by its kind and its path (the build object path, or a path the kind
//! gives it).
//!
//! Pack format (little-endian): "MVSPACK2", the entries' stored bytes back to back, the index, a 24-byte footer.
//! - index: u32 entry count, u32 content-type count, per type (u16 length, UTF-8), then per entry in path order:
//!   f64 offset, u32 stored length, u32 size, u8 type, u8 codec, u16 path length, path UTF-8;
//! - footer: f64 index offset, u32 index length, u32 0, "MVSPIDX2".
//!
//! An entry is stored as it is (codec 0) or as one zstd frame (codec 1).
//!
//! A [`Store`] is shared by every part of the game that reads it (a clone is the same store): each kind's pack is
//! opened once, its index read once, and its entries read by positioned reads, so any number of threads read one pack
//! at once. What the game builds from the entries is kept in the store's [`cache::Cache`].

pub mod cache;
pub mod manage;
pub mod writer;

pub use cache::{Cache, CacheStats};
pub use manage::write_manifest;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

const HEAD: &[u8; 8] = b"MVSPACK2";
const FOOT: &[u8; 8] = b"MVSPIDX2";
const FOOTER: u64 = 24;
const CODEC_RAW: u8 = 0;
const CODEC_ZSTD: u8 = 1;
/// The manifest's and the packs' format this reader reads.
pub const STORE_FORMAT: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("store {0}: {1}")]
    Format(PathBuf, String),
    #[error("store: no {kind} kind in {dir} (run the import)")]
    NoKind { kind: String, dir: PathBuf },
    #[error("store: no {kind} entry {path}")]
    NoEntry { kind: String, path: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// One kind's line of the manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KindRecord {
    pub version: u64,
    /// The pack file's name in the store folder.
    pub file: String,
    pub entries: u64,
    pub bytes: u64,
    pub failed: u64,
    /// Written from a narrowed list: stale for the next import.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// The pack file of each kind it was built from (`Kind::reads` of the importer), as the store named it when the
    /// build started: the kind is stale once one of them is no longer the store's (that kind rebuilt since). Absent in
    /// a record written before the importer kept it, and for a kind that reads none.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub reads: IndexMap<String, String>,
}

/// `store.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub build_id: String,
    /// The catalog file's name, absent until an import wrote one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_version: Option<u32>,
    pub kinds: IndexMap<String, KindRecord>,
}

/// An entry's index record.
#[derive(Clone, Copy, Debug)]
pub struct EntryRecord {
    pub offset: u64,
    pub length: u32,
    pub size: u32,
    /// The content type's index in the pack's type list.
    pub content_type: u8,
    pub codec: u8,
}

/// A store folder and its manifest, its packs opened so far and the cache of what was built from them. A clone
/// shares all three.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    manifest: Manifest,
    /// The packs opened, by kind (a cache: two threads asking for one kind read its index once, another kind's
    /// open does not wait for it).
    packs: Cache,
    cache: Cache,
    /// The cache's clock when the last load began (`Store::begin_load`).
    load_start: AtomicU64,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("dir", &self.inner.dir).finish_non_exhaustive()
    }
}

impl Store {
    /// Opens the store in `dir` (its manifest read once).
    pub fn open(dir: impl Into<PathBuf>) -> Result<Store> {
        let dir = dir.into();
        let file = dir.join("store.json");
        let text = std::fs::read_to_string(&file).map_err(|e| StoreError::Io(file.clone(), e))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| StoreError::Format(file.clone(), e.to_string()))?;
        if manifest.format != STORE_FORMAT {
            return Err(StoreError::Format(
                file,
                format!("format {} (this reader reads {STORE_FORMAT})", manifest.format),
            ));
        }
        Ok(Store {
            inner: Arc::new(Inner {
                dir,
                manifest,
                packs: Cache::new(),
                cache: Cache::new(),
                load_start: AtomicU64::new(0),
            }),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    pub fn manifest(&self) -> &Manifest {
        &self.inner.manifest
    }

    /// The kind's version in this store, if it has the kind.
    pub fn version(&self, kind: &str) -> Option<u64> {
        self.inner.manifest.kinds.get(kind).map(|k| k.version)
    }

    /// One kind's pack: opened and its index read on the first call, shared after.
    pub fn kind(&self, kind: &str) -> Result<Arc<Pack>> {
        let rec = self
            .inner
            .manifest
            .kinds
            .get(kind)
            .ok_or_else(|| StoreError::NoKind { kind: kind.to_string(), dir: self.inner.dir.clone() })?;
        self.inner.packs.get_or(kind, || Pack::open(kind, self.inner.dir.join(&rec.file)))
    }

    /// What the game built from this store's entries, kept for the process (cache.rs).
    pub fn cache(&self) -> &Cache {
        &self.inner.cache
    }

    /// The value the cache holds for `key`, built by `make` on the first ask; `weight` its bytes for a trim.
    pub fn cached<T, E>(
        &self,
        key: &str,
        make: impl FnOnce() -> std::result::Result<T, E>,
        weight: impl FnOnce(&T) -> usize,
    ) -> std::result::Result<Arc<T>, E>
    where
        T: std::any::Any + Send + Sync,
    {
        self.inner.cache.get_or_weighed(key, make, weight)
    }

    /// A load begins (a match's, or a screen's that lets the last one's go): what it asks of the cache from now
    /// on is its own, and `trim` lets the rest go.
    pub fn begin_load(&self) {
        self.inner.load_start.store(self.inner.cache.clock(), Ordering::Relaxed);
    }

    /// Lets go of the cached values nobody holds and nothing asked since the load began (what a match or a
    /// preview holds stays, and what the load read and let go). The bytes let go.
    pub fn trim(&self) -> usize {
        self.inner.cache.trim_older(self.inner.load_start.load(Ordering::Relaxed))
    }
}

/// One kind's pack: its index in memory, entries read by a positioned read (any number of threads at once).
pub struct Pack {
    kind: String,
    path: PathBuf,
    file: File,
    types: Vec<String>,
    index: IndexMap<String, EntryRecord>,
}

impl Pack {
    pub fn open(kind: &str, path: impl Into<PathBuf>) -> Result<Pack> {
        let path = path.into();
        let io = |e| StoreError::Io(path.clone(), e);
        let bad = |why: String| StoreError::Format(path.clone(), why);
        let mut file = File::open(&path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        if len < HEAD.len() as u64 + FOOTER {
            return Err(bad(format!("{len} bytes, too short for a pack")));
        }
        let mut head = [0u8; 8];
        file.read_exact(&mut head).map_err(io)?;
        if &head != HEAD {
            return Err(bad("not a pack (no MVSPACK2)".into()));
        }
        let mut foot = [0u8; FOOTER as usize];
        read_at(&file, &mut foot, len - FOOTER).map_err(io)?;
        if &foot[16..24] != FOOT {
            return Err(bad("no MVSPIDX2 footer".into()));
        }
        let index_offset = f64::from_le_bytes(foot[0..8].try_into().unwrap());
        let index_len = u32::from_le_bytes(foot[8..12].try_into().unwrap()) as u64;
        if !(index_offset >= 0.0 && index_offset.fract() == 0.0) || index_offset as u64 + index_len + FOOTER != len {
            return Err(bad(format!("index at {index_offset} of {index_len} bytes in a {len}-byte pack")));
        }
        let mut raw = vec![0u8; index_len as usize];
        read_at(&file, &mut raw, index_offset as u64).map_err(io)?;
        let mut r = Cursor { b: &raw, at: 0 };
        let short = || bad("index cut short".into());
        let count = r.u32().ok_or_else(short)? as usize;
        let type_count = r.u32().ok_or_else(short)? as usize;
        let mut types = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            let n = r.u16().ok_or_else(short)? as usize;
            types.push(r.str(n).ok_or_else(short)?);
        }
        let mut index = IndexMap::with_capacity(count);
        for _ in 0..count {
            let offset = r.f64().ok_or_else(short)?;
            let length = r.u32().ok_or_else(short)?;
            let size = r.u32().ok_or_else(short)?;
            let content_type = r.u8().ok_or_else(short)?;
            let codec = r.u8().ok_or_else(short)?;
            let n = r.u16().ok_or_else(short)? as usize;
            let p = r.str(n).ok_or_else(short)?;
            match codec {
                CODEC_RAW if length != size => {
                    return Err(bad(format!("{p}: a stored entry of {length} bytes for {size}")));
                }
                CODEC_RAW | CODEC_ZSTD => {}
                c => return Err(bad(format!("{p}: codec {c}"))),
            }
            if content_type as usize >= types.len() {
                return Err(bad(format!("{p}: content type {content_type} of {}", types.len())));
            }
            index.insert(p, EntryRecord { offset: offset as u64, length, size, content_type, codec });
        }
        if r.at != raw.len() {
            return Err(bad(format!("{} bytes after the index's last entry", raw.len() - r.at)));
        }
        Ok(Pack { kind: kind.to_string(), path, file, types, index })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The entries' paths, in the pack's (path) order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn has(&self, path: &str) -> bool {
        self.index.contains_key(path)
    }

    pub fn record(&self, path: &str) -> Option<&EntryRecord> {
        self.index.get(path)
    }

    /// An entry's content type (`application/json`, ...).
    pub fn content_type(&self, path: &str) -> Option<&str> {
        self.index.get(path).map(|e| self.types[e.content_type as usize].as_str())
    }

    /// An entry's own bytes, whatever its codec.
    pub fn bytes(&self, path: &str) -> Result<Vec<u8>> {
        let e = *self
            .index
            .get(path)
            .ok_or_else(|| StoreError::NoEntry { kind: self.kind.clone(), path: path.to_string() })?;
        let io = |err| StoreError::Io(self.path.clone(), err);
        let mut stored = vec![0u8; e.length as usize];
        read_at(&self.file, &mut stored, e.offset).map_err(io)?;
        if e.codec == CODEC_RAW {
            return Ok(stored);
        }
        let out = zstd::bulk::decompress(&stored, e.size as usize)
            .map_err(|err| StoreError::Format(self.path.clone(), format!("{path}: zstd: {err}")))?;
        if out.len() != e.size as usize {
            return Err(StoreError::Format(
                self.path.clone(),
                format!("{path}: {} bytes decoded for {}", out.len(), e.size),
            ));
        }
        Ok(out)
    }
}

/// Fills `buf` from `file` at `offset` without moving a shared position: threads read one file at once.
fn read_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, buf, offset)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes(s.try_into().unwrap()))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|s| f64::from_le_bytes(s.try_into().unwrap()))
    }
    fn str(&mut self, n: usize) -> Option<String> {
        self.take(n).and_then(|s| String::from_utf8(s.to_vec()).ok())
    }
}

/// The developer's store folder: `CONTENT_STORE`, else `content/local.json`'s "store" (relative to `content/`; a
/// worktree's points at main's), else `content/.store` (the tree found from this crate's manifest directory at build
/// time). `content/` holds only these untracked files.
pub fn dev_store_dir() -> PathBuf {
    if let Some(s) = std::env::var_os("CONTENT_STORE") {
        return PathBuf::from(s);
    }
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let local = std::fs::read_to_string(content.join("local.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("store").and_then(|s| s.as_str()).map(PathBuf::from));
    match local {
        Some(s) => content.join(s),
        None => content.join(".store"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of the developer's store opens and its first and last entries read at their size. A tree with no
    /// store says so and passes.
    #[test]
    fn reads_the_dev_store() {
        let dir = dev_store_dir();
        let store = match Store::open(&dir) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no store to read ({e}): skipped");
                return;
            }
        };
        let mut read = 0;
        for kind in store.manifest().kinds.keys() {
            let pack = store.kind(kind).unwrap();
            assert_eq!(pack.len() as u64, store.manifest().kinds[kind].entries, "{kind}: entry count");
            for p in [pack.paths().next(), pack.paths().last()].into_iter().flatten() {
                let b = pack.bytes(p).unwrap();
                assert_eq!(b.len(), pack.record(p).unwrap().size as usize, "{kind} {p}");
                read += 1;
            }
        }
        eprintln!("{} kinds, {read} entries read", store.manifest().kinds.len());
    }

    /// A kind asked for twice is one pack; threads read it at once and each gets its entry.
    #[test]
    fn one_pack_read_by_threads() {
        let Ok(store) = Store::open(dev_store_dir()) else {
            eprintln!("no store to read: skipped");
            return;
        };
        let Some(kind) = store.manifest().kinds.keys().find(|k| store.manifest().kinds[*k].entries > 64).cloned()
        else {
            return;
        };
        let pack = store.kind(&kind).unwrap();
        assert!(Arc::ptr_eq(&pack, &store.clone().kind(&kind).unwrap()));
        let paths: Vec<String> = pack.paths().take(64).map(str::to_owned).collect();
        let want: Vec<Vec<u8>> = paths.iter().map(|p| pack.bytes(p).unwrap()).collect();
        std::thread::scope(|s| {
            for t in 0..4 {
                let (pack, paths, want) = (&pack, &paths, &want);
                s.spawn(move || {
                    for (i, p) in paths.iter().enumerate().skip(t) {
                        assert_eq!(&pack.bytes(p).unwrap(), &want[i], "{p}");
                    }
                });
            }
        });
    }
}
