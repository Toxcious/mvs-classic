//! The EOS backend (backend/eos.rs) against the real SDK, as the oracle's EOS pair check does: two EOS
//! identities on this PC, one process each. The host signs in, opens a lobby and starts the guest (this test binary
//! again), which signs in with a fresh Device ID and joins it by its id. Then: packets both ways on both channels, the
//! round trip at 60 Hz, lobby member changes, a custom invite, kick, peer closed and the host leaving; a listed lobby
//! found by search with its attributes, joined, out of the searches once full (in a search of the full ones too), its
//! attributes changed, and once unlisted; the relay control (the oracle's relays option: what kind of link each pair of
//! modes makes, and what a mode changed while a link stands does to it); then the park (a shutdown: a request refused
//! while parked), the reopen (signed in again as the same player, a link again) and the end.
//!
//! Runs only when MVS_EOS_DLL and MVS_EOS_CREDENTIALS are set (it says so and passes otherwise); it contacts Epic's
//! servers. It prints no id and nothing of the credentials. Note: the guest's fresh Device ID replaces this Windows
//! user's for good (the next sign-in is the new identity), as the oracle's check does.
//!   cargo test -p mvs-net --test eos_pair -- --nocapture

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use indexmap::{IndexMap, IndexSet};
use serde_json::{Value, json};

use mvs_net::backend::eos::{EosFiles, EosOptions, end_eos, load_eos};
use mvs_net::backend::{
    Answer, Ask, Backend, BackendEvent, Channel, FoundLobby, Listed, LobbyChange, P2P_MAX_PACKET, RelayMode,
};

const ROLE: &str = "MVS_EOS_PAIR_ROLE";
/// Two listing texts (a custom lobby's is JSON of up to 1000 bytes).
fn info() -> String {
    json!({ "v": 1, "name": "tester", "maps": ["map_m011_largenoplat"], "pad": "x".repeat(800) }).to_string()
}
fn info_2() -> String {
    json!({ "v": 1, "name": "tester", "state": "match" }).to_string()
}
const BURST: usize = 60;
const PING_SECONDS: f64 = 5.0;
/// The relay steps' ping, and its bytes.
const PING_MS: f64 = 3000.0;
const PING: u8 = 0xaa;
const PONG: u8 = 0xbb;
/// A round trip under this is a direct link on one PC; Epic's relays answer in 15 ms and more.
const DIRECT_RTT_MS: f64 = 5.0;

fn files() -> Option<(PathBuf, PathBuf)> {
    let dll = std::env::var_os("MVS_EOS_DLL")?;
    let credentials = std::env::var_os("MVS_EOS_CREDENTIALS")?;
    Some((PathBuf::from(dll), PathBuf::from(credentials)))
}

/// A backend driven as the service drives it: polled at its next_due, its events kept.
struct Eos {
    role: &'static str,
    b: Box<dyn Backend>,
    t0: Instant,
    cache: PathBuf,
    failures: u32,
    answers: IndexMap<Ask, Result<Answer, String>>,
    changes: Vec<LobbyChange>,
    /// Each lobby's members as of its last change.
    members: IndexMap<String, Vec<String>>,
    /// Who may open a link besides the members (the guest: its host).
    extra_accept: IndexSet<String>,
    closed_peers: Vec<String>,
    invites: Vec<(String, String)>,
    packets: Vec<(String, Channel, Vec<u8>)>,
    links: Vec<(String, bool)>,
    signed_in: bool,
    signed_out: Option<String>,
    /// Every packet sent back where it came from.
    echo: bool,
    /// Pings answered, pongs timed.
    pinger: bool,
    sent_at: IndexMap<u32, f64>,
    rtts: Vec<f64>,
    seq: u32,
    /// The other process's messages.
    inbox: Vec<Value>,
    from_other: Option<Receiver<Value>>,
    to_other: Option<Box<dyn Write>>,
}

impl Eos {
    fn new(role: &'static str, fresh: bool) -> Eos {
        let (dll, credentials) = files().expect("checked by the caller");
        let cache = std::env::temp_dir().join(format!("mvs-eos-{role}-{}", std::process::id()));
        let o = EosOptions {
            files: EosFiles { dll, credentials, cache: cache.clone() },
            product: "MVS Classic".to_owned(),
            version: "test".to_owned(),
            fresh_device: fresh,
            spin: false,
        };
        let b = load_eos(&o).unwrap_or_else(|e| panic!("[{role}] load_eos: {e}"));
        let mut e = Eos {
            role,
            b,
            t0: Instant::now(),
            cache,
            failures: 0,
            answers: IndexMap::new(),
            changes: Vec::new(),
            members: IndexMap::new(),
            extra_accept: IndexSet::new(),
            closed_peers: Vec::new(),
            invites: Vec::new(),
            packets: Vec::new(),
            links: Vec::new(),
            signed_in: false,
            signed_out: None,
            echo: false,
            pinger: false,
            sent_at: IndexMap::new(),
            rtts: Vec::new(),
            seq: 0,
            inbox: Vec::new(),
            from_other: None,
            to_other: None,
        };
        let t = Instant::now();
        let ok = e.until(|e| e.signed_in || e.signed_out.is_some(), 30000);
        assert!(ok && e.signed_in, "[{role}] no sign-in: {:?}", e.signed_out);
        e.log(&format!("signed in ({} ms)", t.elapsed().as_millis()));
        e
    }

