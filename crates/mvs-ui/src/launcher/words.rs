//! The launcher's words (app/launcher/index.html and launcher.ts): the port's own, which the beta has no line for.

use super::Way;
use super::import_view::{CheckWhy, GroupName, Say, Why};

pub const TAGLINE: &str = "The 2023 Open Beta, playable again from your own copy.";
pub const FOOT: &str =
    "MVS Classic is an unofficial fan project. It includes no game files and uses your own copy of the Open Beta.";
pub const STEPS: [(&str, &str); 3] = [
    ("Install location", "Where the game is installed"),
    ("Your Open Beta", "Located on your PC, never modified"),
    ("Build the game", "A one-time setup from your copy"),
];
pub const STARTING: &str = "Starting";

pub fn step_of(n: u32) -> String {
    format!("Step {n} of 3")
}
pub const ONLY_ONCE: &str = " \u{b7} Only happens once";

pub const INSTALL_TITLE: &str = "Choose where to install";
pub const INSTALL_LEAD: &str = "MVS Classic builds the game into this folder from your copy of the Open Beta. This is a one-time step that needs about 8 GB of free space.";
pub const INSTALL_LABEL: &str = "Install location";
pub const CHANGE_LOCATION: &str = "Change location";
pub const BACK_TO_DEFAULT: &str = "Back to the default";
pub const CANCEL: &str = "Cancel";
pub const CONTINUE: &str = "Continue";
pub const BACK: &str = "Back";

pub fn space_ok(free: &str) -> String {
    format!("{free} free on this drive.")
}
pub fn space_short(free: &str, need: &str) -> String {
    format!("Not enough space: {free} free on this drive, the game needs {need}.")
}
pub fn rebuild_space_short(free: &str, need: &str) -> String {
    format!(
        "Not enough space to rebuild: {free} free on this drive, the parts to rebuild take {need} while they are written."
    )
}
pub fn folder_unusable(why: &str) -> String {
    format!("This folder cannot be used: {why}")
}
pub const NOT_WRITABLE: &str = "MVS Classic does not have permission to write to this folder. Folders such as Program Files require administrator rights, so choose a different folder.";
pub const ONEDRIVE: &str = "This folder is synced by OneDrive, which would upload the entire game (about 8 GB). Choose a folder outside OneDrive.";
pub const TEMP_FOLDER: &str = "This is a temporary folder that Windows may clear. Choose a permanent folder instead.";
pub const APP_IN_TEMP: &str = "MVS Classic is running from inside a zip file or a temporary folder. Close it, extract the entire zip to a folder of your choice (right-click the zip and choose Extract All), and start it from there.";
pub const APP_NOT_WRITABLE: &str = "MVS Classic cannot write to its own folder, so it cannot save its settings or install updates. Move the MVS Classic folder to a location outside Program Files.";

pub const BETA_TITLE: &str = "Locate your Open Beta";
pub const BETA_LEAD: &str = "MVS Classic reads the Open Beta files where they are. Nothing is copied or modified.";
pub fn beta_again(why: &str) -> String {
    format!("MVS Classic needs your Open Beta folder again: {why}.")
}
pub const LOOKING: &str = "Searching for the Open Beta…";
pub const NONE_FOUND: &str = "The Open Beta was not found on this PC.";
pub const CHOOSE_FOLDER: &str = "Choose folder";
pub const AUTO_DETECT: &str = "Auto detect";
pub const NO_BETA: &str = "Don't have the Open Beta?";
pub const NO_BETA_TEXT: &str = "Follow the step-by-step guide to download it through Steam.";
pub const SHOW_ME: &str = "View the guide";

