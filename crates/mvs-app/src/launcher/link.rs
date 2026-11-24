//! (the port's own) Join links: `mvsclassic://join/<code>` (mvs_release::URL_SCHEME). The launcher registers the
//! scheme for the Windows user (HKCU\Software\Classes\mvsclassic, no admin), so a click runs
//! `mvs-launcher --link <url>`: the code is checked, then handed to the running game (its join-link file, and its
//! window raised) or the game is started with `--online-open join:<code>`.
//!
//! The link is read strictly: its code must be one the join box takes (letters and digits, then
//! mvs_net::codes::decode_lobby_code), and it goes to the game as a value, never through a shell. A code that names
//! another relay still asks the player in the game, as a pasted one does.

use std::path::{Path, PathBuf};

use mvs_ui::launcher::words as w;
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};
#[cfg(windows)]
use windows::core::HSTRING;

use super::game::{Game, GameArgs};
use super::host::Options;
use super::setup_os::{data_root, store_dir};
use crate::online::link as game_link;

/// Where the current user's classes live under HKEY_CURRENT_USER.
#[cfg(windows)]
const USER_CLASSES: &str = r"Software\Classes";

/// A registry the scheme's keys are read from and written to (HKCU in the program; a map in the tests).
pub trait Keys {
    /// The string value `name` ("" the default value) of `key`, under the classes' root.
    fn get(&self, key: &str, name: &str) -> Option<String>;
    fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String>;
}

/// HKEY_CURRENT_USER under `base` (`Software\Classes`).
#[cfg(windows)]
#[derive(Debug)]
pub struct UserKeys {
    pub base: String,
}

#[cfg(windows)]
impl UserKeys {
    fn path(&self, key: &str) -> HSTRING {
        HSTRING::from(format!("{}\\{key}", self.base))
    }
}

#[cfg(windows)]
impl Keys for UserKeys {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        let mut buf = vec![0u16; 2048];
        let mut size = u32::try_from(buf.len() * 2).unwrap_or(0);
        // SAFETY: a buffer of `size` bytes the call writes a terminated string into, its size given and written back.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &self.path(key),
                &HSTRING::from(name),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
        let path = self.path(key);
        let mut k = HKEY::default();
        // SAFETY: a key of the current user's made or opened for writing; `k` is closed below.
        let r = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &path,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut k,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            return Err(format!("{path}: {}", windows::core::Error::from(r.to_hresult()).message()));
        }
        let bytes: Vec<u8> = value.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
        // SAFETY: a key opened above, the value's UTF-16 bytes with their terminator.
        let r = unsafe { RegSetValueExW(k, &HSTRING::from(name), None, REG_SZ, Some(&bytes)) };
        // SAFETY: the key opened above.
        unsafe {
            let _ = RegCloseKey(k);
        }
        if r != ERROR_SUCCESS {
            return Err(format!("{path} {name}: {}", windows::core::Error::from(r.to_hresult()).message()));
        }
        Ok(())
    }
}

/// The scheme's values for the launcher at `exe`: (key, value name, value).
pub fn scheme_values(scheme: &str, exe: &Path) -> Vec<(String, &'static str, String)> {
    let exe = exe.display();
    vec![
        (scheme.to_owned(), "", "URL:MVS Classic".to_owned()),
        (scheme.to_owned(), "URL Protocol", String::new()),
        (format!(r"{scheme}\DefaultIcon"), "", format!("\"{exe}\",0")),
        (format!(r"{scheme}\shell\open\command"), "", format!("\"{exe}\" --link \"%1\"")),
    ]
}

/// The scheme's values written when one of them is missing or other; true when they were written.
pub fn register_in(keys: &mut dyn Keys, scheme: &str, exe: &Path) -> Result<bool, String> {
    let values = scheme_values(scheme, exe);
    if values.iter().all(|(k, n, v)| keys.get(k, n).as_deref() == Some(v.as_str())) {
        return Ok(false);
    }
    for (k, n, v) in &values {
        keys.set(k, n, v)?;
    }
    Ok(true)
}

/// Writes the scheme's keys for this launcher when they are not there or name another program.
#[cfg(windows)]
pub fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut keys = UserKeys { base: USER_CLASSES.to_owned() };
    if register_in(&mut keys, mvs_release::URL_SCHEME, &exe)? {
        eprintln!("mvs-launcher: join links ({}://) open {}", mvs_release::URL_SCHEME, exe.display());
    }
    Ok(())
}

