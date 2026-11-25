//! The release build: builds `mvs`, `mvs-launcher` and `mvs-relay` for a version and feed, lays
//! out the release folder (the launcher and the game side by side), zips it, zips the relay, and runs the guard over both zips. What
//! the guard refuses is deleted (the folders stay for a look).

use std::path::{Path, PathBuf};
use std::process::Command;

use mvs_release::{
    BUILT_GAME_EXE, BUILT_LAUNCHER_EXE, EOS_CREDENTIALS, EOS_DLL, GAME_EXE, LAUNCHER_EXE, LICENSE, LICENSES_DIR,
    ONLINE_JSON, RELEASE_FILES, SELF_CHECK_JSON, SelfCheckRef, in_game, parse_version,
};
use sha2::{Digest, Sha256};

use crate::guard::{self, LICENSE_CUE4PARSE, NOTICE_IMPORTER, Zip};
use mvs_release::zip::{self, Entry};

/// Epic's notice, after the project's licence in a release with the EOS SDK.
const EOS_NOTICE: &str = include_str!("../notices/eos-notice.txt");
/// Epic's SDK runtime as the repo keeps it (`third_party/eos/README.md`).
const EOS_DLL_IN_REPO: &str = "third_party/eos/EOSSDK-Win64-Shipping.dll";
/// The file that marks a folder as this tool's output, which its next run may empty.
const OUT_MARK: &str = ".mvs-dist";

pub use mvs_release::game_zip_name;

/// The relay's zip of a version.
pub fn relay_zip_name(version: &str) -> String {
    format!("MVS-Classic-Relay-{version}.zip")
}

pub struct Options {
    /// `x.y.z`
    pub version: String,
    /// The update feed the exe checks (mvs_release::FEED: the GitHub API's latest release); None: a build that never
    /// checks.
    pub feed: Option<String>,
    /// Pack the EOS SDK's DLL (the repo's, or `MVS_EOS_DLL`) and the credentials (`MVS_EOS_CREDENTIALS`).
    pub eos: bool,
    /// The output folder (made, or emptied when it is an earlier output).
    pub out: PathBuf,
    /// The workspace root.
    pub repo: PathBuf,
    /// A Linux `mvs-relay` built elsewhere, packed beside the Windows one.
    pub relay_linux: Option<PathBuf>,
    /// Skip the cargo builds and pack the last ones (a re-pack of the same version).
    pub no_build: bool,
    /// Skip the cargo builds and pack the programs in this folder (`mvs.exe`, `mvs-launcher.exe`, `mvs-relay.exe`):
    /// the release workflow's, fetched and verified by `tools/release-fetch.ps1`.
    pub programs: Option<PathBuf>,
    /// The store the self check's match runs on (None: the one the built exe finds, its data root's).
    pub store: Option<PathBuf>,
    /// The self check's reference made beforehand (the repo's self-check.json, made by `mvs --self-test --check-out`
    /// on a store of the release's import): packed as it is, no store read. It must be for the id the packed exe's
    /// `--compat-id` prints, committed, with no change to the code after its commit. The release workflow packs with
    /// it, where no store is.
    pub self_check: Option<PathBuf>,
}

/// The EOS files a release adds, checked before anything is built: a bad credentials file is named by its path and
/// the missing entry's name.
struct EosFiles {
    dll: PathBuf,
    credentials: Vec<u8>,
}

fn eos_files(repo: &Path) -> Result<EosFiles, String> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let Some(credentials) = var("MVS_EOS_CREDENTIALS") else {
        return Err("dist: --eos needs MVS_EOS_CREDENTIALS".to_owned());
    };
    let dll = var("MVS_EOS_DLL").unwrap_or_else(|| repo.join(EOS_DLL_IN_REPO));
    let named = dll.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(EOS_DLL));
    if !named || !dll.is_file() {
        return Err(format!("dist: MVS_EOS_DLL is not an {EOS_DLL}: {}", dll.display()));
    }
    let text = std::fs::read(&credentials)
        .map_err(|_| format!("dist: MVS_EOS_CREDENTIALS does not read: {}", credentials.display()))?;
    let json: serde_json::Value = serde_json::from_slice(&text)
        .map_err(|_| format!("dist: MVS_EOS_CREDENTIALS is not a JSON file: {}", credentials.display()))?;
    // the keys mvs-net's EOS start reads
    for k in ["productId", "sandboxId", "deploymentId", "clientId", "clientSecret"] {
        if json.get(k).and_then(|v| v.as_str()).is_none_or(str::is_empty) {
            return Err(format!("dist: {} has no {k}", credentials.display()));
        }
    }
    Ok(EosFiles { dll, credentials: text })
}

