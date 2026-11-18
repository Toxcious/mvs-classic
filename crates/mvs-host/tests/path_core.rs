//! A match's path on a core that measures (`HostOptions::paths`): path.peers only when the match is measured, the
//! choice told as pm.path, a chosen human's app told ref.go and named as the referee at match.go, its news and a
//! lost referee, and the choice made at match.start when reports are late. A core with paths off does none of it.

mod common;

use std::sync::{Arc, Mutex};

use common::{ManualClock, MemoryPeer, PeerLog};
use mvs_host::{ConnKey, Core, HOST_TIMES, HostOptions, HostRand, MatchReferee, RefereeStart, SessionKind, SystemRand};
use mvs_protocol::path::PATH_WAIT_MS;
use mvs_protocol::version::PROTOCOL;
use serde_json::{Value, json};

/// The core's referee's calls as text.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

struct TestReferee(Seen);

impl MatchReferee for TestReferee {
    fn start(&mut self, m: &RefereeStart, _now: f64) -> String {
        self.0.0.lock().expect("calls").push(format!("start {}", m.match_id));
        format!("relay.m{}", m.match_id)
    }

    fn left(&mut self, match_id: u32, slot: u32) {
        self.0.0.lock().expect("calls").push(format!("left {match_id} {slot}"));
    }

    fn end(&mut self, match_id: u32) {
        self.0.0.lock().expect("calls").push(format!("end {match_id}"));
    }
}

struct Player {
    key: ConnKey,
    log: Arc<Mutex<PeerLog>>,
}

impl Player {
    fn of(&self, t: &str) -> Vec<Value> {
        self.log.lock().expect("log").got.iter().filter(|m| m["t"] == t).cloned().collect()
    }

    fn types(&self) -> Vec<String> {
        self.log.lock().expect("log").got.iter().map(|m| m["t"].as_str().unwrap_or("").to_string()).collect()
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

impl Run {
    fn new(kind: SessionKind, host: &str, paths: bool) -> Run {
        let seen = Seen::default();
        let rand: Box<dyn HostRand> = Box::new(SystemRand);
        let core = Core::new(
            HostOptions {
                kind,
                host: host.to_string(),
                name: None,
                lobby_id: Some("L1".to_string()),
                times: HOST_TIMES,
                referee: Some(Box::new(TestReferee(seen.clone()))),
                rand,
                log: Some(Box::new(|_| {})),
                paths,
            },
            0.0,
        );
        Run { core, clock: ManualClock { t: 0.0 }, seen }
    }

    fn send(&mut self, p: &Player, m: &Value) {
        self.core.message(p.key, &m.to_string(), self.clock.t);
    }

    /// A player whose hello names its Epic id `e<n>` (None: not on Epic), its round trip to the relay and Direct.
    fn player(&mut self, n: usize, epic: bool, rtt: f64, direct: bool) -> Player {
        let (peer, log) = MemoryPeer::new(&format!("p{n}"), "");
        let key = self.core.connect(Box::new(peer), self.clock.t);
        let p = Player { key, log };
        let mut hello = json!({ "t": "hello", "proto": PROTOCOL, "name": format!("Player{n}"), "rtt": rtt,
            "direct": direct });
        if epic {
            hello["epic"] = json!(format!("e{n}"));
        }
        self.send(&p, &hello);
        p
    }

    fn join(&mut self, p: &Player) {
        self.send(p, &json!({ "t": "queue.join", "modes": ["1v1"], "character": "", "skin": "", "compat": "c1" }));
    }

    /// A path.report: the relay's round trip and one link to `slot`.
    fn report(&mut self, p: &Player, relay: f64, slot: u32, rtt: Option<f64>) {
        let mut link = json!({ "slot": slot });
        if let Some(r) = rtt {
            link["rtt"] = json!(r);
        }
        self.send(p, &json!({ "t": "path.report", "relay": relay, "peers": [link] }));
    }

    /// Each player's pm.perks, then their match.loaded.
    fn play(&mut self, ps: &[&Player]) {
        for p in ps {
            self.send(p, &json!({ "t": "pm.perks", "perks": [] }));
        }
        for p in ps {
            assert_eq!(p.of("match.start").len(), 1);
            self.send(p, &json!({ "t": "match.loaded" }));
        }
    }

    fn calls(&self) -> Vec<String> {
        self.seen.0.lock().expect("calls").clone()
    }
}

/// Two far players of the open queue (200 ms from the relay), found and measured.
fn far_pair(r: &mut Run) -> (Player, Player) {
    let (a, b) = (r.player(0, true, 200.0, false), r.player(1, true, 200.0, false));
    r.join(&a);
    r.join(&b);
    (a, b)
}

#[test]
fn far_players_are_measured_and_a_short_epic_way_referees() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    let pa = a.of("path.peers");
    assert_eq!(pa.len(), 1);
    assert_eq!(pa[0]["peers"], json!([{ "slot": 1, "epic": "e1" }]));
    assert_eq!(pa[0]["direct"], json!(false));
    assert_eq!(b.of("path.peers")[0]["peers"], json!([{ "slot": 0, "epic": "e0" }]));
    assert!(a.of("pm.path").is_empty(), "no choice before the reports");
    r.report(&a, 200.0, 1, Some(50.0));
    assert!(a.of("pm.path").is_empty());
    r.report(&b, 200.0, 0, Some(60.4));
    for p in [&a, &b] {
        let path = p.of("pm.path");
        assert_eq!(path.len(), 1);
        assert_eq!(path[0]["referee"], json!(0));
        assert_eq!(path[0]["ms"], json!([0, 60]));
        assert_eq!(path[0]["far"], json!([false, false]));
    }
    r.play(&[&a, &b]);
    assert!(r.calls().is_empty(), "the core's referee is not asked: {:?}", r.calls());
    for p in [&a, &b] {
        assert_eq!(p.of("match.go")[0]["referee"], json!("e0"));
    }
    let go = a.of("ref.go");
    assert_eq!(go.len(), 1);
    assert_eq!(go[0]["in"], json!(250));
    assert_eq!(go[0]["humans"], json!([{ "slot": 0, "epic": "e0" }, { "slot": 1, "epic": "e1" }]));
    assert!(b.of("ref.go").is_empty(), "ref.go to the referee's link alone");
    // the referee's game comes before the go
    let t = a.types();
    let at = |x: &str| t.iter().position(|y| y == x).expect("sent");
    assert!(at("ref.go") < at("match.go"));
}

#[test]
fn near_players_are_not_measured_and_told_the_relay() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = (r.player(0, true, 40.0, false), r.player(1, true, 60.0, false));
    r.join(&a);
    r.join(&b);
    assert!(a.of("path.peers").is_empty() && b.of("path.peers").is_empty());
    let path = a.of("pm.path");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0]["referee"], json!(-1));
    assert_eq!(path[0]["ms"], json!([40, 60]));
    r.play(&[&a, &b]);
    assert_eq!(r.calls(), ["start 1"]);
    assert_eq!(a.of("match.go")[0]["referee"], json!("relay.m1"));
    assert!(a.of("ref.go").is_empty() && b.of("ref.go").is_empty());
}

