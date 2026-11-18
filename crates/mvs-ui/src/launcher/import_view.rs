//! The import's events as the worker writes them and the run's
//! progress the launcher draws: the kinds in the groups a player reads, each
//! kind's rough share of the time, the run's fraction, the time left, the log.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The import's phases (mvs_import::Phase).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPhase {
    Catalog,
    List,
    Build,
    Write,
    Switch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// What a folder holds (mvs_pak::setup::SourceKind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Beta,
    OtherBuild,
    Partial,
    Relaunch,
    NotGame,
}

/// A folder's check (mvs_pak::setup::SourceCheck).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub dir: PathBuf,
    pub paks: PathBuf,
    pub kind: SourceKind,
    pub build_id: Option<String>,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub key_ok: bool,
    pub note: String,
}

/// Where a folder of the detect op was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFrom {
    /// The folder setup used last.
    Last,
    /// Under the Steam root.
    Steam,
    /// Under another library of Steam's libraryfolders.vdf.
    Library,
}

/// A folder the detect op found, checked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceCandidate {
    pub source: SourceInfo,
    pub from: SourceFrom,
}

/// The store against the beta (mvs_store::manage::staleness).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportState {
    /// setup.json names a beta folder checked for this build that still holds it (by names and sizes).
    #[serde(default)]
    pub setup: bool,
    /// Why setup does not hold (`SetupNotDone` before the first run, else the folder's problem); None when it does.
    #[serde(default)]
    pub why: Option<Why>,
    /// The beta's paks folder setup.json names (None before setup).
    #[serde(default)]
    pub beta_dir: Option<PathBuf>,
    /// Every kind current: Play opens the game.
    pub current: bool,
    /// The paks are another build than the store's: every kind is rebuilt.
    pub new_build: bool,
    /// The kinds to rebuild (`catalog` first when the catalog is stale and the build is the store's).
    pub stale: Vec<String>,
}

/// Why setup does not hold: codes and fields, worded by `words::why`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum Why {
    /// No setup.json: before the first run.
    SetupNotDone,
    /// setup.json's folder was checked for another build.
    OtherBuild,
    /// The data root is the beta's install folder or inside it.
    RootInsideBeta {
        root: PathBuf,
        beta: PathBuf,
    },
    BetaGone {
        paks: PathBuf,
    },
    PaksMissing {
        paks: PathBuf,
        count: usize,
    },
    PaksExtra {
        paks: PathBuf,
    },
    PakResized {
        paks: PathBuf,
        file: String,
    },
    ExeGone {
        paks: PathBuf,
    },
    /// Files the build needs beside the paks.
    FilesGone {
        files: Vec<String>,
    },
}

/// Why a run hashes the beta's files again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "check", rename_all = "snake_case")]
pub enum CheckWhy {
    FirstRun,
    /// setup.json names another build.
    OtherBuild,
    OtherFolder,
    Changed {
        why: Why,
    },
}

/// A line the worker says (a log line, a failure): codes and fields, worded by `words::say`. `detail` is an error's
/// own text (the importer's, the paks', the OS's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "say", rename_all = "snake_case")]
pub enum Say {
    /// An error of the importer, the paks or the OS, as it reads.
    Error {
        detail: String,
    },
    NoGameFiles {
        dir: PathBuf,
    },
    NoBetaChosen,
    /// The run's data root cannot be used.
    Unusable {
        why: Why,
    },
    KeyUnread {
        exe: PathBuf,
        detail: String,
    },
    SetupUnwritten {
        file: PathBuf,
        detail: String,
    },
    Checking {
        paks: PathBuf,
        why: CheckWhy,
    },
    Matches {
        build: String,
        secs: f64,
    },
    Checked {
        paks: PathBuf,
        at: String,
    },
    Importing {
        store: PathBuf,
    },
    KindBuilt {
        kind: String,
        version: u64,
        entries: u64,
        bytes: u64,
        failed: u64,
        secs: f64,
    },
    ItemFailed {
        kind: String,
        path: String,
        detail: String,
    },
    MoreFailures {
        count: usize,
    },
    KindNotBuilt {
        kind: String,
        detail: String,
    },
    /// The run's end when kinds were not built.
    KindsNotBuilt {
        kinds: Vec<String>,
    },
    ImportDone {
        secs: f64,
    },
    Cancelled,
    /// The worker ended without its answer.
    WorkerStopped {
        code: Option<i32>,
    },
    /// An op's worker gave no answer.
    NoAnswer,
}

