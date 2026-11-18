//! The runner over kinds of its own: no paks (an empty set), packages that are only names.

use std::path::{Path, PathBuf};

use mvs_pak::{AesKey, BuildPaths, PakSet};
use mvs_store::Store;

use super::*;
use crate::kind::{KindError, not_ported};
use rayon::prelude::*;

/// A kind of the tests: lists `items`; an entry is its path repeated `repeat` times (large ones compress); the path
/// `fail` errs, the path `panic` panics.
struct Fake {
    id: &'static str,
    version: u64,
    items: &'static [&'static str],
    repeat: usize,
    stored: bool,
}

const fn fake(id: &'static str, items: &'static [&'static str], repeat: usize) -> Fake {
    Fake { id, version: 1, items, repeat, stored: true }
}

impl Kind for Fake {
    fn id(&self) -> &'static str {
        self.id
    }
    fn version(&self) -> u64 {
        self.version
    }
    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(self.items.iter().map(|p| Item::new(*p)).collect())
    }
    fn build(&self, _: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        match item.path.as_str() {
            "/Game/X/Fail.Fail" => Err(KindError::Failed("no such thing".into())),
            "/Game/X/Panic.Panic" => panic!("a build that panics"),
            p => Ok(format!("{}:{p};", self.id).repeat(self.repeat).into_bytes()),
        }
    }
    fn stored(&self) -> bool {
        self.stored
    }
}

struct Unported;

impl Kind for Unported {
    fn id(&self) -> &'static str {
        "unported"
    }
    fn version(&self) -> u64 {
        1
    }
    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        not_ported("the unported kind's list")
    }
    fn build(&self, _: &Cx<'_>, _: &Item) -> Result<Vec<u8>, KindError> {
        not_ported("the unported kind's build")
    }
}

static ALPHA: Fake = fake("alpha", &["/Game/P/A.A", "/Game/P/B.B", "/Game/Q/C.C", "/Game/X/Fail.Fail"], 2000);
// shares the package /Game/P with alpha
static BETA: Fake = fake("beta", &["/Game/P/A.A", "/Game/R/D.D", "/Game/X/Panic.Panic"], 1);
static EMPTY: Fake = fake("empty", &[], 1);
static DEV: Fake = Fake { stored: false, ..fake("dev", &["/Game/P/A.A"], 1) };
static REGISTRY: [&dyn Kind; 5] = [&ALPHA, &BETA, &EMPTY, &Unported, &DEV];

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mvs-import-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn run(dir: &Path, f: impl FnOnce(&mut ImportOptions)) -> Result<ImportSummary, ImportError> {
    let paks = Arc::new(PakSet::open(&[], &AesKey([0; 32])).unwrap());
    let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
    let mut opts = ImportOptions { store: dir.to_path_buf(), threads: Some(4), ..Default::default() };
    f(&mut opts);
    let build = opts.build_id.clone().unwrap_or_else(|| "build1".into());
    import_kinds(&paks, &paths, &REGISTRY, &build, &opts, &|_| {})
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

/// Every entry of a kind in the store: (path, bytes).
fn entries(dir: &Path, kind: &str) -> Vec<(String, Vec<u8>)> {
    let s = Store::open(dir).unwrap();
    let p = s.kind(kind).unwrap();
    p.paths().map(|x| (x.to_string(), p.bytes(x).unwrap())).collect()
}

fn status<'a>(s: &'a ImportSummary, kind: &str) -> &'a KindSummary {
    s.kinds.iter().find(|k| k.kind == kind).unwrap()
}