/// The join links' desktop entry, in the user's applications folder.
#[cfg(not(windows))]
const LINK_DESKTOP: &str = "mvsclassic-link.desktop";

/// Writes the scheme's handler for this launcher when it is not there or names another program: the desktop entry
/// [`link_desktop_entry`] in the user's applications folder, then `xdg-mime` makes it the scheme's default.
#[cfg(not(windows))]
pub fn register() -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.to_str().is_none() {
        return Err(format!("{}: not UTF-8, which a desktop entry cannot name", exe.display()));
    }
    let dir = super::setup_os::applications_dir().ok_or("no home folder (HOME)")?;
    let file = dir.join(LINK_DESKTOP);
    let text = link_desktop_entry(&exe);
    let mime = format!("x-scheme-handler/{}", mvs_release::URL_SCHEME);
    let written = std::fs::read_to_string(&file).is_ok_and(|t| t == text);
    let default = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == LINK_DESKTOP);
    if written && default {
        return Ok(());
    }
    if !written {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(&file, &text).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    let o = Command::new("xdg-mime")
        .args(["default", LINK_DESKTOP, &mime])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("xdg-mime: {e}"))?;
    if !o.status.success() {
        return Err(format!("xdg-mime: {} {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    eprintln!("mvs-launcher: join links ({}://) open {}", mvs_release::URL_SCHEME, exe.display());
    Ok(())
}

/// The join links' desktop entry for the launcher at `launcher`: the scheme's handler, `--link <url>` (the
/// registry's command on Windows), kept out of the menu.
#[cfg(not(windows))]
pub fn link_desktop_entry(launcher: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=MVS Classic\nExec={} --link %u\nMimeType=x-scheme-handler/{};\n\
         NoDisplay=true\nTerminal=false\n",
        super::setup_os::desktop_exec_arg(launcher),
        mvs_release::URL_SCHEME
    )
}

/// The lobby code of a join link `<scheme>://join/<code>` (the scheme and `join` in any case, one `/` after the code
/// allowed: a browser may add it), checked as the join box checks a code; else why it is not one.
pub fn code_of_link(url: &str, default_relay: Option<&str>) -> Result<String, String> {
    let scheme = mvs_release::URL_SCHEME;
    let bad = || "not a join link".to_owned();
    let (s, rest) = url.trim().split_once(':').ok_or_else(bad)?;
    if !s.eq_ignore_ascii_case(scheme) {
        return Err(bad());
    }
    let rest = rest.strip_prefix("//").ok_or_else(bad)?;
    let (what, code) = rest.split_once('/').ok_or_else(bad)?;
    if !what.eq_ignore_ascii_case("join") {
        return Err(bad());
    }
    let code = code.strip_suffix('/').unwrap_or(code);
    let code = game_link::plain_code(code)?;
    mvs_net::codes::decode_lobby_code(code, default_relay).map_err(mvs_ui::menu::online_text::code_why_text)?;
    Ok(code.to_owned())
}

/// What a link is handed to: the running game and the game's start (the program's; a test's own).
pub trait LinkOs {
    /// The game holding the data folder: Some(its pid when known); None: none runs.
    fn running(&self, data: &Path) -> Option<Option<u32>>;
    fn hand_over(&mut self, data: &Path, code: &str) -> std::io::Result<()>;
    fn raise(&mut self, pid: u32);
    /// The store holds an import the game can open.
    fn ready(&self, store: &Path) -> bool;
    /// An import runs on the store (its live lock).
    fn importing(&self, store: &Path) -> bool;
    fn start(&mut self, game: &GameArgs, store: &Path, code: &str) -> std::io::Result<()>;
}

/// The program's: the instance's mutex and pid file, the join-link file, the game's window, its start.
#[derive(Debug, Default)]
pub struct RealOs;

impl LinkOs for RealOs {
    fn running(&self, data: &Path) -> Option<Option<u32>> {
        game_link::running_game(data)
    }

    fn hand_over(&mut self, data: &Path, code: &str) -> std::io::Result<()> {
        game_link::hand_over(data, code)
    }

    fn raise(&mut self, pid: u32) {
        super::game::raise(pid);
    }

    fn ready(&self, store: &Path) -> bool {
        matches!(mvs_store::manage::read_manifest(store), Ok(Some(_)))
    }

    fn importing(&self, store: &Path) -> bool {
        import_running(store)
    }

    fn start(&mut self, game: &GameArgs, store: &Path, code: &str) -> std::io::Result<()> {
        Game::start_joining(game, store, code).map(drop)
    }
}

/// A lock file touched this recently is a running import's (the importer's lock beats every 10 s and counts one
/// untouched for a minute as a dead run's: mvs_import's run/store_lock.rs STALE).
const LOCK_LIVE: std::time::Duration = std::time::Duration::from_secs(60);

/// The store's import lock is live (judged by its age alone: a dead run's lock under a minute old counts as live,
/// which only defers the link).
pub fn import_running(store: &Path) -> bool {
    std::fs::metadata(store.join(mvs_store::manage::LOCK))
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().map_or(true, |a| a < LOCK_LIVE))
}

/// Where a link went.
#[derive(Debug, PartialEq, Eq)]
pub enum Routed {
    /// To the running game (its pid, when known).
    Running(Option<u32>),
    /// The game started on it.
    Started,
}

/// The store a link's game plays: `--store`'s, else the data root's.
fn store_of(o: &Options) -> PathBuf {
    o.store.clone().unwrap_or_else(|| store_dir(&data_root(&mvs_settings::Settings::load(&o.data), &o.data)))
}

/// A link taken to the game: checked, then to the running game or the game started on it; else the player's words.
pub fn route(url: &str, o: &Options, os: &mut dyn LinkOs) -> Result<Routed, String> {
    let online = mvs_settings::online::read_online_config(&o.online_defaults, &mut |l| eprintln!("mvs-launcher: {l}"));
    let code = code_of_link(url, online.relay.as_deref()).map_err(|why| w::link_bad(&why))?;
    // (an import rewrites the store's packs: neither a game started nor one that runs takes a join meanwhile)
    let store = store_of(o);
    if os.importing(&store) {
        return Err(w::LINK_NOT_READY.to_owned());
    }
    if let Some(pid) = os.running(&o.data) {
        os.hand_over(&o.data, &code).map_err(|e| w::link_game_failed(&e.to_string()))?;
        if let Some(pid) = pid {
            os.raise(pid);
        }
        return Ok(Routed::Running(pid));
    }
    if !os.ready(&store) {
        return Err(w::LINK_NOT_READY.to_owned());
    }
    let game = GameArgs {
        exe: o.game.clone(),
        data: o.data_given.then(|| o.data.clone()),
        online_defaults: o.online_defaults.clone(),
        adapter: o.adapter.clone(),
        pass_on: o.pass_on.clone(),
    };
    os.start(&game, &store, &code).map_err(|e| w::link_game_failed(&format!("{}: {e}", o.game.display())))?;
    Ok(Routed::Started)
}

/// `mvs-launcher --link <url>` with the launcher's other options: the link's code to the game; the exit code. A
/// link that cannot be followed says why in a box (a click on a link has no console).
#[cfg(windows)]
pub fn forward(url: &str, o: &Options) -> u8 {
    match route(url, o, &mut RealOs) {
        Ok(r) => {
            eprintln!("mvs-launcher: --link: {r:?}");
            0
        }
        Err(why) => {
            eprintln!("mvs-launcher: --link: {why}");
            // SAFETY: a modal box of the launcher's own, its strings alive through the call.
            unsafe {
                MessageBoxW(None, &HSTRING::from(why), &HSTRING::from("MVS Classic"), MB_OK | MB_ICONWARNING);
            }
            1
        }
    }
}

/// `mvs-launcher --link <url>` with the launcher's other options: the link's code to the game; the exit code. A
/// link that cannot be followed says why on stderr only.
// Deviation: Windows shows why in a message box, as a click on a link from the desktop has no terminal; here it goes
// to stderr only until SDL's message box is there (Task 7).
#[cfg(not(windows))]
pub fn forward(url: &str, o: &Options) -> u8 {
    match route(url, o, &mut RealOs) {
        Ok(r) => {
            eprintln!("mvs-launcher: --link: {r:?}");
            0
        }
        Err(why) => {
            eprintln!("mvs-launcher: --link: {why}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MapKeys(BTreeMap<(String, String), String>, u32);

    impl Keys for MapKeys {
        fn get(&self, key: &str, name: &str) -> Option<String> {
            self.0.get(&(key.to_owned(), name.to_owned())).cloned()
        }
        fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
            self.1 += 1;
            self.0.insert((key.to_owned(), name.to_owned()), value.to_owned());
            Ok(())
        }
    }

    #[test]
    fn the_keys_are_written_only_when_they_differ() {
        let mut k = MapKeys::default();
        let exe = Path::new(r"C:\Games\MVS Classic\mvs-launcher.exe");
        assert_eq!(register_in(&mut k, "mvsclassic", exe), Ok(true));
        assert_eq!(
            k.get(r"mvsclassic\shell\open\command", "").as_deref(),
            Some(r#""C:\Games\MVS Classic\mvs-launcher.exe" --link "%1""#)
        );
        assert_eq!(k.get("mvsclassic", "URL Protocol").as_deref(), Some(""));
        let writes = k.1;
        assert_eq!(register_in(&mut k, "mvsclassic", exe), Ok(false));
        assert_eq!(k.1, writes, "the same launcher: nothing written");
        assert_eq!(register_in(&mut k, "mvsclassic", Path::new(r"D:\MVS\mvs-launcher.exe")), Ok(true));
    }

    #[cfg(windows)]
    #[test]
    fn the_users_registry_takes_and_gives_the_values() {
        // (a test key of its own under the user's classes, removed after: never the scheme's)
        let scheme = format!("mvsclassic-test-{}", std::process::id());
        let mut k = UserKeys { base: USER_CLASSES.to_owned() };
        let exe = Path::new(r"C:\MVS\mvs-launcher.exe");
        let first = register_in(&mut k, &scheme, exe);
        let again = register_in(&mut k, &scheme, exe);
        let command = k.get(&format!(r"{scheme}\shell\open\command"), "");
        let path = HSTRING::from(format!(r"{USER_CLASSES}\{scheme}"));
        // SAFETY: the test's own key, removed whole.
        let gone = unsafe { windows::Win32::System::Registry::RegDeleteTreeW(HKEY_CURRENT_USER, &path) };
        assert_eq!(first, Ok(true));
        assert_eq!(again, Ok(false));
        assert_eq!(command.as_deref(), Some(r#""C:\MVS\mvs-launcher.exe" --link "%1""#));
        assert_eq!(gone, ERROR_SUCCESS);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_desktop_entry_names_the_launcher_quoted() {
        let exec = |p: &str| {
            let text = link_desktop_entry(Path::new(p));
            assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
            assert!(text.lines().any(|l| l == "MimeType=x-scheme-handler/mvsclassic;"), "{text}");
            text.lines().find(|l| l.starts_with("Exec=")).map(str::to_owned)
        };
        // (`--link` first: the launcher reads a link only there)
        assert_eq!(exec("/a b/MVSC Launcher").as_deref(), Some(r#"Exec="/a b/MVSC Launcher" --link %u"#));
        assert_eq!(exec("/a$b/MVSC Launcher").as_deref(), Some(r#"Exec="/a\\$b/MVSC Launcher" --link %u"#));
        assert_eq!(exec("/a\"b/MVSC Launcher").as_deref(), Some(r#"Exec="/a\\"b/MVSC Launcher" --link %u"#));
    }

    /// A relay code of the release's relay, as a host writes it.
    fn a_code() -> String {
        let c = mvs_net::codes::LobbyCode::Relay { lobby: "l0123456789abcdef0123".to_owned(), relay: None };
        mvs_net::codes::encode_lobby_code(&c, Some("relay.example:7777")).unwrap()
    }

    #[test]
    fn a_fresh_import_lock_is_live() {
        let dir = std::env::temp_dir().join(format!("mvs-link-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!import_running(&dir));
        std::fs::write(dir.join(mvs_store::manage::LOCK), "{}").unwrap();
        assert!(import_running(&dir));
        let old = std::time::SystemTime::now() - LOCK_LIVE * 2;
        std::fs::File::options()
            .write(true)
            .open(dir.join(mvs_store::manage::LOCK))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(!import_running(&dir), "a dead run's");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_holds_one_plain_code() {
        let code = a_code();
        let ok = |u: &str| code_of_link(u, None);
        assert_eq!(ok(&format!("mvsclassic://join/{code}")), Ok(code.clone()));
        assert_eq!(ok(&format!("MVSClassic://JOIN/{code}/")), Ok(code.clone()));
        let mut typo = code.clone().into_bytes();
        let last = typo.len() - 1;
        typo[last] = if typo[last] == b'a' { b'b' } else { b'a' };
        let typo = String::from_utf8(typo).unwrap();
        for bad in [
            "mvsclassic://join/".to_owned(),
            format!("mvsclassic://join/{typo}"),
            format!("mvsclassic://join/{code}?x=1"),
            format!("mvsclassic://join/{code}/more"),
            format!("mvsclassic://join/%20{code}"),
            format!("mvsclassic://join/{code}\"&calc"),
            format!("mvsclassic://open/{code}"),
            format!("mvsclassic:join/{code}"),
            format!("https://join/{code}"),
        ] {
            assert!(ok(&bad).is_err(), "{bad}");
        }
    }

    /// The test's OS: a game running or not, what was handed over and started.
    #[derive(Default)]
    struct FakeOs {
        running: Option<Option<u32>>,
        ready: bool,
        importing: bool,
        handed: Vec<String>,
        raised: Vec<u32>,
        started: Vec<(Vec<String>, String)>,
    }

    impl LinkOs for FakeOs {
        fn running(&self, _: &Path) -> Option<Option<u32>> {
            self.running
        }
        fn hand_over(&mut self, _: &Path, code: &str) -> std::io::Result<()> {
            self.handed.push(code.to_owned());
            Ok(())
        }
        fn raise(&mut self, pid: u32) {
            self.raised.push(pid);
        }
        fn ready(&self, _: &Path) -> bool {
            self.ready
        }
        fn importing(&self, _: &Path) -> bool {
            self.importing
        }
        fn start(&mut self, game: &GameArgs, store: &Path, code: &str) -> std::io::Result<()> {
            let line = game.join_line(store, code).iter().map(|s| s.to_string_lossy().into_owned()).collect();
            self.started.push((line, code.to_owned()));
            Ok(())
        }
    }

    fn options() -> Options {
        Options {
            data: std::env::temp_dir().join("mvs-link-route-test"),
            store: Some(PathBuf::from(r"D:\store")),
            online_defaults: PathBuf::from(r"D:\no-online.json"),
            game: PathBuf::from(r"C:\MVS\mvs.exe"),
            ..Default::default()
        }
    }

    #[test]
    fn a_link_goes_to_the_running_game_or_starts_it() {
        let code = a_code();
        let url = format!("mvsclassic://join/{code}");
        let o = options();
        // (the game open: handed over, its window raised, nothing started)
        let mut os = FakeOs { running: Some(Some(42)), ready: true, ..Default::default() };
        assert_eq!(route(&url, &o, &mut os), Ok(Routed::Running(Some(42))));
        assert_eq!((os.handed.as_slice(), os.raised.as_slice()), (&[code.clone()][..], &[42][..]));
        assert!(os.started.is_empty());
        // (the game closed: started on the store with the join)
        let mut os = FakeOs { ready: true, ..Default::default() };
        assert_eq!(route(&url, &o, &mut os), Ok(Routed::Started));
        let (line, _) = &os.started[0];
        assert_eq!(line[..3], ["--play", "--store", r"D:\store"]);
        assert_eq!(line[line.len() - 2..], ["--online-open".to_owned(), format!("join:{code}")]);
        assert!(os.handed.is_empty());
        // (not set up: nothing started, the player told)
        let mut os = FakeOs::default();
        assert_eq!(route(&url, &o, &mut os), Err(w::LINK_NOT_READY.to_owned()));
        assert!(os.started.is_empty());
        // (an import under way: neither the running game nor a new one takes it)
        for running in [Some(Some(42)), None] {
            let mut os = FakeOs { running, ready: true, importing: true, ..Default::default() };
            assert_eq!(route(&url, &o, &mut os), Err(w::LINK_NOT_READY.to_owned()));
            assert!(os.handed.is_empty() && os.started.is_empty());
        }
        // (a bad link: nothing at all)
        let mut os = FakeOs { running: Some(Some(42)), ready: true, ..Default::default() };
        assert!(route("mvsclassic://join/a&b", &o, &mut os).is_err());
        assert!(os.handed.is_empty() && os.started.is_empty());
    }
}
