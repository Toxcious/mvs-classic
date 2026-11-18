//! Setup: the check of the player's copy of the beta where they keep it (nothing is copied), and the key read from
//! their own exe. It also carries the build list (`build.json`: per pak its name, size and SHA-256; the build files
//! beside the paks with the hashes of the tables cut out of them). The list holds nothing from inside the paks.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use sha2::{Digest, Sha256};

use crate::{AesKey, PakError, PakSet, Result};

/// One pak of the build list.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BuildPak {
    pub name: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
}

/// A table an importer cuts out of a build file, checked by its hash ([`check_table`]).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BuildTable {
    /// What the table is: `codebooks` (exe), `erPatterns` (AkRoomVerb.dll).
    pub name: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the table's bytes as the importer cuts them.
    pub sha256: String,
}

/// A build file the import reads beside the paks: the exe (its sound engine's Vorbis codebooks), AkRoomVerb.dll (the
/// RoomVerb early reflection patterns). Not hashed whole (the beta's patcher changes one byte of the exe): each table
/// cut out of it is.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct BuildFile {
    /// Its path in the install (the folder holding `MultiVersus/`), forward slashes.
    pub path: String,
    pub tables: Vec<BuildTable>,
}

/// The build list this code reads (`build.json`).
#[derive(Clone, Debug)]
pub struct BuildList {
    /// First 16 hex digits of the SHA-256 of the lines "<name> <size> <sha256>\n", in list order.
    pub id: String,
    pub paks: Vec<BuildPak>,
    /// The build files the import reads beside the paks (empty in a test's list). Not in `id`: what the import cuts
    /// out of them is checked by table.
    pub files: Vec<BuildFile>,
}

#[derive(serde::Deserialize)]
struct BuildJson {
    id: String,
    paks: Vec<BuildPak>,
    #[serde(default)]
    files: Vec<BuildFile>,
}

/// What a folder the player picked is (named without hashing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Beta,
    OtherBuild,
    Partial,
    Relaunch,
    NotGame,
}

/// Progress of a hash pass: bytes done of all bytes, and the file being read.
pub type Progress<'a> = &'a (dyn Fn(u64, u64, &str) + Sync);

const SHIPPING_EXE: &str = "MultiVersus-Win64-Shipping.exe";
/// A hash pass reads a file in pieces of this many bytes.
const CHUNK: usize = 8 << 20;

fn setup_err(why: impl Into<String>) -> PakError {
    PakError::Setup(why.into())
}

/// The id of a pak list: the first 16 hex digits of the SHA-256 of its lines "<name> <size> <sha256>\n".
pub fn list_id(paks: &[BuildPak]) -> String {
    let mut h = Sha256::new();
    for p in paks {
        h.update(format!("{} {} {}\n", p.name, p.size, p.sha256).as_bytes());
    }
    hex(&h.finalize())[..16].to_string()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn parsed_build() -> &'static std::result::Result<BuildList, String> {
    static BUILD: OnceLock<std::result::Result<BuildList, String>> = OnceLock::new();
    BUILD.get_or_init(|| {
        let b: BuildJson =
            serde_json::from_str(include_str!("../build.json")).map_err(|e| format!("build list: {e}"))?;
        Ok(BuildList { id: b.id, paks: b.paks, files: b.files })
    })
}

/// The build list compiled into the program, its id checked against its paks.
pub fn build_list() -> Result<BuildList> {
    let b = parsed_build().clone().map_err(setup_err)?;
    check_list(&b)?;
    Ok(b)
}

/// Fails unless `list`'s id is the id of its paks.
pub fn check_list(list: &BuildList) -> Result<()> {
    if list.id != list_id(&list.paks) {
        return Err(setup_err(format!("build list: id {} does not match its paks", list.id)));
    }
    Ok(())
}