/// A kind's line of the run's summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KindSummary {
    pub kind: String,
    pub status: String,
    pub entries: u64,
    pub failed: u64,
    pub ms: u64,
}

/// One line the worker writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImportEvent {
    /// The beta's files hashed: `done` of `total` bytes, the file at.
    Check {
        done: u64,
        total: u64,
        file: String,
    },
    /// A phase's progress (mvs_import::Progress).
    Progress {
        phase: ImportPhase,
        kind: Option<String>,
        done: u64,
        total: u64,
        bytes: u64,
    },
    Log {
        level: LogLevel,
        say: Say,
    },
    /// The `state` op's answer.
    State {
        state: ImportState,
    },
    /// The `detect` op's answer: every folder found that holds game files, checked, one per paks folder, in the
    /// order looked at (the last folder, the Steam root, its libraries).
    Detected {
        found: Vec<SourceCandidate>,
    },
    /// The `check` op's answer.
    Source {
        source: SourceInfo,
    },
    Done {
        kinds: Vec<KindSummary>,
    },
    /// A run, or an op, that failed; also what the parent's poll gives for a worker that ended without its answer.
    Failed {
        say: Say,
    },
    /// The run cancelled: the store keeps every kind switched in before it.
    Cancelled,
}

impl ImportEvent {
    /// Whether the event ends the worker's op: a run's end, or the answer of the other ops.
    pub fn ends(&self) -> bool {
        !matches!(self, ImportEvent::Check { .. } | ImportEvent::Progress { .. } | ImportEvent::Log { .. })
    }
}

/// A group of kinds a player reads.
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub name: GroupName,
    pub kinds: &'static [&'static str],
}

/// The steps of a run as the launcher names them (the words are the UI text module's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupName {
    /// The check of the beta's files (the `check` events).
    Check,
    /// The reading of its packages (catalog, list).
    Read,
    Characters,
    Stages,
    Models,
    Effects,
    Sound,
    Menus,
}

/// The import's kinds in the groups a player reads: the kinds the native import stores (`mvs_import::kinds::KINDS`
/// whose `stored()` holds; a test of mvs-app holds the two equal), so no group waits on a kind the run never builds.
pub const GROUPS: &[Group] = &[
    Group { name: GroupName::Characters, kinds: &["fighter", "gamedata", "skeleton", "animation"] },
    Group {
        name: GroupName::Stages,
        kinds: &["stage", "sky", "scenery", "lighting", "stagelight", "atmosphere", "stagedraw"],
    },
    Group { name: GroupName::Models, kinds: &["mesh", "texture"] },
    Group { name: GroupName::Effects, kinds: &["particles", "material", "shader"] },
    Group { name: GroupName::Sound, kinds: &["sound", "soundbank"] },
    Group {
        name: GroupName::Menus,
        kinds: &["ui", "banner", "menuscene", "font", "image", "literals", "text", "config"],
    },
];

/// A kind's rough share of a run's time (measured runs: textures the most, then meshes, animations and sounds).
pub fn weight(kind: &str) -> f64 {
    match kind {
        "texture" => 8.0,
        "mesh" | "animation" | "sound" => 4.0,
        "shader" | "shadermap" | "program" => 3.0,
        _ => 1.0,
    }
}

/// The share of the bar the check and the reading take, before the kinds'.
pub const CHECK_SHARE: f64 = 0.06;
pub const READ_SHARE: f64 = 0.04;

/// The group a kind is drawn in; a kind no group names, the last group's.
pub fn group_of(kind: &str) -> GroupName {
    GROUPS.iter().find(|g| g.kinds.contains(&kind)).map_or(GROUPS[GROUPS.len() - 1].name, |g| g.name)
}

