//! What a release of the game is: its version and update feed, the feed's `latest.json`, its two programs (the
//! launcher and the game), the files a release folder holds beside them, and the zip it is handed out in. The
//! packaging tool writes them, the programs read them.

pub mod zip;

use serde::{Deserialize, Serialize};

/// This build's version: `MVS_VERSION` when it was built (the packaging tool sets it), else the crate's own (a dev
/// build, `0.0.0`).
pub const VERSION: &str = match option_env!("MVS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The update feed this build checks: `MVS_UPDATE_FEED` when it was built, the GitHub API's address of the
/// repository's latest release (`https://api.github.com/repos/<owner>/<repo>/releases/latest`, or a local one in its
/// shape for the release check). A build without one never checks.
pub const FEED: Option<&str> = option_env!("MVS_UPDATE_FEED");

/// The game's program in a release, in [`GAME_DIR`]: `--play --store <dir>` runs the menus and their matches on an
/// imported store.
pub const GAME_EXE: &str = "MVS Classic.exe";
/// The launcher's program in a release, at the top of the release folder: setup, the import, Home with its Play, the
/// settings, the updater. A player starts this one (the shortcuts point at it); its Play starts the game's.
pub const LAUNCHER_EXE: &str = "MVSC Launcher.exe";
/// The folder of a release that holds the game's program and the files it reads beside it (the EOS SDK, the online
/// defaults, the self check's reference, the notices): the top level holds only the launcher, the licence and
/// `data\`, so a player sees the one program to start.
pub const GAME_DIR: &str = "game";
/// The top-level names of a release before [`GAME_DIR`] (0.1.0), each now under it: an update removes them.
pub const OLD_TOP_LEVEL: [&str; 6] = [GAME_EXE, EOS_DLL, ONLINE_JSON, SELF_CHECK_JSON, "resources", LICENSES_DIR];
/// Beside the game's program: every file of the release, itself included, one `/`-separated path from the release
/// folder a line. An update replaces and removes only the files the old and the new release list: a file the player
/// put in the folder stays.
pub const RELEASE_FILES: &str = "release-files.txt";
/// The game's program as Cargo builds it (Cargo's names take no spaces); the packaging tool renames it to
/// [`GAME_EXE`].
pub const BUILT_GAME_EXE: &str = "mvs.exe";
/// The launcher's program as Cargo builds it, renamed to [`LAUNCHER_EXE`] in a release.
pub const BUILT_LAUNCHER_EXE: &str = "mvs-launcher.exe";
/// The game's program in a Linux release, in [`GAME_DIR`]: [`GAME_EXE`] without `.exe`.
pub const LINUX_GAME_EXE: &str = "MVS Classic";
/// The launcher's program in a Linux release, at the top of the release folder: [`LAUNCHER_EXE`] without `.exe`.
pub const LINUX_LAUNCHER_EXE: &str = "MVSC Launcher";
/// The game's program as Cargo builds it for Linux, renamed to [`LINUX_GAME_EXE`] in a release.
pub const BUILT_LINUX_GAME_EXE: &str = "mvs";
/// The launcher's program as Cargo builds it for Linux, renamed to [`LINUX_LAUNCHER_EXE`] in a release.
pub const BUILT_LINUX_LAUNCHER_EXE: &str = "mvs-launcher";

/// This platform's [`GAME_EXE`]: the Windows name on Windows, else [`LINUX_GAME_EXE`].
#[cfg(windows)]
pub const PLATFORM_GAME_EXE: &str = GAME_EXE;
/// This platform's [`GAME_EXE`]: the Windows name on Windows, else [`LINUX_GAME_EXE`].
#[cfg(not(windows))]
pub const PLATFORM_GAME_EXE: &str = LINUX_GAME_EXE;
/// This platform's [`LAUNCHER_EXE`]: the Windows name on Windows, else [`LINUX_LAUNCHER_EXE`].
#[cfg(windows)]
pub const PLATFORM_LAUNCHER_EXE: &str = LAUNCHER_EXE;
/// This platform's [`LAUNCHER_EXE`]: the Windows name on Windows, else [`LINUX_LAUNCHER_EXE`].
#[cfg(not(windows))]
pub const PLATFORM_LAUNCHER_EXE: &str = LINUX_LAUNCHER_EXE;
/// This platform's [`BUILT_GAME_EXE`]: the Windows name on Windows, else [`BUILT_LINUX_GAME_EXE`].
#[cfg(windows)]
pub const PLATFORM_BUILT_GAME_EXE: &str = BUILT_GAME_EXE;
/// This platform's [`BUILT_GAME_EXE`]: the Windows name on Windows, else [`BUILT_LINUX_GAME_EXE`].
#[cfg(not(windows))]
pub const PLATFORM_BUILT_GAME_EXE: &str = BUILT_LINUX_GAME_EXE;
/// This platform's [`BUILT_LAUNCHER_EXE`]: the Windows name on Windows, else [`BUILT_LINUX_LAUNCHER_EXE`].
#[cfg(windows)]
pub const PLATFORM_BUILT_LAUNCHER_EXE: &str = BUILT_LAUNCHER_EXE;
/// This platform's [`BUILT_LAUNCHER_EXE`]: the Windows name on Windows, else [`BUILT_LINUX_LAUNCHER_EXE`].
#[cfg(not(windows))]
pub const PLATFORM_BUILT_LAUNCHER_EXE: &str = BUILT_LINUX_LAUNCHER_EXE;

/// The game's program for the launcher in folder `dir`: a release's, in [`GAME_DIR`], else (a build in `target\`)
/// Cargo's beside it.
pub fn game_exe(dir: &std::path::Path) -> std::path::PathBuf {
    let r = dir.join(GAME_DIR).join(PLATFORM_GAME_EXE);
    if r.is_file() { r } else { dir.join(PLATFORM_BUILT_GAME_EXE) }
}

/// A file's name in a release zip under [`GAME_DIR`] (`game/MVS Classic.exe`), `/` between the parts.
pub fn in_game(name: &str) -> String {
    format!("{GAME_DIR}/{name}")
}

/// The release folder of a program in folder `dir`: the folder above for the game's in [`GAME_DIR`] (the launcher
/// beside that folder), else `dir` itself (the launcher's, a build in `target\`).
pub fn app_root(dir: &std::path::Path) -> std::path::PathBuf {
    match dir.parent() {
        Some(up) if dir.file_name().is_some_and(|n| n == GAME_DIR) && up.join(PLATFORM_LAUNCHER_EXE).is_file() => {
            up.to_path_buf()
        }
        _ => dir.to_path_buf(),
    }
}

/// The online defaults for a program in release folder `root`: the game folder's in a release, else beside it.
pub fn online_json(root: &std::path::Path) -> std::path::PathBuf {
    let r = root.join(GAME_DIR).join(ONLINE_JSON);
    if r.is_file() { r } else { root.join(ONLINE_JSON) }
}

/// The launcher's program in folder `dir`: a release's name, else (a build in `target\`) Cargo's.
pub fn launcher_exe(dir: &std::path::Path) -> std::path::PathBuf {
    beside(dir, PLATFORM_LAUNCHER_EXE, PLATFORM_BUILT_LAUNCHER_EXE)
}

fn beside(dir: &std::path::Path, release: &str, built: &str) -> std::path::PathBuf {
    let r = dir.join(release);
    if r.is_file() { r } else { dir.join(built) }
}

/// The line `mvs.exe --version` prints.
pub fn version_line() -> String {
    format!("mvs {VERSION}")
}

/// The line `mvs-launcher.exe --version` prints.
pub fn launcher_version_line() -> String {
    format!("mvs-launcher {VERSION}")
}

/// The line `mvs.exe --self-test` ends with when every check passed (`self-test result: FAIL` when one did not).
pub const SELF_TEST_PASS: &str = "self-test result: PASS";

/// Beside the game's program in a release folder: the EOS SDK's DLL (a release built with Epic's SDK; none without it).
pub const EOS_DLL: &str = "EOSSDK-Win64-Shipping.dll";
/// Beside the game's program in the Linux release folder: the EOS SDK's library for x86_64 (a release built with
/// Epic's SDK; none without it).
pub const EOS_SO: &str = "libEOSSDK-Linux-Shipping.so";
/// Beside the game's program: the product's EOS credentials, read only by the SDK's start.
pub const EOS_CREDENTIALS: &str = "resources/eos.json";
/// At the top of the release folder: the player's data (settings, store, profile), which an update never touches.
pub const DATA_DIR: &str = "data";
/// Under the data folder: where an update is downloaded and unpacked.
pub const UPDATE_DIR: &str = "update";
/// At the top of the release folder: the licence text (the project's MIT licence, then the Epic notice when the SDK is packed).
pub const LICENSE: &str = "LICENSE.txt";
/// Beside the game's program: the third-party notices (the importer's, CUE4Parse's).
pub const LICENSES_DIR: &str = "licenses";
/// Beside the game's program: the release's online defaults (the relay a player signs in to, Epic, LAN, direct links;
/// mvs_settings::online), the repo root's `online.json`. Its `relayId` is the id of the key the release's relay proves
/// (`relay id ...` in the relay's log): the game refuses a relay at that address that proves another. A release
/// without it is not pinned (a fork's or a dev config).
pub const ONLINE_JSON: &str = "online.json";
/// Beside the game's program: the self check's reference, the hash of the fixed check match as the packing run of this release
/// made it ([`SelfCheckRef`]).
pub const SELF_CHECK_JSON: &str = "self-check.json";
/// The join links' scheme, registered for the Windows user by the launcher: `mvsclassic://join/<code>`.
pub const URL_SCHEME: &str = "mvsclassic";

/// `self-check.json`: the fixed check match's hash (mvs_net::session::wire_hash of its final world) for the game
/// data of compatibility id `compat`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfCheckRef {
    pub compat: String,
    pub hash: u32,
}