/// The build id (the build list's): which build of the beta a store was imported from.
pub fn build_id() -> Result<String> {
    let id = build_list()?.id;
    let plain = !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c));
    if !plain || id == "." || id == ".." {
        return Err(setup_err("build list has no usable id"));
    }
    Ok(id)
}

/// The build list's pak sizes by lower-cased name ([`PakSet::open`] checks a listed pak's size).
pub(crate) fn build_sizes() -> HashMap<String, u64> {
    match parsed_build() {
        Ok(b) => b.paks.iter().map(|p| (p.name.to_lowercase(), p.size)).collect(),
        Err(_) => HashMap::new(),
    }
}

/// A path made absolute with its `.` and `..` resolved (as node's path.resolve), else as given.
fn resolve(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

/// The pak chunk number of a base pak name (`pakchunk<n>-WindowsNoEditor.pak`, any case).
fn chunk_of(name: &str) -> Option<u64> {
    let low = name.to_ascii_lowercase();
    let rest = low.strip_prefix("pakchunk")?;
    let d = rest.bytes().take_while(u8::is_ascii_digit).count();
    if d == 0 || &rest[d..] != "-windowsnoeditor.pak" {
        return None;
    }
    // a number past u64 is as large as JavaScript's Number makes it: last, with the names that are no chunk
    Some(rest[..d].parse().unwrap_or(u64::MAX))
}

/// The .pak files of a folder, by chunk number; other names come last, by name.
pub fn paks_in(dir: &Path) -> Result<Vec<String>> {
    let rd = std::fs::read_dir(dir).map_err(|e| PakError::Io(dir.to_path_buf(), e))?;
    let mut names = Vec::new();
    for ent in rd {
        let name = ent.map_err(|e| PakError::Io(dir.to_path_buf(), e))?.file_name().to_string_lossy().into_owned();
        if name.to_lowercase().ends_with(".pak") {
            names.push(name);
        }
    }
    names.sort_by(|a, b| {
        let (ca, cb) = (chunk_of(a).unwrap_or(u64::MAX), chunk_of(b).unwrap_or(u64::MAX));
        ca.cmp(&cb).then_with(|| crate::cmp_utf16(a, b))
    });
    Ok(names)
}

/// A folder's .pak files against a build list, by name and size (no hashing).
#[derive(Clone, Debug, Default)]
pub struct PakNames {
    /// The .pak names in the folder.
    pub names: Vec<String>,
    /// The list's paks the folder lacks.
    pub missing: Vec<String>,
    /// The folder's paks the list does not have (another version's chunk, a patch or mod pak).
    pub extra: Vec<String>,
    /// The list's paks the folder has at another size: (name, size here, size listed).
    pub wrong_size: Vec<(String, u64, u64)>,
}

/// Compares the .pak files of `dir` with `build` by name (without regard to case) and size.
pub fn compare_paks(dir: &Path, build: &BuildList) -> Result<PakNames> {
    let names = paks_in(dir)?;
    let want: HashMap<String, &BuildPak> = build.paks.iter().map(|p| (p.name.to_lowercase(), p)).collect();
    let have: HashMap<String, &String> = names.iter().map(|n| (n.to_lowercase(), n)).collect();
    let extra = names.iter().filter(|n| !want.contains_key(&n.to_lowercase())).cloned().collect();
    let missing =
        build.paks.iter().filter(|p| !have.contains_key(&p.name.to_lowercase())).map(|p| p.name.clone()).collect();
    let mut wrong_size = Vec::new();
    for p in &build.paks {
        let Some(n) = have.get(&p.name.to_lowercase()) else { continue };
        let f = dir.join(n);
        let size = std::fs::metadata(&f).map_err(|e| PakError::Io(f.clone(), e))?.len();
        if size != p.size {
            wrong_size.push((p.name.clone(), size, p.size));
        }
    }
    Ok(PakNames { names, missing, extra, wrong_size })
}

/// Whether `cancel` is set.
fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

/// Streams `file` through SHA-256 (one thread reads ahead while this one hashes); returns the hex digest, or None
/// when `cancel` was seen between two pieces. `on_bytes` is called per piece hashed.
fn hash_file(
    file: &Path,
    on_bytes: &(dyn Fn(u64) + Sync),
    cancel: Option<&AtomicBool>,
) -> std::io::Result<Option<String>> {
    let mut f = File::open(file)?;
    let (tx, rx) = std::sync::mpsc::sync_channel::<std::io::Result<Vec<u8>>>(2);
    let (back_tx, back_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::scope(|s| {
        s.spawn(move || {
            loop {
                let mut buf = back_rx.try_recv().unwrap_or_else(|_| vec![0u8; CHUNK]);
                buf.resize(CHUNK, 0);
                let mut n = 0;
                while n < CHUNK {
                    match f.read(&mut buf[n..]) {
                        Ok(0) => break,
                        Ok(k) => n += k,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
                buf.truncate(n);
                let done = n < CHUNK;
                if tx.send(Ok(buf)).is_err() || done {
                    return;
                }
            }
        });
        let mut h = Sha256::new();
        for piece in rx {
            // (the reader ends at its next send once `rx` is dropped)
            if cancelled(cancel) {
                return Ok(None);
            }
            let piece = piece?;
            h.update(&piece);
            on_bytes(piece.len() as u64);
            let _ = back_tx.send(piece);
        }
        Ok(Some(hex(&h.finalize())))
    })
}

/// Hashes `files` (path, display name, size) on all cores, the largest first; their hex digests in their order.
/// `cancel` is checked per piece of a file: once it is set the pass ends and [`PakError::Cancelled`] is returned.
fn hash_files(
    files: &[(PathBuf, String, u64)],
    progress: Option<Progress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<std::io::Result<String>>> {
    let total: u64 = files.iter().map(|f| f.2).sum();
    let done = AtomicU64::new(0);
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| files[b].2.cmp(&files[a].2));
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(files.len().max(1));
    let results: Vec<std::sync::Mutex<Option<std::io::Result<Option<String>>>>> =
        files.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = order.get(k) else { return };
                    if cancelled(cancel) {
                        return;
                    }
                    let (path, name, _) = &files[i];
                    let r = hash_file(
                        path,
                        &|n| {
                            let d = done.fetch_add(n, Ordering::Relaxed) + n;
                            if let Some(p) = progress {
                                p(d, total, name);
                            }
                        },
                        cancel,
                    );
                    *results[i].lock().unwrap() = Some(r);
                }
            });
        }
    });
    if cancelled(cancel) {
        return Err(PakError::Cancelled);
    }
    // (not cancelled: every file was hashed whole)
    Ok(results.into_iter().map(|m| m.into_inner().unwrap().unwrap().map(|h| h.unwrap_or_default())).collect())
}

