//! The release build: builds `mvs`, `mvs-launcher` and `mvs-relay` for a version and feed, lays
//! out the release folder (the launcher and the game side by side), zips it, zips the relay, and runs the guard over both zips. What
//! the guard refuses is deleted (the folders stay for a look). With `--game-linux`, a Linux game built elsewhere
//! (`tools/linux-build.sh release`) is laid out and zipped beside the Windows one, and guarded with it.
//!
//! On a Linux host the Windows programs are cross-built (`tools/windows-cross.sh`) and run under Wine in a prefix
//! made for the run, and the Linux game and relay are built here too (`tools/linux-build.sh`) unless given.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mvs_release::{
    BUILT_GAME_EXE, BUILT_LAUNCHER_EXE, EOS_CREDENTIALS, EOS_DLL, EOS_SO, GAME_EXE, LAUNCHER_EXE, LICENSE,
    LICENSES_DIR, LINUX_GAME_EXE, LINUX_LAUNCHER_EXE, ONLINE_JSON, RELEASE_FILES, SELF_CHECK_JSON, SelfCheckRef,
    game_zip_name_linux, in_game, parse_version,
};
use sha2::{Digest, Sha256};

use crate::guard::{self, LICENSE_CUE4PARSE, LICENSE_DXVK, LICENSE_SDL3, LINUX_LIBS, NOTICE_IMPORTER, Zip};
use mvs_release::zip::{self, Entry};

/// Epic's notice, after the project's licence in a release with the EOS SDK.
const EOS_NOTICE: &str = include_str!("../notices/eos-notice.txt");
/// Epic's SDK runtime as the repo keeps it (`third_party/eos/README.md`).
const EOS_DLL_IN_REPO: &str = "third_party/eos/EOSSDK-Win64-Shipping.dll";
/// Epic's Linux SDK runtime as the repo keeps it, for the Linux game's zip.
const EOS_SO_IN_REPO: &str = "third_party/eos/libEOSSDK-Linux-Shipping.so";
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
    /// Pack the EOS SDK's DLL (the repo's, or `MVS_EOS_DLL`) and the credentials (`MVS_EOS_CREDENTIALS`), and with
    /// `game_linux` its Linux library (the repo's, or `MVS_EOS_SO`).
    pub eos: bool,
    /// The output folder (made, or emptied when it is an earlier output).
    pub out: PathBuf,
    /// The workspace root.
    pub repo: PathBuf,
    /// A Linux `mvs-relay` built elsewhere, packed beside the Windows one.
    pub relay_linux: Option<PathBuf>,
    /// A Linux game built elsewhere (`tools/linux-build.sh release`'s folder), zipped beside the Windows one.
    pub game_linux: Option<PathBuf>,
    /// Skip the cargo builds and pack the last ones (a re-pack of the same version).
    pub no_build: bool,
    /// Skip the cargo builds and pack the programs in this folder (`mvs.exe`, `mvs-launcher.exe`, `mvs-relay.exe`):
    /// the release workflow's (`release.yml`), downloaded from the build workflow's run.
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
    /// The Linux library, for a release with the Linux game (None without it).
    so: Option<PathBuf>,
    credentials: Vec<u8>,
}

/// `linux`: a release with the Linux game, which takes the Linux library too.
fn eos_files(repo: &Path, linux: bool) -> Result<EosFiles, String> {
    eos_files_from(|k| std::env::var_os(k).map(PathBuf::from), repo, linux)
}

/// [`eos_files`] with the variables `var` gives.
fn eos_files_from(var: impl Fn(&str) -> Option<PathBuf>, repo: &Path, linux: bool) -> Result<EosFiles, String> {
    let Some(credentials) = var("MVS_EOS_CREDENTIALS") else {
        return Err("dist: --eos needs MVS_EOS_CREDENTIALS".to_owned());
    };
    let dll = var("MVS_EOS_DLL").unwrap_or_else(|| repo.join(EOS_DLL_IN_REPO));
    let named = dll.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(EOS_DLL));
    if !named || !dll.is_file() {
        return Err(format!("dist: MVS_EOS_DLL is not an {EOS_DLL}: {}", dll.display()));
    }
    let so = if linux {
        let so = var("MVS_EOS_SO").unwrap_or_else(|| repo.join(EOS_SO_IN_REPO));
        let named = so.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(EOS_SO));
        if !named || !so.is_file() {
            return Err(format!("dist: MVS_EOS_SO is not a {EOS_SO}: {}", so.display()));
        }
        Some(so)
    } else {
        None
    };
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
    Ok(EosFiles { dll, so, credentials: text })
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
///
/// On a Linux host the build goes through tools/windows-cross.sh, which adds the xwin splat's library paths after
/// `+crt-static` in the same variable.
fn cargo_build(o: &Options, host: Host, package: &str, bins: &[&str]) -> Result<(), String> {
    let slot = o.repo.join(CARGO_SLOT).is_file();
    let mut cmd = build_command(o, host, slot, package, bins);
    let bin = bins.join(", ");
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

/// The host a release is packed on: Windows builds and runs its programs itself; Linux (any host but Windows)
/// cross-builds them, runs them under Wine, and builds the Linux game and relay in the container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Host {
    Windows,
    Linux,
}

impl Host {
    fn here() -> Host {
        if cfg!(windows) { Host::Windows } else { Host::Linux }
    }
}

/// The repo's build slots (Windows only).
const CARGO_SLOT: &str = "tools/cargo-slot.ps1";
/// The cross-build of the Windows programs on Linux, and its target dir and target.
const CROSS_SCRIPT: &str = "tools/windows-cross.sh";
const CROSS_TARGET_DIR: &str = "target/xwin";
const CROSS_TARGET: &str = "x86_64-pc-windows-msvc";
/// The Linux builds in the container, and what they leave: the game's folder and the relay.
const LINUX_SCRIPT: &str = "tools/linux-build.sh";
const LINUX_RELEASE: &str = "target/linux-release";
const LINUX_RELAY: &str = "target/linux-sniper/release/mvs-relay";

