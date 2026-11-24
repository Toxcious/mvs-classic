//! mvs-dist: the developer's release tool.
//!
//!   mvs-dist build --version <x.y.z> [--feed <url>] [--eos] [--out <dir>] [--relay-linux <file>]
//!       [--game-linux <dir>] [--no-build] [--store <dir>]
//!       builds mvs.exe and mvs-relay.exe for the version (and the feed the exe checks), writes into <dir> (default
//!       <repo>/target/dist) the release folder, MVS-Classic-<version>.zip and MVS-Classic-Relay-<version>.zip, then
//!       guards both zips. --eos packs the EOS SDK's DLL and credentials named by MVS_EOS_DLL and MVS_EOS_CREDENTIALS.
//!       --relay-linux packs a Linux mvs-relay built elsewhere. --game-linux packs the Linux game built elsewhere
//!       (tools/linux-build.sh release's folder, its stamp naming the version) into MVS-Classic-<version>-linux.zip,
//!       with the self check of the Windows run; with --eos, its EOS library is MVS_EOS_SO's. --no-build packs the
//!       last build again (built for the same version and feed). The built mvs.exe runs the self check's match on
//!       --store (default: the store it finds itself) for the release's self-check.json.
//!       On a Linux host the Windows programs are cross-built (tools/windows-cross.sh, target/xwin) and run under Wine
//!       in a prefix made for the run beside <dir> (<dir>.wine, removed at the end), and the Linux game and relay are
//!       built in the container (tools/linux-build.sh) unless --game-linux and --relay-linux name them; --no-build
//!       packs their last builds too.
//!   mvs-dist guard <game zip> [<relay zip>] [--key <local.json>]
//!       the guard alone (the key from MVS_PAK_KEY, else <repo>/content/local.json unless --key names one: its
//!       "key", else the exe beside its "paks")
//!   mvs-dist serve <dir> [--port <n>]
//!       serves <dir> on 127.0.0.1 (port 0 or none: any free port; the URL is printed) until stopped
//!   mvs-dist release-check [--store <dir>] [--keep]
//!       builds 0.1.0 and 0.1.1 test releases against a local feed and walks them through the guard, the
//!       self-test and an update (mvs_dist::check), in <repo>/.scratch/release-check (deleted at the end unless
//!       --keep); --store names the developer's store the self-test reads (default content/.store). Windows only: it
//!       tests the Windows updater.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mvs_dist::check;
use mvs_dist::guard::{self, Zip};
use mvs_dist::pack::{self, Options};
use mvs_dist::serve;

const USAGE: &str =
    "usage: mvs-dist build --version <x.y.z> [--feed <url>] [--eos] [--out <dir>] [--relay-linux <file>]
           [--game-linux <dir>] [--no-build] [--store <dir>]
       mvs-dist guard <game zip> [<relay zip>] [--key <local.json>]
       mvs-dist serve <dir> [--port <n>]
       mvs-dist release-check [--store <dir>] [--keep]

On Linux, build cross-builds the Windows programs (tools/windows-cross.sh), runs them under Wine in a prefix of
its own, and builds the Linux game and relay unless --game-linux and --relay-linux name them. release-check needs
Windows.";

/// The arguments: flags with their values, and the rest in order.
struct Args {
    rest: Vec<String>,
    pairs: Vec<(String, String)>,
    flags: Vec<String>,
}

fn parse(args: &[String], with_value: &[&str], bare: &[&str]) -> Result<Args, String> {
    let mut a = Args { rest: Vec::new(), pairs: Vec::new(), flags: Vec::new() };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        if with_value.contains(&x.as_str()) {
            let v = it.next().ok_or_else(|| format!("{x} needs a value"))?;
            a.pairs.push((x.clone(), v.clone()));
        } else if bare.contains(&x.as_str()) {
            a.flags.push(x.clone());
        } else if x.starts_with("--") {
            return Err(format!("unknown option {x}"));
        } else {
            a.rest.push(x.clone());
        }
    }
    Ok(a)
}

impl Args {
    fn get(&self, k: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn has(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }
}

fn repo() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    pack::find_repo(&cwd)
        .or_else(|| std::env::current_exe().ok().and_then(|e| pack::find_repo(&e)))
        .ok_or_else(|| "dist: run inside the repo (no workspace with crates/mvs-app above here)".to_owned())
}

