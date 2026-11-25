//! A match's path end to end (online plan v4 section 5): an mvs-relay on 127.0.0.1 running the real host cores (the
//! relay server's, which measure), each player a net service signed in to it by its own relay client, under 50 ms
//! of delay each way (a round trip over `path::NEAR_MS`), and to Epic by a memory backend (5 ms each way). The core
//! sends path.peers, the games ping each other and report, pm.path names a player, the match goes through that
//! player's app: its referee rules both ways over Epic, tells the core who went silent, is told who left and the
//! end. Held Epic delivery stands for the referee's app cut off: back within RESUME_MS it rules on; past it the
//! match ends on every page. Near players and a player without Epic are never measured: the relay referees.
//! Real sockets and the wall clock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexSet;
use serde_json::{Value, json};

use mvs_net::backend::memory::{MemoryBackend, MemoryNet, MemoryNetOptions};
use mvs_net::backend::{Ask, Backend, BackendEvent, BackendKind, Channel, Listed, Presence, RelayMode};
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{ClientOptions, NetFaults, RelayClient};
use mvs_net::service::api::{NetEvent, NetRequest, SessionTarget};
use mvs_net::service::page::PageId;
use mvs_net::service::{NetService, ServiceConfig, ServiceOptions};
use mvs_protocol::match_wire::{INPUT_BYTES, MSG_INPUT, RESUME_MS, SILENT_MS, input_header};
use mvs_protocol::relay_wire::is_host_id;
use mvs_protocol::version::PROTOCOL;
use mvs_relay::server::{Options, start_relay};

/// A relay on 127.0.0.1 on a thread of its own (it runs to the test's end): its address.
fn relay() -> String {
    let (tx, rx) = mpsc::channel();
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

/// A memory Epic backend whose delivery can be held both ways: while held, nothing it is sent goes and nothing for
/// it is taken (it waits on the net, taken in a burst after). While muted, what it is sent is dropped and what comes
/// for it is taken.
struct Holdable {
    b: MemoryBackend,
    held: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
}

impl Backend for Holdable {
    fn me(&self) -> &str {
        self.b.me()
    }
    fn kind(&self) -> BackendKind {
        self.b.kind()
    }
    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        self.b.create_lobby(bucket, max, listed)
    }
    fn host_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Option<Ask> {
        self.b.host_lobby(bucket, max, listed)
    }
    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        self.b.search_lobbies(bucket, limit, full)
    }
    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        self.b.set_listed(id, listed)
    }
    fn join_lobby(&mut self, id: &str) -> Ask {
        self.b.join_lobby(id)
    }
    fn leave_lobby(&mut self, id: &str) -> Ask {
        self.b.leave_lobby(id)
    }
    fn kick(&mut self, lobby: &str, member: &str) {
        self.b.kick(lobby, member);
    }
    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        if self.held.load(Ordering::SeqCst) || self.muted.load(Ordering::SeqCst) {
            return true;
        }
        self.b.send(to, channel, bytes)
    }
    fn close_peer(&mut self, peer: &str) {
        self.b.close_peer(peer);
    }
    fn relayed(&self, peer: &str) -> Option<bool> {
        self.b.relayed(peer)
    }
    fn linked_peers(&self) -> Vec<String> {
        self.b.linked_peers()
    }
    fn set_relay_mode(&mut self, mode: RelayMode) -> Result<(), String> {
        self.b.set_relay_mode(mode)
    }
    fn set_accept(&mut self, ids: IndexSet<String>) {
        self.b.set_accept(ids);
    }
    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        self.b.invite(to, payload)
    }
    fn presence(&mut self) -> Option<&mut dyn Presence> {
        self.b.presence()
    }
    fn poll(&mut self, now: f64) -> Option<BackendEvent> {
        if self.held.load(Ordering::SeqCst) {
            return None;
        }
        self.b.poll(now)
    }
    fn next_due(&self) -> Option<f64> {
        self.b.next_due()
    }
    fn shutdown(&mut self) -> Ask {
        self.b.shutdown()
    }
}

/// One player: its service, its page, the page's events, the messages taken, its ids, its Epic hold, its log.
struct P {
    svc: NetService,
    page: PageId,
    me: String,
    epic: Option<String>,
    name: String,
    events: Vec<NetEvent>,
    taken: Vec<bool>,
    held: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
}