pub const GUIDE_TITLE: &str = "Download the Open Beta from Steam";
pub const GUIDE_INTRO: &str = "The Open Beta is still available on Steam to anyone who has MultiVersus in their library. It is downloaded through the Steam console and is about 8 GB.";
pub const GUIDE_1: &str = "Start Steam and sign in.";
pub const GUIDE_2: &str = "Open Steam's console:";
pub const STEAM_CONSOLE: &str = "Open the Steam console";
pub const GUIDE_2_OR: &str = "(or enter steam://open/console in your browser's address bar or the Windows Run dialog)";
pub const GUIDE_3: &str = "Paste this into the console and press Enter:";
pub const DEPOT: &str = "download_depot 1818750 1818751 8780239795816477473";
pub const COPY: &str = "Copy";
pub const COPIED: &str = "Copied";
pub const GUIDE_4: &str = "The console shows no progress while it downloads. Wait for the message \"Depot download complete\", which includes the folder it was saved to.";
/// Step 5 with its bold words between asterisks.
pub const GUIDE_5: &str = "Press *Auto detect* and MVS Classic will find the folder. If it does not, press *Choose folder* and select the depot_1818751 folder shown by Steam.";
pub const GOT_IT: &str = "Got it";

pub const BUILDING: &str = "Building the game";
pub const BUILD_STOPPED: &str = "The build stopped";
pub fn installing_to(root: &str) -> String {
    format!("Installing to {root}")
}
pub const SHOW_DETAILS: &str = "Show details";
pub const HIDE_DETAILS: &str = "Hide details";
pub const BACK_TO_SETUP: &str = "Back to setup";
pub const TRY_AGAIN: &str = "Try again";
pub const WORKING_OUT: &str = "Working out the time left";
pub fn time_left(min: u32) -> String {
    if min <= 1 { "About a minute left".into() } else { format!("About {min} minutes left") }
}
pub fn written(size: &str) -> String {
    format!("{size} written")
}
pub fn stopped(message: &str) -> String {
    format!("The build stopped: {message}")
}
pub fn not_started(why: &str) -> String {
    format!("The build did not start: {why}")
}
pub const CANCELLED: &str =
    "The build was cancelled. Completed parts are kept, and the next build continues from there.";

pub fn group(g: GroupName) -> &'static str {
    match g {
        GroupName::Check => "Verifying your files",
        GroupName::Read => "Reading game packages",
        GroupName::Characters => "Characters",
        GroupName::Stages => "Stages",
        GroupName::Models => "Models and textures",
        GroupName::Effects => "Effects and materials",
        GroupName::Sound => "Sound and music",
        GroupName::Menus => "Menus and text",
    }
}

pub const PLAY: &str = "PLAY";
pub const PLAY_BUILDING: &str = "BUILDING";
pub const FIX_IT: &str = "FIX IT";
pub const RUNNING: &str = "RUNNING";
pub const PLAY_HINT: &str = "You can close the launcher while you play.";
pub const PLAYING: &str = "Playing";
pub const GAME_OPEN: &str = "The game is open";
pub const EM_GAME_CLOSED: &str = "Game closed";
pub fn after(elapsed: &str) -> String {
    format!("after {elapsed}")
}
/// The time the game has run (launcher.ts elapsed): m:ss, or h:mm:ss past an hour.
pub fn elapsed(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}
pub fn game_not_started(why: &str) -> String {
    format!("The game could not start: {why}")
}
pub const GAME_RUNNING: &str = "The game is already running. Close it first";
/// (a dev run's `--store`: the store is played as it is)
pub const STORE_GIVEN: &str = "This store was named with --store: it is played as it is, never rebuilt";
pub const READY: &str = "Ready";
pub const READY_TO_PLAY: &str = "Ready to play";
pub const REBUILT_FIRST: &str = "The game will be rebuilt from the Open Beta first";
pub fn parts_rebuilt_first(n: usize) -> String {
    format!("{} will be rebuilt first", plural(n, "part"))
}
/// The log's first line (its word: Started).
pub const LAUNCHER_OPENED: &str = "MVS Classic launcher opened";
pub const BUILDING_STATUS: &str = "Building";
pub const STARTS_AFTER: &str = "The game will start when it is done";
pub const CANT_PLAY: &str = "Can't play";
pub const UNREADABLE: &str = "Your Open Beta files could not be read";
pub fn unreadable(why: &str) -> String {
    format!("Your Open Beta files could not be read: {why}.")
}
pub const LOCATE_IT: &str = "Locate it";
pub const OPEN_INSTALL_FOLDER: &str = "Open install folder";
pub const REBUILD: &str = "Rebuild from the Open Beta";

