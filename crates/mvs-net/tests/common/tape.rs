//! The net service against the oracle's tape (`netproc-tape.json`: the oracle's netproc, queue and party tests run on
//! the oracle NetService over the memory EOS backend): each service of a world is replayed on a NetService of its own,
//! its backend a `TapeBackend` that answers each call as the tape's and hands nothing over by itself (the tape's
//! answers and events are the replay's inputs), its host cores' draws the tape's stream.
//!
//! The steps: a driver input (a page's request, startBackend, configure, ...) comes after every timer of the last
//! advance; a backend's answer or event comes after the timers due before it, and after a timer due at its own time
//! only while the tape's outputs before it are not all made yet (the oracle ran its clock's timers and its deliveries
//! in one queue, by the order they were set: the tape does not say which came first).
//!
//! What the port writes in another form is compared through one table each (`ts_event`, `ts_call` and the `*_text`
//! functions): a code (ClosedWhy, OffWhy, StatusWhy, BrowseWhy, ...) as the oracle's text; a lobby.error's code as its
//! oracle text; a lobby row's empty icon left out (the oracle keeps an `icon: ''`); a message in pieces compared whole
//! (the pieces of one message are as many as its length makes them); a host's bye compared by its why's oracle text
//! (the port's is a ClosedWhy's JSON, the oracle's the text: an oracle bye fed in is turned into the port's code where
//! the table has it). The page events are compared in order among themselves, the backend calls in order among
//! themselves (the oracle posts an event inside the step, the port hands the page's events over after it).
//!
//! A hosting service's referee packets (`MSG_RULED`, to its own page and to each peer) are taken out of both streams
//! and compared by the ruled ticks they carry (`ruled_apart`): the oracle's referee sent a peer every tick it had not
//! acknowledged at every pump, the port's sends a ruled tick once and again only past the peer's resend timeout. Each
//! ruled tick a destination is sent carries the oracle's input or substitution, and goes to it no later than the
//! oracle sent it first; the stream ends are the oracle's and go to it no later than the oracle's did; a ruled packet's clock, stamp and held echo those of one
//! of the oracle's packets to the same destination in the same ms.
#![allow(dead_code, reason = "each test binary uses a part")]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::{IndexMap, IndexSet};
use mvs_host::{HostRand, HostWhy};
use mvs_net::backend::{
    Answer, Ask, Backend, BackendEvent, BackendKind, Channel, FoundLobby, Listed, LobbyChange, Presence, RelayMode,
    RelayPresence, Who,
};
use mvs_net::codes::CodeWhy;
use mvs_net::service::api::{
    AskWhat, BrowseWhy, ClosedWhy, EpicMissing, InviteKind, JoinWhy, NetEvent, NetRequest, OffWhy, PartyJoinWhy,
    PartySay, SessionTarget, StatusWhy,
};
use mvs_net::service::session::Via;
use mvs_net::service::{NetService, PageId, ServiceConfig, ServiceOptions};
use mvs_protocol::match_wire::{MSG_RULED, NO_END, RuledHeader, WireInput, runs};
use serde_json::{Value, json};

// ---------- the tape ----------

/// The tape's test of `file` (the oracle test's name); None with a note when there is no tape (the test passes).
pub fn load(file: &str) -> Option<Value> {
    let tape = mvs_protocol::vectors::read_tape("netproc-tape.json")?;
    let t = tape["tests"].as_array()?.iter().find(|t| t["file"] == file).cloned();
    assert!(t.is_some(), "netproc-tape.json has no test of {file}");
    t
}

/// What one service's replay came to.
#[derive(Debug, Default)]
pub struct SvcReport {
    pub world: usize,
    pub service: usize,
    /// The page events: equal (the longest run in order of equal ones), the tape's, the port's.
    pub events_equal: usize,
    pub events_tape: usize,
    pub events_port: usize,
    /// The backend calls the same way (a message in pieces is one).
    pub calls_equal: usize,
    pub calls_tape: usize,
    pub calls_port: usize,
    /// The ruled ticks of the referee's packets the tape sent that the port sent the same (`ruled_apart`).
    pub ruled: usize,
    /// Equal events or calls at another time than the tape's.
    pub late: usize,
    /// Where both streams are compared up to, and why: the ms of the port's first match.away, where the oracle's
    /// referee had a silent peer leave (match.left disconnected), the rest the port's own away rule; or of a local
    /// lobby the port opens where the oracle closed the session as offline.
    pub cut: Option<(f64, &'static str)>,
    /// Checks of the replay itself that failed (an accept, a draw), in words.
    pub misses: Vec<String>,
    /// The first event and the first call that differ (the tape's, the port's), in words.
    pub first_event: Option<String>,
    pub first_call: Option<String>,
}

impl SvcReport {
    pub fn equal(&self) -> bool {
        self.events_equal == self.events_tape
            && self.events_port == self.events_tape
            && self.calls_equal == self.calls_tape
            && self.calls_port == self.calls_tape
            && self.misses.is_empty()
    }
}

/// Replays every service of every world of a test of the tape, in order.
pub fn replay(test: &Value) -> Vec<SvcReport> {
    let rand = Draws::of(&test["random"]);
    let mut out = Vec::new();
    for w in test["worlds"].as_array().into_iter().flatten() {
        for s in w["services"].as_array().into_iter().flatten() {
            let mut r = replay_service(s, &rand, idx(&w["index"]));
            r.world = idx(&w["index"]);
            r.service = idx(&s["index"]);
            out.push(r);
        }
    }
    out
}

/// Prints each service's line and panics when one is not equal.
/// A difference from the tape a ledger row names: in `world`'s `service`, one page event and one backend call sent at
/// `port_t` where the tape has them at `tape_t`, every other event and call equal. `port_t` None: one backend call
/// alone, which the port makes after the tape's end (a longer wait than the oracle's), every other event and call
/// equal.
pub struct Known {
    pub world: usize,
    pub service: usize,
    pub tape_t: f64,
    pub port_t: Option<f64>,
}

impl Known {
    fn covers(&self, r: &SvcReport) -> bool {
        let at = format!("tape t {} ", self.tape_t);
        let same = r.world == self.world && r.service == self.service && r.misses.is_empty();
        let Some(port_t) = self.port_t else {
            return same
                && r.events_port == r.events_tape
                && r.events_equal == r.events_tape
                && r.calls_port + 1 == r.calls_tape
                && r.calls_equal == r.calls_port
                && r.first_event.is_none()
                && r.first_call.as_ref().is_some_and(|f| f.contains(&at));
        };
        let port = format!("port t {port_t} ");
        let one = |f: &Option<String>| f.as_ref().is_some_and(|f| f.contains(&at) && f.contains(&port));
        same && r.events_port == r.events_tape
            && r.calls_port == r.calls_tape
            && r.events_equal + 1 == r.events_tape
            && r.calls_equal + 1 == r.calls_tape
            && one(&r.first_event)
            && one(&r.first_call)
    }
}

pub fn check(name: &str, reports: &[SvcReport], known: &[Known]) {
    let (mut ev, mut evn, mut ca, mut can) = (0, 0, 0, 0);
    let mut bad = 0;
    for r in reports {
        ev += r.events_equal;
        evn += r.events_tape;
        ca += r.calls_equal;
        can += r.calls_tape;
        eprintln!(
            "{name} world {} service {}: events {}/{} (port {}), calls {}/{} (port {}), ruled ticks {}, late {}{}{}",
            r.world,
            r.service,
            r.events_equal,
            r.events_tape,
            r.events_port,
            r.calls_equal,
            r.calls_tape,
            r.calls_port,
            r.ruled,
            r.late,
            r.cut.map_or(String::new(), |(t, why)| format!(", up to {why} at {t}")),
            if r.misses.is_empty() { String::new() } else { format!(", misses {}", r.misses.len()) }
        );
        if !r.equal() {
            if known.iter().any(|k| k.covers(r)) {
                eprintln!("    the known difference (its ledger row)");
                continue;
            }
            bad += 1;
            for m in r.misses.iter().take(5) {
                eprintln!("    miss: {m}");
            }
            if let Some(f) = &r.first_event {
                eprintln!("    first event: {f}");
            }
            if let Some(f) = &r.first_call {
                eprintln!("    first call: {f}");
            }
        }
    }
    eprintln!("{name}: events {ev}/{evn}, calls {ca}/{can}");
    assert!(bad == 0, "{name}: {bad} of {} services differ from the tape", reports.len());
}

fn idx(v: &Value) -> usize {
    v.as_u64().map_or(0, |n| n as usize)
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().map(text).collect()
}

/// A tape value's bytes ({ $bytes: hex }).
pub fn bytes_of(v: &Value) -> Vec<u8> {
    let h = v["$bytes"].as_str().unwrap_or("");
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0)).collect()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `{ $undef }` as null, every number as f64, a lobby row's empty icon left out.
pub fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("a number")),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) if o.contains_key("$undef") => Value::Null,
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, x)| !(k.as_str() == "icon" && x.as_str() == Some("")))
                .map(|(k, x)| (k.clone(), norm(x)))
                .collect(),
        ),
        _ => v.clone(),
    }
}