/// Hashes the paks of `dir` into a build list (what `build.json` holds); `cancel` as
/// [`setup_check`]'s.
pub fn hash_pak_dir(dir: &Path, progress: Option<Progress<'_>>, cancel: Option<&AtomicBool>) -> Result<BuildList> {
    let names = paks_in(dir)?;
    let mut files = Vec::with_capacity(names.len());
    for n in &names {
        let f = dir.join(n);
        let size = std::fs::metadata(&f).map_err(|e| PakError::Io(f.clone(), e))?.len();
        files.push((f, n.clone(), size));
    }
    let mut paks = Vec::with_capacity(files.len());
    for ((f, name, size), sha) in files.iter().zip(hash_files(&files, progress, cancel)?) {
        let sha256 = sha.map_err(|e| PakError::Io(f.clone(), e))?;
        paks.push(BuildPak { name: name.clone(), size: *size, sha256 });
    }
    Ok(BuildList { id: list_id(&paks), paks, files: Vec::new() })
}

/// The install folder (the one holding `MultiVersus/`) of a paks folder `<install>/MultiVersus/Content/Paks`.
pub fn install_root(paks_dir: &Path) -> PathBuf {
    resolve(&paks_dir.join("..").join("..").join(".."))
}

/// A build file's place in the install the paks of `paks_dir` lie in.
fn build_file_path(paks_dir: &Path, path: &str) -> PathBuf {
    let mut f = install_root(paks_dir);
    f.extend(path.split('/'));
    f
}

