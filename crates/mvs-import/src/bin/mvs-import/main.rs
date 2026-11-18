//! The development CLI over the import's library (the checks; players run the import from the launcher).
//!
//! The paks are the developer's (`--paks <dir>`, else `MVS_PAKS`, else `content/local.json`'s "paks"); the key is read
//! from the exe beside them, never given or printed. Every command prints its wall time.

mod compare;
mod menuscene_compare;
mod soundbank_compare;

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use compare::Format;
use mvs_import::catalog::scan_catalog;
use mvs_import::kind::{Cx, Failure, Item, Kind, Phase, Progress, Shared};
use mvs_import::kinds::{self, KINDS};
use mvs_import::lock::MachineLock;
use mvs_import::reads::StoreReads;
use mvs_import::run::{Budget, ImportOptions, KindStatus, import};
use mvs_pak::{BuildPaths, PakSet, pak_files_in, setup};
use mvs_store::writer::{PackWriter, encode_entry};
use mvs_store::{Pack, Store, dev_store_dir};
use mvs_uasset::PackageCache;
use mvs_uasset::package::FileSource;
use rayon::prelude::*;

const USAGE: &str = "mvs-import [--paks <dir>] <command>
  setup <paks dir>                   what the folder is, the setup check (hash pass on all cores), the key's indexes
  ls [prefix]                        the paks' paths (with a prefix: those under it, without regard to case)
  cat <pak path> [-o <file>]         one file of the paks, to stdout or a file
  read-all [--ext <.uasset>] [--threads <n>]
                                     reads every file with that extension once, on all cores (the reader's speed)
  kind <id> [--out <pack>] [--catalog <file>] [--filter <path prefix>] [--threads <n>] [--budget <MB>]
                                     [--store <dir>]
                                     one kind over the build (the catalog, its list, its builds); written only with
                                     --out (--catalog: the catalog too, as the store's file); takes the machine's
                                     import lock; --store: the store a game kind reads, else the dev store
  import --store <dir> [--kind <id>]... [--force] [--threads <n>] [--filter <path prefix>] [--dev-<id>]...
                                     [--budget <MB>]
                                     every stale kind into the store (--kind: these alone; --dev-object,
                                     --dev-shadermap: a kind the game does not read, built as its proof;
                                     --budget: the package cache's, else from the free memory); takes the
                                     machine's import lock and the store's
  entry <kind> <path> [--source <object path>] [-o <file>] [--store <dir>]
                                     one entry of a kind (its JSON, or a container's header, to stdout; -o: its bytes;
                                     --source: the item's source, a shader code's material; --store: as kind)
  compare <ours.pack> --kind <id> [--store <oracle store dir>] [--report <file>] [--deep]
                                     a kind's pack against the oracle store's (--deep: decode equal entries too)";

type Res<T> = Result<T, String>;

fn main() {
    let t0 = Instant::now();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let paks = take_opt(&mut args, "--paks").map(PathBuf::from);
    let r = match args.first().map(String::as_str) {
        Some("setup") => cmd_setup(&args[1..]),
        Some("ls") => cmd_ls(paks, &args[1..]),
        Some("cat") => cmd_cat(paks, &mut args[1..].to_vec()),
        Some("read-all") => cmd_read_all(paks, &mut args[1..].to_vec()),
        Some("kind") => cmd_kind(paks, &mut args[1..].to_vec()),
        Some("import") => cmd_import(paks, &mut args[1..].to_vec()),
        Some("entry") => cmd_entry(paks, &mut args[1..].to_vec()),
        Some("compare") => cmd_compare(&mut args[1..].to_vec()),
        _ => Err(USAGE.to_string()),
    };
    let wall = t0.elapsed().as_secs_f64();
    match r {
        Ok(()) => eprintln!("wall {wall:.2} s"),
        Err(e) => {
            eprintln!("{e}");
            eprintln!("wall {wall:.2} s");
            std::process::exit(1);
        }
    }
}

/// Removes `--name <value>` from `args`, returning the value.
fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn take_num(args: &mut Vec<String>, name: &str) -> Res<Option<usize>> {
    take_opt(args, name).map(|v| v.parse().map_err(|_| format!("{name}: not a number: {v}"))).transpose()
}

/// The developer's paks folder: `--paks`, `MVS_PAKS`, else `content/local.json`'s "paks".
fn paks_dir(given: Option<PathBuf>) -> Res<PathBuf> {
    if let Some(d) = given.or_else(|| std::env::var_os("MVS_PAKS").map(PathBuf::from)) {
        return Ok(d);
    }
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/local.json");
    let text = std::fs::read_to_string(&local).map_err(|_| format!("no --paks, MVS_PAKS or {}", local.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", local.display()))?;
    v.get("paks")
        .and_then(|p| p.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} has no \"paks\"", local.display()))
}

/// The paks of the folder, opened with the key from the exe beside them.
fn open_paks(given: Option<PathBuf>) -> Res<Arc<PakSet>> {
    let dir = paks_dir(given)?;
    let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).map_err(|e| e.to_string())?;
    let files = pak_files_in(&dir).map_err(|e| e.to_string())?;
    let t = Instant::now();
    let set = PakSet::open(&files, &key).map_err(|e| e.to_string())?;
    eprintln!(
        "{} paks, {} paths, opened in {:.0} ms; {}",
        files.len(),
        set.len(),
        t.elapsed().as_secs_f64() * 1e3,
        peak_memory()
    );
    Ok(Arc::new(set))
}

fn mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
}

/// The system allocator, the bytes it holds for the program counted: what the import's own data takes, apart from
/// the allocator's free lists and the stacks that commit also counts.
struct Counting;

static HEAP: AtomicUsize = AtomicUsize::new(0);
static HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);