/// The game's zip of release `version`, the asset of its GitHub release the updater downloads.
pub fn game_zip_name(version: &str) -> String {
    format!("MVS-Classic-{version}.zip")
}

/// The Linux game's zip of release `version`, beside [`game_zip_name`]'s. The updater takes its asset by that exact
/// name, so a Windows client never takes this one.
pub fn game_zip_name_linux(version: &str) -> String {
    format!("MVS-Classic-{version}-linux.zip")
}

/// The release the feed offers: its game zip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// `x.y.z`
    pub version: String,
    /// The zip's name ([`game_zip_name`]).
    pub file: String,
    /// Where the zip is downloaded from.
    pub url: String,
    /// The zip's size in bytes.
    pub size: u64,
    /// The zip's SHA-256, lowercase hex.
    pub sha256: String,
}

/// The fields the updater reads of the GitHub API's release.
#[derive(Serialize, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Serialize, Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    /// `sha256:<64 hex digits>`, which GitHub works out for each uploaded file.
    digest: Option<String>,
    browser_download_url: String,
}

impl Release {
    /// The GitHub API's release (`GET /repos/<owner>/<repo>/releases/latest`): its tag `v<x.y.z>` and its asset
    /// [`game_zip_name`] with the asset's size, digest and download address. Refused when one does not read.
    pub fn from_github(text: &str) -> Result<Release, String> {
        let g: GitHubRelease = serde_json::from_str(text).map_err(|e| format!("not a GitHub release: {e}"))?;
        let version = g.tag_name.strip_prefix('v').unwrap_or(&g.tag_name).to_owned();
        if parse_version(&version).is_none() {
            return Err(format!("its tag {} is not v<x.y.z>", g.tag_name));
        }
        let file = game_zip_name(&version);
        let a = g.assets.into_iter().find(|a| a.name == file).ok_or_else(|| format!("it has no {file}"))?;
        let sha256 = a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).unwrap_or("").to_ascii_lowercase();
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{file} has no SHA-256 digest"));
        }
        if !a.browser_download_url.starts_with("https://") && !a.browser_download_url.starts_with("http://") {
            return Err(format!("{file}'s address {} is not a web address", a.browser_download_url));
        }
        Ok(Release { version, file, url: a.browser_download_url, size: a.size, sha256 })
    }

    /// The release in the GitHub API's shape, the fields [`Release::from_github`] reads only: the release check's
    /// and the tests' local feed.
    pub fn to_github(&self) -> String {
        let g = GitHubRelease {
            tag_name: format!("v{}", self.version),
            assets: vec![GitHubAsset {
                name: self.file.clone(),
                size: self.size,
                digest: Some(format!("sha256:{}", self.sha256)),
                browser_download_url: self.url.clone(),
            }],
        };
        serde_json::to_string_pretty(&g).expect("a release always serialises")
    }
}