/// A group's line: its fraction done and its kinds done of its kinds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupLine {
    pub name: GroupName,
    pub frac: f64,
    pub done: usize,
    pub kinds: usize,
}

/// The line under the percent: what the step at hand counts.
#[derive(Clone, Debug, PartialEq)]
pub enum CountLine {
    /// Nothing (the reading of the packages, and before the first event).
    None,
    /// The check: bytes hashed of the bytes to hash.
    Check { done: u64, total: u64 },
    /// A kind's build: its items done of its items.
    Build { kind: String, done: u64, total: u64 },
    /// A kind listed, written or switched in.
    Kind(String),
}

/// A line of the run's log with the wall clock it came at.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub at: std::time::SystemTime,
    pub level: LogLevel,
    pub text: String,
}

/// A run's progress.
#[derive(Clone, Debug)]
pub struct ImportView {
    /// Setup's step 3, else a rebuild on Home.
    pub setup: bool,
    /// Play once it ends well.
    pub then_play: bool,
    /// The data root the store goes in ("Installing to ..."; None when none was chosen).
    pub data_root: Option<PathBuf>,
    pub kinds: Vec<String>,
    /// Each kind's fraction done and its stored bytes, by `kinds`' index.
    frac: Vec<f64>,
    bytes: Vec<u64>,
    check: f64,
    read: f64,
    /// The step going on.
    pub now: GroupName,
    /// The line under the percent.
    pub count: CountLine,
    /// The run's start, s on the caller's clock.
    started: f64,
    /// The log's lines.
    pub log: Vec<LogLine>,
    /// The run's end (None while it runs): Done, Failed or Cancelled.
    pub ended: Option<ImportEvent>,
}

/// A kind's fraction from its phase: building is most of it, then its pack is written and switched in
/// (a kind's list, which the events do not carry per kind, is its start).
fn kind_fraction(phase: ImportPhase, done: u64, total: u64) -> f64 {
    let p = if total > 0 { (done as f64 / total as f64).min(1.0) } else { 1.0 };
    match phase {
        ImportPhase::Catalog | ImportPhase::List => 0.0,
        ImportPhase::Build => 0.85 * p,
        ImportPhase::Write => 0.85 + 0.1 * p,
        ImportPhase::Switch => 0.95 + 0.05 * p,
    }
}

impl ImportView {
    /// A run: every kind for setup, a forced run, a new build or no stale list, else the
    /// stale kinds (those of the groups first, in the list's order, then the rest). `now`: s on the caller's clock.
    pub fn new(
        setup: bool,
        then_play: bool,
        force: bool,
        state: Option<&ImportState>,
        data_root: Option<PathBuf>,
        now: f64,
    ) -> ImportView {
        let in_groups = |k: &str| GROUPS.iter().any(|g| g.kinds.contains(&k));
        let stale: &[String] = state.map_or(&[], |s| &s.stale);
        let new_build = state.is_some_and(|s| s.new_build);
        let kinds: Vec<String> = if setup || force || new_build || stale.is_empty() {
            GROUPS.iter().flat_map(|g| g.kinds.iter().map(|k| k.to_string())).collect()
        } else {
            stale.iter().filter(|k| in_groups(k)).chain(stale.iter().filter(|k| !in_groups(k))).cloned().collect()
        };
        ImportView {
            setup,
            then_play,
            data_root,
            frac: vec![0.0; kinds.len()],
            bytes: vec![0; kinds.len()],
            kinds,
            check: 0.0,
            read: 0.0,
            now: if setup { GroupName::Check } else { GroupName::Read },
            count: CountLine::None,
            started: now,
            log: Vec::new(),
            ended: None,
        }
    }

    /// A kind's index; a kind the run did not name is added.
    fn kind_at(&mut self, kind: &str) -> usize {
        if let Some(i) = self.kinds.iter().position(|k| k == kind) {
            return i;
        }
        self.kinds.push(kind.to_string());
        self.frac.push(0.0);
        self.bytes.push(0);
        self.kinds.len() - 1
    }