fn heap_add(n: usize) {
    let now = HEAP.fetch_add(n, Ordering::Relaxed) + n;
    HEAP_PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: forwards every call to the system allocator unchanged; the counters are atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: the caller's contract is the system allocator's.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            heap_add(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        HEAP.fetch_sub(l.size(), Ordering::Relaxed);
        // SAFETY: `p` came from `alloc` above, which is the system allocator's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        // SAFETY: as `alloc`.
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            HEAP.fetch_sub(l.size(), Ordering::Relaxed);
            heap_add(n);
        }
        q
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The process's peak working set and peak commit (Windows), and the program's heap now and at its peak.
fn peak_memory() -> String {
    let heap = format!(
        "heap {} (peak {})",
        mb(HEAP.load(Ordering::Relaxed) as u64),
        mb(HEAP_PEAK.load(Ordering::Relaxed) as u64)
    );
    format!("{}, {heap}", os_peak_memory())
}

/// The process's peak working set and peak commit (Windows).
fn os_peak_memory() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut c = PROCESS_MEMORY_COUNTERS::default();
        // SAFETY: the current process's pseudo handle and a counters struct of the size given.
        let ok =
            unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, size_of::<PROCESS_MEMORY_COUNTERS>() as u32) };
        if ok.is_ok() {
            return format!(
                "peak working set {}, peak commit {}",
                mb(c.PeakWorkingSetSize as u64),
                mb(c.PeakPagefileUsage as u64)
            );
        }
    }
    "peak memory unknown".into()
}

fn pool(threads: Option<usize>) -> Res<rayon::ThreadPool> {
    let mut b = rayon::ThreadPoolBuilder::new();
    if let Some(n) = threads {
        b = b.num_threads(n);
    }
    b.build().map_err(|e| e.to_string())
}

fn cmd_setup(args: &[String]) -> Res<()> {
    let [dir] = args else { return Err("setup <paks dir>".into()) };
    let build = setup::build_list().map_err(|e| e.to_string())?;
    let t = Instant::now();
    let src = setup::source_check(Path::new(dir), &build);
    println!("source: {:?} ({}) [{:.0} ms]", src.kind, src.note, t.elapsed().as_secs_f64() * 1e3);
    if src.paks.as_os_str().is_empty() {
        return Err("no paks folder".into());
    }
    println!("paks folder: {}", src.paks.display());
    let key = setup::key_from_exe(&setup::exe_beside_paks(&src.paks)).map_err(|e| e.to_string())?;
    let last = AtomicU64::new(u64::MAX);
    let progress = |done: u64, total: u64, file: &str| {
        let pct = done * 20 / total.max(1);
        if last.swap(pct, Ordering::Relaxed) != pct {
            eprintln!("  hashed {:>3}% ({}), {file}", pct * 5, mb(done));
        }
    };
    let t = Instant::now();
    setup::setup_check(&src.paks, &key, &build, Some(&progress), None).map_err(|e| e.to_string())?;
    let s = t.elapsed().as_secs_f64();
    let total: u64 = build.paks.iter().map(|p| p.size).sum();
    println!(
        "setup check passed: {} paks, {} hashed in {s:.1} s ({:.0} MB/s), build {}",
        build.paks.len(),
        mb(total),
        total as f64 / s / (1u64 << 20) as f64,
        build.id
    );
    let files: Vec<PathBuf> = build.paks.iter().map(|p| src.paks.join(&p.name)).collect();
    let t = Instant::now();
    let set = PakSet::open(&files, &key).map_err(|e| e.to_string())?;
    println!("the key opens every index: {} paths in {:.0} ms", set.len(), t.elapsed().as_secs_f64() * 1e3);
    Ok(())
}