/// `x.y.z` as three numbers: ASCII digits only, no sign, no leading zero.
pub fn parse_version(v: &str) -> Option<[u32; 3]> {
    let mut it = v.split('.');
    let mut out = [0; 3];
    for o in &mut out {
        let part = it.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        *o = part.parse().ok()?;
    }
    it.next().is_none().then_some(out)
}

/// `a` is a later version than `b`; a version that does not read is never later.
pub fn newer(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// A release's text, as the launcher's patch notes show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseNotes {
    /// `x.y.z`
    pub version: String,
    /// The release's name ("" none).
    pub name: String,
    /// When it came out, ISO 8601 ("" unknown).
    pub published_at: String,
    /// Its text, Markdown.
    pub body: String,
}

/// The releases the patch notes list at most.
pub const NOTES_KEPT: usize = 10;

/// The fields the patch notes read of a release in the GitHub API's list.
#[derive(Deserialize)]
struct GitHubNotes {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The address of the releases' list beside the update feed's latest release: the GitHub API's
/// `.../releases/latest` gives `.../releases`. Another feed (the release check's local one) has no list.
pub fn notes_feed(feed: &str) -> Option<String> {
    feed.strip_suffix("/releases/latest").map(|repo| format!("{repo}/releases?per_page={NOTES_KEPT}"))
}

/// The GitHub API's list of releases (`GET /repos/<owner>/<repo>/releases`) as the patch notes' releases, the
/// latest version first, [`NOTES_KEPT`] at most. A draft, a prerelease and a release whose tag is not `v<x.y.z>`
/// are left out.
pub fn notes_from_github(text: &str) -> Result<Vec<ReleaseNotes>, String> {
    let list: Vec<GitHubNotes> = serde_json::from_str(text).map_err(|e| format!("not a list of releases: {e}"))?;
    let mut out: Vec<([u32; 3], ReleaseNotes)> = list
        .into_iter()
        .filter(|g| !g.draft && !g.prerelease)
        .filter_map(|g| {
            let version = g.tag_name.strip_prefix('v').unwrap_or(&g.tag_name).to_owned();
            let at = parse_version(&version)?;
            let notes = ReleaseNotes {
                version,
                name: g.name.unwrap_or_default(),
                published_at: g.published_at.unwrap_or_default(),
                body: g.body.unwrap_or_default(),
            };
            Some((at, notes))
        })
        .collect();
    out.sort_by_key(|n| std::cmp::Reverse(n.0));
    out.dedup_by(|a, b| a.0 == b.0);
    out.truncate(NOTES_KEPT);
    Ok(out.into_iter().map(|(_, n)| n).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_releases_list_is_read_for_the_notes() {
        assert_eq!(
            notes_feed("https://api.github.com/repos/o/r/releases/latest").as_deref(),
            Some("https://api.github.com/repos/o/r/releases?per_page=10")
        );
        assert_eq!(notes_feed("http://127.0.0.1:8080/"), None);
        let list = r#"[
            {"tag_name":"v0.1.9","name":"Nine","published_at":"2026-02-01T10:00:00Z","body":"- a","draft":false,
             "prerelease":false,"assets":[]},
            {"tag_name":"v0.2.0-rc1","name":"RC","body":"x","prerelease":true},
            {"tag_name":"v0.3.0","name":"Draft","body":"x","draft":true},
            {"tag_name":"nightly","name":"Nightly","body":"x"},
            {"tag_name":"v0.1.10","name":null,"published_at":null,"body":null}
        ]"#;
        let notes = notes_from_github(list).unwrap();
        assert_eq!(notes.iter().map(|n| n.version.as_str()).collect::<Vec<_>>(), ["0.1.10", "0.1.9"]);
        assert_eq!(notes[0].name, "");
        assert_eq!(
            notes[1],
            ReleaseNotes {
                version: "0.1.9".into(),
                name: "Nine".into(),
                published_at: "2026-02-01T10:00:00Z".into(),
                body: "- a".into()
            }
        );
        assert!(notes_from_github(r#"{"message":"Not Found"}"#).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn the_game_folder_is_found_under_the_release_folder() {
        let root = std::env::temp_dir().join(format!("mvs-release-root-{}", std::process::id()));
        let game = root.join(GAME_DIR);
        std::fs::create_dir_all(&game).unwrap();
        // no launcher above: the game folder is its own
        assert_eq!(app_root(&game), game);
        std::fs::write(root.join(LAUNCHER_EXE), b"MZ").unwrap();
        assert_eq!(app_root(&game), root);
        assert_eq!(app_root(&root), root);
        assert_eq!(game_exe(&root), root.join(BUILT_GAME_EXE));
        std::fs::write(game.join(GAME_EXE), b"MZ").unwrap();
        assert_eq!(game_exe(&root), game.join(GAME_EXE));
        std::fs::write(game.join(ONLINE_JSON), b"{}").unwrap();
        assert_eq!(online_json(&root), game.join(ONLINE_JSON));
        assert_eq!(in_game(GAME_EXE), "game/MVS Classic.exe");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_linux_game_folder_is_found_under_the_release_folder() {
        let root = std::env::temp_dir().join(format!("mvs-release-linux-root-{}", std::process::id()));
        let game = root.join(GAME_DIR);
        std::fs::create_dir_all(&game).unwrap();
        // no launcher above: the game folder is its own
        assert_eq!(app_root(&game), game);
        // the Windows launcher's name does not make a Linux release
        std::fs::write(root.join(LAUNCHER_EXE), b"MZ").unwrap();
        assert_eq!(app_root(&game), game);
        std::fs::write(root.join(LINUX_LAUNCHER_EXE), b"\x7fELF").unwrap();
        assert_eq!(app_root(&game), root);
        assert_eq!(app_root(&root), root);
        assert_eq!(game_exe(&root), root.join(BUILT_LINUX_GAME_EXE));
        std::fs::write(game.join(GAME_EXE), b"MZ").unwrap();
        assert_eq!(game_exe(&root), root.join(BUILT_LINUX_GAME_EXE));
        std::fs::write(game.join(LINUX_GAME_EXE), b"\x7fELF").unwrap();
        assert_eq!(game_exe(&root), game.join(LINUX_GAME_EXE));
        std::fs::write(game.join(ONLINE_JSON), b"{}").unwrap();
        assert_eq!(online_json(&root), game.join(ONLINE_JSON));
        assert_eq!(in_game(LINUX_GAME_EXE), "game/MVS Classic");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_linux_launcher_is_found_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("mvs-release-linux-launcher-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // a dev build: Cargo's
        assert_eq!(launcher_exe(&dir), dir.join(BUILT_LINUX_LAUNCHER_EXE));
        std::fs::write(dir.join(LAUNCHER_EXE), b"MZ").unwrap();
        assert_eq!(launcher_exe(&dir), dir.join(BUILT_LINUX_LAUNCHER_EXE));
        std::fs::write(dir.join(LINUX_LAUNCHER_EXE), b"\x7fELF").unwrap();
        assert_eq!(launcher_exe(&dir), dir.join(LINUX_LAUNCHER_EXE));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_linux_programs_are_the_windows_names_without_exe() {
        assert_eq!(LINUX_GAME_EXE, "MVS Classic");
        assert_eq!(LINUX_LAUNCHER_EXE, "MVSC Launcher");
        assert_eq!(BUILT_LINUX_GAME_EXE, "mvs");
        assert_eq!(BUILT_LINUX_LAUNCHER_EXE, "mvs-launcher");
        assert_eq!(PLATFORM_GAME_EXE, LINUX_GAME_EXE);
        assert_eq!(PLATFORM_LAUNCHER_EXE, LINUX_LAUNCHER_EXE);
        assert_eq!(PLATFORM_BUILT_GAME_EXE, BUILT_LINUX_GAME_EXE);
        assert_eq!(PLATFORM_BUILT_LAUNCHER_EXE, BUILT_LINUX_LAUNCHER_EXE);
        assert_eq!(GAME_EXE.strip_suffix(".exe"), Some(LINUX_GAME_EXE));
        assert_eq!(LAUNCHER_EXE.strip_suffix(".exe"), Some(LINUX_LAUNCHER_EXE));
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_programs_are_this_platforms() {
        assert_eq!(PLATFORM_GAME_EXE, GAME_EXE);
        assert_eq!(PLATFORM_LAUNCHER_EXE, LAUNCHER_EXE);
        assert_eq!(PLATFORM_BUILT_GAME_EXE, BUILT_GAME_EXE);
        assert_eq!(PLATFORM_BUILT_LAUNCHER_EXE, BUILT_LAUNCHER_EXE);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.0.1"));
        assert!(!newer("0.1.1", "x"));
        assert_eq!(parse_version("+0.1.0"), None);
        assert_eq!(parse_version("0.01.0"), None);
        assert_eq!(parse_version("10.0.3"), Some([10, 0, 3]));
    }

    #[test]
    fn the_github_release_is_read() {
        let r = Release {
            version: "0.1.1".into(),
            file: game_zip_name("0.1.1"),
            url: "https://github.com/o/r/releases/download/v0.1.1/MVS-Classic-0.1.1.zip".into(),
            size: 12,
            sha256: "ab".repeat(32),
        };
        assert_eq!(Release::from_github(&r.to_github()).unwrap(), r);
        let upper = r.to_github().replace(&"ab".repeat(32), &"AB".repeat(32));
        assert_eq!(Release::from_github(&upper).unwrap(), r);
        assert!(Release::from_github(&r.to_github().replace("v0.1.1", "v0.1")).is_err());
        assert!(Release::from_github(&r.to_github().replace("sha256:", "md5:")).is_err());
        assert!(Release::from_github(&r.to_github().replace("\"MVS-Classic-0.1.1.zip\"", "\"x.zip\"")).is_err());
        assert!(Release::from_github(&r.to_github().replace("https://", "file://")).is_err());
        // (GitHub's own answer holds many more fields)
        let full = r#"{"url":"u","tag_name":"v0.2.0","name":"n","body":"b","assets":[{"name":"SHA256SUMS","size":1,
            "digest":null,"browser_download_url":"https://x/S"},{"name":"MVS-Classic-0.2.0.zip","size":5,"digest":
            "sha256:00000000000000000000000000000000000000000000000000000000000000ff","browser_download_url":"https://x/z",
            "uploader":{"login":"o"}}]}"#;
        assert_eq!(Release::from_github(full).unwrap().size, 5);
    }
}