/// The workspace root above `from`: the folder whose Cargo.toml is a workspace holding `crates/mvs-app`.
pub fn find_repo(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|d| {
            d.join("crates/mvs-app/Cargo.toml").is_file()
                && std::fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|t| t.contains("[workspace]"))
        })
        .map(Path::to_path_buf)
}

/// `cargo build --release -p <package> --bin <bin>..` with the version and feed the build takes, through the repo's
/// build slots (tools/cargo-slot.ps1) when it has them. One package a build: a build naming other packages' binaries
/// may leave an old `mvs.exe`.
///
/// The C runtime is linked statically (`+crt-static`): Windows 10 and 11 hold the Universal CRT, but not
/// VCRUNTIME140.dll, which only the Visual C++ Redistributable installs. The guard refuses an exe that still needs
/// it. A `RUSTFLAGS` set in the environment overrides this, and the guard then catches it.
fn cargo_build(o: &Options, package: &str, bins: &[&str]) -> Result<(), String> {
    let slot = o.repo.join("tools/cargo-slot.ps1");
    let mut args = vec!["build", "--release", "-p", package];
    for b in bins {
        args.extend(["--bin", b]);
    }
    let bin = bins.join(", ");
    let mut cmd = if slot.is_file() {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(&slot).args(args);
        c
    } else {
        let mut c = Command::new("cargo");
        c.args(args);
        c
    };
    cmd.current_dir(&o.repo)
        .env("MVS_VERSION", &o.version)
        .env("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS", "-C target-feature=+crt-static");
    match &o.feed {
        Some(f) => cmd.env("MVS_UPDATE_FEED", f),
        None => cmd.env_remove("MVS_UPDATE_FEED"),
    };
    println!(
        "dist: building {bin} {}{}",
        o.version,
        o.feed.as_deref().map(|f| format!(" (feed {f})")).unwrap_or_default()
    );
    let status = cmd.status().map_err(|e| format!("dist: cargo for {bin} did not start: {e}"))?;
    if !status.success() {
        return Err(format!("dist: the build of {bin} failed ({status})"));
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("dist: {}: {e}", path.display()))
}

/// Writes `files` under `dir` (made fresh) and returns them as zip entries.
fn lay_out(dir: &Path, files: Vec<(String, Vec<u8>, bool)>) -> Result<Vec<Entry>, String> {
    let io = |e: std::io::Error| format!("dist: {}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut entries = Vec::new();
    for (name, data, executable) in files {
        let path = dir.join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&path, &data).map_err(io)?;
        entries.push(Entry { name, data, executable });
    }
    Ok(entries)
}

/// A fresh output folder: only a folder this tool made (it holds OUT_MARK), or an empty one, is emptied.
fn fresh_out(out: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| format!("dist: {}: {e}", out.display());
    if out.exists() {
        let empty = std::fs::read_dir(out).map_err(io)?.next().is_none();
        if !empty && !out.join(OUT_MARK).is_file() {
            return Err(format!(
                "dist: {} is not an earlier output of mvs-dist; name an empty or new folder",
                out.display()
            ));
        }
        std::fs::remove_dir_all(out).map_err(io)?;
    }
    std::fs::create_dir_all(out).map_err(io)?;
    std::fs::write(out.join(OUT_MARK), "The output of mvs-dist: emptied by its next run.\n").map_err(io)
}