#[test]
fn a_human_without_epic_is_not_measured() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = (r.player(0, true, 200.0, false), r.player(1, false, 200.0, false));
    r.join(&a);
    r.join(&b);
    assert!(a.of("path.peers").is_empty());
    assert_eq!(a.of("pm.path")[0]["referee"], json!(-1));
}

#[test]
fn two_humans_with_one_epic_id_are_not_measured() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = (r.player(0, true, 200.0, false), r.player(1, false, 200.0, false));
    let hello = json!({ "t": "hello", "proto": PROTOCOL, "name": "Player1", "rtt": 200.0, "direct": false,
        "epic": "e0" });
    r.send(&b, &hello);
    r.join(&a);
    r.join(&b);
    assert!(a.of("path.peers").is_empty() && b.of("path.peers").is_empty());
    assert_eq!(a.of("pm.path")[0]["referee"], json!(-1));
}

#[test]
fn a_core_with_paths_off_says_nothing_of_paths() {
    let mut r = Run::new(SessionKind::OpenQueue, "", false);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.report(&b, 200.0, 0, Some(50.0));
    r.play(&[&a, &b]);
    for p in [&a, &b] {
        assert!(p.of("path.peers").is_empty() && p.of("pm.path").is_empty() && p.of("ref.go").is_empty());
        assert_eq!(p.of("match.go")[0]["referee"], json!("relay.m1"));
    }
    assert_eq!(r.calls(), ["start 1"]);
}

#[test]
fn a_failed_link_keeps_the_relay() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.report(&b, 200.0, 0, None);
    assert_eq!(a.of("pm.path")[0]["referee"], json!(-1));
    assert_eq!(a.of("pm.path")[0]["ms"], json!([200, 200]));
    r.play(&[&a, &b]);
    assert_eq!(r.calls(), ["start 1"]);
}