/// The build's files (the list's `files`: the exe, AkRoomVerb.dll) missing from the install the paks of `paks_dir`
/// lie in, each as its path in the install; checked by stat only.
pub fn missing_build_files(paks_dir: &Path, build: &BuildList) -> Vec<String> {
    build.files.iter().map(|f| f.path.clone()).filter(|p| !build_file_path(paks_dir, p).is_file()).collect()
}

/// The bytes of a build file, from the install the paks of `paks` lie in; fails naming the file when it is not there.
pub fn read_build_file(paks: &PakSet, path: &str) -> Result<Vec<u8>> {
    let Some(dir) = paks.pak_files().first().and_then(|p| p.parent()) else {
        return Err(setup_err(format!("{path}: these paks name no folder, so no build file beside them can be read")));
    };
    let file = build_file_path(dir, path);
    std::fs::read(&file)
        .map_err(|e| setup_err(format!("the build file {path} is not beside the paks ({}): {e}", file.display())))
}

/// Fails unless `bytes` is the table `name` of the build file `path` as `files` (the build list's) lists it.
pub fn check_table(files: &[BuildFile], path: &str, name: &str, bytes: &[u8]) -> Result<()> {
    let Some(t) = files.iter().find(|f| f.path == path).and_then(|f| f.tables.iter().find(|t| t.name == name)) else {
        return Err(setup_err(format!("the build list lists no table {name} of {path}")));
    };
    let sha = hex(&Sha256::digest(bytes));
    if bytes.len() as u64 != t.size || sha != t.sha256 {
        return Err(setup_err(format!(
            "the {name} table of {path} is not the open beta's ({} bytes, SHA-256 {sha}; want {}, {})",
            bytes.len(),
            t.size,
            t.sha256
        )));
    }
    Ok(())
}

/// Whether a folder holds pak or UE5 container files.
fn holds_paks(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.ends_with(".pak") || n.ends_with(".utoc") || n.ends_with(".ucas")
        })
    })
}

/// A path's real path (links and junctions followed), without the `\\?\` prefix Windows gives it.
fn real_path(p: &Path) -> PathBuf {
    let Ok(r) = std::fs::canonicalize(p) else { return p.to_path_buf() };
    let s = r.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => r,
    }
}

/// The pak folder of the install the player picked (the folder itself, the install root, or the exe's folder).
///
/// The folder itself, `Paks` below it (Content), `Content/Paks` (MultiVersus) or `MultiVersus/Content/Paks` (the
/// install root); empty when none of them holds paks. The path is kept as given: the exe and the build files are
/// found from it. Only when no exe lies beside it but one lies beside its real path is the real path taken.
pub fn paks_folder_of(dir: &Path) -> PathBuf {
    let d = resolve(dir);
    for p in
        [d.clone(), d.join("Paks"), d.join("Content").join("Paks"), d.join("MultiVersus").join("Content").join("Paks")]
    {
        if !is_dir(&p) || !holds_paks(&p) {
            continue;
        }
        if exe_beside_paks(&p).exists() {
            return p;
        }
        let real = real_path(&p);
        return if exe_beside_paks(&real).exists() { real } else { p };
    }
    PathBuf::new()
}

/// What a folder is, with the notes the launcher shows.
#[derive(Clone, Debug)]
pub struct SourceCheck {
    pub dir: PathBuf,
    /// The paks folder ([`paks_folder_of`]); empty when there is none.
    pub paks: PathBuf,
    pub kind: SourceKind,
    pub build_id: Option<String>,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub key_ok: bool,
    pub note: String,
}

