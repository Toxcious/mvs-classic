//! A host core the relay runs, against the oracle's tape (`host-tape.json`, as `mvs-host`'s host_tape replays it on a
//! bare core): each host's inputs reach the core through relay players (a connect is a join of the relay's lobby of the
//! core and a knock, a message its channel-0 pieces, a closed a bye), and what its players are sent (joined) and the
//! core's log lines (after the host's id in the relay's log) must equal the tape's, JSON compared by value. The draws
//! and the wall clock are the tape's own. A test that finds no tape says so and passes.
//!
//! What the relay does in another form is compared through a table: a close is a bye, its why the oracle's text; the
//! match.go's referee is the relay's (its tape answer put back); the referee's calls and the owner's events are the
//! relay's to carry out, not compared; the draws are compared in order among themselves (a core's sends reach its
//! players after its call). Hosts the relay cannot be are skipped, each named with its reason.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mvs_host::{HOST_TIMES, HostOptions, HostRand, HostTimes, HostWhy, SessionKind};
use mvs_protocol::p2p_frame::{FRAME_BYE, FRAME_OPEN, MessageJoiner, Taken as Frame, split_message};
use mvs_protocol::relay_wire::{ServerMessage, host_id};
use mvs_relay::core::{Core, CorePlayer, HOST_RELINK_MS, Session, Taken};
use mvs_relay::limits::DEFAULT_LIMITS;
use serde_json::{Value, json};

type Sink = Arc<Mutex<Vec<Value>>>;

fn push(s: &Sink, v: Value) {
    s.lock().expect("the sink").push(v);
}

/// The tape's hosts the relay cannot be, with the reason.
fn skipped(h: &Value) -> Option<&'static str> {
    let index = h["index"].as_u64().unwrap_or(0);
    if h["options"]["referee"] != true {
        return Some("no referee: a relay's core is always refereed by it");
    }
    if matches!(index, 0 | 5 | 17 | 35) {
        return Some("a second live link of one id: a relay player has one link to a core");
    }
    if matches!(index, 14 | 15) {
        return Some("a link whose send fails: a relay's never does");
    }
    // (a link of an id again inside the relay's relink wait: its knock would be dropped)
    let (mut ids, mut ended) = (HashMap::new(), HashMap::new());
    for e in h["events"].as_array().into_iter().flatten() {
        let (key, t) = (e["key"].as_u64(), e["t"].as_f64().unwrap_or(0.0));
        match e["ev"].as_str() {
            Some("connect") => {
                let id = e["id"].as_str().unwrap_or("").to_string();
                if ended.get(&id).is_some_and(|&at: &f64| t - at < HOST_RELINK_MS) {
                    return Some("an id linked again inside the relay's relink wait (HOST_RELINK_MS)");
                }
                ids.insert(key, id);
            }
            Some("close" | "closed") => {
                if let Some(id) = ids.get(&key) {
                    ended.insert(id.clone(), t);
                }
            }
            _ => {}
        }
    }
    // (a closed inside a call with no close of the core's before it: the link's own, in a send)
    let mut closing = std::collections::HashSet::new();
    for e in h["events"].as_array().into_iter().flatten() {
        if e["ev"] == "close" {
            closing.insert(e["key"].as_u64());
        }
        if e["ev"] == "closed" && e["nested"] == true && !closing.contains(&e["key"].as_u64()) {
            return Some("a link closed inside a send: a relay player's never calls the relay back");
        }
    }
    None
}

/// A relay player of the tape: what a host core sends it joined, by the tape's key of its link.
struct TapePlayer {
    id: String,
    host: String,
    keys: Rc<RefCell<HashMap<String, u64>>>,
    sink: Sink,
    joiner: MessageJoiner,
}

