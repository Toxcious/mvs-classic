//! A match's path over the real EOS SDK (online plan v4 section 5), two identities on this PC, one process each as
//! eos_pair.rs: the host process starts a relay on 127.0.0.1 and the guest (this test binary again, with a fresh
//! Device ID); each runs a net service signed in to the relay (50 ms each way: far, so the core measures) and to
//! Epic. A queue match: the path's links are relayed, a player's app referees it over Epic, both pages ruled; a
//! custom lobby with Direct connections on both sides: the link to the referee is direct.
//!
//! Runs only when MVS_EOS_DLL and MVS_EOS_CREDENTIALS are set (it says so and passes otherwise); it contacts Epic's
//! servers. It prints no id and nothing of the credentials. Note: the guest's fresh Device ID replaces this Windows
//! user's for good, as eos_pair's does.
//!   cargo test -p mvs-net --test eos_path -- --nocapture

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use mvs_net::backend::eos::{EosFiles, EosOptions, load_eos};
use mvs_net::backend::{BackendEvent, BackendKind};
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{ClientOptions, NetFaults, RelayClient};
use mvs_net::service::api::{NetEvent, NetRequest, SessionTarget};
use mvs_net::service::page::PageId;
use mvs_net::service::{NetService, ServiceConfig, ServiceOptions};
use mvs_protocol::match_wire::{INPUT_BYTES, MSG_INPUT, input_header};
use mvs_protocol::version::PROTOCOL;
use mvs_relay::server::{Options, start_relay};

const ROLE: &str = "MVS_EOS_PAIR_ROLE";

fn files() -> Option<(PathBuf, PathBuf)> {
    let dll = std::env::var_os("MVS_EOS_DLL")?;
    let credentials = std::env::var_os("MVS_EOS_CREDENTIALS")?;
    Some((PathBuf::from(dll), PathBuf::from(credentials)))
}

/// A relay on 127.0.0.1 on a thread of its own: its address.
fn relay() -> String {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async move {
            let r = start_relay(Options { host: "127.0.0.1".into(), port: 0, ..Options::default() }).expect("a relay");
            let _ = tx.send(r.port);
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }));
    });
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("the relay's port");
    format!("127.0.0.1:{port}")
}

