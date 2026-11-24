//! The launcher's Online tab ("Online (Settings > Online)"): the relay's switch and address, Epic, LAN
//! and its port and the direct links over online.json's, saved into the player's settings. The look at a typed
//! address runs on a thread and only the newest look's answer is shown or saved.
//!
//! The look at an address is the relay client's probe (mvs_net probe_relay): the wire hello
//! under a key of its own, the relay's wire version, the id of the key it proved and whether UDP gets through. The id
//! is checked against the one expected: online.json's relayId for the release's relay (another: a bad line, nothing
//! kept), the one kept for a relay the player said yes to (another: a bad line, and Enter again trusts the new key);
//! a relay not known before is kept with its id once reached (Settings.online's known relays: the game connects to
//! it pinned to that id).
//! While the game the launcher started runs, the sign-in lines of the relay, Epic and LAN are its net service's
//! (mvs_net NetState, read from the data folder's NET_STATE_FILE).
//! Deviation: the net service is the game's, so with no game running the lines say only what each switch says (a
//! net process does not run with the launcher).
//! At the top (the port's own) where custom games and matchmaking go (`summary_of_config`, `summary_of_state`); under
//! it the menus' one line of online play, from the game's net state while it runs, else from the last Test connection
//! (net_test.rs, a child process), whose lines it lists.
//! A change the player makes reaches a running game through the settings file (glue.rs follow_settings): at once in
//! the menus, else once its lobby or queue closes (NetState::held, "Applies when you leave the lobby").
//! A relay whose welcome names another game protocol than this game's (or whose lobby did, while the game runs:
//! NetState::relay_proto) is reached but not used: its lines warn, and the summary leaves it out (with no game, from
//! the newest look or test of the address in use).

use std::io::Read;
use std::path::Path;
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use mvs_net::backend::BackendKind;
use mvs_net::link::relay::{ClientOptions, RelayProbe, probe_relay};
use mvs_net::service::api::{BackendOff, NET_STATE_FILE, NetState, OffWhy, StatusWhy};
use mvs_ui::menu::online::{OfflineWhy, OnlineLine};
use mvs_ui::menu::online_line::{self, LineTone};
use mvs_ui::menu::online_text::{host_start_why_text, off_why_text};

use mvs_protocol::relay_wire::{KnownRelay, RELAY_PORT, parse_address};
use mvs_protocol::version::PROTOCOL;
use mvs_settings::online::{
    LAN_DISCOVERY_PORT, OnlineConfig, OnlineSetting, effective_online, read_online_config, relay_address, same_address,
};
use mvs_ui::launcher::{Note, OnlineSettingsView, OnlineSummary, Tone, Way, words as w};

use super::game::GameArgs;
use super::net_test::{self, LanPort, TestResult, TestUses};
use crate::online::hub::{LineLatch, line_of};
use crate::online::port::NetStatus;

/// One look at an address: the probe's answer.
type Look = RelayProbe;

/// The game's net state is read again this often while it runs.
const STATE_EVERY: Duration = Duration::from_millis(500);
/// The test child is stopped when it runs longer (its own waits end in about 8 s).
const TEST_LIMIT: Duration = Duration::from_secs(15);
/// The test child is looked at this often while it runs.
const TEST_POLL: Duration = Duration::from_millis(50);

/// A Test connection under way: its child (killed past TEST_LIMIT, or when the launcher quits), its answer, and
/// whether it tested the relay alone (a game ran) with what started it (a full test again if the game quit).
#[derive(Debug)]
struct Testing {
    child: Arc<Mutex<Child>>,
    answer: Receiver<Result<TestResult, String>>,
    relay_only: bool,
    game: GameArgs,
}

#[derive(Debug)]
pub struct OnlineTab {
    /// online.json's values.
    file: OnlineConfig,
    probe: Option<(u32, String, Receiver<Look>)>,
    seq: u32,
    status: Note,
    relay_bad: bool,
    port_bad: bool,
    lan_line: Note,
    /// The characters typed since the last frame (the boxes').
    pub typed: Vec<char>,
    /// The relay box's text as last typed.
    pub relay_text: String,
    /// The running game's net state (None: no game, or none written since it started).
    net: Option<NetState>,
    state_read: Option<Instant>,
    /// The Advanced fold is open.
    advanced: bool,
    /// The Test connection under way.
    test: Option<Testing>,
    /// The last test's result, or why it could not run.
    tested: Option<Result<TestResult, String>>,
    /// The line from the game's net state (read_state), kept at not reached through the relay's retries.
    game_line: Option<OnlineLine>,
    latch: LineLatch,
    /// The newest look's relay and the id it proved, kept by the next `set_relay` of its address.
    found: Option<KnownRelay>,
    /// The newest look's relay, known with another id than it proved: kept by an Enter on its address (`set_relay`
    /// before the look again), dropped at the next text.
    other_key: Option<KnownRelay>,
    /// The newest look or test that reached a relay: its address and the game protocol its welcome named (None: it
    /// did not say). The summary with no game leaves out a relay of another version at the address in use.
    relay_seen: Option<(String, Option<u32>)>,
}

impl Drop for OnlineTab {
    /// A test child under way ends with the launcher.
    fn drop(&mut self) {
        if let Some(t) = &self.test {
            let _ = lock(&t.child).kill();
        }
    }
}

fn lock(c: &Arc<Mutex<Child>>) -> std::sync::MutexGuard<'_, Child> {
    c.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The relay's probe at `text`, under the game's product and version (the game's own sign-in's `app`).
fn look(text: &str) -> Look {
    let app = format!("{} {}", crate::online::glue::PRODUCT, mvs_release::VERSION);
    probe_relay(text, &ClientOptions::new(&app, Instant::now()))
}

/// A port a LAN game may listen on: 1 to 65535 but the discovery port.
fn port_of(text: &str) -> Option<u16> {
    let t = text.trim();
    if t.is_empty() || t.len() > 5 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = t.parse().ok()?;
    u16::try_from(n).ok().filter(|p| *p != 0 && *p != LAN_DISCOVERY_PORT)
}

