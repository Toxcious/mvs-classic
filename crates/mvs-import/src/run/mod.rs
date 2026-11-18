//! The import: every stale kind of `kinds::KINDS` built from the paks into a store, once, up front (the launcher's
//! import screen, the CLI's `import`). One process with shared memory needs no worker machinery.
//!
//! Phases, each reported through the progress callback:
//! - catalog: every package's header (`crate::catalog`), scanned when the store's is stale, else read from it; written
//!   to the store and switched in before the kinds;
//! - list: every stale kind's list, one after another (each may run in parallel inside);
//! - build: the items of every kind cut into one task per source package (`tasks`: the largest, then stripes), run by every
//!   thread of one rayon pool over one package cache; each entry compressed on the thread that built it and sent to
//!   its kind's pack writer thread, the bytes in flight bounded (`memory::Gate`);
//! - write: a kind's pack finished once its last item has landed, checked and renamed into place;
//! - switch: the manifest replaced to name it, and what no manifest names removed (`mvs_store::manage`).
//!
//! A kind switched in stays: a failure later keeps every kind finished before it. A store of another build stays whole
//! until this build's catalog and every kind of the run are written, then one switch. An item that fails (an error or
//! a panic of its build) is collected with its message and counted in the kind's `failed`; a kind whose list fails or
//! whose pack does not write is not switched in (the store keeps its previous pack) and the other kinds go on; a
//! manifest that does not write ends the run. The entries do not depend on the thread count or the task order.
//!
//! Kinds built from the store (`Kind::reads`) run in a later wave than the kinds they read, and read them through
//! `Cx::store` as switched in. A kind whose reads the store does not hold at their versions when its wave starts (none
//! there, or its read failed in an earlier wave of this run) fails whole and keeps its record; the other kinds go on.
//! Its record keeps the pack files it read (as named when its wave starts): it is stale once one of them is rebuilt, by
//! this run or a later one (`mvs_store::manage::staleness`).

pub mod memory;
mod store_lock;
pub mod tasks;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Instant, SystemTime};

use indexmap::IndexMap;
use mvs_pak::{BuildPaths, PakSet, setup};
use mvs_store::manage::{Asked, clean_store, rename_retry, staleness, write_synced};
use mvs_store::writer::{PackWriter, encode_entry};
use mvs_store::{KindRecord, Manifest, STORE_FORMAT, StoreError, write_manifest};
use mvs_uasset::PackageCache;
use mvs_uasset::package::FileSource;

use crate::catalog::{CATALOG_VERSION, Catalog, scan_catalog};
use crate::kind::{Cx, Failure, Item, Kind, Phase, Progress, Shared};
use crate::kinds::KINDS;
use crate::reads::StoreReads;
pub use memory::Budget;
use memory::Gate;
use store_lock::StoreLock;

/// What one import run does.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// The store folder (made when missing).
    pub store: PathBuf,
    /// The kinds to build when stale (empty: every stored kind).
    pub kinds: Vec<String>,
    /// Build the kinds even when current.
    pub force: bool,
    /// Worker threads (`None`: the machine's logical cores).
    pub threads: Option<usize>,
    /// Only the items whose path starts with this: the kinds are recorded partial, stale for the next run.
    pub filter: Option<String>,
    /// Kinds not stored (`Kind::stored` false: `object`, `shadermap`) built too, into the store, as the proof of
    /// their ports (the CLI's `--dev-<id>`). Such a kind is built only when named here.
    pub unstored: Vec<String>,
    /// The build's id (`None`: the build list's).
    pub build_id: Option<String>,
    /// The memory bounds (`None`: from the memory free at the start).
    pub budget: Option<Budget>,
    /// Set from another thread to end the run (`ImportError::Cancelled`): checked before the catalog, between the
    /// waves and between the build tasks; a kind not written whole when it is seen is not switched in, the kinds
    /// switched in before it stay.
    pub cancel: Arc<AtomicBool>,
}