pub const SETTINGS: &str = "Settings";
pub const TAB_GENERAL: &str = "General";
pub const TAB_FILES: &str = "Game files";
pub const TAB_ABOUT: &str = "About";
pub const TAB_ONLINE: &str = "Online";
pub const RELAY: &str = "Relay server";
pub const RELAY_DESC: &str = "Hosts custom games, party invites and matchmaking.";
pub const RELAY_PLACEHOLDER: &str = "Server address, for example relay.example.org";
pub const EOS_LABEL: &str = "Epic Online Services";
pub const EOS_DESC: &str = "Use Epic Online Services for custom games and party invites, and for matchmaking when no \
                            relay server is available.";
pub const LAN_LABEL: &str = "Local network (LAN)";
pub const LAN_DESC: &str = "Host and join custom games with players on the same local network. The first time you \
                            host, Windows may ask you to allow MVS Classic through its firewall.";
pub const LAN_PORT: &str = "LAN port";
pub const DIRECT_LABEL: &str = "Direct connections";
pub const DIRECT_DESC: &str = "Connect directly to friends in a custom game when both networks allow it, which can \
                               lower latency. The players in your lobby can then see your IP address.";
pub const SHORTCUTS: &str = "Shortcuts";
pub const SHORTCUTS_DESC: &str = "MVS Classic does not install anything on your PC; it runs from its own folder. \
                                  Add a shortcut to open it from the desktop or the Start menu.";
pub fn shortcut_desktop(there: bool) -> &'static str {
    if there { "Desktop shortcut added" } else { "Add a desktop shortcut" }
}
pub fn shortcut_start(there: bool) -> &'static str {
    if there { "Added to the Start menu" } else { "Add to the Start menu" }
}
pub const STATUS: &str = "Status";
pub const GAME_DATA: &str = "Game data";
pub const OPEN_BETA: &str = "Open Beta";
pub const VERSION: &str = "Version";
pub const REBUILD_NEEDED: &str = "Rebuild needed";
pub fn parts_to_rebuild(n: usize) -> String {
    format!("{} to rebuild", plural(n, "part"))
}
pub const FOUND: &str = "Found";
pub const NOT_FOUND: &str = "Not found";
pub const INSTALL_LOCATION: &str = "Install location";
pub const ROOT_NOTE: &str = "The game built from your Open Beta is stored here.";
pub const MOVE: &str = "Move";
pub const OPEN: &str = "Open";
pub const BETA_FOLDER: &str = "Open Beta folder";
pub const BETA_FOLDER_DESC: &str = "Used when the game is built or rebuilt.";
pub const NOT_CHOSEN: &str = "Not chosen yet";
pub const CHANGE: &str = "Change";
pub const PROJECT: &str = "About MVS Classic";
pub const PROJECT_DESC: &str = "MVS Classic is a free, unofficial fan project that brings back the MultiVersus Open Beta. It includes no game files: you provide your own copy of the Open Beta, and the launcher builds the game from it on your PC, once.";
pub const DISCLAIMER: &str = "Disclaimer";
pub const DISCLAIMER_DESC: &str = "MVS Classic is not affiliated with, endorsed by or associated with Warner Bros. \
                                   Games or Player First Games in any way. MultiVersus and its characters, names and \
                                   marks belong to their owners.";