impl OnlineTab {
    pub fn new(file: &Path) -> OnlineTab {
        OnlineTab {
            file: read_online_config(file, &mut |_| {}),
            probe: None,
            seq: 0,
            status: Note::default(),
            relay_bad: false,
            port_bad: false,
            lan_line: Note::default(),
            typed: Vec::new(),
            relay_text: String::new(),
            net: None,
            state_read: None,
            advanced: false,
            test: None,
            tested: None,
            game_line: None,
            latch: LineLatch::default(),
            found: None,
            other_key: None,
            relay_seen: None,
        }
    }

    /// The game's net state from `data`, at most every STATE_EVERY; `since` the game's start (None: no game runs).
    /// A file older than the game is a run before's (one that did not end cleanly) and is not read.
    pub fn read_state(&mut self, data: &Path, since: Option<SystemTime>) {
        let Some(since) = since else {
            self.net = None;
            self.state_read = None;
            self.game_line = None;
            return;
        };
        if self.state_read.is_some_and(|t| t.elapsed() < STATE_EVERY) {
            return;
        }
        self.state_read = Some(Instant::now());
        let file = data.join(NET_STATE_FILE);
        let fresh = std::fs::metadata(&file).and_then(|m| m.modified()).is_ok_and(|m| m >= since);
        self.net = fresh
            .then(|| std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<NetState>(&b).ok()))
            .flatten();
        // (the update and self checks are the game's own and not in its net state: no gate)
        self.game_line = self.net.as_ref().map(|n| {
            let status = status_of(n);
            self.latch.follow(line_of(&status, None, n.udp == Some(false)), &status)
        });
    }

    /// A backend's sign-in line: None when the game's state does not name it.
    fn backend_line(&self, kind: BackendKind, up: &str, off: Note) -> Option<Note> {
        let b = self.net.as_ref()?.backends.iter().find(|b| b.kind == kind)?;
        Some(if !b.on {
            off
        } else if b.online {
            Note::new(up, Tone::Ok)
        } else {
            let why = b.why.as_ref().unwrap_or(&OffWhy::Off);
            let text = off_why_text(kind, why);
            if matches!(why, OffWhy::SigningIn | OffWhy::Connecting { .. }) {
                Note::new(w::sign_in_pending(&text), Tone::Plain)
            } else {
                Note::new(w::not_signed_in(&text), Tone::Bad)
            }
        })
    }

    /// LAN's line: what a custom game this PC hosts listens on, or why it does not.
    fn lan_line_now(&self) -> Option<Note> {
        let net = self.net.as_ref()?;
        let b = net.backends.iter().find(|b| b.kind == BackendKind::Lan)?;
        if !b.on {
            return Some(Note::new(w::LAN_OFF, Tone::Plain));
        }
        if !b.online {
            let why = off_why_text(BackendKind::Lan, b.why.as_ref().unwrap_or(&OffWhy::Off));
            return Some(Note::new(w::lan_not_on(&why), Tone::Bad));
        }
        let Some(d) = &net.direct else { return Some(Note::new(w::LAN_ON, Tone::Ok)) };
        Some(if !d.listening.is_empty() {
            let moved = d.listening.iter().any(|l| !l.ends_with(&format!(":{}", d.port)));
            Note::new(w::lan_listening(&d.listening, d.port, moved), Tone::Ok)
        } else if let Some(why) = &d.why {
            Note::new(w::lan_not_listening(&host_start_why_text(why)), Tone::Bad)
        } else if let Some(a) = d.addresses.first() {
            Note::new(w::lan_ready(a, d.port), Tone::Ok)
        } else {
            Note::new(w::LAN_NO_NETWORK, Tone::Bad)
        })
    }

    pub fn effective(&self, s: &OnlineSetting) -> OnlineConfig {
        effective_online(&self.file, s)
    }

    /// What the tab draws from the player's setting.
    pub fn view(&self, s: &OnlineSetting) -> OnlineSettingsView {
        let c = self.effective(s);
        let address = relay_address(&self.file, s).unwrap_or_default();
        let relay_now = if c.relay.is_some() {
            Note::default()
        } else if s.relay_on == Some(false) {
            Note::new(w::RELAY_OFF, Tone::Plain)
        } else {
            Note::new(w::RELAY_NO_ADDRESS, Tone::Plain)
        };
        let default = self.file.relay.clone().unwrap_or_default();
        let region = self.file.relay_region.clone().unwrap_or_default();
        // (what the region means, while the default relay is on: another relay's region is not known)
        let far = !region.is_empty() && c.relay.as_deref().is_some_and(|r| same_address(r, &default));
        let lan_now = if c.lan.is_none() {
            Note::new(w::LAN_OFF, Tone::Plain)
        } else if self.lan_line.text.is_empty() {
            self.lan_line_now().unwrap_or_default()
        } else {
            self.lan_line.clone()
        };
        // (the relay the game signed into when it runs: its own setting is read at its start, the launcher's may have changed)
        let signed = self.net.as_ref().and_then(|n| n.relay.as_deref()).or(c.relay.as_deref()).unwrap_or_default();
        let relay_up = w::relay_connected(signed);
        let relay_now = self.backend_line(BackendKind::Relay, &relay_up, relay_now.clone()).unwrap_or(relay_now);
        // (signed in to a relay of another game version: reached, not used)
        let relay_now = match self.net.as_ref().and_then(|n| n.relay_proto) {
            Some(p) if relay_now.tone == Tone::Ok => Note::new(w::relay_other_version(p, PROTOCOL), Tone::Warn),
            _ => relay_now,
        };
        let epic_off = if c.eos { Note::default() } else { Note::new(w::EPIC_OFF, Tone::Plain) };
        let epic_now = self.backend_line(BackendKind::Epic, w::EPIC_SIGNED_IN, epic_off.clone()).unwrap_or(epic_off);
        let (line, line_detail) = self.line(&c);
        OnlineSettingsView {
            summary: self.net.as_ref().map_or_else(|| summary_of_config(&c, self.relay_other(&c)), summary_of_state),
            held: self.net.as_ref().is_some_and(|n| n.held),
            relay: address,
            relay_on: c.relay.is_some(),
            relay_far: if far { w::relay_region(&region) } else { String::new() },
            relay_default: default,
            relay_region: region,
            eos: c.eos,
            lan: c.lan,
            lan_port: c.lan.or(self.file.lan).unwrap_or(RELAY_PORT).to_string(),
            direct: c.direct,
            relay_status: self.status.clone(),
            relay_now,
            epic_now,
            lan_now,
            direct_now: Note::new(if c.direct { w::DIRECT_ON } else { w::DIRECT_OFF }, Tone::Plain),
            relay_bad: self.relay_bad,
            port_bad: self.port_bad,
            line,
            line_detail,
            test: match &self.tested {
                Some(Ok(r)) => net_test::notes(r),
                Some(Err(why)) => vec![Note::new(w::test_failed(why), Tone::Bad)],
                None => Vec::new(),
            },
            testing: self.test.is_some(),
            advanced: self.advanced,
        }
    }

    /// The newest look or test found the relay `c` uses running another game version.
    fn relay_other(&self, c: &OnlineConfig) -> bool {
        match (&self.relay_seen, &c.relay) {
            (Some((at, Some(p))), Some(r)) => *p != PROTOCOL && same_address(at, r),
            _ => false,
        }
    }

    /// The one line of online play: the game's while it runs (its net state; the update and self checks are the
    /// game's own and not in it), else the last test's, else "turned off" when every switch is off (None shown
    /// before a test).
    fn line(&self, c: &OnlineConfig) -> (Note, String) {
        let line = if self.net.is_some() {
            self.game_line.clone()
        } else if let Some(Ok(r)) = &self.tested {
            Some(line_of_test(r))
        } else if c.relay.is_none() && !c.eos && c.lan.is_none() {
            Some(OnlineLine::Offline(OfflineWhy::TurnedOff))
        } else {
            None
        };
        line.map(|l| line_note(&l)).unwrap_or_default()
    }

    /// Test connection: a child `mvs.exe --net-test` on the game's data and online defaults (net_test), its result
    /// taken by `poll_test`. While the game runs the child tests the relay alone and Epic and LAN are the game's.
    pub fn start_test(&mut self, game: &GameArgs) {
        if self.test.is_some() {
            return;
        }
        // (the game's instance mutex: a game just started, or one this launcher did not start, counts)
        let data = mvs_settings::data_dir(game.data.as_deref());
        let relay_only = crate::online::link::running_game(&data).is_some();
        let mut cmd = std::process::Command::new(&game.exe);
        cmd.arg("--net-test").arg("--online-defaults").arg(&game.online_defaults);
        if let Some(d) = &game.data {
            cmd.arg("--data").arg(d);
        }
        if relay_only {
            cmd.arg("--relay-only");
        }
        if let Some(d) = game.exe.parent() {
            cmd.current_dir(d);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up for the child
            cmd.creation_flags(0x0800_0000);
        }
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        self.tested = None;
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.tested = Some(Err(e.to_string()));
                return;
            }
        };
        let stdout = child.stdout.take();
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = mpsc::channel();
        let waited = child.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run_child(&waited, stdout));
        });
        self.test = Some(Testing { child, answer: rx, relay_only, game: game.clone() });
    }

    /// A Test connection runs (PLAY waits for it: its Epic sign-in would take the game's place).
    pub fn testing(&self) -> bool {
        self.test.is_some()
    }

    /// The test's result once the child gave it: true when the view changed. A relay-only test takes Epic's and
    /// LAN's lines from the game's net state.
    /// A relay-only test whose game quit before the answer runs again in full (the game's state is gone, and Epic and
    /// LAN are free to test).
    pub fn poll_test(&mut self) -> bool {
        let Some(t) = &self.test else { return false };
        let got = match t.answer.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err(w::TEST_STOPPED.to_owned()),
        };
        let Some(t) = self.test.take() else { return false };
        if let Ok(r) = &got {
            self.relay_seen = r.relay_address.clone().filter(|_| r.relay.is_ok()).map(|a| (a, r.relay_proto));
        }
        if t.relay_only && got.is_ok() {
            if let Some(n) = &self.net {
                self.tested = Some(got.map(|mut r| {
                    game_lines(&mut r, n);
                    r
                }));
                return true;
            }
            let data = mvs_settings::data_dir(t.game.data.as_deref());
            if crate::online::link::running_game(&data).is_none() {
                self.start_test(&t.game);
                return true;
            }
        }
        self.tested = Some(got);
        true
    }

    /// The last test's result as text for the clipboard (None: no test run yet).
    pub fn copy_test(&self) -> Option<String> {
        match self.tested.as_ref()? {
            Ok(r) => Some(net_test::as_text(r)),
            Err(why) => Some(w::test_failed(why)),
        }
    }

    pub fn toggle_advanced(&mut self) {
        self.advanced = !self.advanced;
    }

    /// Saves the box's address as the relay (empty: none), when it is one, with the id the newest look found it
    /// proving (a relay known with another id: only after that look's bad line, an Enter trusting the new key). The
    /// log line when the setting changed.
    pub fn set_relay(&mut self, s: &mut OnlineSetting, text: &str) -> Option<String> {
        let t = text.trim();
        let (value, mut said) = if t.is_empty() {
            (None, w::RELAY_NONE_SAVED.to_string())
        } else if parse_address(t).is_some() {
            (Some(t.to_string()), w::relay_saved(t))
        } else {
            return None;
        };
        let next = (self.file.relay != value).then_some(value);
        let mut changed = s.relay != next;
        s.relay = next;
        // (only while the look's bad line stands for this very text: any typing drops it, `typed`)
        if let Some(k) = self.other_key.take().filter(|k| k.address == t)
            && s.know_relay(&self.file, k)
        {
            said = w::relay_key_trusted(t);
            changed = true;
        }
        if let Some(k) = self.found.take().filter(|k| same_address(&k.address, t)) {
            changed |= s.know_relay(&self.file, k);
        }
        changed.then_some(said)
    }

    /// The relay's switch: off keeps the address (D6); on with no address takes online.json's.
    pub fn toggle_relay(&self, s: &mut OnlineSetting) -> String {
        if self.effective(s).relay.is_some() {
            s.relay_on = Some(false);
            return w::RELAY_OFF_SAVED.to_string();
        }
        s.relay_on = None;
        if relay_address(&self.file, s).is_none() {
            s.relay = None;
        }
        w::RELAY_ON_SAVED.to_string()
    }

    pub fn toggle_eos(&self, s: &mut OnlineSetting) -> String {
        let on = !self.effective(s).eos;
        s.eos = (self.file.eos != on).then_some(on);
        if on { w::EPIC_ON_SAVED } else { w::EPIC_OFF_SAVED }.to_string()
    }

    pub fn toggle_direct(&self, s: &mut OnlineSetting) -> String {
        let on = !self.effective(s).direct;
        s.direct = (self.file.direct != on).then_some(on);
        if on { w::DIRECT_ON_SAVED } else { w::DIRECT_OFF_SAVED }.to_string()
    }

    /// The LAN switch: off when on; on with the port box's port (else online.json's, else the relay's).
    pub fn toggle_lan(&mut self, s: &mut OnlineSetting, port_text: &str) -> String {
        if self.effective(s).lan.is_some() {
            s.lan = self.file.lan.is_some().then_some(None);
            return w::LAN_OFF_SAVED.to_string();
        }
        let port = port_of(port_text).or(self.file.lan).unwrap_or(RELAY_PORT);
        s.lan = (self.file.lan != Some(port)).then_some(Some(port));
        self.port_bad = false;
        self.lan_line = Note::default();
        w::lan_on_saved(port)
    }

    /// The port box's text, saved when it is a port.
    pub fn set_port(&mut self, s: &mut OnlineSetting, text: &str) -> Option<String> {
        let Some(port) = port_of(text) else {
            self.port_bad = true;
            self.lan_line = Note::new(w::bad_port(LAN_DISCOVERY_PORT), Tone::Bad);
            return None;
        };
        self.port_bad = false;
        self.lan_line = Note::default();
        let next = (self.file.lan != Some(port)).then_some(Some(port));
        (s.lan != next).then(|| s.lan = next).map(|()| w::lan_saved(port))
    }

    /// The relay box changed: an answer for the text before is stale now.
    pub fn typed(&mut self, text: &str) {
        self.seq += 1;
        self.probe = None;
        self.found = None;
        self.other_key = None;
        self.relay_bad = !text.trim().is_empty() && parse_address(text).is_none();
        self.status = if self.relay_bad { Note::new(w::NOT_AN_ADDRESS, Tone::Bad) } else { Note::default() };
    }

    /// Looks at the address on a thread (nothing for an empty or bad one).
    pub fn start_look(&mut self, text: &str) {
        self.typed(text);
        if text.trim().is_empty() || self.relay_bad {
            return;
        }
        self.status = Note::new(w::LOOKING_RELAY, Tone::Plain);
        let (tx, rx) = mpsc::channel();
        let t = text.trim().to_string();
        std::thread::spawn(move || {
            let _ = tx.send(look(&t));
        });
        self.probe = Some((self.seq, text.to_string(), rx));
    }

    /// The newest look's answer, if it came: the text to save when it reached the relay.
    pub fn poll(&mut self, s: &OnlineSetting) -> Option<String> {
        let (seq, text, rx) = self.probe.as_ref()?;
        let r = rx.try_recv().ok()?;
        let (seq, text) = (*seq, text.clone());
        self.probe = None;
        if seq != self.seq || text != self.relay_text {
            return None;
        }
        self.answer(s, text, r)
    }

    /// The id the relay at `address` must prove: online.json's relayId for the release's relay (true), else the one
    /// kept for a relay the player said yes to (false); None: any.
    fn expected(&self, s: &OnlineSetting, address: &str) -> Option<(String, bool)> {
        if let (Some(f), Some(id)) = (&self.file.relay, &self.file.relay_id)
            && same_address(f, address)
        {
            return Some((id.clone(), true));
        }
        s.known_relay(address).and_then(|k| k.id.clone()).map(|id| (id, false))
    }

    /// The look's answer for `text`: the line, and the text to save when it reached the relay and the relay proved
    /// the key expected of it (or none was).
    fn answer(&mut self, s: &OnlineSetting, text: String, r: Look) -> Option<String> {
        match r {
            RelayProbe::Failed(why) => {
                self.relay_seen = None;
                let saved = self.effective(s).relay.as_deref() == Some(text.trim());
                self.status = Note::new(w::not_reached(&why, saved), Tone::Bad);
                None
            }
            RelayProbe::Found { version, relay, ms, udp, proto } => {
                let address = text.trim();
                match self.expected(s, address) {
                    Some((want, true)) if want != relay => {
                        self.status = Note::new(w::relay_other_key_release(&relay), Tone::Bad);
                        return None;
                    }
                    Some((want, false)) if want != relay => {
                        self.status = Note::new(w::relay_other_key_known(&relay), Tone::Bad);
                        self.other_key = Some(KnownRelay { address: address.to_string(), id: Some(relay) });
                        return None;
                    }
                    _ => {}
                }
                self.relay_seen = Some((address.to_string(), proto));
                // (a relay UDP does not reach is a plain line, not a good one; one of another game version a warning:
                // its address is kept all the same, the game skips it)
                self.status = match proto.filter(|p| *p != PROTOCOL) {
                    Some(p) => Note::new(w::relay_other_version(p, PROTOCOL), Tone::Warn),
                    None => Note::new(w::reached(ms, version, udp), if udp { Tone::Ok } else { Tone::Plain }),
                };
                // (a probe that read no key id keeps nothing: the id is what the game is pinned to)
                if mvs_protocol::relay_wire::is_key_id(&relay) {
                    self.found = Some(KnownRelay { address: address.to_string(), id: Some(relay) });
                }
                Some(text)
            }
        }
    }
}