/// What became of a kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KindStatus {
    /// Written by this run and switched in.
    Built,
    /// In the store already, for this build and version.
    Current,
    /// Not written: why (its list failed, its pack did not write). The store keeps its previous pack.
    Failed(String),
}

/// One kind's outcome.
#[derive(Clone, Debug)]
pub struct KindSummary {
    pub kind: String,
    pub version: u64,
    pub status: KindStatus,
    pub entries: u64,
    /// The pack's bytes on disk.
    pub bytes: u64,
    pub failed: u64,
    pub seconds: f64,
}

/// A run's outcome.
#[derive(Clone, Debug)]
pub struct ImportSummary {
    pub build_id: String,
    /// In the registry's order.
    pub kinds: Vec<KindSummary>,
    /// The catalog's first, then by kind in the registry's order, then by path.
    pub failures: Vec<Failure>,
    /// The packages of the catalog (0 when no kind was stale).
    pub packages: usize,
    pub threads: usize,
    pub budget: Budget,
    /// The most bytes of entries in flight to the writers at once.
    pub entries_peak: u64,
    pub seconds: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("import: {0}")]
    Kinds(String),
    #[error("import: {0}")]
    Busy(String),
    #[error("import: {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("import: {0}")]
    Store(#[from] StoreError),
    #[error("import: the paks: {0}")]
    Paks(String),
    /// `ImportOptions::cancel` was set.
    #[error("import: cancelled")]
    Cancelled,
}

/// Imports every stale kind of the registry into `opts.store`; `progress` hears each phase, from any thread.
pub fn import(
    paks: &Arc<PakSet>,
    opts: &ImportOptions,
    progress: &(dyn Fn(Progress<'_>) + Sync),
) -> Result<ImportSummary, ImportError> {
    let paths = Arc::new(BuildPaths::from_paks(paks).map_err(|e| ImportError::Paks(e.to_string()))?);
    let build_id = match &opts.build_id {
        Some(b) => b.clone(),
        None => setup::build_id().map_err(|e| ImportError::Paks(e.to_string()))?,
    };
    import_kinds(paks, &paths, KINDS, &build_id, opts, progress)
}

/// What a dead import left in the store in `dir` removed (its temp files, and the packs and catalogs no manifest
/// names): the store's lock taken, over a dead holder's (a killed or crashed run), then `clean_store` against the
/// manifest. [`ImportError::Busy`] while a live import holds the lock; a manifest that does not read is an error and
/// nothing is removed. The names removed; none when there is no store folder.
pub fn clean(dir: &Path) -> Result<Vec<String>, ImportError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let _lock = StoreLock::take(dir)?;
    let m = mvs_store::manage::read_manifest(dir)?;
    Ok(clean_store(dir, m.as_ref(), &|_| false)?)
}

/// A run's name in its files: the time in base 36 and six hex digits.
fn run_id() -> String {
    let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let mut ms = t.as_millis() as u64;
    let mut s = Vec::new();
    while ms > 0 || s.is_empty() {
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(ms % 36) as usize]);
        ms /= 36;
    }
    s.reverse();
    let tail = (u64::from(t.subsec_nanos()) ^ u64::from(std::process::id()).wrapping_mul(0x9e37_79b9)) & 0xff_ffff;
    format!("{}{tail:06x}", String::from_utf8_lossy(&s))
}

/// What the manifest holds next, and when it is written.
struct Switch<'a> {
    dir: &'a Path,
    state: Mutex<SwitchState>,
    /// This run's packs being written or written and not yet switched in: no switch removes them.
    writing: Mutex<Vec<String>>,
    /// What the later waves read: each kind switched in (deferred or not) is entered.
    reads: &'a StoreReads,
}

struct SwitchState {
    next: Manifest,
    /// Another build's store: every piece waits for one switch at the end of the first wave.
    deferred: bool,
}