fn cmd_ls(paks: Option<PathBuf>, args: &[String]) -> Res<()> {
    let set = open_paks(paks)?;
    let prefix = args.first().map(|p| p.to_lowercase()).unwrap_or_default();
    let mut n = 0;
    let mut out = String::new();
    for e in set.entries() {
        if e.path.to_lowercase().starts_with(&prefix) {
            out.push_str(&format!("{:>12} {}\n", e.uncompressed_size, e.path));
            n += 1;
        }
    }
    print!("{out}");
    eprintln!("{n} paths");
    Ok(())
}

fn cmd_cat(paks: Option<PathBuf>, args: &mut Vec<String>) -> Res<()> {
    let out = take_opt(args, "-o");
    let [path] = &args[..] else { return Err("cat <pak path> [-o <file>]".into()) };
    let set = open_paks(paks)?;
    let t = Instant::now();
    let b = set.read(path).map_err(|e| e.to_string())?;
    eprintln!("{} bytes read in {:.1} ms", b.len(), t.elapsed().as_secs_f64() * 1e3);
    match out {
        Some(f) => std::fs::write(&f, &b).map_err(|e| format!("{f}: {e}")),
        None => {
            use std::io::Write;
            std::io::stdout().write_all(&b).map_err(|e| e.to_string())
        }
    }
}

fn cmd_read_all(paks: Option<PathBuf>, args: &mut Vec<String>) -> Res<()> {
    let ext = take_opt(args, "--ext").unwrap_or_else(|| ".uasset".into()).to_lowercase();
    let threads = take_num(args, "--threads")?;
    let set = open_paks(paks)?;
    let list: Vec<&str> = set.paths().filter(|p| p.to_lowercase().ends_with(&ext)).collect();
    let pool = pool(threads)?;
    let t = Instant::now();
    let bytes = AtomicU64::new(0);
    let failed: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pool.install(|| {
        list.par_iter().for_each(|p| match set.read(p) {
            Ok(b) => {
                bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
            }
            Err(e) => failed.lock().unwrap().push(format!("{p}: {e}")),
        })
    });
    let s = t.elapsed().as_secs_f64();
    let b = bytes.into_inner();
    let failed = failed.into_inner().unwrap();
    println!(
        "{} files of {ext} read on {} threads: {} in {s:.2} s ({:.0} MB/s, {:.0} files/s), {} failed; {}",
        list.len(),
        pool.current_num_threads(),
        mb(b),
        b as f64 / s / (1u64 << 20) as f64,
        list.len() as f64 / s,
        failed.len(),
        peak_memory()
    );
    for f in failed.iter().take(20) {
        println!("  {f}");
    }
    Ok(())
}

/// The kind named `id`.
fn kind_named(id: &str) -> Res<&'static dyn Kind> {
    kinds::kind(id).ok_or_else(|| {
        let ids: Vec<&str> = KINDS.iter().map(|k| k.id()).collect();
        format!("no kind {id}; the kinds: {}", ids.join(", "))
    })
}

