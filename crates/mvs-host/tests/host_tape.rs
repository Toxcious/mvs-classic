//! The host core against the oracle's tape (`host-tape.json`: the oracle's host test run on its
//! core): each host's inputs are replayed on a Core and every output must equal the tape's, JSON compared by value
//! (numbers as f64). The draws, the wall clock and the referee's answers are the tape's own. A test that finds no
//! tape says so and passes. The tape is the oracle's widened to PROTOCOL 23 (the oracle's fixture widener), read at
//! this one's (`vectors::read_tape`).
//!
//! The referee's away and back (the port's own) have no tape: a host with silent humans is replayed once more with
//! each silent first told away and back, which must tell the match's other peers (those the tape tells it left) and
//! change nothing else.
//!
//! What the port writes in another form is compared through one table each: a lobby.error's code is its oracle text
//! (`lobby_error_text`), a close's or an end's code its oracle text (`ts_why`), a log line is compared to its first
//! line (the oracle's holds a stack), a lobby row's empty icon is left out (the oracle keeps an `icon: ''`). The core's
//! events are compared in order among themselves (the oracle calls them inside the step, the port hands them over after
//! it); onListing is on for no host of the tape.
//!
//! The lobby's style is the port's own (`LobbyStyle`: Versus and Free For All), the tape's the build's ETeamStyle:
//! `vectors::read_tape` moves the tape's to this build's (1v1 and 2v2 are Versus). A host whose lobby the port's
//! Versus answers otherwise (`VERSUS_DEPARTS`) is replayed up to the first input it answers otherwise.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::ManualClock;
use mvs_host::{
    ConnKey, Connection, Core, HOST_TIMES, HostEvent, HostOptions, HostRand, HostTimes, HostWhy, MatchReferee,
    RefereeStart, SessionKind,
};
use serde_json::{Value, json};

type Sink = Arc<Mutex<Vec<Value>>>;

fn push(s: &Sink, v: Value) {
    s.lock().expect("the sink").push(v);
}

struct TapePeer {
    id: String,
    name: String,
    key: u64,
    sink: Sink,
    sends: usize,
    throw_at: Option<usize>,
}

