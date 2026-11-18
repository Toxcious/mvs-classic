//! A kind of the store as the import builds it: what it lists from the build and the bytes it gives per entry. Each
//! kind is one [`Kind`] entered once in `kinds::KINDS`; the runner (`run`) and the development CLI's `kind` command
//! run its list, then its builds on all cores. The kinds are built from the paks and the game kinds (built from what
//! other kinds wrote into the store: `reads`).

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex, OnceLock};

use indexmap::IndexMap;
use mvs_pak::paths::split_object_path;
use mvs_pak::{BuildPaths, PakSet};
use mvs_uasset::typed::ObjectError;
use mvs_uasset::{Package, PackageCache, ReadError};

use crate::catalog::Catalog;
use crate::json::JsonError;
use crate::reads::StoreReads;

/// One asset of a kind as its list names it: the entry's path in the pack, and, for an entry not named by the object
/// it is built from (the shader kind's `/ShaderCode/<sha1>`), the object path its build reads (`Importer.source`).
/// The runner sends the items of one package (the source's, else the path's) to one task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: String,
    pub source: Option<String>,
}

impl Item {
    pub fn new(path: impl Into<String>) -> Item {
        Item { path: path.into(), source: None }
    }

    /// The package the item's build reads: its source's, else its own path's.
    pub fn package(&self) -> &str {
        split_object_path(self.source.as_deref().unwrap_or(&self.path)).0
    }
}

/// Why an entry was not built. Every one is a failure of its path (collected, counted in the kind's `failed`), never
/// fatal for the run.
#[derive(Clone, Debug, thiserror::Error)]
pub enum KindError {
    /// No such asset: no package at the path, no such export, or not of the kind.
    #[error("{0}")]
    NotFound(String),
    /// Something the port does not read yet (a kind's list or build not ported, a reader's `not ported`).
    #[error("not ported: {0}")]
    NotPorted(String),
    #[error("{0}")]
    Failed(String),
}

impl From<ReadError> for KindError {
    fn from(e: ReadError) -> KindError {
        match e {
            ReadError::NotPorted(m) => KindError::NotPorted(m),
            e => KindError::Failed(e.to_string()),
        }
    }
}

impl From<ObjectError> for KindError {
    fn from(e: ObjectError) -> KindError {
        KindError::Failed(e.0)
    }
}

impl From<JsonError> for KindError {
    fn from(e: JsonError) -> KindError {
        KindError::Failed(e.to_string())
    }
}

/// A path that failed, with its kind (`catalog` for a package whose header did not read) and why.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Failure {
    pub kind: String,
    pub path: String,
    pub message: String,
}

/// The import's phases, as progress names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Every package's header (the catalog).
    Catalog,
    /// A kind's list.
    List,
    /// A kind's entries built.
    Build,
    /// A kind's pack finished.
    Write,
    /// The manifest switched to name it.
    Switch,
}

/// One progress report: the phase, its kind (the list, build, write and switch phases), how far, and the kind's
/// stored bytes so far (the build phase).
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    pub phase: Phase,
    pub kind: Option<&'a str>,
    pub done: u64,
    pub total: u64,
    pub bytes: u64,
}

/// What one run of the import shares between its kinds and threads, built once on first ask by the first that
/// needs it: the material scan the shadermap and shader kinds' lists share, the global shader cache, the animation
/// kind's bone counts per skeleton. Keyed by the value's type (each kind's own type, in its own file), so a kind adds
/// one without touching this file. A second asker of the same type waits for the first's.
#[derive(Default)]
pub struct Shared {
    slots: Mutex<IndexMap<TypeId, Arc<Slot>>>,
}

/// One shared value, made once.
type Slot = OnceLock<Arc<dyn Any + Send + Sync>>;

impl Shared {
    /// The run's one value of type `T`, made by `init` on the first ask.
    pub fn get_or_init<T: Any + Send + Sync>(&self, init: impl FnOnce() -> T) -> Arc<T> {
        let slot = self.slots.lock().unwrap_or_else(|e| e.into_inner()).entry(TypeId::of::<T>()).or_default().clone();
        let v = slot.get_or_init(|| Arc::new(init())).clone();
        v.downcast::<T>().unwrap_or_else(|_| unreachable!("a slot holds its own type"))
    }
}

/// One value per key made once in a run, a failure kept as the value (the per-path memos of the helpers several
/// kinds call: a mesh's header, a particle system read whole). A helper holds it in a type of its own in
/// [`Shared`] (`struct Headers(Memo<..>)`); a second asker of a key waits for the first's.
pub struct Memo<V>(Mutex<IndexMap<String, Arc<OnceLock<V>>>>);

impl<V> Default for Memo<V> {
    fn default() -> Memo<V> {
        Memo(Mutex::new(IndexMap::new()))
    }
}

impl<V: Clone> Memo<V> {
    /// The value of `key`, made by `make` on the first ask.
    pub fn get(&self, key: &str, make: impl FnOnce() -> V) -> V {
        let slot = self.0.lock().unwrap_or_else(|e| e.into_inner()).entry(key.to_owned()).or_default().clone();
        slot.get_or_init(make).clone()
    }
}