// The launcher log's lines.
pub const EM_STARTED: &str = "Started";
pub const EM_INSTALL: &str = "Install location";
pub const EM_FOUND: &str = "Open beta found";
pub const EM_BUILDING: &str = "Building";
pub const EM_REBUILDING: &str = "Rebuilding";
pub const EM_BUILD_DONE: &str = "Build done";
pub const EM_SHORTCUT: &str = "Shortcut";
pub const EM_ERROR: &str = "Error";
pub const FROM_BETA: &str = "from your open beta";
pub const WHOLE_GAME: &str = "the whole game, from the open beta";
pub const ON_DESKTOP: &str = "On the desktop";
pub const IN_START_MENU: &str = "In the Start menu";
pub fn shortcut_failed(why: &str) -> String {
    format!("The shortcut could not be made: {why}")
}
pub fn open_failed(why: &str) -> String {
    format!("The install folder could not be opened: {why}")
}
pub fn windows_answered(code: usize) -> String {
    format!("Windows answered {code}")
}
pub const NO_SHORTCUTS_DEV: &str = "a dev run makes no shortcuts";
pub const NO_SHORTCUT_FOLDER: &str = "the folder for the shortcut was not found";
pub const NOT_READY: &str = "The game could not start: the game data is not ready";
pub fn built(parts: usize, minutes: u64, skipped: u64) -> String {
    let mut s = format!("{} built in {}", plural(parts, "part"), plural(minutes as usize, "minute"));
    if skipped > 0 {
        s += &format!(", {} skipped", plural(skipped as usize, "file"));
    }
    s
}

/// `n word` or `n words`, with thousands separated.
pub fn plural(n: usize, word: &str) -> String {
    format!("{} {word}{}", thousands(n as u64), if n == 1 { "" } else { "s" })
}

/// `n` with commas between its thousands (toLocaleString, en).
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Bytes in GB to a tenth.
pub fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1e9)
}

/// Bytes in MB below a GB, else in GB.
pub fn size(b: u64) -> String {
    if b < 1_000_000_000 { format!("{} MB", (b as f64 / 1e6).round()) } else { gb(b) }
}

// The folder pickers' titles.
pub const PICK_BETA: &str = "Select the MultiVersus Open Beta folder";
pub const PICK_ROOT: &str = "Select where to install MVS Classic";

/// The check's count line: bytes hashed of the bytes to hash.
pub fn checked(done: u64, total: u64) -> String {
    format!("{} of {}", gb(done), gb(total))
}
/// A kind's build count line.
pub fn kind_count(kind: &str, done: u64, total: u64) -> String {
    format!("{kind} {} / {}", thousands(done), thousands(total))
}

// The worker's lines

/// Why setup does not hold.
pub fn why(w: &Why) -> String {
    match w {
        Why::SetupNotDone => "setup not done".into(),
        Why::OtherBuild => "the beta folder was checked for another build".into(),
        Why::RootInsideBeta { root, beta } => format!(
            "the data folder {} is inside the beta's folder {}: choose a folder outside it",
            root.display(),
            beta.display()
        ),
        Why::BetaGone { paks } => format!("the beta folder {} is gone or cannot be read", paks.display()),
        Why::PaksMissing { paks, count } => format!("{count} paks are missing from {}", paks.display()),
        Why::PaksExtra { paks } => format!("{} holds paks the open beta does not have", paks.display()),
        Why::PakResized { paks, file } => format!("{file} in {} changed size", paks.display()),
        Why::ExeGone { paks } => format!("the game's exe is gone from beside {}", paks.display()),
        Why::FilesGone { files } => {
            let be = if files.len() > 1 { "are" } else { "is" };
            format!("{} {be} gone from the beta's folder", files.join(", "))
        }
    }
}

fn check_why(c: &CheckWhy) -> String {
    match c {
        CheckWhy::FirstRun => "first run".into(),
        CheckWhy::OtherBuild => "setup.json names another build".into(),
        CheckWhy::OtherFolder => "another beta folder".into(),
        CheckWhy::Changed { why: w } => why(w),
    }
}