#[test]
fn a_run_then_nothing_stale_then_one_forced() {
    let dir = scratch("runs");
    let s = run(&dir, |_| {}).unwrap();
    assert_eq!(s.kinds.iter().map(|k| k.kind.as_str()).collect::<Vec<_>>(), ["alpha", "beta", "empty", "unported"]);
    let a = status(&s, "alpha");
    assert_eq!((&a.status, a.entries, a.failed), (&KindStatus::Built, 3, 1));
    let b = status(&s, "beta");
    assert_eq!((&b.status, b.entries, b.failed), (&KindStatus::Built, 2, 1));
    let e = status(&s, "empty");
    assert_eq!((&e.status, e.entries), (&KindStatus::Built, 0));
    assert!(matches!(&status(&s, "unported").status, KindStatus::Failed(w) if w.contains("not ported")));
    // the failures: by kind in the registry's order, then path; a panic is its item's failure
    let f: Vec<(&str, &str)> = s.failures.iter().map(|f| (f.kind.as_str(), f.path.as_str())).collect();
    assert_eq!(f, [("alpha", "/Game/X/Fail.Fail"), ("beta", "/Game/X/Panic.Panic")]);
    assert!(s.failures[1].message.contains("a build that panics"));
    let got = entries(&dir, "alpha");
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].1, "alpha:/Game/P/A.A;".repeat(2000).into_bytes());
    assert_eq!(entries(&dir, "beta")[0].1, b"beta:/Game/P/A.A;");
    let m = Store::open(&dir).unwrap().manifest().clone();
    assert_eq!(m.build_id, "build1");
    assert!(m.catalog.as_deref().is_some_and(|c| dir.join(c).is_file()));
    assert!(!m.kinds.contains_key("unported") && !m.kinds.contains_key("dev"));
    assert_eq!(m.kinds["alpha"].failed, 1);
    let before = files(&dir);
    assert!(!before.iter().any(|f| f.ends_with(".tmp") || f == "import.lock"), "{before:?}");

    // a second run: only the unported kind is stale (it was never written); nothing else is built (a kind built
    // writes a pack of a new name)
    let s = run(&dir, |_| {}).unwrap();
    assert_eq!(status(&s, "alpha").status, KindStatus::Current);
    assert_eq!(status(&s, "alpha").entries, 3);
    assert_eq!(files(&dir), before);
    let s = run(&dir, |o| o.kinds = vec!["alpha".into(), "beta".into()]).unwrap();
    assert_eq!(s.packages, 0);
    assert_eq!(files(&dir), before);

    // forced: alpha alone rebuilt, its old pack removed, the others as they were
    let s = run(&dir, |o| {
        o.kinds = vec!["alpha".into()];
        o.force = true;
    })
    .unwrap();
    assert_eq!(s.kinds.len(), 1);
    assert_eq!(status(&s, "alpha").status, KindStatus::Built);
    let after = files(&dir);
    let alpha = |v: &[String]| v.iter().filter(|f| f.starts_with("alpha@")).cloned().collect::<Vec<_>>();
    assert_eq!(alpha(&after).len(), 1);
    assert_ne!(alpha(&after), alpha(&before));
    assert_eq!(after.iter().filter(|f| f.starts_with("beta@")).count(), 1);
    assert!(before.contains(after.iter().find(|f| f.starts_with("beta@")).unwrap()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn entries_do_not_depend_on_threads() {
    let (one, many) = (scratch("one"), scratch("many"));
    run(&one, |o| o.threads = Some(1)).unwrap();
    // a tiny entry bound: every send waits for the writer
    let s = run(&many, |o| {
        o.threads = Some(8);
        o.budget = Some(Budget { free: 0, cache: 1, entries: 1 });
    })
    .unwrap();
    assert!(s.entries_peak >= 1);
    for k in ["alpha", "beta", "empty"] {
        assert_eq!(entries(&one, k), entries(&many, k), "{k}");
    }
    std::fs::remove_dir_all(&one).unwrap();
    std::fs::remove_dir_all(&many).unwrap();
}

#[test]
fn filter_unstored_and_new_build() {
    let dir = scratch("filter");
    assert!(matches!(run(&dir, |o| o.kinds = vec!["dev".into()]), Err(ImportError::Kinds(_))));
    assert!(matches!(run(&dir, |o| o.kinds = vec!["nothing".into()]), Err(ImportError::Kinds(_))));
    let s = run(&dir, |o| {
        o.kinds = vec!["alpha".into()];
        o.unstored = vec!["dev".into()];
        o.filter = Some("/Game/P/".into());
    })
    .unwrap();
    assert_eq!(s.kinds.iter().map(|k| k.kind.as_str()).collect::<Vec<_>>(), ["alpha", "dev"]);
    assert_eq!(status(&s, "alpha").entries, 2);
    let m = Store::open(&dir).unwrap().manifest().clone();
    assert!(m.kinds["alpha"].partial && m.kinds["dev"].partial);
    // partial: stale for the next run
    let s = run(&dir, |o| o.kinds = vec!["alpha".into()]).unwrap();
    assert_eq!((&status(&s, "alpha").status, status(&s, "alpha").entries), (&KindStatus::Built, 3));
    // another build: one switch at the end, the old build's kinds gone (dev was not built for it)
    let s = run(&dir, |o| {
        o.kinds = vec!["beta".into()];
        o.build_id = Some("build2".into());
    })
    .unwrap();
    assert_eq!(s.packages, 0);
    let m = Store::open(&dir).unwrap().manifest().clone();
    assert_eq!(m.build_id, "build2");
    assert_eq!(m.kinds.keys().collect::<Vec<_>>(), ["beta"]);
    let f = files(&dir);
    assert!(!f.iter().any(|f| f.starts_with("alpha@") || f.starts_with("dev@")), "{f:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_package_is_one_task_across_kinds() {
    let paks = PakSet::open(&[], &AesKey([0; 32])).unwrap();
    let paths = BuildPaths::new("MultiVersus", &[]).unwrap();
    let a: Vec<Item> = ALPHA.items.iter().map(|p| Item::new(*p)).collect();
    let b: Vec<Item> = BETA.items.iter().map(|p| Item::new(*p)).collect();
    let t = tasks::cut(&[&a, &b], &paks, &paths, 4);
    let p = t.iter().find(|t| t.package == "/Game/P/A").unwrap();
    assert_eq!(p.items, [(0, 0), (1, 0)]);
    assert_eq!(t.len(), 6);
    // a shader-like item goes with its source's package
    let s = [Item { path: "/ShaderCode/01".into(), source: Some("/Game/Q/C.C".into()) }];
    let t = tasks::cut(&[&a, &s], &paks, &paths, 4);
    assert_eq!(t.iter().find(|t| t.package == "/Game/Q/C").unwrap().items, [(0, 2), (1, 0)]);
}

/// Kinds whose lists share one value made by a parallel loop (the material scan of shadermap and shader).
struct Scanning(&'static str);

struct Scan(u64);

impl Kind for Scanning {
    fn id(&self) -> &'static str {
        self.0
    }
    fn version(&self) -> u64 {
        1
    }
    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        let scan = cx.shared.get_or_init(|| {
            Scan(
                (0..256u64)
                    .into_par_iter()
                    .map(|i| {
                        std::thread::sleep(std::time::Duration::from_micros(50));
                        i
                    })
                    .sum(),
            )
        });
        Ok(vec![Item::new(format!("/Game/S/{}.{}", scan.0, self.0))])
    }
    fn build(&self, _: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        Ok(item.path.clone().into_bytes())
    }
}

#[test]
fn lists_sharing_a_scan_do_not_deadlock() {
    static KS: [Scanning; 6] =
        [Scanning("sa"), Scanning("sb"), Scanning("sc"), Scanning("sd"), Scanning("se"), Scanning("sf")];
    let registry: Vec<&dyn Kind> = KS.iter().map(|k| k as &dyn Kind).collect();
    let paks = Arc::new(PakSet::open(&[], &AesKey([0; 32])).unwrap());
    let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
    for round in 0..40 {
        let dir = scratch(&format!("scan{round}"));
        let opts = ImportOptions { store: dir.clone(), threads: Some(4), ..Default::default() };
        let s = import_kinds(&paks, &paths, &registry, "b", &opts, &|_| {}).unwrap();
        assert!(s.kinds.iter().all(|k| k.status == KindStatus::Built && k.entries == 1));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// A kind built from the store: an entry per kind it reads, the read kind's entries' paths and sizes.
struct Reader {
    id: &'static str,
    reads: &'static [(&'static str, u64)],
}

impl Kind for Reader {
    fn id(&self) -> &'static str {
        self.id
    }
    fn version(&self) -> u64 {
        1
    }
    fn content_type(&self, _: &[u8]) -> &'static str {
        "text/plain"
    }
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(self.reads.iter().map(|r| Item::new(format!("/Read/{}.{}", r.0, r.0))).collect())
    }
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let kind = item.path.rsplit('.').next().unwrap();
        let mut out = String::new();
        for p in cx.store.paths(kind)? {
            out += &format!("{p} {}\n", cx.store.entry(kind, &p)?.len());
        }
        Ok(out.into_bytes())
    }
    fn reads(&self) -> &'static [(&'static str, u64)] {
        self.reads
    }
}