// ---------- the host cores' draws ----------

/// The tape's draws, one queue per service index (the worlds are replayed in their order, so each service's draws
/// come in the order the tape has them).
#[derive(Clone, Default)]
struct Draws {
    by_svc: Arc<Mutex<HashMap<usize, VecDeque<Value>>>>,
    misses: Arc<Mutex<Vec<String>>>,
}

impl Draws {
    fn of(v: &Value) -> Draws {
        let d = Draws::default();
        for x in v.as_array().into_iter().flatten() {
            d.lock().entry(idx(&x["svc"])).or_default().push_back(x.clone());
        }
        d
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<usize, VecDeque<Value>>> {
        self.by_svc.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn miss(&self, m: String) {
        self.misses.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(m);
    }
}

struct TapeRand {
    draws: Draws,
    svc: usize,
    clock: Clock,
}

impl TapeRand {
    fn next(&mut self, f: &str) -> Option<Value> {
        let now = self.clock.get();
        let front = {
            let mut all = self.draws.lock();
            // (a draw the tape gives another service at this time: the recorder names the service whose step it ran
            // in, and a memory world's packets run the receiver's core inside the sender's step)
            let mut keys: Vec<usize> = all.keys().copied().collect();
            keys.sort_unstable();
            for k in keys {
                let q = all.get_mut(&k).expect("a key");
                if let Some(i) = q.iter().position(|d| d["fn"] == f && d["t"].as_f64() == Some(now)) {
                    return q.remove(i);
                }
            }
            let q = all.entry(self.svc).or_default();
            if q.front().is_some_and(|d| d["fn"] == f) {
                return q.pop_front();
            }
            q.front().cloned()
        };
        self.draws.miss(format!("draw {f}: the tape's next is {front:?}"));
        None
    }
}

impl HostRand for TapeRand {
    fn int(&mut self, lo: u64, hi: u64) -> u64 {
        let Some(d) = self.next("randomInt") else { return lo };
        let a = d["args"].as_array().cloned().unwrap_or_default();
        let (tlo, thi) = if a.len() == 1 {
            (0, a[0].as_u64().unwrap_or(0))
        } else {
            (a[0].as_u64().unwrap_or(0), a[1].as_u64().unwrap_or(0))
        };
        if (tlo, thi) != (lo, hi) {
            self.draws.miss(format!("randomInt({lo}, {hi}): the tape's is ({tlo}, {thi})"));
        }
        d["value"].as_u64().unwrap_or(lo)
    }

    fn bytes(&mut self, out: &mut [u8]) {
        let Some(d) = self.next("randomBytes") else { return };
        let b: Vec<u8> = bytes_of(&json!({ "$bytes": d["value"] }));
        if b.len() != out.len() {
            self.draws.miss(format!("randomBytes({}): the tape's is {}", out.len(), b.len()));
        }
        for (o, x) in out.iter_mut().zip(b) {
            *o = x;
        }
    }

    fn wall_ms(&mut self) -> u64 {
        // (the recorder's Date.now: 1.7e12 + the clock)
        match self.next("dateNow") {
            Some(d) => d["value"].as_u64().unwrap_or(0),
            None => 1_700_000_000_000 + self.clock.get() as u64,
        }
    }
}

// ---------- the clock ----------

#[derive(Clone, Default)]
struct Clock(Arc<Mutex<f64>>);

impl Clock {
    fn get(&self) -> f64 {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set(&self, t: f64) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = t;
    }
}

// ---------- the backend ----------

/// A call the tape has: its name, args, what it returned, its id (an ask's answer comes under it).
struct CallRec {
    name: String,
    args: Value,
    returns: Value,
    id: u32,
    used: bool,
}

/// What the service's backends of one replay share: the tape's calls, the calls made, the accept set, the links.
#[derive(Default)]
struct BackState {
    tape: Vec<CallRec>,
    made: Vec<(f64, String, Value)>,
    accept: IndexSet<String>,
    links: IndexMap<String, Option<bool>>,
    asks: u32,
}

#[derive(Clone, Default)]
struct Back(Arc<Mutex<BackState>>);

impl Back {
    fn lock(&self) -> MutexGuard<'_, BackState> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A backend that answers each call as the tape's call of that name and args did.
struct TapeBackend {
    me: String,
    kind: BackendKind,
    st: Back,
    clock: Clock,
}

impl TapeBackend {
    /// The call made, and the tape's call it is taken for (the first one not taken of that name and args, else of that
    /// name): its id and what it returned.
    fn call(&self, name: &str, args: Value) -> Option<(u32, Value)> {
        let mut st = self.st.lock();
        st.made.push((self.clock.get(), name.to_string(), args.clone()));
        let want = norm_args(name, &args);
        let i = st
            .tape
            .iter()
            .position(|c| !c.used && c.name == name && norm_args(name, &c.args) == want)
            .or_else(|| st.tape.iter().position(|c| !c.used && c.name == name))?;
        st.tape[i].used = true;
        Some((st.tape[i].id, st.tape[i].returns.clone()))
    }

    fn ask(&self, name: &str, args: Value) -> Ask {
        match self.call(name, args) {
            Some((id, _)) => Ask(id),
            None => {
                let mut st = self.st.lock();
                st.asks += 1;
                Ask(1_000_000_000 + st.asks)
            }
        }
    }
}

fn channel_n(c: Channel) -> u32 {
    match c {
        Channel::Messages => 0,
        Channel::Match => 1,
    }
}

impl Backend for TapeBackend {
    fn me(&self) -> &str {
        &self.me
    }

    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        let mut o = json!({ "bucket": bucket, "max": max });
        if let Some(l) = listed {
            o["listed"] = serde_json::to_value(l).expect("a listing");
        }
        self.ask("createLobby", json!([o]))
    }

    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        self.ask("searchLobbies", json!([{ "bucket": bucket, "limit": limit, "full": full }]))
    }

    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        self.ask("setListed", json!([id, listed.map(|l| serde_json::to_value(l).expect("a listing"))]))
    }

    fn join_lobby(&mut self, id: &str) -> Ask {
        self.ask("joinLobby", json!([id]))
    }

    fn leave_lobby(&mut self, id: &str) -> Ask {
        self.ask("leaveLobby", json!([id]))
    }