/// (the port's own) Where custom games and matchmaking go with no game running, from the switches (the net service's
/// order, service.rs open: the relay, then Epic, then LAN, else a local lobby; the queue on the relay or Epic alone).
/// `relay_other`: the newest look or test found the relay running another game version, which the game skips.
// Unverified: whether each one signs in is not known before the game runs; the summary says where it would go.
pub fn summary_of_config(c: &OnlineConfig, relay_other: bool) -> OnlineSummary {
    let relay = c.relay.is_some() && !relay_other;
    let custom = if relay {
        Way::Relay
    } else if c.eos {
        Way::Epic
    } else if c.lan.is_some() {
        Way::Lan
    } else {
        Way::Local
    };
    let queue = if relay {
        Way::Relay
    } else if c.eos {
        Way::Epic
    } else if relay_other && c.relay.is_some() {
        Way::RelayVersion
    } else {
        Way::Off
    };
    OnlineSummary { custom, queue }
}

/// (the port's own) Where custom games and matchmaking go while the game runs, from its net state: the first of the
/// order signed in (LAN: listening); else signing in while one on still is; else a local lobby, and matchmaking not
/// available (signed out when the relay or Epic is on, else off). A relay signed in that runs another game version
/// (`NetState::relay_proto`) is left out: matchmaking then says why when Epic is not signed in.
pub fn summary_of_state(n: &NetState) -> OnlineSummary {
    let of = |k: BackendKind| n.backends.iter().find(|b| b.kind == k && b.on);
    let signed = |k: BackendKind| of(k).is_some_and(|b| b.online);
    let other = n.relay_proto.is_some() && signed(BackendKind::Relay);
    let up = |k: BackendKind| signed(k) && !(k == BackendKind::Relay && other);
    let pending = |k: BackendKind| {
        of(k).is_some_and(|b| !b.online && matches!(b.why, None | Some(OffWhy::SigningIn | OffWhy::Connecting { .. })))
    };
    let all = [BackendKind::Relay, BackendKind::Epic, BackendKind::Lan];
    let custom = if up(BackendKind::Relay) {
        Way::Relay
    } else if up(BackendKind::Epic) {
        Way::Epic
    } else if up(BackendKind::Lan) {
        Way::Lan
    } else if all.into_iter().any(pending) {
        Way::SigningIn
    } else {
        Way::Local
    };
    let online = [BackendKind::Relay, BackendKind::Epic];
    let queue = if up(BackendKind::Relay) {
        Way::Relay
    } else if up(BackendKind::Epic) {
        Way::Epic
    } else if online.into_iter().any(pending) {
        Way::SigningIn
    } else if other {
        Way::RelayVersion
    } else if online.into_iter().any(|k| of(k).is_some()) {
        Way::SignedOut
    } else {
        Way::Off
    };
    OnlineSummary { custom, queue }
}