/// The store a game kind reads (`--store`, else the dev store), checked to hold its reads; none for a kind built from
/// the paks alone.
fn store_reads(given: Option<String>, kind: &dyn Kind) -> Res<StoreReads> {
    let dir = given.map(PathBuf::from).unwrap_or_else(dev_store_dir);
    if kind.reads().is_empty() {
        return Ok(StoreReads::none());
    }
    let reads = StoreReads::open(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    reads.check(kind.reads()).map_err(|why| format!("the {} kind {why} at {}", kind.id(), dir.display()))?;
    Ok(reads)
}

/// An entry on its way to the pack: path, content type, stored bytes, codec, own size.
type Stored = (String, &'static str, Vec<u8>, u8, u32);

fn cmd_kind(paks: Option<PathBuf>, args: &mut Vec<String>) -> Res<()> {
    let out = take_opt(args, "--out").map(PathBuf::from);
    let catalog_file = take_opt(args, "--catalog");
    let filter = take_opt(args, "--filter");
    let threads = take_num(args, "--threads")?;
    let budget = take_num(args, "--budget")?.unwrap_or(1024) << 20;
    let store = take_opt(args, "--store");
    let [id] = &args[..] else { return Err(format!("kind <id> [--out <pack>] ...; unknown arguments {args:?}")) };
    let kind = kind_named(id)?;
    let reads = store_reads(store, kind)?;
    mvs_import::run::memory::lower_priority();
    let _lock = MachineLock::take(|pid| eprintln!("waiting for the import of process {pid} (the machine's lock)"))
        .map_err(|e| format!("the machine's import lock: {e}"))?;
    let set = open_paks(paks)?;
    let paths = Arc::new(BuildPaths::from_paks(&set).map_err(|e| e.to_string())?);
    let pool = pool(threads)?;

    let t = Instant::now();
    let (catalog, catalog_failures) = pool.install(|| scan_catalog(&set, &paths, &|_| {}));
    println!(
        "catalog: {} packages, {} exports, {} classes, {} failures in {:.1} s",
        catalog.packages.len(),
        catalog.len(),
        catalog.classes.len(),
        catalog_failures.len(),
        t.elapsed().as_secs_f64()
    );
    if let Some(f) = &catalog_file {
        let b = mvs_import::json::to_vec(&catalog).map_err(|e| e.to_string())?;
        std::fs::write(f, b).map_err(|e| format!("{f}: {e}"))?;
    }
    let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), budget);
    let shared = Shared::default();
    let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };

    let t = Instant::now();
    let mut items = pool.install(|| kind.list(&cx)).map_err(|e| format!("{id} list: {e}"))?;
    if let Some(f) = &filter {
        items.retain(|i| i.path.starts_with(f.as_str()));
    }
    println!("{id}: {} items listed in {:.1} s", items.len(), t.elapsed().as_secs_f64());

    // the workers build and compress; one writer thread appends
    let (tx, rx) = mpsc::sync_channel::<Stored>(1024);
    let writer = out.clone().map(|file| {
        let id = id.clone();
        std::thread::spawn(move || -> Res<Option<mvs_store::writer::PackSummary>> {
            let mut w = PackWriter::create(&file).map_err(|e| e.to_string())?;
            for (path, ty, data, codec, size) in rx {
                w.add_stored(&path, ty, &data, codec, size).map_err(|e| format!("{id} write: {e}"))?;
            }
            w.finish().map(Some).map_err(|e| e.to_string())
        })
    });
    let entries = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let failures: Mutex<Vec<Failure>> = Mutex::new(catalog_failures);
    let fail = |path: &str, message: String| {
        failures.lock().unwrap().push(Failure { kind: id.clone(), path: path.to_string(), message })
    };
    let t = Instant::now();
    pool.install(|| {
        items.par_iter().for_each_with(tx, |tx, item| {
            match kind.build(&cx, item) {
                Ok(b) => {
                    entries.fetch_add(1, Ordering::Relaxed);
                    bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
                    if out.is_some() {
                        let ty = kind.content_type(&b);
                        match (encode_entry(&b, ty), u32::try_from(b.len())) {
                            // a send fails only once the writer has stopped on an error, which it reports
                            (Ok((data, codec)), Ok(size)) => {
                                let _ = tx.send((item.path.clone(), ty, data.into_owned(), codec, size));
                            }
                            (Err(e), _) => fail(&item.path, e.to_string()),
                            (_, Err(_)) => fail(&item.path, format!("an entry of {} bytes", b.len())),
                        }
                    }
                }
                Err(e) => fail(&item.path, e.to_string()),
            }
            cache.trim();
        })
    });
    let summary = match writer {
        Some(w) => w.join().map_err(|_| "the writer thread panicked".to_string())??,
        None => None,
    };
    let s = t.elapsed().as_secs_f64();
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    for f in &failures {
        println!("FAIL {} {}: {}", f.kind, f.path, f.message);
    }
    println!(
        "{id} v{}: {} items, {} entries ({}), {} failures in {s:.1} s on {} threads; {}",
        kind.version(),
        items.len(),
        entries.into_inner(),
        mb(bytes.into_inner()),
        failures.len(),
        pool.current_num_threads(),
        peak_memory()
    );
    if let Some(sum) = summary {
        println!("wrote {} ({} entries, {})", sum.path.display(), sum.entries, mb(sum.bytes));
    }
    Ok(())
}

