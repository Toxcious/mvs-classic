//! (the port's own) The launcher's Test connection: a child `mvs.exe --net-test` (EOS cannot start twice in one
//! process) tries each way online on the player's settings for a few seconds and prints the result as JSON: the
//! relay reached and its round trip (or the other key it proved than the one the game expects of it), UDP through
//! or not, Epic signed in or why not, the LAN port free or in use, and what online play will use. While the game
//! runs, Epic's and LAN's lines are its net state's. A relay whose welcome names another game protocol than this
//! game's is reached but not used (the net service skips it: service.rs open).

use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mvs_net::backend::BackendKind;
use mvs_net::link::relay::{ClientOptions, NetFaults, RelayProbe, probe_relay};
use mvs_net::service::api::{BackendState, OffWhy, OnlineBackends, RelayPin};
use mvs_net::service::{ServiceOptions, spawn};
use mvs_protocol::version::PROTOCOL;
use mvs_ui::launcher::{Note, Tone, words as w};
use mvs_ui::menu::online_text::off_why_text;
use serde::{Deserialize, Serialize};

/// What the test is run on: the data folder (the player's settings) and the release's online defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestOptions {
    pub data: PathBuf,
    pub online_defaults: PathBuf,
    /// `--direct-loopback`: the LAN port bound on 127.0.0.1 alone (the tests' runs: a bind on every address makes
    /// Windows ask for a firewall rule for each new exe path).
    pub loopback: bool,
}

/// The LAN port as the test found it (None in the settings: LAN off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "port", rename_all = "snake_case")]
pub enum LanPort {
    #[default]
    Off,
    Free(u16),
    InUse(u16),
    /// The running game's LAN host listens on it (the port it really holds).
    Game(u16),
}

/// What online play will use, as one sentence of the result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestUses {
    /// The relay, its matches over UDP.
    Relay,
    /// The relay by TCP alone (slower).
    RelaySlower,
    /// Epic Online Services (no relay reached).
    Epic,
    /// LAN alone: custom games on the local network.
    Lan,
    /// Nothing: online play is off.
    #[default]
    Nothing,
}

/// The test's result (the child's JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    /// The relay's hello time (ms) and wire version, or why it was not reached ('' none set).
    pub relay: Result<(u64, u32), String>,
    /// A UDP probe came back (None: no relay reached).
    pub udp: Option<bool>,
    pub epic: Result<(), OffWhy>,
    pub lan: LanPort,
    pub uses: TestUses,
    /// The relay reached proved another key than the one the game expects of it (`RelayOptions::pin`): whether it is
    /// the release's relay, and the id it proved. The game will not use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_key: Option<(bool, String)>,
    /// The address tested and the game protocol of the cores the relay reached hosts, as its welcome said (None: no
    /// relay set, or it did not say).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_proto: Option<u32>,
}

impl Default for TestResult {
    /// Nothing tested.
    fn default() -> Self {
        TestResult {
            relay: Err(String::new()),
            udp: None,
            epic: Err(OffWhy::NotOn),
            lan: LanPort::Off,
            uses: TestUses::Nothing,
            other_key: None,
            relay_address: None,
            relay_proto: None,
        }
    }
}

impl TestResult {
    /// The relay reached runs another game protocol than this game's: its number.
    pub fn relay_other_version(&self) -> Option<u32> {
        self.relay_proto.filter(|p| self.relay.is_ok() && *p != PROTOCOL)
    }
}

/// The longest the test waits for Epic's sign-in.
const EPIC_WAIT: Duration = Duration::from_secs(8);
/// Epic's sign-in is looked at this often while it is under way.
const EPIC_POLL: Duration = Duration::from_millis(50);

/// Runs the test on `o` (the child's; blocks for up to EPIC_WAIT and a little).
pub fn run(o: &TestOptions) -> TestResult {
    test(o, true)
}