impl Switch<'_> {
    /// Changes the next manifest and, unless deferred, writes it and cleans the folder against it.
    fn switch_in(&self, change: impl FnOnce(&mut Manifest)) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut s.next);
        if s.deferred {
            return Ok(());
        }
        self.write(&s.next)
    }

    fn write(&self, m: &Manifest) -> Result<(), StoreError> {
        write_manifest(self.dir, m)?;
        // (asked per name, after the folder is read: a pack that starts being written meanwhile stays)
        clean_store(self.dir, Some(m), &|n| {
            self.writing.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|w| w == n)
        })?;
        Ok(())
    }

    /// The deferred switch: the manifest written once.
    fn undefer(&self) -> Result<(), StoreError> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.deferred {
            return Ok(());
        }
        s.deferred = false;
        self.write(&s.next)
    }

    fn deferred(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).deferred
    }

    fn writing(&self, names: [&str; 2], on: bool) {
        let mut w = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        if on {
            w.extend(names.iter().map(|n| n.to_string()));
        } else {
            w.retain(|n| !names.contains(&n.as_str()));
        }
    }
}

/// The text of a panic's payload.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default()
}

/// The catalog file's shape (the store's catalog, as `Catalog` serializes it).
#[derive(serde::Deserialize)]
struct CatalogFile {
    classes: Vec<String>,
    packages: Vec<(String, Vec<(String, u32)>)>,
}

