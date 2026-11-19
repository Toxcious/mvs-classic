//! The relay's open queue (`SessionKind::OpenQueue`): nobody hosts it, so every join is a ticket of its own, many
//! matches run at once and a player who leaves ends only what they were in. Pairing by distance (queue.join `rtt`,
//! `queue_rules::rtt_band`) and the referee's room (`MatchReferee::room`) hold tickets back.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{ManualClock, MemoryPeer, PeerLog};
use mvs_host::{ConnKey, Core, HOST_TIMES, HostOptions, HostRand, MatchReferee, RefereeStart, SessionKind, SystemRand};
use mvs_protocol::version::PROTOCOL;
use serde_json::{Value, json};

/// The referee's calls as text, and its room: none while `full`, else (`max` > 0) for `max` matches running and
/// pending.
#[derive(Clone, Default)]
struct Seen {
    calls: Arc<Mutex<Vec<String>>>,
    full: Arc<AtomicBool>,
    max: Arc<AtomicUsize>,
}

struct TestReferee(Seen, usize);

impl MatchReferee for TestReferee {
    fn start(&mut self, m: &RefereeStart, _now: f64) -> String {
        let ids: Vec<&str> = m.humans.iter().map(|h| h.id.as_str()).collect();
        self.0.calls.lock().expect("calls").push(format!("start {} {}", m.match_id, ids.join(" ")));
        self.1 += 1;
        format!("queue.m{}", m.match_id)
    }

    fn left(&mut self, match_id: u32, slot: u32) {
        self.0.calls.lock().expect("calls").push(format!("left {match_id} {slot}"));
    }

    fn end(&mut self, match_id: u32) {
        self.0.calls.lock().expect("calls").push(format!("end {match_id}"));
        self.1 -= 1;
    }

    fn room(&self, pending: usize) -> bool {
        let max = self.0.max.load(Ordering::Relaxed);
        !self.0.full.load(Ordering::Relaxed) && (max == 0 || self.1 + pending < max)
    }
}

struct Player {
    id: String,
    key: ConnKey,
    log: Arc<Mutex<PeerLog>>,
}

impl Player {
    fn got(&self) -> Vec<Value> {
        self.log.lock().expect("log").got.clone()
    }

    fn of(&self, t: &str) -> Vec<Value> {
        self.got().into_iter().filter(|m| m["t"] == t).collect()
    }

    fn clear(&self) {
        self.log.lock().expect("log").got.clear();
    }
}

struct Run {
    core: Core,
    clock: ManualClock,
    seen: Seen,
}

fn session(kind: SessionKind, host: &str) -> Run {
    let seen = Seen::default();
    let rand: Box<dyn HostRand> = Box::new(SystemRand);
    let core = Core::new(
        HostOptions {
            kind,
            host: host.to_string(),
            name: None,
            lobby_id: None,
            times: HOST_TIMES,
            referee: Some(Box::new(TestReferee(seen.clone(), 0))),
            rand,
            log: Some(Box::new(|_| {})),
            paths: false,
        },
        0.0,
    );
    Run { core, clock: ManualClock { t: 0.0 }, seen }
}

impl Run {
    fn open() -> Run {
        session(SessionKind::OpenQueue, "")
    }

    fn send(&mut self, p: &Player, m: &Value) {
        self.core.message(p.key, &m.to_string(), self.clock.t);
    }

    fn player(&mut self, n: usize) -> Player {
        let id = format!("p{n}");
        let (peer, log) = MemoryPeer::new(&id, "");
        let key = self.core.connect(Box::new(peer), self.clock.t);
        let p = Player { id, key, log };
        self.send(&p, &json!({ "t": "hello", "proto": PROTOCOL, "name": format!("Player{n}") }));
        p
    }

    /// A queue.join of 1v1, `rtt` its round trip (None: none sent).
    fn join(&mut self, p: &Player, mode: &str, rtt: Option<f64>) {
        let mut m = json!({ "t": "queue.join", "modes": [mode], "character": "", "skin": "", "compat": "c1" });
        if let Some(r) = rtt {
            m["rtt"] = json!(r);
        }
        self.send(p, &m);
    }

    fn advance(&mut self, ms: f64) {
        self.clock.advance(&mut self.core, ms);
    }

    fn calls(&self) -> Vec<String> {
        self.seen.calls.lock().expect("calls").clone()
    }

    /// Each player's pm.perks, then their match.loaded once match.start came.
    fn play(&mut self, ps: &[&Player]) {
        for p in ps {
            self.send(p, &json!({ "t": "pm.perks", "perks": [] }));
        }
        for p in ps {
            assert_eq!(p.of("match.start").len(), 1, "{} has its match.start", p.id);
            self.send(p, &json!({ "t": "match.loaded" }));
        }
    }
}