/// The test: the relay probed on a thread while Epic signs in on a service of its own (with `all`; else Epic and
/// LAN are left as not tested, for the running game's net state to fill), the LAN port bound.
fn test(o: &TestOptions, all: bool) -> TestResult {
    let settings = mvs_settings::Settings::load(&o.data);
    let (b, _) =
        crate::online::glue::backends_of(&o.online_defaults, &settings, &o.data, false, false, NetFaults::default());
    let address = b.relay.as_ref().map(|r| r.address.clone());
    let pin = b.relay.as_ref().and_then(|r| r.pin.clone());
    let (probe, epic) = std::thread::scope(|s| {
        let probe = s.spawn(|| relay_test(address.as_deref()));
        let epic = if all { epic_test(&b) } else { Err(OffWhy::NotOn) };
        (probe.join().unwrap_or_else(|_| Some(RelayProbe::Failed(w::TEST_PROBE_STOPPED.to_owned()))), epic)
    });
    let (relay, udp, proved, relay_proto) = match probe {
        None => (Err(String::new()), None, String::new(), None),
        Some(RelayProbe::Found { version, relay, ms, udp, proto }) => (Ok((ms, version)), Some(udp), relay, proto),
        Some(RelayProbe::Failed(why)) => (Err(why), None, String::new(), None),
    };
    let lan = if all { lan_test(b.lan.as_ref().map(|l| l.port), o.loopback) } else { LanPort::Off };
    let other_key = other_key_of(pin.as_ref(), relay.is_ok(), proved);
    let mut r =
        TestResult { relay, udp, epic, lan, uses: TestUses::Nothing, other_key, relay_address: address, relay_proto };
    r.uses = uses_of(&r);
    r
}

/// The relay's probe (the launcher's look at an address: its hello and a UDP probe), under the game's product and
/// version: the id of the key it proved and the game protocol its welcome named among what it found. None: no relay
/// set.
fn relay_test(address: Option<&str>) -> Option<RelayProbe> {
    let app = format!("{} {}", crate::online::glue::PRODUCT, mvs_release::VERSION);
    address.map(|a| probe_relay(a, &ClientOptions::new(&app, Instant::now())))
}

/// The relay reached proved another id than the game's pin of it (`reached`: the probe reached it): whether the pin
/// is the release's, and the id proved. (The probe is unpinned: the game's own sign-in refuses the relay.)
fn other_key_of(pin: Option<&RelayPin>, reached: bool, proved: String) -> Option<(bool, String)> {
    pin.filter(|p| reached && p.id != proved).map(|p| (p.release, proved))
}

/// Epic's sign-in on a net service of its own with Epic alone in it, waited for up to EPIC_WAIT: Ok once signed in,
/// else its last reason (still signing in at the end: SigningIn). Epic off in the settings: NotOn.
fn epic_test(b: &OnlineBackends) -> Result<(), OffWhy> {
    let Some(epic) = b.epic.clone() else { return Err(OffWhy::NotOn) };
    let handle = spawn(ServiceOptions::default());
    let only = OnlineBackends {
        relay: None,
        epic: Some(epic),
        lan: None,
        allow_direct: false,
        default_relay: b.default_relay.clone(),
        known_relays: Vec::new(),
    };
    let direct = mvs_net::service::api::DirectOptions {
        key_file: PathBuf::new(),
        product: crate::online::glue::PRODUCT.to_owned(),
        version: mvs_release::VERSION.to_owned(),
        loopback: false,
    };
    handle.backends(only, direct);
    let end = Instant::now() + EPIC_WAIT;
    let mut last = OffWhy::SigningIn;
    while Instant::now() < end {
        if let Some(s) = handle.state().into_iter().find(|s| s.kind == BackendKind::Epic) {
            match epic_state(&s) {
                Some(done) => {
                    handle.quit();
                    return done;
                }
                None => last = s.why.unwrap_or(OffWhy::SigningIn),
            }
        }
        std::thread::sleep(EPIC_POLL);
    }
    handle.quit();
    Err(last)
}

/// A backend's sign-in as the test's answer: Some once it is in or off for good, None while it signs in.
fn epic_state(s: &BackendState) -> Option<Result<(), OffWhy>> {
    if s.online {
        return Some(Ok(()));
    }
    match s.why.clone().unwrap_or(OffWhy::SigningIn) {
        OffWhy::SigningIn | OffWhy::Connecting { .. } | OffWhy::SettingsChanged | OffWhy::SignInChanged => None,
        // (signin.rs's lane before its options came: off, signing in next)
        OffWhy::Off if s.on => None,
        why => Some(Err(why)),
    }
}