    fn kick(&mut self, lobby: &str, member: &str) {
        self.call("kick", json!([lobby, member]));
    }

    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        let r = self.call("send", json!([to, channel_n(channel), { "$bytes": hex(bytes) }]));
        r.is_none_or(|(_, v)| v != json!(false))
    }

    fn close_peer(&mut self, peer: &str) {
        self.call("closePeer", json!([peer]));
        self.st.lock().links.shift_remove(peer);
    }

    fn relayed(&self, peer: &str) -> Option<bool> {
        match self.call("relayed", json!([peer])) {
            Some((_, v)) => v.as_bool(),
            None => self.st.lock().links.get(peer).copied().flatten(),
        }
    }

    fn linked_peers(&self) -> Vec<String> {
        match self.call("linkedPeers", json!([])) {
            Some((_, v)) => strings(&v),
            None => self.st.lock().links.keys().cloned().collect(),
        }
    }

    fn set_relay_mode(&mut self, mode: RelayMode) -> Result<(), String> {
        let m = match mode {
            RelayMode::Allow => "allow",
            RelayMode::Force => "force",
        };
        self.call("setRelayMode", json!([m]));
        Ok(())
    }

    fn set_accept(&mut self, ids: IndexSet<String>) {
        self.st.lock().accept = ids;
    }

    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        self.ask("invite", json!([to, payload]))
    }

    fn presence(&mut self) -> Option<&mut dyn Presence> {
        if self.kind == BackendKind::Relay { Some(self) } else { None }
    }

    fn poll(&mut self, _now: f64) -> Option<BackendEvent> {
        None
    }

    fn shutdown(&mut self) -> Ask {
        self.ask("shutdown", json!([]))
    }
}

impl Presence for TapeBackend {
    fn set(&mut self, p: RelayPresence) -> Ask {
        self.ask("presence.set", json!([serde_json::to_value(p).expect("a presence")]))
    }

    fn who(&mut self, limit: u32) -> Ask {
        self.ask("presence.who", json!([limit]))
    }
}

// ---------- the forms compared ----------

/// A call's args as compared: numbers as f64, a search's
/// `full` false when left out.
fn norm_args(name: &str, args: &Value) -> Value {
    let mut a = norm(args);
    if name == "searchLobbies"
        && let Some(o) = a.get_mut(0).and_then(Value::as_object_mut)
    {
        o.entry("full").or_insert(json!(false));
    }
    a
}

/// A packet of the tape as the port is given it: a host's bye text as the port's code.
fn packet_in(channel: u32, b: &[u8]) -> Vec<u8> {
    if channel != 0 {
        return b.to_vec();
    }
    if b.first() == Some(&3) && b.len() > 1 {
        let t = String::from_utf8_lossy(&b[1..]).to_string();
        if let Some(w) = closed_of_text(&t) {
            let mut out = vec![3];
            out.extend(serde_json::to_vec(&w).expect("a why"));
            return out;
        }
    }
    b.to_vec()
}

/// A page message's text as compared: a lobby.error's code as its text (one a
/// guest of the port passes on from the tape's host has its text already).
fn message(text: &str, tape: bool) -> Value {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else { return json!({ "unparsed": text }) };
    if let Some(o) = v.as_object_mut()
        && !tape
        && o.get("t").and_then(Value::as_str) == Some("lobby.error")
        && o.contains_key("why")
    {
        let why = o.remove("why").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
        let name = o.remove("name").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
        o.insert("text".to_string(), json!(lobby_error_text(&why, &name)));
    }
    norm(&v)
}

/// A lobby.error's oracle text by its code (as host_tape.rs).
pub fn lobby_error_text(why: &str, name: &str) -> String {
    match why {
        "queued_self" => "Leave matchmaking first".to_string(),
        "no_lobby" => "The lobby no longer exists".to_string(),
        "lobby_full" => "The lobby is full".to_string(),
        "compat_missing" => {
            "Your game could not tell the server its game data version. Reload the page and try again".to_string()
        }
        "compat_differs" => "Your game data differs from this lobby's (another game version or another import). \
                             Update the game and import again, then try again"
            .to_string(),
        "compat_mine_older" => format!("Your game is older than {name}'s. Update it, then try again"),
        "compat_theirs_older" => format!("{name} has an older version of the game and has to update it"),
        _ => format!("(unknown code {why})"),
    }
}

/// A page event as compared (`tape`: the tape's oracle JSON; else the port's, already in the oracle's form).
fn event_cmp(page: usize, e: &Value, tape: bool) -> Value {
    let mut e = e.clone();
    if e["t"] == "msg" {
        e["text"] = message(e["text"].as_str().unwrap_or(""), tape);
    }
    norm(&json!({ "page": page, "ev": e }))
}

/// The calls as compared: a message's pieces joined (one call when its last piece goes), its text as compared, a bye
/// by its why's oracle text (`tape`: the tape's, a text already), a match packet by its bytes.
fn calls_cmp(calls: &[(f64, String, Value)], tape: bool) -> Vec<(f64, Value)> {
    let mut out = Vec::new();
    let mut pieces: HashMap<String, Vec<u8>> = HashMap::new();
    for (t, name, args) in calls {
        if name != "send" {
            out.push((*t, json!({ "call": name, "args": norm_args(name, args) })));
            continue;
        }
        let to = text(&args[0]);
        let ch = args[1].as_u64().unwrap_or(0);
        let b = bytes_of(&args[2]);
        if ch != 0 {
            out.push((*t, json!({ "send": to, "ch": ch, "bytes": hex(&if tape { b } else { seq_cleared(b) }) })));
            continue;
        }
        match b.first() {
            Some(0) => pieces.entry(to).or_default().extend(&b[1..]),
            Some(1) => {
                let mut m = pieces.remove(&to).unwrap_or_default();
                m.extend(&b[1..]);
                out.push((*t, json!({ "send": to, "msg": message(&String::from_utf8_lossy(&m), tape) })));
            }
            Some(3) => {
                let body = &b[1..];
                let why = if tape {
                    String::from_utf8_lossy(body).to_string()
                } else {
                    closed_text(&mvs_net::service::hosting::bye_why(body))
                };
                out.push((*t, json!({ "send": to, "bye": why })));
            }
            // (a party's bye: the port's why by its oracle text, its other packets as they are)
            Some(0x50) if !tape => out.push((*t, json!({ "send": to, "bytes": hex(&party_bye_ts(&b)) }))),
            _ => out.push((*t, json!({ "send": to, "bytes": hex(&b) }))),
        }
    }
    out
}

/// A ruled tick a referee packet carries: its stream, its tick (u32::MAX: a run of none, its end only), the input
/// (None: substituted) and the stream's end.
type Ticked = (u64, u32, Option<WireInput>, u32);

/// The ruled ticks of a referee packet.
fn ticked(b: &[u8]) -> Vec<Ticked> {
    let mut out = Vec::new();
    for run in runs(b) {
        let mut at = 0;
        for k in 0..u32::from(run.head.count) {
            let i = if (run.head.mask >> k) & 1 == 1 {
                None
            } else {
                at += 1;
                Some(run.inputs[at - 1])
            };
            out.push((u64::from(run.head.slot), run.head.first + k, i, run.head.end));
        }
        if run.head.count == 0 {
            out.push((u64::from(run.head.slot), u32::MAX, None, run.head.end));
        }
    }
    out
}

/// The referee packets taken out of a service's page events and calls: (ms, destination, bytes).
fn take_ruled(events: &mut Vec<(f64, Value)>, calls: &mut Vec<(f64, Value)>) -> Vec<(f64, String, Vec<u8>)> {
    let mut out = Vec::new();
    for (t, v) in events.iter_mut() {
        if v["ev"]["t"] != "packets" {
            continue;
        }
        let to = format!("page {} from {}", v["page"], v["ev"]["from"]);
        let Some(packets) = v["ev"]["packets"].as_array_mut() else { continue };
        packets.retain(|p| {
            let b = bytes_of(p);
            let ruled = b.first() == Some(&MSG_RULED);
            if ruled {
                out.push((*t, to.clone(), b));
            }
            !ruled
        });
    }
    events.retain(|(_, v)| v["ev"]["t"] != "packets" || v["ev"]["packets"].as_array().is_some_and(|p| !p.is_empty()));
    calls.retain(|(t, v)| {
        let Some(h) = v["bytes"].as_str().filter(|_| v["ch"] == channel_n(Channel::Match)) else { return true };
        let b = bytes_of(&json!({ "$bytes": h }));
        let ruled = b.first() == Some(&MSG_RULED);
        if ruled {
            out.push((*t, format!("send {}", v["send"]), b));
        }
        !ruled
    });
    out
}