#[test]
fn a_choice_is_made_at_match_start_when_reports_are_late() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "pm.perks", "perks": [] }));
    }
    // (b's report missing: its link counts as failed, the relay referees)
    let t = a.types();
    let path = t.iter().position(|x| x == "pm.path").expect("pm.path at the start");
    let start = t.iter().position(|x| x == "match.start").expect("match.start");
    assert!(path < start);
    assert_eq!(a.of("pm.path")[0]["referee"], json!(-1));
    // the wait's end later changes nothing
    r.clock.advance(&mut r.core, PATH_WAIT_MS);
    assert_eq!(a.of("pm.path").len(), 1);
}

#[test]
fn the_wait_chooses_with_the_reports_that_came() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.clock.advance(&mut r.core, PATH_WAIT_MS - 1.0);
    assert!(a.of("pm.path").is_empty());
    r.clock.advance(&mut r.core, 1.0);
    assert_eq!(a.of("pm.path")[0]["referee"], json!(-1));
    assert_eq!(b.of("pm.path").len(), 1);
}

/// A far pair whose match p0's app referees, going.
fn refereed() -> (Run, Player, Player) {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.report(&b, 200.0, 0, Some(50.0));
    r.play(&[&a, &b]);
    a.clear();
    b.clear();
    (r, a, b)
}

#[test]
fn news_from_the_referee_alone_is_taken() {
    let (mut r, a, b) = refereed();
    // (the match field is not read: the sender's match is the one)
    r.send(&b, &json!({ "t": "ref.news", "match": 0, "slot": 0, "news": "away" }));
    assert!(a.of("match.away").is_empty() && b.of("match.away").is_empty(), "from another link: dropped");
    r.send(&a, &json!({ "t": "ref.news", "match": 0, "slot": 1, "news": "away" }));
    assert!(b.of("match.away").is_empty(), "the away human is not told itself");
    assert_eq!(a.of("match.away"), [json!({ "t": "match.away", "slot": 1, "away": true })]);
    r.send(&a, &json!({ "t": "ref.news", "match": 0, "slot": 1, "news": "back" }));
    assert_eq!(a.of("match.away").len(), 2);
    // (a 1v1 has no third human to tell: the gone slot is out of the match)
    r.send(&a, &json!({ "t": "ref.news", "match": 0, "slot": 1, "news": "gone" }));
    assert_eq!(b.of("match.left")[0]["slot"], json!(1));
    assert_eq!(a.of("ref.left"), [json!({ "t": "ref.left", "match": 1, "slot": 1 })]);
    assert!(r.calls().is_empty());
}

#[test]
fn a_leaver_and_the_end_reach_the_referee_alone() {
    let (mut r, a, b) = refereed();
    r.send(&b, &json!({ "t": "match.leave" }));
    assert_eq!(a.of("ref.left"), [json!({ "t": "ref.left", "match": 1, "slot": 1 })]);
    assert!(b.of("ref.left").is_empty() && b.of("ref.end").is_empty());
    r.send(&a, &json!({ "t": "match.leave" }));
    assert_eq!(a.of("ref.end"), [json!({ "t": "ref.end", "match": 1 })]);
    assert!(b.of("ref.end").is_empty());
    assert!(r.calls().is_empty());
}

#[test]
fn a_lost_referee_ends_the_match() {
    let (mut r, a, b) = refereed();
    // (from the referee's own link: dropped)
    r.send(&a, &json!({ "t": "ref.lost", "match": 0 }));
    assert!(a.of("match.left").is_empty() && b.of("match.left").is_empty());
    r.send(&b, &json!({ "t": "ref.lost", "match": 0 }));
    assert_eq!(a.of("match.left")[0]["slot"], json!(0), "the referee is told it is out");
    assert_eq!(a.of("ref.end").len(), 1);
    let left: Vec<Value> = b.of("match.left").iter().map(|m| m["slot"].clone()).collect();
    assert_eq!(left, [json!(0), json!(-1)], "the other is told the referee left, then the match's end");
    // (the match is over: a second word changes nothing)
    r.send(&b, &json!({ "t": "ref.lost", "match": 0 }));
    assert_eq!(b.of("match.left").len(), 2);
}

#[test]
fn the_referee_leaving_ends_the_match() {
    let (mut r, a, b) = refereed();
    r.core.closed(a.key, r.clock.t);
    let left: Vec<Value> = b.of("match.left").iter().map(|m| m["slot"].clone()).collect();
    assert_eq!(left, [json!(0), json!(-1)]);
}