/// The import into a store: progress lines on stderr, a line per kind and every failure on stdout.
fn cmd_import(paks: Option<PathBuf>, args: &mut Vec<String>) -> Res<()> {
    let store = take_opt(args, "--store").map(PathBuf::from).ok_or("import: --store <dir>")?;
    let mut opts = ImportOptions {
        store,
        threads: take_num(args, "--threads")?,
        filter: take_opt(args, "--filter"),
        ..Default::default()
    };
    if let Some(mb) = take_num(args, "--budget")? {
        let mut b = Budget::now();
        b.cache = mb << 20;
        opts.budget = Some(b);
    }
    while let Some(k) = take_opt(args, "--kind") {
        opts.kinds.push(k);
    }
    let mut rest = Vec::new();
    for a in args.drain(..) {
        match a.strip_prefix("--dev-") {
            Some(id) => opts.unstored.push(id.to_string()),
            None if a == "--force" => opts.force = true,
            None => rest.push(a),
        }
    }
    if !rest.is_empty() {
        return Err(format!("import: unknown arguments {rest:?}"));
    }
    let _lock = MachineLock::take(|pid| eprintln!("waiting for the import of process {pid} (the machine's lock)"))
        .map_err(|e| format!("the machine's import lock: {e}"))?;
    let set = open_paks(paks)?;
    let t0 = Instant::now();
    // a line per tenth of a phase (per kind)
    let last: Mutex<Vec<(Phase, String, u64)>> = Mutex::new(Vec::new());
    let progress = |p: Progress<'_>| {
        let kind = p.kind.unwrap_or("");
        let tenth = p.done * 10 / p.total.max(1);
        let mut l = last.lock().unwrap();
        match l.iter_mut().find(|e| e.0 == p.phase && e.1 == kind) {
            Some(e) if e.2 == tenth => return,
            Some(e) => e.2 = tenth,
            None => l.push((p.phase, kind.to_string(), tenth)),
        }
        drop(l);
        let bytes = if p.bytes > 0 { format!(", {}", mb(p.bytes)) } else { String::new() };
        let mem = format!("; {}", peak_memory());
        eprintln!("  {:?} {kind} {}/{}{bytes} at {:.1} s{mem}", p.phase, p.done, p.total, t0.elapsed().as_secs_f64());
    };
    let s = import(&set, &opts, &progress).map_err(|e| e.to_string())?;
    for f in &s.failures {
        println!("FAIL {} {}: {}", f.kind, f.path, f.message);
    }
    let mut failed_kinds = Vec::new();
    for k in &s.kinds {
        let status = match &k.status {
            KindStatus::Built => "built".to_string(),
            KindStatus::Current => "current".to_string(),
            KindStatus::Failed(why) => {
                failed_kinds.push(k.kind.clone());
                format!("NOT BUILT: {why}")
            }
        };
        println!(
            "{} v{}: {status}; {} entries, {}, {} failed, {:.1} s",
            k.kind,
            k.version,
            k.entries,
            mb(k.bytes),
            k.failed,
            k.seconds
        );
    }
    println!(
        "import of build {} into {}: {} packages, {} failures, {:.1} s on {} threads; cache budget {}, entries in flight at most {} of {}; {}",
        s.build_id,
        opts.store.display(),
        s.packages,
        s.failures.len(),
        s.seconds,
        s.threads,
        mb(s.budget.cache as u64),
        mb(s.entries_peak),
        mb(s.budget.entries),
        peak_memory()
    );
    if failed_kinds.is_empty() { Ok(()) } else { Err(format!("kinds not built: {}", failed_kinds.join(", "))) }
}