/// A line of the worker.
pub fn say(s: &Say) -> String {
    match s {
        Say::Error { detail } => detail.clone(),
        Say::NoGameFiles { dir } => format!("no game files in {}", dir.display()),
        Say::NoBetaChosen => "no beta folder has been chosen: choose one in setup".into(),
        Say::Unusable { why: w } => why(w),
        Say::KeyUnread { exe, detail } => format!("the key could not be read from {}: {detail}", exe.display()),
        Say::SetupUnwritten { file, detail } => format!("{} could not be written: {detail}", file.display()),
        Say::Checking { paks, why } => format!("checking the beta folder {} ({})", paks.display(), check_why(why)),
        Say::Matches { build, secs } => format!("the beta folder matches build {build} ({secs:.1} s)"),
        Say::Checked { paks, at } => format!("beta folder {}, checked {at}", paks.display()),
        Say::Importing { store } => format!("importing into {}", store.display()),
        Say::KindBuilt { kind, version, entries, bytes, failed, secs } => {
            let failed = if *failed > 0 { format!(", {failed} failed") } else { String::new() };
            format!("{kind} v{version}: {entries} entries, {:.1} MB{failed}, {secs:.1} s", *bytes as f64 / 1e6)
        }
        Say::ItemFailed { kind, path, detail } => format!("{kind} {path}: {detail}"),
        Say::MoreFailures { count } => format!("... and {count} more failures"),
        Say::KindNotBuilt { kind, detail } => format!("{kind}: not built: {detail}"),
        Say::KindsNotBuilt { kinds } => format!("kinds not built: {}", kinds.join(", ")),
        Say::ImportDone { secs } => format!("import done in {secs:.1} s"),
        Say::Cancelled => "import cancelled: the store keeps every kind switched in before it".into(),
        Say::WorkerStopped { code } => {
            let code = code.map_or("none".to_string(), |c| c.to_string());
            format!(
                "the import process stopped (exit code {code}); start again to finish: the kinds already imported are kept"
            )
        }
        Say::NoAnswer => "the import process answered nothing".into(),
    }
}

// The Online tab's sentences (launcher.ts drawOnline / probe / savePort).
pub const NOT_AN_ADDRESS: &str = "Enter a valid address, such as relay.example.org. Add :port at the end if the \
                                  server does not use port 7777.";
pub const LOOKING_RELAY: &str = "Connecting to the relay server…";
pub const EPIC_OFF: &str = "Disabled.";
pub const LAN_OFF: &str = "Disabled.";
pub const DIRECT_ON: &str = "Enabled. Players in your lobby can see your IP address.";
pub const DIRECT_OFF: &str =
    "Disabled. Games with friends are routed through a relay, and your IP address stays hidden.";
pub const RELAY_OFF: &str = "Disabled.";
pub const RELAY_NO_ADDRESS: &str = "No server address.";
/// The sign-in lines while the game runs (launcher.ts drawBackend / drawLan, the game's net state).
pub const EPIC_SIGNED_IN: &str = "Signed in to Epic Online Services.";
pub const LAN_ON: &str = "Enabled.";
pub const LAN_NO_NETWORK: &str = "This PC is not on a network: nobody can join a game it hosts.";
pub fn relay_connected(address: &str) -> String {
    format!("Connected to the relay server at {address}.")
}
/// The relay reached runs another version of the game (`theirs`, this game `ours`): the look's line, the running
/// game's and the test's.
pub fn relay_other_version(theirs: u32, ours: u32) -> String {
    format!(
        "Connected, but this server runs another game version ({theirs}, this game {ours}). Custom games and matchmaking will not use it."
    )
}
/// A sign-in under way: the reason as a sentence.
pub fn sign_in_pending(why: &str) -> String {
    sentence(why)
}
pub fn not_signed_in(why: &str) -> String {
    format!("Not signed in: {why}.")
}
pub fn lan_not_on(why: &str) -> String {
    format!("Not running: {why}.")
}
/// `moved`: a listener is not on the setting's port (it was in use).
pub fn lan_listening(at: &[String], port: u16, moved: bool) -> String {
    let moved = if moved { format!(" (port {port} was in use)") } else { String::new() };
    format!("Listening on {}{moved}.", at.join(", "))
}
pub fn lan_not_listening(why: &str) -> String {
    format!("Not listening: {why}.")
}
pub fn lan_ready(address: &str, port: u16) -> String {
    format!("Ready: a custom game you host listens on {address}:{port} (or the next free port).")
}
/// The first letter up and a full stop at the end (launcher.ts sentence).
fn sentence(why: &str) -> String {
    let mut c = why.chars();
    let mut s: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    if !s.ends_with('.') && !s.ends_with('…') {
        s.push('.');
    }
    s
}
pub const RELAY_NONE_SAVED: &str = "Relay server removed";
pub const RELAY_ON_SAVED: &str = "Relay server enabled";
pub const RELAY_OFF_SAVED: &str = "Relay server disabled";
pub const EPIC_ON_SAVED: &str = "Epic Online Services enabled";
pub const EPIC_OFF_SAVED: &str = "Epic Online Services disabled";
pub const LAN_OFF_SAVED: &str = "LAN play disabled";
pub const DIRECT_ON_SAVED: &str = "Direct connections enabled";
pub const DIRECT_OFF_SAVED: &str = "Direct connections disabled";
pub const EM_ONLINE: &str = "Online";