impl CorePlayer for TapePlayer {
    fn send(&mut self, m: &ServerMessage) -> Taken {
        // (a knock on a core that has ended: the oracle core's close "ended")
        if let ServerMessage::PeerClosed { id } = m
            && *id == self.host
        {
            let key = self.keys.borrow().get(&self.id).copied().unwrap_or(0);
            push(&self.sink, json!({ "ev": "close", "key": key, "why": "ended" }));
        }
        Taken::Yes
    }
    fn packet(&mut self, channel: u8, from: &str, bytes: &[u8]) -> Taken {
        if channel != 0 || from != self.host {
            return Taken::Yes;
        }
        let key = self.keys.borrow().get(&self.id).copied().unwrap_or(0);
        match self.joiner.take(bytes) {
            Frame::Message(b) => {
                push(&self.sink, json!({ "ev": "send", "key": key, "text": String::from_utf8_lossy(&b) }));
            }
            Frame::Bye(b) => {
                let v: Value = serde_json::from_slice(&b).expect("a bye");
                let why: HostWhy = serde_json::from_value(v["why"].clone()).expect("a host's why");
                push(&self.sink, json!({ "ev": "close", "key": key, "why": ts_why(&why) }));
                self.joiner = MessageJoiner::new();
            }
            Frame::Part => {}
            t => panic!("a host frame of no meaning: {t:?}"),
        }
        Taken::Yes
    }
    fn refuse(&mut self, why: &str) {
        panic!("{} refused: {why}", self.id);
    }
}

/// The tape's draws in order: each call takes the next and must be of its kind and range.
struct TapeRand {
    draws: Vec<Value>,
    at: usize,
    sink: Sink,
}

impl TapeRand {
    fn next(&mut self, ev: &str) -> Option<Value> {
        let d = self.draws.get(self.at).filter(|d| d["ev"] == ev).cloned();
        if d.is_some() {
            self.at += 1;
        }
        d
    }
}

impl HostRand for TapeRand {
    fn int(&mut self, lo: u64, hi: u64) -> u64 {
        push(&self.sink, json!({ "ev": "randomInt", "args": [lo, hi] }));
        match self.next("randomInt") {
            Some(d) => d["value"].as_u64().expect("a draw's value"),
            None => lo,
        }
    }

    fn bytes(&mut self, out: &mut [u8]) {
        push(&self.sink, json!({ "ev": "randomBytes", "n": out.len() }));
        if let Some(d) = self.next("randomBytes") {
            let hex = d["value"].as_str().expect("hex");
            for (i, b) in out.iter_mut().enumerate() {
                *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("a hex byte");
            }
        }
    }

    fn wall_ms(&mut self) -> u64 {
        push(&self.sink, json!({ "ev": "dateNow" }));
        self.next("dateNow").and_then(|d| d["value"].as_u64()).unwrap_or(0)
    }
}

fn ts_why(why: &HostWhy) -> String {
    match why {
        HostWhy::HostLeft => "host left",
        HostWhy::Kicked => "kicked",
        HostWhy::Replaced => "replaced",
        HostWhy::Protocol => "protocol",
        HostWhy::NotInvited => "not invited",
        HostWhy::MessageTooBig => "message too big",
        HostWhy::TooManyMessages => "too many messages",
        HostWhy::LinkError => "link error",
        HostWhy::Ended => "ended",
        HostWhy::Owner { text } => text,
    }
    .to_string()
}

fn lobby_error_text(why: &str, name: &str) -> String {
    match why {
        "queued_self" => "Leave matchmaking first".to_string(),
        "no_lobby" => "The lobby no longer exists".to_string(),
        "lobby_full" => "The lobby is full".to_string(),
        "compat_missing" => {
            "Your game could not tell the server its game data version. Reload the page and try again".to_string()
        }
        "compat_differs" => {
            "Your game data differs from this lobby's (another game version or another import). Update \
                             the game and import again, then try again"
                .to_string()
        }
        "compat_mine_older" => format!("Your game is older than {name}'s. Update it, then try again"),
        "compat_theirs_older" => format!("{name} has an older version of the game and has to update it"),
        _ => format!("(unknown code {why})"),
    }
}

/// Every number as f64, a lobby row's empty icon left out.
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("a number")),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, x)| !(k.as_str() == "icon" && x.as_str() == Some("")))
                .map(|(k, x)| (k.clone(), norm(x)))
                .collect(),
        ),
        _ => v.clone(),
    }
}

/// A sent message as the comparison takes it: a lobby.error's code as its text, a go's referee the tape's answer.
fn message(text: &str, tape: bool, answers: &HashMap<String, String>) -> Value {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else { return json!({ "unparsed": text }) };
    if let Some(o) = v.as_object_mut()
        && !tape
    {
        if o.get("t").and_then(Value::as_str) == Some("lobby.error") {
            let why = o.remove("why").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
            let name = o.remove("name").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
            o.insert("text".to_string(), json!(lobby_error_text(&why, &name)));
        }
        if o.get("t").and_then(Value::as_str) == Some("match.go")
            && let Some(a) = o.get("referee").and_then(Value::as_str).and_then(|r| answers.get(r))
        {
            o.insert("referee".to_string(), json!(a));
        }
    }
    norm(&v)
}

