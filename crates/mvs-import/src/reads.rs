//! What a game kind reads of the store it is imported into (`Kind::reads`): the entries of other kinds by kind and
//! path, the kinds switched in before it in the run, and those the store held when the run started (until the Rust
//! importer builds them, the earlier packs in a mixed store: gamedata, scenery, the namers of materials).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use mvs_store::manage::read_manifest;
use mvs_store::{KindRecord, Manifest, Pack, StoreError};

use crate::kind::KindError;

/// A store's packs as the game kinds read them. Each pack is opened once, on first ask.
#[derive(Default)]
pub struct StoreReads {
    dir: Option<PathBuf>,
    kinds: Mutex<IndexMap<String, KindRecord>>,
    packs: Mutex<IndexMap<String, Arc<Pack>>>,
}

impl StoreReads {
    /// No store: every read fails (a kind that reads none needs none).
    pub fn none() -> StoreReads {
        StoreReads::default()
    }

    /// The kinds `manifest` names in the store folder `dir` (`None`: a store with none yet).
    pub fn new(dir: &Path, manifest: Option<&Manifest>) -> StoreReads {
        StoreReads {
            dir: Some(dir.to_path_buf()),
            kinds: Mutex::new(manifest.map(|m| m.kinds.clone()).unwrap_or_default()),
            packs: Mutex::default(),
        }
    }

    /// The store in `dir` as its manifest names it now.
    pub fn open(dir: &Path) -> Result<StoreReads, StoreError> {
        Ok(StoreReads::new(dir, read_manifest(dir)?.as_ref()))
    }

    /// A kind the run has written and switched in: later reads of it read the new pack.
    pub fn switched(&self, kind: &str, rec: &KindRecord) {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner()).insert(kind.to_string(), rec.clone());
        self.packs.lock().unwrap_or_else(|e| e.into_inner()).shift_remove(kind);
    }

    /// The version of `kind` the store holds.
    pub fn version(&self, kind: &str) -> Option<u64> {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner()).get(kind).map(|r| r.version)
    }

    /// Whether the store holds every kind of `reads` at its version; else why not, for the first one it lacks.
    pub fn check(&self, reads: &[(&str, u64)]) -> Result<(), String> {
        for &(kind, version) in reads {
            match self.version(kind) {
                Some(v) if v == version => {}
                Some(v) => return Err(format!("needs {kind} v{version} in the store, which holds v{v}")),
                None => return Err(format!("needs {kind} v{version} in the store")),
            }
        }
        Ok(())
    }

    /// The pack file of each kind of `reads` the store names now (a kind built from them keeps it in its record:
    /// `KindRecord::reads`); a kind it does not hold is left out.
    pub fn files(&self, reads: &[(&str, u64)]) -> IndexMap<String, String> {
        let kinds = self.kinds.lock().unwrap_or_else(|e| e.into_inner());
        reads.iter().filter_map(|(k, _)| Some((k.to_string(), kinds.get(*k)?.file.clone()))).collect()
    }

    /// The pack of `kind`.
    pub fn pack(&self, kind: &str) -> Result<Arc<Pack>, KindError> {
        if let Some(p) = self.packs.lock().unwrap_or_else(|e| e.into_inner()).get(kind) {
            return Ok(p.clone());
        }
        let file = self.kinds.lock().unwrap_or_else(|e| e.into_inner()).get(kind).map(|r| r.file.clone());
        let (Some(dir), Some(file)) = (&self.dir, file) else {
            return Err(KindError::Failed(format!("needs the {kind} kind in the store")));
        };
        let p = Arc::new(Pack::open(kind, dir.join(file)).map_err(|e| KindError::Failed(e.to_string()))?);
        Ok(self.packs.lock().unwrap_or_else(|e| e.into_inner()).entry(kind.to_string()).or_insert(p).clone())
    }

    /// The bytes of entry `path` of `kind`; `NotFound` when the pack has none.
    pub fn entry(&self, kind: &str, path: &str) -> Result<Vec<u8>, KindError> {
        self.pack(kind)?.bytes(path).map_err(|e| match e {
            StoreError::NoEntry { .. } => KindError::NotFound(e.to_string()),
            e => KindError::Failed(e.to_string()),
        })
    }

    /// The entries' paths of `kind`, in the pack's (path) order.
    pub fn paths(&self, kind: &str) -> Result<Vec<String>, KindError> {
        Ok(self.pack(kind)?.paths().map(str::to_owned).collect())
    }
}