/// What a kind's list and builds read: the paks, their mount table, the run's package
/// cache, the catalog, the run's shared values and the store's kinds. One for the list and the builds alike. The
/// build's objects are read as the object kind gives them through [`Cx::object`] (`objview.rs`).
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub paks: &'a Arc<PakSet>,
    pub paths: &'a BuildPaths,
    pub cache: &'a PackageCache,
    /// Every top-level export of the build (empty where only one entry is built: the CLI's `entry`).
    pub catalog: &'a Catalog,
    pub shared: &'a Shared,
    /// The kinds of the store a game kind reads (`Kind::reads`).
    pub store: &'a StoreReads,
}

impl Cx<'_> {
    /// The package and the top-level export a path names: `<package>.<export>`
    /// that export, a bare package path the export named as the package (UE's asset object: `/Game/A/B` is
    /// `/Game/A/B.B`).
    pub fn read(&self, path: &str) -> Result<(Arc<Package>, usize), KindError> {
        let (p, object) = split_object_path(path);
        let Some(pkg) = self.cache.package(p)? else {
            return Err(KindError::NotFound(format!("content: the paks hold no package {p}")));
        };
        match export_index_of(&pkg, path) {
            Some(i) => Ok((pkg, i)),
            None => {
                let names: Vec<&str> =
                    pkg.exports.iter().filter(|e| e.outer == 0).map(|e| e.object_name.as_str()).collect();
                Err(KindError::NotFound(format!(
                    "content: {p} has no top-level export {}; its exports: {}",
                    object.unwrap_or("(named as the package)"),
                    names.join(", ")
                )))
            }
        }
    }

    /// The class of export `index` of `pkg` as an object path, imports resolved;
    /// `(none)` without one, as the kinds' messages print it.
    pub fn class_of(&self, pkg: &Arc<Package>, index: usize) -> Result<String, KindError> {
        Ok(match self.cache.try_resolve(pkg, pkg.exports[index].class_index)? {
            Some(c) => c.object_path(self.cache),
            None => "(none)".into(),
        })
    }
}

/// The top-level export a path names: `.<export>` (the last of that exact name,
/// else the first without regard to case), for a bare path the export named as the package.
pub fn export_index_of(pkg: &Package, path: &str) -> Option<usize> {
    let (p, object) = split_object_path(path);
    let want = object.unwrap_or_else(|| &p[p.rfind('/').map_or(0, |s| s + 1)..]);
    let mut found = None;
    for (i, e) in pkg.exports.iter().enumerate() {
        if e.outer != 0 {
            continue;
        }
        if e.object_name == want || found.is_none() && e.object_name.to_lowercase() == want.to_lowercase() {
            found = Some(i);
        }
    }
    found
}

/// One kind of the store built from the paks.
pub trait Kind: Sync {
    /// Its id: the store's kind name and the pack's (`texture`).
    fn id(&self) -> &'static str;

    /// Bumped by every change to what the kind lists or gives: the import rebuilds the kind.
    fn version(&self) -> u64;

    /// The content type of an entry, kept with it in the pack.
    fn content_type(&self, bytes: &[u8]) -> &'static str;

    /// Every asset of the kind the import writes. May read packages and the paks.
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError>;

    /// The entry of one listed item: the bytes the game's loader of the kind reads.
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError>;

    /// The kinds whose entries it reads from the store (`cx.store`), each with the version it reads: the runner
    /// starts it once each is switched in by this run or held by the store at that version; else the kind fails
    /// whole ("needs <kind> v<n> in the store"). A kind built from the paks that it calls in-process (texture,
    /// lighting, config, shadermap) is no read.
    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[]
    }

    /// Written into the store by a plain import; `false` for a kind built only as the proof of its port, behind
    /// the runner's dev flag (`object`, `shadermap`: no reader in the game reads them).
    fn stored(&self) -> bool {
        true
    }
}

/// A list or build stub: the error its kind gives until its track ports it.
pub fn not_ported<T>(what: &str) -> Result<T, KindError> {
    Err(KindError::NotPorted(what.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_once_per_type() {
        struct Scan(u32);
        struct Other(&'static str);
        let s = Shared::default();
        let a = s.get_or_init(|| Scan(1));
        let b = s.get_or_init(|| Scan(2));
        assert_eq!((a.0, b.0), (1, 1));
        assert_eq!(s.get_or_init(|| Other("x")).0, "x");
    }

    #[test]
    fn memo_once_per_key() {
        let m = Memo::<Result<u32, KindError>>::default();
        assert_eq!(m.get("a", || Ok(1)).unwrap(), 1);
        assert_eq!(m.get("a", || Ok(2)).unwrap(), 1);
        assert!(matches!(m.get("b", || Err(KindError::NotFound("b".into()))), Err(KindError::NotFound(_))));
        assert!(m.get("b", || Ok(3)).is_err());
    }

    #[test]
    fn item_package() {
        assert_eq!(Item::new("/Game/A/B.B").package(), "/Game/A/B");
        let shader = Item { path: "/ShaderCode/0123".into(), source: Some("/Game/M/Mat.Mat".into()) };
        assert_eq!(shader.package(), "/Game/M/Mat");
    }
}