/// A service's page events and calls, by the ms each came at.
type Streams<'a> = (&'a mut Vec<(f64, Value)>, &'a mut Vec<(f64, Value)>);

/// The referee's packets of the tape and of the port taken out of both streams and compared by the ruled ticks they
/// carry (the module's note); what differs goes to `misses`. The ruled ticks of the tape's the port sent the same.
fn ruled_apart(tape: Streams<'_>, port: Streams<'_>, misses: &mut Vec<String>) -> usize {
    let (want, got) = (take_ruled(tape.0, tape.1), take_ruled(port.0, port.1));
    // (the tape's ruled streams: by stream and tick the input, by stream the end, by destination, stream and tick
    // when it first went)
    let mut ruled: IndexMap<(u64, u32), Option<WireInput>> = IndexMap::new();
    let mut ends: IndexMap<u64, u32> = IndexMap::new();
    let mut first: IndexMap<(String, u64, u32), f64> = IndexMap::new();
    // (by destination and stream when the stream's end first went)
    let mut end_first: IndexMap<(String, u64), f64> = IndexMap::new();
    for (t, to, b) in &want {
        for (y, k, i, end) in ticked(b) {
            if end != NO_END {
                if *ends.entry(y).or_insert(end) != end {
                    misses.push(format!("t {t}: the tape's two ends of slot {y}"));
                }
                end_first.entry((to.clone(), y)).or_insert(*t);
            }
            if k != u32::MAX {
                if *ruled.entry((y, k)).or_insert(i) != i {
                    misses.push(format!("t {t}: the tape's slot {y} tick {k} twice"));
                }
                first.entry((to.clone(), y, k)).or_insert(*t);
            }
        }
    }
    let mut same = 0;
    let mut got_first: IndexMap<(String, u64, u32), f64> = IndexMap::new();
    let mut got_end: IndexMap<(String, u64), f64> = IndexMap::new();
    for (t, to, b) in &got {
        let Some(h) = RuledHeader::read(b) else {
            misses.push(format!("t {t}: a referee packet to {to} the port cannot read"));
            continue;
        };
        // (several pumps can fall in one ms: the header of one of the tape's packets to it in that ms)
        let heads: Vec<(i32, u32, u16)> = want
            .iter()
            .filter(|w| w.0 == *t && w.1 == *to)
            .filter_map(|w| RuledHeader::read(&w.2))
            .map(|o| (o.clock, o.stamp, o.held))
            .collect();
        if !heads.is_empty() && !heads.contains(&(h.clock, h.stamp, h.held)) {
            misses.push(format!(
                "t {t}: a referee packet's header to {to}: {:?}, the tape's {heads:?}",
                (h.clock, h.stamp, h.held)
            ));
        }
        for (y, k, i, end) in ticked(b) {
            if end != NO_END {
                if ends.get(&y) != Some(&end) {
                    misses.push(format!("t {t}: slot {y}'s end {end} to {to}, the tape's {:?}", ends.get(&y)));
                }
                got_end.entry((to.clone(), y)).or_insert(*t);
            }
            if k == u32::MAX {
                continue;
            }
            // (a tick the tape sent nobody is not known: the port may send it sooner)
            if let Some(w) = ruled.get(&(y, k)) {
                if i == *w {
                    same += 1;
                } else {
                    misses.push(format!("t {t}: slot {y} tick {k} to {to}: the port's {i:?}, the tape's {w:?}"));
                }
            }
            got_first.entry((to.clone(), y, k)).or_insert(*t);
        }
    }
    for ((to, y, k), t) in &first {
        match got_first.get(&(to.clone(), *y, *k)) {
            Some(g) if g <= t => {}
            g => misses.push(format!("t {t}: slot {y} tick {k} to {to}: the port's first at {g:?}")),
        }
    }
    for ((to, y), t) in &end_first {
        match got_end.get(&(to.clone(), *y)) {
            Some(g) if g <= t => {}
            g => misses.push(format!("t {t}: slot {y}'s end to {to}: the port's first at {g:?}")),
        }
    }
    same
}

/// An input packet of the port's with its sequence byte 0, as the tape's read (the oracle's had none); any other as
/// it is.
fn seq_cleared(mut b: Vec<u8>) -> Vec<u8> {
    use mvs_protocol::match_wire::{INPUT_BYTES, MSG_INPUT, input_header};
    if b.first() == Some(&MSG_INPUT) && b.len() > 6 {
        let inputs = usize::from(b[6]) * INPUT_BYTES;
        if let Some(n) = (1..=8).find(|&n| input_header(n) + inputs == b.len()) {
            b[input_header(n) - 1] = 0;
        }
    }
    b
}

/// A party packet of the port's with its bye's why (a ClosedWhy) as the oracle's text; any other as it is.
fn party_bye_ts(b: &[u8]) -> Vec<u8> {
    let Ok(m) = serde_json::from_slice::<Value>(&b[1..]) else { return b.to_vec() };
    if m["k"] != "bye" || !m["why"].is_object() {
        return b.to_vec();
    }
    let Ok(why) = serde_json::from_value::<ClosedWhy>(m["why"].clone()) else { return b.to_vec() };
    let mut out = vec![0x50];
    out.extend(serde_json::to_vec(&json!({ "k": "bye", "why": closed_text(&why) })).expect("a bye"));
    out
}

/// How many of `a` and `b` are equal in order (their longest common run), how many of those at another time, and
/// the first place they part (the tape's, the port's).
/// The calls compared as `compare` does, each peer's sends among themselves and the other calls among themselves:
/// the order of sends to two peers is no one's to see (a kicked member's bye goes after the core's step in the port,
/// inside it in the oracle, after the lobby.state the others are sent).
fn compare_by_peer(a: &[(f64, Value)], b: &[(f64, Value)]) -> (usize, usize, Option<String>) {
    let key = |v: &Value| v["send"].as_str().map_or(String::new(), str::to_string);
    let mut keys: Vec<String> = a.iter().chain(b).map(|x| key(&x.1)).collect();
    keys.sort();
    keys.dedup();
    let (mut eq, mut late, mut first) = (0, 0, None);
    for k in keys {
        let pick = |l: &[(f64, Value)]| l.iter().filter(|x| key(&x.1) == k).cloned().collect::<Vec<_>>();
        let (x, y) = (pick(a), pick(b));
        let (e, l, f) = compare(&x, &y);
        eq += e;
        late += l;
        if first.is_none() && !(e == x.len() && e == y.len()) {
            first = f.map(|f| format!("{} {f}", if k.is_empty() { "calls" } else { &k }));
        }
    }
    (eq, late, first)
}