impl P {
    /// A player on the relay (`delay_ms` each way) and, with `epic`, on the memory Epic as that id.
    fn new(address: &str, net: &MemoryNet, name: &str, epic: Option<&str>, delay_ms: f64, epoch: Instant) -> P {
        let now = ms(epoch);
        let log = Arc::new(Mutex::new(Vec::new()));
        let lines = log.clone();
        let sink: Box<dyn FnMut(&str) + Send> = Box::new(move |l: &str| {
            lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(l.to_string());
        });
        let mut svc = NetService::new(ServiceOptions { epoch, log: Some(sink), ..ServiceOptions::default() }, now);
        svc.configure(ServiceConfig { relay: Some(address.to_string()), ..ServiceConfig::default() }, now);
        let mut o = ClientOptions::new("mvs-test 0", epoch);
        o.faults = NetFaults { delay_ms, ..NetFaults::default() };
        let c = RelayClient::connect(address, &RelayKey::fresh(), &o).expect("signed in to the relay");
        let me = Backend::me(&c).to_string();
        svc.start_backend(BackendKind::Relay, Box::new(c), now);
        let held = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        if let Some(id) = epic {
            let b = Holdable { b: net.player(Some(id), BackendKind::Epic), held: held.clone(), muted: muted.clone() };
            svc.start_backend(BackendKind::Epic, Box::new(b), now);
        }
        let page = svc.open_page(now);
        P {
            svc,
            page,
            me,
            epic: epic.map(str::to_string),
            name: name.to_string(),
            events: Vec::new(),
            taken: Vec::new(),
            held,
            muted,
            log,
        }
    }
    fn post(&mut self, m: NetRequest, epoch: Instant) {
        self.svc.take(self.page, m, ms(epoch));
    }
    fn say(&mut self, t: &str, body: Value, epoch: Instant) {
        let mut m = body;
        if t == "hello" {
            m["proto"] = json!(PROTOCOL);
            m["app"] = json!("test");
            m["name"] = json!(self.name);
        }
        if t == "lobby.enter" || t == "lobby.join" {
            m["compat"] = json!("ctest.1");
        }
        m["t"] = json!(t);
        self.post(NetRequest::Msg(m.to_string()), epoch);
    }
    fn msgs(&self) -> Vec<(usize, Value)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                NetEvent::Msg(t) => serde_json::from_str(t).ok().map(|v| (i, v)),
                _ => None,
            })
            .collect()
    }
    fn has(&self, t: &str, test: impl Fn(&Value) -> bool) -> bool {
        self.msgs().iter().any(|(i, m)| !self.taken.get(*i).copied().unwrap_or(false) && m["t"] == t && test(m))
    }
    fn next(&mut self, t: &str) -> Value {
        let (i, m) = self
            .msgs()
            .into_iter()
            .find(|(i, m)| !self.taken.get(*i).copied().unwrap_or(false) && m["t"] == t)
            .unwrap_or_else(|| panic!("no {t} message"));
        if self.taken.len() <= i {
            self.taken.resize(i + 1, false);
        }
        self.taken[i] = true;
        m
    }
    fn packets(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, NetEvent::Packets { .. })).count()
    }
    fn logged(&self, part: &str) -> bool {
        self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|l| l.contains(part))
    }
}

fn ms(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1000.0
}

/// Pumps every player (and the memory Epic) for `ms` of wall time.
fn pump(ps: &mut [P], net: &MemoryNet, epoch: Instant, for_ms: f64) {
    let end = ms(epoch) + for_ms;
    while ms(epoch) < end {
        step(ps, net, epoch);
    }
}

fn step(ps: &mut [P], net: &MemoryNet, epoch: Instant) {
    net.pump(ms(epoch));
    for p in ps.iter_mut() {
        let now = ms(epoch);
        p.svc.pump(now);
        let page = p.page;
        p.events.extend(p.svc.events(page));
    }
    std::thread::sleep(Duration::from_millis(1));
}