    /// An event of the worker taken in (kindFraction, the counts, the bytes, the log).
    pub fn event(&mut self, e: &ImportEvent) {
        match e {
            ImportEvent::Log { level, say } => {
                self.log.push(LogLine { at: std::time::SystemTime::now(), level: *level, text: super::words::say(say) })
            }
            ImportEvent::Check { done, total, .. } => {
                self.now = GroupName::Check;
                self.check = if *total > 0 { *done as f64 / *total as f64 } else { 0.0 };
                self.count = CountLine::Check { done: *done, total: *total };
            }
            ImportEvent::Progress { phase, kind, done, total, bytes } => {
                self.check = 1.0;
                match kind {
                    // the catalog: the reading of the packages (its open and list phases are one scan here)
                    None => {
                        self.now = GroupName::Read;
                        self.read = if *total > 0 { *done as f64 / *total as f64 } else { 1.0 };
                        self.count = CountLine::None;
                    }
                    Some(kind) => {
                        self.read = 1.0;
                        self.now = group_of(kind);
                        let i = self.kind_at(kind);
                        self.frac[i] = kind_fraction(*phase, *done, *total);
                        // (the build's events carry the bytes sent so far, the write's last one the pack's; the
                        // others none)
                        if *bytes > 0 {
                            self.bytes[i] = *bytes;
                        }
                        self.count = match phase {
                            ImportPhase::Build => CountLine::Build { kind: kind.clone(), done: *done, total: *total },
                            _ => CountLine::Kind(kind.clone()),
                        };
                    }
                }
            }
            ImportEvent::Done { .. } | ImportEvent::Failed { .. } | ImportEvent::Cancelled => {
                self.ended = Some(e.clone())
            }
            ImportEvent::State { .. } | ImportEvent::Detected { .. } | ImportEvent::Source { .. } => {}
        }
    }

    /// The run's whole fraction done, at most 1 (runFraction under drawRun's min).
    pub fn fraction(&self) -> f64 {
        let total: f64 = self.kinds.iter().map(|k| weight(k)).sum();
        let total = if total == 0.0 { 1.0 } else { total };
        let kinds = self.kinds.iter().zip(&self.frac).map(|(k, f)| weight(k) * f).sum::<f64>() / total;
        (CHECK_SHARE * self.check + READ_SHARE * self.read + (1.0 - CHECK_SHARE - READ_SHARE) * kinds).min(1.0)
    }

    /// The groups this run builds, in order, each with its fraction done (runGroups); the check and the reading
    /// first, with no count (`done` and `kinds` 0).
    pub fn groups(&self) -> Vec<GroupLine> {
        let mut out = vec![
            GroupLine { name: GroupName::Check, frac: self.check, done: 0, kinds: 0 },
            GroupLine { name: GroupName::Read, frac: self.read, done: 0, kinds: 0 },
        ];
        for g in GROUPS {
            let mine: Vec<(&String, f64)> =
                self.kinds.iter().zip(self.frac.iter().copied()).filter(|(k, _)| group_of(k) == g.name).collect();
            if mine.is_empty() {
                continue;
            }
            let w: f64 = mine.iter().map(|(k, _)| weight(k)).sum();
            let frac = mine.iter().map(|(k, f)| weight(k) * f).sum::<f64>() / w;
            let done = mine.iter().filter(|(_, f)| *f >= 1.0).count();
            out.push(GroupLine { name: g.name, frac, done, kinds: mine.len() });
        }
        out
    }

    /// The minutes left at `now` (timeLeft): None while too little is done to tell (under 3 % or 5 s); the
    /// caller says "about a minute" for 1 or less.
    pub fn minutes_left(&self, now: f64) -> Option<u32> {
        let f = self.fraction();
        let spent = now - self.started;
        if f < 0.03 || spent < 5.0 {
            return None;
        }
        Some(((spent * (1.0 - f)) / f / 60.0).ceil() as u32)
    }

    /// The seconds since the run started, at `now`.
    pub fn elapsed(&self, now: f64) -> f64 {
        now - self.started
    }

    /// The run's minutes at `now` for its end line, at least 1 (runEnded: the run's own time, not the sum of its
    /// kinds', which build side by side).
    pub fn minutes_spent(&self, now: f64) -> u32 {
        (self.elapsed(now) / 60.0).round().max(1.0) as u32
    }