/// The match id of a player's queue.found.
fn found_of(p: &Player) -> Option<String> {
    p.of("queue.found").first().map(|m| m["match"].as_str().unwrap_or("").to_string())
}

#[test]
fn many_groups_find_their_matches_at_once() {
    let mut r = Run::open();
    let ps: Vec<Player> = (0..8).map(|n| r.player(n)).collect();
    for p in &ps {
        r.join(p, "1v1", None);
    }
    // four matches, each of two of the queue's order, all found at once
    let ids: Vec<String> = ps.iter().map(|p| found_of(p).expect("found")).collect();
    for pair in ids.chunks(2) {
        assert_eq!(pair[0], pair[1]);
    }
    let mut distinct = ids.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 4);
    for pair in ps.chunks(2) {
        r.play(&[&pair[0], &pair[1]]);
    }
    let starts: Vec<String> = r.calls().into_iter().filter(|c| c.starts_with("start")).collect();
    assert_eq!(starts, ["start 1 p0 p1", "start 2 p2 p3", "start 3 p4 p5", "start 4 p6 p7"]);
    for (i, p) in ps.iter().enumerate() {
        let go = p.of("match.go");
        assert_eq!(go.len(), 1);
        assert_eq!(go[0]["referee"], format!("queue.m{}", i / 2 + 1));
    }
    // a later pair queues beside the matches that run
    let more: Vec<Player> = (8..10).map(|n| r.player(n)).collect();
    r.join(&more[0], "2v2", None);
    r.join(&more[1], "2v2", None);
    assert!(found_of(&more[0]).is_none(), "2v2 waits for four or its bots");
    r.advance(60_000.0);
    assert!(found_of(&more[0]).is_some() && found_of(&more[0]) == found_of(&more[1]), "the bot fill as before");
    assert!(!r.core.ended());
}

#[test]
fn a_quitter_in_a_pre_match_ends_that_match_alone() {
    let mut r = Run::open();
    let ps: Vec<Player> = (0..4).map(|n| r.player(n)).collect();
    for p in &ps {
        r.join(p, "1v1", None);
    }
    for p in &ps[2..] {
        p.clear();
    }
    r.core.closed(ps[0].key, r.clock.t);
    assert!(!r.core.ended(), "nobody's leave ends the open queue");
    assert!(!ps[1].got().is_empty(), "the quitter's partner is told");
    assert!(ps[2].got().is_empty() && ps[3].got().is_empty(), "the other match hears nothing");
    r.play(&[&ps[2], &ps[3]]);
    assert_eq!(r.calls(), ["start 1 p2 p3"]);
    // the partner queues again and is matched with a newcomer
    let p4 = r.player(4);
    r.join(&ps[1], "1v1", None);
    r.join(&p4, "1v1", None);
    assert!(found_of(&p4).is_some());
}

#[test]
fn a_quitter_in_a_match_ends_that_match_alone() {
    let mut r = Run::open();
    let ps: Vec<Player> = (0..4).map(|n| r.player(n)).collect();
    for p in &ps {
        r.join(p, "1v1", None);
    }
    r.play(&[&ps[0], &ps[1]]);
    r.play(&[&ps[2], &ps[3]]);
    for p in &ps {
        p.clear();
    }
    r.send(&ps[2], &json!({ "t": "match.leave" }));
    let calls = r.calls();
    assert_eq!(&calls[2..], ["left 2 0"], "{calls:?}");
    assert!(ps[0].got().is_empty() && ps[1].got().is_empty(), "the other match hears nothing");
    assert_eq!(ps[3].of("match.left").len(), 1, "the quitter's partner is told");
    // a link that closes in a match: the same
    r.core.closed(ps[0].key, r.clock.t);
    assert_eq!(r.calls().last().map(String::as_str), Some("left 1 0"));
    assert!(ps[3].of("match.left").len() == 1 && !r.core.ended());
}

