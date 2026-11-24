//! Where the player's beta may be, in order: the folder setup used last, the Steam root
//! (HKCU\Software\Valve\Steam SteamPath, else C:\Program Files (x86)\Steam) and every library of its
//! steamapps\libraryfolders.vdf; under each library steamapps\content\app_1818750\depot_* (where Steam's
//! download_depot puts the beta). Steam's own install of the game (steamapps\common\MultiVersus, the 2025 relaunch)
//! is not offered; picked by hand, the check names it and it is refused. Each folder is named by
//! mvs_pak::setup::source_check; one paks folder found twice is listed once. Runs in the import worker.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use mvs_pak::setup::{BuildList, SourceKind};
use mvs_ui::launcher::import_view::{SourceCandidate, SourceFrom};

use super::serve::source_info;

/// The beta's Steam app id (the open beta's depot is app_1818750\depot_1818751).
const APP_ID: &str = "1818750";
#[cfg(windows)]
const DEFAULT_STEAM: &str = r"C:\Program Files (x86)\Steam";

/// A KeyValues value: a string or a block.
#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Block(IndexMap<String, Vdf>),
}

/// Valve's text KeyValues (libraryfolders.vdf): quoted keys, quoted values or `{ ... }` blocks, `\\` and `\"`
/// escapes, `//` comments. The top level's pairs; an error naming where the text does not read.
pub fn parse_vdf(text: &str) -> Result<IndexMap<String, Vdf>, String> {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    block(&b, &mut i, true)
}

fn skip_ws(b: &[char], i: &mut usize) {
    loop {
        while *i < b.len() && b[*i].is_whitespace() {
            *i += 1;
        }
        if *i + 1 < b.len() && b[*i] == '/' && b[*i + 1] == '/' {
            while *i < b.len() && b[*i] != '\n' {
                *i += 1;
            }
        } else {
            return;
        }
    }
}

fn quoted(b: &[char], i: &mut usize) -> Result<String, String> {
    if b.get(*i) != Some(&'"') {
        return Err(format!("vdf: a quoted string expected at {i}"));
    }
    let mut out = String::new();
    *i += 1;
    while *i < b.len() {
        let c = b[*i];
        if c == '"' {
            *i += 1;
            return Ok(out);
        }
        if c == '\\' && *i + 1 < b.len() {
            *i += 1;
            out.push(match b[*i] {
                'n' => '\n',
                't' => '\t',
                n => n,
            });
        } else {
            out.push(c);
        }
        *i += 1;
    }
    Err("vdf: a string runs to the end".into())
}

fn block(b: &[char], i: &mut usize, top: bool) -> Result<IndexMap<String, Vdf>, String> {
    let mut o = IndexMap::new();
    loop {
        skip_ws(b, i);
        if *i >= b.len() {
            return if top { Ok(o) } else { Err("vdf: a block runs to the end".into()) };
        }
        if b[*i] == '}' {
            if top {
                return Err(format!("vdf: a stray }} at {i}"));
            }
            *i += 1;
            return Ok(o);
        }
        let k = quoted(b, i)?;
        skip_ws(b, i);
        if b.get(*i) == Some(&'{') {
            *i += 1;
            o.insert(k, Vdf::Block(block(b, i, false)?));
        } else {
            o.insert(k, Vdf::Str(quoted(b, i)?));
        }
    }
}

/// The library folders a libraryfolders.vdf lists (each entry's "path"; the old format's "1" "D:\\SteamLibrary"),
/// in its order.
pub fn library_paths(vdf: &IndexMap<String, Vdf>) -> Vec<PathBuf> {
    let Some(Vdf::Block(top)) = vdf.get("libraryfolders").or_else(|| vdf.get("LibraryFolders")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in top.values() {
        match v {
            Vdf::Str(s) => {
                let c = s.as_bytes();
                let drive = c.len() >= 2 && c[0].is_ascii_alphabetic() && c[1] == b':';
                if drive || s.starts_with(r"\\") {
                    out.push(PathBuf::from(s));
                }
            }
            Vdf::Block(b) => {
                if let Some(Vdf::Str(p)) = b.get("path") {
                    out.push(PathBuf::from(p));
                }
            }
        }
    }
    out
}

/// A path's real path (links and junctions followed, Steam's lower case made the disk's), without the `\\?\`
/// prefix; else the path as given.
pub(crate) fn real_path(p: &Path) -> PathBuf {
    let Ok(r) = std::fs::canonicalize(p) else { return p.to_path_buf() };
    let s = r.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => r,
    }
}