/// The game's sign-in as its net state has it (the menus' NetStatus): online while any backend is, else each one
/// off with its reason (none yet: signing in).
fn status_of(n: &NetState) -> NetStatus {
    if n.backends.iter().any(|b| b.online) {
        return NetStatus { online: true, me: String::new(), why: StatusWhy::Online };
    }
    let backends: Vec<BackendOff> =
        n.backends.iter().filter_map(|b| b.why.clone().map(|why| BackendOff { kind: b.kind, why })).collect();
    let why = if backends.is_empty() { StatusWhy::SigningIn } else { StatusWhy::Off { backends } };
    NetStatus { online: false, me: String::new(), why }
}

/// The line a test's result gives: what online uses, else why not.
fn line_of_test(r: &TestResult) -> OnlineLine {
    match r.uses {
        TestUses::Relay | TestUses::Epic | TestUses::Lan => OnlineLine::Online,
        TestUses::RelaySlower => OnlineLine::Slower,
        TestUses::Nothing => {
            let off = |e: &Result<(), OffWhy>| matches!(e, Err(OffWhy::NotOn | OffWhy::TurnedOff | OffWhy::Off));
            OnlineLine::Offline(match &r.relay {
                Err(why) if why.is_empty() && off(&r.epic) => OfflineWhy::TurnedOff,
                Err(why) if !why.is_empty() => OfflineWhy::NotReached,
                _ => OfflineWhy::Other,
            })
        }
    }
}