fn list_names(names: &[String]) -> String {
    const MOST: usize = 3;
    if names.len() <= MOST {
        names.join(", ")
    } else {
        format!("{} and {} more", names[..MOST].join(", "), names.len() - MOST)
    }
}

/// Whether the key in the exe beside `paks` opens the index of the smallest of `files`.
fn key_opens(paks: &Path, files: &[PathBuf]) -> std::result::Result<(), String> {
    let exe = exe_beside_paks(paks);
    if !exe.exists() {
        return Err(
            "the game's exe (MultiVersus\\Binaries\\Win64\\MultiVersus-Win64-Shipping.exe) is not beside the paks"
                .into(),
        );
    }
    let key = key_from_exe(&exe).map_err(|e| format!("the key could not be read from the exe ({e})"))?;
    let smallest = files
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok().map(|m| (m.len(), f)))
        .min_by_key(|x| x.0)
        .ok_or("no pak to open")?;
    PakSet::open(std::slice::from_ref(smallest.1), &key)
        .map(|_| ())
        .map_err(|e| format!("the exe's key does not open the paks ({e})"))
}

/// Names the folder `dir`: what it holds, by names and sizes, and whether the key in the exe beside its paks opens
/// one index.
pub fn source_check(dir: &Path, build: &BuildList) -> SourceCheck {
    let paks = paks_folder_of(dir);
    let mut out = SourceCheck {
        dir: dir.to_path_buf(),
        paks: paks.clone(),
        kind: SourceKind::NotGame,
        build_id: None,
        missing: Vec::new(),
        extra: Vec::new(),
        key_ok: false,
        note: String::new(),
    };
    if paks.as_os_str().is_empty() {
        out.note = "No game files here: choose the folder that holds MultiVersus (or its Content\\Paks folder).".into();
        return out;
    }
    let names: Vec<String> = match std::fs::read_dir(&paks) {
        Ok(rd) => rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => {
            out.note = format!("This folder cannot be read ({}).", paks.display());
            return out;
        }
    };
    if names.iter().any(|n| {
        let n = n.to_lowercase();
        n.ends_with(".utoc") || n.ends_with(".ucas")
    }) {
        out.kind = SourceKind::Relaunch;
        out.note = "MultiVersus 2025 (the relaunch): not supported. MVS Classic needs the 2022 open beta.".into();
        return out;
    }
    let cmp = match compare_paks(&paks, build) {
        Ok(c) => c,
        Err(_) => {
            out.note = format!("This folder cannot be read ({}).", paks.display());
            return out;
        }
    };
    out.missing = cmp.missing.clone();
    out.extra = cmp.extra.clone();
    if cmp.names.is_empty() {
        out.note = "No .pak files here.".into();
        return out;
    }
    let be = |n: usize| if n > 1 { "are" } else { "is" };
    if !cmp.extra.is_empty() {
        out.kind = SourceKind::OtherBuild;
        out.note = format!(
            "Another build of MultiVersus: {} {} not part of the open beta.",
            list_names(&cmp.extra),
            be(cmp.extra.len())
        );
        return out;
    }
    if !cmp.missing.is_empty() {
        out.kind = SourceKind::Partial;
        out.note = format!(
            "Partial: {} of {} paks missing ({}).",
            cmp.missing.len(),
            build.paks.len(),
            list_names(&cmp.missing)
        );
        return out;
    }
    if !cmp.wrong_size.is_empty() {
        out.kind = SourceKind::OtherBuild;
        let names: Vec<String> = cmp.wrong_size.iter().map(|w| w.0.clone()).collect();
        out.note = format!(
            "Another build of MultiVersus, or damaged files: {} {} in size from the open beta's.",
            list_names(&names),
            if names.len() > 1 { "differ" } else { "differs" }
        );
        return out;
    }
    // (the exe: key_opens names it)
    let gone: Vec<String> =
        missing_build_files(&paks, build).into_iter().filter(|p| !p.to_lowercase().ends_with(".exe")).collect();
    if !gone.is_empty() {
        out.kind = SourceKind::Partial;
        out.note = format!("Partial: {} {} missing from the beta's folder.", list_names(&gone), be(gone.len()));
        out.missing = gone;
        return out;
    }
    let files: Vec<PathBuf> = build.paks.iter().map(|p| paks.join(&p.name)).collect();
    let key = key_opens(&paks, &files);
    out.key_ok = key.is_ok();
    out.build_id = Some(build.id.clone());
    out.kind = SourceKind::Beta;
    out.note = match key {
        Ok(()) => format!("MultiVersus open beta, build {}", build.id),
        Err(why) => format!("MultiVersus open beta, build {}, but {why}.", build.id),
    };
    out
}

