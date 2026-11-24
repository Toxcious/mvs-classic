//! The release check: two test releases built against a local feed and walked through
//! what a player's copy does, driven by the programs' own flags and exit codes. One line per step; the first failed
//! step ends the check.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mvs_release::zip::{self, Entry};
use mvs_release::{
    DATA_DIR, EOS_CREDENTIALS, EOS_DLL, GAME_DIR, GAME_EXE, LAUNCHER_EXE, LICENSE, LICENSES_DIR, Release,
    SELF_TEST_PASS, UPDATE_DIR,
};

use crate::guard::{self, LICENSE_CUE4PARSE, NOTICE_IMPORTER, Zip};
use crate::pack::{game_zip_name, relay_zip_name};
use crate::serve;

/// The two versions: the installed one and the update.
const OLD: &str = "0.1.0";
const NEW: &str = "0.1.1";
/// The file that marks the scratch folder as the check's own, which its next run may empty.
const MARK: &str = ".release-check";
/// `mvs.exe --update-now`'s exit codes (mvs-app update::UPDATE_HANDED_OVER, UPDATE_NONE).
const HANDED_OVER: i32 = 0;
const NO_UPDATE: i32 = 3;
/// The longest a build, a run of a program, and the copy over may take.
const BUILD_LIMIT: Duration = Duration::from_secs(1800);
const RUN_LIMIT: Duration = Duration::from_secs(180);
const COPY_LIMIT: Duration = Duration::from_secs(120);
/// The longest a program's output is waited for after it ended (another process may hold its pipe).
const DRAIN_LIMIT: Duration = Duration::from_secs(5);
/// The longest the whole check may take: past it, what runs from its folder is ended and the check fails.
const CHECK_LIMIT: Duration = Duration::from_secs(90 * 60);
/// The restarted launcher clears `data\update\` 5 s after it opens (mvs-app update CLEAR_AFTER).
const CLEAR_LIMIT: Duration = Duration::from_secs(90);

/// A fake developer's key and EOS pair, planted to see the guard fail: never the real ones.
pub const FAKE_KEY: [u8; 32] = [
    0x5e, 0x11, 0xa7, 0x3c, 0xd2, 0x08, 0x9f, 0x64, 0xb1, 0x2d, 0xe6, 0x47, 0x0a, 0xc9, 0x73, 0x18, 0xfb, 0x52, 0x8e,
    0x21, 0x6d, 0xc0, 0x34, 0x99, 0x1f, 0xa8, 0x45, 0xde, 0x07, 0x7b, 0xb3, 0x6a,
];
const FAKE_SECRET: &str = "release-check-FAKE-client-secret-000";
const FAKE_CLIENT: &str = "release-check-FAKE-client-id-000";

pub struct CheckOptions {
    /// The workspace root (the builds, LICENSE, the scratch folder `.scratch/release-check`).
    pub repo: PathBuf,
    /// The developer's store the self-test reads (only read: given with `--store`).
    pub store: PathBuf,
    /// Leave the scratch folder.
    pub keep: bool,
}

/// A program run to its end: its exit code (None: ended at the limit) and its output, stdout then stderr.
struct Ran {
    code: Option<i32>,
    out: String,
}