/// Pumps until `cond` (at most `within` ms).
fn until(ps: &mut [P], net: &MemoryNet, epoch: Instant, within: f64, what: &str, cond: impl Fn(&[P]) -> bool) {
    let end = ms(epoch) + within;
    while !cond(ps) {
        assert!(ms(epoch) < end, "in time: {what}: {:?} / {:?}", ps[0].msgs(), ps[1].msgs());
        step(ps, net, epoch);
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

/// Two players: far from the relay (50 ms each way) unless `near`, the second on Epic unless `no_epic`.
fn two(near: bool, no_epic: bool) -> (Vec<P>, MemoryNet, Instant) {
    let address = relay();
    let epoch = Instant::now();
    let net = MemoryNet::new(MemoryNetOptions { delay_ms: 5.0, ..Default::default() }, ms(epoch));
    let delay = if near { 0.0 } else { 50.0 };
    let mut ps = vec![
        P::new(&address, &net, "Velma", Some("e0"), delay, epoch),
        P::new(&address, &net, "Fred", (!no_epic).then_some("e1"), delay, epoch),
    ];
    // (the hello carries the round trip to the relay: measured first)
    pump(&mut ps, &net, epoch, 2500.0);
    (ps, net, epoch)
}

/// Both players into one 1v1 of the relay's queue, to its queue.found (a queue's pre-match is its perks alone).
fn queue_to_pm(ps: &mut [P], net: &MemoryNet, epoch: Instant) {
    for p in ps.iter_mut() {
        p.post(NetRequest::Open(SessionTarget::Queue), epoch);
    }
    until(ps, net, epoch, 5000.0, "server", |ps| ps.iter().all(|p| p.has("server", |_| true)));
    for p in ps.iter_mut() {
        p.say("hello", json!({}), epoch);
    }
    until(ps, net, epoch, 5000.0, "welcome", |ps| ps.iter().all(|p| p.has("welcome", |_| true)));
    for (i, p) in ps.iter_mut().enumerate() {
        let draw = 7919 * (u32::try_from(i).expect("a player") + 1);
        let join = json!({ "modes": ["1v1"], "character": "shaggy", "skin": "", "compat": "ctest.1", "draw": draw,
            "botAfter": 60 });
        p.say("queue.join", join, epoch);
    }
    until(ps, net, epoch, 15000.0, "queue.found", |ps| ps.iter().all(|p| p.has("queue.found", |_| true)));
}

/// A custom lobby of the first player's on the relay, the second in it by its code, to its pm.begin.
fn custom_to_pm(ps: &mut [P], net: &MemoryNet, epoch: Instant) {
    ps[0].post(NetRequest::Open(SessionTarget::Host), epoch);
    until(ps, net, epoch, 5000.0, "the host's server message", |ps| ps[0].has("server", |_| true));
    let code = ps[0]
        .events
        .iter()
        .find_map(|e| match e {
            NetEvent::Opened { code, .. } => Some(code.clone()),
            _ => None,
        })
        .expect("opened");
    ps[0].say("hello", json!({}), epoch);
    until(ps, net, epoch, 5000.0, "welcome", |ps| ps[0].has("welcome", |_| true));
    ps[0].say("lobby.enter", json!({}), epoch);
    until(ps, net, epoch, 5000.0, "the host's lobby.state", |ps| ps[0].has("lobby.state", |_| true));
    ps[1].post(NetRequest::Open(SessionTarget::Join(code.clone())), epoch);
    until(ps, net, epoch, 5000.0, "the guest's server message", |ps| ps[1].has("server", |_| true));
    ps[1].say("hello", json!({}), epoch);
    until(ps, net, epoch, 5000.0, "the guest's welcome", |ps| ps[1].has("welcome", |_| true));
    ps[1].say("lobby.join", json!({ "lobby": code }), epoch);
    until(ps, net, epoch, 5000.0, "the guest in the lobby", |ps| ps[1].has("lobby.state", |m| m["ev"] == "started"));
    let g = ps[1].me.clone();
    until(ps, net, epoch, 5000.0, "the guest in the core's lobby", |ps| {
        ps[0].msgs().iter().any(|(_, m)| m["t"] == "lobby.state" && m["data"].to_string().contains(&g))
    });
    ps[0].say("lobby.setTeamStyle", json!({ "style": 0 }), epoch);
    ps[0].say("lobby.setMaps", json!({ "maps": ["map_a"] }), epoch);
    ps[1].say("lobby.setReady", json!({ "on": true }), epoch);
    pump(ps, net, epoch, 100.0);
    ps[0].say("lobby.startMatch", json!({}), epoch);
    until(ps, net, epoch, 5000.0, "pm.begin", |ps| ps.iter().all(|p| p.has("pm.begin", |_| true)));
}

/// The match from its pre-match (a custom lobby's pm.begin, a queue's found) to its go, each page given the match:
/// each page's slot and the go's referee.
fn pm_to_go(ps: &mut [P], net: &MemoryNet, epoch: Instant, queue: bool) -> (Vec<u8>, String) {
    if !queue {
        for (i, p) in ps.iter_mut().enumerate() {
            p.next("pm.begin");
            let loadout = json!({ "taunts": ["", "", "", ""], "ringOut": format!("ROV_{i}") });
            p.say("pm.lock", json!({ "character": "shaggy", "skin": "", "loadout": loadout }), epoch);
        }
        until(ps, net, epoch, 10000.0, "pm.all", |ps| ps.iter().all(|p| p.has("pm.all", |_| true)));
    }
    for p in ps.iter_mut() {
        p.say("pm.perks", json!({ "perks": [] }), epoch);
    }
    until(ps, net, epoch, 30000.0, "match.start", |ps| ps.iter().all(|p| p.has("match.start", |_| true)));
    let starts: Vec<Value> = ps.iter_mut().map(|p| p.next("match.start")).collect();
    for p in ps.iter_mut() {
        p.say("match.loaded", json!({}), epoch);
    }
    until(ps, net, epoch, 5000.0, "match.go", |ps| ps.iter().all(|p| p.has("match.go", |_| true)));
    let gos: Vec<Value> = ps.iter_mut().map(|p| p.next("match.go")).collect();
    let slots: Vec<u8> =
        starts.iter().map(|s| u8::try_from(s["slot"].as_u64().expect("a slot")).expect("a slot")).collect();
    let referee = gos[0]["referee"].as_str().expect("a referee").to_string();
    assert!(gos.iter().all(|x| x["referee"] == referee.as_str()), "one referee for both: {gos:?}");
    for (i, p) in ps.iter_mut().enumerate() {
        let peers =
            gos[i]["peers"].as_array().expect("peers").iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        p.post(NetRequest::Match { peers, slot: u32::from(slots[i]), referee: referee.clone() }, epoch);
    }
    (slots, referee)
}

/// The go's referee is the relay's (a core's match id).
fn relays(referee: &str) -> bool {
    referee.rsplit_once(".m").is_some_and(|(h, n)| is_host_id(h) && n.parse::<u32>().is_ok())
}

/// Both pages got pm.path naming a player (its slot), with a round trip for each.
fn path_names_a_player(ps: &[P]) -> i64 {
    let paths: Vec<Value> =
        ps.iter().filter_map(|p| p.msgs().into_iter().map(|(_, m)| m).find(|m| m["t"] == "pm.path")).collect();
    assert_eq!(paths.len(), 2, "pm.path to both pages");
    let r = paths[0]["referee"].as_i64().expect("a referee slot");
    assert!(r >= 0 && paths[1]["referee"] == r, "pm.path names a player: {paths:?}");
    r
}

/// The referee's player's index: the one whose Epic id the go names.
fn referee_of(ps: &[P], referee: &str) -> usize {
    ps.iter()
        .position(|p| p.epic.as_deref() == Some(referee))
        .unwrap_or_else(|| panic!("the go names a player's Epic id: {referee}"))
}

/// Both pages send an input every 100 ms for `rounds` rounds (`silent` sends none), from `tick`.
fn play(
    ps: &mut [P],
    net: &MemoryNet,
    epoch: Instant,
    slots: &[u8],
    tick: &mut u32,
    rounds: u32,
    silent: Option<usize>,
) {
    for _ in 0..rounds {
        for (i, p) in ps.iter_mut().enumerate() {
            if Some(i) != silent {
                p.post(NetRequest::Send(input(slots[i], *tick, 1)), epoch);
            }
        }
        *tick += 6;
        pump(ps, net, epoch, 100.0);
    }
}

/// `play`'s rounds (one every 100 ms) to the first step after which `cond` holds, at most `within` ms: whether it did.
/// `cond` is looked at after every step, not every round.
fn play_until(
    ps: &mut [P],
    net: &MemoryNet,
    epoch: Instant,
    slots: &[u8],
    tick: &mut u32,
    within: f64,
    cond: impl Fn(&[P]) -> bool,
) -> bool {
    let end = ms(epoch) + within;
    let mut round = ms(epoch);
    while !cond(ps) {
        if ms(epoch) >= end {
            return false;
        }
        if ms(epoch) >= round {
            for (i, p) in ps.iter_mut().enumerate() {
                p.post(NetRequest::Send(input(slots[i], *tick, 1)), epoch);
            }
            *tick += 6;
            round += 100.0;
        }
        step(ps, net, epoch);
    }
    true
}

#[test]
fn a_far_queue_match_goes_through_a_players_app() {
    let (mut ps, net, epoch) = two(false, false);
    queue_to_pm(&mut ps, &net, epoch);
    until(&mut ps, &net, epoch, 3000.0, "path.peers", |ps| ps.iter().all(|p| p.logged("path: measuring")));
    assert!(ps.iter().all(|p| p.logged("path: measuring 1 peers")), "both games measure: path.peers came");
    // (the reports go PATH_PINGS * PATH_PING_MS on: pm.path before the lock's end)
    until(&mut ps, &net, epoch, 6000.0, "pm.path", |ps| ps.iter().all(|p| p.has("pm.path", |_| true)));
    path_names_a_player(&ps);
    let (slots, referee) = pm_to_go(&mut ps, &net, epoch, true);
    let r = referee_of(&ps, &referee);
    assert!(ps[r].logged("path: this app referees match"), "the referee's game told the go");
    let mut tick = 2;
    play(&mut ps, &net, epoch, &slots, &mut tick, 5, None);
    assert!(
        ps.iter().all(|p| p.packets() > 0),
        "the referee app rules both ways: {} / {}",
        ps[0].packets(),
        ps[1].packets()
    );
    // (the other leaves: the referee told it left)
    let o = 1 - r;
    ps[o].say("match.leave", json!({}), epoch);
    ps[o].post(NetRequest::MatchEnd, epoch);
    until(&mut ps, &net, epoch, 5000.0, "ref.left", |ps| ps[r].logged("left match"));
    // (a leaver ends no match: the referee's own game ends it, its result then its leave, the core's last peer)
    ps[r].say("match.result", json!({ "team": 0 }), epoch);
    ps[r].say("match.leave", json!({}), epoch);
    until(&mut ps, &net, epoch, 5000.0, "ref.end", |ps| ps[r].logged("over"));
    ps[r].post(NetRequest::MatchEnd, epoch);
    pump(&mut ps, &net, epoch, 1500.0);
    assert!(net.links().is_empty(), "the path's links let go: {:?}", net.links());
}

#[test]
fn a_far_custom_match_tells_a_silent_human_away_then_gone() {
    let (mut ps, net, epoch) = two(false, false);
    custom_to_pm(&mut ps, &net, epoch);
    until(&mut ps, &net, epoch, 6000.0, "pm.path", |ps| ps.iter().all(|p| p.has("pm.path", |_| true)));
    path_names_a_player(&ps);
    let (slots, referee) = pm_to_go(&mut ps, &net, epoch, false);
    let r = referee_of(&ps, &referee);
    let o = 1 - r;
    let mut tick = 2;
    play(&mut ps, &net, epoch, &slots, &mut tick, 5, None);
    assert!(ps.iter().all(|p| p.packets() > 0), "ruled both ways");
    // (the other human falls silent: away at SILENT_MS, gone at RESUME_MS, as the relay's referee tells them)
    let away = |m: &Value| m["slot"] == u64::from(slots[o]) && m["away"] == true;
    let rounds = ((SILENT_MS + 1500.0) / 100.0) as u32;
    play(&mut ps, &net, epoch, &slots, &mut tick, rounds, Some(o));
    assert!(ps[r].has("match.away", away), "the silent human told away to the referee's page");
    let rounds = ((RESUME_MS - SILENT_MS + 1000.0) / 100.0) as u32;
    play(&mut ps, &net, epoch, &slots, &mut tick, rounds, Some(o));
    let left = |m: &Value| m["slot"] == u64::from(slots[o]);
    assert!(ps[r].has("match.left", left), "the silent human gone: the core takes it out");
}

#[test]
fn the_referee_app_cut_off_and_back_rules_on() {
    let (mut ps, net, epoch) = two(false, false);
    custom_to_pm(&mut ps, &net, epoch);
    until(&mut ps, &net, epoch, 6000.0, "pm.path", |ps| ps.iter().all(|p| p.has("pm.path", |_| true)));
    let (slots, referee) = pm_to_go(&mut ps, &net, epoch, false);
    let r = referee_of(&ps, &referee);
    let o = 1 - r;
    let mut tick = 2;
    play(&mut ps, &net, epoch, &slots, &mut tick, 5, None);
    ps[r].held.store(true, Ordering::SeqCst);
    play(&mut ps, &net, epoch, &slots, &mut tick, 100, None);
    ps[r].held.store(false, Ordering::SeqCst);
    let before = ps[o].packets();
    play(&mut ps, &net, epoch, &slots, &mut tick, 30, None);
    let away = |m: &Value| m["slot"] == u64::from(slots[o]) && m["away"] == true;
    let back = |m: &Value| m["slot"] == u64::from(slots[o]) && m["away"] == false;
    assert!(ps[r].has("match.away", away) && ps[r].has("match.away", back), "away and back");
    assert!(ps[o].packets() > before, "ruled again after the hold");
    assert!(!ps.iter().any(|p| p.has("match.left", |_| true)), "no one taken out");
    assert!(!ps[o].logged("lost"), "no ref.lost within RESUME_MS");
}

#[test]
fn the_referee_app_cut_off_past_resume_ends_the_match() {
    let (mut ps, net, epoch) = two(false, false);
    custom_to_pm(&mut ps, &net, epoch);
    until(&mut ps, &net, epoch, 6000.0, "pm.path", |ps| ps.iter().all(|p| p.has("pm.path", |_| true)));
    let (slots, referee) = pm_to_go(&mut ps, &net, epoch, false);
    let r = referee_of(&ps, &referee);
    let o = 1 - r;
    let mut tick = 2;
    play(&mut ps, &net, epoch, &slots, &mut tick, 5, None);
    ps[r].held.store(true, Ordering::SeqCst);
    play(&mut ps, &net, epoch, &slots, &mut tick, 210, None);
    assert!(ps[o].logged("lost"), "the other human's game tells the core the referee is lost");
    ps[r].held.store(false, Ordering::SeqCst);
    play(&mut ps, &net, epoch, &slots, &mut tick, 10, None);
    assert!(ps.iter().all(|p| p.has("match.left", |_| true)), "the match ends on every page");
}

/// Decision 2's case: the referee app up but ruling nothing out (the others' packets taken, none sent): no ruled
/// packet comes to the other human for RESUME_MS, its ref.lost goes, and that ends the match.
#[test]
fn a_referee_app_that_sends_nothing_is_lost() {
    let (mut ps, net, epoch) = two(false, false);
    custom_to_pm(&mut ps, &net, epoch);
    until(&mut ps, &net, epoch, 6000.0, "pm.path", |ps| ps.iter().all(|p| p.has("pm.path", |_| true)));
    let (slots, referee) = pm_to_go(&mut ps, &net, epoch, false);
    let r = referee_of(&ps, &referee);
    let o = 1 - r;
    let mut tick = 2;
    play(&mut ps, &net, epoch, &slots, &mut tick, 5, None);
    ps[r].muted.store(true, Ordering::SeqCst);
    // (looked at after every step: the ref.lost's own match.left is back from the core a round trip after it, which a
    // look every few rounds, late on a slow machine, took for an end that came first)
    let lost = play_until(&mut ps, &net, epoch, &slots, &mut tick, RESUME_MS + 10000.0, |ps| {
        if ps[o].logged("lost") {
            return true;
        }
        assert!(!ps.iter().any(|p| p.has("match.left", |_| true)), "nothing ended the match before the ref.lost");
        false
    });
    assert!(lost, "a ref.lost within RESUME_MS and 10 s");
    play(&mut ps, &net, epoch, &slots, &mut tick, 20, None);
    // (the ref.lost takes the referee's slot out; the referee app, which heard the other human all along, gave no one
    // gone)
    let the_referee = |m: &Value| m["slot"] == u64::from(slots[r]);
    assert!(ps.iter().all(|p| p.has("match.left", the_referee)), "the ref.lost ends the match on every page");
    let the_other = |m: &Value| m["slot"] == u64::from(slots[o]);
    assert!(!ps.iter().any(|p| p.has("match.left", the_other)), "the other human is not taken out");
}

#[test]
fn near_players_are_never_measured() {
    let (mut ps, net, epoch) = two(true, false);
    custom_to_pm(&mut ps, &net, epoch);
    let (_, referee) = pm_to_go(&mut ps, &net, epoch, false);
    assert!(!ps.iter().any(|p| p.logged("path: measuring")), "no path.peers");
    assert!(relays(&referee), "the relay referees: {referee}");
    assert!(
        ps.iter().all(|p| p.msgs().iter().any(|(_, m)| m["t"] == "pm.path" && m["referee"] == -1)),
        "pm.path names the relay"
    );
}

#[test]
fn a_player_without_epic_gives_the_relay() {
    let (mut ps, net, epoch) = two(false, true);
    queue_to_pm(&mut ps, &net, epoch);
    let (_, referee) = pm_to_go(&mut ps, &net, epoch, true);
    assert!(!ps.iter().any(|p| p.logged("path: measuring")), "no path.peers");
    assert!(relays(&referee), "the relay referees: {referee}");
}