/// The `@@` lines of a pipe, as JSON, on a thread.
fn lines(r: Box<dyn std::io::Read + Send>) -> Receiver<Value> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines() {
            let Ok(line) = line else { break };
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

/// One side: its service on the relay and Epic, its page, its events and log, the other process.
struct Side {
    role: &'static str,
    svc: NetService,
    page: PageId,
    epoch: Instant,
    epic_me: String,
    events: Vec<NetEvent>,
    taken: usize,
    /// The event and log line this match's checks start from (`mark`).
    since: (usize, usize),
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    failures: u32,
    from_other: Receiver<Value>,
    to_other: Box<dyn Write>,
    /// The SDK's cache folder of this run (removed at the end).
    cache: std::path::PathBuf,
}

impl Side {
    fn new(
        role: &'static str,
        address: &str,
        fresh: bool,
        from_other: Receiver<Value>,
        to_other: Box<dyn Write>,
    ) -> Side {
        let epoch = Instant::now();
        let (dll, credentials) = files().expect("checked by the caller");
        let cache = std::env::temp_dir().join(format!("mvs-eos-path-{role}-{}", std::process::id()));
        let o = EosOptions {
            files: EosFiles { dll, credentials, cache: cache.clone() },
            product: "MVS Classic".to_owned(),
            version: "test".to_owned(),
            fresh_device: fresh,
            spin: false,
        };
        let mut eos = load_eos(&o).unwrap_or_else(|e| panic!("[{role}] load_eos: {e}"));
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(Instant::now() < end, "[{role}] no sign-in in 30 s");
            match eos.poll(epoch.elapsed().as_secs_f64() * 1000.0) {
                Some(BackendEvent::SignedIn) => break,
                Some(BackendEvent::SignedOut(why)) => panic!("[{role}] sign-in failed: {why}"),
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let epic_me = eos.me().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let lines = log.clone();
        let sink: Box<dyn FnMut(&str) + Send> = Box::new(move |l: &str| {
            lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(l.to_string());
        });
        let now = epoch.elapsed().as_secs_f64() * 1000.0;
        let mut svc = NetService::new(ServiceOptions { epoch, log: Some(sink), ..ServiceOptions::default() }, now);
        svc.configure(ServiceConfig { relay: Some(address.to_string()), ..ServiceConfig::default() }, now);
        let mut co = ClientOptions::new("mvs-test 0", epoch);
        co.faults = NetFaults { delay_ms: 50.0, ..NetFaults::default() };
        let c = RelayClient::connect(address, &RelayKey::fresh(), &co).expect("signed in to the relay");
        svc.start_backend(BackendKind::Relay, Box::new(c), now);
        svc.start_backend(BackendKind::Epic, eos, now);
        let page = svc.open_page(now);
        let mut s = Side {
            role,
            svc,
            page,
            epoch,
            epic_me,
            events: Vec::new(),
            taken: 0,
            since: (0, 0),
            log,
            failures: 0,
            from_other,
            to_other,
            cache,
        };
        // (the hello carries the round trip to the relay: measured first)
        s.pause(2500);
        s
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    fn say_log(&self, s: &str) {
        eprintln!("[{}] {s}", self.role);
    }

    fn check(&mut self, ok: bool, what: &str) {
        self.say_log(&format!("{} {what}", if ok { "ok  " } else { "FAIL" }));
        if !ok {
            self.failures += 1;
        }
    }

    fn tell(&mut self, v: Value) {
        let _ = writeln!(self.to_other, "@@ {v}");
        let _ = self.to_other.flush();
    }

    /// The other process's next message of kind `t` (stepping meanwhile).
    fn heard(&mut self, t: &str, ms: u64) -> Value {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            match self.from_other.try_recv() {
                Ok(v) if v["t"] == t => return v,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.dump();
                    panic!("[{}] the other process is gone (waiting for {t})", self.role);
                }
                _ => {}
            }
            if Instant::now() >= end {
                self.dump();
                panic!("[{}] nothing heard of {t}", self.role);
            }
            self.step();
        }
    }

    /// The service's last log lines and page messages, for a failed wait.
    fn dump(&self) {
        let log = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for l in log.iter().rev().take(40).rev() {
            eprintln!("[{}] log {l}", self.role);
        }
        for m in self.msgs_from(self.events.len().saturating_sub(15)) {
            eprintln!("[{}] msg {}", self.role, m["t"]);
        }
    }

    fn step(&mut self) {
        let now = self.now();
        self.svc.pump(now);
        let page = self.page;
        self.events.extend(self.svc.events(page));
        std::thread::sleep(Duration::from_millis(1));
    }

    /// The run's end: the service shut down (its backends parked), then the SDK's cache folder removed.
    fn finish(mut self) -> u32 {
        let now = self.now();
        self.svc.shutdown(now);
        self.pause(1000);
        let failures = self.failures;
        drop(self.svc);
        let _ = std::fs::remove_dir_all(&self.cache);
        failures
    }

    fn pause(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step();
        }
    }

    fn post(&mut self, m: NetRequest) {
        let now = self.now();
        self.svc.take(self.page, m, now);
    }

    fn say(&mut self, t: &str, body: Value) {
        let mut m = body;
        if t == "hello" {
            m["proto"] = json!(PROTOCOL);
            m["app"] = json!("test");
            m["name"] = json!(self.role);
        }
        if t == "lobby.enter" || t == "lobby.join" {
            m["compat"] = json!("ctest.1");
        }
        m["t"] = json!(t);
        self.post(NetRequest::Msg(m.to_string()));
    }

    /// The page's messages from `from` on.
    fn msgs_from(&self, from: usize) -> Vec<Value> {
        self.events[from..]
            .iter()
            .filter_map(|e| match e {
                NetEvent::Msg(t) => serde_json::from_str(t).ok(),
                _ => None,
            })
            .collect()
    }

    /// The first message of kind `t` since the last one taken (stepping until it comes, `ms` at the most).
    fn next(&mut self, t: &str, ms: u64) -> Option<Value> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            let at = self.events[self.taken..].iter().position(|e| match e {
                NetEvent::Msg(x) => serde_json::from_str::<Value>(x).is_ok_and(|m| m["t"] == t),
                _ => false,
            });
            if let Some(i) = at {
                let i = self.taken + i;
                self.taken = i + 1;
                return self.msgs_from(i).into_iter().next();
            }
            if Instant::now() > end {
                return None;
            }
            self.step();
        }
    }

    /// A log line since the match began (`self.since`).
    fn logged(&self, part: &str) -> bool {
        let log = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        log.iter().skip(self.since.1).any(|l| l.contains(part))
    }

    /// Where this match's events and log lines start.
    fn mark(&mut self) {
        let lines = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len();
        self.since = (self.events.len(), lines);
    }

    /// Epic's last word on the link to `peer` as the page was told it.
    fn link_of(&self, peer: &str) -> Option<bool> {
        self.events[self.since.0..].iter().rev().find_map(|e| match e {
            NetEvent::Link { peer: p, relayed } if p == peer => Some(*relayed),
            _ => None,
        })?
    }

    /// From the pre-match to the match's end: (a custom lobby's pm.begin, the lock, pm.all; a queue's found has
    /// none) the go, inputs for 3 s, the path's checks, the leave. `direct`: the link to a player's app is direct.
    fn play(&mut self, what: &str, direct: bool) {
        let queue = what == "queue";
        if !queue {
            let begun = self.next("pm.begin", 30000).is_some();
            self.check(begun, &format!("{what}: pm.begin"));
        }
        // (the path measures while the players pick: its pm.path that names a player, as a pre-match's seconds
        // give it)
        let end = Instant::now() + Duration::from_secs(30);
        let mut referee_slot = -1;
        while referee_slot < 0 && Instant::now() < end {
            let path = self.next("pm.path", 1000);
            referee_slot = path.as_ref().and_then(|p| p["referee"].as_i64()).unwrap_or(-1);
        }
        self.check(self.logged("path: measuring"), &format!("{what}: the core measures (path.peers)"));
        self.check(referee_slot >= 0, &format!("{what}: pm.path names a player"));
        if !queue {
            let loadout = json!({ "taunts": ["", "", "", ""] });
            self.say("pm.lock", json!({ "character": "shaggy", "skin": "", "loadout": loadout }));
            let all = self.next("pm.all", 30000).is_some();
            self.check(all, &format!("{what}: pm.all"));
        }
        self.say("pm.perks", json!({ "perks": [] }));
        let start = self.next("match.start", 30000).unwrap_or(Value::Null);
        let slot = u8::try_from(start["slot"].as_u64().unwrap_or(0)).unwrap_or(0);
        self.say("match.loaded", json!({}));
        let go = self.next("match.go", 30000).unwrap_or(Value::Null);
        let referee = go["referee"].as_str().unwrap_or_default().to_string();
        let peers =
            go["peers"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        self.check(!referee.contains(".m"), &format!("{what}: the go names a player's app, not the relay's referee"));
        self.post(NetRequest::Match { peers, slot: u32::from(slot), referee: referee.clone() });
        let mine = referee == self.epic_me;
        let mut tick = 2u32;
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            self.post(NetRequest::Send(input(slot, tick, 1)));
            tick += 6;
            self.pause(100);
        }
        let ruled = self.events[self.since.0..].iter().filter(|e| matches!(e, NetEvent::Packets { .. })).count();
        self.check(ruled > 10, &format!("{what}: the page ruled over Epic ({ruled} packets)"));
        if mine {
            self.check(self.logged("path: this app referees"), &format!("{what}: this app referees"));
        } else {
            let link = self.link_of(&referee);
            self.check(link == Some(!direct), &format!("{what}: the link to the referee app relayed {:?}", link));
        }
        self.say("match.leave", json!({}));
        self.post(NetRequest::MatchEnd);
        self.pause(2000);
        self.post(NetRequest::Close);
        self.pause(1000);
    }

    fn queue(&mut self) {
        self.post(NetRequest::Open(SessionTarget::Queue));
        let _ = self.next("server", 10000);
        self.say("hello", json!({}));
        let _ = self.next("welcome", 10000);
        let draw = if self.role == "host" { 7919 } else { 2 * 7919 };
        let join = json!({ "modes": ["1v1"], "character": "shaggy", "skin": "", "compat": "ctest.1", "draw": draw, "botAfter": 60 });
        self.say("queue.join", join);
    }
}