/// The line as the launcher's card: the headline as a note, and under it what it means or what to do (on the Online
/// tab itself, "turned off" says no more).
fn line_note(l: &OnlineLine) -> (Note, String) {
    let detail = match l {
        OnlineLine::Offline(OfflineWhy::TurnedOff) => String::new(),
        _ => online_line::advice(l).unwrap_or_default(),
    };
    let head = online_line::headline(l);
    let tone = match online_line::tone(l) {
        LineTone::Good => Tone::Ok,
        LineTone::Plain => Tone::Plain,
        LineTone::Bad => Tone::Bad,
    };
    (Note::new(head, tone), detail)
}

/// A relay-only test's Epic and LAN from the running game's net state: Epic signed in or its reason; LAN's port free
/// while the game's LAN host listens on it (its own games take it), in use when it could not; what online uses again.
fn game_lines(r: &mut TestResult, n: &NetState) {
    let of = |k: BackendKind| n.backends.iter().find(|b| b.kind == k);
    r.epic = match of(BackendKind::Epic) {
        Some(b) if b.online => Ok(()),
        Some(b) if b.on => Err(b.why.clone().unwrap_or(OffWhy::SigningIn)),
        _ => Err(OffWhy::NotOn),
    };
    r.lan = match (of(BackendKind::Lan), &n.direct) {
        (Some(b), Some(d)) if b.on => match d.listening.iter().find_map(|a| port_of_address(a)) {
            Some(p) => LanPort::Game(p),
            // (the game's host does not listen: whether another program holds the port)
            None => net_test::lan_test(Some(d.port), false),
        },
        _ => LanPort::Off,
    };
    r.uses = net_test::uses_of(r);
}

/// The port of a listening address ("192.168.1.20:7777").
fn port_of_address(a: &str) -> Option<u16> {
    a.rsplit_once(':')?.1.parse().ok()
}