/// Runs `cmd` to its end or `limit` (then killed). The developer's EOS pair is never passed on; `eos` sets a fake one.
fn run(mut cmd: Command, limit: Duration, eos: Option<(&Path, &Path)>) -> Result<Ran, String> {
    let what = format!("{}", Path::new(cmd.get_program()).display());
    cmd.env_remove("MVS_EOS_DLL").env_remove("MVS_EOS_CREDENTIALS");
    if let Some((dll, json)) = eos {
        cmd.env("MVS_EOS_DLL", dll).env("MVS_EOS_CREDENTIALS", json);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{what} did not start: {e}"))?;
    // Each pipe is read on a thread into a shared buffer: a program the child started may inherit the pipe and hold
    // it open after the child ends (the updater's hand-over, the restarted launcher), so its end is never waited for
    // past DRAIN_LIMIT.
    type Buf = Arc<Mutex<(Vec<u8>, bool)>>;
    fn drain(mut r: impl Read + Send + 'static) -> Buf {
        let buf: Buf = Arc::default();
        let b = Arc::clone(&buf);
        std::thread::spawn(move || {
            let mut chunk = [0; 4096];
            loop {
                match r.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => b.lock().unwrap_or_else(PoisonError::into_inner).0.extend_from_slice(&chunk[..n]),
                }
            }
            b.lock().unwrap_or_else(PoisonError::into_inner).1 = true;
        });
        buf
    }
    let bufs: Vec<Buf> =
        [child.stdout.take().map(drain), child.stderr.take().map(drain)].into_iter().flatten().collect();
    let start = Instant::now();
    let code = loop {
        if let Some(s) = child.try_wait().map_err(|e| format!("{what}: {e}"))? {
            break s.code();
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let done = |b: &Buf| b.lock().unwrap_or_else(PoisonError::into_inner).1;
    wait_for(DRAIN_LIMIT, || bufs.iter().all(done));
    let text = bufs
        .iter()
        .map(|b| String::from_utf8_lossy(&b.lock().unwrap_or_else(PoisonError::into_inner).0).into_owned())
        .collect();
    Ok(Ran { code, out: text })
}

/// `exe` with `args`, run from its own folder.
fn program(exe: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(d) = exe.parent() {
        cmd.current_dir(d);
    }
    cmd
}

/// The last lines of a program's output, for a failed step's line.
fn tail(out: &str) -> String {
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(12)..].join("\n    ")
}

/// The ids of the processes whose program is under `dir`.
fn pids_under(dir: &Path) -> Vec<u32> {
    // (the path as Windows writes it: `-like` compares the text)
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()).display().to_string();
    let dir = dir.replace('/', "\\").replace('\'', "''");
    let ps = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.ExecutablePath -like '{dir}\\*' }} | ForEach-Object {{ $_.ProcessId }}"
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
    run(cmd, Duration::from_secs(60), None)
        .map(|r| r.out.split_whitespace().filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_default()
}

/// Ends the processes under `dir` by their PIDs: only programs this check started, or that they started.
fn end_under(dir: &Path) {
    for pid in pids_under(dir) {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = run(cmd, Duration::from_secs(30), None);
    }
}

/// Waits until `f` holds or `limit` passes.
fn wait_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    f()
}

/// `<exe> --version`'s first line.
fn version_of(exe: &Path) -> Result<String, String> {
    let r = run(program(exe, &["--version"]), RUN_LIMIT, None)?;
    Ok(r.out.lines().next().unwrap_or("").trim().to_owned())
}

/// The key as hex with every other digit upper case.
fn mixed_hex(key: &[u8]) -> String {
    key.iter()
        .flat_map(|b| [b >> 4, b & 15])
        .enumerate()
        .map(|(i, d)| {
            let c = char::from_digit(u32::from(d), 16).unwrap_or('0');
            if i % 2 == 1 { c.to_ascii_uppercase() } else { c }
        })
        .collect()
}

/// The plants the guard must refuse, each put in a release's files: what it is, the planted file (one of the same
/// name replaced), and the text its finding holds.
pub fn plants() -> Vec<(&'static str, Entry, &'static str)> {
    let file = |name: &str, data: Vec<u8>| Entry { name: name.to_owned(), data, executable: false };
    let mut raw = b"MZ".to_vec();
    raw.extend_from_slice(&FAKE_KEY);
    raw.extend_from_slice(&[0; 16]);
    // the key one byte into longer data
    let mut inner = vec![7];
    inner.extend_from_slice(&FAKE_KEY);
    inner.extend_from_slice(&[1, 2, 3]);
    vec![
        ("a game data file", file("planted.wem", b"x".to_vec()), "a game data file (.wem)"),
        (
            "the key as mixed-case hex",
            file(LICENSE, format!("a line, then {} in it", mixed_hex(&FAKE_KEY)).into_bytes()),
            "as hex",
        ),
        ("the key as raw bytes", file(LICENSE, raw), "as raw bytes"),
        ("the key inside base64url data", file(LICENSE, URL_SAFE_NO_PAD.encode(&inner).into_bytes()), "as base64url"),
        (
            "the EOS client credentials outside resources/eos.json",
            file(LICENSE, format!("{{\"secret\":\"{FAKE_SECRET}\"}}").into_bytes()),
            "the EOS client credentials",
        ),
    ]
}

/// The fake EOS pair's credentials file.
fn fake_eos_json() -> String {
    format!(
        r#"{{"productId":"fake-product","sandboxId":"fake-sandbox","deploymentId":"fake-deployment","clientId":"{FAKE_CLIENT}","clientSecret":"{FAKE_SECRET}"}}"#
    )
}