/// What `dir` holds, by names and sizes alone.
pub fn check_source(dir: &Path, build: &BuildList) -> Result<SourceKind> {
    Ok(source_check(dir, build).kind)
}

/// The shipping exe beside the paks (`<root>/MultiVersus/Content/Paks` -> `<root>/MultiVersus/Binaries/Win64`).
pub fn exe_beside_paks(paks_dir: &Path) -> PathBuf {
    resolve(&paks_dir.join("..").join("..").join("Binaries").join("Win64").join(SHIPPING_EXE))
}

// The pak key, read from the player's own shipping exe. The exe never holds the key whole: its key callback stores 32
// dwords whose low bytes, XORed with every fourth byte of a 128-byte mask in .data that the engine's masked key setup
// reads, give the 32 key bytes. Both places are found by signature, with no address of this build, and the key is only
// kept once it opens a pak index (setup's key check). This is the one place that knows where the key comes from.

/// A byte signature: hex bytes, `??` for any byte.
fn sig(s: &str) -> Vec<Option<u8>> {
    s.split(' ').map(|b| if b == "??" { None } else { Some(u8::from_str_radix(b, 16).unwrap()) }).collect()
}

/// The key callback's prologue, stack cookie and `lea rdx,[rbp+0x46]`; its dword stores follow.
const CALLBACK: &str =
    "40 55 48 8D 6C 24 A9 48 81 EC ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 47 48 8D 55 46";
/// The masked rijndaelSetupEncrypt's head; the movsx of the mask is at +14.
const KEY_SETUP: &str = "40 53 55 56 57 41 57 44 0F B6 1A 4C 8B CA 0F BE 05 ?? ?? ?? ?? 4C 8B F9 44 33 D8";

fn exekey(why: impl std::fmt::Display) -> PakError {
    setup_err(format!("exekey: {why}"))
}

/// The one offset of `s` in `b`; fails when there is none or more than one.
fn find_once(b: &[u8], s: &str, what: &str) -> Result<usize> {
    let s = sig(s);
    let first = s[0].unwrap();
    let mut hit = None;
    let mut i = 0;
    while let Some(k) = b[i..].iter().position(|&c| c == first) {
        let at = i + k;
        i = at + 1;
        if at + s.len() > b.len() {
            break;
        }
        if s.iter().zip(&b[at..]).all(|(w, &c)| w.is_none_or(|w| w == c)) {
            if hit.is_some() {
                return Err(exekey(format!("the {what} signature matches more than once")));
            }
            hit = Some(at);
        }
    }
    hit.ok_or_else(|| exekey(format!("the {what} signature is not in this exe (another build?)")))
}

struct Section {
    rva: u32,
    vsize: u32,
    raw: u32,
    raw_size: u32,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| exekey("the exe is cut short"))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| exekey("the exe is cut short"))
}

