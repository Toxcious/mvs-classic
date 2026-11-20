//! The worker's side: its ops, the setup record, the run's events.
//!
//! - run: the setup check of the beta folder when setup.json is missing, names another build or another folder, or
//!   the folder's paks changed size (then setup.json is written); the key read again from the exe beside the paks;
//!   the import (mvs_import::run::import, as the CLI's) into `<data root>/store`. Its events go out as they come,
//!   a few a second per stream; `cancel` stops it at the next hash piece or build task. A run that died (killed,
//!   crashed) is cleaned by the next run's start (mvs_import::run::import takes the dead lock over and removes its
//!   temp files); the kinds it switched in stay.
//! - state: setup.json still holds (build, folder, sizes, exe) and the store's staleness over every stored kind.
//! - detect: steam.rs's candidates, each checked.
//! - check: mvs_pak::setup::source_check of one folder.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use mvs_import::kind::{Phase, Progress};
use mvs_import::run::{Budget, ImportError, ImportOptions, ImportSummary, KindStatus, memory::free_memory};
use mvs_pak::setup::{self, BuildList, SourceCheck};
use mvs_pak::{PakError, PakSet};
use mvs_ui::launcher::import_view::{
    CheckWhy, ImportEvent, ImportPhase, ImportState, KindSummary, LogLevel, Say, SourceInfo, SourceKind, Why,
};

use super::{ImportRequest, WorkerOp, store_dir};

/// setup.json: the beta folder checked for a build.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetupRecord {
    pub build_id: String,
    pub paks: PathBuf,
    /// When the check passed, UTC, ISO 8601.
    pub checked_at: String,
}

/// setup.json's place in the data root.
pub fn setup_file(root: &Path) -> PathBuf {
    root.join("setup.json")
}

/// setup.json, or None when there is none or it does not read.
pub fn read_setup(root: &Path) -> Option<SetupRecord> {
    serde_json::from_slice(&std::fs::read(setup_file(root)).ok()?).ok()
}

fn write_setup(root: &Path, rec: &SetupRecord) -> std::io::Result<()> {
    let file = setup_file(root);
    std::fs::create_dir_all(root)?;
    let part = root.join("setup.json.part");
    let mut text = serde_json::to_vec_pretty(rec).map_err(std::io::Error::other)?;
    text.push(b'\n');
    std::fs::write(&part, text)?;
    std::fs::rename(&part, &file)
}