    /// The bytes the run has written.
    pub fn written(&self) -> u64 {
        self.bytes.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(phase: ImportPhase, kind: &str, done: u64, total: u64, bytes: u64) -> ImportEvent {
        ImportEvent::Progress { phase, kind: Some(kind.into()), done, total, bytes }
    }

    #[test]
    fn a_run_goes_from_the_check_to_its_end() {
        let mut v = ImportView::new(true, false, false, None, None, 0.0);
        assert_eq!(v.kinds.len(), GROUPS.iter().map(|g| g.kinds.len()).sum::<usize>());
        assert_eq!(v.now, GroupName::Check);
        v.event(&ImportEvent::Check { done: 50, total: 100, file: "a.pak".into() });
        assert!((v.fraction() - CHECK_SHARE * 0.5).abs() < 1e-12);
        assert_eq!(v.count, CountLine::Check { done: 50, total: 100 });
        v.event(&ImportEvent::Progress { phase: ImportPhase::Catalog, kind: None, done: 10, total: 10, bytes: 0 });
        assert_eq!(v.now, GroupName::Read);
        assert!((v.fraction() - CHECK_SHARE - READ_SHARE).abs() < 1e-12);
        v.event(&progress(ImportPhase::Build, "texture", 5, 10, 300));
        assert_eq!(v.now, GroupName::Models);
        assert_eq!(v.written(), 300);
        let models = v.groups().into_iter().find(|g| g.name == GroupName::Models).unwrap();
        // texture 8 of the group's 12, at 0.425
        assert!((models.frac - 8.0 * 0.425 / 12.0).abs() < 1e-12);
        v.event(&progress(ImportPhase::Write, "texture", 1, 1, 900));
        v.event(&progress(ImportPhase::Switch, "texture", 1, 1, 0));
        assert_eq!(v.written(), 900);
        let models = v.groups().into_iter().find(|g| g.name == GroupName::Models).unwrap();
        assert_eq!((models.done, models.kinds), (1, 2));
        for k in v.kinds.clone() {
            v.event(&progress(ImportPhase::Switch, &k, 1, 1, 0));
        }
        assert!((v.fraction() - 1.0).abs() < 1e-12);
        assert_eq!(v.minutes_left(4.0), None);
        assert!(v.minutes_left(120.0).is_some_and(|m| m <= 1));
        assert_eq!(v.minutes_spent(10.0), 1);
        assert_eq!(v.minutes_spent(200.0), 3);
        v.event(&ImportEvent::Cancelled);
        assert_eq!(v.ended, Some(ImportEvent::Cancelled));
    }

    #[test]
    fn a_rebuild_takes_the_stale_kinds() {
        let st = ImportState { stale: vec!["catalog".into(), "sound".into(), "fighter".into()], ..Default::default() };
        let v = ImportView::new(false, true, false, Some(&st), None, 0.0);
        assert_eq!(v.kinds, ["sound", "fighter", "catalog"]);
        assert_eq!(v.now, GroupName::Read);
        assert_eq!(group_of("catalog"), GroupName::Menus);
        let v = ImportView::new(false, true, true, Some(&st), None, 0.0);
        assert!(v.kinds.len() > 3);
    }

    #[test]
    fn events_read_back() {
        let e = ImportEvent::Detected {
            found: vec![SourceCandidate {
                source: SourceInfo {
                    dir: "D:/x".into(),
                    paks: "D:/x/MultiVersus/Content/Paks".into(),
                    kind: SourceKind::Beta,
                    build_id: Some("ab".into()),
                    missing: vec![],
                    extra: vec![],
                    key_ok: true,
                    note: "n".into(),
                },
                from: SourceFrom::Library,
            }],
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(line.starts_with(r#"{"type":"detected""#), "{line}");
        assert_eq!(serde_json::from_str::<ImportEvent>(&line).unwrap(), e);
        let s: ImportState = serde_json::from_str(r#"{"current":true,"new_build":false,"stale":[]}"#).unwrap();
        assert!(!s.setup && s.why.is_none());
    }
}