fn absolute(p: &str) -> Result<PathBuf, String> {
    std::path::absolute(p).map_err(|e| format!("{p}: {e}"))
}

fn build(args: &[String]) -> Result<(), String> {
    let a = parse(
        args,
        &["--version", "--feed", "--out", "--relay-linux", "--game-linux", "--store"],
        &["--eos", "--no-build"],
    )?;
    if !a.rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let repo = repo()?;
    let version = a.get("--version").ok_or("build needs --version")?.to_owned();
    pack::run(&Options {
        version,
        feed: a.get("--feed").map(str::to_owned),
        eos: a.has("--eos"),
        out: a.get("--out").map_or_else(|| Ok(repo.join("target/dist")), absolute)?,
        relay_linux: a.get("--relay-linux").map(absolute).transpose()?,
        game_linux: a.get("--game-linux").map(absolute).transpose()?,
        no_build: a.has("--no-build"),
        store: a.get("--store").map(absolute).transpose()?,
        repo,
    })
}

fn guard_only(args: &[String]) -> Result<(), String> {
    let a = parse(args, &["--key"], &[])?;
    if a.rest.is_empty() || a.rest.len() > 2 {
        return Err(USAGE.to_owned());
    }
    let repo = repo().ok();
    let key_file = match a.get("--key") {
        Some(k) => Some(PathBuf::from(k)),
        None => repo.as_ref().map(|r| r.join("content/local.json")).filter(|f| f.is_file()),
    };
    let key = guard::key_here(key_file.as_deref())?;
    match &key {
        Some(k) => println!("guard: the key from {}", k.source),
        None => println!("guard: {}", guard::NO_KEY),
    }
    let key = key.map(|k| k.key);
    let mut findings = Vec::new();
    // the build list the exe embeds, when run inside the repo
    match &repo {
        Some(r) => {
            let list = r.join(pack::BUILD_LIST);
            let text = std::fs::read_to_string(&list).map_err(|e| format!("guard: {}: {e}", list.display()))?;
            findings.extend(guard::check_build_json(pack::BUILD_LIST, &text));
        }
        None => println!("guard: not in the repo: the build list check is skipped"),
    }
    // the game's zip first: the relay's is searched for the credentials of its resources/eos.json too
    let mut secrets = Vec::new();
    for (i, z) in a.rest.iter().enumerate() {
        let kind = if i == 0 { Zip::Game } else { Zip::Relay };
        let r = guard::check_zip(Path::new(z), kind, key.as_ref(), &secrets)?;
        println!("guard: {z}: {} files, {} bytes read", r.files, r.bytes);
        findings.extend(r.findings);
        secrets = r.secrets;
    }
    if !findings.is_empty() {
        for f in &findings {
            eprintln!("guard: {f}");
        }
        return Err(format!("guard: FAIL, {} finding{}", findings.len(), if findings.len() == 1 { "" } else { "s" }));
    }
    println!("guard: PASS");
    Ok(())
}

fn serve_dir(args: &[String]) -> Result<(), String> {
    let a = parse(args, &["--port"], &[])?;
    let [dir] = a.rest.as_slice() else { return Err(USAGE.to_owned()) };
    let port = a.get("--port").map_or(Ok(0), |p| p.parse::<u16>().map_err(|_| format!("--port {p} is not a port")))?;
    let feed = serve::start(Path::new(dir), port).map_err(|e| format!("serve: {dir}: {e}"))?;
    println!("serve: {} serving {dir}", feed.url());
    feed.thread.join().map_err(|_| "serve: the listener stopped".to_owned())
}

fn release_check(args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("release-check: it tests the Windows updater and needs Windows".to_owned());
    }
    let a = parse(args, &["--store"], &["--keep"])?;
    if !a.rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let repo = repo()?;
    let store = a.get("--store").map_or_else(|| Ok(repo.join("content/.store")), absolute)?;
    let r = check::run_check(&check::CheckOptions { repo, store, keep: a.has("--keep") });
    println!("{}", if r.is_ok() { "release-check: all passed" } else { "release-check: FAILED" });
    r
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("build") => build(&args[1..]),
        Some("guard") => guard_only(&args[1..]),
        Some("serve") => serve_dir(&args[1..]),
        Some("release-check") => release_check(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
