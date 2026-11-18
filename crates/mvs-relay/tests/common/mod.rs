//! The core's checks' players and clock (the oracle relay test's), and a match of a custom lobby the relay hosts (a
//! host core of its own, `mvs_relay::hosted`) to its go.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mvs_protocol::p2p_frame::{FRAME_BYE, FRAME_OPEN, MessageJoiner, Taken as Frame, split_message};
use mvs_protocol::relay_wire::{ServerMessage, is_host_id};
use mvs_protocol::version::PROTOCOL;
use mvs_relay::core::{Core, CorePlayer, Session, Taken};
use mvs_relay::hosted::HostedOptions;
use mvs_relay::limits::{DEFAULT_LIMITS, RelayLimits};
use serde_json::{Value, json};

#[derive(Default)]
pub struct FakeState {
    pub msgs: Vec<Value>,
    /// The packets but a host core's frames (those are joined into `host_msgs` and `byes`).
    pub packets: Vec<(u8, String, Vec<u8>)>,
    pub dropped: Option<String>,
    /// Packets from this id are answered Lost (the player's reader too slow).
    pub lose_from: Option<String>,
    /// Packets from this id are answered NoHandle (the player out of peer handles).
    pub no_handle_from: Option<String>,
    /// Messages past this many are answered Lost (and not taken).
    pub lose_msgs_at: Option<usize>,
    /// The channel-0 frames taken, from anyone (a hello's count for a resume).
    pub got0: u64,
    joiners: HashMap<String, MessageJoiner>,
    /// What host cores sent it, joined, by host id: messages and byes.
    pub host_msgs: Vec<(String, Value)>,
    pub byes: Vec<(String, Value)>,
    /// The next host core's frame panics (a bug in the relay's sink, once).
    pub panic_at_host: bool,
}

#[derive(Clone, Default)]
pub struct Fake(pub Rc<RefCell<FakeState>>);

impl CorePlayer for Fake {
    fn send(&mut self, m: &ServerMessage) -> Taken {
        let mut s = self.0.borrow_mut();
        if s.lose_msgs_at.is_some_and(|n| s.msgs.len() >= n) {
            return Taken::Lost;
        }
        s.msgs.push(serde_json::to_value(m).unwrap());
        Taken::Yes
    }
    fn packet(&mut self, channel: u8, from: &str, bytes: &[u8]) -> Taken {
        let mut s = self.0.borrow_mut();
        if s.no_handle_from.as_deref() == Some(from) {
            return Taken::NoHandle;
        }
        if channel == 0 {
            s.got0 += 1;
        }
        if is_host_id(from) {
            assert_eq!(channel, 0, "a host core's frame on channel 0");
            if std::mem::take(&mut s.panic_at_host) {
                drop(s);
                panic!("a sink's bug");
            }
            let t = s.joiners.entry(from.to_string()).or_default().take(bytes);
            match t {
                Frame::Message(b) => s.host_msgs.push((from.to_string(), serde_json::from_slice(&b).unwrap())),
                Frame::Bye(b) => {
                    s.byes.push((from.to_string(), serde_json::from_slice(&b).unwrap()));
                    s.joiners.remove(from);
                }
                Frame::Part => {}
                t => panic!("a host frame of no meaning: {t:?}"),
            }
        } else {
            s.packets.push((channel, from.to_string(), bytes.to_vec()));
        }
        if s.lose_from.as_deref() == Some(from) { Taken::Lost } else { Taken::Yes }
    }
    fn refuse(&mut self, why: &str) {
        self.0.borrow_mut().dropped = Some(why.to_string());
    }
}