impl Connection for TapePeer {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        let threw = self.throw_at.is_some_and(|n| self.sends >= n);
        self.sends += 1;
        if threw {
            push(&self.sink, json!({ "ev": "send", "key": self.key, "text": text, "threw": true }));
            return Err("Error: broken link".to_string());
        }
        push(&self.sink, json!({ "ev": "send", "key": self.key, "text": text }));
        Ok(())
    }

    fn close(&mut self, why: HostWhy) {
        push(&self.sink, json!({ "ev": "close", "key": self.key, "why": ts_why(&why) }));
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

struct TapeReferee {
    answers: Vec<String>,
    sink: Sink,
}

impl MatchReferee for TapeReferee {
    fn start(&mut self, m: &RefereeStart, _now: f64) -> String {
        let humans: Vec<Value> = m.humans.iter().map(|h| json!({ "slot": h.slot, "id": h.id })).collect();
        push(&self.sink, json!({ "ev": "referee.start", "match": m.match_id, "humans": humans, "tick0": m.tick0 }));
        if self.answers.is_empty() { String::new() } else { self.answers.remove(0) }
    }

    fn left(&mut self, match_id: u32, slot: u32) {
        push(&self.sink, json!({ "ev": "referee.left", "match": match_id, "slot": slot }));
    }

    fn end(&mut self, match_id: u32) {
        push(&self.sink, json!({ "ev": "referee.end", "match": match_id }));
    }
}

/// A lobby.error's oracle text by its code.
/// A close's or an end's code as the oracle's text.
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

/// A sent message as the comparison takes it: a lobby.error's code as its text.
fn message(text: &str, tape: bool) -> Value {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else { return json!({ "unparsed": text }) };
    if let Some(o) = v.as_object_mut()
        && !tape
        && o.get("t").and_then(Value::as_str) == Some("lobby.error")
    {
        let why = o.remove("why").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
        let name = o.remove("name").and_then(|w| w.as_str().map(str::to_string)).unwrap_or_default();
        o.insert("text".to_string(), json!(lobby_error_text(&why, &name)));
    }
    norm(&v)
}

/// An output as the comparison takes it.
fn output(e: &Value, tape: bool) -> Value {
    let ev = e["ev"].as_str().unwrap_or("");
    match ev {
        "send" => {
            json!({ "ev": ev, "key": e["key"], "msg": message(e["text"].as_str().unwrap_or(""), tape), "threw": e.get("threw") == Some(&json!(true)) })
        }
        "close" => json!({ "ev": ev, "key": e["key"], "why": e["why"] }),
        "log" => json!({ "ev": ev, "text": e["text"].as_str().unwrap_or("").split('\n').next().unwrap_or("") }),
        "randomInt" => {
            // (randomInt(n) is randomInt(0, n))
            let a = e["args"].as_array().cloned().unwrap_or_default();
            let args = if a.len() == 1 { vec![json!(0), a[0].clone()] } else { a };
            json!({ "ev": ev, "args": norm(&Value::Array(args)) })
        }
        "randomBytes" => json!({ "ev": ev, "n": norm(&e["n"]) }),
        "dateNow" => json!({ "ev": ev }),
        "referee.start" => {
            json!({ "ev": ev, "match": norm(&e["match"]), "humans": norm(&e["humans"]), "tick0": norm(&e["tick0"]) })
        }
        "referee.left" => json!({ "ev": ev, "match": norm(&e["match"]), "slot": norm(&e["slot"]) }),
        "referee.end" => json!({ "ev": ev, "match": norm(&e["match"]) }),
        _ => {
            let mut v = norm(e);
            if let Some(o) = v.as_object_mut() {
                o.remove("dir");
                o.remove("t");
            }
            v
        }
    }
}

fn is_event(e: &Value) -> bool {
    matches!(e["ev"].as_str(), Some("onEnd" | "onKicked" | "onFound" | "onListing"))
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

struct HostRun {
    groups: usize,
    equal_groups: usize,
    events: usize,
    equal_events: usize,
    diffs: Vec<String>,
    /// (away_back) the silents told away and back first.
    told: usize,
}

/// One host of the tape replayed; `away_back`: each silent human told away and back first (checked in `diffs`).
#[allow(clippy::too_many_lines, reason = "the replay's one loop")]
fn run_host(h: &Value, away_back: bool) -> HostRun {
    let o = &h["options"];
    let events = h["events"].as_array().expect("a host's events");
    let sink: Sink = Arc::default();
    let draws: Vec<Value> = events
        .iter()
        .filter(|e| matches!(e["ev"].as_str(), Some("randomInt" | "randomBytes" | "dateNow")))
        .cloned()
        .collect();
    let answers: Vec<String> = events
        .iter()
        .filter(|e| e["ev"] == "referee.start")
        .map(|e| e["returns"].as_str().unwrap_or("").to_string())
        .collect();
    let log_sink = Arc::clone(&sink);
    let start = o["start"].as_f64().expect("a start");
    let opts = HostOptions {
        kind: if o["kind"] == "custom" { SessionKind::Custom } else { SessionKind::Queue },
        host: o["host"].as_str().unwrap_or("").to_string(),
        name: o["name"].as_str().map(str::to_string),
        lobby_id: o["lobbyId"].as_str().map(str::to_string),
        times: times(&o["times"]),
        referee: (o["referee"] == true)
            .then(|| Box::new(TapeReferee { answers, sink: Arc::clone(&sink) }) as Box<dyn MatchReferee>),
        rand: Box::new(TapeRand { draws, at: 0, sink: Arc::clone(&sink) }),
        log: Some(Box::new(move |line: &str| push(&log_sink, json!({ "ev": "log", "text": line })))),
        paths: false,
    };
    let flag = |k: &str| o[k] == true;
    let mut core = Core::new(opts, start);
    let mut clock = ManualClock { t: start };
    // each send index of a link from which its sends fail
    let mut throw_at: HashMap<u64, usize> = HashMap::new();
    let mut sends: HashMap<u64, usize> = HashMap::new();
    for e in events {
        if e["ev"] == "send" {
            let k = e["key"].as_u64().expect("a key");
            let n = sends.entry(k).or_default();
            if e.get("threw") == Some(&json!(true)) {
                throw_at.entry(k).or_insert(*n);
            }
            *n += 1;
        }
    }
    let mut keys: HashMap<u64, ConnKey> = HashMap::new();
    // the time of each link's last hello message (the rename exemption below)
    let mut hello_at: HashMap<u64, f64> = HashMap::new();
    let mut run =
        HostRun { groups: 0, equal_groups: 0, events: events.len(), equal_events: 0, diffs: Vec::new(), told: 0 };
    // the tape's groups: a replayed input and what came of it (its nested inputs are the core's own)
    let mut starts: Vec<usize> =
        (0..events.len()).filter(|&i| events[i]["dir"] == "in" && events[i]["nested"] != true).collect();
    starts.push(events.len());
    if starts[0] != 0 {
        run.diffs.push(format!("host {}: {} outputs before the first input", h["index"], starts[0]));
    }
    for g in 0..starts.len() - 1 {
        let (a, b) = (starts[g], starts[g + 1]);
        let input = &events[a];
        let t = input["t"].as_f64().expect("an input's time");
        sink.lock().expect("the sink").clear();
        let hello = input["ev"] == "message" && input["text"].as_str().is_some_and(|t| t.contains(r#""t":"hello""#));
        let prev_hello = if hello { hello_at.insert(input["key"].as_u64().unwrap_or(0), t) } else { None };
        // (away_back: the keys told away, and back, of the silent of this input)
        let mut told: Option<(Vec<Value>, Vec<Value>)> = None;
        if input["ev"] != "advance" && t != clock.t {
            run.diffs.push(format!("host {} input {a}: the clock at {} for an input at {t}", h["index"], clock.t));
        }
        match input["ev"].as_str().unwrap_or("") {
            "connect" => {
                let k = input["key"].as_u64().expect("a key");
                let peer = TapePeer {
                    id: input["id"].as_str().unwrap_or("").to_string(),
                    name: input["name"].as_str().unwrap_or("").to_string(),
                    key: k,
                    sink: Arc::clone(&sink),
                    sends: 0,
                    throw_at: throw_at.get(&k).copied(),
                };
                let ck = core.connect(Box::new(peer), t);
                keys.insert(k, ck);
            }
            "message" => {
                let text = input["text"].as_str().unwrap_or("");
                core.message(keys[&input["key"].as_u64().expect("a key")], text, t);
            }
            "closed" => core.closed(keys[&input["key"].as_u64().expect("a key")], t),
            "end" => core.end(HostWhy::Owner { text: input["why"].as_str().unwrap_or("").to_string() }, t),
            "silent" => {
                let (m, s) = (input["match"].as_u64().expect("a match"), input["slot"].as_u64().expect("a slot"));
                let (m, s) = (u32::try_from(m).expect("a match"), u32::try_from(s).expect("a slot"));
                if away_back {
                    let keys = |away: bool, run: &mut HostRun| -> Vec<Value> {
                        let out: Vec<Value> =
                            sink.lock().expect("the sink").drain(..).map(|e| output(&e, false)).collect();
                        let want = norm(&json!({ "t": "match.away", "slot": s, "away": away }));
                        let mut keys = Vec::new();
                        for o in out {
                            if o["ev"] == "send" && o["msg"] == want {
                                keys.push(o["key"].clone());
                            } else if o["ev"] != "log" {
                                run.diffs.push(format!("host {} input {a}: away {away}: an output {o}", h["index"]));
                            }
                        }
                        keys
                    };
                    core.away(m, s, t);
                    let away = keys(true, &mut run);
                    core.back(m, s, t);
                    let back = keys(false, &mut run);
                    run.told += 1;
                    told = Some((away, back));
                }
                core.gone(m, s, t);
            }
            "advance" => clock.advance(&mut core, input["ms"].as_f64().expect("ms")),
            x => panic!("host tape: an input {x}"),
        }
        // (a link that closed by itself inside the call, not by the core's close: the oracle took it as gone at the end
        // of the call; its owner here reports it once the call is over)
        for i in a + 1..b {
            let (e, before) = (&events[i], &events[i - 1]);
            if e["ev"] == "closed" && !(before["ev"] == "close" && before["key"] == e["key"]) {
                core.closed(keys[&e["key"].as_u64().expect("a key")], t);
            }
        }
        let mut got: Vec<Value> = sink.lock().expect("the sink").drain(..).map(|e| output(&e, false)).collect();
        for ev in core.take_events() {
            match ev {
                HostEvent::End(why) if flag("onEnd") => got.push(json!({ "ev": "onEnd", "why": ts_why(&why) })),
                HostEvent::Kicked(id) if flag("onKicked") => got.push(json!({ "ev": "onKicked", "id": id })),
                HostEvent::Found(ids) if flag("onFound") => got.push(json!({ "ev": "onFound", "humans": ids })),
                _ => {}
            }
        }
        let want: Vec<Value> = events[a + 1..b].iter().filter(|e| e["dir"] == "out").map(|e| output(e, true)).collect();
        if let Some((away, back)) = told {
            // (the others: told it left, after the silent's own match.left)
            let slot = norm(&input["slot"]);
            let mut left: Vec<Value> = want
                .iter()
                .filter(|e| e["ev"] == "send" && e["msg"]["t"] == "match.left" && e["msg"]["slot"] == slot)
                .map(|e| e["key"].clone())
                .collect();
            if !left.is_empty() {
                left.remove(0);
            }
            if away != left || back != left {
                run.diffs.push(format!(
                    "host {} input {a}: told away {away:?} and back {back:?}, the others {left:?}",
                    h["index"]
                ));
            }
        }
        let split = |v: &[Value]| -> (Vec<Value>, Vec<Value>) { v.iter().cloned().partition(|e| !is_event(e)) };
        let (got_out, got_ev) = split(&got);
        let (want_out, want_ev) = split(&want);
        run.groups += 1;
        if got_out == want_out && got_ev == want_ev {
            run.equal_groups += 1;
            run.equal_events += b - a;
            continue;
        }
        // Deviation (core.rs RENAME_MS): a hello with another name inside RENAME_MS is answered welcome with the name
        // the core keeps; the oracle core sent nothing
        let rename_answered = want_out.is_empty()
            && want_ev.is_empty()
            && got_ev.is_empty()
            && got_out.len() == 1
            && got_out[0]["msg"]["t"] == "welcome"
            && prev_hello.is_some_and(|p| t - p < 2000.0);
        if rename_answered {
            run.equal_groups += 1;
            run.equal_events += b - a;
            continue;
        }
        let n = got_out.len().max(want_out.len());
        let at = (0..n).find(|&i| got_out.get(i) != want_out.get(i));
        let mut d = format!(
            "host {} input {a} ({}): {} outputs, the tape {}; events {:?} / the tape {:?}",
            h["index"],
            serde_json::to_string(input).unwrap_or_default().chars().take(160).collect::<String>(),
            got_out.len(),
            want_out.len(),
            got_ev,
            want_ev
        );
        if let Some(i) = at {
            let show =
                |v: Option<&Value>| v.map_or("(none)".to_string(), |x| x.to_string().chars().take(400).collect());
            d += &format!(
                "\n    first differs at {i}:\n    port {}\n    tape {}",
                show(got_out.get(i)),
                show(want_out.get(i))
            );
        }
        run.diffs.push(d);
    }
    run
}

/// The tape's hosts whose lobby the port's Versus answers otherwise than the oracle's styles: the host's index, its
/// first event the port answers otherwise (an input), and why.
const VERSUS_DEPARTS: [(u64, usize, &str); 1] =
    [(3, 23, "its 2v2 lobby of 2 + 1 is refused (Teams are unbalanced): Versus starts it as a 2v1")];

/// A host of the tape as replayed: one of VERSUS_DEPARTS up to its departing input, the rest whole.
fn up_to_departure(h: &Value) -> Value {
    let mut h = h.clone();
    if let Some(&(_, at, _)) = VERSUS_DEPARTS.iter().find(|(i, ..)| h["index"].as_u64() == Some(*i))
        && let Some(e) = h["events"].as_array_mut()
    {
        e.truncate(at);
    }
    h
}

#[test]
fn the_core_replays_the_ts_tape() {
    let Some(tape) = mvs_protocol::vectors::read_tape("host-tape.json") else {
        eprintln!("host_tape: no host-tape.json, nothing checked");
        return;
    };
    let hosts = tape["hosts"].as_array().expect("hosts");
    let (mut equal_hosts, mut events, mut equal_events, mut groups, mut equal_groups) = (0, 0, 0, 0, 0);
    let mut diffs: Vec<String> = Vec::new();
    for h in hosts.iter().map(up_to_departure) {
        let r = run_host(&h, false);
        if r.diffs.is_empty() {
            equal_hosts += 1;
        }
        events += r.events;
        equal_events += r.equal_events;
        groups += r.groups;
        equal_groups += r.equal_groups;
        diffs.extend(r.diffs);
    }
    eprintln!(
        "host_tape: {equal_hosts} / {} hosts equal ({} up to where the port's Versus answers otherwise), \
         {equal_events} / {events} events, {equal_groups} / {groups} inputs",
        hosts.len(),
        VERSUS_DEPARTS.len()
    );
    for d in diffs.iter().take(40) {
        eprintln!("{d}");
    }
    assert!(diffs.is_empty(), "{} inputs differ from the tape", diffs.len());
}

/// The tape's hosts with silent humans, each silent told away and back first: the others are told each, and the rest
/// of the replay is the tape's.
#[test]
fn away_and_back_tell_the_others() {
    let Some(tape) = mvs_protocol::vectors::read_tape("host-tape.json") else {
        eprintln!("host_tape: no host-tape.json, nothing checked");
        return;
    };
    let (mut told, mut diffs) = (0, Vec::new());
    for h in tape["hosts"].as_array().expect("hosts").iter().map(up_to_departure) {
        if !h["events"].as_array().expect("events").iter().any(|e| e["ev"] == "silent") {
            continue;
        }
        let r = run_host(&h, true);
        told += r.told;
        diffs.extend(r.diffs);
    }
    eprintln!("host_tape: {told} silents told away and back first");
    for d in diffs.iter().take(20) {
        eprintln!("{d}");
    }
    assert!(diffs.is_empty(), "{} differ", diffs.len());
    assert!(told > 0, "no silent in the tape");
}