    fn log(&self, s: &str) {
        eprintln!("[{}] {s}", self.role);
    }

    fn check(&mut self, ok: bool, what: &str) {
        self.log(&format!("{} {what}", if ok { "ok  " } else { "FAIL" }));
        if !ok {
            self.failures += 1;
        }
    }

    fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64() * 1000.0
    }

    fn me(&self) -> String {
        self.b.me().to_owned()
    }

    fn set_accept(&mut self) {
        let me = self.me();
        let mut ids: IndexSet<String> = self.members.values().flatten().filter(|m| **m != me).cloned().collect();
        ids.extend(self.extra_accept.iter().cloned());
        self.b.set_accept(ids);
    }

    fn take(&mut self, e: BackendEvent) {
        match e {
            BackendEvent::Answered(ask, r) => {
                self.answers.insert(ask, r);
            }
            BackendEvent::Lobby(c) => {
                match &c {
                    LobbyChange::Members { lobby, members } => {
                        self.log(&format!("lobby change: {} members", members.len()));
                        self.members.insert(lobby.clone(), members.clone());
                    }
                    LobbyChange::Closed { lobby, why } => {
                        self.log(&format!("lobby change: closed ({why})"));
                        self.members.shift_remove(lobby);
                    }
                }
                self.changes.push(c);
                self.set_accept();
            }
            BackendEvent::Packet { from, channel, bytes } => {
                if self.echo {
                    self.b.send(&from, channel, &bytes);
                } else if self.pinger && channel == Channel::Match && bytes.len() == 8 {
                    if bytes[4] == PING {
                        let mut back = bytes.clone();
                        back[4] = PONG;
                        self.b.send(&from, Channel::Match, &back);
                    } else if bytes[4] == PONG {
                        let seq = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        if let Some(at) = self.sent_at.shift_remove(&seq) {
                            self.rtts.push(self.now() - at);
                        }
                    }
                } else {
                    self.packets.push((from, channel, bytes));
                }
            }
            BackendEvent::PeerClosed(p) => self.closed_peers.push(p),
            BackendEvent::Link { peer, relayed } => {
                if let Some(r) = relayed {
                    self.log(&format!("link {}", if r { "relayed" } else { "direct" }));
                    self.links.push((peer, r));
                }
            }
            BackendEvent::Invite { from, payload } => self.invites.push((from, payload)),
            BackendEvent::SignedIn => self.signed_in = true,
            BackendEvent::SignedOut(why) => {
                self.log(&format!("signed out: {why}"));
                self.signed_out = Some(why);
            }
            BackendEvent::Listing { .. } => {}
        }
    }

    /// Polls once at the backend's time, then sleeps to its next_due (5 ms at the most).
    fn step(&mut self) {
        let now = self.now();
        while let Some(e) = self.b.poll(now) {
            self.take(e);
        }
        if let Some(rx) = &self.from_other {
            while let Ok(m) = rx.try_recv() {
                self.inbox.push(m);
            }
        }
        let due = self.b.next_due().unwrap_or(f64::INFINITY);
        let wait = (due - self.now()).clamp(0.0, 5.0);
        if wait > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(wait / 1000.0));
        }
    }

    /// Steps until `pred` holds, `ms` at most; true when it did.
    fn until(&mut self, pred: impl Fn(&Eos) -> bool, ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        while !pred(self) {
            if Instant::now() > end {
                return false;
            }
            self.step();
        }
        true
    }

    fn pause(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step();
        }
    }

    /// The answer to `ask`, stepping until it comes (30 s at the most).
    fn answer(&mut self, ask: Ask) -> Result<Answer, String> {
        if !self.until(|e| e.answers.contains_key(&ask), 30000) {
            return Err("no answer in 30 s".to_owned());
        }
        self.answers.shift_remove(&ask).unwrap_or_else(|| Err("no answer".to_owned()))
    }

    fn create(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> String {
        let ask = self.b.create_lobby(bucket, max, listed);
        match self.answer(ask) {
            Ok(Answer::Created(id)) => {
                let me = self.me();
                self.members.insert(id.clone(), vec![me]);
                id
            }
            r => panic!("[{}] create_lobby: {r:?}", self.role),
        }
    }

    fn join(&mut self, id: &str) -> Result<(String, Vec<String>), String> {
        let ask = self.b.join_lobby(id);
        match self.answer(ask)? {
            Answer::Joined { owner, members, .. } => {
                self.members.insert(id.to_owned(), members.clone());
                self.set_accept();
                Ok((owner, members))
            }
            a => Err(format!("{a:?}")),
        }
    }

    fn leave(&mut self, id: &str) {
        let ask = self.b.leave_lobby(id);
        let r = self.answer(ask);
        let ok = matches!(r, Ok(Answer::Done));
        self.check(ok, &format!("leave_lobby answered ({})", if ok { "done".to_owned() } else { format!("{r:?}") }));
    }

    fn search(&mut self, bucket: &str, full: bool) -> Vec<FoundLobby> {
        let ask = self.b.search_lobbies(bucket, 5, full);
        match self.answer(ask) {
            Ok(Answer::Found(l)) => l,
            r => {
                self.log(&format!("search: {r:?}"));
                Vec::new()
            }
        }
    }

    fn members_of(&self, lobby: &str) -> Vec<String> {
        self.members.get(lobby).cloned().unwrap_or_default()
    }

    fn closed(&self, lobby: &str, why: Option<&str>) -> bool {
        self.changes
            .iter()
            .any(|c| matches!(c, LobbyChange::Closed { lobby: l, why: w } if l == lobby && why.is_none_or(|y| y == w)))
    }

    // ---- the other process

    fn tell(&mut self, m: Value) {
        let w = self.to_other.as_mut().expect("a pipe to the other process");
        writeln!(w, "@@ {m}").expect("the other process reads");
        w.flush().expect("the other process reads");
    }

    /// The next message of kind `t` from the other process, stepping while it waits.
    fn heard(&mut self, t: &str, ms: u64) -> Value {
        let mut at = None;
        let ok = self.until(|e| e.inbox.iter().any(|m| m["t"] == t), ms);
        assert!(ok, "[{}] no '{t}' from the other process", self.role);
        for (i, m) in self.inbox.iter().enumerate() {
            if m["t"] == t {
                at = Some(i);
                break;
            }
        }
        self.inbox.remove(at.expect("found above"))
    }

    // ---- the round trips

    fn ping_once(&mut self, to: &str) {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&self.seq.to_le_bytes());
        b[4] = PING;
        let now = self.now();
        self.sent_at.insert(self.seq, now);
        self.seq += 1;
        self.b.send(to, Channel::Match, &b);
    }

    /// Pings `to` at 60 Hz for `ms`: (sent, answered, the median round trip of the second half).
    fn ping(&mut self, to: &str, ms: f64) -> (u32, usize, f64) {
        self.rtts.clear();
        self.sent_at.clear();
        let t0 = self.now();
        let mut sent = 0u32;
        while self.now() - t0 < ms {
            if self.now() >= t0 + f64::from(sent) * 1000.0 / 60.0 {
                self.ping_once(to);
                sent += 1;
            }
            self.step();
        }
        self.pause(300);
        let mut late: Vec<f64> = self.rtts[self.rtts.len() / 2..].to_vec();
        late.sort_by(f64::total_cmp);
        let median = late.get(late.len() / 2).copied().unwrap_or(f64::NAN);
        (sent, self.rtts.len(), median)
    }

    /// Shuts the backend down (the park) and waits for its answer.
    fn park(&mut self) {
        let ask = self.b.shutdown();
        let r = self.answer(ask);
        let ok = matches!(r, Ok(Answer::Done));
        self.check(ok, "shutdown (the park) answered");
    }

    fn end(mut self) -> u32 {
        self.park();
        end_eos();
        let _ = std::fs::remove_dir_all(&self.cache);
        self.failures
    }
}