fn sections(b: &[u8]) -> Result<Vec<Section>> {
    if u16_at(b, 0)? != 0x5a4d {
        return Err(exekey("not an exe (no MZ header)"));
    }
    let pe = u32_at(b, 0x3c)? as usize;
    if u32_at(b, pe)? != 0x4550 {
        return Err(exekey("not a PE file"));
    }
    let count = u16_at(b, pe + 6)?;
    let opt_size = u16_at(b, pe + 20)? as usize;
    let mut out = Vec::new();
    let mut o = pe + 24 + opt_size;
    for _ in 0..count {
        out.push(Section {
            vsize: u32_at(b, o + 8)?,
            rva: u32_at(b, o + 12)?,
            raw_size: u32_at(b, o + 16)?,
            raw: u32_at(b, o + 20)?,
        });
        o += 40;
    }
    Ok(out)
}

fn off_to_rva(secs: &[Section], off: u64) -> Result<u64> {
    let s = secs
        .iter()
        .find(|x| off >= u64::from(x.raw) && off < u64::from(x.raw) + u64::from(x.raw_size))
        .ok_or_else(|| exekey(format!("file offset 0x{off:x} is in no section")))?;
    Ok(off - u64::from(s.raw) + u64::from(s.rva))
}

fn rva_to_off(secs: &[Section], rva: u64, len: u64) -> Result<usize> {
    let s = secs
        .iter()
        .find(|x| rva >= u64::from(x.rva) && rva + len <= u64::from(x.rva) + u64::from(x.vsize.min(x.raw_size)))
        .ok_or_else(|| exekey(format!("address 0x{rva:x} is not backed by the file")))?;
    Ok((rva - u64::from(s.rva) + u64::from(s.raw)) as usize)
}

/// The 32 low bytes of the key callback's dword stores, by stack offset.
fn callback_bytes(b: &[u8]) -> Result<[u8; 32]> {
    let mut p = find_once(b, CALLBACK, "key callback")? + sig(CALLBACK).len();
    let at = |k: usize| b.get(k).copied().unwrap_or(0);
    let mut stores: Vec<(i8, u8)> = Vec::new();
    loop {
        if at(p) == 0xc7 && at(p + 1) == 0x45 {
            stores.push((at(p + 2) as i8, at(p + 3)));
            p += 7;
        } else if at(p) == 0x48 && at(p + 1) == 0x8d && at(p + 2) == 0x41 && at(p + 3) == 0x7f {
            p += 4;
        } else if at(p) == 0x48 && at(p + 1) == 0x3b && at(p + 2) == 0xca {
            break;
        } else {
            return Err(exekey(format!("unexpected instruction in the key callback after {} stores", stores.len())));
        }
    }
    if stores.len() != 32 {
        return Err(exekey(format!("the key callback has {} stores, not 32", stores.len())));
    }
    stores.sort_by_key(|s| s.0);
    for (i, s) in stores.iter().enumerate() {
        if i32::from(s.0) != i32::from(stores[0].0) + 4 * i as i32 {
            return Err(exekey("the key callback stores are not contiguous"));
        }
    }
    let mut out = [0u8; 32];
    for (o, s) in out.iter_mut().zip(&stores) {
        *o = s.1;
    }
    Ok(out)
}

/// The 128-byte mask the masked key setup reads with its movsx (rip-relative).
fn mask_bytes<'a>(b: &'a [u8], secs: &[Section]) -> Result<&'a [u8]> {
    let at = find_once(b, KEY_SETUP, "key setup")? + 14;
    let disp = i64::from(u32_at(b, at + 3)? as i32);
    let target = (off_to_rva(secs, at as u64)? as i64 + 7 + disp) as u64;
    let off = rva_to_off(secs, target, 128)?;
    Ok(&b[off..off + 128])
}