/// A time as UTC ISO 8601 with milliseconds (Date.toISOString).
pub fn iso_utc(t: SystemTime) -> String {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // (civil from days: H. Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// A path made absolute (path.resolve).
fn resolve(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn same_path(a: &Path, b: &Path) -> bool {
    resolve(a).to_string_lossy().to_lowercase() == resolve(b).to_string_lossy().to_lowercase()
}

/// Whether `child` is `parent` or lies inside it, without regard to case.
fn inside(child: &Path, parent: &Path) -> bool {
    let c = child.to_string_lossy().to_lowercase().replace('/', "\\");
    let p = parent.to_string_lossy().to_lowercase().replace('/', "\\");
    let p = p.trim_end_matches('\\');
    c == p || c.strip_prefix(p).is_some_and(|rest| rest.starts_with('\\'))
}

/// Why a data root cannot be used with the paks folder `paks`: it is the beta's install folder or inside it (the
/// paks folder too), by the paths as given and as they really are; None when it can.
pub fn root_inside_beta(data_root: &Path, paks: &Path) -> Option<Why> {
    let real = super::steam::real_path;
    let install = setup::install_root(paks);
    for root in [resolve(data_root), real(data_root)] {
        for beta in [resolve(&install), real(&install), resolve(paks), real(paks)] {
            if inside(&root, &beta) {
                return Some(Why::RootInsideBeta { root: data_root.to_path_buf(), beta: install });
            }
        }
    }
    None
}

/// Why the folder setup.json names no longer holds this build, by name and size; None when it does.
fn folder_changed(paks: &Path, build: &BuildList) -> Option<Why> {
    let p = || paks.to_path_buf();
    let Ok(cmp) = setup::compare_paks(paks, build) else {
        return Some(Why::BetaGone { paks: p() });
    };
    if !cmp.missing.is_empty() {
        return Some(Why::PaksMissing { paks: p(), count: cmp.missing.len() });
    }
    if !cmp.extra.is_empty() {
        return Some(Why::PaksExtra { paks: p() });
    }
    if let Some(w) = cmp.wrong_size.first() {
        return Some(Why::PakResized { paks: p(), file: w.0.to_string() });
    }
    if !setup::exe_beside_paks(paks).exists() {
        return Some(Why::ExeGone { paks: p() });
    }
    let gone = setup::missing_build_files(paks, build);
    if !gone.is_empty() {
        return Some(Why::FilesGone { files: gone.iter().map(|f| f.to_string()).collect() });
    }
    None
}

/// Why setup does not hold for this build (no hashing: the folder by name and size); None when it does.
fn setup_holds(root: &Path, build: &BuildList) -> Option<Why> {
    let Some(rec) = read_setup(root) else { return Some(Why::SetupNotDone) };
    if rec.build_id != build.id {
        return Some(Why::OtherBuild);
    }
    root_inside_beta(root, &rec.paks).or_else(|| folder_changed(&rec.paks, build))
}

/// The check's answer as the launcher reads it.
pub(crate) fn source_info(s: &SourceCheck) -> SourceInfo {
    SourceInfo {
        dir: s.dir.clone(),
        paks: s.paks.clone(),
        kind: match s.kind {
            setup::SourceKind::Beta => SourceKind::Beta,
            setup::SourceKind::OtherBuild => SourceKind::OtherBuild,
            setup::SourceKind::Partial => SourceKind::Partial,
            setup::SourceKind::Relaunch => SourceKind::Relaunch,
            setup::SourceKind::NotGame => SourceKind::NotGame,
        },
        build_id: s.build_id.clone(),
        missing: s.missing.clone(),
        extra: s.extra.clone(),
        key_ok: s.key_ok,
        note: s.note.clone(),
    }
}

/// The worker's stdout: one JSON line per event, flushed (a parent gone is not an error of the worker's).
pub struct Out {
    out: Mutex<std::io::Stdout>,
}

impl Default for Out {
    fn default() -> Out {
        Out { out: Mutex::new(std::io::stdout()) }
    }
}

impl Out {
    pub fn send(&self, e: &ImportEvent) {
        let Ok(mut line) = serde_json::to_vec(e) else { return };
        line.push(b'\n');
        let mut o = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = o.write_all(&line);
        let _ = o.flush();
    }

    fn log(&self, level: LogLevel, say: Say) {
        self.send(&ImportEvent::Log { level, say });
    }
}

/// At most a few events a second of each stream (a phase and kind, or the check: the kinds being built overlap),
/// a stream's first and last at once.
struct Throttle {
    last: Mutex<HashMap<String, Instant>>,
}

const THROTTLE_MS: u128 = 250;

impl Throttle {
    fn new() -> Throttle {
        Throttle { last: Mutex::new(HashMap::new()) }
    }

    fn pass(&self, key: &str, done: u64, total: u64) -> bool {
        let now = Instant::now();
        let mut l = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = l.get(key)
            && done != total
            && now.duration_since(*at).as_millis() < THROTTLE_MS
        {
            return false;
        }
        l.insert(key.to_string(), now);
        true
    }
}

fn phase(p: Phase) -> ImportPhase {
    match p {
        Phase::Catalog => ImportPhase::Catalog,
        Phase::List => ImportPhase::List,
        Phase::Build => ImportPhase::Build,
        Phase::Write => ImportPhase::Write,
        Phase::Switch => ImportPhase::Switch,
    }
}

/// An error of the importer, the paks or the OS, as it reads.
fn error(e: impl std::fmt::Display) -> Say {
    Say::Error { detail: e.to_string() }
}

/// How a run ended.
enum Ended {
    Done,
    Failed(Say),
    Cancelled,
}

/// The run's lines for the kinds it built and the items that failed.
fn report(out: &Out, s: &ImportSummary) {
    const MOST: usize = 200;
    for k in s.kinds.iter().filter(|k| k.status == KindStatus::Built) {
        let level = if k.failed > 0 { LogLevel::Warn } else { LogLevel::Info };
        out.log(
            level,
            Say::KindBuilt {
                kind: k.kind.clone(),
                version: k.version,
                entries: k.entries,
                bytes: k.bytes,
                failed: k.failed,
                secs: k.seconds,
            },
        );
    }
    for f in s.failures.iter().take(MOST) {
        let (kind, path, detail) = (f.kind.to_string(), f.path.to_string(), f.message.to_string());
        out.log(LogLevel::Warn, Say::ItemFailed { kind, path, detail });
    }
    if s.failures.len() > MOST {
        out.log(LogLevel::Warn, Say::MoreFailures { count: s.failures.len() - MOST });
    }
}

/// The import run: Ok on Done; the message of a failure; Cancelled through `cancel`.
fn run(req: &ImportRequest, out: &Out, cancel: &Arc<AtomicBool>) -> Result<(), Say> {
    let t0 = Instant::now();
    let secs = |t: Instant| t.elapsed().as_secs_f64();
    let build = setup::build_list().map_err(error)?;
    let rec = read_setup(&req.data_root);
    let paks = if !req.paks.as_os_str().is_empty() {
        let p = setup::paks_folder_of(&req.paks);
        if p.as_os_str().is_empty() {
            return Err(Say::NoGameFiles { dir: req.paks.clone() });
        }
        p
    } else if let Some(r) = &rec {
        r.paks.clone()
    } else {
        return Err(Say::NoBetaChosen);
    };
    if let Some(why) = root_inside_beta(&req.data_root, &paks) {
        return Err(Say::Unusable { why });
    }
    // the key, read again from the exe at every run; the setup check and the paks' open prove it on the indexes
    let exe = setup::exe_beside_paks(&paks);
    let key = setup::key_from_exe(&exe).map_err(|e| Say::KeyUnread { exe: exe.clone(), detail: e.to_string() })?;

    let why = match &rec {
        None => Some(CheckWhy::FirstRun),
        Some(r) if r.build_id != build.id => Some(CheckWhy::OtherBuild),
        Some(r) if !same_path(&r.paks, &paks) => Some(CheckWhy::OtherFolder),
        Some(_) => folder_changed(&paks, &build).map(|why| CheckWhy::Changed { why }),
    };
    let tick = Throttle::new();
    if let Some(why) = why {
        out.log(LogLevel::Info, Say::Checking { paks: paks.clone(), why });
        let t = Instant::now();
        let progress = |done: u64, total: u64, file: &str| {
            if tick.pass("check", done, total) {
                out.send(&ImportEvent::Check { done, total, file: file.to_string() });
            }
        };
        setup::setup_check(&paks, &key, &build, Some(&progress), Some(cancel)).map_err(|e| match e {
            PakError::Cancelled => Say::Cancelled,
            e => error(e),
        })?;
        let rec =
            SetupRecord { build_id: build.id.clone(), paks: paks.clone(), checked_at: iso_utc(SystemTime::now()) };
        write_setup(&req.data_root, &rec)
            .map_err(|e| Say::SetupUnwritten { file: setup_file(&req.data_root), detail: e.to_string() })?;
        out.log(LogLevel::Info, Say::Matches { build: build.id.to_string(), secs: secs(t) });
    } else if let Some(r) = &rec {
        out.log(LogLevel::Info, Say::Checked { paks: paks.clone(), at: r.checked_at.clone() });
    }

    let root = store_dir(&req.data_root);
    let files: Vec<PathBuf> = build.paks.iter().map(|p| paks.join(&p.name)).collect();
    let set = Arc::new(PakSet::open(&files, &key).map_err(error)?);
    // the import's memory bounds from the free memory, at most the job's cap
    let free = free_memory().min(super::job::own_memory_cap().unwrap_or(u64::MAX));
    let opts = ImportOptions {
        store: root.clone(),
        force: req.force,
        threads: (req.threads > 0).then_some(req.threads),
        budget: Some(Budget::from_free(free)),
        cancel: cancel.clone(),
        ..Default::default()
    };
    let progress = |p: Progress<'_>| {
        let key = format!("{:?}{}", p.phase, p.kind.unwrap_or(""));
        if tick.pass(&key, p.done, p.total) {
            out.send(&ImportEvent::Progress {
                phase: phase(p.phase),
                kind: p.kind.map(str::to_string),
                done: p.done,
                total: p.total,
                bytes: p.bytes,
            });
        }
    };
    out.log(LogLevel::Info, Say::Importing { store: root.clone() });
    let s = mvs_import::run::import(&set, &opts, &progress).map_err(|e| match e {
        ImportError::Cancelled => Say::Cancelled,
        e => error(e),
    })?;
    report(out, &s);
    // a kind not built ends the run as failed (the CLI's exit), after its report
    let failed: Vec<&str> =
        s.kinds.iter().filter(|k| matches!(k.status, KindStatus::Failed(_))).map(|k| k.kind.as_str()).collect();
    if !failed.is_empty() {
        for k in s.kinds.iter() {
            if let KindStatus::Failed(why) = &k.status {
                out.log(LogLevel::Warn, Say::KindNotBuilt { kind: k.kind.clone(), detail: why.to_string() });
            }
        }
        return Err(Say::KindsNotBuilt { kinds: failed.iter().map(|k| k.to_string()).collect() });
    }
    out.log(LogLevel::Info, Say::ImportDone { secs: secs(t0) });
    out.send(&ImportEvent::Done {
        kinds: s
            .kinds
            .iter()
            .map(|k| KindSummary {
                kind: k.kind.clone(),
                status: if k.status == KindStatus::Built { "built" } else { "current" }.into(),
                entries: k.entries,
                failed: k.failed,
                ms: (k.seconds * 1000.0).round() as u64,
            })
            .collect(),
    });
    Ok(())
}

/// The state op: setup.json against the folder, the store against every stored kind. It reads
/// only and takes no lock (a run started while it goes on is not Busy); a dead run's leftovers are the next run's to
/// clean.
fn state(req: &ImportRequest) -> Result<ImportState, Say> {
    let build = setup::build_list().map_err(error)?;
    let why = setup_holds(&req.data_root, &build);
    let root = store_dir(&req.data_root);
    let asked: Vec<mvs_store::manage::Asked<'_>> = mvs_import::kinds::KINDS
        .iter()
        .filter(|k| k.stored())
        .map(|k| mvs_store::manage::Asked { id: k.id(), version: k.version(), reads: k.reads() })
        .collect();
    let st = mvs_store::manage::staleness(&root, &build.id, mvs_import::catalog::CATALOG_VERSION, &asked, false);
    // (the catalog, the import's list of the build's exports, is rebuilt by the next run as a kind is)
    let stale: Vec<String> = if st.catalog && !st.new_build {
        std::iter::once("catalog".to_string()).chain(st.kinds).collect()
    } else {
        st.kinds
    };
    Ok(ImportState {
        setup: why.is_none(),
        why,
        beta_dir: read_setup(&req.data_root).map(|r| r.paks),
        current: stale.is_empty() && !st.new_build,
        new_build: st.new_build,
        stale,
    })
}

/// One op served; its events written to `out`. The process's exit code: 0 for a run done or an op answered.
pub fn serve(op: WorkerOp, req: &ImportRequest, out: &Out, cancel: &Arc<AtomicBool>) -> i32 {
    let r = match op {
        WorkerOp::State => state(req).map(|state| out.send(&ImportEvent::State { state })),
        WorkerOp::Detect => setup::build_list().map_err(error).map(|b| {
            let found = super::steam::detect_sources(req.last.as_deref(), None, &b);
            out.send(&ImportEvent::Detected { found })
        }),
        WorkerOp::Check => setup::build_list()
            .map_err(error)
            .map(|b| out.send(&ImportEvent::Source { source: source_info(&setup::source_check(&req.paks, &b)) })),
        WorkerOp::Run => match run(req, out, cancel) {
            Ok(()) => Ok(()),
            Err(_) if cancel.load(Ordering::Relaxed) => {
                return ended(out, Ended::Cancelled);
            }
            Err(e) => Err(e),
        },
    };
    ended(
        out,
        match r {
            Ok(()) => Ended::Done,
            Err(e) => Ended::Failed(e),
        },
    )
}

fn ended(out: &Out, e: Ended) -> i32 {
    match e {
        Ended::Done => 0,
        Ended::Failed(say) => {
            out.log(LogLevel::Error, say.clone());
            out.send(&ImportEvent::Failed { say });
            1
        }
        Ended::Cancelled => {
            out.log(LogLevel::Info, Say::Cancelled);
            out.send(&ImportEvent::Cancelled);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_762_675_200_123);
        assert_eq!(iso_utc(t), "2025-11-09T08:00:00.123Z");
        assert_eq!(iso_utc(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    /// The launcher's groups name the kinds the native import builds, each once: no group waits on a kind the run
    /// never builds, and no built kind is left to the last group by default.
    #[test]
    fn the_groups_are_the_native_kinds() {
        let mut grouped: Vec<&str> =
            mvs_ui::launcher::import_view::GROUPS.iter().flat_map(|g| g.kinds.iter().copied()).collect();
        let mut built: Vec<&str> = mvs_import::kinds::KINDS.iter().filter(|k| k.stored()).map(|k| k.id()).collect();
        grouped.sort_unstable();
        built.sort_unstable();
        assert_eq!(grouped, built);
    }

    #[test]
    fn paths_inside() {
        assert!(inside(Path::new(r"D:\Beta\data"), Path::new(r"d:\beta")));
        assert!(inside(Path::new(r"D:\Beta"), Path::new(r"d:\beta\")));
        assert!(!inside(Path::new(r"D:\Beta2"), Path::new(r"D:\Beta")));
        let paks = Path::new(r"D:\Steam\depot_1\MultiVersus\Content\Paks");
        assert!(root_inside_beta(Path::new(r"D:\Steam\depot_1\mvs"), paks).is_some());
        assert!(root_inside_beta(Path::new(r"D:\Games\MVS Classic"), paks).is_none());
    }
}