fn compare(a: &[(f64, Value)], b: &[(f64, Value)]) -> (usize, usize, Option<String>) {
    let (n, m) = (a.len(), b.len());
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i].1 == b[j].1 { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut late) = (0, 0, 0);
    while i < n && j < m {
        if a[i].1 == b[j].1 {
            if a[i].0 != b[j].0 {
                late += 1;
                if std::env::var_os("MVS_TAPE_LATE").is_some() {
                    eprintln!("late: tape t {} port t {} {}", a[i].0, b[j].0, short(&a[i].1));
                }
            }
            i += 1;
            j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let first = (0..n.max(m)).find(|&k| a.get(k).map(|x| &x.1) != b.get(k).map(|x| &x.1)).map(|k| {
        let s = |x: Option<&(f64, Value)>| x.map_or("(none)".to_string(), |(t, v)| format!("t {t} {}", short(v)));
        format!("#{k}: tape {} | port {}", s(a.get(k)), s(b.get(k)))
    });
    (l[0][0] as usize, late, first)
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 400 { format!("{}...", &s[..s.floor_char_boundary(400)]) } else { s }
}

// ---------- the replay ----------

/// The tape's request in the port's form.
pub fn request_of(r: &Value) -> Option<NetRequest> {
    Some(match r["t"].as_str()? {
        "open" => {
            let g = &r["target"];
            NetRequest::Open(if g.get("host").is_some() {
                SessionTarget::Host
            } else if let Some(c) = g["join"].as_str() {
                SessionTarget::Join(c.to_string())
            } else {
                SessionTarget::Queue
            })
        }
        "msg" => NetRequest::Msg(r["text"].as_str()?.to_string()),
        "close" => NetRequest::Close,
        "match" => NetRequest::Match {
            peers: strings(&r["peers"]),
            slot: r["slot"].as_u64()? as u32,
            referee: text(&r["referee"]),
        },
        "send" => NetRequest::Send(bytes_of(&r["bytes"])),
        "matchEnd" => NetRequest::MatchEnd,
        "invite" => NetRequest::Invite { to: text(&r["to"]), name: text(&r["name"]) },
        "browse" => NetRequest::Browse { id: r["id"].as_u64()? as u32 },
        "answer" => NetRequest::Answer { id: r["id"].as_u64()? as u32, yes: r["yes"].as_bool()? },
        "presence" => NetRequest::Presence(serde_json::from_value(r["presence"].clone()).ok()?),
        "who" => NetRequest::Who { id: r["id"].as_u64()? as u32 },
        "party.invite" => NetRequest::PartyInvite { to: text(&r["to"]), name: text(&r["name"]) },
        "party.join" => NetRequest::PartyJoin { code: text(&r["code"]) },
        "party.leave" => NetRequest::PartyLeave,
        "party.say" => NetRequest::PartySay(serde_json::from_value::<PartySay>(r["say"].clone()).ok()?),
        _ => return None,
    })
}

fn kind_of(s: &str) -> BackendKind {
    match s {
        "relay" => BackendKind::Relay,
        "lan" => BackendKind::Lan,
        _ => BackendKind::Epic,
    }
}

/// An answer of the tape in the port's form, by its call's name.
fn answer_of(e: &Value) -> Result<Answer, String> {
    if e["ok"] != json!(true) {
        return Err(text(&e["error"]));
    }
    let v = &e["value"];
    Ok(match e["name"].as_str().unwrap_or("") {
        "createLobby" => Answer::Created(text(v)),
        "joinLobby" => Answer::Joined { owner: text(&v["owner"]), members: strings(&v["members"]), lobby: None },
        "searchLobbies" => Answer::Found(
            v.as_array()
                .into_iter()
                .flatten()
                .map(|f| FoundLobby {
                    id: text(&f["id"]),
                    owner: text(&f["owner"]),
                    members: num(&f["members"]) as u32,
                    max: num(&f["max"]) as u32,
                    at: num(&f["at"]),
                    info: text(&f["info"]),
                    code: text(&f["code"]),
                })
                .collect(),
        ),
        "presence.who" => Answer::Who(serde_json::from_value::<Vec<Who>>(v.clone()).unwrap_or_default()),
        _ => Answer::Done,
    })
}

/// A backend event of the tape in the port's form (None: `accept`, a check of the accept set, not an event).
fn backend_event_of(e: &Value) -> Option<BackendEvent> {
    let a = &e["args"];
    Some(match e["name"].as_str()? {
        "onLobby" => {
            let c = &a[0];
            let lobby = text(&c["lobby"]);
            BackendEvent::Lobby(match c.get("closed") {
                Some(w) => LobbyChange::Closed { lobby, why: text(w) },
                None => LobbyChange::Members { lobby, members: strings(&c["members"]) },
            })
        }
        "onPacket" => {
            let ch = a[1].as_u64().unwrap_or(0) as u32;
            BackendEvent::Packet {
                from: text(&a[0]),
                channel: if ch == 1 { Channel::Match } else { Channel::Messages },
                bytes: packet_in(ch, &bytes_of(&a[2])),
            }
        }
        "onPeerClosed" => BackendEvent::PeerClosed(text(&a[0])),
        "onLink" => BackendEvent::Link { peer: text(&a[0]), relayed: a[1].as_bool() },
        "onInvite" => BackendEvent::Invite { from: text(&a[0]), payload: text(&a[1]) },
        "onSignedOut" => BackendEvent::SignedOut(text(&a[0])),
        _ => return None,
    })
}

fn is_driver(ev: &str) -> bool {
    !matches!(ev, "eos.answer" | "eos.event")
}

/// Replays one service of the tape.
fn replay_service(s: &Value, rand: &Draws, world: usize) -> SvcReport {
    let events = s["events"].as_array().cloned().unwrap_or_default();
    let svc_n = idx(&s["index"]);
    let clock = Clock::default();
    clock.set(1000.0);
    let back = Back::default();
    // (the tape's outputs: its calls for the backend to answer, its page events to compare)
    let mut tape_events = Vec::new();
    let mut tape_calls = Vec::new();
    for e in &events {
        if e["dir"] != "out" {
            continue;
        }
        if e["ev"] == "eos.call" {
            back.lock().tape.push(CallRec {
                name: text(&e["name"]),
                args: e["args"].clone(),
                returns: e.get("returns").cloned().unwrap_or(Value::Null),
                id: e["id"].as_u64().unwrap_or(0) as u32,
                used: false,
            });
            tape_calls.push((num(&e["t"]), text(&e["name"]), e["args"].clone()));
        } else if e["ev"] == "event" {
            tape_events.push((num(&e["t"]), event_cmp(idx(&e["page"]), &e["event"], true)));
        }
    }
    let wall_clock = clock.clone();
    let rand_clock = clock.clone();
    let draws = rand.clone();
    let o = ServiceOptions {
        wall: Some(Box::new(move || wall_clock.get())),
        host_rand: Some(Box::new(move || {
            Box::new(TapeRand { draws: draws.clone(), svc: svc_n, clock: rand_clock.clone() }) as Box<dyn HostRand>
        })),
        ..ServiceOptions::default()
    };
    let mut svc = NetService::new(o, 1000.0);
    let mut r = SvcReport::default();
    let mut pages: Vec<PageId> = Vec::new();
    let mut port_events: Vec<(f64, Value)> = Vec::new();
    let mut made_seen = 0usize;
    let mut on = BackendKind::Epic;
    let mut advanced_to = 1000.0f64;
    let mut flush_at = 1000.0f64;
    // (the outs the tape has after each input, up to the next)
    let mut want_after: Vec<usize> = Vec::new();
    let mut n_out = 0usize;
    for e in &events {
        if e["dir"] == "in" {
            want_after.push(n_out);
            n_out = 0;
        } else {
            n_out += 1;
        }
    }
    want_after.push(n_out);
    want_after.remove(0);
    // (the outs made since the last input)
    let mut made_since = 0usize;
    let mut drain = |svc: &mut NetService, pages: &[PageId], t: f64, port_events: &mut Vec<(f64, Value)>| -> usize {
        let mut n = 0;
        for (i, p) in pages.iter().enumerate() {
            for ev in svc.events(*p) {
                port_events.push((t, event_cmp(i, &ts_event(&ev), false)));
                n += 1;
            }
        }
        let made = back.lock().made.len();
        n += made - made_seen;
        made_seen = made;
        n
    };
    let mut k = 0usize;
    for e in &events {
        if e["dir"] != "in" {
            continue;
        }
        let te = num(&e["t"]);
        let ev = e["ev"].as_str().unwrap_or("");
        // (the timers before it)
        while let Some(at) = svc.next_due() {
            let due = if is_driver(ev) {
                at <= advanced_to
            } else {
                at < te || (at == te && made_since < want_after.get(k.wrapping_sub(1)).copied().unwrap_or(0))
            };
            if !due {
                break;
            }
            clock.set(at);
            svc.run_first(at);
            made_since += drain(&mut svc, &pages, at, &mut port_events);
        }
        // (the oracle's microtasks after its advance)
        if is_driver(ev) && flush_at < advanced_to {
            clock.set(advanced_to);
            svc.flush(advanced_to);
            drain(&mut svc, &pages, advanced_to, &mut port_events);
            flush_at = advanced_to;
        }
        clock.set(te);
        made_since = 0;
        match ev {
            "startBackend" => {
                let kind = kind_of(e["kind"].as_str().unwrap_or(""));
                on = kind;
                let b = TapeBackend { me: text(&e["me"]), kind, st: back.clone(), clock: clock.clone() };
                svc.start_backend(kind, Box::new(b), te);
            }
            "stopBackend" => {
                let kind = kind_of(e["args"][0].as_str().unwrap_or(""));
                svc.stop_backend(kind, off_of_text(kind, e["args"][1].as_str().unwrap_or("")), te);
            }
            "configure" => {
                let c = &e["args"][0];
                svc.configure(
                    ServiceConfig {
                        relay_faults: Default::default(),
                        relay: c["relay"]["address"].as_str().map(str::to_string),
                        allow_direct: c["allowDirect"].as_bool().unwrap_or(true),
                        default_relay: c["defaultRelay"].as_str().map(str::to_string),
                        known_relays: strings(&c["knownRelays"])
                            .into_iter()
                            .map(|address| mvs_protocol::relay_wire::KnownRelay { address, id: None })
                            .collect(),
                    },
                    te,
                );
            }
            "page" => pages.push(svc.open_page(te)),
            "req" => match request_of(&e["req"]) {
                Some(m) => {
                    let p = pages.get(idx(&e["page"])).copied().unwrap_or(PageId(u32::MAX));
                    svc.take(p, m, te);
                }
                // (a presence missing a field: the oracle's presenceOf drops it, and the port's typed request has none)
                None if e["req"]["t"] == "presence" => {}
                None => r.misses.push(format!("t {te}: a request the replay cannot read: {}", e["req"])),
            },
            "eos.answer" => {
                let ask = Ask(e["id"].as_u64().unwrap_or(0) as u32);
                svc.event(Via::Backend(on), BackendEvent::Answered(ask, answer_of(e)), te);
            }
            "eos.event" if e["name"] == "accept" => {
                let peer = text(&e["args"][0]);
                let yes = back.lock().accept.contains(&peer);
                if json!(yes) != e["returns"] {
                    r.misses.push(format!("t {te}: accept {peer}: the port's {yes}, the tape's {}", e["returns"]));
                }
            }
            "eos.event" => match backend_event_of(e) {
                Some(b) => {
                    if let BackendEvent::Link { peer, relayed } = &b {
                        let mut st = back.lock();
                        if relayed.is_some() {
                            st.links.insert(peer.clone(), *relayed);
                        } else {
                            st.links.shift_remove(peer);
                        }
                    }
                    svc.event(Via::Backend(on), b, te);
                }
                None => r.misses.push(format!("t {te}: an event the replay cannot read: {}", e["name"])),
            },
            "advance" => advanced_to = te + num(&e["ms"]),
            "shutdown" => svc.shutdown(te),
            _ => r.misses.push(format!("t {te}: an input the replay cannot read: {ev}")),
        }
        made_since += drain(&mut svc, &pages, te, &mut port_events);
        k += 1;
    }
    while let Some(at) = svc.next_due().filter(|&at| at <= advanced_to) {
        clock.set(at);
        svc.run_first(at);
        drain(&mut svc, &pages, at, &mut port_events);
    }
    clock.set(advanced_to);
    svc.flush(advanced_to);
    drain(&mut svc, &pages, advanced_to, &mut port_events);
    let made = std::mem::take(&mut back.lock().made);
    let mut port_calls = calls_cmp(&made, false);
    let mut tape_calls = calls_cmp(&tape_calls, true);
    // Deviation: the port's own away rule (a dropped peer stays away until back or gone) parts from the oracle's at its
    // silent; what follows it has no tape.
    let is_away = |v: &Value| v["ev"]["text"]["t"] == "match.away";
    if let Some(cut) = port_events.iter().find(|(_, v)| is_away(v)).map(|x| x.0) {
        let left = |v: &Value| v["ev"]["text"]["t"] == "match.left" && v["ev"]["text"]["why"] == "disconnected";
        if !tape_events.iter().any(|(t, v)| *t == cut && left(v)) {
            r.misses.push(format!("t {cut}: the port's match.away where the tape has no silent peer left"));
        }
        for l in [&mut tape_events, &mut port_events, &mut tape_calls, &mut port_calls] {
            l.retain(|x| x.0 < cut);
        }
        r.cut = Some((cut, "the away"));
    }
    // Deviation: the port's own local lobby (a custom game with no connection on) where the oracle closed the session
    // as offline; what follows it has no tape.
    let is_local = |v: &Value| v["ev"]["t"] == "opened" && v["ev"]["code"] == "" && v["ev"]["me"] == "local";
    if let Some(cut) = port_events.iter().find(|(_, v)| is_local(v)).map(|x| x.0) {
        let offline = |v: &Value| {
            v["ev"]["t"] == "closed" && v["ev"]["why"].as_str().is_some_and(|w| w.starts_with("online play is off"))
        };
        if !tape_events.iter().any(|(t, v)| *t == cut && offline(v)) {
            r.misses.push(format!("t {cut}: the port's local lobby where the tape's session did not close offline"));
        }
        for l in [&mut tape_events, &mut port_events, &mut tape_calls, &mut port_calls] {
            l.retain(|x| x.0 < cut);
        }
        r.cut = Some((cut, "the local lobby"));
    }
    r.ruled = ruled_apart((&mut tape_events, &mut tape_calls), (&mut port_events, &mut port_calls), &mut r.misses);
    // (MVS_TAPE_DUMP=<world>.<service>: both streams of that service on stderr)
    if std::env::var("MVS_TAPE_DUMP").ok() == Some(format!("{world}.{svc_n}")) {
        for (name, a, b) in [("event", &tape_events, &port_events), ("call", &tape_calls, &port_calls)] {
            for (t, v) in a {
                eprintln!("tape {name} t {t} {}", short(v));
            }
            for (t, v) in b {
                eprintln!("port {name} t {t} {}", short(v));
            }
        }
    }
    let (eq, late_e, first_e) = compare(&tape_events, &port_events);
    let (cq, late_c, first_c) = compare_by_peer(&tape_calls, &port_calls);
    r.events_equal = eq;
    r.events_tape = tape_events.len();
    r.events_port = port_events.len();
    r.calls_equal = cq;
    r.calls_tape = tape_calls.len();
    r.calls_port = port_calls.len();
    r.late = late_e + late_c;
    r.first_event = first_e.filter(|_| !(eq == tape_events.len() && eq == port_events.len()));
    r.first_call = first_c.filter(|_| !(cq == tape_calls.len() && cq == port_calls.len()));
    r.misses.extend(std::mem::take(&mut *rand.misses.lock().unwrap_or_else(std::sync::PoisonError::into_inner)));
    r
}

// ---------- the codes as the oracle's text ----------

/// The oracle's backend names.
pub fn backend_name(k: BackendKind) -> &'static str {
    match k {
        BackendKind::Epic => "Epic",
        BackendKind::Relay => "the relay",
        BackendKind::Lan => "LAN",
    }
}