#[test]
fn kinds_read_the_store() {
    static READS_ALPHA: Reader = Reader { id: "ra", reads: &[("alpha", 1)] };
    static READS_RA: Reader = Reader { id: "rra", reads: &[("ra", 1)] };
    static READS_NONE: Reader = Reader { id: "rn", reads: &[("missing", 1)] };
    static READS_RN: Reader = Reader { id: "rrn", reads: &[("rn", 1)] };
    static READS_OLD: Reader = Reader { id: "ro", reads: &[("beta", 2)] };
    let registry: [&dyn Kind; 7] = [&READS_RA, &READS_RN, &ALPHA, &BETA, &READS_ALPHA, &READS_NONE, &READS_OLD];
    let paks = Arc::new(PakSet::open(&[], &AesKey([0; 32])).unwrap());
    let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
    let dir = scratch("reads");
    let all = ImportOptions { store: dir.clone(), threads: Some(4), ..Default::default() };
    let opts =
        ImportOptions { kinds: ["rra", "rrn", "alpha", "beta", "ra", "rn"].map(String::from).into(), ..all.clone() };
    let s = import_kinds(&paks, &paths, &registry, "b", &opts, &|_| {}).unwrap();
    // a kind read in the run is switched in before its reader starts, its reader's reader after that
    assert_eq!(status(&s, "ra").status, KindStatus::Built);
    assert_eq!(status(&s, "rra").status, KindStatus::Built);
    let a = entries(&dir, "ra");
    assert_eq!(String::from_utf8(a[0].1.clone()).unwrap().lines().count(), 3);
    assert!(String::from_utf8(entries(&dir, "rra")[0].1.clone()).unwrap().starts_with("/Read/alpha.alpha "));
    // a kind whose read is in no store fails whole, and so does its reader
    let failed = |s: &ImportSummary, k: &str| match &status(s, k).status {
        KindStatus::Failed(why) => why.clone(),
        other => panic!("{k}: {other:?}"),
    };
    assert_eq!(failed(&s, "rn"), "needs missing v1 in the store");
    assert_eq!(failed(&s, "rrn"), "its read rn failed in this run");
    assert_eq!(status(&s, "alpha").status, KindStatus::Built);
    // one that reads a kind of the store (not of the run) at another version
    let s = import_kinds(
        &paks,
        &paths,
        &registry,
        "b",
        &ImportOptions { kinds: vec!["ro".into()], ..all.clone() },
        &|_| {},
    )
    .unwrap();
    assert_eq!(failed(&s, "ro"), "needs beta v2 in the store, which holds v1");

    // the next run: the readers read the store's packs (alpha current, not rebuilt)
    let opts = ImportOptions { kinds: vec!["ra".into()], force: true, ..all };
    let s = import_kinds(&paks, &paths, &registry, "b", &opts, &|_| {}).unwrap();
    assert_eq!(status(&s, "ra").status, KindStatus::Built);
    assert_eq!(entries(&dir, "ra"), a);

    // a registry whose reader reads a kind of the run at another version is refused
    static READS_ALPHA2: Reader = Reader { id: "ra2", reads: &[("alpha", 2)] };
    let registry: [&dyn Kind; 2] = [&ALPHA, &READS_ALPHA2];
    let opts = ImportOptions { kinds: vec![], force: true, ..opts };
    assert!(matches!(import_kinds(&paks, &paths, &registry, "b", &opts, &|_| {}), Err(ImportError::Kinds(_))));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A rebuilt kind makes the kinds built from it stale, and theirs: rebuilt by this run (in later waves), by a forced run
/// of it alone, or by a narrowed one; a plain run after them finds the store current.
#[test]
fn readers_rebuild_after_what_they_read() {
    static RA: Flaky = Flaky { reader: Reader { id: "ra", reads: &[("alpha", 1)] }, fails: &RA_FAILS };
    static RRA: Reader = Reader { id: "rra", reads: &[("ra", 1)] };
    let registry: [&dyn Kind; 4] = [&RRA, &ALPHA, &BETA, &RA];
    let dir = scratch("stale");
    let built = |s: &ImportSummary| -> Vec<String> {
        s.kinds.iter().filter(|k| k.status == KindStatus::Built).map(|k| k.kind.clone()).collect()
    };
    let s = run_with(&registry, &dir, "b", |_| {}).unwrap();
    assert_eq!(built(&s), ["rra", "alpha", "beta", "ra"]);
    // each reader's record names the pack it read
    let m = Store::open(&dir).unwrap().manifest().clone();
    assert_eq!(m.kinds["ra"].reads.iter().collect::<Vec<_>>(), [(&"alpha".to_string(), &m.kinds["alpha"].file)]);
    assert_eq!(m.kinds["rra"].reads["ra"], m.kinds["ra"].file);
    assert!(m.kinds["alpha"].reads.is_empty());
    assert!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()).is_empty());

    // alpha forced alone: its readers are not run, and the next plain run rebuilds them (not beta)
    let s = run_with(&registry, &dir, "b", |o| {
        o.kinds = vec!["alpha".into()];
        o.force = true;
    })
    .unwrap();
    assert_eq!(built(&s), ["alpha"]);
    assert_eq!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()), ["rra", "ra"]);
    assert!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()).is_empty());

    // alpha narrowed (forced, partial): a plain run rebuilds it and, after it, its readers, which read the new pack
    run_with(&registry, &dir, "b", |o| {
        o.kinds = vec!["alpha".into()];
        o.filter = Some("/Game/P/".into());
        o.force = true;
    })
    .unwrap();
    assert_eq!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()), ["rra", "alpha", "ra"]);
    let m = Store::open(&dir).unwrap().manifest().clone();
    assert_eq!(m.kinds["ra"].reads["alpha"], m.kinds["alpha"].file);
    assert_eq!(String::from_utf8(entries(&dir, "ra")[0].1.clone()).unwrap().lines().count(), 3);
    assert!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()).is_empty());

    // a store written before records kept their reads: the kinds that read are rebuilt once
    let mut text: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("store.json")).unwrap()).unwrap();
    for k in text["kinds"].as_object_mut().unwrap().values_mut() {
        k.as_object_mut().unwrap().remove("reads");
    }
    std::fs::write(dir.join("store.json"), serde_json::to_vec(&text).unwrap()).unwrap();
    assert!(Store::open(&dir).is_ok());
    assert_eq!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()), ["rra", "ra"]);
    assert!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()).is_empty());

    // a read kind that fails in the run: its reader in a later wave fails whole and keeps its record (stale still),
    // and is rebuilt by the run in which its read builds
    let rec = |k: &str| Store::open(&dir).unwrap().manifest().kinds[k].file.clone();
    let (ra, rra) = (rec("ra"), rec("rra"));
    run_with(&registry, &dir, "b", |o| {
        o.kinds = vec!["alpha".into()];
        o.force = true;
    })
    .unwrap();
    RA_FAILS.store(true, Ordering::Relaxed);
    let s = run_with(&registry, &dir, "b", |_| {}).unwrap();
    RA_FAILS.store(false, Ordering::Relaxed);
    assert!(matches!(&status(&s, "ra").status, KindStatus::Failed(w) if w.contains("no list")));
    assert_eq!(status(&s, "rra").status, KindStatus::Failed("its read ra failed in this run".into()));
    assert_eq!((rec("ra"), rec("rra")), (ra, rra));
    assert_eq!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()), ["rra", "ra"]);
    assert!(built(&run_with(&registry, &dir, "b", |_| {}).unwrap()).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Set: the `ra` reader of `readers_rebuild_after_what_they_read` fails its list.
static RA_FAILS: AtomicBool = AtomicBool::new(false);

/// A reader whose list fails while `fails` is set.
struct Flaky {
    reader: Reader,
    fails: &'static AtomicBool,
}

impl Kind for Flaky {
    fn id(&self) -> &'static str {
        self.reader.id
    }
    fn version(&self) -> u64 {
        1
    }
    fn content_type(&self, b: &[u8]) -> &'static str {
        self.reader.content_type(b)
    }
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(KindError::Failed("no list".into()));
        }
        self.reader.list(cx)
    }
    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        self.reader.build(cx, item)
    }
    fn reads(&self) -> &'static [(&'static str, u64)] {
        self.reader.reads
    }
}