/// An input packet of `slot` at `tick` acknowledging every ruled tick (netproc_relay.rs input).
/// (its matches are 1v1s: two slots)
fn input(slot: u8, tick: u32, mark: u8) -> Vec<u8> {
    let n = 2;
    let mut p = vec![0u8; input_header(n) + INPUT_BYTES];
    p[0] = MSG_INPUT;
    p[1] = slot;
    p[2..6].copy_from_slice(&tick.to_le_bytes());
    for k in 0..n {
        p[11 + 4 * k..15 + 4 * k].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    }
    p[6] = 1;
    let last = p.len() - 1;
    p[last] = mark;
    p
}

#[test]
fn eos_path() {
    if std::env::var_os(ROLE).is_some() {
        return;
    }
    if files().is_none() {
        println!("eos_path: skipped (MVS_EOS_DLL and MVS_EOS_CREDENTIALS not set)");
        return;
    }
    let address = relay();
    let exe = std::env::current_exe().expect("the test binary");
    let mut child = Command::new(exe)
        .args(["eos_path_guest", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE, "guest")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the guest starts");
    let mut stdin = child.stdin.take().expect("piped");
    let rx = lines(Box::new(child.stdout.take().expect("piped")));
    let _ = writeln!(stdin, "@@ {}", json!({ "t": "relay", "address": address }));
    let mut s = Side::new("host", &address, false, rx, Box::new(stdin));
    // (the guest's fresh Device ID deletes this Windows user's: it signs in only once the host has)
    s.tell(json!({ "t": "go" }));
    s.heard("ready", 60000);

    // a queue match: never direct
    s.tell(json!({ "t": "queue" }));
    s.mark();
    s.queue();
    s.play("queue", false);
    s.heard("queue-done", 60000);

    // a custom lobby, Direct on both sides
    s.mark();
    s.post(NetRequest::Open(SessionTarget::Host));
    let _ = s.next("server", 10000);
    let code = s.events[s.since.0..].iter().find_map(|e| match e {
        NetEvent::Opened { code, .. } => Some(code.clone()),
        _ => None,
    });
    s.say("hello", json!({}));
    let _ = s.next("welcome", 10000);
    s.say("lobby.enter", json!({}));
    let _ = s.next("lobby.state", 10000);
    s.tell(json!({ "t": "custom", "code": code }));
    s.heard("joined", 60000);
    s.pause(1000);
    s.say("lobby.setTeamStyle", json!({ "style": 0 }));
    s.say("lobby.setMaps", json!({ "maps": ["map_a"] }));
    s.pause(500);
    s.say("lobby.startMatch", json!({}));
    s.play("custom", true);
    let done = s.heard("custom-done", 60000);
    let guest_failures = u32::try_from(done["failures"].as_u64().unwrap_or(1)).unwrap_or(1);
    let _ = child.wait();
    let failures = s.finish();
    assert_eq!(failures + guest_failures, 0, "eos_path: see the FAIL lines");
}

#[test]
fn eos_path_guest() {
    if std::env::var_os(ROLE).is_none() || files().is_none() {
        return;
    }
    let rx = lines(Box::new(std::io::stdin()));
    let first = rx.recv().expect("the relay's address");
    let address = first["address"].as_str().unwrap_or_default().to_owned();
    let go = rx.recv().expect("the host signed in");
    assert_eq!(go["t"], "go");
    let mut s = Side::new("guest", &address, true, rx, Box::new(std::io::stdout()));
    s.tell(json!({ "t": "ready" }));

    s.heard("queue", 60000);
    s.mark();
    s.queue();
    s.play("queue", false);
    s.tell(json!({ "t": "queue-done" }));

    let c = s.heard("custom", 60000);
    let code = c["code"].as_str().unwrap_or_default().to_string();
    s.mark();
    s.post(NetRequest::Open(SessionTarget::Join(code.clone())));
    if s.next("server", 10000).is_none() {
        s.dump();
    }
    s.say("hello", json!({}));
    let _ = s.next("welcome", 10000);
    s.say("lobby.join", json!({ "lobby": code }));
    if s.next("lobby.state", 10000).is_none() {
        s.dump();
    }
    s.say("lobby.setReady", json!({ "on": true }));
    s.tell(json!({ "t": "joined" }));
    s.play("custom", true);
    let failures = s.failures;
    s.tell(json!({ "t": "custom-done", "failures": failures }));
    s.finish();
}