pub fn relay_saved(t: &str) -> String {
    format!("Relay server set to {t}")
}
pub fn lan_saved(port: u16) -> String {
    format!("LAN port set to {port}")
}
pub fn lan_on_saved(port: u16) -> String {
    format!("LAN play enabled on port {port}")
}
pub fn bad_port(discovery: u16) -> String {
    format!("Enter a port from 1 to 65535 (the default is 7777). Port {discovery} is reserved for finding LAN games.")
}
pub fn not_reached(why: &str, saved: bool) -> String {
    let tail = if saved {
        "The address is kept, and the game will keep trying to connect."
    } else {
        "Press Enter to use this address anyway."
    };
    format!("Could not connect: {why}. {tail}")
}
/// The probe's answer: its hello's time, the relay's wire version, whether UDP got through.
pub fn reached(ms: u64, version: u32, udp: bool) -> String {
    let way = if udp {
        "Matches go over UDP."
    } else {
        "UDP does not get through (a firewall on the way?): matches go over TCP, which can lag more."
    };
    format!("Connected ({ms} ms, wire version {version}). {way}")
}
/// The look's relay proved another key than the one expected (its id `id`): the release's own relay (the game will
/// not connect to it), one played on before (a second Enter trusts the new key).
pub fn relay_other_key_release(id: &str) -> String {
    format!(
        "This is the game's relay server address, but the server there did not prove the key this version expects (it proved relay id {id}). It may not be the game's server: the game will not connect to it."
    )
}
pub fn relay_other_key_known(id: &str) -> String {
    format!(
        "This relay server proved another key than when you last played on it (now relay id {id}). Its owner may have changed it, or it may not be the same server. Press Enter to trust the new key."
    )
}
pub fn relay_key_trusted(t: &str) -> String {
    format!("Relay server at {t} trusted with its new key")
}
pub fn not_saved(e: &str) -> String {
    format!("The setting could not be saved: {e}")
}

// The Online tab's summary and the default relay (the port's own).
pub const SUMMARY_CUSTOM: &str = "Custom games";
pub const SUMMARY_QUEUE: &str = "Matchmaking";
/// A way of the summary: its words, and what they mean ('' none), dim after them.
pub fn way(w: Way) -> (&'static str, &'static str) {
    match w {
        Way::Relay => ("Relay server", ""),
        Way::Epic => ("Epic Online Services", ""),
        Way::Lan => ("Local network", ""),
        Way::Local => ("Local lobby", "this PC and bots only"),
        Way::SigningIn => ("Signing in…", ""),
        Way::Off => ("Not available", "needs the relay server or Epic"),
        Way::SignedOut => ("Not available", "not signed in"),
        Way::RelayVersion => ("Not available", "the relay server runs another game version"),
    }
}
/// A change the running game takes once its lobby or queue closes.
pub const APPLIES_LATER: &str = "Applies when you leave the lobby";
/// Under the relay's box: online.json's relay, and where it is hosted when the file says.
pub fn relay_default(address: &str, region: &str) -> String {
    if region.is_empty() { format!("Default: {address}") } else { format!("Default: {address} (hosted in {region})") }
}
pub const USE_DEFAULT: &str = "Use default";
/// What the default relay's region means for a player far from it.
pub fn relay_region(region: &str) -> String {
    format!(
        "Far from {region}? You still play: a far match goes over Epic when it is on, and the lobby shows the ping."
    )
}

