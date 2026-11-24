//! The import worker through Cargo's `mvs`: its ops on fixture folders, a killed run's leftovers cleaned by the next
//! start. The real-store checks (`#[ignore]`, run with `--release -- --ignored`) need the beta's paks in
//! `MVS_TEST_PAKS` and a scratch folder in `MVS_TEST_DIR`; `MVS_TEST_CLI_STORE` names the CLI's store of the same
//! paks, compared pack for pack.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mvs_app::import::{ImportRequest, Worker, WorkerOp};
use mvs_ui::launcher::import_view::{ImportEvent, ImportPhase, ImportState, Say, SourceInfo, SourceKind, Why};

const GB: u64 = 1 << 30;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_mvs"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mvs-import-worker-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// What to do at an event.
#[derive(PartialEq)]
enum Act {
    Go,
    Cancel,
    Kill,
    /// The cancel asked, then the kill at once (a worker that cannot answer it).
    CancelKill,
}

/// The longest a poll may take: it runs on the launcher's frame.
const POLL_MS: f64 = 50.0;

/// The op run to its end; `at` sees each event. Every poll is timed against [`POLL_MS`]; the seconds from the cancel
/// to the end are printed.
fn run_op(
    op: WorkerOp,
    req: &ImportRequest,
    cap: u64,
    mut at: impl FnMut(&ImportEvent, &Worker) -> Act,
) -> Vec<ImportEvent> {
    let mut w = Worker::start_exe(exe(), op, req, cap).unwrap();
    let mut all = Vec::new();
    let t = Instant::now();
    let mut cancelled: Option<Instant> = None;
    let mut acted = false;
    while !w.ended() {
        let p = Instant::now();
        let got = w.poll();
        let ms = p.elapsed().as_secs_f64() * 1e3;
        assert!(ms < POLL_MS, "a poll took {ms:.1} ms");
        for e in got {
            let act = if acted { Act::Go } else { at(&e, &w) };
            match act {
                Act::Go => {}
                Act::Cancel => {
                    w.cancel();
                    cancelled.get_or_insert_with(Instant::now);
                    acted = true;
                }
                Act::Kill => {
                    w.kill();
                    all.push(e);
                    return all;
                }
                Act::CancelKill => {
                    w.cancel();
                    w.kill();
                    acted = true;
                }
            }
            all.push(e);
        }
        assert!(t.elapsed() < Duration::from_secs(3600), "the worker runs past an hour");
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(c) = cancelled {
        let s = c.elapsed().as_secs_f64();
        eprintln!("  the cancel answered in {s:.2} s");
        assert!(s < 5.0, "the cancel took {s:.1} s: the worker was killed");
    }
    all
}

fn answer(op: WorkerOp, req: &ImportRequest) -> ImportEvent {
    let ev = run_op(op, req, 4 * GB, |_, _| Act::Go);
    ev.into_iter().find(ImportEvent::ends).expect("an answer")
}

fn check(dir: &Path) -> SourceInfo {
    match answer(WorkerOp::Check, &ImportRequest { paks: dir.into(), ..Default::default() }) {
        ImportEvent::Source { source } => source,
        e => panic!("check {}: {e:?}", dir.display()),
    }
}

fn state(root: &Path) -> ImportState {
    match answer(WorkerOp::State, &ImportRequest { data_root: root.into(), ..Default::default() }) {
        ImportEvent::State { state } => state,
        e => panic!("state: {e:?}"),
    }
}

/// The install layout `<dir>/MultiVersus/Content/Paks` with the files named.
fn paks_fixture(dir: &Path, files: &[&str]) -> PathBuf {
    let p = dir.join("MultiVersus").join("Content").join("Paks");
    std::fs::create_dir_all(&p).unwrap();
    for f in files {
        std::fs::write(p.join(f), b"not a pak").unwrap();
    }
    p
}

#[test]
fn check_names_each_kind_of_folder() {
    let d = scratch("check");
    let empty = d.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(check(&empty).kind, SourceKind::NotGame);
    let relaunch = d.join("relaunch");
    paks_fixture(&relaunch, &["pakchunk0-WindowsClient.utoc", "pakchunk0-WindowsClient.ucas"]);
    assert_eq!(check(&relaunch).kind, SourceKind::Relaunch);
    let other = d.join("other");
    paks_fixture(&other, &["pakchunk0-WindowsNoEditor.pak", "pakchunk99-WindowsNoEditor.pak"]);
    let c = check(&other);
    assert_eq!(
        (c.kind, c.extra.as_slice()),
        (SourceKind::OtherBuild, ["pakchunk99-WindowsNoEditor.pak".to_string()].as_slice())
    );
    let partial = d.join("partial");
    let p = paks_fixture(&partial, &["pakchunk0-WindowsNoEditor.pak"]);
    let c = check(&partial);
    assert_eq!(c.kind, SourceKind::Partial);
    assert_eq!(c.paks, p);
    assert!(!c.missing.is_empty() && !c.key_ok);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn detect_finds_the_depots_of_every_library() {
    let d = scratch("detect");
    let steam = d.join("Steam");
    let lib = d.join("Lib2");
    let depot = |root: &Path, n: &str| root.join("steamapps").join("content").join("app_1818750").join(n);
    paks_fixture(&depot(&steam, "depot_1818751"), &["pakchunk0-WindowsNoEditor.pak"]);
    std::fs::create_dir_all(depot(&steam, "depot_2")).unwrap();
    paks_fixture(&depot(&lib, "depot_1818751"), &["x.utoc"]);
    paks_fixture(&depot(&lib, "not_a_depot"), &["pakchunk0-WindowsNoEditor.pak"]);
    let vdf = format!(
        "\"libraryfolders\"\n{{\n\t\"0\" {{ \"path\" \"{}\" }}\n\t\"1\" {{ \"path\" \"{}\" }}\n}}\n",
        steam.display().to_string().replace('\\', "\\\\"),
        lib.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(steam.join("steamapps").join("libraryfolders.vdf"), vdf).unwrap();
    let last = d.join("last");
    paks_fixture(&last, &["foo.pak"]);
    let build = mvs_pak::setup::build_list().unwrap();
    let found = mvs_app::import::steam::detect_sources(Some(&last), Some(&steam), &build);
    use mvs_ui::launcher::import_view::SourceFrom;
    let got: Vec<(SourceFrom, SourceKind)> = found.iter().map(|c| (c.from, c.source.kind)).collect();
    assert_eq!(
        got,
        [
            (SourceFrom::Last, SourceKind::OtherBuild),
            (SourceFrom::Steam, SourceKind::Partial),
            (SourceFrom::Library, SourceKind::Relaunch)
        ]
    );
    // one folder given twice (the last one is the Steam depot) is named once
    let again = mvs_app::import::steam::detect_sources(Some(&depot(&steam, "depot_1818751")), Some(&steam), &build);
    assert_eq!(again.len(), 2);
    let _ = std::fs::remove_dir_all(&d);
}

/// The PID of a process that has exited.
#[cfg(windows)]
fn exited_process() -> u32 {
    let mut gone = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    pid
}

/// The PID of a process that has exited.
#[cfg(not(windows))]
fn exited_process() -> u32 {
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    pid
}

#[test]
fn state_before_setup_and_a_killed_runs_leftovers() {
    let root = scratch("state");
    let s = state(&root);
    assert!(!s.setup && !s.current);
    assert_eq!(s.why, Some(Why::SetupNotDone));
    assert!(s.stale.len() > 10 && s.beta_dir.is_none());

    // a store a killed run left: its lock (its process gone), a pack it was writing, a catalog no manifest names
    let store = mvs_app::import::store_dir(&root);
    std::fs::create_dir_all(&store).unwrap();
    let m = mvs_store::Manifest {
        format: mvs_store::STORE_FORMAT,
        build_id: mvs_pak::setup::build_id().unwrap(),
        catalog: None,
        catalog_version: None,
        kinds: Default::default(),
    };
    mvs_store::write_manifest(&store, &m).unwrap();
    let pid = exited_process();
    std::fs::write(
        store.join("import.lock"),
        format!("{{\"pid\":{pid},\"started\":\"2026-11-09T00:00:00.000Z\",\"nonce\":\"0123456789abcdef\"}}"),
    )
    .unwrap();
    std::fs::write(store.join("texture@2-mabc123456789.pack.tmp"), b"partial").unwrap();
    std::fs::write(store.join("catalog-mabc123456789.json"), b"{}").unwrap();
    std::fs::write(store.join("notes.txt"), b"not the store's").unwrap();
    let names = || -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(&store).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        v.sort();
        v
    };
    // the state op reads only: the leftovers stay for the next run
    state(&root);
    assert_eq!(names().len(), 5);
    // the next run's start (the import takes the dead lock over and cleans against the manifest)
    mvs_import::run::clean(&store).unwrap();
    assert_eq!(names(), ["notes.txt", "store.json"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_worker_gone_without_its_answer_fails() {
    let root = scratch("gone");
    // a worker killed before its answer is reported by the poll
    let mut w = Worker::start_exe(
        exe(),
        WorkerOp::Run,
        &ImportRequest { data_root: root.clone(), ..Default::default() },
        4 * GB,
    )
    .unwrap();
    w.kill();
    let mut ev = Vec::new();
    let t = Instant::now();
    while !w.ended() && t.elapsed() < Duration::from_secs(30) {
        ev.extend(w.poll());
        std::thread::sleep(Duration::from_millis(20));
    }
    #[cfg(windows)]
    assert!(matches!(ev.last(), Some(ImportEvent::Failed { say: Say::WorkerStopped { code: Some(1) } })), "{ev:?}");
    #[cfg(not(windows))]
    assert!(matches!(ev.last(), Some(ImportEvent::Failed { say: Say::WorkerStopped { code: Some(137) } })), "{ev:?}");
    // a run with no beta folder chosen fails with its reason
    let e = answer(WorkerOp::Run, &ImportRequest { data_root: root.clone(), ..Default::default() });
    assert!(matches!(&e, ImportEvent::Failed { say: Say::NoBetaChosen }), "{e:?}");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------------------------
// The real store (the beta's paks)

fn real() -> Option<(PathBuf, PathBuf)> {
    let paks = std::env::var_os("MVS_TEST_PAKS")?;
    let dir = std::env::var_os("MVS_TEST_DIR")?;
    Some((paks.into(), dir.into()))
}

/// Every file the manifest names is there and finished, and the store opens.
fn assert_whole(store: &Path) -> mvs_store::Manifest {
    let m = mvs_store::manage::read_manifest(store).unwrap().expect("a manifest");
    if let Some(c) = &m.catalog {
        assert!(store.join(c).is_file(), "the catalog {c}");
    }
    for (k, r) in &m.kinds {
        assert!(mvs_store::manage::pack_complete(&store.join(&r.file)), "{k}: {} not whole", r.file);
    }
    mvs_store::Store::open(store).unwrap();
    m
}

fn priority_class(pid: u32) -> String {
    let o = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("(Get-Process -Id {pid}).PriorityClass")])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn ended_with(ev: &[ImportEvent]) -> &ImportEvent {
    ev.iter().rev().find(|e| e.ends()).expect("an end")
}

/// A run that ended Done: the native import builds every kind the game reads.
fn assert_run_ended(ev: &[ImportEvent]) {
    match ended_with(ev) {
        ImportEvent::Done { .. } => {}
        ImportEvent::Failed { say } => panic!("{}", mvs_ui::launcher::words::say(say)),
        e => panic!("{e:?}"),
    }
}

/// The min-spec run (1 thread, a 2.5 GB cap, below normal priority) into a fresh data root, then its store
/// against the CLI's, pack for pack.
#[test]
#[ignore]
fn min_spec_run_matches_the_cli() {
    let Some((paks, dir)) = real() else { return eprintln!("no MVS_TEST_PAKS / MVS_TEST_DIR: skipped") };
    let root = dir.join("min_spec");
    let _ = std::fs::remove_dir_all(&root);
    let req = ImportRequest { data_root: root.clone(), paks, threads: 1, setup: true, ..Default::default() };
    let mut prio = String::new();
    let mut peak = 0;
    let t = Instant::now();
    let ev = run_op(WorkerOp::Run, &req, 5 * GB / 2, |e, w| {
        if prio.is_empty() && matches!(e, ImportEvent::Progress { .. }) {
            prio = priority_class(w.pid());
        }
        peak = peak.max(w.peak_memory());
        Act::Go
    });
    eprintln!(
        "min-spec run: {:.0} s, peak commit {:.2} GB, priority {prio}",
        t.elapsed().as_secs_f64(),
        peak as f64 / GB as f64
    );
    assert_run_ended(&ev);
    assert_eq!(prio, "BelowNormal");
    let store = mvs_app::import::store_dir(&root);
    let ours = assert_whole(&store);
    let s = state(&root);
    let mut missing: Vec<String> = mvs_import::kinds::KINDS
        .iter()
        .filter(|k| k.stored() && !ours.kinds.contains_key(k.id()))
        .map(|k| k.id().to_string())
        .collect();
    let mut stale = s.stale.clone();
    missing.sort();
    stale.sort();
    assert!(s.setup && s.why.is_none() && !s.new_build && stale == missing, "{s:?}");
    assert_eq!(s.beta_dir.as_deref(), Some(req.paks.as_path()));

    assert_matches_the_cli(&store);
}

/// The min-spec run's store (already made) against the CLI's.
#[test]
#[ignore]
fn min_spec_store_matches_the_cli() {
    let Some((_, dir)) = real() else { return eprintln!("no MVS_TEST_PAKS / MVS_TEST_DIR: skipped") };
    assert_matches_the_cli(&mvs_app::import::store_dir(&dir.join("min_spec")));
}

/// `store` against `MVS_TEST_CLI_STORE`: the catalog byte for byte, every kind's record (version, entries, pack
/// bytes, failed) and every entry (path, content type, bytes). A pack's entry order follows the order its builds
/// land in (the threads'), so two runs' packs differ in layout, not in what they hold.
fn assert_matches_the_cli(store: &Path) {
    let ours = assert_whole(store);
    let Some(cli) = std::env::var_os("MVS_TEST_CLI_STORE").map(PathBuf::from) else { return };
    let theirs = assert_whole(&cli);
    assert_eq!(ours.build_id, theirs.build_id);
    let read = |d: &Path, f: &str| std::fs::read(d.join(f)).unwrap();
    assert!(read(store, ours.catalog.as_ref().unwrap()) == read(&cli, theirs.catalog.as_ref().unwrap()), "the catalog");
    // (the manifest names the kinds in the order they were switched in)
    let keys = |m: &mvs_store::Manifest| {
        let mut k: Vec<String> = m.kinds.keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&ours), keys(&theirs));
    for (k, r) in &ours.kinds {
        let c = &theirs.kinds[k];
        assert_eq!(
            (r.version, r.entries, r.bytes, r.failed, r.partial),
            (c.version, c.entries, c.bytes, c.failed, c.partial),
            "{k}"
        );
        let a = mvs_store::Pack::open(k, store.join(&r.file)).unwrap();
        let b = mvs_store::Pack::open(k, cli.join(&c.file)).unwrap();
        let mut pa: Vec<&str> = a.paths().collect();
        let mut pb: Vec<&str> = b.paths().collect();
        pa.sort_unstable();
        pb.sort_unstable();
        assert!(pa == pb, "{k}: the paths differ");
        for p in pa {
            assert_eq!(a.content_type(p), b.content_type(p), "{k} {p}");
            assert!(a.bytes(p).unwrap() == b.bytes(p).unwrap(), "{k} {p}: the entry differs");
        }
    }
    eprintln!("{} kinds (every entry) and the catalog identical to the CLI's", ours.kinds.len());
}

/// A cancel in each phase keeps the store whole; a killed run's leftovers are cleaned by the next state op. Runs
/// on the min-spec run's data root, at the machine's threads.
#[test]
#[ignore]
fn cancel_in_each_phase_and_a_kill() {
    let Some((paks, dir)) = real() else { return eprintln!("no MVS_TEST_PAKS / MVS_TEST_DIR: skipped") };
    let root = dir.join("min_spec");
    let store = mvs_app::import::store_dir(&root);
    assert_whole(&store);
    let before = std::fs::read(store.join("store.json")).unwrap();
    let setup_json = root.join("setup.json");
    let saved = std::fs::read(&setup_json).expect("the min-spec run's setup.json");

    // the check: setup.json gone, the run hashes the paks again; cancelled, nothing of the store changes
    std::fs::remove_file(&setup_json).unwrap();
    let req = ImportRequest { data_root: root.clone(), paks: paks.clone(), ..Default::default() };
    let t = Instant::now();
    let ev =
        run_op(
            WorkerOp::Run,
            &req,
            4 * GB,
            |e, _| if matches!(e, ImportEvent::Check { .. }) { Act::Cancel } else { Act::Go },
        );
    eprintln!("cancel in the check: ended in {:.1} s", t.elapsed().as_secs_f64());
    assert_eq!(ended_with(&ev), &ImportEvent::Cancelled);
    assert!(!setup_json.exists());
    assert_whole(&store);
    assert!(std::fs::read(store.join("store.json")).unwrap() == before, "the manifest changed");
    // a worker killed right after the cancel (it cannot answer) ends Cancelled too
    let ev = run_op(WorkerOp::Run, &req, 4 * GB, |e, _| {
        if matches!(e, ImportEvent::Check { .. }) { Act::CancelKill } else { Act::Go }
    });
    assert_eq!(ended_with(&ev), &ImportEvent::Cancelled);
    assert!(std::fs::read(store.join("store.json")).unwrap() == before, "the manifest changed");
    std::fs::write(&setup_json, &saved).unwrap();

    // the import's phases: a forced run cancelled at its first event of each
    let forced = ImportRequest { data_root: root.clone(), paks: PathBuf::new(), force: true, ..Default::default() };
    for phase in [ImportPhase::Catalog, ImportPhase::List, ImportPhase::Build, ImportPhase::Write, ImportPhase::Switch]
    {
        let t = Instant::now();
        let ev = run_op(WorkerOp::Run, &forced, 4 * GB, |e, _| match e {
            ImportEvent::Progress { phase: p, .. } if *p == phase => Act::Cancel,
            _ => Act::Go,
        });
        eprintln!("cancel in {phase:?}: ended in {:.1} s", t.elapsed().as_secs_f64());
        assert_eq!(ended_with(&ev), &ImportEvent::Cancelled, "{phase:?}");
        assert_whole(&store);
    }

    // a kill in the build: the lock and the run's partial packs stay, the next run's start cleans them
    let ev = run_op(WorkerOp::Run, &forced, 4 * GB, |e, _| match e {
        ImportEvent::Progress { phase: ImportPhase::Build, done, .. } if *done > 0 => Act::Kill,
        _ => Act::Go,
    });
    assert!(!ev.iter().any(|e| matches!(e, ImportEvent::Done { .. })));
    let names = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        v.sort();
        v
    };
    let left = names(&store);
    assert!(left.iter().any(|n| n == "import.lock"), "{left:?}");
    assert!(left.iter().any(|n| n.ends_with(".tmp")), "{left:?}");
    state(&root);
    assert_eq!(names(&store), left);
    mvs_import::run::clean(&store).unwrap();
    let m = assert_whole(&store);
    let mut want: Vec<String> = m.kinds.values().map(|r| r.file.clone()).chain(m.catalog.clone()).collect();
    want.push("store.json".into());
    want.sort();
    assert_eq!(names(&store), want);

    // and a plain run brings it current again
    let ev =
        run_op(WorkerOp::Run, &ImportRequest { data_root: root.clone(), ..Default::default() }, 4 * GB, |_, _| Act::Go);
    assert_run_ended(&ev);
    assert!(state(&root).setup);
}