/// The pak key, from the shipping exe's bytes. Not yet proved: the caller opens a pak index with it.
pub fn key_from_exe_bytes(b: &[u8]) -> Result<AesKey> {
    let low = callback_bytes(b)?;
    let mask = mask_bytes(b, &sections(b)?)?;
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        let m = mask[4 * i];
        // The engine loads the mask byte sign-extended; a byte of 0x80 or above would smear into the higher key
        // bytes, which no known build has, so it is refused rather than modelled.
        if m >= 0x80 {
            return Err(exekey("a mask byte of 0x80 or above (an unknown build)"));
        }
        *k = low[i] ^ m;
    }
    Ok(AesKey(key))
}

/// The pak key, read from the player's own shipping exe by signature.
pub fn key_from_exe(exe: &Path) -> Result<AesKey> {
    if !exe.exists() {
        return Err(exekey(format!("no {}", exe.display())));
    }
    key_from_exe_bytes(&std::fs::read(exe).map_err(|e| PakError::Io(exe.to_path_buf(), e))?)
}

/// The setup check: every pak of the build in `paks_dir` by name and size, their SHA-256 against the list (all
/// cores), and `key` opening every index. Fails naming the first file that is not the build's.
///
/// Refuses a missing pak, a pak the build does not have (another version's chunk, a patch or mod pak: refused until
/// the engine's pak order is decoded, see [`PakSet::open`]), a pak whose size differs, a build file missing from the
/// install the paks lie in and a key that does not open their indexes (all before the hash pass, the long part), and
/// a pak whose SHA-256 differs. Reads only; the .sig files are not read. `cancel`, set from another thread, ends the
/// hash pass at its next piece (8 MB of a file) with [`PakError::Cancelled`].
pub fn setup_check(
    paks_dir: &Path,
    key: &AesKey,
    build: &BuildList,
    progress: Option<Progress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    check_list(build)?;
    let found = compare_paks(paks_dir, build)
        .map_err(|_| setup_err(format!("cannot read the folder {}", paks_dir.display())))?;
    if found.names.is_empty() {
        return Err(setup_err(format!(
            "no .pak files in {}; choose the beta's MultiVersus/Content/Paks folder",
            paks_dir.display()
        )));
    }
    let be = |n: usize| if n > 1 { "are" } else { "is" };
    if !found.extra.is_empty() {
        return Err(setup_err(format!(
            "{} {} not part of the open beta build ({}); this folder holds another version, or a patch or mod pak",
            found.extra.join(", "),
            be(found.extra.len()),
            build.id
        )));
    }
    if !found.missing.is_empty() {
        return Err(setup_err(format!("{} missing from {}", found.missing.join(", "), paks_dir.display())));
    }
    if let Some((name, size, want)) = found.wrong_size.first() {
        return Err(setup_err(format!(
            "{name} is {size} bytes, the open beta's is {want}: another version, or a damaged or partial file"
        )));
    }
    let gone = missing_build_files(paks_dir, build);
    if !gone.is_empty() {
        return Err(setup_err(format!(
            "{} {} missing from the beta's folder ({})",
            gone.join(", "),
            be(gone.len()),
            install_root(paks_dir).display()
        )));
    }
    let files: Vec<PathBuf> = build.paks.iter().map(|p| paks_dir.join(&p.name)).collect();
    if let Err(e) = PakSet::open(&files, key) {
        return Err(setup_err(format!("the key does not open the paks in {} ({e})", paks_dir.display())));
    }

    let list: Vec<(PathBuf, String, u64)> =
        build.paks.iter().zip(&files).map(|(p, f)| (f.clone(), p.name.clone(), p.size)).collect();
    for (p, sha) in build.paks.iter().zip(hash_files(&list, progress, cancel)?) {
        let sha = sha.map_err(|e| setup_err(format!("reading {} failed: {e}", p.name)))?;
        if sha != p.sha256 {
            return Err(setup_err(format!(
                "{} does not match the open beta's (SHA-256 differs): another version, or a damaged file",
                p.name
            )));
        }
    }
    Ok(())
}
