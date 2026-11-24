//! A custom game of six started through the host core (the port's own big matches): four humans and two bots in a
//! Versus 4v2 (sides uneven by two: the match plays the lobby's columns, not the slots' alternation), the leader's
//! start naming the maps that hold it; a start whose maps hold none, or that names none, is refused. The match
//! the core starts is the set-up its humans' sessions then play over the in-memory network (`common::check`).
//! Without a store the session part checks nothing.

mod common;

use std::sync::{Arc, Mutex};

use mvs_host::{Connection, Core, HOST_TIMES, HostOptions, HostRand, HostWhy, SessionKind, SystemRand};
use mvs_protocol::version::PROTOCOL;
use serde_json::{Value, json};

use common::{Net, check};

/// A player on an in-memory link to the core: the messages it was sent.
struct Peer {
    id: String,
    got: Arc<Mutex<Vec<Value>>>,
}

impl Connection for Peer {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.id
    }
    fn send(&mut self, text: &str) -> Result<(), String> {
        self.got.lock().expect("a log").push(serde_json::from_str(text).expect("the core sends JSON"));
        Ok(())
    }
    fn close(&mut self, _why: HostWhy) {}
}

/// The messages a peer was sent.
type Log = Arc<Mutex<Vec<Value>>>;

const HUMANS: [&str; 4] = ["p0", "p1", "p2", "p3"];
const PICKS: [&str; 4] = ["velma", "superman", "irongiant", "shaggy"];

/// The core and its humans' keys and logs, each signed in, in the lobby (p0 and p2 on the first team, p1 and p3 on
/// the second), the leader's Versus with two bots on the first team and the maps `map_a` and `map_b` enabled.
fn lobby() -> (Core, Vec<mvs_host::ConnKey>, Vec<Log>) {
    let rand: Box<dyn HostRand> = Box::new(SystemRand);
    let mut core = Core::new(
        HostOptions {
            kind: SessionKind::Custom,
            host: "p0".to_string(),
            name: None,
            lobby_id: Some("L1".to_string()),
            times: HOST_TIMES,
            referee: None,
            rand,
            log: Some(Box::new(|_| {})),
            paths: false,
        },
        0.0,
    );
    let (mut keys, mut logs) = (Vec::new(), Vec::new());
    for (i, id) in HUMANS.iter().enumerate() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let k = core.connect(Box::new(Peer { id: (*id).to_string(), got: got.clone() }), 0.0);
        core.message(k, &json!({ "t": "hello", "proto": PROTOCOL, "name": id }).to_string(), 0.0);
        let enter = if i == 0 { json!({ "t": "lobby.enter" }) } else { json!({ "t": "lobby.join", "lobby": "L1" }) };
        let mut enter = enter;
        enter["compat"] = json!("c1");
        core.message(k, &enter.to_string(), 0.0);
        keys.push(k);
        logs.push(got);
    }
    let say = |core: &mut Core, k, m: Value| core.message(k, &m.to_string(), 0.0);
    say(&mut core, keys[0], json!({ "t": "lobby.setMaps", "maps": ["map_a", "map_b"] }));
    for team in [0, 0] {
        say(
            &mut core,
            keys[0],
            json!({ "t": "lobby.addBot", "team": team, "character": "shaggy", "skin": "", "difficulty": 4 }),
        );
    }
    for &k in &keys[1..] {
        say(&mut core, k, json!({ "t": "lobby.setReady", "on": true }));
    }
    (core, keys, logs)
}

/// The last lobby state the leader was sent.
fn state(log: &Log) -> Value {
    log.lock().expect("a log").iter().rev().find(|m| m["t"] == "lobby.state").cloned().expect("a lobby state")
}

fn has(log: &Log, t: &str) -> bool {
    log.lock().expect("a log").iter().any(|m| m["t"] == t)
}

#[test]
fn a_custom_game_of_six_starts_from_its_lobby_and_plays() {
    let (mut core, keys, logs) = lobby();
    let teams: Vec<usize> = state(&logs[0])["data"]["teams"]
        .as_array()
        .expect("teams")
        .iter()
        .map(|t| t.as_array().unwrap().len())
        .collect();
    assert_eq!(teams, [4, 2], "a 4v2: {}", state(&logs[0]));
    // (a start of a big match that names no maps, or whose maps hold none, is refused)
    core.message(keys[0], &json!({ "t": "lobby.startMatch" }).to_string(), 0.0);
    assert!(has(&logs[0], "lobby.startFailed"), "no maps named: the start is refused");
    logs[0].lock().expect("a log").clear();
    core.message(keys[0], &json!({ "t": "lobby.startMatch", "maps": [] }).to_string(), 0.0);
    assert!(has(&logs[0], "lobby.startFailed"), "no map left: the start is refused");
    assert!(!has(&logs[0], "pm.begin"));
    core.message(keys[0], &json!({ "t": "lobby.startMatch", "maps": ["map_b", "map_c"] }).to_string(), 0.0);
    assert!(has(&logs[0], "pm.begin"), "the start goes on the map that holds it");
    for (i, &k) in keys.iter().enumerate() {
        core.message(k, &json!({ "t": "pm.lock", "character": PICKS[i], "skin": "" }).to_string(), 0.0);
    }
    let later = 600_000.0;
    core.pump(later);
    for &k in &keys {
        core.message(k, &json!({ "t": "pm.perks", "perks": [] }).to_string(), later);
    }
    core.pump(later + 60_000.0);
    let start = logs[0].lock().expect("a log").iter().find(|m| m["t"] == "match.start").cloned().unwrap_or_else(|| {
        panic!("no match.start: {:?}", logs[0].lock().unwrap().iter().map(|m| m["t"].clone()).collect::<Vec<_>>())
    });
    assert_eq!(start["map"], "map_b", "drawn from the maps that hold it: {start}");
    let setup = &start["setup"];
    let players = setup["players"].as_array().expect("players");
    assert_eq!((setup["teamStyle"].as_u64(), players.len()), (Some(1), 6), "{setup}");
    assert_eq!(players.iter().filter(|p| p["bot"] == true).count(), 2);
    // the lobby's columns: the first team's four (p0, p2 and the bots) on team 0, p1 and p3 on team 1
    let names: Vec<&str> =
        start["usernames"].as_array().expect("usernames").iter().map(|n| n.as_str().unwrap_or("")).collect();
    let wire = common::setup(setup);
    let n = wire.players.len();
    let team_of = |slot: usize| wire.players[slot].team.unwrap_or(u8::try_from(slot % 2).expect("a team"));
    for (slot, name) in names.iter().enumerate() {
        let want = u8::from(*name == "p1" || *name == "p3");
        assert_eq!(team_of(slot), want, "slot {slot} {name}: {names:?}");
    }
    assert_eq!((0..n).filter(|&s| team_of(s) == 0).count(), 4);
    // every human's game is told the same set-up
    for log in &logs[1..] {
        let s = log.lock().expect("a log").iter().find(|m| m["t"] == "match.start").cloned().expect("a match.start");
        assert_eq!(&s["setup"], setup);
    }
    // the set-up the sessions play: the core's, over the in-memory network (characters the session data loads)
    let mut setup = common::setup(setup);
    for p in &mut setup.players {
        let c = p.character.get_or_insert_with(|| "shaggy".to_owned());
        assert!(PICKS.contains(&c.as_str()), "a character of the session's data: {c}");
    }
    let n = Net { lat: vec![1.0, 2.0, 1.5, 0.5], jitter: 1.0, ..Net::default() };
    check("session, a 4v2 custom game of four humans and two bots", &setup, 900, &n);
}