/// [`cargo_build`]'s command: on Windows through the build slots when `slot` (the repo has them) or cargo, on Linux
/// through [`CROSS_SCRIPT`].
fn build_command(o: &Options, host: Host, slot: bool, package: &str, bins: &[&str]) -> Command {
    let mut args = vec!["build", "--release", "-p", package];
    for b in bins {
        args.extend(["--bin", b]);
    }
    let mut cmd = match host {
        Host::Windows if slot => {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(o.repo.join(CARGO_SLOT)).args(args);
            c
        }
        Host::Windows => {
            let mut c = Command::new("cargo");
            c.args(args);
            c
        }
        Host::Linux => {
            let mut c = Command::new(o.repo.join(CROSS_SCRIPT));
            c.args(args);
            c
        }
    };
    cmd.current_dir(&o.repo)
        .env("MVS_VERSION", &o.version)
        .env("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS", "-C target-feature=+crt-static");
    match &o.feed {
        Some(f) => cmd.env("MVS_UPDATE_FEED", f),
        None => cmd.env_remove("MVS_UPDATE_FEED"),
    };
    cmd
}

/// Where the Windows programs are built: `<target dir>/release` on Windows, `<target
/// dir>/x86_64-pc-windows-msvc/release` from the cross-build; the target dir is `target_dir` (CARGO_TARGET_DIR) when
/// set, else the repo's `target`, or `target/xwin` for the cross-build.
fn built_dir(host: Host, repo: &Path, target_dir: Option<PathBuf>) -> PathBuf {
    match host {
        Host::Windows => target_dir.unwrap_or_else(|| repo.join("target")).join("release"),
        Host::Linux => target_dir.unwrap_or_else(|| repo.join(CROSS_TARGET_DIR)).join(CROSS_TARGET).join("release"),
    }
}

/// `tools/linux-build.sh <args>` for the version: the build never checks a feed.
fn linux_build_command(o: &Options, args: &[&str]) -> Command {
    let mut c = Command::new(o.repo.join(LINUX_SCRIPT));
    c.args(args).current_dir(&o.repo).env("MVS_VERSION", &o.version).env_remove("MVS_UPDATE_FEED");
    c
}

/// The container's build of the Linux game's folder ([`LINUX_RELEASE`]).
const LINUX_GAME_BUILD: &[&str] = &["release"];
/// The container's build of the Linux relay ([`LINUX_RELAY`]).
const LINUX_RELAY_BUILD: &[&str] = &["cargo", "build", "--release", "-p", "mvs-relay", "--bin", "mvs-relay"];

fn linux_build(o: &Options, args: &[&str], what: &str) -> Result<(), String> {
    println!("dist: building {what} {} in the container", o.version);
    let status = linux_build_command(o, args)
        .status()
        .map_err(|e| format!("dist: {LINUX_SCRIPT} for {what} did not start: {e}"))?;
    if !status.success() {
        return Err(format!("dist: the build of {what} failed ({status})"));
    }
    Ok(())
}

/// On a Linux host, the line that says where each part of the release comes from.
fn sources_line(o: &Options) -> String {
    let linux = |given: Option<&PathBuf>, here: &str| match given {
        Some(p) => format!("given ({})", p.display()),
        None if o.no_build => format!("from the last build ({here})"),
        None => format!("built here ({here})"),
    };
    format!(
        "dist: the Windows programs {}; the Linux game {}; the Linux relay {}",
        if let Some(p) = &o.programs {
            format!("given ({})", p.display())
        } else if o.no_build {
            format!("from the last build ({CROSS_SCRIPT})")
        } else {
            format!("built here ({CROSS_SCRIPT})")
        },
        linux(o.game_linux.as_ref(), LINUX_RELEASE),
        linux(o.relay_linux.as_ref(), LINUX_RELAY)
    )
}