/// A kind whose list fails.
struct BadList;

impl Kind for BadList {
    fn id(&self) -> &'static str {
        "badlist"
    }
    fn version(&self) -> u64 {
        1
    }
    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/json"
    }
    fn list(&self, _: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Err(KindError::Failed("no list".into()))
    }
    fn build(&self, _: &Cx<'_>, _: &Item) -> Result<Vec<u8>, KindError> {
        unreachable!("nothing listed")
    }
}

fn run_with(
    registry: &[&dyn Kind],
    dir: &Path,
    build: &str,
    f: impl FnOnce(&mut ImportOptions),
) -> Result<ImportSummary, ImportError> {
    let paks = Arc::new(PakSet::open(&[], &AesKey([0; 32])).unwrap());
    let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
    let mut opts = ImportOptions { store: dir.to_path_buf(), threads: Some(4), ..Default::default() };
    f(&mut opts);
    import_kinds(&paks, &paths, registry, build, &opts, &|_| {})
}

#[test]
fn another_build_stays_whole_when_a_kind_fails() {
    let dir = scratch("keepold");
    run_with(&[&ALPHA, &BETA], &dir, "build1", |_| {}).unwrap();
    let manifest = std::fs::read(dir.join("store.json")).unwrap();
    let old: Vec<String> = files(&dir);
    let before = entries(&dir, "alpha");

    let s = run_with(&[&ALPHA, &BETA, &BadList], &dir, "build2", |_| {}).unwrap();
    assert!(matches!(&status(&s, "badlist").status, KindStatus::Failed(w) if w.contains("no list")));
    // the kinds written are not switched in
    assert!(matches!(&status(&s, "alpha").status, KindStatus::Failed(w) if w.contains("not switched in")));
    assert_eq!(std::fs::read(dir.join("store.json")).unwrap(), manifest);
    for f in &old {
        assert!(dir.join(f).is_file(), "{f} of the old build is gone");
    }
    assert_eq!(entries(&dir, "alpha"), before);
    assert_eq!(Store::open(&dir).unwrap().manifest().build_id, "build1");

    // a run that does build everything switches
    let s = run_with(&[&ALPHA, &BETA], &dir, "build2", |_| {}).unwrap();
    assert_eq!(status(&s, "alpha").status, KindStatus::Built);
    assert_eq!(Store::open(&dir).unwrap().manifest().build_id, "build2");
    for f in old.iter().filter(|f| f.contains('@') || f.starts_with("catalog")) {
        assert!(!dir.join(f).exists(), "{f} of the old build is left");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_switch_leaves_a_pack_being_written() {
    let dir = scratch("writing");
    std::fs::create_dir_all(&dir).unwrap();
    let next = Manifest {
        format: STORE_FORMAT,
        build_id: "b".into(),
        catalog: None,
        catalog_version: None,
        kinds: Default::default(),
    };
    let reads = StoreReads::none();
    let switch = Switch {
        dir: &dir,
        state: Mutex::new(SwitchState { next, deferred: false }),
        writing: Mutex::new(Vec::new()),
        reads: &reads,
    };
    let tmp = dir.join("alpha@1-zz.pack.tmp");
    std::fs::write(&tmp, b"x").unwrap();
    // registered after the caller started, before the folder is read
    switch.writing(["alpha@1-zz.pack", "alpha@1-zz.pack.tmp"], true);
    switch.switch_in(|_| {}).unwrap();
    assert!(tmp.is_file());
    switch.writing(["alpha@1-zz.pack", "alpha@1-zz.pack.tmp"], false);
    switch.switch_in(|_| {}).unwrap();
    assert!(!tmp.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_entry_that_does_not_encode_drops_the_kind() {
    let dir = scratch("encode");
    std::fs::create_dir_all(&dir).unwrap();
    let next = Manifest {
        format: STORE_FORMAT,
        build_id: "b".into(),
        catalog: None,
        catalog_version: None,
        kinds: Default::default(),
    };
    let reads = StoreReads::none();
    let switch = Switch {
        dir: &dir,
        state: Mutex::new(SwitchState { next, deferred: false }),
        writing: Mutex::new(Vec::new()),
        reads: &reads,
    };
    let kr = KindRun {
        kind: &ALPHA,
        list: Vec::new(),
        remaining: AtomicUsize::new(0),
        done: AtomicU64::new(0),
        failed: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        dead: AtomicBool::new(false),
        started: Instant::now(),
        reads: IndexMap::new(),
    };
    kr.encode_failed();
    assert_eq!(kr.failed.load(Ordering::Relaxed), 1);
    let (tx, rx) = mpsc::channel();
    tx.send(Msg::End).unwrap();
    drop(tx);
    let gate = Gate::new(1 << 20);
    let (fatal, abort) = (Mutex::new(None), AtomicBool::new(false));
    let opts = ImportOptions::default();
    let r = write_kind(&kr, rx, &gate, &switch, "r1", &opts, &|_| {}, &fatal, &abort);
    assert!(matches!(r, Err(w) if w.contains("did not encode")));
    assert!(!switch.state.lock().unwrap().next.kinds.contains_key("alpha"));
    assert!(!files(&dir).iter().any(|f| f.ends_with(".pack")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cancelled_run_switches_nothing_in() {
    let dir = scratch("cancel");
    let paks = Arc::new(PakSet::open(&[], &AesKey([0; 32])).unwrap());
    let paths = Arc::new(BuildPaths::new("MultiVersus", &[]).unwrap());
    // cancelled before it starts: no store is made
    let opts = ImportOptions { store: dir.clone(), threads: Some(4), ..Default::default() };
    opts.cancel.store(true, Ordering::Relaxed);
    let r = import_kinds(&paks, &paths, &REGISTRY, "build1", &opts, &|_| {});
    assert!(matches!(r, Err(ImportError::Cancelled)), "{r:?}");
    assert!(Store::open(&dir).is_err());
    // a whole run, then a forced one cancelled as its builds start: the first run's kinds stay (its new catalog,
    // switched in before the builds, stays too)
    run(&dir, |_| {}).unwrap();
    let before = Store::open(&dir).unwrap().manifest().clone();
    let opts = ImportOptions { store: dir.clone(), threads: Some(4), force: true, ..Default::default() };
    let cancel = opts.cancel.clone();
    let r = import_kinds(&paks, &paths, &REGISTRY, "build1", &opts, &|p| {
        if p.phase == Phase::Build {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(r, Err(ImportError::Cancelled)), "{r:?}");
    let after = Store::open(&dir).unwrap().manifest().clone();
    assert_eq!(serde_json::to_value(&after.kinds).unwrap(), serde_json::to_value(&before.kinds).unwrap());
    assert!(!files(&dir).iter().any(|f| f.ends_with(".tmp")));
    std::fs::remove_dir_all(&dir).unwrap();
}