impl Fake {
    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut self.0.borrow_mut().msgs)
    }
    pub fn packets(&self) -> Vec<(u8, String, Vec<u8>)> {
        self.0.borrow().packets.clone()
    }
    pub fn npackets(&self) -> usize {
        self.0.borrow().packets.len()
    }
    pub fn clear_packets(&self) {
        self.0.borrow_mut().packets.clear();
    }
    pub fn dropped(&self) -> Option<String> {
        self.0.borrow().dropped.clone()
    }
    /// The host messages of kind `t` not taken yet, from the first such on, taken.
    pub fn host_took(&self, t: &str) -> Option<Value> {
        let mut s = self.0.borrow_mut();
        let i = s.host_msgs.iter().position(|(_, m)| m["t"] == t)?;
        Some(s.host_msgs.remove(i).1)
    }
    pub fn host_has(&self, t: &str) -> bool {
        self.0.borrow().host_msgs.iter().any(|(_, m)| m["t"] == t)
    }
    pub fn host_kinds(&self) -> Vec<String> {
        self.0.borrow().host_msgs.iter().map(|(_, m)| m["t"].as_str().unwrap_or("").to_string()).collect()
    }
    pub fn clear_host(&self) {
        self.0.borrow_mut().host_msgs.clear();
    }
    pub fn byes(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.0.borrow_mut().byes)
    }
    pub fn got0(&self) -> u64 {
        self.0.borrow().got0
    }
}

#[derive(Clone)]
pub struct P {
    pub f: Fake,
    pub s: Session,
    pub id: String,
}

pub struct World {
    pub core: Core,
    pub t: f64,
    pub logs: Rc<RefCell<Vec<String>>>,
}

impl World {
    pub fn new(limits: RelayLimits) -> World {
        World::make(limits, None)
    }
    /// A relay that runs host cores (`queue`: its queue's too).
    pub fn hosting(limits: RelayLimits, queue: bool) -> World {
        // (paths off: a test of the relay reads no pm.path)
        World::make(limits, Some(HostedOptions { paths: false, ..HostedOptions::system(queue) }))
    }
    fn make(limits: RelayLimits, hosted: Option<HostedOptions>) -> World {
        let logs = Rc::new(RefCell::new(Vec::new()));
        let l2 = logs.clone();
        World {
            core: Core::new(&limits, hosted, Box::new(move |l| l2.borrow_mut().push(l.to_string()))),
            t: 1000.0,
            logs,
        }
    }
    pub fn join(&mut self, id: &str) -> P {
        self.join_at(id, "10.0.0.1")
    }
    /// A sign-in as the server makes it: join, resume (a fresh one: no count), the welcome, resumed.
    pub fn join_at(&mut self, id: &str, address: &str) -> P {
        self.rejoin_at(id, address, None).0
    }
    /// A sign-in with the hello's count `got`: the player, whether it took a seat over, the welcome's `resumed`.
    pub fn rejoin_at(&mut self, id: &str, address: &str, got: Option<u64>) -> (P, bool, Option<u64>) {
        let f = Fake::default();
        let (s, took_over) = self.core.join(id, address, Box::new(f.clone()), self.t).expect("refused");
        let resumed = self.core.resume(&s, got, self.t);
        self.core.resumed(&s, self.t);
        (P { f, s, id: id.to_string() }, took_over, resumed)
    }
    pub fn rejoin(&mut self, id: &str, got: Option<u64>) -> (P, bool, Option<u64>) {
        self.rejoin_at(id, "10.0.0.7", got)
    }
    /// A rejoin whose player is `f` (a fake set to lose or refuse what it is sent).
    pub fn rejoin_with(&mut self, id: &str, got: Option<u64>, f: Fake) -> (P, Option<u64>) {
        let (s, _) = self.core.join(id, "10.0.0.7", Box::new(f.clone()), self.t).expect("refused");
        let resumed = self.core.resume(&s, got, self.t);
        self.core.resumed(&s, self.t);
        (P { f, s, id: id.to_string() }, resumed)
    }
    /// The connection dropped without a bye.
    pub fn cut(&mut self, p: &P) {
        self.core.park(&p.s, self.t);
    }
    pub fn req(&mut self, p: &P, r: Value) -> Value {
        self.core.request(&p.s, &r, self.t).unwrap_or_else(|e| panic!("refused {e}: {r}"))
    }
    pub fn refusal(&mut self, p: &P, r: Value) -> String {
        match self.core.request(&p.s, &r, self.t) {
            Ok(_) => "answered".into(),
            Err(e) => e.to_string(),
        }
    }
    pub fn packet(&mut self, p: &P, channel: u8, to: &str, bytes: &[u8]) {
        self.core.packet(&p.s, channel, to, bytes, self.t);
    }
    pub fn close_peer(&mut self, p: &P, to: &str) {
        self.core.close_peer(&p.s, to, self.t);
    }
    pub fn leave(&mut self, p: &P) {
        self.core.leave(&p.s, self.t);
    }
    /// The clock `ms` on (no pump: [`World::later`] pumps).
    pub fn wait(&mut self, ms: f64) {
        self.t += ms;
    }
    /// The clock `ms` on, then the referees' and the host cores' pump.
    pub fn later(&mut self, ms: f64) {
        self.t += ms;
        self.core.pump(self.t);
    }
    pub fn create(&mut self, p: &P, bucket: &str, max: u32, at: Option<f64>, info: Option<&str>) -> String {
        let mut r = json!({ "op": "create", "bucket": bucket, "max": max });
        if let Some(at) = at {
            r["listed"] = json!({ "at": at });
            if let Some(info) = info {
                r["listed"]["info"] = json!(info);
            }
        }
        self.req(p, r)["lobby"].as_str().unwrap().to_string()
    }
    pub fn lobby(&mut self, p: &P) -> String {
        self.create(p, "b", 4, None, None)
    }
    pub fn search(&mut self, p: &P, bucket: &str, limit: u32, full: bool) -> Vec<Value> {
        let mut r = json!({ "op": "search", "bucket": bucket, "limit": limit });
        if full {
            r["full"] = json!(true);
        }
        self.req(p, r)["lobbies"].as_array().unwrap().clone()
    }
    /// `players` in one lobby (the first's), their messages taken.
    pub fn together(&mut self, players: &[&P]) -> String {
        let l = self.create(players[0], "b", 8, None, None);
        for p in &players[1..] {
            self.req(p, json!({ "op": "join", "lobby": l }));
        }
        for p in players {
            p.f.take();
        }
        l
    }

