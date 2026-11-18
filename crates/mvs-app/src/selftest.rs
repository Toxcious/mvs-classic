//! `mvs.exe --self-test [--data <dir>] [--store <dir>]`: one `self-test <what>: <value>` line
//! per check on stdout (the version, the exe, the feed, EOS's files, the data root, its store and its import state),
//! then `self-test result: PASS` or `FAIL` and the exit code; no window opens. The sim line is the self check's match
//! (`crate::self_check`) against the release's reference; `--check-out <file>` (the release tool's packing run) runs
//! the store and sim lines alone and writes the reference (`mvs_release::SelfCheckRef`) to <file>.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mvs_release::SelfCheckRef;
use mvs_ui::launcher::import_view::{ImportEvent, Why};
use mvs_ui::launcher::words as w;

use crate::import::{ImportRequest, Worker, WorkerOp};
use crate::launcher::setup_os::{data_root, store_dir};

const USAGE: &str = "mvs --self-test [--data <dir>] [--store <dir>] [--check-out <file>]";

/// The longest the import worker's state answer is waited for.
const STATE_WAIT: Duration = Duration::from_secs(60);

/// The exe runs from a release folder: the licence the packaging tool puts at its top is there.
// Deviation: the exe has no packaging of its own to ask.
pub fn is_release(exe: &Path) -> bool {
    exe.parent().is_some_and(|d| mvs_release::app_root(d).join(mvs_release::LICENSE).is_file())
}

/// The store line's value: the folder, its build and its kinds (each `name@version`, by name); Err when it does not
/// open, holds no kind or was imported from another build than `build` (the beta build this exe reads, the
/// store's refusal of another build).
pub fn store_line(dir: &Path, build: &str) -> Result<String, String> {
    let store = mvs_store::Store::open(dir).map_err(|e| e.to_string())?;
    let m = store.manifest();
    let mut kinds: Vec<String> = m.kinds.iter().map(|(k, r)| format!("{k}@{}", r.version)).collect();
    kinds.sort();
    let line = format!("{}: build {}, {} kinds ({})", dir.display(), m.build_id, kinds.len(), kinds.join(", "));
    if kinds.is_empty() {
        Err(format!("{line}: no kind"))
    } else if m.build_id != build {
        Err(format!("{line}: a store for another build (this exe reads build {build})"))
    } else {
        Ok(line)
    }
}