fn pattern(n: usize, seed: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + seed) & 255) as u8).collect()
}

fn quantile(s: &[f64], p: f64) -> f64 {
    let i = ((p * s.len() as f64) as usize).min(s.len().saturating_sub(1));
    s.get(i).copied().unwrap_or(f64::NAN)
}

fn epoch_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

/// The relay steps: each side's mode, who opens a new link (or 'kept': the link of the step
/// before stays, the modes changed under it), and what the link is to be.
struct Step {
    name: &'static str,
    host: RelayMode,
    guest: RelayMode,
    open: &'static str,
    relayed: bool,
}

const fn step(name: &'static str, host: RelayMode, guest: RelayMode, open: &'static str, relayed: bool) -> Step {
    Step { name, host, guest, open, relayed }
}

use RelayMode::{Allow, Force};
const STEPS: [Step; 9] = [
    step("both allow, a new link", Allow, Allow, "guest", false),
    step("both changed to force, the link kept", Force, Force, "kept", false),
    step("both force, the link closed and opened again", Force, Force, "guest", true),
    step("both changed to allow, the link kept", Allow, Allow, "kept", true),
    step("both allow, the link closed and opened again", Allow, Allow, "guest", false),
    step("host allow, guest force, the guest opens", Allow, Force, "guest", true),
    step("host allow, guest force, the host opens", Allow, Force, "host", true),
    step("host force, guest allow, the guest opens", Force, Allow, "guest", true),
    step("host force, guest allow, the host opens", Force, Allow, "host", true),
];

