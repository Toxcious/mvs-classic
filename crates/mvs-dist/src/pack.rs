//! The release build: builds `mvs`, `mvs-launcher` and `mvs-relay` for a version and feed, lays
//! out the release folder (the launcher and the game side by side), zips it, zips the relay, and runs the guard over both zips. What
//! the guard refuses is deleted (the folders stay for a look). With `--game-linux`, a Linux game built elsewhere
//! (`tools/linux-build.sh release`) is laid out and zipped beside the Windows one, and guarded with it.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// Pack the EOS SDK's DLL and the credentials (`MVS_EOS_DLL`, `MVS_EOS_CREDENTIALS`), and with `game_linux` its
    /// Linux library (`MVS_EOS_SO`).
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
    /// The store the self check's match runs on (None: the one the built exe finds, its data root's).
    pub store: Option<PathBuf>,
}

/// The EOS files a release adds, checked before anything is built. The credentials are never printed: a bad file is
/// named by its path and the missing key's name only.
struct EosFiles {
    dll: PathBuf,
    /// The Linux library, for a release with the Linux game (None without it).
    so: Option<PathBuf>,
    credentials: Vec<u8>,
}

/// `linux`: a release with the Linux game, which needs `MVS_EOS_SO` too.
fn eos_files(linux: bool) -> Result<EosFiles, String> {
    eos_files_from(|k| std::env::var_os(k).map(PathBuf::from), linux)
}

/// [`eos_files`] with the variables `var` gives.
fn eos_files_from(var: impl Fn(&str) -> Option<PathBuf>, linux: bool) -> Result<EosFiles, String> {
    let (Some(dll), Some(credentials)) = (var("MVS_EOS_DLL"), var("MVS_EOS_CREDENTIALS")) else {
        return Err("dist: --eos needs MVS_EOS_DLL and MVS_EOS_CREDENTIALS".to_owned());
    };
    let named = dll.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(EOS_DLL));
    if !named || !dll.is_file() {
        return Err(format!("dist: MVS_EOS_DLL is not an {EOS_DLL}: {}", dll.display()));
    }
    let so = if linux {
        let so = var("MVS_EOS_SO").ok_or("dist: --eos with --game-linux needs MVS_EOS_SO")?;
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
        let so = e.so.clone().ok_or("dist: --eos with --game-linux needs MVS_EOS_SO")?;
        files.push((in_game(EOS_SO), so, false));
        files.push((in_game(EOS_CREDENTIALS), e.credentials.clone(), false));
    }
    Ok((windows, Some(with_list(files))))
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
    let eos = if o.eos { Some(eos_files(o.game_linux.is_some())?) } else { None };
    if let Some(l) = &o.relay_linux
        && !l.is_file()
    {
        return Err(format!("dist: no Linux relay at {}", l.display()));
    }
    let linux = o.game_linux.as_deref().map(|d| linux_folder(d, &o.version)).transpose()?;
    let key_file = o.repo.join("content/local.json");
    let key = if key_file.is_file() { Some(guard::read_key(&key_file)?) } else { None };
    // the build list the exe embeds (mvs-pak's include_str!): names, sizes and hashes only
    let list = BUILD_LIST;
    let found = guard::check_build_json(list, &String::from_utf8_lossy(&read(&o.repo.join(list))?));
    if !found.is_empty() {
        return Err(format!("dist: the guard refused the build list: {}", found.join("; ")));
    }
    fresh_out(&o.out)?;

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| o.repo.join("target"), PathBuf::from);
    let built = target.join("release");
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
    if o.no_build {
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
    let check = self_check(&exe, o.store.as_deref(), &o.out.join(SELF_CHECK_JSON))?;

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
    fn eos_with_the_linux_game_needs_the_so() {
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
        let err = eos_files_from(vars(None), true).err().expect("refused");
        assert_eq!(err, "dist: --eos with --game-linux needs MVS_EOS_SO");
        let err = eos_files_from(vars(Some("libother.so")), true).err().expect("refused");
        assert!(err.contains("MVS_EOS_SO is not a libEOSSDK-Linux-Shipping.so") && !err.contains(secret), "{err}");
        assert_eq!(eos_files_from(vars(Some(EOS_SO)), true).unwrap().so, Some(dir.join(EOS_SO)));
        // without the Linux game the Windows files are enough, and the library is not taken
        assert_eq!(eos_files_from(vars(Some(EOS_SO)), false).unwrap().so, None);
        assert!(eos_files_from(vars(None), false).is_ok());
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

    #[test]
    fn the_linux_zip_is_never_the_windows_ones_name() {
        assert_eq!(game_zip_name_linux("0.1.0"), "MVS-Classic-0.1.0-linux.zip");
        assert_ne!(game_zip_name_linux("0.1.0"), game_zip_name("0.1.0"));
    }
}