/// A host core's why as the oracle's text (as host_tape.rs).
pub fn host_why_text(w: &HostWhy) -> String {
    w.log_name().to_string()
}

/// An off reason of `kind` as the oracle's text.
pub fn off_text(kind: BackendKind, w: &OffWhy) -> String {
    match w {
        OffWhy::Off => "off".into(),
        OffWhy::NotOn => format!("{} is not turned on in the launcher", backend_name(kind)),
        OffWhy::SigningIn => "signing in to Epic Online Services".into(),
        OffWhy::Connecting { address } => format!("connecting to the relay at {address}"),
        OffWhy::SignInChanged => format!("the sign-in to {} changed", backend_name(kind)),
        OffWhy::LanFailed => "LAN did not start".into(),
        OffWhy::Missing { why: EpicMissing::NotInstalled } => "the online files are missing from this install".into(),
        OffWhy::Missing { why: EpicMissing::NotSetUp } => {
            "not set up in this dev run (MVS_EOS_DLL and MVS_EOS_CREDENTIALS are not set)".into()
        }
        OffWhy::Backend { text } => text.clone(),
        OffWhy::Retrying { text, in_ms } => {
            let wait = if *in_ms >= 1000 {
                format!("{} s", (f64::from(*in_ms) / 1000.0).round())
            } else {
                format!("{in_ms} ms")
            };
            format!("{text} (trying again in {wait})")
        }
        OffWhy::SettingsChanged => "the online settings changed".into(),
        OffWhy::TurnedOff => "turned off in the online settings".into(),
        OffWhy::LanPortChanged => "the LAN port changed".into(),
        OffWhy::LanSettingChanged => "the LAN setting changed".into(),
        // (the port's own, wire 7: no oracle tape holds one)
        OffWhy::RelayKey { release, in_ms } => format!("relay key (release {release}, {in_ms} ms)"),
    }
}