/// [`import`] over the kinds of `registry` (the tests' kinds), the paks' mount table and the build id given.
pub fn import_kinds(
    paks: &Arc<PakSet>,
    paths: &Arc<BuildPaths>,
    registry: &[&dyn Kind],
    build_id: &str,
    opts: &ImportOptions,
    progress: &(dyn Fn(Progress<'_>) + Sync),
) -> Result<ImportSummary, ImportError> {
    let t0 = Instant::now();
    memory::lower_priority();
    let dir = opts.store.as_path();
    // the kinds of the run, in the registry's order
    for id in opts.kinds.iter().chain(&opts.unstored) {
        let Some(k) = registry.iter().find(|k| k.id() == id) else {
            let ids: Vec<&str> = registry.iter().map(|k| k.id()).collect();
            return Err(ImportError::Kinds(format!("no kind {id}; the kinds: {}", ids.join(", "))));
        };
        if !k.stored() && !opts.unstored.contains(id) {
            return Err(ImportError::Kinds(format!(
                "the {id} kind is not stored: built only as a dev proof (--dev-{id})"
            )));
        }
    }
    let chosen: Vec<&dyn Kind> = registry
        .iter()
        .copied()
        .filter(|k| {
            let id = &k.id().to_string();
            opts.unstored.contains(id) || opts.kinds.contains(id) || (opts.kinds.is_empty() && k.stored())
        })
        .collect();

    std::fs::create_dir_all(dir).map_err(|e| ImportError::Io(dir.to_path_buf(), e))?;
    let _lock = StoreLock::take(dir)?;
    let asked: Vec<Asked<'_>> =
        chosen.iter().map(|k| Asked { id: k.id(), version: k.version(), reads: k.reads() }).collect();
    let st = staleness(dir, build_id, CATALOG_VERSION, &asked, opts.force);
    let live = st.manifest.as_ref().filter(|m| m.build_id == build_id);
    clean_store(dir, st.manifest.as_ref(), &|_| false)?;
    let stale: Vec<&dyn Kind> = chosen.iter().copied().filter(|k| st.kinds.iter().any(|s| s == k.id())).collect();
    let budget = opts.budget.unwrap_or_else(Budget::now);
    let pool = {
        let mut b = rayon::ThreadPoolBuilder::new().thread_name(|i| format!("import {i}"));
        if let Some(n) = opts.threads {
            b = b.num_threads(n.max(1));
        }
        b.build().map_err(|e| ImportError::Kinds(format!("the thread pool: {e}")))?
    };
    let mut summary = ImportSummary {
        build_id: build_id.to_string(),
        kinds: Vec::new(),
        failures: Vec::new(),
        packages: 0,
        threads: pool.current_num_threads(),
        budget,
        entries_peak: 0,
        seconds: 0.0,
    };
    for k in &chosen {
        if stale.iter().any(|s| s.id() == k.id()) {
            continue;
        }
        let rec = &live.expect("a current kind is in this build's manifest").kinds[k.id()];
        summary.kinds.push(KindSummary {
            kind: k.id().into(),
            version: k.version(),
            status: KindStatus::Current,
            entries: rec.entries,
            bytes: rec.bytes,
            failed: rec.failed,
            seconds: 0.0,
        });
    }
    // a kind built from the store runs after each kind it reads that this run builds (at the version it reads: else
    // the registry is wrong). One this run does not build is read as the store holds it, checked as its wave starts.
    let mut wave: Vec<usize> = vec![0; stale.len()];
    for _ in 0..=stale.len() {
        for (i, k) in stale.iter().enumerate() {
            for &(r, v) in k.reads() {
                if let Some(j) = stale.iter().position(|s| s.id() == r) {
                    if stale[j].version() != v {
                        return Err(ImportError::Kinds(format!(
                            "the {} kind reads the {r} kind at version {v}; this run builds it at version {}",
                            k.id(),
                            stale[j].version()
                        )));
                    }
                    wave[i] = wave[i].max(wave[j] + 1);
                }
            }
        }
    }
    if wave.iter().any(|&w| w >= stale.len().max(1)) {
        return Err(ImportError::Kinds("the kinds' reads make a loop".into()));
    }
    if stale.is_empty() && !st.catalog {
        summary.seconds = t0.elapsed().as_secs_f64();
        return Ok(summary);
    }

    let reads = StoreReads::new(dir, live);
    let switch = Switch {
        dir,
        reads: &reads,
        state: Mutex::new(SwitchState {
            next: live.cloned().unwrap_or_else(|| Manifest {
                format: STORE_FORMAT,
                build_id: build_id.to_string(),
                catalog: None,
                catalog_version: None,
                kinds: Default::default(),
            }),
            deferred: st.new_build,
        }),
        writing: Mutex::new(Vec::new()),
    };
    let run = run_id();
    let cancelled = || opts.cancel.load(Ordering::Relaxed);
    if cancelled() {
        return Err(ImportError::Cancelled);
    }

    // catalog
    let catalog = if st.catalog {
        let (c, failed) = pool.install(|| scan_catalog(paks, paths, progress));
        summary.failures.extend(failed);
        let name = format!("catalog-{run}.json");
        let bytes = crate::json::to_vec(&c).map_err(|e| ImportError::Kinds(format!("the catalog: {e}")))?;
        let tmp = dir.join(format!("{name}.tmp"));
        switch.writing([&name, &format!("{name}.tmp")], true);
        write_synced(&tmp, &bytes)?;
        rename_retry(&tmp, &dir.join(&name))?;
        let r = switch.switch_in(|m| {
            m.catalog = Some(name.clone());
            m.catalog_version = Some(CATALOG_VERSION);
        });
        switch.writing([&name, &format!("{name}.tmp")], false);
        r?;
        c
    } else {
        let file = dir.join(live.and_then(|m| m.catalog.as_ref()).expect("a current catalog is named"));
        let text = std::fs::read(&file).map_err(|e| ImportError::Io(file.clone(), e))?;
        let c: CatalogFile = serde_json::from_slice(&text)
            .map_err(|e| ImportError::Store(StoreError::Format(file.clone(), format!("not a catalog: {e}"))))?;
        let n = c.packages.len() as u64;
        progress(Progress { phase: Phase::Catalog, kind: None, done: n, total: n, bytes: 0 });
        Catalog { classes: c.classes, packages: c.packages }
    };
    summary.packages = catalog.packages.len();
    if cancelled() {
        return Err(ImportError::Cancelled);
    }

    let cache = PackageCache::new(paks.clone() as Arc<dyn FileSource>, paths.clone(), budget.cache);
    let shared = Shared::default();
    let cx = Cx { paks, paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
    let gate = Gate::new(budget.entries);
    let failures = Mutex::new(std::mem::take(&mut summary.failures));
    let waves = wave.iter().max().map_or(0, |w| w + 1);
    let mut skipped = false;
    // the kinds that failed in an earlier wave: a kind reading one fails whole (the store's pack of it is not the one
    // this run meant it to read)
    let mut failed: Vec<String> = Vec::new();
    for w in 0..waves {
        if cancelled() {
            return Err(ImportError::Cancelled);
        }
        let ks: Vec<&dyn Kind> = stale.iter().zip(&wave).filter(|(_, x)| **x == w).map(|(k, _)| *k).collect();
        let mut done = run_wave(&ks, &cx, &pool, &gate, &switch, &run, opts, &failed, &failures, progress)?;
        failed.extend(done.iter().filter(|k| matches!(k.status, KindStatus::Failed(_))).map(|k| k.kind.clone()));
        if cancelled() {
            return Err(ImportError::Cancelled);
        }
        // another build's store: switched in once, after the kinds that read none, and only when every kind of that
        // wave is written (at the first failure: the old build stays whole, the packs of
        // this run are left for the next run's clean)
        if switch.deferred() {
            if done.iter().all(|k| k.status == KindStatus::Built) {
                switch.undefer()?;
            } else {
                let why = "another kind failed: the store of the other build stays whole";
                for k in done.iter_mut().filter(|k| k.status == KindStatus::Built) {
                    k.status = KindStatus::Failed(format!("not switched in; {why}"));
                }
                summary.kinds.extend(done);
                for (k, _) in stale.iter().zip(&wave).filter(|(_, x)| **x > w) {
                    summary.kinds.push(KindSummary {
                        kind: k.id().into(),
                        version: k.version(),
                        status: KindStatus::Failed(format!("not run; {why}")),
                        entries: 0,
                        bytes: 0,
                        failed: 0,
                        seconds: 0.0,
                    });
                }
                skipped = true;
                break;
            }
        }
        summary.kinds.extend(done);
    }
    if cancelled() {
        return Err(ImportError::Cancelled);
    }
    if waves == 0 && !skipped {
        switch.undefer()?;
    }
    summary.entries_peak = gate.peak();

    let order = |k: &str| registry.iter().position(|r| r.id() == k);
    summary.kinds.sort_by_key(|k| order(&k.kind));
    let mut failures = failures.into_inner().unwrap_or_else(|e| e.into_inner());
    failures.sort_by(|a, b| order(&a.kind).cmp(&order(&b.kind)).then_with(|| a.path.cmp(&b.path)));
    summary.failures = failures;
    summary.seconds = t0.elapsed().as_secs_f64();
    Ok(summary)
}

/// An entry on its way to its kind's writer, or the kind's end (every item landed).
enum Msg {
    Entry { path: String, content_type: &'static str, data: Vec<u8>, codec: u8, size: u32 },
    End,
}

/// One kind in its wave.
struct KindRun<'k> {
    kind: &'k dyn Kind,
    list: Vec<Item>,
    remaining: AtomicUsize,
    done: AtomicU64,
    failed: AtomicU64,
    /// The stored bytes sent to the writer so far.
    bytes: AtomicU64,
    /// Its writer failed: its builds are skipped.
    dead: AtomicBool,
    started: Instant,
    /// The pack file of each kind it reads, as the store named it when its wave started (its record's `reads`).
    reads: IndexMap<String, String>,
}

impl KindRun<'_> {
    /// An entry did not encode: counted, and the kind is dropped (its pack is not switched in).
    fn encode_failed(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        self.dead.store(true, Ordering::Relaxed);
    }
}

/// Lists, builds, writes and switches in one wave's kinds; their summaries.
#[allow(clippy::too_many_arguments)]
fn run_wave(
    ks: &[&dyn Kind],
    cx: &Cx<'_>,
    pool: &rayon::ThreadPool,
    gate: &Gate,
    switch: &Switch<'_>,
    run: &str,
    opts: &ImportOptions,
    failed: &[String],
    failures: &Mutex<Vec<Failure>>,
    progress: &(dyn Fn(Progress<'_>) + Sync),
) -> Result<Vec<KindSummary>, ImportError> {
    let started = Instant::now();
    let fail = |kind: &str, path: &str, message: String| {
        failures.lock().unwrap_or_else(|e| e.into_inner()).push(Failure {
            kind: kind.into(),
            path: path.into(),
            message,
        })
    };
    let mut out = Vec::new();
    // a kind whose reads the store does not hold (none there, or at another version), or one of which failed in an
    // earlier wave, fails whole
    let ks: Vec<&dyn Kind> = ks
        .iter()
        .copied()
        .filter(|k| {
            let ok = match k.reads().iter().find(|(r, _)| failed.iter().any(|f| f == r)) {
                Some((r, _)) => Err(format!("its read {r} failed in this run")),
                None => cx.store.check(k.reads()),
            };
            match ok {
                Ok(()) => true,
                Err(why) => {
                    out.push(KindSummary {
                        kind: k.id().into(),
                        version: k.version(),
                        status: KindStatus::Failed(why),
                        entries: 0,
                        bytes: 0,
                        failed: 0,
                        seconds: 0.0,
                    });
                    false
                }
            }
        })
        .collect();
    // list: one kind after another, each on the whole pool. Two lists at once could deadlock: a list that makes a
    // shared value (`Shared::get_or_init`: the material scan of shadermap and shader) runs a parallel loop inside
    // its initializer, and a thread waiting on that loop may steal the other list's job, which waits on the same
    // initializer, on the same thread.
    let lists: Vec<Result<Vec<Item>, String>> = pool.install(|| {
        ks.iter()
            .map(|k| {
                progress(Progress { phase: Phase::List, kind: Some(k.id()), done: 0, total: 1, bytes: 0 });
                let r = match catch_unwind(AssertUnwindSafe(|| k.list(cx))) {
                    Ok(Ok(mut l)) => {
                        if let Some(f) = &opts.filter {
                            l.retain(|i| i.path.starts_with(f.as_str()));
                        }
                        Ok(l)
                    }
                    Ok(Err(e)) => Err(format!("its list: {e}")),
                    Err(p) => Err(format!("its list panicked: {}", panic_text(&*p))),
                };
                progress(Progress { phase: Phase::List, kind: Some(k.id()), done: 1, total: 1, bytes: 0 });
                r
            })
            .collect()
    });
    let mut runs: Vec<KindRun<'_>> = Vec::new();
    for (k, l) in ks.iter().zip(lists) {
        match l {
            Ok(list) => runs.push(KindRun {
                kind: *k,
                remaining: AtomicUsize::new(list.len()),
                list,
                done: AtomicU64::new(0),
                failed: AtomicU64::new(0),
                bytes: AtomicU64::new(0),
                dead: AtomicBool::new(false),
                started,
                reads: cx.store.files(k.reads()),
            }),
            Err(why) => out.push(KindSummary {
                kind: k.id().into(),
                version: k.version(),
                status: KindStatus::Failed(why),
                entries: 0,
                bytes: 0,
                failed: 0,
                seconds: started.elapsed().as_secs_f64(),
            }),
        }
    }
    let lists: Vec<&[Item]> = runs.iter().map(|r| r.list.as_slice()).collect();
    let tasks = tasks::cut(&lists, cx.paks, cx.paths, pool.current_num_threads());

    // a manifest that did not write: the run ends
    let fatal: Mutex<Option<StoreError>> = Mutex::new(None);
    let abort = AtomicBool::new(false);
    let built = std::thread::scope(|scope| {
        let mut senders = Vec::with_capacity(runs.len());
        let mut writers = Vec::with_capacity(runs.len());
        for kr in &runs {
            let (tx, rx) = mpsc::channel::<Msg>();
            let id = kr.kind.id();
            progress(Progress { phase: Phase::Build, kind: Some(id), done: 0, total: kr.list.len() as u64, bytes: 0 });
            if kr.list.is_empty() {
                let _ = tx.send(Msg::End);
            }
            senders.push(tx);
            let (fatal, abort) = (&fatal, &abort);
            writers.push(
                std::thread::Builder::new()
                    .name(format!("pack {id}"))
                    .spawn_scoped(scope, move || write_kind(kr, rx, gate, switch, run, opts, progress, fatal, abort)),
            );
        }
        let next = AtomicUsize::new(0);
        pool.broadcast(|_| {
            loop {
                if abort.load(Ordering::Relaxed) || opts.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Some(task) = tasks.get(next.fetch_add(1, Ordering::Relaxed)) else { break };
                for &(k, i) in &task.items {
                    let kr = &runs[k];
                    let id = kr.kind.id();
                    let item = &kr.list[i];
                    if !kr.dead.load(Ordering::Relaxed) {
                        match catch_unwind(AssertUnwindSafe(|| kr.kind.build(cx, item))) {
                            Ok(Ok(b)) => {
                                let content_type = kr.kind.content_type(&b);
                                match (encode_entry(&b, content_type), u32::try_from(b.len())) {
                                    (Ok((data, codec)), Ok(size)) => {
                                        let data = match data {
                                            Cow::Owned(d) => d,
                                            Cow::Borrowed(_) => b,
                                        };
                                        let n = data.len() as u64;
                                        gate.take(n);
                                        kr.bytes.fetch_add(n, Ordering::Relaxed);
                                        let msg =
                                            Msg::Entry { path: item.path.clone(), content_type, data, codec, size };
                                        // (a writer gone has failed, and says so)
                                        if senders[k].send(msg).is_err() {
                                            gate.give(n);
                                        }
                                    }
                                    // an entry that does not encode drops the kind (`PackWriter.add` throws)
                                    (Err(e), _) => {
                                        kr.encode_failed();
                                        fail(id, &item.path, e.to_string());
                                    }
                                    (_, Err(_)) => {
                                        kr.encode_failed();
                                        fail(id, &item.path, format!("an entry of {} bytes", b.len()));
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                kr.failed.fetch_add(1, Ordering::Relaxed);
                                fail(id, &item.path, e.to_string());
                            }
                            Err(p) => {
                                kr.failed.fetch_add(1, Ordering::Relaxed);
                                fail(id, &item.path, format!("its build panicked: {}", panic_text(&*p)));
                            }
                        }
                    }
                    let total = kr.list.len() as u64;
                    let done = kr.done.fetch_add(1, Ordering::Relaxed) + 1;
                    if done.is_multiple_of(64) || done == total {
                        let bytes = kr.bytes.load(Ordering::Relaxed);
                        progress(Progress { phase: Phase::Build, kind: Some(id), done, total, bytes });
                    }
                    // the last item of the kind: every entry is sent before its end
                    if kr.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                        let _ = senders[k].send(Msg::End);
                    }
                }
                cx.cache.trim();
            }
        });
        drop(senders);
        writers
            .into_iter()
            .map(|w| match w.map(|h| h.join()) {
                Ok(Ok(s)) => s,
                Ok(Err(p)) => Err(format!("its writer panicked: {}", panic_text(&*p))),
                Err(e) => Err(format!("its writer thread: {e}")),
            })
            .collect::<Vec<_>>()
    });
    if let Some(e) = fatal.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e.into());
    }
    for (kr, r) in runs.iter().zip(built) {
        let failed = kr.failed.load(Ordering::Relaxed);
        let seconds = kr.started.elapsed().as_secs_f64();
        let (kind, version) = (kr.kind.id().to_string(), kr.kind.version());
        out.push(match r {
            Ok((rec, seconds)) => KindSummary {
                kind,
                version,
                status: KindStatus::Built,
                entries: rec.entries,
                bytes: rec.bytes,
                failed,
                seconds,
            },
            Err(why) => {
                KindSummary { kind, version, status: KindStatus::Failed(why), entries: 0, bytes: 0, failed, seconds }
            }
        });
    }
    Ok(out)
}

/// One kind's writer thread: appends its entries, then finishes the pack and switches it in once every item has
/// landed. A write that fails marks the kind dead (its builds are skipped) and drains what is still sent.
#[allow(clippy::too_many_arguments)]
fn write_kind(
    kr: &KindRun<'_>,
    rx: mpsc::Receiver<Msg>,
    gate: &Gate,
    switch: &Switch<'_>,
    run: &str,
    opts: &ImportOptions,
    progress: &(dyn Fn(Progress<'_>) + Sync),
    fatal: &Mutex<Option<StoreError>>,
    abort: &AtomicBool,
) -> Result<(KindRecord, f64), String> {
    let id = kr.kind.id();
    let name = format!("{id}@{}-{run}.pack", kr.kind.version());
    let tmp = format!("{name}.tmp");
    switch.writing([&name, &tmp], true);
    let mut err: Option<String> = None;
    let mut writer = match PackWriter::create(&switch.dir.join(&name)) {
        Ok(w) => Some(w),
        Err(e) => {
            err = Some(format!("its pack: {e}"));
            kr.dead.store(true, Ordering::Relaxed);
            None
        }
    };
    let mut ended = false;
    for msg in rx.iter() {
        match msg {
            Msg::Entry { path, content_type, data, codec, size } => {
                if let Some(w) = writer.as_mut()
                    && let Err(e) = w.add_stored(&path, content_type, &data, codec, size)
                {
                    err = Some(format!("its pack: {e}"));
                    writer = None;
                }
                gate.give(data.len() as u64);
            }
            Msg::End => {
                ended = true;
                break;
            }
        }
        if writer.is_none() {
            kr.dead.store(true, Ordering::Relaxed);
        }
    }
    let r = (|| {
        if let Some(e) = err {
            return Err(e);
        }
        if kr.dead.load(Ordering::Relaxed) && writer.is_some() {
            return Err("an entry did not encode".to_string());
        }
        let w = writer.ok_or("its pack was not written")?;
        if !ended {
            return Err("the run ended before its builds".to_string());
        }
        if opts.cancel.load(Ordering::Relaxed) {
            return Err("the run was cancelled".to_string());
        }
        progress(Progress { phase: Phase::Write, kind: Some(id), done: 0, total: 1, bytes: 0 });
        let sum = w.finish().map_err(|e| format!("its pack: {e}"))?;
        progress(Progress { phase: Phase::Write, kind: Some(id), done: 1, total: 1, bytes: sum.bytes });
        let rec = KindRecord {
            version: kr.kind.version(),
            file: name.clone(),
            entries: sum.entries,
            bytes: sum.bytes,
            failed: kr.failed.load(Ordering::Relaxed),
            partial: opts.filter.is_some(),
            reads: kr.reads.clone(),
        };
        progress(Progress { phase: Phase::Switch, kind: Some(id), done: 0, total: 1, bytes: 0 });
        if let Err(e) = switch.switch_in(|m| {
            m.kinds.insert(id.to_string(), rec.clone());
        }) {
            let why = format!("the manifest: {e}");
            fatal.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
            abort.store(true, Ordering::Relaxed);
            return Err(why);
        }
        switch.reads.switched(id, &rec);
        progress(Progress { phase: Phase::Switch, kind: Some(id), done: 1, total: 1, bytes: 0 });
        Ok((rec, kr.started.elapsed().as_secs_f64()))
    })();
    // what is still sent once this kind is dead: drained, its bytes given back
    if r.is_err() {
        for msg in rx.iter() {
            if let Msg::Entry { data, .. } = msg {
                gate.give(data.len() as u64);
            }
        }
    }
    switch.writing([&name, &tmp], false);
    r
}