/// The guard (with the fake key) refuses `entries` with `plant` in them and quotes no secret: Err says what went
/// wrong.
pub fn guard_refuses(entries: &[Entry], plant: &Entry, expect: &str) -> Result<(), String> {
    let mut planted: Vec<Entry> = entries
        .iter()
        .filter(|e| e.name != plant.name)
        .map(|e| Entry { name: e.name.clone(), data: e.data.clone(), executable: e.executable })
        .collect();
    planted.push(Entry { name: plant.name.clone(), data: plant.data.clone(), executable: false });
    let r = guard::check_files("planted.zip", &planted, Zip::Game, Some(&FAKE_KEY), &[]);
    let hex: String = FAKE_KEY.iter().map(|b| format!("{b:02x}")).collect();
    let all = r.findings.join("; ");
    if all.to_ascii_lowercase().contains(&hex[..16]) || all.contains(FAKE_SECRET) {
        return Err("a finding quotes the planted secret".to_owned());
    }
    if !r.findings.iter().any(|f| f.contains(expect)) {
        return Err(format!("no finding \"{expect}\" (found: {})", if all.is_empty() { "nothing" } else { &all }));
    }
    Ok(())
}

/// The developer's key (MVS_PAK_KEY or content/local.json), when there is one: the guard's real search over the
/// builds.
fn real_key(repo: &Path) -> Result<Option<[u8; 32]>, String> {
    let f = repo.join("content/local.json");
    Ok(guard::key_here(Some(f.as_path()).filter(|f| f.is_file()))?.map(|k| k.key))
}

fn read(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display()))
}

/// A step passed.
fn pass(step: &str, detail: &str) {
    if detail.is_empty() {
        println!("PASS {step}");
    } else {
        println!("PASS {step}: {detail}");
    }
}

/// Runs a step: Ok(detail) passes, Err(why) ends the check.
fn step(name: &str, f: impl FnOnce() -> Result<String, String>) -> Result<(), String> {
    match f() {
        Ok(d) => {
            pass(name, &d);
            Ok(())
        }
        Err(e) => Err(format!("FAIL {name}: {e}")),
    }
}

/// `mvs-dist build` (this program) of `version` against the feed into `out`; `extra` its other flags.
fn build(c: &Paths, version: &str, out: &Path, extra: &[&str], eos: Option<(&Path, &Path)>) -> Result<String, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(me);
    cmd.current_dir(&c.repo).arg("build").args(["--version", version, "--feed", &c.feed_url, "--out"]).arg(out);
    cmd.arg("--store").arg(&c.store);
    cmd.args(extra);
    let r = run(cmd, BUILD_LIMIT, eos)?;
    if r.code != Some(0) {
        return Err(format!("exit {:?}\n    {}", r.code, tail(&r.out)));
    }
    let size = std::fs::metadata(out.join(game_zip_name(version))).map(|m| m.len()).unwrap_or(0);
    Ok(format!("{} ({size} bytes), guard passed", game_zip_name(version)))
}

/// Where the check works.
struct Paths {
    repo: PathBuf,
    /// The developer's store (the builds' self check runs on it).
    store: PathBuf,
    feed_url: String,
}

/// The whole check; Err is the failed step's line.
pub fn run_check(o: &CheckOptions) -> Result<(), String> {
    let s = o.repo.join(".scratch").join("release-check");
    if s.exists() {
        if !s.join(MARK).is_file() {
            return Err(format!("release-check: {} is not this check's folder; move it away", s.display()));
        }
        end_under(&s);
        std::fs::remove_dir_all(&s).map_err(|e| format!("release-check: {}: {e}", s.display()))?;
    }
    write(&s.join(MARK), "The release check's folder: emptied by its next run.\n")?;
    // the watchdog: no step hangs the check
    let watched = s.clone();
    std::thread::spawn(move || {
        std::thread::sleep(CHECK_LIMIT);
        println!(
            "FAIL release-check: not done within {} min; what runs from its folder is ended",
            CHECK_LIMIT.as_secs() / 60
        );
        end_under(&watched);
        std::process::exit(1);
    });
    let result = walk(o, &s);
    // (whatever is still running from the check's folders ends before they go)
    end_under(&s);
    if o.keep {
        println!("release-check: {} kept", s.display());
    } else if let Err(e) = std::fs::remove_dir_all(&s) {
        println!("release-check: {} was not removed: {e}", s.display());
    }
    result
}