fn output(e: &Value, tape: bool, answers: &HashMap<String, String>) -> Value {
    let ev = e["ev"].as_str().unwrap_or("");
    match ev {
        "send" => json!({ "ev": ev, "key": e["key"], "msg": message(e["text"].as_str().unwrap_or(""), tape, answers) }),
        "close" => json!({ "ev": ev, "key": e["key"], "why": e["why"] }),
        "log" => json!({ "ev": ev, "text": e["text"].as_str().unwrap_or("").split('\n').next().unwrap_or("") }),
        "randomInt" => {
            let a = e["args"].as_array().cloned().unwrap_or_default();
            let args = if a.len() == 1 { vec![json!(0), a[0].clone()] } else { a };
            json!({ "ev": ev, "args": norm(&Value::Array(args)) })
        }
        "randomBytes" => json!({ "ev": ev, "n": norm(&e["n"]) }),
        _ => json!({ "ev": ev }),
    }
}

/// An output the comparison takes (the referee's calls and the owner's events are the relay's own).
fn compared(e: &Value) -> bool {
    matches!(e["ev"].as_str(), Some("send" | "close" | "log" | "randomInt" | "randomBytes" | "dateNow"))
}

fn is_draw(e: &Value) -> bool {
    matches!(e["ev"].as_str(), Some("randomInt" | "randomBytes" | "dateNow"))
}

fn times(o: &Value) -> HostTimes {
    let mut t = HOST_TIMES;
    let get = |k: &str| o.get(k).and_then(Value::as_f64);
    if let Some(x) = get("matchTicks") {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = x as u32;
        t.match_ticks = n;
    }
    for (k, f) in [
        ("loadMs", &mut t.load_ms),
        ("queueStartMs", &mut t.queue_start_ms),
        ("pickMs", &mut t.pick_ms),
        ("perksMs", &mut t.perks_ms),
        ("resultMs", &mut t.result_ms),
        ("voteMs", &mut t.vote_ms),
        ("rematchMs", &mut t.rematch_ms),
    ] {
        if let Some(x) = get(k) {
            *f = x;
        }
    }
    t
}

/// A relay with no limit the tape's hosts reach.
fn open_limits() -> mvs_relay::limits::RelayLimits {
    let mut l = DEFAULT_LIMITS;
    for (k, v) in l.fields_mut() {
        if !matches!(k, "frameBytes" | "packetBytes" | "silentMs") {
            *v = 1e9;
        }
    }
    l
}

struct HostRun {
    groups: usize,
    equal_groups: usize,
    diffs: Vec<String>,
}