/// The import worker's state of `root`, worded (setup, then the store's staleness).
fn import_state(root: &Path) -> Result<String, String> {
    let req = ImportRequest { data_root: root.to_path_buf(), ..Default::default() };
    let mut worker = Worker::start(WorkerOp::State, &req).map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        for e in worker.poll() {
            match e {
                ImportEvent::State { state: s } => {
                    let setup = match &s.why {
                        None if s.setup => "setup done".to_owned(),
                        None | Some(Why::SetupNotDone) => "setup not done".to_owned(),
                        Some(why) => format!("setup not done: {}", w::why(why)),
                    };
                    let store = if s.new_build {
                        "store for another build".to_owned()
                    } else if !s.stale.is_empty() {
                        format!("stale: {}", s.stale.join(", "))
                    } else {
                        "store current".to_owned()
                    };
                    return Ok(format!("{setup}, {store}"));
                }
                ImportEvent::Failed { say } => return Err(w::say(&say)),
                _ => {}
            }
        }
        if worker.ended() {
            return Err("the worker ended without its answer".to_owned());
        }
        if start.elapsed() > STATE_WAIT {
            worker.kill();
            return Err(format!("no answer in {} s", STATE_WAIT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The kinds of the store at `dir` that this version's importer would build again (mvs_import::kinds::KINDS against
/// its manifest, on the store's own build); every kind when it has no manifest.
pub fn stale_kinds(dir: &Path) -> Vec<String> {
    let build = mvs_store::Store::open(dir).map(|s| s.manifest().build_id.clone()).unwrap_or_default();
    let asked: Vec<mvs_store::manage::Asked<'_>> = mvs_import::kinds::KINDS
        .iter()
        .filter(|k| k.stored())
        .map(|k| mvs_store::manage::Asked { id: k.id(), version: k.version(), reads: k.reads() })
        .collect();
    mvs_store::manage::staleness(dir, &build, mvs_import::catalog::CATALOG_VERSION, &asked, false).kinds
}

/// The sim line's value: the check match's hash on the store at `dir`, its compatibility id, and what the release's
/// reference in `program` (the exe's folder) says of it; Err when the match does not run or its hash differs.
pub fn sim_line(dir: &Path, program: Option<&Path>) -> Result<(String, SelfCheckRef), String> {
    let store = mvs_store::Store::open(dir).map_err(|e| e.to_string())?;
    let compat = crate::online::glue::compat_id(store.manifest());
    let start = Instant::now();
    let hash = crate::self_check::run(&store)?;
    let line = format!(
        "{hash:08x} after {} ticks in {} ms, for {compat}",
        crate::self_check::CHECK_TICKS,
        start.elapsed().as_millis()
    );
    let made = SelfCheckRef { compat, hash };
    match program.and_then(|p| crate::self_check::reference(p, &made.compat)) {
        Some(r) if r == hash => Ok((format!("{line}: the release's"), made)),
        Some(r) => Err(format!("{line}: the release's is {r:08x} (online play stays off)")),
        None => Ok((format!("{line}: no reference for it beside the exe"), made)),
    }
}

/// The self-test run on the arguments after `--self-test`; the exit code.
pub fn main(mut args: impl Iterator<Item = String>) -> u8 {
    let mut data = None;
    let mut store = None;
    let mut check_out = None;
    while let Some(a) = args.next() {
        let dir = match a.as_str() {
            "--data" => &mut data,
            "--store" => &mut store,
            "--check-out" => &mut check_out,
            _ => {
                eprintln!("mvs: unknown argument {a}\nusage: {USAGE}");
                return 2;
            }
        };
        match args.next() {
            Some(v) => *dir = Some(PathBuf::from(v)),
            None => {
                eprintln!("mvs: {a}: no value\nusage: {USAGE}");
                return 2;
            }
        }
    }
    let say = |k: &str, v: &str| println!("self-test {k}: {v}");
    let mut ok = true;

    say("version", &format!("app {}", mvs_release::VERSION));
    let exe = std::env::current_exe();
    let program = exe.as_ref().ok().and_then(|e| e.parent()).map(Path::to_path_buf);
    match &exe {
        Ok(exe) => {
            let kind = if is_release(exe) { "release" } else { "not a release (no licence beside the exe)" };
            say("build", &format!("{kind}; {}", exe.display()));
        }
        Err(e) => {
            say("build", &format!("FAILED the exe's path: {e}"));
            ok = false;
        }
    }
    say("update feed", mvs_release::FEED.unwrap_or("none: this build never checks"));
    // (no sign-in: only where online play would find EOS)
    match crate::online::glue::eos_files() {
        Some((dll, credentials)) => {
            say("eos", &format!("SDK {}, credentials {}", dll.display(), credentials.display()))
        }
        None => say("eos", "no SDK or credentials: online play is off"),
    }

    let data = mvs_settings::data_dir(data.as_deref());
    say("data", &data.display().to_string());
    let settings = mvs_settings::Settings::load(&data);
    let root = data_root(&settings, &data);
    say("data root", &root.display().to_string());
    let store = store.unwrap_or_else(|| store_dir(&root));
    let build = mvs_pak::setup::build_id().unwrap_or_else(|e| format!("(no build list: {e})"));
    match store_line(&store, &build) {
        Ok(line) => say("store", &line),
        Err(e) => {
            say("store", &format!("FAILED {e}"));
            ok = false;
        }
    }
    let made = if ok {
        match sim_line(&store, program.as_deref()) {
            Ok((line, made)) => {
                say("sim", &line);
                Some(made)
            }
            Err(e) => {
                say("sim", &format!("FAILED {e}"));
                ok = false;
                None
            }
        }
    } else {
        say("sim", "not run: no store");
        None
    };
    if let Some(out) = check_out {
        // (a reference made on a store this version would import again is one no player's store matches)
        let stale = stale_kinds(&store);
        let made = if stale.is_empty() {
            made
        } else {
            say("check out", &format!("FAILED the store's kinds {} are not this version's import", stale.join(", ")));
            None
        };
        let written = made.ok_or_else(|| "no hash".to_owned()).and_then(|m| {
            let text = serde_json::to_string_pretty(&m).expect("a SelfCheckRef always serialises") + "\n";
            std::fs::write(&out, text).map_err(|e| e.to_string())
        });
        match written {
            Ok(()) => say("check out", &out.display().to_string()),
            Err(e) => {
                say("check out", &format!("FAILED {}: {e}", out.display()));
                ok = false;
            }
        }
        // (the packing run: the release's other checks are the release check's)
        return result(ok);
    }
    // Deviation: the worker's state is the data root's store and setup.json; a `--store` elsewhere is checked by the
    // store line alone, so the line names the store it read.
    match import_state(&root) {
        Ok(line) => say("import process", &format!("{}: {line}", store_dir(&root).display())),
        Err(e) => {
            say("import process", &format!("FAILED {e}"));
            ok = false;
        }
    }

    result(ok)
}

/// The result line and the exit code.
fn result(ok: bool) -> u8 {
    if ok {
        println!("{}", mvs_release::SELF_TEST_PASS);
        0
    } else {
        println!("self-test result: FAIL");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_store_fails_and_a_release_needs_its_licence() {
        let dir = std::env::temp_dir().join(format!("mvs-selftest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(store_line(&dir.join("store"), "b").is_err());
        let store = dir.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let manifest = |build: &str| {
            format!(
                r#"{{"format":{},"buildId":"{build}","kinds":{{"text":{{"version":1,"file":"text.pack","entries":0,"bytes":0,"failed":0}}}}}}"#,
                mvs_store::STORE_FORMAT
            )
        };
        std::fs::write(store.join("store.json"), manifest("b")).unwrap();
        assert!(store_line(&store, "b").is_ok());
        std::fs::write(store.join("store.json"), manifest("0000other")).unwrap();
        assert!(store_line(&store, "b").is_err());
        let exe = dir.join("mvs.exe");
        assert!(!is_release(&exe));
        std::fs::write(dir.join(mvs_release::LICENSE), "MIT").unwrap();
        assert!(is_release(&exe));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