// The Online tab's Test connection and its Advanced fold (the port's own).
pub const ADVANCED: &str = "Advanced";
pub const TEST_CONNECTION: &str = "Test connection";
/// The test's box before a test has run.
pub const TEST_HINT: &str = "Press Test connection to check the relay server, UDP, Epic Online Services and your LAN \
                             port, and to see which way online play will go.";
/// The status card before the game runs or a test is done.
pub const LINE_UNTESTED: &str = "Not tested yet";
pub const TESTING: &str = "Testing the connection…";
pub const COPY_TEST: &str = "Copy result";
pub const TEST_COPIED: &str = "Test result copied to the clipboard";
pub const EM_TEST: &str = "Test";
pub fn test_failed(why: &str) -> String {
    format!("The test could not run: {why}.")
}
pub fn test_relay_reached(ms: u64) -> String {
    format!("Relay server reached ({ms} ms).")
}
pub fn test_relay_not_reached(why: &str) -> String {
    format!("Relay server not reached: {why}.")
}
/// The relay reached proved another key than the one expected (`release`: the release's own relay).
pub fn test_relay_other_key(release: bool, id: &str) -> String {
    let whose = if release {
        "the key this version of the game expects"
    } else {
        "the key it proved when you last played on it"
    };
    format!("Relay server reached, but it proved another key than {whose} (relay id {id}): the game will not use it.")
}
pub const TEST_NO_RELAY: &str = "No relay server set.";
pub const TEST_UDP_ON: &str = "UDP gets through: matches go over UDP.";
pub const TEST_UDP_OFF: &str = "UDP is blocked, likely by a firewall: matches will go over TCP, which can lag more.";
pub const TEST_EPIC_ON: &str = "Signed in to Epic Online Services.";
pub fn test_epic_off(why: &str) -> String {
    format!("Epic Online Services: {why}.")
}
pub const TEST_LAN_OFF: &str = "LAN play is disabled.";
pub fn test_lan_free(port: u16) -> String {
    format!("LAN port {port} is free.")
}
pub fn test_lan_in_use(port: u16) -> String {
    format!("LAN port {port} is in use by another program: a game you host takes the next free port.")
}
pub const TEST_USES_RELAY: &str = "Online play will use the relay server.";
pub const TEST_USES_RELAY_SLOWER: &str = "Online play will use the relay server over TCP.";
pub const TEST_USES_EPIC: &str = "Online play will use Epic Online Services.";
pub const TEST_USES_LAN: &str = "Online play is limited to custom games on your local network.";
pub const TEST_USES_NOTHING: &str = "Online play is unavailable with these settings.";
/// The test child ended with no result (`test_failed`'s why).
pub const TEST_STOPPED: &str = "it stopped without a result";
/// The test child ran past its time and was stopped (`test_failed`'s why).
pub const TEST_TOO_LONG: &str = "it did not finish within 15 seconds";
/// The relay's probe thread ended with no answer (the relay's line's why).
pub const TEST_PROBE_STOPPED: &str = "the probe stopped";
/// Epic still signing in when the test stopped waiting for it.
pub const TEST_EPIC_SIGNING_IN: &str = "Still signing in to Epic Online Services after 8 seconds.";
pub fn test_lan_game(port: u16) -> String {
    format!("LAN port {port} is in use by the game, which hosts LAN games on it.")
}
/// PLAY while a Test connection runs (the test's Epic sign-in would take the game's place).
pub const PLAY_TESTING: &str = "TESTING";