/// One entry of a kind, built with the catalog the whole import scans (a build that lists packages needs it).
fn cmd_entry(paks: Option<PathBuf>, args: &mut Vec<String>) -> Res<()> {
    let out = take_opt(args, "-o");
    let source = take_opt(args, "--source");
    let store = take_opt(args, "--store");
    let [id, path] = &args[..] else {
        return Err("entry <kind> <path> [--source <object path>] [-o <file>] [--store <dir>]".into());
    };
    let kind = kind_named(id)?;
    let reads = store_reads(store, kind)?;
    let set = open_paks(paks)?;
    let paths = Arc::new(BuildPaths::from_paks(&set).map_err(|e| e.to_string())?);
    let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), 1 << 30);
    let (catalog, _) = scan_catalog(&set, &paths, &|_| {});
    let shared = Shared::default();
    let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
    let item = Item { path: path.clone(), source };
    let b = kind.build(&cx, &item).map_err(|e| format!("{path}: {e}"))?;
    let ty = kind.content_type(&b);
    if let Some(f) = out {
        eprintln!("{} bytes, {ty}", b.len());
        return std::fs::write(&f, &b).map_err(|e| format!("{f}: {e}"));
    }
    match compare::format_of(id) {
        Some(Format::JsonBin(magic)) if b.len() >= 8 && &b[..4] == magic => {
            let len = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
            println!("{}", String::from_utf8_lossy(&b[8..(8 + len).min(b.len())]));
            eprintln!("(a json-bin container: the header above, a body of {} bytes)", b.len() - ((8 + len + 15) & !15));
        }
        Some(Format::GameData) if b.len() >= 8 && &b[..4] == b"MVGD" => {
            let len = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
            println!("{}", String::from_utf8_lossy(&b[8..(8 + len).min(b.len())]));
            eprintln!("(a gamedata entry: its JSON above, a body of {} bytes)", b.len() - ((8 + len + 15) & !15));
        }
        Some(Format::Raw) => eprintln!("{} bytes, {ty} (-o <file> to write them)", b.len()),
        _ => println!("{}", String::from_utf8_lossy(&b)),
    }
    Ok(())
}

fn cmd_compare(args: &mut Vec<String>) -> Res<()> {
    let store = take_opt(args, "--store").map(PathBuf::from).unwrap_or_else(dev_store_dir);
    let report = take_opt(args, "--report").map(PathBuf::from);
    let deep = args.iter().position(|a| a == "--deep").map(|i| args.remove(i)).is_some();
    let id = take_opt(args, "--kind").ok_or("compare: --kind <id>")?;
    if id == "soundbank" {
        let [ours] = &args[..] else {
            return Err("compare <ours.pack> --kind soundbank [--store <dir>] [--report <file>]".into());
        };
        let ours = Pack::open(&id, ours).map_err(|e| e.to_string())?;
        let store = Store::open(&store).map_err(|e| e.to_string())?;
        let oracle = store.kind(&id).map_err(|e| e.to_string())?;
        println!("oracle: {} {id} kind v{}", store.dir().display(), store.version(&id).unwrap_or(0));
        return soundbank_compare::run(&ours, &oracle, report.as_deref());
    }
    if id == "menuscene" {
        let [ours] = &args[..] else {
            return Err("compare <ours.pack> --kind menuscene [--store <dir>] [--report <file>]".into());
        };
        let ours = Pack::open(&id, ours).map_err(|e| e.to_string())?;
        let store = Store::open(&store).map_err(|e| e.to_string())?;
        let oracle = store.kind(&id).map_err(|e| e.to_string())?;
        println!("oracle: {} {id} kind v{}", store.dir().display(), store.version(&id).unwrap_or(0));
        return menuscene_compare::run(&ours, &oracle, report.as_deref());
    }
    let format = compare::format_of(&id).ok_or_else(|| format!("no kind {id}"))?;
    let [ours] = &args[..] else {
        return Err("compare <ours.pack> --kind <id> [--store <dir>] [--report <file>] [--deep]".into());
    };
    let ours = Pack::open(&id, ours).map_err(|e| e.to_string())?;
    let store = Store::open(&store).map_err(|e| e.to_string())?;
    let oracle = store.kind(&id).map_err(|e| e.to_string())?;
    println!("oracle: {} {id} kind v{}", store.dir().display(), store.version(&id).unwrap_or(0));
    compare::run(format, compare::left_out_of(&id), &ours, &oracle, report.as_deref(), deep)
}