/// An oracle off text as the port's code (the code whose text it is, else the backend's own words).
pub fn off_of_text(kind: BackendKind, t: &str) -> OffWhy {
    let codes = [
        OffWhy::Off,
        OffWhy::NotOn,
        OffWhy::SigningIn,
        OffWhy::SignInChanged,
        OffWhy::LanFailed,
        OffWhy::SettingsChanged,
        OffWhy::TurnedOff,
        OffWhy::LanPortChanged,
        OffWhy::LanSettingChanged,
    ];
    if t.is_empty() {
        return OffWhy::Off;
    }
    codes.into_iter().find(|c| off_text(kind, c) == t).unwrap_or(OffWhy::Backend { text: t.to_string() })
}

/// The status's why as the oracle's text.
pub fn status_text(w: &StatusWhy) -> String {
    match w {
        StatusWhy::Online => String::new(),
        StatusWhy::SigningIn => "signing in to online play".into(),
        StatusWhy::Off { backends } => backends
            .iter()
            .map(|b| format!("{}: {}", backend_name(b.kind), off_text(b.kind, &b.why)))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// A browse's or the online list's why as the oracle's (why, text).
pub fn browse_text(w: &BrowseWhy) -> (&'static str, String) {
    match w {
        BrowseWhy::All => ("", String::new()),
        BrowseWhy::Offline { why } => ("offline", format!("online play is off: {}", status_text(why))),
        BrowseWhy::ListOff { why } => (
            "offline",
            format!(
                "the online list is the relay's, and the relay is not connected ({}): Epic and LAN have none",
                off_text(BackendKind::Relay, why)
            ),
        ),
        BrowseWhy::NoList => ("failed", "this relay has no online list (an older relay)".into()),
        BrowseWhy::ListFailed { text } => ("failed", format!("the relay did not give the online list: {text}")),
        BrowseWhy::Failed { backends } => (
            "failed",
            backends.iter().map(|b| format!("{}: {}", backend_name(b.kind), b.text)).collect::<Vec<_>>().join("; "),
        ),
    }
}

/// A lobby code's why as the oracle's text.
pub fn code_why_text(w: CodeWhy) -> String {
    match w {
        CodeWhy::Empty => "there is no lobby code".into(),
        CodeWhy::UnknownKind => {
            "that is not a lobby code this app can read (mistyped, or made by a newer version of the app)".into()
        }
        CodeWhy::Mistyped => "that lobby code is mistyped (a character is wrong, missing or extra)".into(),
        CodeWhy::Version(v) => {
            format!("that lobby code is of another version ({v}): one of the two apps needs an update")
        }
        CodeWhy::Spelled => {
            "that lobby code is not written the way its host writes it: copy it from the host again".into()
        }
        CodeWhy::DirectMistyped => "that direct code is mistyped (a character is wrong, missing or extra)".into(),
        CodeWhy::DirectVersion(v) => {
            format!("that direct code is of another version ({v}): one of the two apps needs an update")
        }
        CodeWhy::DirectOutside => "that direct code names an address outside a local network".into(),
    }
}

/// A join's why as the oracle's text.
pub fn join_text(w: &JoinWhy) -> String {
    match w {
        JoinWhy::BadCode { why } => code_why_text(*why),
        JoinWhy::NoDirect => "direct play is not set up in this app".into(),
        JoinWhy::EpicOff { why } => format!(
            "the lobby is on Epic Online Services, and this app is not signed in to Epic ({})",
            off_text(BackendKind::Epic, why)
        ),
        JoinWhy::NoDefaultRelay => "its code names the release's relay, and this app has none".into(),
        JoinWhy::ShortOtherRelay => {
            "a short code names a lobby on the release's relay, and this app uses another".into()
        }
        JoinWhy::RelayOff { address, why } => {
            format!("its relay ({address}) is not connected: {}", off_text(BackendKind::Relay, why))
        }
        JoinWhy::NoRelayLink => "a link to another relay is not set up in this app".into(),
        JoinWhy::RelayNotTaken { address } => format!("the relay at {address} was not taken"),
        JoinWhy::RelayRefused { address, text } => format!("the relay at {address} did not take the link: {text}"),
        JoinWhy::Gone => "the lobby is gone (its host closed it, or the code is old)".into(),
        JoinWhy::Full => "the lobby is full".into(),
        JoinWhy::Direct { why } => why.log_text(),
        JoinWhy::Backend { text } => text.clone(),
    }
}

/// A party join's why as the oracle's text.
pub fn party_join_text(w: &PartyJoinWhy) -> String {
    match w {
        PartyJoinWhy::NoInvite => "no party invite had that code".into(),
        PartyJoinWhy::NotRelay => "its code names no relay lobby".into(),
        PartyJoinWhy::OtherRelay { address } => {
            format!("its relay ({}) is not this app's", address.as_deref().unwrap_or("the default"))
        }
        PartyJoinWhy::RelayOff { why } => {
            format!("the relay is not connected: {}", off_text(BackendKind::Relay, why))
        }
        PartyJoinWhy::NotInviter => "its leader is not who invited this player".into(),
        PartyJoinWhy::Backend { text } => text.clone(),
        PartyJoinWhy::BadCode { why } => code_why_text(*why),
    }
}

/// The text of NO_QUEUE.
const NO_QUEUE: &str = "the queue needs the relay or Epic, and neither is signed in (LAN has no queue): host a custom \
                        lobby, or turn on the relay or Epic in the launcher";

/// A session's or a party's end as the oracle's text.
pub fn closed_text(w: &ClosedWhy) -> String {
    match w {
        ClosedWhy::Left => "left".into(),
        ClosedWhy::Replaced => "replaced".into(),
        ClosedWhy::Stopped => "the net process stopped".into(),
        ClosedWhy::NoSession => "no such session".into(),
        ClosedWhy::Offline { why } => format!("online play is off: {}", status_text(why)),
        ClosedWhy::NoQueue => format!("online play is off: {NO_QUEUE}"),
        ClosedWhy::RelayVersion => "the relay runs another version of the game".into(),
        ClosedWhy::NotRelayed { text } => format!("Epic could not keep this match relayed: {text}"),
        ClosedWhy::BackendOff { kind, why } => format!("{} is off: {}", backend_name(*kind), off_text(*kind, why)),
        ClosedWhy::RelayLost { text } => format!("the link to the relay was lost: {text}"),
        ClosedWhy::Limited => "limited".into(),
        ClosedWhy::MakeFailed { why } => format!("the lobby could not be made: {why}"),
        ClosedWhy::MakeHostFailed { why } => format!("the lobby could not be made: {why}"),
        ClosedWhy::MakeCodeFailed { why } => format!("the lobby could not be made: {why}"),
        ClosedWhy::JoinFailed { why } => format!("the lobby could not be joined: {}", join_text(why)),
        ClosedWhy::NoHost => "the lobby has no host".into(),
        ClosedWhy::HostSilent => "the host did not answer".into(),
        ClosedWhy::BadPacket => "a bad packet from the host".into(),
        ClosedWhy::BadGuestPacket => "bad packet".into(),
        ClosedWhy::HostClosed => "the host closed the link".into(),
        ClosedWhy::HostLeft => "the host left".into(),
        ClosedWhy::Kicked => "kicked".into(),
        ClosedWhy::LobbyClosed => "the lobby closed".into(),
        ClosedWhy::LobbyGone { text } => format!("the lobby closed: {text}"),
        ClosedWhy::HostLost => "the link to the host was lost".into(),
        ClosedWhy::HostLinkLost { text } => format!("the link to the host was lost: {text}"),
        ClosedWhy::DirectStopped { text } => format!("the direct host stopped: {text}"),
        ClosedWhy::SendFailed => "the link to the host failed".into(),
        ClosedWhy::Host { why } => host_why_text(why),
        ClosedWhy::PartyMakeFailed { why } => format!("the party could not be made: {why}"),
        ClosedWhy::PartyMakeCodeFailed { why } => format!("the party could not be made: {why}"),
        ClosedWhy::NoLeader => "the party has no leader".into(),
        ClosedWhy::PartyJoinFailed { why } => format!("the party could not be joined: {}", party_join_text(why)),
        ClosedWhy::LeaderSilent => "the party leader did not answer".into(),
        ClosedWhy::LeaderLeft => "the party leader left".into(),
        ClosedWhy::LeaderLost => "the link to the party leader was lost".into(),
        ClosedWhy::Other { text } => text.clone(),
        ClosedWhy::PartyEnded => "the party ended".into(),
        ClosedWhy::PartyEndedBy { why } => format!("the party ended: {}", closed_text(why)),
        ClosedWhy::PartyLobbyClosed { text } => format!("the party lobby closed: {text}"),
        ClosedWhy::QueueMoved => "the queue moved on".into(),
        ClosedWhy::QueueUnmatched => "the queue found a match without you".into(),
        ClosedWhy::MakeNoRelay => "the lobby could not be made: the relay's address is not in the online config".into(),
        ClosedWhy::PartyMakeNoRelay => {
            "the party could not be made: the relay's address is not in the online config".into()
        }
        // (the port's own: no oracle text)
        ClosedWhy::BadAnswer => "the online service's answer could not be read".into(),
    }
}

/// An oracle bye's text as the port's code: the code of no field whose text it is (a host core's first), None else.
pub fn closed_of_text(t: &str) -> Option<ClosedWhy> {
    let host = [
        HostWhy::HostLeft,
        HostWhy::Kicked,
        HostWhy::Replaced,
        HostWhy::Protocol,
        HostWhy::NotInvited,
        HostWhy::MessageTooBig,
        HostWhy::TooManyMessages,
        HostWhy::LinkError,
        HostWhy::Ended,
    ]
    .into_iter()
    .map(|why| ClosedWhy::Host { why });
    let plain = [
        ClosedWhy::Left,
        ClosedWhy::Replaced,
        ClosedWhy::Stopped,
        ClosedWhy::NoSession,
        ClosedWhy::Limited,
        ClosedWhy::NoHost,
        ClosedWhy::HostSilent,
        ClosedWhy::BadPacket,
        ClosedWhy::BadGuestPacket,
        ClosedWhy::HostClosed,
        ClosedWhy::HostLeft,
        ClosedWhy::Kicked,
        ClosedWhy::LobbyClosed,
        ClosedWhy::HostLost,
        ClosedWhy::SendFailed,
        ClosedWhy::NoLeader,
        ClosedWhy::LeaderSilent,
        ClosedWhy::LeaderLeft,
        ClosedWhy::LeaderLost,
        ClosedWhy::PartyEnded,
        ClosedWhy::QueueMoved,
        ClosedWhy::QueueUnmatched,
    ];
    host.chain(plain).find(|w| closed_text(w) == t)
}

/// A page event as the oracle's NetEvent JSON.
pub fn ts_event(e: &NetEvent) -> Value {
    match e {
        NetEvent::Status { online, me, why } => {
            json!({ "t": "status", "online": online, "me": me, "why": status_text(why) })
        }
        NetEvent::Opened { code, me, chosen, .. } => json!({ "t": "opened", "code": code, "me": me, "chosen": chosen }),
        NetEvent::Msg(text) => json!({ "t": "msg", "text": text }),
        NetEvent::Closed(why) => json!({ "t": "closed", "why": closed_text(why) }),
        NetEvent::Packets { from, packets } => json!({
            "t": "packets",
            "from": from,
            "packets": packets.iter().map(|p| json!({ "$bytes": hex(p) })).collect::<Vec<_>>(),
        }),
        NetEvent::Invited { kind, code, from_id, from_name } => json!({
            "t": "invited",
            "kind": match kind { InviteKind::Custom => "custom", InviteKind::Party => "party" },
            "code": code,
            "from": { "id": from_id, "name": from_name },
        }),
        NetEvent::Who { id, players, why } => {
            let (w, text) = browse_text(why);
            json!({ "t": "who", "id": id, "players": players, "why": w, "text": text })
        }
        NetEvent::Party { party, why } => json!({
            "t": "party",
            "party": party.as_ref().map(|p| json!({
                "leader": p.leader,
                "members": p.members.iter().map(|m| json!({ "id": m.id, "name": m.name, "icon": m.icon })).collect::<Vec<_>>(),
                "me": p.me,
            })),
            "why": if party.is_some() || *why == ClosedWhy::Left { String::new() } else { closed_text(why) },
        }),
        NetEvent::PartySay { from, say } => json!({ "t": "party.say", "from": from, "say": say }),
        // (the port's own: no oracle tape has one)
        NetEvent::PartyFighter { from, fighter } => json!({ "t": "party.fighter", "from": from, "fighter": fighter }),
        NetEvent::InviteRefused { to } => json!({ "t": "inviteRefused", "to": to }),
        NetEvent::Browsed { id, lobbies, why } => {
            let (w, text) = browse_text(why);
            json!({
                "t": "browsed",
                "id": id,
                "lobbies": lobbies.iter().map(|l| json!({
                    "backend": l.backend.id(), "code": l.code, "members": l.members, "max": l.max, "info": l.info,
                })).collect::<Vec<_>>(),
                "why": w,
                "text": text,
            })
        }
        NetEvent::Ask { id, what } => match what {
            AskWhat::NewRelay { address } => json!({
                "t": "ask",
                "id": id,
                "text": format!(
                    "This lobby is on the relay at {address}, which you have not played on before. Join it through that relay?"
                ),
                "yes": "Join",
                "no": "Cancel",
            }),
            // (the port's own, wire 7: no oracle tape holds one)
            AskWhat::RelayKeyChanged { address } => json!({ "t": "ask", "id": id, "relayKeyChanged": address }),
        },
        NetEvent::Link { peer, relayed } => json!({ "t": "link", "peer": peer, "relayed": relayed }),
        // (the port's own: no oracle tape holds one, and the tapes' memory backend has no UDP link)
        NetEvent::Way { slow } => json!({ "t": "way", "slow": slow }),
    }
}