// Join links (mvsclassic://join/<code>; the port's own).
pub fn link_not_registered(e: &str) -> String {
    format!("Join links could not be set up: {e}")
}
pub fn link_bad(why: &str) -> String {
    format!("That join link does not hold a lobby code: {why}.")
}
pub const LINK_NOT_READY: &str = "MVS Classic is not ready to play yet: finish the setup, then open the link again.";
pub fn link_game_failed(e: &str) -> String {
    format!("The game could not be started for the join link: {e}")
}
/// Linux: the box when no graphics device can be made (no Vulkan driver DXVK can use).
pub fn no_vulkan(e: &str) -> String {
    format!(
        "MVS Classic could not start its graphics: {e}\n\nOn Linux the game draws through DXVK, which needs a \
         working Vulkan driver for your graphics card. Install or update your Vulkan driver (Mesa, or your card \
         maker's) and start the game again."
    )
}

// Updates (the launcher's lines, and why a check or a download did not finish).
pub const CHECKING_UPDATES: &str = "Checking for updates";
pub const CHECK_UPDATES: &str = "Check for updates";
pub const PLAY_NOW: &str = "Play without updating";
pub const UPDATE: &str = "UPDATE";
pub const UPDATING: &str = "UPDATING";
pub const EM_UPDATE: &str = "Update";
pub const UP_TO_DATE: &str = "MVS Classic is up to date";
pub const INSTALLS_SOON: &str = "Press UPDATE to install it, or play without updating from the menu";
pub const UPDATING_STATUS: &str = "Updating";
pub const STARTS_AGAIN: &str = "The launcher will restart when the update is complete";
pub fn is_out(v: &str) -> String {
    format!("MVS Classic {v} is out")
}
pub fn downloading(v: &str) -> String {
    format!("Downloading MVS Classic {v}")
}
pub fn installing(v: &str) -> String {
    format!("MVS Classic {v} downloaded, installing")
}
pub fn check_unfinished(why: &str) -> String {
    format!("The update check did not finish: {why}")
}
pub fn update_unfinished(why: &str) -> String {
    format!("The update did not finish: {why}")
}
pub const CLOSE: &str = "Close";

// Patch notes (the port's own): Home's cards of the latest releases, and a release's notes in a window over Home.
pub const PATCH_NOTES: &str = "Patch notes";
pub const NOTE_NEW: &str = "New";
pub const NOTE_INSTALLED: &str = "Installed";
pub const NOTE_EMPTY: &str = "This release has no notes.";
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// The day a release came out (`month` 1..12).
pub fn note_day(year: u32, month: u32, day: u32) -> String {
    format!("{day} {} {year}", MONTHS[(month.clamp(1, 12) - 1) as usize])
}
pub fn notes_unread(why: &str) -> String {
    format!("The patch notes could not be read: {why}")
}
pub const NO_FEED: &str = "this build names no update feed";
pub const NOTHING_STAGED: &str = "no release downloaded yet";
pub const NOTHING_FOUND: &str = "no update found to download: check for updates first";
pub const NO_EXE_DIR: &str = "this program's folder was not found";
pub const LATEST_TOO_LONG: &str = "it is too long";
pub const DOWNLOAD_HASH: &str = "the download is not the release the feed names (its SHA-256 differs)";
pub fn bad_feed_url(url: &str) -> String {
    format!("the update feed {url} is not an https address (plain http only on this computer)")
}
pub fn feed_unreached(why: &str) -> String {
    format!("the update feed could not be reached ({why})")
}
pub fn feed_answered(status: u32) -> String {
    format!("the update feed answered {status}")
}
pub fn download_answered(status: u32) -> String {
    format!("the download answered {status}")
}
pub fn latest_unread(why: &str) -> String {
    format!("the update feed's release does not read: {why}")
}
pub fn download_short(got: u64, size: u64) -> String {
    format!("the download ended at {got} of {size} bytes")
}
pub fn download_long(size: u64) -> String {
    format!("the download is longer than the {size} bytes the feed names")
}
pub fn not_unpacked(why: &str) -> String {
    format!("the update did not unpack: {why}")
}
pub fn no_exe(exe: &str) -> String {
    format!("the release has no {exe}")
}
pub fn not_under(from: &str, target: &str) -> String {
    format!("{from} is not a release unpacked under {target}")
}