/// The child's run: waited for up to TEST_LIMIT (then killed), its last line of stdout read as the result.
fn run_child(child: &Arc<Mutex<Child>>, stdout: Option<std::process::ChildStdout>) -> Result<TestResult, String> {
    // (read on a thread of its own: a full pipe would hold the child)
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut o) = stdout {
            let _ = o.read_to_string(&mut text);
        }
        text
    });
    let end = Instant::now() + TEST_LIMIT;
    loop {
        if lock(child).try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if Instant::now() >= end {
            let _ = lock(child).kill();
            let _ = lock(child).wait();
            return Err(w::TEST_TOO_LONG.to_owned());
        }
        std::thread::sleep(TEST_POLL);
    }
    let text = reader.join().unwrap_or_default();
    let line = text.lines().rev().find(|l| !l.trim().is_empty()).ok_or_else(|| w::TEST_STOPPED.to_owned())?;
    serde_json::from_str(line).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use mvs_net::service::api::{BackendState, DirectStatus};

    use super::*;

    #[test]
    fn the_games_net_state_gives_the_sign_in_lines() {
        let mut tab = OnlineTab::new(Path::new("no such online.json"));
        let s = OnlineSetting {
            relay: Some(Some("relay.example.org".into())),
            eos: Some(true),
            lan: Some(Some(7777)),
            ..OnlineSetting::default()
        };
        // no game: the switches' lines alone
        let v = tab.view(&s);
        assert!(v.relay_now.text.is_empty() && v.epic_now.text.is_empty() && v.lan_now.text.is_empty());
        let state = |kind, online, why| BackendState { kind, on: true, online, why };
        tab.net = Some(NetState {
            backends: vec![
                state(BackendKind::Relay, true, None),
                state(BackendKind::Epic, false, Some(OffWhy::SigningIn)),
                state(BackendKind::Lan, true, None),
            ],
            direct: Some(DirectStatus {
                addresses: vec!["192.168.1.5".into()],
                port: 7777,
                listening: vec!["192.168.1.5:7778".into()],
                why: None,
            }),
            relay: Some("relay.example.org".into()),
            udp: Some(true),
            held: false,
            relay_proto: None,
        });
        let v = tab.view(&s);
        assert_eq!(v.relay_now, Note::new(w::relay_connected("relay.example.org"), Tone::Ok));
        assert_eq!(v.epic_now.tone, Tone::Plain);
        assert!(v.epic_now.text.ends_with('.') && v.epic_now.text.starts_with(|c: char| c.is_uppercase()));
        assert_eq!(v.lan_now, Note::new(w::lan_listening(&["192.168.1.5:7778".into()], 7777, true), Tone::Ok));
        tab.net.as_mut().expect("set").backends[1] =
            state(BackendKind::Epic, false, Some(OffWhy::Backend { text: "EOS_InvalidAuth".into() }));
        assert_eq!(tab.view(&s).epic_now, Note::new(w::not_signed_in("EOS_InvalidAuth"), Tone::Bad));
        // a relay-only test beside the game: Epic's reason and LAN's port as the game holds them (it moved to 7778)
        let mut r = TestResult { relay: Ok((42, 1)), udp: Some(true), ..TestResult::default() };
        game_lines(&mut r, tab.net.as_ref().expect("set"));
        assert_eq!(r.epic, Err(OffWhy::Backend { text: "EOS_InvalidAuth".into() }));
        assert_eq!(r.lan, LanPort::Game(7778));
        assert_eq!(r.uses, TestUses::Relay);
    }

    const ID_A: &str = "r0123456789abcdef0123456789abcdef";
    const ID_B: &str = "rfedcba9876543210fedcba9876543210";

    fn found(id: &str) -> Look {
        RelayProbe::Found { version: 7, relay: id.into(), ms: 20, udp: true, proto: Some(PROTOCOL) }
    }

    /// A tab over an online.json whose relay is `relay.example` pinned to ID_A.
    fn pinned_tab() -> OnlineTab {
        let mut tab = OnlineTab::new(Path::new("no such online.json"));
        tab.file =
            OnlineConfig { relay: Some("relay.example".into()), relay_id: Some(ID_A.into()), ..tab.file.clone() };
        tab
    }

    /// The look's answer as the launcher's frame takes it: the text saved when it comes back (launcher.rs frame).
    fn look_answer(tab: &mut OnlineTab, s: &mut OnlineSetting, text: &str, r: Look) -> Option<String> {
        tab.relay_text = text.into();
        tab.typed(text);
        let text = tab.answer(s, text.to_string(), r)?;
        tab.set_relay(s, &text)
    }

    /// The release's address proving another id: a bad line, nothing saved; proving its own: nothing kept (its id is
    /// online.json's).
    #[test]
    fn the_releases_relay_must_prove_its_id() {
        let mut tab = pinned_tab();
        let mut s = OnlineSetting::default();
        assert_eq!(look_answer(&mut tab, &mut s, "RELAY.example:7777", found(ID_B)), None);
        assert_eq!(tab.status, Note::new(w::relay_other_key_release(ID_B), Tone::Bad));
        assert_eq!(s, OnlineSetting::default());
        // an Enter does not trust it
        assert_eq!(tab.set_relay(&mut s, "relay.example"), None);
        assert!(s.known_relays.is_empty());
        let said = look_answer(&mut tab, &mut s, "relay.example", found(ID_A));
        assert_eq!(said, None);
        assert_eq!(tab.status.tone, Tone::Ok);
        assert!(s.known_relays.is_empty());
    }

    /// A new address is kept with the id it proved; a known one proving another id: a bad line, then Enter trusts it.
    #[test]
    fn a_relays_new_key_is_trusted_by_a_second_enter() {
        let mut tab = pinned_tab();
        let mut s = OnlineSetting::default();
        let said = look_answer(&mut tab, &mut s, "mine.example:9000", found(ID_A));
        assert_eq!(said, Some(w::relay_saved("mine.example:9000")));
        assert_eq!(s.relay, Some(Some("mine.example:9000".into())));
        assert_eq!(s.known_relays, vec![KnownRelay { address: "mine.example:9000".into(), id: Some(ID_A.into()) }]);
        // the same id again: nothing changes
        assert_eq!(look_answer(&mut tab, &mut s, "mine.example:9000", found(ID_A)), None);
        // another id: the bad line, nothing kept
        assert_eq!(look_answer(&mut tab, &mut s, "Mine.example:9000", found(ID_B)), None);
        assert_eq!(tab.status, Note::new(w::relay_other_key_known(ID_B), Tone::Bad));
        assert_eq!(s.known_relays[0].id.as_deref(), Some(ID_A));
        // more typing drops it; Enter on the address after the bad line trusts the new key
        tab.typed("Mine.example:900");
        assert_eq!(tab.set_relay(&mut s, "Mine.example:9000"), Some(w::relay_saved("Mine.example:9000")));
        assert_eq!(s.known_relays[0].id.as_deref(), Some(ID_A));
        // another spelling of the address than the one the bad line stands for: no trust
        assert_eq!(look_answer(&mut tab, &mut s, "Mine.example:9000", found(ID_B)), None);
        assert_eq!(tab.set_relay(&mut s, "mine.example:9000"), Some(w::relay_saved("mine.example:9000")));
        assert_eq!(s.known_relays[0].id.as_deref(), Some(ID_A));
        assert_eq!(look_answer(&mut tab, &mut s, "Mine.example:9000", found(ID_B)), None);
        assert_eq!(tab.set_relay(&mut s, "Mine.example:9000"), Some(w::relay_key_trusted("Mine.example:9000")));
        assert_eq!(s.known_relays[0].id.as_deref(), Some(ID_B));
        assert_eq!(s.known_relays.len(), 1);
        // a look that fails keeps nothing
        let failed = look_answer(&mut tab, &mut s, "new.example", RelayProbe::Failed("nothing answers".into()));
        assert_eq!((failed, s.known_relays.len()), (None, 1));
    }

    /// The line from the game's state stays at not reached through the relay's next try.
    #[test]
    fn the_games_line_does_not_flip_at_each_retry() {
        let dir = std::env::temp_dir().join(format!("mvs-online-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("made");
        let mut tab = OnlineTab::new(Path::new("no such online.json"));
        let relay = |why| BackendState { kind: BackendKind::Relay, on: true, online: false, why: Some(why) };
        let since = SystemTime::now() - Duration::from_secs(60);
        let read = |tab: &mut OnlineTab, why: OffWhy| {
            let st = NetState { backends: vec![relay(why)], ..NetState::default() };
            std::fs::write(dir.join(NET_STATE_FILE), serde_json::to_vec(&st).expect("json")).expect("written");
            tab.state_read = None;
            tab.read_state(&dir, Some(since));
            tab.game_line.clone()
        };
        let not_reached = Some(OnlineLine::Offline(OfflineWhy::NotReached));
        assert_eq!(read(&mut tab, OffWhy::Retrying { text: "refused".into(), in_ms: 2000 }), not_reached);
        assert_eq!(read(&mut tab, OffWhy::Connecting { address: "relay.example.org".into() }), not_reached);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn config(relay: bool, eos: bool, lan: bool) -> OnlineConfig {
        OnlineConfig {
            relay: relay.then(|| "relay.example".to_string()),
            eos,
            lan: lan.then_some(7777),
            ..OnlineConfig::default()
        }
    }

    fn summary(custom: Way, queue: Way) -> OnlineSummary {
        OnlineSummary { custom, queue }
    }

    /// With no game running: the relay, else Epic, else LAN, else a local lobby; matchmaking on the relay or Epic.
    #[test]
    fn the_summary_follows_the_switches() {
        assert_eq!(summary_of_config(&config(true, true, true), false), summary(Way::Relay, Way::Relay));
        assert_eq!(summary_of_config(&config(false, true, true), false), summary(Way::Epic, Way::Epic));
        assert_eq!(summary_of_config(&config(false, false, true), false), summary(Way::Lan, Way::Off));
        assert_eq!(summary_of_config(&config(false, false, false), false), summary(Way::Local, Way::Off));
        // (a relay of another game version: left out)
        assert_eq!(summary_of_config(&config(true, true, true), true), summary(Way::Epic, Way::Epic));
        assert_eq!(summary_of_config(&config(true, false, true), true), summary(Way::Lan, Way::RelayVersion));
        assert_eq!(summary_of_config(&config(true, false, false), true), summary(Way::Local, Way::RelayVersion));
    }

    /// A look at a relay of another game version: a warning line, the address kept all the same; the summary with no
    /// game leaves that relay out while it is the one in use, until a look or test finds it on this game's version.
    #[test]
    fn a_look_at_a_relay_of_another_version_warns() {
        let mut tab = default_tab();
        let mut s = OnlineSetting::default();
        let other = RelayProbe::Found { version: 7, relay: ID_A.into(), ms: 20, udp: true, proto: Some(PROTOCOL - 1) };
        assert_eq!(
            look_answer(&mut tab, &mut s, "mine.example:9000", other.clone()),
            Some(w::relay_saved("mine.example:9000"))
        );
        assert_eq!(tab.status, Note::new(w::relay_other_version(PROTOCOL - 1, PROTOCOL), Tone::Warn));
        let v = tab.view(&s);
        assert_eq!(v.relay_status.tone, Tone::Warn);
        assert_eq!(v.summary, summary(Way::Epic, Way::Epic));
        tab.toggle_eos(&mut s);
        assert_eq!(tab.view(&s).summary, summary(Way::Local, Way::RelayVersion));
        // (another address in use: as before)
        tab.set_relay(&mut s, "relay.example");
        assert_eq!(tab.view(&s).summary, summary(Way::Relay, Way::Relay));
        // (none said, an older relay: as before; this game's: the relay)
        for proto in [None, Some(PROTOCOL)] {
            let r = RelayProbe::Found { version: 7, relay: ID_A.into(), ms: 20, udp: true, proto };
            look_answer(&mut tab, &mut s, "mine.example:9000", r);
            assert_eq!(tab.status, Note::new(w::reached(20, 7, true), Tone::Ok));
            assert_eq!(tab.view(&s).summary, summary(Way::Relay, Way::Relay));
        }
        // (a test of the address in use says it too, and a failed look forgets it)
        tab.tested = None;
        let mut r = TestResult { relay: Ok((20, 7)), udp: Some(true), ..TestResult::default() };
        r.relay_address = Some("mine.example:9000".into());
        r.relay_proto = Some(PROTOCOL + 1);
        tab.relay_seen = r.relay_address.clone().map(|a| (a, r.relay_proto));
        assert_eq!(tab.view(&s).summary, summary(Way::Local, Way::RelayVersion));
        look_answer(&mut tab, &mut s, "mine.example:9000", RelayProbe::Failed("nothing answers".into()));
        assert_eq!(tab.view(&s).summary, summary(Way::Relay, Way::Relay));
    }

    /// While the game runs signed in to a relay of another game version: its line warns, and the summary leaves it
    /// out (matchmaking says why when Epic is not signed in).
    #[test]
    fn a_running_games_relay_of_another_version_warns() {
        let mut tab = default_tab();
        let s = OnlineSetting::default();
        let b = |kind, online, why| BackendState { kind, on: true, online, why };
        let relay_up = b(BackendKind::Relay, true, None);
        let epic_up = b(BackendKind::Epic, true, None);
        let other = |backends| NetState {
            backends,
            relay: Some("relay.example".into()),
            relay_proto: Some(PROTOCOL - 1),
            ..NetState::default()
        };
        tab.net = Some(other(vec![relay_up.clone(), epic_up]));
        let v = tab.view(&s);
        assert_eq!(v.relay_now, Note::new(w::relay_other_version(PROTOCOL - 1, PROTOCOL), Tone::Warn));
        assert_eq!(v.summary, summary(Way::Epic, Way::Epic));
        tab.net = Some(other(vec![relay_up.clone()]));
        assert_eq!(tab.view(&s).summary, summary(Way::Local, Way::RelayVersion));
        let signing = b(BackendKind::Epic, false, Some(OffWhy::SigningIn));
        assert_eq!(summary_of_state(&other(vec![relay_up.clone(), signing])), summary(Way::SigningIn, Way::SigningIn));
        // (this game's: the relay as before)
        tab.net = Some(NetState { relay_proto: None, ..other(vec![relay_up]) });
        let v = tab.view(&s);
        assert_eq!(v.relay_now, Note::new(w::relay_connected("relay.example"), Tone::Ok));
        assert_eq!(v.summary, summary(Way::Relay, Way::Relay));
    }

    /// While the game runs: the first signed in of the order, signing in while one on still is, else a local lobby
    /// and matchmaking signed out (or off when the relay and Epic are).
    #[test]
    fn the_summary_follows_the_games_net_state() {
        let b = |kind, online, why| BackendState { kind, on: true, online, why };
        let state = |backends| NetState { backends, ..NetState::default() };
        let relay_down = b(BackendKind::Relay, false, Some(OffWhy::Retrying { text: "refused".into(), in_ms: 2000 }));
        let epic_up = b(BackendKind::Epic, true, None);
        let lan_up = b(BackendKind::Lan, true, None);
        let all_up = state(vec![b(BackendKind::Relay, true, None), epic_up.clone(), lan_up.clone()]);
        assert_eq!(summary_of_state(&all_up), summary(Way::Relay, Way::Relay));
        assert_eq!(summary_of_state(&state(vec![relay_down.clone(), epic_up])), summary(Way::Epic, Way::Epic));
        assert_eq!(
            summary_of_state(&state(vec![relay_down.clone(), lan_up.clone()])),
            summary(Way::Lan, Way::SignedOut)
        );
        assert_eq!(summary_of_state(&state(vec![lan_up])), summary(Way::Lan, Way::Off));
        let signing = b(BackendKind::Epic, false, Some(OffWhy::SigningIn));
        assert_eq!(
            summary_of_state(&state(vec![relay_down.clone(), signing])),
            summary(Way::SigningIn, Way::SigningIn)
        );
        assert_eq!(summary_of_state(&state(vec![relay_down])), summary(Way::Local, Way::SignedOut));
        // (a backend in the state but not on: as if absent)
        let off = BackendState { kind: BackendKind::Relay, on: false, online: false, why: Some(OffWhy::Off) };
        assert_eq!(summary_of_state(&state(vec![off])), summary(Way::Local, Way::Off));
        assert_eq!(summary_of_state(&NetState::default()), summary(Way::Local, Way::Off));
    }

    /// A tab over an online.json whose relay is `relay.example` in Europe.
    fn default_tab() -> OnlineTab {
        let mut tab = OnlineTab::new(Path::new("no such online.json"));
        tab.file = OnlineConfig {
            relay: Some("relay.example".into()),
            relay_region: Some("Europe".into()),
            ..tab.file.clone()
        };
        tab
    }

    /// The relay's switch keeps the address typed; on with none takes the default; the default's hint and region
    /// line; "Use default" (the box's Enter on the default's address) gives the setting back to online.json.
    #[test]
    fn the_relays_switch_keeps_its_address() {
        let mut tab = default_tab();
        let mut s = OnlineSetting::default();
        let v = tab.view(&s);
        assert_eq!((v.relay.as_str(), v.relay_on), ("relay.example", true));
        assert_eq!((v.relay_default.as_str(), v.relay_region.as_str()), ("relay.example", "Europe"));
        assert_eq!(v.relay_far, w::relay_region("Europe"));
        assert_eq!(v.summary, summary(Way::Relay, Way::Relay));
        // another address: no region line
        assert_eq!(tab.set_relay(&mut s, "mine.example:9000"), Some(w::relay_saved("mine.example:9000")));
        let v = tab.view(&s);
        assert_eq!((v.relay.as_str(), v.relay_far.as_str()), ("mine.example:9000", ""));
        // off: the address kept, the summary without the relay
        assert_eq!(tab.toggle_relay(&mut s), w::RELAY_OFF_SAVED);
        let v = tab.view(&s);
        assert_eq!((v.relay.as_str(), v.relay_on), ("mine.example:9000", false));
        assert_eq!(v.relay_now, Note::new(w::RELAY_OFF, Tone::Plain));
        assert_eq!(v.relay_far, "");
        assert_eq!(v.summary, summary(Way::Epic, Way::Epic));
        assert_eq!(tab.effective(&s).relay, None);
        // on again: the same address
        assert_eq!(tab.toggle_relay(&mut s), w::RELAY_ON_SAVED);
        assert_eq!(tab.effective(&s).relay.as_deref(), Some("mine.example:9000"));
        // Use default: the setting follows online.json again
        assert_eq!(tab.set_relay(&mut s, "relay.example"), Some(w::relay_saved("relay.example")));
        assert_eq!(s.relay, None);
        // an emptied box: no relay, the switch shown off; on again takes the default
        tab.set_relay(&mut s, "");
        let v = tab.view(&s);
        assert_eq!((v.relay.as_str(), v.relay_on), ("", false));
        assert_eq!(v.relay_now, Note::new(w::RELAY_NO_ADDRESS, Tone::Plain));
        tab.toggle_relay(&mut s);
        assert_eq!(s, OnlineSetting::default());
    }

    /// The other switches as before, each against online.json's value; a running game's held change shows.
    #[test]
    fn the_switches_and_a_held_change() {
        let mut tab = default_tab();
        let mut s = OnlineSetting::default();
        assert_eq!(tab.toggle_eos(&mut s), w::EPIC_OFF_SAVED);
        assert_eq!(s.eos, Some(false));
        assert_eq!(tab.toggle_eos(&mut s), w::EPIC_ON_SAVED);
        assert_eq!(s.eos, None);
        assert_eq!(tab.toggle_lan(&mut s, "9000"), w::lan_on_saved(9000));
        assert_eq!(s.lan, Some(Some(9000)));
        assert_eq!(tab.toggle_lan(&mut s, "9000"), w::LAN_OFF_SAVED);
        assert_eq!(s.lan, None);
        assert!(!tab.view(&s).held);
        tab.net = Some(NetState { held: true, ..NetState::default() });
        let v = tab.view(&s);
        assert!(v.held);
        assert_eq!(v.summary, summary(Way::Local, Way::Off));
    }
}