/// Pairing by distance: like round trips at once, others as the first ticket's wait widens the band (30 ms, 10 ms
/// more a second, any from 20 s), a ticket with none with anyone; the bot fill's wait is the same.
#[test]
fn tickets_pair_by_distance() {
    // like round trips (within 30 ms): at once
    let mut r = Run::open();
    let (a, b) = (r.player(0), r.player(1));
    r.join(&a, "1v1", Some(20.0));
    r.join(&b, "1v1", Some(50.0));
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&b));

    // 10 and 100: a band of 90 ms is 6 s of the first one's wait; a nearer newcomer meanwhile pairs first
    let mut r = Run::open();
    let (a, b, c) = (r.player(0), r.player(1), r.player(2));
    r.join(&a, "1v1", Some(10.0));
    r.join(&b, "1v1", Some(100.0));
    assert!(found_of(&a).is_none());
    r.advance(5_750.0);
    assert!(found_of(&a).is_none(), "a band of 87.5 ms at 5.75 s");
    r.advance(250.0);
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&b), "a band of 90 ms at 6 s");
    r.join(&c, "1v1", Some(10.0));
    assert!(found_of(&c).is_none());

    let mut r = Run::open();
    let (a, b, c) = (r.player(0), r.player(1), r.player(2));
    r.join(&a, "1v1", Some(10.0));
    r.join(&b, "1v1", Some(100.0));
    r.advance(1_000.0);
    r.join(&c, "1v1", Some(45.0));
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&c), "35 ms within the 40 ms of 1 s");
    assert!(found_of(&b).is_none());

    // far apart: any with any from 20 s, before the bots' 60 s
    let mut r = Run::open();
    let (a, b) = (r.player(0), r.player(1));
    r.join(&a, "1v1", Some(10.0));
    r.join(&b, "1v1", Some(1000.0));
    r.advance(19_750.0);
    assert!(found_of(&a).is_none());
    r.advance(250.0);
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&b));
    assert!(a.of("queue.found")[0]["players"].as_array().is_some_and(|ps| ps.iter().all(|p| p["bot"] != true)));

    // no round trip: with anyone at once
    let mut r = Run::open();
    let (a, b) = (r.player(0), r.player(1));
    r.join(&a, "1v1", Some(10.0));
    r.join(&b, "1v1", None);
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&b));

    // the bot fill: a lone ticket's wait (botAfter 3 s) as before
    let mut r = Run::open();
    let a = r.player(0);
    r.send(
        &a,
        &json!({ "t": "queue.join", "modes": ["1v1"], "character": "", "skin": "", "compat": "c1", "rtt": 10, "botAfter": 3 }),
    );
    r.advance(2_750.0);
    assert!(found_of(&a).is_none());
    r.advance(250.0);
    assert!(found_of(&a).is_some(), "bots at 3 s");

    // a party's ticket goes by its slowest member
    let mut r = Run::open();
    let (a, b, c, d) = (r.player(0), r.player(1), r.player(2), r.player(3));
    let party = json!({ "members": ["p0", "p1"] });
    for (p, rtt) in [(&a, 10.0), (&b, 90.0)] {
        r.send(
            p,
            &json!({ "t": "queue.join", "modes": ["2v2"], "character": "", "skin": "", "compat": "c1", "rtt": rtt, "party": party }),
        );
    }
    r.join(&c, "2v2", Some(10.0));
    r.join(&d, "2v2", Some(10.0));
    assert!(found_of(&a).is_none() && found_of(&c).is_none(), "the party's 90 ms is far from 10 ms");
    r.advance(5_000.0);
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&d), "a band of 80 ms at 5 s");
}

#[test]
fn no_room_holds_the_tickets() {
    let mut r = Run::open();
    r.seen.full.store(true, Ordering::Relaxed);
    let (a, b) = (r.player(0), r.player(1));
    r.join(&a, "1v1", None);
    r.join(&b, "1v1", None);
    r.advance(61_000.0);
    assert!(found_of(&a).is_none(), "no match while there is no room, not even the bots'");
    assert_eq!(a.of("queue.state").last().map(|m| m["on"].clone()), Some(json!(true)), "still searching");
    r.seen.full.store(false, Ordering::Relaxed);
    r.advance(250.0);
    assert!(found_of(&a).is_some() && found_of(&a) == found_of(&b), "matched at the next beat");
}

/// The found matches not started yet count against the referee's room: a group waits while they would fill it.
#[test]
fn pending_matches_hold_the_queue() {
    let mut r = Run::open();
    r.seen.max.store(1, Ordering::Relaxed);
    let ps: Vec<Player> = (0..4).map(|n| r.player(n)).collect();
    for p in &ps {
        r.join(p, "1v1", None);
    }
    assert!(found_of(&ps[0]).is_some() && found_of(&ps[2]).is_none(), "one match pending fills a room of one");
    r.play(&[&ps[0], &ps[1]]);
    assert_eq!(r.calls(), ["start 1 p0 p1"]);
    r.advance(61_000.0);
    assert!(found_of(&ps[2]).is_none(), "one running fills it too");
    r.send(&ps[0], &json!({ "t": "match.leave" }));
    r.send(&ps[1], &json!({ "t": "match.leave" }));
    assert_eq!(r.calls().last().map(String::as_str), Some("end 1"));
    r.advance(250.0);
    assert!(found_of(&ps[2]).is_some() && found_of(&ps[2]) == found_of(&ps[3]), "matched once it is over");
}