/// The LAN port bound for TCP and UDP on every IPv4 address (`loopback`: on 127.0.0.1 alone) and let go: free when
/// both bind (a running LAN host holds both). None: LAN off.
// Deviation: the port is checked on this PC alone; whether a firewall lets others reach it is not.
pub(crate) fn lan_test(port: Option<u16>, loopback: bool) -> LanPort {
    let Some(port) = port else { return LanPort::Off };
    let ip = if loopback { Ipv4Addr::LOCALHOST } else { Ipv4Addr::UNSPECIFIED };
    let tcp = TcpListener::bind((ip, port)).is_ok();
    let udp = UdpSocket::bind((ip, port)).is_ok();
    if tcp && udp { LanPort::Free(port) } else { LanPort::InUse(port) }
}

/// What online play will use: the relay first (by UDP, else the slower way; not one of another key or another game
/// version), then Epic, then LAN alone.
pub fn uses_of(r: &TestResult) -> TestUses {
    if r.relay.is_ok() && r.other_key.is_none() && r.relay_other_version().is_none() {
        return if r.udp == Some(true) { TestUses::Relay } else { TestUses::RelaySlower };
    }
    if r.epic.is_ok() {
        return TestUses::Epic;
    }
    if r.lan != LanPort::Off {
        return TestUses::Lan;
    }
    TestUses::Nothing
}

/// The result as the launcher's lines: the relay, UDP (once a relay answered), Epic, LAN, then what online uses.
pub fn notes(r: &TestResult) -> Vec<Note> {
    let mut out = vec![match (&r.relay, &r.other_key, r.relay_other_version()) {
        (Ok(_), Some((release, id)), _) => Note::new(w::test_relay_other_key(*release, id), Tone::Bad),
        (Ok(_), None, Some(p)) => Note::new(w::relay_other_version(p, PROTOCOL), Tone::Warn),
        (Ok((ms, _)), None, None) => Note::new(w::test_relay_reached(*ms), Tone::Ok),
        (Err(why), _, _) if why.is_empty() => Note::new(w::TEST_NO_RELAY, Tone::Plain),
        (Err(why), _, _) => Note::new(w::test_relay_not_reached(why), Tone::Bad),
    }];
    // (no UDP line for a relay the game will not use)
    match r.udp.filter(|_| r.other_key.is_none() && r.relay_other_version().is_none()) {
        Some(true) => out.push(Note::new(w::TEST_UDP_ON, Tone::Ok)),
        // (a relay UDP does not reach is a plain line, not a bad one)
        Some(false) => out.push(Note::new(w::TEST_UDP_OFF, Tone::Plain)),
        None => {}
    }
    out.push(match &r.epic {
        Ok(()) => Note::new(w::TEST_EPIC_ON, Tone::Ok),
        // (still on its way in when the test stopped waiting: not a failure)
        Err(OffWhy::SigningIn | OffWhy::Connecting { .. } | OffWhy::SettingsChanged | OffWhy::SignInChanged) => {
            Note::new(w::TEST_EPIC_SIGNING_IN, Tone::Plain)
        }
        Err(why @ (OffWhy::NotOn | OffWhy::TurnedOff | OffWhy::Off)) => {
            Note::new(w::test_epic_off(&off_why_text(BackendKind::Epic, why)), Tone::Plain)
        }
        Err(why) => Note::new(w::test_epic_off(&off_why_text(BackendKind::Epic, why)), Tone::Bad),
    });
    out.push(match r.lan {
        LanPort::Off => Note::new(w::TEST_LAN_OFF, Tone::Plain),
        LanPort::Free(p) => Note::new(w::test_lan_free(p), Tone::Ok),
        LanPort::InUse(p) => Note::new(w::test_lan_in_use(p), Tone::Plain),
        LanPort::Game(p) => Note::new(w::test_lan_game(p), Tone::Ok),
    });
    out.push(match r.uses {
        TestUses::Relay => Note::new(w::TEST_USES_RELAY, Tone::Ok),
        TestUses::RelaySlower => Note::new(w::TEST_USES_RELAY_SLOWER, Tone::Plain),
        TestUses::Epic => Note::new(w::TEST_USES_EPIC, Tone::Ok),
        TestUses::Lan => Note::new(w::TEST_USES_LAN, Tone::Plain),
        TestUses::Nothing => Note::new(w::TEST_USES_NOTHING, Tone::Bad),
    });
    out
}

/// The result as text, for the Copy button (a bug report's lines): the release's version, then the lines.
pub fn as_text(r: &TestResult) -> String {
    let mut s = format!("mvs {}\n", mvs_release::VERSION);
    for n in notes(r) {
        s += &n.text;
        s.push('\n');
    }
    s
}