/// The build list the exe embeds, under the repo.
pub const BUILD_LIST: &str = "crates/mvs-pak/build.json";
/// Beside the release build: the version and feed mvs-dist built it for, and the programs' hashes, so `--no-build`
/// packs only programs built for the same version and feed.
const STAMP: &str = "mvs-dist-build.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct Stamp {
    version: String,
    feed: Option<String>,
    exe: String,
    #[serde(default)]
    launcher: String,
    relay: String,
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs `<exe> --version` and refuses a first line other than `want` (the relay may add its wire version after a
/// space).
fn check_version(exe: &Path, want: &str) -> Result<(), String> {
    let out = Command::new(exe)
        .arg("--version")
        .output()
        .map_err(|e| format!("dist: {} --version did not start: {e}", exe.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim_end();
    if !out.status.success() || !(line == want || line.starts_with(&format!("{want} "))) {
        return Err(format!("dist: {} --version says \"{line}\", not \"{want}\"", exe.display()));
    }
    Ok(())
}

/// The self check's reference (SELF_CHECK_JSON): the built exe's run of the check match on `store`
/// (`mvs --self-test --check-out`, mvs_app::selftest), written to `tmp` and read back.
fn self_check(exe: &Path, store: Option<&Path>, tmp: &Path) -> Result<Vec<u8>, String> {
    let mut c = Command::new(exe);
    c.arg("--self-test").arg("--check-out").arg(tmp);
    if let Some(s) = store {
        c.arg("--store").arg(s);
    }
    let out = c.output().map_err(|e| format!("dist: {} --self-test did not start: {e}", exe.display()))?;
    let text = read(tmp);
    let _ = std::fs::remove_file(tmp);
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = said.lines().filter(|l| l.contains("FAILED")).collect();
        return Err(format!(
            "dist: the self check's match did not run (give the store with --store): {}",
            lines.join("; ")
        ));
    }
    let text = text?;
    let r: SelfCheckRef =
        serde_json::from_slice(&text).map_err(|e| format!("dist: {} is not a self check: {e}", tmp.display()))?;
    println!("dist: self check {:08x} for {}", r.hash, r.compat);
    Ok(text)
}

/// A self check's reference made beforehand: `file` read, and held to the id `exe --compat-id` prints (the id a
/// player's fresh import gets; a reference for another one is never used).
fn made_self_check(exe: &Path, file: &Path, repo: &Path) -> Result<Vec<u8>, String> {
    let text = read(file)?;
    let r: SelfCheckRef =
        serde_json::from_slice(&text).map_err(|e| format!("dist: {} is not a self check: {e}", file.display()))?;
    let out = Command::new(exe)
        .arg("--compat-id")
        .output()
        .map_err(|e| format!("dist: {} --compat-id did not start: {e}", exe.display()))?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || id.is_empty() {
        return Err(format!("dist: {} --compat-id gave no id", exe.display()));
    }
    if r.compat != id {
        return Err(format!(
            "dist: {} is for {}, the release's id is {id}: make it again with `mvs --self-test --check-out`",
            file.display(),
            r.compat
        ));
    }
    code_unchanged_since(repo, file)?;
    println!("dist: self check {:08x} for {} (from {})", r.hash, r.compat, file.display());
    Ok(text)
}

/// The paths a self check's hash comes from: the code and what it is built with. The id holds the version, so a
/// reference of another release is caught by it; this catches one of this version made before a later change.
const CHECKED_CODE: [&str; 4] = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];

/// Err unless `file` is the repo's, committed as it is, and no commit after the last one that holds it (nor the
/// working tree) changed the code it was made from. Needs the history back to that commit (no shallow clone).
fn code_unchanged_since(repo: &Path, file: &Path) -> Result<(), String> {
    let full = |p: &Path| p.canonicalize().map_err(|e| format!("dist: {}: {e}", p.display()));
    let (repo_full, file_full) = (full(repo)?, full(file)?);
    let Ok(rel) = file_full.strip_prefix(&repo_full) else {
        return Err(format!("dist: {} is not in the repo: --self-check takes the committed one", file.display()));
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    let git = |args: &[&str]| {
        Command::new("git").arg("-C").arg(repo).args(args).output().map_err(|e| format!("dist: git did not start: {e}"))
    };
    let out = git(&["status", "--porcelain", "--", &rel])?;
    if !out.status.success() || !out.stdout.is_empty() {
        return Err(format!("dist: {rel} is not committed as it is: commit it with the release"));
    }
    let out = git(&["log", "-1", "--format=%H", "--", &rel])?;
    let made = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || made.is_empty() {
        return Err(format!("dist: no commit holds {rel} (a shallow clone? fetch the history back to it)"));
    }
    let since = [&["diff", "--quiet", &made, "HEAD", "--"][..], &CHECKED_CODE[..]].concat();
    let now = [&["diff", "--quiet", "HEAD", "--"][..], &CHECKED_CODE[..]].concat();
    for (args, what) in [(since, format!("since {}", &made[..made.len().min(10)])), (now, "in the working tree".into())]
    {
        match git(&args)?.status.code() {
            Some(0) => {}
            Some(1) => {
                return Err(format!(
                    "dist: the code changed {what}, after {rel} was made: make it again (tools/self-check.ps1)"
                ));
            }
            _ => return Err(format!("dist: git diff failed on {}", args.join(" "))),
        }
    }
    Ok(())
}

/// The whole release; Err names the step that failed.
pub fn run(o: &Options) -> Result<(), String> {
    if parse_version(&o.version).is_none() {
        return Err(format!("dist: version {} is not x.y.z", o.version));
    }
    if let Some(f) = &o.feed
        && !(f.starts_with("https://") || f.starts_with("http://"))
    {
        return Err(format!("dist: the feed {f} is not an http(s) URL"));
    }
    let eos = if o.eos { Some(eos_files(&o.repo)?) } else { None };
    if let Some(l) = &o.relay_linux
        && !l.is_file()
    {
        return Err(format!("dist: no Linux relay at {}", l.display()));
    }
    let key_file = o.repo.join("content/local.json");
    let key = if key_file.is_file() { Some(guard::read_key(&key_file)?) } else { None };
    // the build list the exe embeds (mvs-pak's include_str!): names, sizes and hashes only
    let list = BUILD_LIST;
    let found = guard::check_build_json(list, &String::from_utf8_lossy(&read(&o.repo.join(list))?));
    if !found.is_empty() {
        return Err(format!("dist: the guard refused the build list: {}", found.join("; ")));
    }
    if o.programs.is_some() && o.no_build {
        return Err("dist: --programs and --no-build both name what to pack; give one".to_owned());
    }
    if o.self_check.is_some() && o.store.is_some() {
        return Err("dist: --self-check and --store both name the self check's source; give one".to_owned());
    }
    if let Some(dir) = &o.programs
        && !dir.is_dir()
    {
        return Err(format!("dist: no programs folder at {}", dir.display()));
    }
    fresh_out(&o.out)?;

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| o.repo.join("target"), PathBuf::from);
    let built = o.programs.clone().unwrap_or_else(|| target.join("release"));
    let (exe, launcher_exe, relay_exe) =
        (built.join(BUILT_GAME_EXE), built.join(BUILT_LAUNCHER_EXE), built.join("mvs-relay.exe"));
    let stamp = Stamp {
        version: o.version.clone(),
        feed: o.feed.clone(),
        exe: String::new(),
        launcher: String::new(),
        relay: String::new(),
    };
    let stamp_file = built.join(STAMP);
    if o.programs.is_some() {
        // the workflow built them for the tag's version and the feed; check_version below holds them to the version,
        // this to the feed (the exe holds the feed's URL as text)
        if let Some(f) = &o.feed {
            let has = |p: &Path| read(p).map(|b| b.windows(f.len()).any(|w| w == f.as_bytes()));
            if !has(&exe)? || !has(&launcher_exe)? {
                return Err(format!("dist: --programs: mvs.exe or mvs-launcher.exe was not built with the feed {f}"));
            }
        }
        println!("dist: packing the programs in {}", built.display());
    } else if o.no_build {
        let had = std::fs::read(&stamp_file)
            .ok()
            .and_then(|t| serde_json::from_slice::<Stamp>(&t).ok())
            .ok_or_else(|| format!("dist: --no-build: {} names no build of mvs-dist's", stamp_file.display()))?;
        if (&had.version, &had.feed) != (&stamp.version, &stamp.feed) {
            return Err(format!(
                "dist: --no-build: the last build is {} with feed {}, not {} with feed {}",
                had.version,
                had.feed.as_deref().unwrap_or("none"),
                stamp.version,
                stamp.feed.as_deref().unwrap_or("none")
            ));
        }
        if had.exe != sha256_hex(&read(&exe)?)
            || had.launcher != sha256_hex(&read(&launcher_exe)?)
            || had.relay != sha256_hex(&read(&relay_exe)?)
        {
            return Err("dist: --no-build: mvs.exe, mvs-launcher.exe or mvs-relay.exe was built again since mvs-dist \
                        built them"
                .to_owned());
        }
    } else {
        let _ = std::fs::remove_file(&stamp_file);
        cargo_build(o, "mvs-app", &["mvs", "mvs-launcher"])?;
        cargo_build(o, "mvs-relay", &["mvs-relay"])?;
        let stamp = Stamp {
            exe: sha256_hex(&read(&exe)?),
            launcher: sha256_hex(&read(&launcher_exe)?),
            relay: sha256_hex(&read(&relay_exe)?),
            ..stamp
        };
        let text = serde_json::to_string_pretty(&stamp).expect("a Stamp always serialises");
        std::fs::write(&stamp_file, text).map_err(|e| format!("dist: {}: {e}", stamp_file.display()))?;
    }
    // what the programs say they are
    check_version(&exe, &format!("mvs {}", o.version))?;
    check_version(&launcher_exe, &format!("mvs-launcher {}", o.version))?;
    check_version(&relay_exe, &format!("mvs-relay {}", o.version))?;
    let check = match &o.self_check {
        Some(file) => made_self_check(&exe, file, &o.repo)?,
        None => self_check(&exe, o.store.as_deref(), &o.out.join(SELF_CHECK_JSON))?,
    };

    // the game's folder and zip
    let licence = read(&o.repo.join("LICENSE"))?;
    let mut game_licence = licence.clone();
    if eos.is_some() {
        game_licence.extend_from_slice(b"\n\n");
        game_licence.extend_from_slice(EOS_NOTICE.as_bytes());
    }
    let uasset = o.repo.join("crates/mvs-uasset");
    // the importer's notices: CUE4Parse's (mvs-uasset), then ww2ogg's (mvs-import)
    let mut notices = read(&uasset.join("NOTICE"))?;
    notices.extend_from_slice(b"\n");
    notices.extend_from_slice(&read(&o.repo.join("crates/mvs-import/NOTICE"))?);
    let mut files = vec![
        (in_game(GAME_EXE), read(&exe)?, false),
        (LAUNCHER_EXE.to_owned(), read(&launcher_exe)?, false),
        (LICENSE.to_owned(), game_licence, false),
        (in_game(&format!("{LICENSES_DIR}/{NOTICE_IMPORTER}")), notices, false),
        (in_game(&format!("{LICENSES_DIR}/{LICENSE_CUE4PARSE}")), read(&uasset.join("LICENSE-CUE4Parse"))?, false),
        (in_game(ONLINE_JSON), read(&o.repo.join(ONLINE_JSON))?, false),
        (in_game(SELF_CHECK_JSON), check, false),
    ];
    if let Some(e) = &eos {
        files.push((in_game(EOS_DLL), read(&e.dll)?, false));
        files.push((in_game(EOS_CREDENTIALS), e.credentials.clone(), false));
    }
    // the release's own files, the list itself with them: the files an update may replace or remove
    let list = in_game(RELEASE_FILES);
    let mut names: Vec<&str> = files.iter().map(|(n, _, _)| n.as_str()).chain([list.as_str()]).collect();
    names.sort_unstable();
    let text = names.iter().map(|n| format!("{n}\n")).collect::<String>().into_bytes();
    files.push((list, text, false));
    let game_name = game_zip_name(&o.version);
    let game = lay_out(&o.out.join(format!("MVS-Classic-{}", o.version)), files)?;
    let game_zip = zip::write(&game)?;
    let game_path = o.out.join(&game_name);
    std::fs::write(&game_path, &game_zip).map_err(|e| format!("dist: {}: {e}", game_path.display()))?;
    println!("dist: {game_name} ({} bytes, SHA-256 {})", game_zip.len(), sha256_hex(&game_zip));

    // the relay's
    let mut files = vec![
        ("mvs-relay.exe".to_owned(), read(&relay_exe)?, false),
        ("README.md".to_owned(), read(&o.repo.join("docs/relay.md"))?, false),
        (LICENSE.to_owned(), licence, false),
    ];
    match &o.relay_linux {
        Some(l) => files.push(("mvs-relay".to_owned(), read(l)?, true)),
        None => println!("dist: no --relay-linux: the relay zip holds the Windows program only"),
    }
    let relay_name = relay_zip_name(&o.version);
    let relay = lay_out(&o.out.join(format!("MVS-Classic-Relay-{}", o.version)), files)?;
    let relay_zip = zip::write(&relay)?;
    let relay_path = o.out.join(&relay_name);
    std::fs::write(&relay_path, &relay_zip).map_err(|e| format!("dist: {}: {e}", relay_path.display()))?;
    println!("dist: {relay_name} ({} bytes)", relay_zip.len());

    // the guard over what is handed out
    if key.is_none() {
        println!("dist: no content/local.json: the key search is skipped");
    }
    let mut secrets = eos.as_ref().and_then(|e| guard::eos_secrets(&e.credentials)).unwrap_or_default();
    let mut findings = Vec::new();
    for (path, kind) in [(&game_path, Zip::Game), (&relay_path, Zip::Relay)] {
        let r = guard::check_zip(path, kind, key.as_ref(), &secrets)?;
        println!("guard: {}: {} files, {} bytes read", path.display(), r.files, r.bytes);
        findings.extend(r.findings);
        secrets = r.secrets;
    }
    if !findings.is_empty() {
        for f in &findings {
            eprintln!("guard: {f}");
        }
        for p in [&game_path, &relay_path] {
            let _ = std::fs::remove_file(p);
        }
        return Err(format!(
            "dist: the guard refused the build ({} finding{}); its zips are deleted, the folders stay",
            findings.len(),
            if findings.len() == 1 { "" } else { "s" }
        ));
    }
    println!("guard: PASS");
    println!("dist: done, in {}", o.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::code_unchanged_since;
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git").arg("-C").arg(dir).args(args).status().expect("git runs").success();
        assert!(ok, "git {args:?}");
    }

    fn commit(dir: &Path, what: &str) {
        git(dir, &["add", "-A"]);
        git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", what]);
    }

    #[test]
    fn a_reference_is_refused_once_the_code_changes_after_it() {
        let dir = std::env::temp_dir().join(format!("mvs-dist-selfcheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crates")).unwrap();
        git(&dir, &["init", "-q"]);
        let (code, reference) = (dir.join("crates/a.rs"), dir.join("self-check.json"));
        std::fs::write(&code, "1").unwrap();
        std::fs::write(&reference, "{}").unwrap();
        commit(&dir, "code and its reference");
        assert!(code_unchanged_since(&dir, &reference).is_ok());

        std::fs::write(&code, "2").unwrap();
        assert!(code_unchanged_since(&dir, &reference).is_err(), "a change in the working tree");
        commit(&dir, "a later change");
        assert!(code_unchanged_since(&dir, &reference).is_err(), "a change committed after the reference");

        std::fs::write(&reference, "{ }").unwrap();
        assert!(code_unchanged_since(&dir, &reference).is_err(), "a reference not committed as it is");
        commit(&dir, "the reference made again");
        assert!(code_unchanged_since(&dir, &reference).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