fn mode_text(m: RelayMode) -> &'static str {
    if m == Allow { "allow" } else { "force" }
}

fn kind_of(r: Option<bool>) -> &'static str {
    match r {
        None => "none",
        Some(true) => "relayed",
        Some(false) => "direct",
    }
}

/// The guest: this binary again, its stdin the host's messages, its stdout's `@@` lines its own.
fn start_guest() -> (Child, ChildStdin, Receiver<Value>) {
    let exe = std::env::current_exe().expect("the test binary");
    let mut child = Command::new(exe)
        .args(["eos_pair_guest", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE, "guest")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the guest starts");
    let stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    (child, stdin, lines(Box::new(stdout)))
}

/// The `@@` lines of a pipe, as JSON, on a thread.
fn lines(r: Box<dyn std::io::Read + Send>) -> Receiver<Value> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines() {
            let Ok(line) = line else { break };
            // (the first may follow the test harness's "test eos_pair_guest ... ")
            if let Some(at) = line.find("@@ ")
                && let Ok(v) = serde_json::from_str::<Value>(&line[at + 3..])
                && tx.send(v).is_err()
            {
                break;
            }
        }
    });
    rx
}

#[test]
fn eos_pair() {
    if std::env::var_os(ROLE).is_some() {
        return;
    }
    if files().is_none() {
        println!("eos_pair: skipped (MVS_EOS_DLL and MVS_EOS_CREDENTIALS not set)");
        return;
    }
    let mut e = Eos::new("host", false);
    let lobby = e.create("mvs:test", 4, None);
    e.log("lobby made");
    e.set_accept();
    let (mut child, stdin, rx) = start_guest();
    e.from_other = Some(rx);
    e.to_other = Some(Box::new(stdin));
    let me = e.me();
    e.tell(json!({ "t": "lobby", "lobby": lobby, "host": me }));

    let joined = e.heard("joined", 60000);
    let guest = joined["guest"].as_str().unwrap_or_default().to_owned();
    let ok = e.until(|e| e.members_of(&lobby).contains(&guest), 15000);
    e.check(ok, "host sees the guest join (member change)");

    // echo everything back, on the channel it came
    e.echo = true;
    e.heard("echo-done", 120000);
    let r = e.b.relayed(&guest);
    e.check(r == Some(true), &format!("host's link is relayed ({})", kind_of(r)));
    e.heard("ping-done", 60000);
    e.echo = false;

    // invite
    let ask = e.b.invite(&guest, &json!({ "lobby": lobby }).to_string());
    match e.answer(ask) {
        Ok(_) => e.log("invite sent"),
        Err(why) => e.log(&format!("invite refused by EOS: {why}")),
    }
    e.tell(json!({ "t": "invited" }));
    e.heard("invite-checked", 30000);

    // kick: a member change here, closed for the guest
    e.b.kick(&lobby, &guest);
    let ok = e.until(|e| !e.members_of(&lobby).contains(&guest), 15000);
    e.check(ok, "host sees the guest kicked (member change)");
    e.tell(json!({ "t": "kicked" }));
    e.heard("rejoined", 30000);
    let ok = e.until(|e| e.members_of(&lobby).contains(&guest), 15000);
    e.check(ok, "host sees the guest join again");

    // peer closed: the host ends the link, the guest hears of it
    e.b.close_peer(&guest);
    e.heard("peer-checked", 30000);

    // the host leaves: the lobby goes for both
    e.leave(&lobby);
    let c = e.closed(&lobby, None);
    e.check(c, "host gets closed after leaving");
    e.tell(json!({ "t": "left" }));
    e.heard("closed-checked", 30000);

    // search: a listed lobby of max 2 in a bucket of this run, found by the guest, then full
    let bucket = format!("mvs:test:{:x}{:04x}", epoch_ms() as u64, rand::random::<u16>());
    let at = epoch_ms();
    let listed = e.create(&bucket, 2, Some(Listed { at, info: Some(info()), code: None }));
    e.tell(json!({ "t": "listed", "bucket": bucket, "lobby": listed, "at": at }));
    e.heard("found", 60000);
    let mine = e.search(&bucket, false);
    let ok = !mine.iter().any(|l| l.id == listed);
    e.check(ok, &format!("host's own search leaves its lobby out ({} found)", mine.len()));
    e.tell(json!({ "t": "searched" }));
    e.heard("full-checked", 60000);
    e.leave(&listed);

    // unlist: a fresh listed lobby, found, then out of the searches
    let fresh = e.create(&bucket, 2, Some(Listed { at: epoch_ms(), info: None, code: None }));
    e.tell(json!({ "t": "fresh", "lobby": fresh }));
    e.heard("fresh-found", 60000);
    let ask = e.b.set_listed(&fresh, Some(Listed { at: 1.0, info: Some(info_2()), code: None }));
    let r = e.answer(ask);
    e.check(matches!(r, Ok(Answer::Done)), "set_listed (new attributes) answered");
    e.tell(json!({ "t": "relisted" }));
    e.heard("relist-checked", 60000);
    let ask = e.b.set_listed(&fresh, None);
    let r = e.answer(ask);
    e.check(matches!(r, Ok(Answer::Done)), "set_listed (unlisted) answered");
    e.tell(json!({ "t": "unlisted" }));
    e.heard("unlist-checked", 60000);
    e.leave(&fresh);

    // ---- the relay control
    let lobby = e.create("mvs:test", 2, None);
    e.set_accept();
    e.tell(json!({ "t": "relays", "lobby": lobby }));
    e.heard("joined", 60000);
    let ok = e.until(|e| e.members_of(&lobby).contains(&guest), 15000);
    e.check(ok, "the guest is in the relay steps' lobby");
    e.pinger = true;
    for s in &STEPS {
        if s.open != "kept" && e.b.relayed(&guest).is_some() {
            // (both sides close, and wait until the other has heard)
            e.b.close_peer(&guest);
            e.tell(json!({ "t": "close" }));
            e.heard("closed", 30000);
            e.pause(500);
        }
        let set = e.b.set_relay_mode(s.host);
        e.check(set.is_ok(), &format!("relay mode {} set ({set:?})", mode_text(s.host)));
        e.tell(json!({ "t": "mode", "mode": mode_text(s.guest) }));
        e.heard("mode-set", 30000);
        e.links.clear();
        e.tell(json!({ "t": "step", "open": s.open }));
        let mine = if s.open == "host" { Some(e.ping(&guest, PING_MS)) } else { None };
        let theirs = e.heard("stepped", 60000);
        let own_kind = kind_of(e.b.relayed(&guest));
        let own_links: Vec<&str> = e.links.iter().map(|(_, r)| if *r { "relayed" } else { "direct" }).collect();
        let rtt = match mine {
            Some((_, _, m)) => m,
            None => theirs["median"].as_f64().unwrap_or(f64::NAN),
        };
        let path = if rtt < DIRECT_RTT_MS { "direct" } else { "relayed" };
        let want = if s.relayed { "relayed" } else { "direct" };
        let their_links = theirs["links"].as_str().unwrap_or_default().to_owned();
        let kept_ok = (s.open == "kept") == (own_links.is_empty() && their_links.is_empty());
        let ok = path == want && own_kind == want && theirs["kind"] == want && kept_ok;
        e.check(
            ok,
            &format!(
                "{}: {want} (the round trip's {path}, {rtt:.2} ms; host says {own_kind}, links [{}]; guest says {}, \
                 links [{their_links}])",
                s.name,
                own_links.join(","),
                theirs["kind"].as_str().unwrap_or_default()
            ),
        );
    }
    e.pinger = false;

    // ---- the park: a request while parked is refused; the reopen signs in as the same player and links again
    e.park();
    e.tell(json!({ "t": "parked" }));
    // (the parked platform polled at its next_due, as the service polls it)
    e.heard("refused-checked", 120000);
    let me = e.me();
    let (dll, credentials) = files().expect("checked above");
    let o = EosOptions {
        files: EosFiles { dll, credentials, cache: e.cache.clone() },
        product: "MVS Classic".to_owned(),
        version: "test".to_owned(),
        fresh_device: false,
        spin: false,
    };
    let b = load_eos(&o);
    e.check(b.is_ok(), &format!("load_eos on the parked platform ({:?})", b.as_ref().err()));
    if let Ok(b) = b {
        e.b = b;
        e.signed_in = false;
        let ok = e.until(|e| e.signed_in || e.signed_out.is_some(), 30000);
        e.check(ok && e.signed_in, "the reopen signs in");
        e.check(e.me() == me, "the reopen is the same player");
        let lobby = e.create("mvs:test", 2, None);
        e.tell(json!({ "t": "reopened", "lobby": lobby }));
        e.heard("joined", 60000);
        e.echo = true;
        e.heard("reopen-echoed", 60000);
        e.echo = false;
    }
    e.tell(json!({ "t": "bye" }));
    let end = Instant::now() + Duration::from_secs(60);
    let code = loop {
        e.step();
        if let Some(s) = child.try_wait().expect("the guest's state") {
            break s.code();
        }
        if Instant::now() > end {
            let _ = child.kill();
            break None;
        }
    };
    let mut failures = e.end();
    if code != Some(0) {
        eprintln!("[host] the guest failed ({code:?})");
        failures += 1;
    }
    assert_eq!(failures, 0, "eos_pair: {failures} failed");
    eprintln!("eos_pair: passed");
}