/// `mvs --net-test [--data <dir>] [--online-defaults <file>] [--relay-only] [--out <file>]`: the test, its result as
/// one JSON line on stdout. `--relay-only`: the relay alone (the launcher's while the game runs, whose net state has
/// Epic's and LAN's: a second Epic sign-in would take the game's place, its LAN host holds the port). `--out`: the
/// JSON written to that file too (the launcher's: on Linux the EOS SDK prints lines of its own on stdout); a file not
/// written is one line on stderr, and stdout still carries the JSON.
pub fn main(mut args: impl Iterator<Item = String>) -> u8 {
    let mut data = None;
    let mut online_defaults = None;
    let mut all = true;
    let mut loopback = false;
    let mut out = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "--online-defaults" => online_defaults = args.next().map(PathBuf::from),
            "--relay-only" => all = false,
            "--direct-loopback" => loopback = true,
            "--out" => out = args.next().map(PathBuf::from),
            _ => {
                eprintln!(
                    "mvs: usage: mvs --net-test [--data <dir>] [--online-defaults <file>] [--relay-only] \
                     [--direct-loopback] [--out <file>]"
                );
                return 2;
            }
        }
    }
    let o = TestOptions {
        data: mvs_settings::data_dir(data.as_deref()),
        online_defaults: online_defaults.unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(mvs_release::online_json))
                .unwrap_or_else(|| PathBuf::from(mvs_release::ONLINE_JSON))
        }),
        loopback,
    };
    match serde_json::to_string(&test(&o, all)) {
        Ok(j) => {
            println!("{j}");
            // (not written: stdout still carries the JSON, so the run did not fail)
            if let Some((f, Err(e))) = out.as_ref().map(|f| (f, std::fs::write(f, &j))) {
                eprintln!("mvs: --net-test: {}: {e}", f.display());
            }
            0
        }
        Err(e) => {
            eprintln!("mvs: --net-test: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(
        relay: Result<(u64, u32), String>,
        udp: Option<bool>,
        epic: Result<(), OffWhy>,
        lan: LanPort,
    ) -> TestResult {
        let mut r = TestResult { relay, udp, epic, lan, ..TestResult::default() };
        r.uses = uses_of(&r);
        r
    }

    /// A relay reached whose welcome names another game protocol: a warning line, no UDP line, online does not use
    /// it; one naming this game's, or none (an older relay), is used.
    #[test]
    fn a_relay_of_another_game_version_is_reported() {
        let mut r = result(Ok((42, 7)), Some(true), Ok(()), LanPort::Off);
        r.relay_address = Some("relay.example".into());
        r.relay_proto = Some(PROTOCOL - 1);
        r.uses = uses_of(&r);
        assert_eq!(r.relay_other_version(), Some(PROTOCOL - 1));
        assert_eq!(r.uses, TestUses::Epic);
        let n = notes(&r);
        assert_eq!(n[0], Note::new(w::relay_other_version(PROTOCOL - 1, PROTOCOL), Tone::Warn));
        assert_eq!(n.len(), 4, "no UDP line: {n:?}");
        let j = serde_json::to_string(&r).expect("json");
        assert_eq!(serde_json::from_str::<TestResult>(&j).expect("read"), r);
        for p in [Some(PROTOCOL), None] {
            r.relay_proto = p;
            r.uses = uses_of(&r);
            assert_eq!((r.relay_other_version(), r.uses), (None, TestUses::Relay), "{p:?}");
            assert_eq!(notes(&r)[0], Note::new(w::test_relay_reached(42), Tone::Ok));
        }
        // (no relay reached: nothing said of its version)
        let mut down = result(Err("refused".into()), None, Ok(()), LanPort::Off);
        down.relay_proto = Some(PROTOCOL - 1);
        assert_eq!(down.relay_other_version(), None);
        // (an older launcher's JSON, with neither field, reads)
        let old = r#"{"relay":{"Ok":[42,7]},"udp":true,"epic":{"Ok":null},"lan":{"state":"off"},"uses":"relay"}"#;
        let o: TestResult = serde_json::from_str(old).expect("read");
        assert_eq!((o.relay_address, o.relay_proto), (None, None));
    }

    /// A relay reached that proves another key than the game's pin: a bad line, no UDP line, online does not use it.
    #[test]
    fn a_relay_with_another_key_is_reported() {
        let id_a = "r0123456789abcdef0123456789abcdef";
        let id_b = "rfedcba9876543210fedcba9876543210";
        let pin = RelayPin { id: id_a.into(), release: true };
        assert_eq!(other_key_of(Some(&pin), true, id_a.into()), None);
        assert_eq!(other_key_of(Some(&pin), false, String::new()), None);
        assert_eq!(other_key_of(None, true, id_b.into()), None);
        let other = other_key_of(Some(&pin), true, id_b.into());
        assert_eq!(other, Some((true, id_b.to_string())));
        let mut r = result(Ok((42, 7)), Some(true), Ok(()), LanPort::Off);
        r.other_key = other;
        r.uses = uses_of(&r);
        assert_eq!(r.uses, TestUses::Epic);
        let n = notes(&r);
        assert_eq!(n[0], Note::new(w::test_relay_other_key(true, id_b), Tone::Bad));
        assert_eq!(n.len(), 4);
        let j = serde_json::to_string(&r).expect("json");
        assert_eq!(serde_json::from_str::<TestResult>(&j).expect("read"), r);
    }

    /// What online uses, and the lines: a relay with UDP through, with UDP closed, down; Epic off; LAN alone.
    #[test]
    fn the_result_says_what_online_uses() {
        let up = result(Ok((42, 9)), Some(true), Err(OffWhy::NotOn), LanPort::Off);
        assert_eq!(up.uses, TestUses::Relay);
        let udp_closed = result(Ok((42, 9)), Some(false), Ok(()), LanPort::Free(7777));
        assert_eq!(udp_closed.uses, TestUses::RelaySlower);
        let n = notes(&udp_closed);
        assert_eq!(n[1], Note::new(w::TEST_UDP_OFF, Tone::Plain));
        assert_eq!(n.last(), Some(&Note::new(w::TEST_USES_RELAY_SLOWER, Tone::Plain)));
        let down = result(Err("nothing answers at 127.0.0.1:1".into()), None, Ok(()), LanPort::Off);
        assert_eq!(down.uses, TestUses::Epic);
        assert_eq!(notes(&down)[0].tone, Tone::Bad);
        // no UDP line without a relay reached
        assert_eq!(notes(&down).len(), 4);
        let missing = OffWhy::Missing { why: mvs_net::service::api::EpicMissing::NotSetUp };
        let lan = result(Err(String::new()), None, Err(missing), LanPort::InUse(7777));
        assert_eq!(lan.uses, TestUses::Lan);
        let n = notes(&lan);
        assert_eq!(n[0], Note::new(w::TEST_NO_RELAY, Tone::Plain));
        assert_eq!(n[1].tone, Tone::Bad);
        assert_eq!(n[2], Note::new(w::test_lan_in_use(7777), Tone::Plain));
        let nothing = result(Err(String::new()), None, Err(OffWhy::NotOn), LanPort::Off);
        assert_eq!(nothing.uses, TestUses::Nothing);
        let text = as_text(&nothing);
        assert!(text.starts_with("mvs ") && text.contains(w::TEST_USES_NOTHING), "{text}");
        // the JSON the launcher reads back
        let j = serde_json::to_string(&udp_closed).expect("json");
        assert_eq!(serde_json::from_str::<TestResult>(&j).expect("read"), udp_closed);
    }

    #[test]
    fn a_port_held_is_in_use() {
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bound");
        let port = held.local_addr().expect("address").port();
        assert_eq!(lan_test(Some(port), true), LanPort::InUse(port));
        drop(held);
        assert_eq!(lan_test(None, true), LanPort::Off);
    }

    #[test]
    fn epic_signing_in_is_waited_for() {
        let state = |on, online, why| BackendState { kind: BackendKind::Epic, on, online, why };
        assert_eq!(epic_state(&state(true, true, None)), Some(Ok(())));
        assert_eq!(epic_state(&state(true, false, Some(OffWhy::SigningIn))), None);
        assert_eq!(epic_state(&state(true, false, Some(OffWhy::Off))), None);
        let why = OffWhy::Backend { text: "EOS_InvalidAuth".into() };
        assert_eq!(epic_state(&state(true, false, Some(why.clone()))), Some(Err(why)));
        // still signing in at the end of the wait: a plain line, Epic named once
        let r = result(Err(String::new()), None, Err(OffWhy::SigningIn), LanPort::Off);
        assert_eq!(notes(&r)[1], Note::new(w::TEST_EPIC_SIGNING_IN, Tone::Plain));
    }
}