/// A word or a path on a built Windows program's command line.
enum Arg<'a> {
    Word(&'a str),
    Path(&'a Path),
}

/// How the built Windows programs are started: directly on Windows, elsewhere under Wine in the run's own prefix.
enum Runner {
    Direct,
    Wine(WinePrefix),
}

impl Runner {
    /// The runner for `host`; on Linux it makes the prefix beside `out`.
    fn for_host(host: Host, out: &Path) -> Result<Runner, String> {
        match host {
            Host::Windows => Ok(Runner::Direct),
            Host::Linux => Ok(Runner::Wine(WinePrefix::make(out)?)),
        }
    }

    fn command(&self, exe: &Path, args: &[Arg]) -> Result<Command, String> {
        match self {
            Runner::Direct => {
                let mut c = Command::new(exe);
                for a in args {
                    match a {
                        Arg::Word(w) => c.arg(w),
                        Arg::Path(p) => c.arg(p),
                    };
                }
                Ok(c)
            }
            Runner::Wine(p) => wine_command(&p.dir, exe, args, |path| winepath(&p.dir, path)),
        }
    }
}

/// The file that marks a Wine prefix as one this tool made, which its next run may remove.
const WINE_MARK: &str = ".mvs-dist-wine";

/// The Wine prefix of a run with output folder `out`: `<out>.wine` beside it (not in it: `out` is emptied and zipped
/// from). Never the user's own prefix.
fn wine_prefix(out: &Path) -> Result<PathBuf, String> {
    let (Some(parent), Some(name)) = (out.parent(), out.file_name()) else {
        return Err(format!("dist: {} has no folder beside it for a Wine prefix", out.display()));
    };
    let mut name = name.to_owned();
    name.push(".wine");
    let dir = parent.join(name);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if home.is_some_and(|h| dir == h.join(".wine")) {
        return Err(format!("dist: {} is the default Wine prefix, which this tool never uses", dir.display()));
    }
    Ok(dir)
}

/// What every Wine program of a run is given: its own prefix, no debug output, and no Mono, Gecko or menu entries.
fn wine_env(c: &mut Command, prefix: &Path) {
    c.env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .env("WINEDLLOVERRIDES", "mscoree,mshtml=;winemenubuilder.exe=d");
}

/// `wine <exe> <args>` in `prefix`, each path argument given as `to_windows` writes it.
fn wine_command(
    prefix: &Path,
    exe: &Path,
    args: &[Arg],
    to_windows: impl Fn(&Path) -> Result<String, String>,
) -> Result<Command, String> {
    let mut c = Command::new("wine");
    wine_env(&mut c, prefix);
    c.arg(exe);
    for a in args {
        match a {
            Arg::Word(w) => c.arg(w),
            Arg::Path(p) => c.arg(to_windows(p)?),
        };
    }
    Ok(c)
}

/// `winepath -w <path>` in `prefix`.
fn winepath_command(prefix: &Path, path: &Path) -> Command {
    let mut c = Command::new("winepath");
    wine_env(&mut c, prefix);
    c.arg("-w").arg(path);
    c
}

/// `path` as a Windows program in `prefix` names it.
fn winepath(prefix: &Path, path: &Path) -> Result<String, String> {
    let out = winepath_command(prefix, path)
        .output()
        .map_err(|e| format!("dist: winepath for {} did not start: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim_end();
    if !out.status.success() || line.is_empty() {
        return Err(format!("dist: winepath gave no Windows path for {}", path.display()));
    }
    Ok(line.to_owned())
}

/// `wineserver -k`, then `wineserver -w`, in `prefix`: ends what runs there, and nothing in any other prefix, and
/// waits for its wineserver to be gone.
fn wineserver_stop(prefix: &Path) -> [Command; 2] {
    ["-k", "-w"].map(|flag| {
        let mut c = Command::new("wineserver");
        c.arg(flag).env("WINEPREFIX", prefix);
        c
    })
}

/// The Wine prefix made for one run ([`wine_prefix`], [`WINE_MARK`] in it). Dropped, at the end of the run or on its
/// error, its wineserver is ended and the folder removed.
struct WinePrefix {
    dir: PathBuf,
}

impl WinePrefix {
    fn make(out: &Path) -> Result<WinePrefix, String> {
        let dir = wine_prefix(out)?;
        let io = |e: std::io::Error| format!("dist: {}: {e}", dir.display());
        if dir.exists() {
            if !dir.join(WINE_MARK).is_file() {
                return Err(format!("dist: {} is not a Wine prefix of mvs-dist's; move it away", dir.display()));
            }
            std::fs::remove_dir_all(&dir).map_err(io)?;
        }
        std::fs::create_dir_all(&dir).map_err(io)?;
        std::fs::write(dir.join(WINE_MARK), "A Wine prefix of mvs-dist: removed at the end of its run.\n")
            .map_err(io)?;
        println!("dist: running the Windows programs under Wine in {}", dir.display());
        Ok(WinePrefix { dir })
    }
}

impl Drop for WinePrefix {
    fn drop(&mut self) {
        for mut c in wineserver_stop(&self.dir) {
            let _ = c.stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
fn check_version(runner: &Runner, exe: &Path, want: &str) -> Result<(), String> {
    let out = runner
        .command(exe, &[Arg::Word("--version")])?
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
fn self_check(runner: &Runner, exe: &Path, store: Option<&Path>, tmp: &Path) -> Result<Vec<u8>, String> {
    let mut args = vec![Arg::Word("--self-test"), Arg::Word("--check-out"), Arg::Path(tmp)];
    if let Some(s) = store {
        args.extend([Arg::Word("--store"), Arg::Path(s)]);
    }
    let out = runner
        .command(exe, &args)?
        .output()
        .map_err(|e| format!("dist: {} --self-test did not start: {e}", exe.display()))?;
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

/// A file of a zip: its name in the zip, its bytes, whether it is executable.
type File = (String, Vec<u8>, bool);

/// In the Linux game's folder, `tools/linux-build.sh release`'s stamp: the version the game reports and the two
/// programs' SHA-256. It is not packed.
const LINUX_STAMP: &str = "linux-build.json";

#[derive(serde::Deserialize)]
struct LinuxStamp {
    version: String,
    exe: String,
    launcher: String,
}

/// The files the Linux zip takes from the Linux game's folder, by their names in both, each with whether it is
/// executable: the two programs first, then the libraries and their licenses. Nothing else of the folder is packed.
fn linux_platform_files() -> Vec<(String, bool)> {
    let mut files = vec![(in_game(LINUX_GAME_EXE), true), (LINUX_LAUNCHER_EXE.to_owned(), true)];
    files.extend(LINUX_LIBS.map(|l| (in_game(l), false)));
    files.extend([LICENSE_DXVK, LICENSE_SDL3].map(|l| (in_game(&format!("{LICENSES_DIR}/{l}")), false)));
    files
}

/// The Linux game's platform files ([`linux_platform_files`], in its order), read from its folder.
struct LinuxFolder {
    files: Vec<File>,
}

/// The Linux game's folder `dir`, checked before anything is built: its stamp names `version` and the hashes of the
/// two programs there, and every platform file reads.
fn linux_folder(dir: &Path, version: &str) -> Result<LinuxFolder, String> {
    let stamp_file = dir.join(in_game(LINUX_STAMP));
    let stamp: LinuxStamp = serde_json::from_slice(&read(&stamp_file)?)
        .map_err(|e| format!("dist: {} is not a Linux build's stamp: {e}", stamp_file.display()))?;
    if stamp.version != version {
        return Err(format!("dist: the Linux game in {} is {}, not {version}", dir.display(), stamp.version));
    }
    let files = linux_platform_files()
        .into_iter()
        .map(|(name, executable)| Ok((name.clone(), read(&dir.join(&name))?, executable)))
        .collect::<Result<Vec<File>, String>>()?;
    // the two programs come first
    for ((name, data, _), want) in files.iter().zip([&stamp.exe, &stamp.launcher]) {
        if sha256_hex(data) != want.to_ascii_lowercase() {
            return Err(format!(
                "dist: {name} in {} is not the one its {LINUX_STAMP} names (built again since?)",
                dir.display()
            ));
        }
    }
    Ok(LinuxFolder { files })
}

/// What both game zips take alike: the repo's texts and the Windows `mvs.exe`'s self check.
struct Shared {
    /// The project's license (`LICENSE`), as the relay's zip takes it.
    license: Vec<u8>,
    /// The importer's notices: CUE4Parse's (mvs-uasset), then ww2ogg's (mvs-import).
    notices: Vec<u8>,
    cue4parse: Vec<u8>,
    online: Vec<u8>,
    /// SELF_CHECK_JSON, from the Windows run: no Linux program is run here.
    check: Vec<u8>,
}

/// The EOS files' bytes.
struct EosBytes {
    dll: Vec<u8>,
    so: Option<Vec<u8>>,
    credentials: Vec<u8>,
}

/// `files` with the release's own list after them: the files an update may replace or remove, the list itself among
/// them.
fn with_list(mut files: Vec<File>) -> Vec<File> {
    let list = in_game(RELEASE_FILES);
    let mut names: Vec<&str> = files.iter().map(|(n, _, _)| n.as_str()).chain([list.as_str()]).collect();
    names.sort_unstable();
    let text = names.iter().map(|n| format!("{n}\n")).collect::<String>().into_bytes();
    files.push((list, text, false));
    files
}

/// The files of the game's zips, in order: the Windows zip's from its two programs (`windows`: the game's, the
/// launcher's), and with `linux`, the Linux zip's from that folder's platform files. Everything else both take alike
/// from `shared` and `eos`, each its own platform's EOS library.
fn game_files(
    shared: &Shared,
    windows: [Vec<u8>; 2],
    eos: Option<&EosBytes>,
    linux: Option<LinuxFolder>,
) -> Result<(Vec<File>, Option<Vec<File>>), String> {
    let mut license = shared.license.clone();
    if eos.is_some() {
        license.extend_from_slice(b"\n\n");
        license.extend_from_slice(EOS_NOTICE.as_bytes());
    }
    let common = || -> Vec<File> {
        vec![
            (LICENSE.to_owned(), license.clone(), false),
            (in_game(&format!("{LICENSES_DIR}/{NOTICE_IMPORTER}")), shared.notices.clone(), false),
            (in_game(&format!("{LICENSES_DIR}/{LICENSE_CUE4PARSE}")), shared.cue4parse.clone(), false),
            (in_game(ONLINE_JSON), shared.online.clone(), false),
            (in_game(SELF_CHECK_JSON), shared.check.clone(), false),
        ]
    };
    let [exe, launcher] = windows;
    let mut files = vec![(in_game(GAME_EXE), exe, false), (LAUNCHER_EXE.to_owned(), launcher, false)];
    files.extend(common());
    if let Some(e) = eos {
        files.push((in_game(EOS_DLL), e.dll.clone(), false));
        files.push((in_game(EOS_CREDENTIALS), e.credentials.clone(), false));
    }
    let windows = with_list(files);
    let Some(LinuxFolder { files: mut platform }) = linux else {
        return Ok((windows, None));
    };
    // the two programs, then what the Windows zip has, then the libraries and their licenses
    let rest = platform.split_off(2);
    let mut files = platform;
    files.extend(common());
    files.extend(rest);
    if let Some(e) = eos {
        let so = e.so.clone().ok_or("dist: --eos with --game-linux has no Linux EOS library")?;
        files.push((in_game(EOS_SO), so, false));
        files.push((in_game(EOS_CREDENTIALS), e.credentials.clone(), false));
    }
    Ok((windows, Some(with_list(files))))
}

/// A self check's reference made beforehand: `file` read, and held to the id `exe --compat-id` prints (the id a
/// player's fresh import gets; a reference for another one is never used).
fn made_self_check(runner: &Runner, exe: &Path, file: &Path, repo: &Path) -> Result<Vec<u8>, String> {
    let text = read(file)?;
    let r: SelfCheckRef =
        serde_json::from_slice(&text).map_err(|e| format!("dist: {} is not a self check: {e}", file.display()))?;
    let out = runner
        .command(exe, &[Arg::Word("--compat-id")])?
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
    let host = Host::here();
    // on Linux, the Linux game and relay are this repo's builds unless given; those built in this run are checked
    // once built
    let linux_host = host == Host::Linux;
    let game_linux = o.game_linux.clone().or_else(|| linux_host.then(|| o.repo.join(LINUX_RELEASE)));
    let relay_linux = o.relay_linux.clone().or_else(|| linux_host.then(|| o.repo.join(LINUX_RELAY)));
    let build_game = linux_host && o.game_linux.is_none() && !o.no_build;
    let build_relay = linux_host && o.relay_linux.is_none() && !o.no_build;
    let eos = if o.eos { Some(eos_files(&o.repo, game_linux.is_some())?) } else { None };
    if let Some(l) = &relay_linux
        && !build_relay
        && !l.is_file()
    {
        return Err(format!("dist: no Linux relay at {}", l.display()));
    }
    let mut linux =
        if build_game { None } else { game_linux.as_deref().map(|d| linux_folder(d, &o.version)).transpose()? };
    let key_file = o.repo.join("content/local.json");
    let key = guard::key_here(Some(&key_file).filter(|f| f.is_file()).map(PathBuf::as_path))?;
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
    if linux_host {
        println!("{}", sources_line(o));
    }

    // (the release workflow's programs when --programs names them, else this host's build)
    let built = o
        .programs
        .clone()
        .unwrap_or_else(|| built_dir(host, &o.repo, std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)));
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
        cargo_build(o, host, "mvs-app", &["mvs", "mvs-launcher"])?;
        cargo_build(o, host, "mvs-relay", &["mvs-relay"])?;
        let stamp = Stamp {
            exe: sha256_hex(&read(&exe)?),
            launcher: sha256_hex(&read(&launcher_exe)?),
            relay: sha256_hex(&read(&relay_exe)?),
            ..stamp
        };
        let text = serde_json::to_string_pretty(&stamp).expect("a Stamp always serialises");
        std::fs::write(&stamp_file, text).map_err(|e| format!("dist: {}: {e}", stamp_file.display()))?;
    }
    if build_game {
        linux_build(o, LINUX_GAME_BUILD, "the Linux game")?;
        linux = Some(linux_folder(&o.repo.join(LINUX_RELEASE), &o.version)?);
    }
    if build_relay {
        linux_build(o, LINUX_RELAY_BUILD, "the Linux relay")?;
    }
    // what the programs say they are (the run's Wine prefix, on Linux, is ended and removed with the runner)
    let check = {
        let runner = Runner::for_host(host, &o.out)?;
        check_version(&runner, &exe, &format!("mvs {}", o.version))?;
        check_version(&runner, &launcher_exe, &format!("mvs-launcher {}", o.version))?;
        check_version(&runner, &relay_exe, &format!("mvs-relay {}", o.version))?;
        match &o.self_check {
            Some(file) => made_self_check(&runner, &exe, file, &o.repo)?,
            None => self_check(&runner, &exe, o.store.as_deref(), &o.out.join(SELF_CHECK_JSON))?,
        }
    };
    // the Linux relay built in the container (here or by the last run), run on this host
    if linux_host && o.relay_linux.is_none() {
        check_version(&Runner::Direct, &o.repo.join(LINUX_RELAY), &format!("mvs-relay {}", o.version))?;
    }

    // the game's folders and zips
    let uasset = o.repo.join("crates/mvs-uasset");
    let mut notices = read(&uasset.join("NOTICE"))?;
    notices.extend_from_slice(b"\n");
    notices.extend_from_slice(&read(&o.repo.join("crates/mvs-import/NOTICE"))?);
    let shared = Shared {
        license: read(&o.repo.join("LICENSE"))?,
        notices,
        cue4parse: read(&uasset.join("LICENSE-CUE4Parse"))?,
        online: read(&o.repo.join(ONLINE_JSON))?,
        check,
    };
    let eos_bytes = match &eos {
        Some(e) => Some(EosBytes {
            dll: read(&e.dll)?,
            so: e.so.as_deref().map(read).transpose()?,
            credentials: e.credentials.clone(),
        }),
        None => None,
    };
    let (files, linux_files) = game_files(&shared, [read(&exe)?, read(&launcher_exe)?], eos_bytes.as_ref(), linux)?;
    let game_name = game_zip_name(&o.version);
    let game = lay_out(&o.out.join(format!("MVS-Classic-{}", o.version)), files)?;
    let game_zip = zip::write(&game)?;
    let game_path = o.out.join(&game_name);
    std::fs::write(&game_path, &game_zip).map_err(|e| format!("dist: {}: {e}", game_path.display()))?;
    println!("dist: {game_name} ({} bytes, SHA-256 {})", game_zip.len(), sha256_hex(&game_zip));
    let linux_path = match linux_files {
        Some(files) => {
            let name = game_zip_name_linux(&o.version);
            let linux = lay_out(&o.out.join(format!("MVS-Classic-{}-linux", o.version)), files)?;
            let linux_zip = zip::write(&linux)?;
            let path = o.out.join(&name);
            std::fs::write(&path, &linux_zip).map_err(|e| format!("dist: {}: {e}", path.display()))?;
            println!("dist: {name} ({} bytes, SHA-256 {})", linux_zip.len(), sha256_hex(&linux_zip));
            Some(path)
        }
        None => {
            println!("dist: no --game-linux: the release holds the Windows game only");
            None
        }
    };

    // the relay's
    let mut files = vec![
        ("mvs-relay.exe".to_owned(), read(&relay_exe)?, false),
        ("README.md".to_owned(), read(&o.repo.join("docs/relay.md"))?, false),
        (LICENSE.to_owned(), shared.license, false),
    ];
    match &relay_linux {
        Some(l) => files.push(("mvs-relay".to_owned(), read(l)?, true)),
        None => println!("dist: no --relay-linux: the relay zip holds the Windows program only"),
    }
    let relay_name = relay_zip_name(&o.version);
    let relay = lay_out(&o.out.join(format!("MVS-Classic-Relay-{}", o.version)), files)?;
    let relay_zip = zip::write(&relay)?;
    let relay_path = o.out.join(&relay_name);
    std::fs::write(&relay_path, &relay_zip).map_err(|e| format!("dist: {}: {e}", relay_path.display()))?;
    println!("dist: {relay_name} ({} bytes, SHA-256 {})", relay_zip.len(), sha256_hex(&relay_zip));

    // the guard over what is handed out
    match &key {
        Some(k) => println!("dist: the key from {}", k.source),
        None => println!("dist: {}", guard::NO_KEY),
    }
    let key = key.map(|k| k.key);
    let mut secrets = eos.as_ref().and_then(|e| guard::eos_secrets(&e.credentials)).unwrap_or_default();
    let mut findings = Vec::new();
    // the game's zips first: the relay's is searched for the credentials of their resources/eos.json too
    let mut zips = vec![(&game_path, Zip::Game)];
    zips.extend(linux_path.as_ref().map(|p| (p, Zip::GameLinux)));
    zips.push((&relay_path, Zip::Relay));
    for &(path, kind) in &zips {
        let r = guard::check_zip(path, kind, key.as_ref(), &secrets)?;
        println!("guard: {}: {} files, {} bytes read", path.display(), r.files, r.bytes);
        findings.extend(r.findings);
        secrets = r.secrets;
    }
    if !findings.is_empty() {
        for f in &findings {
            eprintln!("guard: {f}");
        }
        for (p, _) in &zips {
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
    use super::*;

    /// A fresh folder under the temp dir for one test.
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mvs-dist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, data: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    /// A Linux game's folder as `tools/linux-build.sh release` lays it out, for `version`, with a file of its own
    /// beside the platform files and its own online.json.
    fn linux_build(name: &str, version: &str) -> PathBuf {
        let dir = temp(name);
        for (file, _) in linux_platform_files() {
            put(&dir.join(&file), format!("folder's {file}").as_bytes());
        }
        put(&dir.join(in_game(ONLINE_JSON)), b"the folder's online.json");
        put(&dir.join(in_game("notes.txt")), b"not the release's");
        let hash = |f: &str| sha256_hex(&std::fs::read(dir.join(f)).unwrap());
        let stamp = format!(
            r#"{{"version": "{version}", "exe": "{}", "launcher": "{}"}}"#,
            hash(&in_game(LINUX_GAME_EXE)),
            hash(LINUX_LAUNCHER_EXE)
        );
        put(&dir.join(in_game(LINUX_STAMP)), stamp.as_bytes());
        dir
    }

    fn shared() -> Shared {
        Shared {
            license: b"MIT".to_vec(),
            notices: b"the importer's notices".to_vec(),
            cue4parse: b"CUE4Parse's license".to_vec(),
            online: b"the repo's online.json".to_vec(),
            check: br#"{"compat":"c0ffee","hash":305419896}"#.to_vec(),
        }
    }

    fn windows() -> [Vec<u8>; 2] {
        [b"MZ the game".to_vec(), b"MZ the launcher".to_vec()]
    }

    fn eos_bytes() -> EosBytes {
        EosBytes {
            dll: b"MZ the EOS dll".to_vec(),
            so: Some(b"\x7fELF the EOS so".to_vec()),
            credentials: b"{\"clientSecret\":\"s\"}".to_vec(),
        }
    }

    fn names(files: &[File]) -> Vec<&str> {
        files.iter().map(|(n, _, _)| n.as_str()).collect()
    }

    fn file<'a>(files: &'a [File], name: &str) -> &'a [u8] {
        &files.iter().find(|(n, _, _)| n == name).unwrap_or_else(|| panic!("no {name}")).1
    }

    /// Both game zips' files for the Linux folder `name` of version 0.1.0.
    fn both(name: &str, eos: Option<&EosBytes>) -> (Vec<File>, Vec<File>) {
        let dir = linux_build(name, "0.1.0");
        let linux = linux_folder(&dir, "0.1.0").expect("the folder reads");
        let (w, l) = game_files(&shared(), windows(), eos, Some(linux)).expect("the lists");
        std::fs::remove_dir_all(&dir).unwrap();
        (w, l.expect("a Linux zip"))
    }

    #[test]
    fn the_linux_zip_carries_the_windows_self_check() {
        let (w, l) = both("self-check", None);
        let check = in_game(SELF_CHECK_JSON);
        assert_eq!(file(&l, &check), file(&w, &check));
        assert_eq!(file(&l, &check), shared().check.as_slice());
    }

    #[test]
    fn only_the_linux_programs_are_executable() {
        let (w, l) = both("executable", Some(&eos_bytes()));
        let executable: Vec<&str> = l.iter().filter(|f| f.2).map(|f| f.0.as_str()).collect();
        assert_eq!(executable, ["game/MVS Classic", "MVSC Launcher"]);
        assert!(w.iter().all(|f| !f.2), "nothing in the Windows zip is executable");
    }

    #[test]
    fn the_linux_folder_gives_only_its_platform_files() {
        let (_, l) = both("platform-files", None);
        let mut got = names(&l);
        got.sort_unstable();
        assert_eq!(
            got,
            [
                "LICENSE.txt",
                "MVSC Launcher",
                "game/MVS Classic",
                "game/libSDL3.so.0",
                "game/libdxvk_d3d11.so.0",
                "game/libdxvk_dxgi.so.0",
                "game/licenses/LICENSE-CUE4Parse.txt",
                "game/licenses/LICENSE-DXVK.txt",
                "game/licenses/LICENSE-SDL3.txt",
                "game/licenses/NOTICE-importer.txt",
                "game/online.json",
                "game/release-files.txt",
                "game/self-check.json",
            ]
        );
        // from the folder: its platform files; the rest as the Windows zip makes it
        assert_eq!(file(&l, "game/libSDL3.so.0"), b"folder's game/libSDL3.so.0");
        assert_eq!(file(&l, "game/online.json"), b"the repo's online.json");
        assert_eq!(file(&l, LICENSE), b"MIT");
        let list: String = got.iter().map(|n| format!("{n}\n")).collect();
        assert_eq!(file(&l, "game/release-files.txt"), list.as_bytes());
        // with EOS: the Linux library, the same credentials and the notice in the license
        let e = eos_bytes();
        let (w, l) = both("platform-files-eos", Some(&e));
        assert_eq!(file(&l, "game/libEOSSDK-Linux-Shipping.so"), e.so.as_deref().unwrap());
        assert_eq!(file(&l, "game/resources/eos.json"), file(&w, "game/resources/eos.json"));
        assert!(!names(&l).contains(&"game/EOSSDK-Win64-Shipping.dll"));
        assert!(file(&l, LICENSE).ends_with(EOS_NOTICE.as_bytes()));
        assert_eq!(file(&l, LICENSE), file(&w, LICENSE));
    }

    #[test]
    fn a_linux_game_of_another_version_is_refused() {
        let dir = linux_build("version", "0.2.0");
        let err = linux_folder(&dir, "0.1.0").err().expect("refused");
        assert!(err.contains("is 0.2.0, not 0.1.0"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_linux_program_its_stamp_does_not_name_is_refused() {
        for program in [in_game(LINUX_GAME_EXE), LINUX_LAUNCHER_EXE.to_owned()] {
            let dir = linux_build("hash", "0.1.0");
            put(&dir.join(&program), b"built again");
            let err = linux_folder(&dir, "0.1.0").err().expect("refused");
            assert!(err.contains(&format!("{program} in ")) && err.contains("is not the one"), "{err}");
            std::fs::remove_dir_all(&dir).unwrap();
        }
        // a folder without its stamp is refused too
        let dir = linux_build("no-stamp", "0.1.0");
        std::fs::remove_file(dir.join(in_game(LINUX_STAMP))).unwrap();
        assert!(linux_folder(&dir, "0.1.0").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn eos_with_the_linux_game_takes_the_so() {
        let dir = temp("eos-so");
        let secret = "FAKE-client-secret-0123456789abcdef";
        let credentials = dir.join("eos.json");
        put(
            &credentials,
            format!(
                r#"{{"productId":"p","sandboxId":"s","deploymentId":"d","clientId":"c","clientSecret":"{secret}"}}"#
            )
            .as_bytes(),
        );
        put(&dir.join(EOS_DLL), b"MZ");
        put(&dir.join(EOS_SO), b"\x7fELF");
        put(&dir.join("libother.so"), b"\x7fELF");
        let vars = |so: Option<&str>| {
            let dir = dir.clone();
            let credentials = credentials.clone();
            let so = so.map(|s| dir.join(s));
            move |k: &str| match k {
                "MVS_EOS_DLL" => Some(dir.join(EOS_DLL)),
                "MVS_EOS_CREDENTIALS" => Some(credentials.clone()),
                "MVS_EOS_SO" => so.clone(),
                _ => None,
            }
        };
        // without MVS_EOS_SO, the repo's copy: refused when the repo has none, taken when it has
        let err = eos_files_from(vars(None), &dir, true).err().expect("refused");
        let in_dir = dir.join("third_party/eos/libEOSSDK-Linux-Shipping.so");
        assert!(
            err.contains("MVS_EOS_SO is not a libEOSSDK-Linux-Shipping.so")
                && err.contains(&in_dir.display().to_string())
                && !err.contains(secret),
            "{err}"
        );
        let repo = dir.join("repo");
        let in_repo = repo.join("third_party/eos/libEOSSDK-Linux-Shipping.so");
        put(&in_repo, b"\x7fELF");
        assert_eq!(eos_files_from(vars(None), &repo, true).unwrap().so, Some(in_repo));
        let err = eos_files_from(vars(Some("libother.so")), &dir, true).err().expect("refused");
        assert!(err.contains("MVS_EOS_SO is not a libEOSSDK-Linux-Shipping.so") && !err.contains(secret), "{err}");
        assert_eq!(eos_files_from(vars(Some(EOS_SO)), &dir, true).unwrap().so, Some(dir.join(EOS_SO)));
        // without the Linux game the Windows files are enough, and the library is not taken
        assert_eq!(eos_files_from(vars(Some(EOS_SO)), &dir, false).unwrap().so, None);
        assert!(eos_files_from(vars(None), &dir, false).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
        // and the lists refuse a Linux zip with EOS but no library
        let linux = linux_build("eos-so-lists", "0.1.0");
        let folder = linux_folder(&linux, "0.1.0").unwrap();
        let no_so = EosBytes { so: None, ..eos_bytes() };
        assert!(game_files(&shared(), windows(), Some(&no_so), Some(folder)).is_err());
        std::fs::remove_dir_all(&linux).unwrap();
    }

    #[test]
    fn the_windows_zip_is_the_same_with_the_linux_game() {
        for eos in [None, Some(eos_bytes())] {
            let (alone, none) = game_files(&shared(), windows(), eos.as_ref(), None).unwrap();
            assert!(none.is_none());
            let (with_linux, _) = both(&format!("windows-{}", eos.is_some()), eos.as_ref());
            assert_eq!(alone, with_linux);
            // the names and order of the Windows zip before the Linux one
            let mut want = vec![
                "game/MVS Classic.exe",
                "MVSC Launcher.exe",
                "LICENSE.txt",
                "game/licenses/NOTICE-importer.txt",
                "game/licenses/LICENSE-CUE4Parse.txt",
                "game/online.json",
                "game/self-check.json",
            ];
            if eos.is_some() {
                want.extend(["game/EOSSDK-Win64-Shipping.dll", "game/resources/eos.json"]);
            }
            want.push("game/release-files.txt");
            assert_eq!(names(&alone), want);
        }
    }

    fn options(feed: Option<&str>) -> Options {
        Options {
            version: "0.1.0".to_owned(),
            feed: feed.map(str::to_owned),
            eos: false,
            out: PathBuf::from("/r/target/dist"),
            repo: PathBuf::from("/r"),
            relay_linux: None,
            game_linux: None,
            no_build: false,
            programs: None,
            store: None,
            self_check: None,
        }
    }

    fn args(c: &Command) -> Vec<String> {
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    /// The variable `k` as `c` gives it: None when `c` leaves it alone, Some(None) when it removes it.
    fn env(c: &Command, k: &str) -> Option<Option<String>> {
        c.get_envs().find(|(n, _)| *n == k).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    const CRT_STATIC: &str = "-C target-feature=+crt-static";
    const RUSTFLAGS: &str = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS";

    #[test]
    fn the_windows_build_command_is_unchanged() {
        let bins = ["build", "--release", "-p", "mvs-app", "--bin", "mvs", "--bin", "mvs-launcher"];
        for (slot, feed) in [(true, Some("https://feed")), (false, None)] {
            let c = build_command(&options(feed), Host::Windows, slot, "mvs-app", &["mvs", "mvs-launcher"]);
            if slot {
                assert_eq!(c.get_program(), "powershell");
                // (the script's path compared as a path: on Windows the join writes `/r\tools/...`)
                let got = args(&c);
                assert_eq!(got[..4], ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
                assert_eq!(Path::new(&got[4]), Path::new("/r/tools/cargo-slot.ps1"));
                assert_eq!(got[5..], bins);
            } else {
                assert_eq!(c.get_program(), "cargo");
                assert_eq!(args(&c), bins);
            }
            assert_eq!(c.get_current_dir(), Some(Path::new("/r")));
            assert_eq!(env(&c, "MVS_VERSION"), Some(Some("0.1.0".to_owned())));
            assert_eq!(env(&c, RUSTFLAGS), Some(Some(CRT_STATIC.to_owned())));
            assert_eq!(env(&c, "MVS_UPDATE_FEED"), Some(feed.map(str::to_owned)));
            assert_eq!(c.get_envs().count(), 3);
        }
    }

    #[test]
    fn the_linux_build_goes_through_the_cross_script() {
        let c = build_command(&options(Some("https://feed")), Host::Linux, true, "mvs-relay", &["mvs-relay"]);
        assert_eq!(Path::new(c.get_program()), Path::new("/r/tools/windows-cross.sh"));
        assert_eq!(args(&c), ["build", "--release", "-p", "mvs-relay", "--bin", "mvs-relay"]);
        assert_eq!(c.get_current_dir(), Some(Path::new("/r")));
        assert_eq!(env(&c, "MVS_VERSION"), Some(Some("0.1.0".to_owned())));
        // the script adds the splat's paths after this
        assert_eq!(env(&c, RUSTFLAGS), Some(Some(CRT_STATIC.to_owned())));
        assert_eq!(env(&c, "MVS_UPDATE_FEED"), Some(Some("https://feed".to_owned())));
        let c = build_command(&options(None), Host::Linux, false, "mvs-relay", &["mvs-relay"]);
        assert_eq!(env(&c, "MVS_UPDATE_FEED"), Some(None));
    }

    /// The cross-build script with a cargo that prints what it was given, and its own clang-cl, llvm-lib and
    /// lld-link: `flags` in the RUSTFLAGS variable, `target_dir` in CARGO_TARGET_DIR, the splat at `splat`.
    #[cfg(unix)]
    fn cross(
        name: &str,
        flags: Option<&str>,
        target_dir: Option<&Path>,
        splat: Option<&Path>,
    ) -> (bool, String, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp(name);
        let bin = dir.join("bin");
        let cargo = "#!/bin/sh\nprintf 'arg %s\\n' \"$@\"\nprintf 'flags %s\\n' \"$CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS\"\nprintf 'dir %s\\n' \"$CARGO_TARGET_DIR\"\n";
        for (tool, body) in
            [("cargo", cargo), ("clang-cl", "#!/bin/sh\n"), ("llvm-lib", "#!/bin/sh\n"), ("lld-link", "#!/bin/sh\n")]
        {
            let f = bin.join(tool);
            put(&f, body.as_bytes());
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut c = Command::new("bash");
        c.arg(repo.join(CROSS_SCRIPT))
            .args(["build", "--release", "-p", "mvs-relay"])
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XWIN_SPLAT", splat.unwrap_or(&dir.join("no-splat")))
            .env_remove(RUSTFLAGS)
            .env_remove("CARGO_TARGET_DIR");
        if let Some(f) = flags {
            c.env(RUSTFLAGS, f);
        }
        if let Some(d) = target_dir {
            c.env("CARGO_TARGET_DIR", d);
        }
        let out = c.output().expect("bash runs");
        std::fs::remove_dir_all(&dir).unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        (out.status.success(), text(&out.stdout), text(&out.stderr))
    }

    #[cfg(unix)]
    #[test]
    fn the_cross_script_adds_the_splat_after_the_flags_given() {
        let splat = temp("cross-splat");
        std::fs::create_dir_all(splat.join("crt/include")).unwrap();
        let s = splat.display();
        let lib = format!("-Lnative={s}/crt/lib/x64 -Lnative={s}/sdk/lib/um/x64 -Lnative={s}/sdk/lib/ucrt/x64");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        let (ok, out, err) = cross("cross-flags", Some(CRT_STATIC), None, Some(&splat));
        assert!(ok, "{err}");
        let args: Vec<&str> = out.lines().filter_map(|l| l.strip_prefix("arg ")).collect();
        assert_eq!(args, ["build", "--target", "x86_64-pc-windows-msvc", "--release", "-p", "mvs-relay"]);
        assert!(out.contains(&format!("flags {CRT_STATIC} {lib}\n")), "{out}");
        assert!(out.contains(&format!("dir {}\n", repo.join(CROSS_TARGET_DIR).display())), "{out}");
        // with no flags set, the splat's alone; a target dir set is kept
        let (ok, out, err) = cross("cross-alone", None, Some(Path::new("/elsewhere")), Some(&splat));
        assert!(ok, "{err}");
        assert!(out.contains(&format!("flags {lib}\n")) && out.contains("dir /elsewhere\n"), "{out}");
        std::fs::remove_dir_all(&splat).unwrap();
        // without a splat it stops, says how the user makes one, and runs nothing
        let (ok, out, err) = cross("cross-none", Some(CRT_STATIC), None, None);
        assert!(!ok && out.is_empty() && err.contains("xwin --accept-license splat"), "{out}{err}");
    }

    #[test]
    fn the_built_dir_follows_the_host() {
        let repo = Path::new("/r");
        assert_eq!(built_dir(Host::Windows, repo, None), Path::new("/r/target/release"));
        assert_eq!(built_dir(Host::Windows, repo, Some("/t".into())), Path::new("/t/release"));
        assert_eq!(built_dir(Host::Linux, repo, None), Path::new("/r/target/xwin/x86_64-pc-windows-msvc/release"));
        assert_eq!(built_dir(Host::Linux, repo, Some("/t".into())), Path::new("/t/x86_64-pc-windows-msvc/release"));
    }

    #[test]
    fn the_linux_builds_take_the_version_and_no_feed() {
        for (build, want) in [
            (LINUX_GAME_BUILD, vec!["release"]),
            (LINUX_RELAY_BUILD, vec!["cargo", "build", "--release", "-p", "mvs-relay", "--bin", "mvs-relay"]),
        ] {
            let c = linux_build_command(&options(Some("https://feed")), build);
            assert_eq!(Path::new(c.get_program()), Path::new("/r/tools/linux-build.sh"));
            assert_eq!(args(&c), want);
            assert_eq!(c.get_current_dir(), Some(Path::new("/r")));
            assert_eq!(env(&c, "MVS_VERSION"), Some(Some("0.1.0".to_owned())));
            assert_eq!(env(&c, "MVS_UPDATE_FEED"), Some(None));
        }
    }

    #[test]
    fn the_wine_prefix_is_the_runs_own_beside_the_output() {
        let p = wine_prefix(Path::new("/r/target/dist")).unwrap();
        assert_eq!(p, Path::new("/r/target/dist.wine"));
        assert!(!p.starts_with("/r/target/dist/"));
        assert_eq!(wine_prefix(Path::new("/r/target/dist/")).unwrap(), Path::new("/r/target/dist.wine"));
        assert!(wine_prefix(Path::new("/")).is_err());
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            assert_ne!(wine_prefix(&home.join("dist")).unwrap(), home.join(".wine"));
        }
    }

    #[test]
    fn the_wine_command_gives_windows_paths_in_the_runs_prefix() {
        let prefix = Path::new("/r/target/dist.wine");
        let (tmp, store) = (Path::new("/r/target/dist/self-check.json"), Path::new("/s/store é"));
        let argv = [
            Arg::Word("--self-test"),
            Arg::Word("--check-out"),
            Arg::Path(tmp),
            Arg::Word("--store"),
            Arg::Path(store),
        ];
        let converted = std::cell::RefCell::new(Vec::new());
        let c = wine_command(prefix, Path::new("/b/mvs.exe"), &argv, |p| {
            converted.borrow_mut().push(p.to_path_buf());
            Ok(format!("W:{}", p.display()))
        })
        .unwrap();
        assert_eq!(c.get_program(), "wine");
        assert_eq!(
            args(&c),
            ["/b/mvs.exe", "--self-test", "--check-out", "W:/r/target/dist/self-check.json", "--store", "W:/s/store é"]
        );
        assert_eq!(converted.into_inner(), [tmp, store]);
        assert_eq!(env(&c, "WINEPREFIX"), Some(Some("/r/target/dist.wine".to_owned())));
        assert_eq!(env(&c, "WINEDEBUG"), Some(Some("-all".to_owned())));
        // a path that does not convert stops the run
        assert!(wine_command(prefix, Path::new("/b/mvs.exe"), &argv, |_| Err("no".to_owned())).is_err());
        // winepath converts in the same prefix
        let c = winepath_command(prefix, tmp);
        assert_eq!(c.get_program(), "winepath");
        assert_eq!(args(&c), ["-w", "/r/target/dist/self-check.json"]);
        assert_eq!(env(&c, "WINEPREFIX"), Some(Some("/r/target/dist.wine".to_owned())));
        // and the wineserver stopped is that prefix's
        let [kill, wait] = wineserver_stop(prefix);
        for (c, flag) in [(&kill, "-k"), (&wait, "-w")] {
            assert_eq!(c.get_program(), "wineserver");
            assert_eq!(args(c), [flag]);
            assert_eq!(env(c, "WINEPREFIX"), Some(Some("/r/target/dist.wine".to_owned())));
        }
    }

    #[test]
    fn the_windows_host_runs_the_programs_directly() {
        let c = Runner::Direct
            .command(Path::new("/b/mvs.exe"), &[Arg::Word("--check-out"), Arg::Path(Path::new("/o/c.json"))])
            .unwrap();
        assert_eq!(c.get_program(), "/b/mvs.exe");
        assert_eq!(args(&c), ["--check-out", "/o/c.json"]);
        assert_eq!(c.get_envs().count(), 0);
    }

    #[test]
    fn the_linux_host_says_where_each_part_comes_from() {
        let mut o = options(None);
        assert_eq!(
            sources_line(&o),
            "dist: the Windows programs built here (tools/windows-cross.sh); the Linux game built here \
             (target/linux-release); the Linux relay built here (target/linux-sniper/release/mvs-relay)"
        );
        o.no_build = true;
        o.game_linux = Some("/g".into());
        assert_eq!(
            sources_line(&o),
            "dist: the Windows programs from the last build (tools/windows-cross.sh); the Linux game given (/g); the \
             Linux relay from the last build (target/linux-sniper/release/mvs-relay)"
        );
        // the release workflow's programs (--programs)
        o.no_build = false;
        o.programs = Some("/p".into());
        assert!(sources_line(&o).starts_with("dist: the Windows programs given (/p); the Linux game given (/g);"));
    }

    #[test]
    fn the_linux_zip_is_never_the_windows_ones_name() {
        assert_eq!(game_zip_name_linux("0.1.0"), "MVS-Classic-0.1.0-linux.zip");
        assert_ne!(game_zip_name_linux("0.1.0"), game_zip_name("0.1.0"));
    }

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