fn walk(o: &CheckOptions, s: &Path) -> Result<(), String> {
    if !o.store.join("store.json").is_file() {
        return Err(format!("release-check: no store at {} (give one with --store)", o.store.display()));
    }
    // the feed first, on a free port: both builds name it; it serves what is put in its folder later
    let feed_dir = s.join("feed");
    std::fs::create_dir_all(&feed_dir).map_err(|e| e.to_string())?;
    let feed = serve::start(&feed_dir, 0).map_err(|e| format!("release-check: the feed did not start: {e}"))?;
    // (the GitHub API's latest release, in its shape: a file `latest` the update step writes)
    let c = Paths { repo: o.repo.clone(), store: o.store.clone(), feed_url: format!("{}latest", feed.url()) };
    println!("release-check: feed {} in {}", c.feed_url, s.display());

    // 1. two releases against the feed, and the newer one again with a fake EOS pair
    let (rel_old, rel_new, rel_eos) = (s.join(format!("rel-{OLD}")), s.join(format!("rel-{NEW}")), s.join("rel-eos"));
    step(&format!("build {OLD}"), || build(&c, OLD, &rel_old, &[], None))?;
    step(&format!("build {NEW}"), || build(&c, NEW, &rel_new, &[], None))?;
    let (dll, json) = (s.join("fake-eos").join(EOS_DLL), s.join("fake-eos/eos.json"));
    write(&dll, b"MZ a fake EOS SDK for the release check")?;
    write(&json, fake_eos_json())?;
    step(&format!("build {NEW} with a fake EOS pair"), || {
        build(&c, NEW, &rel_eos, &["--no-build", "--eos"], Some((&dll, &json)))
    })?;

    // 2. the guard: passes the builds, refuses each plant
    let key = real_key(&o.repo)?;
    step("the guard passes both releases", || {
        let mut n = 0;
        for rel in [&rel_old, &rel_new] {
            let v = if rel == &rel_old { OLD } else { NEW };
            let g = guard::check_zip(&rel.join(game_zip_name(v)), Zip::Game, key.as_ref(), &[])?;
            let r = guard::check_zip(&rel.join(relay_zip_name(v)), Zip::Relay, key.as_ref(), &g.secrets)?;
            if let Some(f) = g.findings.iter().chain(&r.findings).next() {
                return Err(f.clone());
            }
            n += g.files + r.files;
        }
        Ok(format!(
            "{n} files{}",
            if key.is_some() { ", the developer's key searched" } else { ", no key: no key search" }
        ))
    })?;
    let eos_entries = zip::read(&read(&rel_eos.join(game_zip_name(NEW)))?)?;
    for (what, plant, expect) in plants() {
        step(&format!("the guard refuses {what}"), || {
            guard_refuses(&eos_entries, &plant, expect).map(|()| String::new())
        })?;
    }

    // 3. the old release unpacked
    let app = s.join("app");
    let exe = app.join(GAME_DIR).join(GAME_EXE);
    zip::unpack(&rel_old.join(game_zip_name(OLD)), &app)?;
    step(&format!("{OLD}'s zip unpacks and {GAME_EXE} --version says {OLD}"), || {
        let v = version_of(&exe)?;
        if v == format!("mvs {OLD}") { Ok(v) } else { Err(format!("it says \"{v}\"")) }
    })?;
    step("LICENSE.txt at the top and the importer's notices beside the game's exe", || {
        let licence = String::from_utf8_lossy(&read(&app.join(LICENSE))?).into_owned();
        let notices = [NOTICE_IMPORTER, LICENSE_CUE4PARSE].map(|n| app.join(GAME_DIR).join(LICENSES_DIR).join(n));
        if !licence.starts_with("MIT License") {
            Err("LICENSE.txt does not start \"MIT License\"".to_owned())
        } else if licence.contains("Epic Online Services") || app.join(GAME_DIR).join(EOS_DLL).exists() {
            Err("a build without --eos holds Epic's notice or DLL".to_owned())
        } else if let Some(n) = notices.iter().find(|n| !n.is_file()) {
            Err(format!("no {}", n.display()))
        } else {
            Ok(String::new())
        }
    })?;
    let relay = s.join("relay");
    zip::unpack(&rel_old.join(relay_zip_name(OLD)), &relay)?;
    step(&format!("the relay zip holds mvs-relay.exe, README.md, LICENSE.txt and says {OLD}"), || {
        if let Some(f) = ["mvs-relay.exe", "README.md", LICENSE].iter().find(|f| !relay.join(f).is_file()) {
            return Err(format!("no {f}"));
        }
        let v = version_of(&relay.join("mvs-relay.exe"))?;
        if v == format!("mvs-relay {OLD}") || v.starts_with(&format!("mvs-relay {OLD} ")) {
            Ok(v)
        } else {
            Err(format!("mvs-relay --version says \"{v}\""))
        }
    })?;

    // 4. the self-test, on the developer's store and on an empty data root
    let store = o.store.display().to_string();
    let data_ok = s.join("selftest-data");
    std::fs::create_dir_all(&data_ok).map_err(|e| e.to_string())?;
    let r = run(
        program(&exe, &["--self-test", "--data", &data_ok.display().to_string(), "--store", &store]),
        RUN_LIMIT,
        None,
    )?;
    for l in r.out.lines().filter(|l| l.starts_with("self-test ")) {
        println!("    {l}");
    }
    step("--self-test passes on the developer's store and names the version", || {
        if r.code == Some(0) && r.out.trim_end().ends_with(SELF_TEST_PASS) && r.out.contains(&format!("app {OLD}")) {
            Ok(String::new())
        } else {
            Err(format!("exit {:?}\n    {}", r.code, tail(&r.out)))
        }
    })?;
    step("--self-test without EOS files says online play is off", || {
        if r.out.contains("self-test eos: no SDK or credentials: online play is off") {
            Ok(String::new())
        } else {
            Err("no such eos line".to_owned())
        }
    })?;
    let empty = s.join("selftest-empty");
    std::fs::create_dir_all(&empty).map_err(|e| e.to_string())?;
    let r = run(program(&exe, &["--self-test", "--data", &empty.display().to_string()]), RUN_LIMIT, None)?;
    step("--self-test fails on an empty data root", || {
        if r.code == Some(1) && r.out.trim_end().ends_with("self-test result: FAIL") {
            Ok(String::new())
        } else {
            Err(format!("exit {:?}\n    {}", r.code, tail(&r.out)))
        }
    })?;

    // 5. the update from the feed
    let data = app.join(DATA_DIR);
    let planted = [
        (data.join("planted.txt"), b"the player's own file\n".to_vec()),
        (data.join("store/store.json"), read(&o.store.join("store.json"))?),
    ];
    for (p, d) in &planted {
        write(p, d)?;
    }
    let zip_name = game_zip_name(NEW);
    let zip_bytes = read(&rel_new.join(&zip_name))?;
    write(&feed_dir.join(&zip_name), &zip_bytes)?;
    let offered = Release {
        version: NEW.to_owned(),
        url: format!("{}{zip_name}", feed.url()),
        file: zip_name,
        size: zip_bytes.len() as u64,
        sha256: crate::pack::sha256_hex(&zip_bytes),
    };
    write(&feed_dir.join("latest"), offered.to_github())?;
    let r = run(program(&exe, &["--update-now"]), RUN_LIMIT, None)?;
    for l in r.out.lines().filter(|l| l.starts_with("update-now:")) {
        println!("    {l}");
    }
    step(&format!("--update-now finds {NEW}, downloads it and hands over"), || {
        if r.code == Some(HANDED_OVER) && r.out.contains(&format!("{NEW} is out")) {
            Ok(String::new())
        } else {
            Err(format!("exit {:?}\n    {}", r.code, tail(&r.out)))
        }
    })?;
    // the unpacked copy waits for the old exe, copies the release over the folder and starts the launcher: the copy is
    // done when the folder's game and launcher are the new build's
    let new_dir = rel_new.join(format!("MVS-Classic-{NEW}"));
    let (new_exe, new_launcher) = (read(&new_dir.join(GAME_DIR).join(GAME_EXE))?, read(&new_dir.join(LAUNCHER_EXE))?);
    let launcher = app.join(LAUNCHER_EXE);
    let stage = data.join(UPDATE_DIR);
    step(&format!("the folder's {GAME_EXE} and {LAUNCHER_EXE} become {NEW}'s"), || {
        let copied = wait_for(COPY_LIMIT, || {
            std::fs::read(&exe).is_ok_and(|b| b == new_exe)
                && std::fs::read(&launcher).is_ok_and(|b| b == new_launcher)
                && pids_under(&stage).is_empty()
        });
        if copied { Ok(String::new()) } else { Err(format!("not within {} s", COPY_LIMIT.as_secs())) }
    })?;
    step(&format!("the folder's {LAUNCHER_EXE} starts after the copy"), || {
        if wait_for(Duration::from_secs(60), || !pids_under(&app).is_empty()) {
            Ok(String::new())
        } else {
            Err("no process of the folder's exes".to_owned())
        }
    })?;
    // Deviation: the restarted launcher (a window) is ended by its PID once it has cleared data\update\: the exe has
    // no switch to end at once, and the clear only runs in the launcher (mvs-app update Updater::new).
    let cleared = wait_for(CLEAR_LIMIT, || !stage.exists());
    end_under(&app);
    step("the new start clears data\\update\\", || {
        if cleared {
            Ok(String::new())
        } else {
            Err(format!("{} still there after {} s", stage.display(), CLEAR_LIMIT.as_secs()))
        }
    })?;
    step("data\\ is untouched by the update", || {
        for (p, d) in &planted {
            if read(p)? != *d {
                return Err(format!("{} changed", p.display()));
            }
        }
        Ok(String::new())
    })?;
    step(&format!("{GAME_EXE} --version now says {NEW}"), || {
        let v = version_of(&exe)?;
        if v == format!("mvs {NEW}") { Ok(v) } else { Err(format!("it says \"{v}\"")) }
    })?;

    // 6. nothing newer than the newest
    let r = run(program(&exe, &["--update-now"]), RUN_LIMIT, None)?;
    step(&format!("with {NEW} installed, --update-now finds no newer release (exit {NO_UPDATE})"), || {
        if r.code == Some(NO_UPDATE) {
            Ok(String::new())
        } else {
            Err(format!("exit {:?}\n    {}", r.code, tail(&r.out)))
        }
    })?;
    end_under(&app);

    // 7. the release with the fake EOS pair
    let app_eos = s.join("app-eos");
    zip::unpack(&rel_eos.join(game_zip_name(NEW)), &app_eos)?;
    let r = run(
        program(
            &app_eos.join(GAME_DIR).join(GAME_EXE),
            &["--self-test", "--data", &data_ok.display().to_string(), "--store", &store],
        ),
        RUN_LIMIT,
        None,
    )?;
    step("--self-test of the EOS build names both files beside the exe", || {
        let want = format!(
            "self-test eos: SDK {}, credentials {}",
            app_eos.join(GAME_DIR).join(EOS_DLL).display(),
            app_eos.join(GAME_DIR).join(EOS_CREDENTIALS).display()
        );
        let licence = String::from_utf8_lossy(&read(&app_eos.join(LICENSE))?).into_owned();
        if !licence.contains("Epic Online Services") {
            Err("no Epic notice in LICENSE.txt".to_owned())
        } else if r.out.lines().any(|l| l.replace('/', "\\") == want.replace('/', "\\")) {
            Ok(String::new())
        } else {
            Err(format!("no line \"{want}\"\n    {}", tail(&r.out)))
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_release::in_game;

    #[test]
    fn every_plant_is_refused_and_a_clean_release_is_not() {
        let eos = fake_eos_json();
        let release: Vec<Entry> = [
            (in_game(GAME_EXE), b"MZ the program".as_slice()),
            (LAUNCHER_EXE.to_owned(), b"MZ the launcher"),
            (LICENSE.to_owned(), b"MIT License"),
            (in_game("licenses/NOTICE-importer.txt"), b"n"),
            (in_game("licenses/LICENSE-CUE4Parse.txt"), b"l"),
            (in_game(EOS_DLL), b"MZ"),
            (in_game(EOS_CREDENTIALS), eos.as_bytes()),
        ]
        .iter()
        .map(|(n, d)| Entry { name: n.clone(), data: d.to_vec(), executable: false })
        .collect();
        assert!(guard::check_files("t", &release, Zip::Game, Some(&FAKE_KEY), &[]).findings.is_empty());
        for (what, plant, expect) in plants() {
            guard_refuses(&release, &plant, expect).unwrap_or_else(|e| panic!("{what}: {e}"));
        }
        // a plant the guard lets through is told apart
        let harmless = Entry { name: LICENSE.to_owned(), data: b"fine".to_vec(), executable: false };
        assert!(guard_refuses(&release, &harmless, "as hex").is_err());
        assert_eq!(mixed_hex(&[0xab, 0x0c]), "aB0C");
    }
}