    // ---- the host cores

    pub fn knock(&mut self, p: &P, host: &str) {
        self.packet(p, 0, host, &[FRAME_OPEN]);
    }
    pub fn bye(&mut self, p: &P, host: &str) {
        self.packet(p, 0, host, &[FRAME_BYE]);
    }
    /// A message to a host core, split as a game splits it.
    pub fn say(&mut self, p: &P, host: &str, t: &str, body: Value) {
        let mut m = body;
        if t == "hello" {
            m["proto"] = json!(PROTOCOL);
            m["app"] = json!("test");
            if m.get("name").is_none() {
                m["name"] = json!(format!("N{}", p.id));
            }
        }
        if t == "lobby.enter" || t == "lobby.join" {
            m["compat"] = json!("ctest.1");
        }
        m["t"] = json!(t);
        for piece in split_message(m.to_string().as_bytes(), 1170) {
            self.packet(p, 0, host, &piece);
        }
    }
    /// Pumps 10 ms at a time until `cond` (at most 30 s of the clock).
    pub fn until(&mut self, what: &str, cond: impl Fn() -> bool) {
        let end = self.t + 30_000.0;
        while !cond() {
            assert!(self.t < end, "in time: {what}");
            self.later(10.0);
        }
    }
    /// A custom lobby the relay hosts, its leader `ps[0]`: each in it, linked to its core and said hello, the
    /// leader's lobby entered and the others' joined. (lobby, host id); every message taken.
    pub fn hosted_lobby(&mut self, ps: &[&P]) -> (String, String) {
        let r = self.req(ps[0], json!({ "op": "create", "bucket": "b", "max": 4, "host": true }));
        let (lobby, host) = (r["lobby"].as_str().unwrap().to_string(), r["host"].as_str().unwrap().to_string());
        for p in &ps[1..] {
            self.req(p, json!({ "op": "join", "lobby": lobby }));
        }
        for (i, p) in ps.iter().enumerate() {
            self.knock(p, &host);
            self.say(p, &host, "hello", json!({}));
            let f = p.f.clone();
            self.until("welcome", || f.host_has("welcome"));
            if i == 0 {
                self.say(p, &host, "lobby.enter", json!({}));
            } else {
                self.say(p, &host, "lobby.join", json!({ "lobby": lobby }));
            }
            let f = p.f.clone();
            self.until("lobby.state", || f.host_has("lobby.state"));
        }
        for p in ps {
            p.f.take();
            p.f.clear_host();
        }
        (lobby, host)
    }
    /// The hosted lobby's match to its go: the leader `ps[0]` starts it, each locks and goes.
    /// Two players play 1v1; more a free for all, bots making up four.
    pub fn run_to_the_go(&mut self, host: &str, ps: &[&P]) -> HostedMatch {
        if ps.len() == 2 {
            self.say(ps[0], host, "lobby.setTeamStyle", json!({ "style": 0 }));
        } else {
            self.say(ps[0], host, "lobby.setTeamStyle", json!({ "style": 2 }));
            for _ in ps.len()..4 {
                let bot = json!({ "team": 0, "character": "shaggy", "skin": "", "difficulty": 1 });
                self.say(ps[0], host, "lobby.addBot", bot);
            }
        }
        for p in &ps[1..] {
            self.say(p, host, "lobby.setReady", json!({ "on": true }));
        }
        self.later(100.0);
        self.say(ps[0], host, "lobby.startMatch", json!({}));
        let fs: Vec<Fake> = ps.iter().map(|p| p.f.clone()).collect();
        self.until("pm.begin", || fs.iter().all(|f| f.host_has("pm.begin")));
        for p in ps {
            self.say(p, host, "pm.lock", json!({ "character": "shaggy", "skin": "" }));
        }
        self.until("pm.all", || fs.iter().all(|f| f.host_has("pm.all")));
        for p in ps {
            self.say(p, host, "pm.perks", json!({ "perks": [] }));
        }
        self.until("match.start", || fs.iter().all(|f| f.host_has("match.start")));
        let starts: Vec<Value> = fs.iter().map(|f| f.host_took("match.start").unwrap()).collect();
        for p in ps {
            self.say(p, host, "match.loaded", json!({}));
        }
        self.until("match.go", || fs.iter().all(|f| f.host_has("match.go")));
        let go = fs[0].host_took("match.go").unwrap();
        for f in &fs {
            f.take();
            f.clear_host();
        }
        let rid = go["referee"].as_str().expect("a referee").to_string();
        let slots = starts.iter().map(|s| u8::try_from(s["slot"].as_u64().expect("a slot")).unwrap()).collect();
        let r#match = rid.rsplit_once(".m").and_then(|(_, m)| m.parse().ok()).expect("a match number");
        HostedMatch { rid, r#match, slots, go }
    }
}

/// A hosted lobby's match at its go: its referee's id, the core's number for it, each player's slot (by the order
/// given to [`World::run_to_the_go`]) and the go.
#[derive(Debug)]
pub struct HostedMatch {
    pub rid: String,
    pub r#match: u32,
    pub slots: Vec<u8>,
    pub go: Value,
}

pub fn world() -> World {
    World::new(DEFAULT_LIMITS)
}

pub fn lobby_msg(l: &str, members: &[&str]) -> Value {
    json!({ "t": "lobby", "lobby": l, "members": members })
}
pub fn closed_msg(l: &str, why: &str) -> Value {
    json!({ "t": "lobby", "lobby": l, "closed": why })
}
pub fn peer_closed(id: &str) -> Value {
    json!({ "t": "peerClosed", "id": id })
}
pub fn invite_msg(from: &str, payload: &str) -> Value {
    json!({ "t": "invite", "from": from, "payload": payload })
}