/// The Steam root the registry names (HKCU\Software\Valve\Steam SteamPath), or None (no Steam).
#[cfg(windows)]
pub fn steam_root_from_registry() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let mut buf = [0u16; 1024];
    let mut size = u32::try_from(std::mem::size_of_val(&buf)).unwrap_or(0);
    // SAFETY: a buffer of `size` bytes the call writes a terminated string into, its size given and written back.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Valve\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..n]);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let dir = std::path::absolute(text).unwrap_or_else(|_| PathBuf::from(text));
    Some(real_path(&dir))
}

/// The Steam roots of the home folder `home` that are there, each by its real path and once (`~/.steam/steam` is
/// often a link to the first): the native install's `~/.local/share/Steam`, `~/.steam/steam`, and the Flatpak's
/// `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`.
#[cfg(not(windows))]
fn steam_roots(home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for rel in [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
        let dir = home.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let r = real_path(&dir);
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// The sub folders of `dir` whose names `keep` takes, by name.
fn subdirs(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> =
        rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| keep(n)).collect();
    // (the default sort: by UTF-16 code units; the names are ASCII)
    names.sort();
    names.into_iter().map(|n| dir.join(n)).filter(|p| p.is_dir()).collect()
}

/// `depot_<digits>`, any case.
fn is_depot(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    low.strip_prefix("depot_").is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
}

/// A path made absolute, lower case: one key per folder however it was written.
fn path_key(p: &Path) -> String {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase()
}

/// The folders to name, with where each came from: `last`, then the depots under the Steam root and its libraries.
pub fn candidate_folders(last: Option<&Path>, steam_root: &Path) -> Vec<(PathBuf, SourceFrom)> {
    let mut out = Vec::new();
    if let Some(l) = last {
        out.push((l.to_path_buf(), SourceFrom::Last));
    }
    let libs = std::fs::read_to_string(steam_root.join("steamapps").join("libraryfolders.vdf"))
        .ok()
        .and_then(|t| parse_vdf(&t).ok())
        .map(|v| library_paths(&v))
        .unwrap_or_default();
    let mut seen: Vec<String> = Vec::new();
    for (k, lib) in std::iter::once(steam_root.to_path_buf()).chain(libs).enumerate() {
        let key = path_key(&lib);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let from = if k == 0 { SourceFrom::Steam } else { SourceFrom::Library };
        let app = lib.join("steamapps").join("content").join(format!("app_{APP_ID}"));
        out.extend(subdirs(&app, is_depot).into_iter().map(|d| (d, from)));
        // (Steam on Linux downloads a depot under the library's steamrt64 too)
        #[cfg(not(windows))]
        {
            let rt = lib.join("steamrt64").join("steamapps").join("content").join(format!("app_{APP_ID}"));
            out.extend(subdirs(&rt, is_depot).into_iter().map(|d| (d, from)));
        }
    }
    out
}

/// Every candidate folder that holds game files, checked, each paks folder once (one folder through a junction or
/// in another case is one). `steam_root`: None for the registry's, else C:\Program Files (x86)\Steam.
#[cfg(windows)]
pub fn detect_sources(last: Option<&Path>, steam_root: Option<&Path>, build: &BuildList) -> Vec<SourceCandidate> {
    let root = steam_root.map(Path::to_path_buf).or_else(steam_root_from_registry).unwrap_or(DEFAULT_STEAM.into());
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (dir, from) in candidate_folders(last, &root) {
        let s = mvs_pak::setup::source_check(&dir, build);
        if s.kind == SourceKind::NotGame {
            continue;
        }
        let key = real_path(&s.paks).to_string_lossy().to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(SourceCandidate { source: source_info(&s), from });
    }
    out
}

/// Every candidate folder that holds game files, checked, each paks folder once (one folder through a link or in
/// another case is one). `steam_root`: None for every Steam root of the home folder ([`steam_roots`]).
#[cfg(not(windows))]
pub fn detect_sources(last: Option<&Path>, steam_root: Option<&Path>, build: &BuildList) -> Vec<SourceCandidate> {
    let roots = match steam_root {
        Some(r) => vec![r.to_path_buf()],
        None => std::env::var_os("HOME").map(|h| steam_roots(Path::new(&h))).unwrap_or_default(),
    };
    let mut folders: Vec<(PathBuf, SourceFrom)> =
        last.map(|l| (l.to_path_buf(), SourceFrom::Last)).into_iter().collect();
    for root in &roots {
        folders.extend(candidate_folders(None, root));
    }
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (dir, from) in folders {
        let s = mvs_pak::setup::source_check(&dir, build);
        if s.kind == SourceKind::NotGame {
            continue;
        }
        let key = real_path(&s.paks).to_string_lossy().to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(SourceCandidate { source: source_info(&s), from });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_reads_both_formats() {
        let new = r#"
"libraryfolders"
{
	// a comment
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "1818750" "123" }
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        let v = parse_vdf(new).unwrap();
        assert_eq!(
            library_paths(&v),
            [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
        let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\"1\"\n\t\"1\"\t\"E:\\\\Games\"\n}\n";
        assert_eq!(library_paths(&parse_vdf(old).unwrap()), [PathBuf::from(r"E:\Games")]);
        assert!(parse_vdf("\"a\" {").is_err());
        assert!(parse_vdf("}").is_err());
        assert!(parse_vdf("\"a\" \"b").is_err());
    }

    /// A folder of the test's own under the temp folder, empty.
    #[cfg(not(windows))]
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mvs-steam-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(not(windows))]
    #[test]
    fn finds_the_depot_under_steamrt64() {
        let d = scratch("rt64");
        let depot = d.join("steamrt64/steamapps/content/app_1818750/depot_1818751");
        std::fs::create_dir_all(depot.join("MultiVersus/Content/Paks")).unwrap();
        let got = candidate_folders(None, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(got, [(depot, SourceFrom::Steam)]);
    }

    #[cfg(not(windows))]
    #[test]
    fn reads_a_library_with_a_space_and_an_accent() {
        let d = scratch("library");
        let root = d.join("Steam");
        let lib = d.join("My Games é").join("Steam");
        let depot = lib.join("steamapps/content/app_1818750/depot_1818751");
        std::fs::create_dir_all(depot.join("MultiVersus/Content/Paks")).unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        let vdf = format!("\"libraryfolders\"\n{{\n\t\"1\" {{ \"path\" \"{}\" }}\n}}\n", lib.display());
        std::fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
        let got = candidate_folders(None, &root);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(got, [(depot, SourceFrom::Library)]);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_roots_that_are_there_once_each() {
        let home = scratch("roots");
        assert!(steam_roots(&home).is_empty());
        let first = home.join(".local/share/Steam");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(home.join(".steam")).unwrap();
        std::os::unix::fs::symlink(&first, home.join(".steam/steam")).unwrap();
        let flatpak = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        std::fs::create_dir_all(&flatpak).unwrap();
        let got = steam_roots(&home);
        let want = [real_path(&first), real_path(&flatpak)];
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(got, want);
    }

    #[test]
    fn depot_names() {
        assert!(is_depot("depot_1818751") && is_depot("DEPOT_1"));
        assert!(!is_depot("depot_") && !is_depot("depot_x1") && !is_depot("depots_1"));
    }
}