#[test]
fn eos_pair_guest() {
    if std::env::var_os(ROLE).is_none() || files().is_none() {
        return;
    }
    let rx = lines(Box::new(std::io::stdin()));
    // (the host's lobby first: the guest signs in after the host)
    let first = rx.recv().expect("the host's lobby");
    let lobby = first["lobby"].as_str().unwrap_or_default().to_owned();
    let host = first["host"].as_str().unwrap_or_default().to_owned();
    let mut e = Eos::new("guest", true);
    e.from_other = Some(rx);
    e.to_other = Some(Box::new(std::io::stdout()));
    let me = e.me();
    e.check(me != host, "a second identity (fresh Device ID)");
    e.extra_accept.insert(host.clone());
    e.set_accept();
    match e.join(&lobby) {
        Ok((owner, members)) => {
            e.check(owner == host, "join_lobby names the host as owner");
            let ok = members.contains(&host) && members.contains(&me);
            e.check(ok, &format!("join_lobby's members ({})", members.len()));
        }
        Err(why) => e.check(false, &format!("join_lobby: {why}")),
    }
    e.tell(json!({ "t": "joined", "guest": me }));

    // one reliable packet of the most bytes, then a burst of unreliable ones; each echoed by the host
    let big = pattern(P2P_MAX_PACKET, 3);
    let sent = e.b.send(&host, Channel::Messages, &big);
    e.check(sent, "reliable send accepted");
    let over = e.b.send(&host, Channel::Messages, &vec![0; P2P_MAX_PACKET + 1]);
    e.check(!over, "a packet over the limit refused");
    let ok = e.until(|e| e.packets.iter().any(|p| p.1 == Channel::Messages && p.2 == big), 30000);
    e.check(ok, "reliable 1,170 bytes back on channel 0");
    for i in 0..BURST {
        e.b.send(&host, Channel::Match, &pattern(200, i));
    }
    e.until(|e| e.packets.iter().filter(|p| p.1 == Channel::Match).count() >= BURST, 5000);
    let burst: Vec<(String, Channel, Vec<u8>)> = e.packets.iter().filter(|p| p.1 == Channel::Match).cloned().collect();
    e.check(burst.len() * 10 >= BURST * 9, &format!("unreliable burst back on channel 1: {} of {BURST}", burst.len()));
    let whole = burst.iter().all(|p| p.0 == host && p.2.len() == 200);
    e.check(whole, "burst packets from the host, whole");
    let firsts: IndexSet<u8> = burst.iter().map(|p| p.2[0]).collect();
    let copies = burst.iter().all(|p| p.2 == pattern(200, usize::from(p.2[0]))) && firsts.len() == burst.len();
    e.check(copies, "burst packets kept are copies (each its own bytes)");
    let r = e.b.relayed(&host);
    e.check(r == Some(true), &format!("guest's link is relayed ({})", kind_of(r)));
    e.packets.clear();
    e.tell(json!({ "t": "echo-done" }));

    // the round trip at 60 Hz (the host echoes: a ping comes back as it went, so each is timed as a pong)
    e.rtts.clear();
    e.sent_at.clear();
    let t0 = e.now();
    let mut sent = 0u32;
    let mut b = [0u8; 8];
    while e.now() - t0 < PING_SECONDS * 1000.0 {
        if e.now() >= t0 + f64::from(sent) * 1000.0 / 60.0 {
            b[..4].copy_from_slice(&sent.to_le_bytes());
            b[4] = PONG;
            let now = e.now();
            e.sent_at.insert(sent, now);
            e.b.send(&host, Channel::Match, &b);
            sent += 1;
        }
        e.pinger = true;
        e.step();
    }
    e.pause(300);
    e.pinger = false;
    let mut s = e.rtts.clone();
    s.sort_by(f64::total_cmp);
    e.log(&format!(
        "round trips over {} of {sent} pings at {} Hz: median {:.1} ms, 95% {:.1} ms, worst {:.1} ms",
        s.len(),
        (f64::from(sent) / PING_SECONDS) as u32,
        quantile(&s, 0.5),
        quantile(&s, 0.95),
        quantile(&s, 1.0)
    ));
    e.check(s.len() * 10 >= sent as usize * 9, "pings answered");
    e.tell(json!({ "t": "ping-done" }));

    e.heard("invited", 30000);
    let got = e.until(|e| !e.invites.is_empty(), 10000);
    if got {
        let (from, payload) = e.invites[0].clone();
        let p: Value = serde_json::from_str(&payload).unwrap_or_default();
        e.check(from == host && p["lobby"] == lobby.as_str(), "invite from the host with its payload");
    } else {
        e.log("no invite arrived (see the host's line)");
    }
    e.tell(json!({ "t": "invite-checked" }));

    e.heard("kicked", 30000);
    let ok = e.until(|e| e.closed(&lobby, Some("kicked")), 15000);
    e.check(ok, "guest gets closed (kicked)");
    e.changes.clear();
    match e.join(&lobby) {
        Ok((owner, _)) => e.check(owner == host, "guest joins again after the kick"),
        Err(why) => e.check(false, &format!("guest joins again after the kick: {why}")),
    }
    e.tell(json!({ "t": "rejoined" }));

    let ok = e.until(|e| e.closed_peers.contains(&host), 15000);
    e.check(ok, "guest hears the host closed the link");
    e.tell(json!({ "t": "peer-checked" }));

    e.heard("left", 30000);
    let ok = e.until(|e| e.closed(&lobby, None), 15000);
    let whys: Vec<String> = e
        .changes
        .iter()
        .map(|c| match c {
            LobbyChange::Closed { why, .. } => why.clone(),
            LobbyChange::Members { .. } => "members".to_owned(),
        })
        .collect();
    e.check(ok, &format!("guest gets closed when the host leaves ({})", whys.join(", ")));
    e.tell(json!({ "t": "closed-checked" }));

    // search, polled every SEARCH_POLL_MS: how long after the host's create a lobby shows up, and goes
    const SEARCH_POLL_MS: u64 = 250;
    let poll = |e: &mut Eos, bucket: &str, want: &dyn Fn(&[FoundLobby]) -> bool| {
        let since = Instant::now();
        let mut searches = 0;
        while since.elapsed() < Duration::from_secs(30) {
            let l = e.search(bucket, false);
            searches += 1;
            if want(&l) {
                return Some((l, since.elapsed().as_millis(), searches));
            }
            e.pause(SEARCH_POLL_MS);
        }
        None
    };
    let m = e.heard("listed", 60000);
    let bucket = m["bucket"].as_str().unwrap_or_default().to_owned();
    let listed = m["lobby"].as_str().unwrap_or_default().to_owned();
    let at = m["at"].as_f64().unwrap_or_default();
    let shown = poll(&mut e, &bucket, &|l| l.iter().any(|x| x.id == listed));
    let when =
        shown.as_ref().map_or(String::new(), |s| format!(" {} ms after its create was told ({} searches)", s.1, s.2));
    e.check(shown.is_some(), &format!("guest finds the listed lobby by search{when}"));
    let f = shown.as_ref().and_then(|s| s.0.iter().find(|x| x.id == listed).cloned());
    let ok =
        f.as_ref().is_some_and(|f| f.owner == host && f.members == 1 && f.max == 2 && f.at == at && f.info == info());
    let what = f.as_ref().map_or("none".to_owned(), |f| {
        format!("{}, {}, {}, {}, {}", f.owner == host, f.members, f.max, f.at == at, f.info == info())
    });
    e.check(ok, &format!("found lobby's owner, members, max, at and info ({what})"));
    e.tell(json!({ "t": "found" }));
    e.heard("searched", 30000);
    match e.join(&listed) {
        Ok((owner, members)) => {
            e.check(owner == host && members.len() == 2, "guest joins the found lobby by its id");
        }
        Err(why) => e.check(false, &format!("guest joins the found lobby by its id: {why}")),
    }
    let gone = poll(&mut e, &bucket, &|l| !l.iter().any(|x| x.id == listed));
    let when = gone.as_ref().map_or(String::new(), |s| format!(" ({} ms after the join, {} searches)", s.1, s.2));
    e.check(gone.is_some(), &format!("a full lobby is out of the searches{when}"));
    let with_full = e.search(&bucket, true);
    let ok = with_full.iter().any(|x| x.id == listed && x.members == 2);
    e.check(ok, &format!("a search of the full ones too finds it ({} found)", with_full.len()));
    e.leave(&listed);
    e.tell(json!({ "t": "full-checked" }));

    let m = e.heard("fresh", 60000);
    let fresh = m["lobby"].as_str().unwrap_or_default().to_owned();
    let shown = poll(&mut e, &bucket, &|l| l.iter().any(|x| x.id == fresh));
    e.check(shown.is_some(), "guest finds a fresh listed lobby");
    e.tell(json!({ "t": "fresh-found" }));
    e.heard("relisted", 60000);
    let i2 = info_2();
    let relisted = poll(&mut e, &bucket, &|l| l.iter().any(|x| x.id == fresh && x.info == i2 && x.at == 1.0));
    let when = relisted.as_ref().map_or(String::new(), |s| format!(" ({} ms, {} searches)", s.1, s.2));
    e.check(relisted.is_some(), &format!("a listed lobby's new attributes show in the searches{when}"));
    e.tell(json!({ "t": "relist-checked" }));
    e.heard("unlisted", 60000);
    let unlisted = poll(&mut e, &bucket, &|l| !l.iter().any(|x| x.id == fresh));
    let when = unlisted.as_ref().map_or(String::new(), |s| format!(" ({} ms, {} searches)", s.1, s.2));
    e.check(unlisted.is_some(), &format!("an unlisted lobby is out of the searches{when}"));
    e.tell(json!({ "t": "unlist-checked" }));

    // ---- the relay control
    let m = e.heard("relays", 60000);
    let lobby = m["lobby"].as_str().unwrap_or_default().to_owned();
    if let Err(why) = e.join(&lobby) {
        e.check(false, &format!("join the relay steps' lobby: {why}"));
    }
    e.tell(json!({ "t": "joined", "guest": me }));
    e.pinger = true;
    loop {
        let ok = e.until(|e| !e.inbox.is_empty(), 180000);
        if !ok {
            break;
        }
        let m = e.inbox.remove(0);
        let t = m["t"].as_str().unwrap_or_default().to_owned();
        if t == "parked" {
            break;
        }
        match t.as_str() {
            "close" => {
                e.until(|e| e.closed_peers.contains(&host), 15000);
                e.closed_peers.clear();
                e.b.close_peer(&host);
                e.tell(json!({ "t": "closed" }));
            }
            "mode" => {
                let mode = if m["mode"] == "allow" { Allow } else { Force };
                let set = e.b.set_relay_mode(mode);
                e.check(set.is_ok(), &format!("relay mode set ({set:?})"));
                e.tell(json!({ "t": "mode-set" }));
            }
            "step" => {
                e.links.clear();
                let r = if m["open"] == "host" {
                    e.pause((PING_MS + 300.0) as u64);
                    (0, 0, f64::NAN)
                } else {
                    e.ping(&host, PING_MS)
                };
                let links: Vec<&str> = e.links.iter().map(|(_, r)| if *r { "relayed" } else { "direct" }).collect();
                let kind = kind_of(e.b.relayed(&host));
                let median = if r.2.is_nan() { Value::Null } else { json!(r.2) };
                e.tell(json!({ "t": "stepped", "kind": kind, "links": links.join(","), "median": median, "answered": r.1 }));
            }
            _ => {}
        }
    }
    e.pinger = false;

    // ---- the host parked: a link it is asked for is refused (closed at once)
    e.closed_peers.clear();
    e.b.close_peer(&host);
    e.pause(500);
    e.closed_peers.clear();
    e.b.send(&host, Channel::Messages, b"parked?");
    // (EOS tells a requester nothing of a request the other side closed: its link times out)
    let t = Instant::now();
    let refused = e.until(|e| e.closed_peers.contains(&host), 90000);
    let after = t.elapsed().as_millis();
    e.check(refused, &format!("a link to the parked host is refused (closed, timed out after {after} ms)"));
    let none = e.packets.is_empty();
    e.check(none, "nothing comes back from the parked host");
    e.tell(json!({ "t": "refused-checked" }));

    // ---- the host reopened: a lobby and a link again
    let m = e.heard("reopened", 90000);
    let lobby = m["lobby"].as_str().unwrap_or_default().to_owned();
    match e.join(&lobby) {
        Ok((owner, _)) => e.check(owner == host, "guest joins the reopened host's lobby"),
        Err(why) => e.check(false, &format!("guest joins the reopened host's lobby: {why}")),
    }
    e.tell(json!({ "t": "joined", "guest": me }));
    e.packets.clear();
    e.b.send(&host, Channel::Messages, b"again");
    let ok = e.until(|e| e.packets.iter().any(|p| p.2 == b"again"), 30000);
    e.check(ok, "a packet echoed by the reopened host");
    e.tell(json!({ "t": "reopen-echoed" }));
    e.heard("bye", 120000);
    let failures = e.end();
    assert_eq!(failures, 0, "[guest] {failures} failed");
}