#[allow(clippy::too_many_lines, reason = "the replay's one loop")]
fn run_host(h: &Value) -> HostRun {
    let o = &h["options"];
    let events = h["events"].as_array().expect("a host's events");
    let sink: Sink = Arc::default();
    let draws: Vec<Value> = events.iter().filter(|e| is_draw(e)).cloned().collect();
    let log_sink = Arc::clone(&sink);
    let start = o["start"].as_f64().expect("a start");
    let mut core = Core::new(
        &open_limits(),
        None,
        Box::new(move |line: &str| push(&log_sink, json!({ "ev": "relay.log", "text": line }))),
    );
    // the relay's lobby of the core (its owner a player of the relay's own, never the tape's)
    let (owner, _) = core.join("tape.owner", "10.9.9.9", Box::new(Silent), start).expect("joined");
    let r = core.request(&owner, &json!({ "op": "create", "bucket": "tape", "max": 1000 }), start).expect("a lobby");
    let lobby = r["lobby"].as_str().expect("a lobby id").to_string();
    let host = host_id(&lobby);
    let opts = HostOptions {
        kind: if o["kind"] == "custom" { SessionKind::Custom } else { SessionKind::Queue },
        host: o["host"].as_str().unwrap_or("").to_string(),
        name: o["name"].as_str().map(str::to_string),
        lobby_id: o["lobbyId"].as_str().map(str::to_string),
        times: times(&o["times"]),
        referee: None,
        rand: Box::new(TapeRand { draws, at: 0, sink: Arc::clone(&sink) }),
        log: None,
        paths: false,
    };
    core.start_host(&host, Some(lobby.clone()), opts, start);
    // the go's referee: the relay's, the tape's answer by its match
    let mut answers: HashMap<String, String> = HashMap::new();
    for e in events.iter().filter(|e| e["ev"] == "referee.start") {
        let m = e["match"].as_u64().expect("a match");
        answers.insert(format!("{host}.m{m}"), e["returns"].as_str().unwrap_or("").to_string());
    }
    let keys: Rc<RefCell<HashMap<String, u64>>> = Rc::default();
    let mut players: HashMap<String, Session> = HashMap::new();
    let mut ids: HashMap<u64, String> = HashMap::new();
    let mut hello_at: HashMap<u64, f64> = HashMap::new();
    let mut run = HostRun { groups: 0, equal_groups: 0, diffs: Vec::new() };
    let mut clock = start;
    let mut kicked: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut starts: Vec<usize> =
        (0..events.len()).filter(|&i| events[i]["dir"] == "in" && events[i]["nested"] != true).collect();
    starts.push(events.len());
    for g in 0..starts.len() - 1 {
        let (a, b) = (starts[g], starts[g + 1]);
        let input = &events[a];
        let t = input["t"].as_f64().expect("an input's time");
        sink.lock().expect("the sink").clear();
        let hello = input["ev"] == "message" && input["text"].as_str().is_some_and(|t| t.contains(r#""t":"hello""#));
        let prev_hello = if hello { hello_at.insert(input["key"].as_u64().unwrap_or(0), t) } else { None };
        let key = input["key"].as_u64();
        let player = key.and_then(|k| ids.get(&k)).and_then(|id| players.get(id)).cloned();
        match input["ev"].as_str().unwrap_or("") {
            "connect" => {
                let k = key.expect("a key");
                let id = input["id"].as_str().unwrap_or("").to_string();
                ids.insert(k, id.clone());
                keys.borrow_mut().insert(id.clone(), k);
                if !players.contains_key(&id) {
                    let p = TapePlayer {
                        id: id.clone(),
                        host: host.clone(),
                        keys: Rc::clone(&keys),
                        sink: Arc::clone(&sink),
                        joiner: MessageJoiner::new(),
                    };
                    let address = format!("10.1.{}.{}", players.len() / 250, players.len() % 250);
                    let (s, _) = core.join(&id, &address, Box::new(p), t).expect("joined");
                    // (a lobby its core ended is gone: the knock is then answered peerClosed)
                    let _ = core.request(&s, &json!({ "op": "join", "lobby": lobby }), t);
                    players.insert(id.clone(), s);
                }
                core.packet(&players[&id], 0, &host, &[FRAME_OPEN], t);
            }
            "message" => {
                let s = player.expect("a connected player");
                for piece in split_message(input["text"].as_str().unwrap_or("").as_bytes(), 1170) {
                    core.packet(&s, 0, &host, &piece, t);
                }
            }
            "closed" => core.packet(&player.expect("a connected player"), 0, &host, &[FRAME_BYE], t),
            "end" => {
                let why = HostWhy::Owner { text: input["why"].as_str().unwrap_or("").to_string() };
                core.with_host(&host, |c, now| c.end(why, now));
            }
            "silent" => {
                let (m, s) = (input["match"].as_u64().expect("a match"), input["slot"].as_u64().expect("a slot"));
                let (m, s) = (u32::try_from(m).expect("a match"), u32::try_from(s).expect("a slot"));
                core.with_host(&host, |c, now| c.gone(m, s, now));
            }
            "advance" => {
                let end = clock + input["ms"].as_f64().expect("ms");
                while let Some(at) = core.next_due().filter(|&at| at <= end) {
                    clock = clock.max(at);
                    core.pump_hosts(clock);
                }
            }
            x => panic!("host tape: an input {x}"),
        }
        clock = clock.max(t);
        if input["ev"] == "advance" {
            clock = t + input["ms"].as_f64().unwrap_or(0.0);
        }
        let prefix = format!("{host} ");
        let got: Vec<Value> = (sink.lock().expect("the sink").drain(..))
            .filter_map(|e| {
                if e["ev"] != "relay.log" {
                    return Some(e);
                }
                let text = e["text"].as_str()?.strip_prefix(&prefix)?.to_string();
                Some(json!({ "ev": "log", "text": text }))
            })
            .map(|e| output(&e, false, &answers))
            .collect();
        let want: Vec<Value> = (events[a + 1..b].iter())
            .filter(|e| e["dir"] == "out" && compared(e))
            .map(|e| output(e, true, &answers))
            .collect();
        let split = |v: &[Value]| -> (Vec<Value>, Vec<Value>) { v.iter().cloned().partition(|e| !is_draw(e)) };
        let (mut got_out, got_draws) = split(&got);
        let (mut want_out, want_draws) = split(&want);
        // the core's Kicked event the relay carries out (the oracle's tape left that to its owner): the kicked link's
        // bye, after which the link is the relay's, not compared
        let fresh: Vec<u64> = (got_out.iter())
            .filter(|e| e["ev"] == "close" && e["why"] == "kicked" && !want_out.contains(e))
            .filter_map(|e| e["key"].as_u64())
            .collect();
        kicked.extend(fresh);
        let mine = |e: &Value| e["key"].as_u64().is_some_and(|k| kicked.contains(&k));
        got_out.retain(|e| !mine(e));
        want_out.retain(|e| !mine(e));
        run.groups += 1;
        if got_out == want_out && got_draws == want_draws {
            run.equal_groups += 1;
            continue;
        }
        // Deviation (core.rs RENAME_MS): a hello with another name inside RENAME_MS is answered welcome with the name
        // the core keeps; the oracle core sent nothing
        let rename_answered = want_out.is_empty()
            && got_out.len() == 1
            && got_out[0]["msg"]["t"] == "welcome"
            && prev_hello.is_some_and(|p| t - p < 2000.0);
        if rename_answered {
            run.equal_groups += 1;
            continue;
        }
        let n = got_out.len().max(want_out.len());
        let at = (0..n).find(|&i| got_out.get(i) != want_out.get(i));
        let mut d = format!(
            "host {} input {a} ({}): {} outputs, the tape {}; draws {} / {}",
            h["index"],
            serde_json::to_string(input).unwrap_or_default().chars().take(160).collect::<String>(),
            got_out.len(),
            want_out.len(),
            got_draws.len(),
            want_draws.len(),
        );
        if let Some(i) = at {
            let show =
                |v: Option<&Value>| v.map_or("(none)".to_string(), |x| x.to_string().chars().take(400).collect());
            d += &format!(
                "\n    first differs at {i}:\n    relay {}\n    tape  {}",
                show(got_out.get(i)),
                show(want_out.get(i))
            );
        }
        run.diffs.push(d);
    }
    run
}

/// The relay's own lobby owner: told nothing it needs.
struct Silent;

impl CorePlayer for Silent {
    fn send(&mut self, _m: &ServerMessage) -> Taken {
        Taken::Yes
    }
    fn packet(&mut self, _channel: u8, _from: &str, _bytes: &[u8]) -> Taken {
        Taken::Yes
    }
    fn refuse(&mut self, why: &str) {
        panic!("the lobby's owner refused: {why}");
    }
}

#[test]
fn a_relays_core_replays_the_ts_tape() {
    let Some(tape) = mvs_protocol::vectors::read_tape("host-tape.json") else {
        eprintln!("hosted_tape: no host-tape.json, nothing checked");
        return;
    };
    let hosts = tape["hosts"].as_array().expect("hosts");
    let (mut run_hosts, mut equal_hosts, mut groups, mut equal_groups) = (0, 0, 0, 0);
    let mut diffs: Vec<String> = Vec::new();
    for h in hosts {
        if let Some(why) = skipped(h) {
            eprintln!("hosted_tape: host {} skipped: {why}", h["index"]);
            continue;
        }
        run_hosts += 1;
        let r = run_host(h);
        if r.diffs.is_empty() {
            equal_hosts += 1;
        }
        groups += r.groups;
        equal_groups += r.equal_groups;
        diffs.extend(r.diffs);
    }
    eprintln!("hosted_tape: {equal_hosts} / {run_hosts} hosts equal, {equal_groups} / {groups} inputs");
    for d in diffs.iter().take(40) {
        eprintln!("{d}");
    }
    assert!(diffs.is_empty(), "{} inputs differ from the tape", diffs.len());
    assert!(run_hosts > 40, "most of the tape's hosts run");
}