/// A match whose go comes when the referee has no place for it is ended for its peers, as a failed start.
#[test]
fn no_room_at_the_go_ends_that_match() {
    let mut r = Run::open();
    let ps: Vec<Player> = (0..4).map(|n| r.player(n)).collect();
    for p in &ps {
        r.join(p, "1v1", None);
    }
    r.play(&[&ps[2], &ps[3]]);
    r.seen.full.store(true, Ordering::Relaxed);
    r.play(&[&ps[0], &ps[1]]);
    assert_eq!(r.calls(), ["start 1 p2 p3"], "no referee for the second");
    for p in &ps[..2] {
        assert!(p.of("match.go").is_empty());
        let left = p.of("match.left");
        assert_eq!(left.len(), 1, "{}: {:?}", p.id, p.got());
        assert_eq!(left[0]["why"], "error");
    }
    assert!(ps[2].of("match.left").is_empty() && !r.core.ended());
}

#[test]
fn no_room_refuses_a_lobby_start() {
    let mut r = session(SessionKind::Custom, "p0");
    let a = r.player(0);
    r.send(&a, &json!({ "t": "lobby.enter", "compat": "c1" }));
    r.send(&a, &json!({ "t": "lobby.setTeamStyle", "style": 0 }));
    r.send(&a, &json!({ "t": "lobby.addBot", "team": 1 }));
    r.send(&a, &json!({ "t": "lobby.setReady", "on": true }));
    r.seen.full.store(true, Ordering::Relaxed);
    a.clear();
    r.send(&a, &json!({ "t": "lobby.startMatch" }));
    assert_eq!(a.of("lobby.startFailed").len(), 1, "{:?}", a.got());
    r.seen.full.store(false, Ordering::Relaxed);
    r.send(&a, &json!({ "t": "lobby.startMatch" }));
    assert_eq!(a.of("pm.begin").len(), 1, "{:?}", a.got());
}

/// A lobby bot's difficulty runs from Very Easy (2) to the port's own Expert (6), past it clamped; the match's level
/// is difficulty - 2, Expert's 4.
#[test]
fn a_lobby_bot_takes_the_expert_difficulty() {
    let mut r = session(SessionKind::Custom, "p0");
    let a = r.player(0);
    let bot = |a: &Player| a.of("lobby.state").last().expect("a lobby state")["data"]["teams"][1][0].clone();
    r.send(&a, &json!({ "t": "lobby.enter", "compat": "c1" }));
    r.send(&a, &json!({ "t": "lobby.setTeamStyle", "style": 0 }));
    r.send(&a, &json!({ "t": "lobby.addBot", "team": 1, "character": "shaggy", "skin": "", "difficulty": 9 }));
    assert_eq!(bot(&a)["difficulty"], 6, "{:?}", a.got());
    let id = bot(&a)["accountId"].clone();
    let update = |d: i32| {
        let mut m = json!({ "t": "lobby.updateSettingsForBot", "id": id, "character": "shaggy", "skin": "" });
        m["difficulty"] = json!(d);
        m
    };
    r.send(&a, &update(5));
    assert_eq!(bot(&a)["difficulty"], 5);
    r.send(&a, &update(6));
    assert_eq!(bot(&a)["difficulty"], 6);
    r.send(&a, &update(1));
    assert_eq!(bot(&a)["difficulty"], 2);
    r.send(&a, &update(6));
    r.send(&a, &json!({ "t": "lobby.setReady", "on": true }));
    r.send(&a, &json!({ "t": "lobby.startMatch" }));
    r.send(&a, &json!({ "t": "pm.lock", "character": "", "skin": "" }));
    // (the bot locks its pick when the pick's time is up)
    r.advance(60_000.0);
    r.play(&[&a]);
    let start = &a.of("match.start")[0];
    assert_eq!(start["setup"]["players"][1]["difficulty"], 4, "{start}");
}

/// The queue lobby of a host as before: its host's leave still ends it.
#[test]
fn a_queue_lobby_still_ends_with_its_host() {
    let mut r = session(SessionKind::Queue, "p0");
    let (a, b) = (r.player(0), r.player(1));
    r.join(&a, "2v2", None);
    r.join(&b, "2v2", None);
    r.core.closed(a.key, r.clock.t);
    assert!(r.core.ended());
}

/// queue.join `waited` (time queued under another host) means nothing to the open queue: the bot wait is its own.
#[test]
fn the_open_queue_ignores_waited() {
    let mut r = Run::open();
    let a = r.player(0);
    let join =
        json!({ "t": "queue.join", "modes": ["1v1"], "character": "", "skin": "", "compat": "c1", "waited": 59000 });
    r.send(&a, &join);
    r.advance(1_000.0);
    assert!(found_of(&a).is_none());
    r.advance(59_000.0);
    assert!(found_of(&a).is_some(), "bots at its own 60 s");
}