#[test]
fn a_custom_lobby_with_direct_on_says_so() {
    let mut r = Run::new(SessionKind::Custom, "p0", true);
    let (a, b) = (r.player(0, true, 150.0, true), r.player(1, true, 150.0, true));
    r.send(&a, &json!({ "t": "lobby.enter", "compat": "c1" }));
    r.send(&a, &json!({ "t": "lobby.setTeamStyle", "style": 0 }));
    r.send(&b, &json!({ "t": "lobby.join", "lobby": "L1", "compat": "c1" }));
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "lobby.setReady", "on": true }));
    }
    r.send(&a, &json!({ "t": "lobby.startMatch" }));
    assert_eq!(a.of("pm.begin").len(), 1, "{:?}", a.types());
    let peers = a.of("path.peers");
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["direct"], json!(true));
    // the lobby's own pick and perk phases do not cut the wait
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "pm.lock", "character": "", "skin": "" }));
    }
    r.report(&a, 150.0, 1, Some(30.0));
    r.report(&b, 150.0, 0, Some(30.0));
    assert_eq!(b.of("pm.path")[0]["referee"], json!(0));
    r.play(&[&a, &b]);
    assert_eq!(b.of("match.go")[0]["referee"], json!("e0"));
    assert_eq!(a.of("ref.go").len(), 1);
}

/// A match.result of the winning team (no slot's numbers).
fn result(team: i32) -> Value {
    json!({ "t": "match.result", "team": team })
}

#[test]
fn the_referee_leaving_after_its_result_is_any_leaver() {
    let (mut r, a, b) = refereed();
    // (the app's normal end: its result, then its leave)
    r.send(&a, &result(0));
    r.send(&a, &json!({ "t": "match.leave" }));
    let left: Vec<Value> = b.of("match.left").iter().map(|m| m["slot"].clone()).collect();
    assert_eq!(left, [json!(0)], "told the referee left, the match not ended");
    r.send(&b, &result(0));
    r.send(&b, &json!({ "t": "match.leave" }));
    assert!(a.of("match.set").is_empty() && b.of("match.set").is_empty(), "the set's vote is open");
    for p in [&a, &b] {
        let rec = p.of("match.record");
        assert_eq!(rec.len(), 1, "{:?}", p.types());
        assert!(rec[0].get("left").is_none(), "nobody a leaver");
    }
    assert_eq!(a.of("ref.end").len(), 1, "the match's end reaches the referee");
    // the vote, and the rematch is measured again
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "match.vote", "yes": true }));
    }
    let set = b.of("match.set");
    assert_eq!(set.len(), 1);
    assert_eq!(set[0]["rematch"], json!(true));
    assert_eq!(set[0]["wins"][0], json!(1));
    r.clock.advance(&mut r.core, HOST_TIMES.rematch_ms);
    assert_eq!(a.of("pm.begin").len(), 1);
    assert_eq!(a.of("path.peers").len(), 1, "the rematch measures again");
    r.report(&a, 200.0, 1, Some(50.0));
    r.report(&b, 200.0, 0, Some(50.0));
    assert_eq!(b.of("pm.path")[0]["referee"], json!(0));
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "pm.lock", "character": "", "skin": "" }));
    }
    r.play(&[&a, &b]);
    assert_eq!(a.of("ref.go").len(), 1);
    assert_eq!(b.of("match.go")[0]["referee"], json!("e0"));
    assert!(r.calls().is_empty());
}

#[test]
fn a_lost_referee_is_the_leaver_in_the_record() {
    let (mut r, a, b) = refereed();
    r.send(&b, &json!({ "t": "ref.lost", "match": 0 }));
    let ra = a.of("match.record");
    let rb = b.of("match.record");
    assert_eq!(ra.len(), 1, "a record is written with no result");
    assert_eq!(ra[0]["left"], json!(true), "the referee charged as the leaver");
    assert!(rb[0].get("left").is_none(), "the sender not charged");
    assert_eq!(rb[0]["totals"], json!(true));
}

#[test]
fn a_leave_during_the_report_wait_ends_the_wait() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.send(&b, &json!({ "t": "pm.leave" }));
    r.clock.advance(&mut r.core, PATH_WAIT_MS);
    assert!(a.of("pm.path").is_empty(), "no choice for a pre-match that is over");
}

#[test]
fn a_chosen_human_gone_before_the_go_leaves_the_relay() {
    let mut r = Run::new(SessionKind::OpenQueue, "", true);
    let (a, b) = far_pair(&mut r);
    r.report(&a, 200.0, 1, Some(50.0));
    r.report(&b, 200.0, 0, Some(50.0));
    assert_eq!(b.of("pm.path")[0]["referee"], json!(0));
    for p in [&a, &b] {
        r.send(p, &json!({ "t": "pm.perks", "perks": [] }));
    }
    r.send(&a, &json!({ "t": "match.leave" }));
    r.send(&b, &json!({ "t": "match.loaded" }));
    assert_eq!(r.calls(), ["start 1"]);
    assert_eq!(b.of("match.go")[0]["referee"], json!("relay.m1"));
    assert!(a.of("ref.go").is_empty());
}
