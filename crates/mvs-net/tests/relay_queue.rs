//! The queue and the party over a relay on 127.0.0.1: an
//! mvs-relay in this process, each player a net service signed in to it by its own relay client, driven by the wall
//! clock. Since wire 5 the relay's one queue core holds every queuer: two queuers pair off into one 1v1, four into one
//! 2v2, a queuer alone is filled with a bot; a party of two queues as one ticket (filled with bots, or with a solo
//! queuer and a bot), is out of the queue on both sides when it ends, and out late when its member never joins; a
//! party is made, joined, talked in and left. With `MVS_RELAY_AT` set (`host:port`) the same runs against that relay:
//! a check of a deployed one.

#[path = "common/tape.rs"]
mod tape;

use std::time::{Duration, Instant};

use mvs_net::backend::BackendKind;
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{ClientOptions, RelayClient};
use mvs_net::service::page::PageId;
use mvs_net::service::{NetService, ServiceConfig, ServiceOptions};
use mvs_protocol::version::PROTOCOL;
use mvs_relay::server::{Options, start_relay};
use serde_json::{Value, json};

/// A relay on 127.0.0.1 in a thread of its own, for the rest of the test process: its port.
fn relay() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let o = Options { port: 0, host: "127.0.0.1".into(), ..Options::default() };
            let r = start_relay(o).expect("the relay listens");
            tx.send(r.port).expect("the port");
            std::future::pending::<()>().await;
            drop(r);
        });
    });
    rx.recv_timeout(Duration::from_secs(5)).expect("the relay started")
}

/// A player: its net service on the relay, one page, the page's events as the oracle's JSON.
struct Player {
    svc: NetService,
    page: PageId,
    id: String,
    events: Vec<Value>,
}

struct World {
    epoch: Instant,
    all: Vec<Player>,
}

impl World {
    /// `MVS_RELAY_AT` (`host:port`) runs the test against that relay, a deployed one, instead of its own.
    fn new(n: usize) -> World {
        let addr = std::env::var("MVS_RELAY_AT").unwrap_or_else(|_| format!("127.0.0.1:{}", relay()));
        let epoch = Instant::now();
        let mut all = Vec::new();
        for _ in 0..n {
            let now = epoch.elapsed().as_secs_f64() * 1000.0;
            let c = RelayClient::connect(&addr, &RelayKey::fresh(), &ClientOptions::new("mvs-test 0", epoch))
                .expect("signed in to the relay");
            let mut svc = NetService::new(ServiceOptions::default(), now);
            // (the config names the relay: a relay lobby's code holds its address)
            let config = ServiceConfig {
                relay: Some(addr.clone()),
                known_relays: vec![mvs_protocol::relay_wire::KnownRelay { address: addr.clone(), id: None }],
                ..ServiceConfig::default()
            };
            svc.configure(config, now);
            svc.start_backend(BackendKind::Relay, Box::new(c), now);
            let page = svc.open_page(now);
            all.push(Player { svc, page, id: String::new(), events: Vec::new() });
        }
        let mut w = World { epoch, all };
        w.until(5000, |w| w.all.iter().all(|p| p.status_me().is_some()));
        for p in &mut w.all {
            p.id = p.status_me().unwrap_or_default();
        }
        w
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    /// Runs every service until `f` holds, `ms` at the most; whether it held.
    fn until(&mut self, ms: u64, mut f: impl FnMut(&World) -> bool) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            self.step();
            if f(self) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn run(&mut self, ms: u64) {
        self.until(ms, |_| false);
    }

    fn step(&mut self) {
        let now = self.now();
        for p in &mut self.all {
            p.svc.pump(now);
            let page = p.page;
            p.events.extend(p.svc.events(page).iter().map(tape::ts_event));
        }
    }

    fn post(&mut self, i: usize, req: Value) {
        let now = self.now();
        let p = &mut self.all[i];
        let r = tape::request_of(&req).unwrap_or_else(|| panic!("a request: {req}"));
        p.svc.take(p.page, r, now);
    }

    fn say(&mut self, i: usize, m: Value) {
        self.post(i, json!({ "t": "msg", "text": m.to_string() }));
    }
}

impl Player {
    fn status_me(&self) -> Option<String> {
        self.events
            .iter()
            .rev()
            .find(|e| e["t"] == "status" && e["online"] == true)
            .and_then(|e| e["me"].as_str().map(str::to_string))
    }

    /// The page's messages of kind `t`.
    fn msgs(&self, t: &str) -> Vec<Value> {
        self.events
            .iter()
            .filter(|e| e["t"] == "msg")
            .filter_map(|e| serde_json::from_str::<Value>(e["text"].as_str()?).ok())
            .filter(|m| m["t"] == t)
            .collect()
    }

    fn last(&self, t: &str) -> Option<&Value> {
        self.events.iter().rev().find(|e| e["t"] == t)
    }

    /// The party the page was last told of (None: none).
    fn party(&self) -> Option<&Value> {
        self.last("party").map(|e| &e["party"]).filter(|p| !p.is_null())
    }

    fn party_size(&self) -> usize {
        self.party().and_then(|p| p["members"].as_array()).map_or(0, Vec::len)
    }
}

fn presence(name: &str) -> Value {
    json!({ "t": "presence", "presence": { "name": name, "icon": "PI_Test", "busy": false, "listed": true, "invites": true } })
}

/// The oracle's queue test's queue(): open, hello, then the join.
fn queue(w: &mut World, i: usize, name: &str, modes: &[&str], draw: u32) {
    queue_with(w, i, name, modes, draw, None, 60);
}

/// queue() with a party's ids and a bot wait.
fn queue_with(
    w: &mut World,
    i: usize,
    name: &str,
    modes: &[&str],
    draw: u32,
    party: Option<&[String]>,
    bot_after: u32,
) {
    w.post(i, json!({ "t": "open", "target": { "queue": true } }));
    assert!(w.until(5000, |w| !w.all[i].msgs("server").is_empty()), "{name}: no server message");
    w.say(i, json!({ "t": "hello", "proto": PROTOCOL, "app": "test", "name": name }));
    assert!(w.until(5000, |w| !w.all[i].msgs("welcome").is_empty()), "{name}: no welcome");
    let mut join = json!({ "t": "queue.join", "modes": modes, "character": "shaggy", "skin": "", "compat": "ctest.1",
        "draw": draw, "botAfter": bot_after });
    if let Some(ids) = party {
        join["party"] = json!({ "members": ids });
    }
    w.say(i, join);
}

/// The humans' ids of a queue.found, sorted, and its bots.
fn humans(m: &Value) -> (Vec<String>, usize) {
    let all: Vec<&Value> = m["players"].as_array().into_iter().flatten().collect();
    let mut ids: Vec<String> =
        all.iter().filter(|p| p["bot"] == false).filter_map(|p| p["id"].as_str().map(str::to_string)).collect();
    ids.sort();
    (ids, all.iter().filter(|p| p["bot"] == true).count())
}

/// Every player's first queue.found, once each has one (`ms` at the most).
fn all_found(w: &mut World, ms: u64) -> Option<Vec<Value>> {
    let found = w.until(ms, |w| w.all.iter().all(|p| !p.msgs("queue.found").is_empty()));
    found.then(|| w.all.iter().filter_map(|p| p.msgs("queue.found").first().cloned()).collect())
}

/// A queue.state off with `why` came to the player's page.
fn off(p: &Player, why: &str) -> bool {
    p.msgs("queue.state").iter().any(|m| m["on"] == false && m["why"] == why)
}

/// The oracle's party test's form(): the leader's invite, the member's join, both told the party of two.
fn form(w: &mut World, a: usize, b: usize) -> bool {
    let to = w.all[b].id.clone();
    w.post(a, json!({ "t": "party.invite", "to": to, "name": "Leader" }));
    if !w.until(5000, |w| w.all[b].last("invited").is_some()) {
        return false;
    }
    let code = w.all[b].last("invited").map(|e| e["code"].clone()).unwrap_or_default();
    w.post(b, json!({ "t": "party.join", "code": code }));
    w.until(8000, |w| w.all[a].party_size() == 2 && w.all[b].party_size() == 2)
}

#[test]
fn party_queues_together_over_the_relay() {
    let mut w = World::new(2);
    w.post(0, presence("Velma"));
    w.post(1, presence("Fred"));
    w.run(300);
    assert!(form(&mut w, 0, 1), "a party of two");
    let ids = vec![w.all[0].id.clone(), w.all[1].id.clone()];
    queue_with(&mut w, 0, "Velma", &["2v2"], 7919, Some(&ids), 5);
    queue_with(&mut w, 1, "Fred", &["2v2"], 2 * 7919, Some(&ids), 5);
    let found = w.until(20000, |w| w.all.iter().all(|p| !p.msgs("queue.found").is_empty()));
    let f: Vec<Value> = w.all.iter().filter_map(|p| p.msgs("queue.found").first().cloned()).collect();
    assert!(found, "both found a match: {f:?}");
    assert_eq!(f[0]["match"], f[1]["match"], "one match of both");
    let humans = f[0]["players"].as_array().into_iter().flatten().filter(|p| p["bot"] == false).count();
    assert_eq!(humans, 2, "its humans the party: {}", f[0]);
    let b = &w.all[1];
    let on = b.msgs("queue.state").iter().filter(|m| m["on"] == true).count();
    assert!(
        b.msgs("server").len() == 1 && b.msgs("welcome").len() == 1 && on == 1,
        "the member saw one server, one welcome, one queue.state on"
    );
    assert!(w.all.iter().all(|p| p.party_size() == 2), "the party stays");
}

#[test]
fn queue_pairs_two_queuers_over_the_relay() {
    let mut w = World::new(2);
    queue(&mut w, 0, "Velma", &["1v1"], 7919);
    // (10 ms apart: lobbies of one `at` go by their ids, which a relay draws at random)
    w.run(10);
    queue(&mut w, 1, "Fred", &["1v1"], 2 * 7919);
    let found = w.until(15000, |w| w.all.iter().all(|p| !p.msgs("queue.found").is_empty()));
    let f: Vec<Value> = w.all.iter().filter_map(|p| p.msgs("queue.found").first().cloned()).collect();
    assert!(found, "both found a match: {f:?}");
    let ids = |m: &Value| -> Vec<String> {
        let mut v: Vec<String> = m["players"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["id"].as_str().map(str::to_string))
            .collect();
        v.sort();
        v
    };
    let mut both = vec![w.all[0].id.clone(), w.all[1].id.clone()];
    both.sort();
    assert_eq!(ids(&f[0]), both, "the match is of the two queuers");
    assert_eq!(f[0]["match"], f[1]["match"], "one match");
    assert!(
        w.all.iter().all(|p| p.msgs("server").len() == 1 && p.msgs("welcome").len() == 1),
        "one server, one welcome"
    );
    // (a cancel after the found match ends nothing on its own; the page closes the session)
    w.post(1, json!({ "t": "close" }));
    w.run(1500);
    assert!(w.all[1].last("closed").is_some(), "the closed session is told so");
}

#[test]
fn party_made_joined_talked_in_and_left_over_the_relay() {
    let mut w = World::new(2);
    w.post(0, presence("Velma"));
    w.post(1, presence("Fred"));
    w.run(300);
    let b = w.all[1].id.clone();
    w.post(0, json!({ "t": "party.invite", "to": b, "name": "Velma" }));
    let came = w.until(5000, |w| w.all[1].last("invited").is_some());
    assert!(came, "the invite came: {:?} / {:?}", w.all[0].events, w.all[1].events);
    let inv = w.all[1].last("invited").cloned().unwrap_or_default();
    assert_eq!(inv["kind"], "party");
    assert_eq!(inv["from"]["id"].as_str(), Some(w.all[0].id.as_str()));
    assert_eq!(w.all[0].party_size(), 1, "the leader's party of one at once");
    w.post(1, json!({ "t": "party.join", "code": inv["code"] }));
    let joined = w.until(8000, |w| w.all[0].party_size() == 2 && w.all[1].party_size() == 2);
    assert!(joined, "a party of two: {:?} {:?}", w.all[0].last("party"), w.all[1].last("party"));
    let names = |p: &Player| -> String {
        p.party()
            .and_then(|x| x["members"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|m| m["name"].as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(names(&w.all[0]), "Velma,Fred");
    assert_eq!(names(&w.all[1]), "Velma,Fred");
    // says both ways, each its own kinds
    w.post(0, json!({ "t": "party.say", "say": { "k": "queue", "modes": ["2v2", "ffa"] } }));
    w.post(1, json!({ "t": "party.say", "say": { "k": "queue", "modes": ["2v2"] } }));
    w.post(1, json!({ "t": "party.say", "say": { "k": "ready" } }));
    w.run(500);
    let kinds = |p: &Player| -> String {
        p.events
            .iter()
            .filter(|e| e["t"] == "party.say")
            .filter_map(|e| e["say"]["k"].as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(kinds(&w.all[1]), "queue", "the leader's say to the member");
    assert_eq!(kinds(&w.all[0]), "ready", "the member's ready to the leader, its queue dropped");
    // the leader leaves: the member's party ends, the leader left
    w.post(0, json!({ "t": "party.leave" }));
    let ended = w.until(5000, |w| w.all[1].last("party").is_some_and(|e| e["party"].is_null()));
    assert!(ended, "the member's party ended");
    let why = w.all[1].last("party").map(|e| e["why"].clone()).unwrap_or_default();
    assert_eq!(why, "the party leader left");
    assert!(
        w.all[0].last("party").is_some_and(|e| e["party"].is_null() && e["why"] == ""),
        "the leader's own leave: no why"
    );
}

#[test]
fn four_queuers_make_one_2v2_over_the_relay() {
    let mut w = World::new(4);
    for i in 0..4 {
        queue_with(&mut w, i, &format!("P{i}"), &["2v2"], 7919 * (i as u32 + 1), None, 60);
    }
    let f = all_found(&mut w, 15000).expect("all four found a match");
    let mut ids: Vec<String> = w.all.iter().map(|p| p.id.clone()).collect();
    ids.sort();
    assert!(f.iter().all(|m| m["match"] == f[0]["match"]), "one match: {f:?}");
    assert_eq!(humans(&f[0]), (ids, 0), "of the four queuers, no bot");
}

#[test]
fn a_queuer_alone_is_filled_with_a_bot_over_the_relay() {
    let mut w = World::new(1);
    queue_with(&mut w, 0, "Velma", &["1v1"], 7919, None, 1);
    let on = w.until(5000, |w| w.all[0].msgs("queue.state").iter().any(|m| m["on"] == true));
    assert!(on, "queued: {:?}", w.all[0].events);
    let f = all_found(&mut w, 15000).expect("a match found");
    assert_eq!(humans(&f[0]), (vec![w.all[0].id.clone()], 1), "the queuer and a bot: {}", f[0]);
}

#[test]
fn a_party_and_a_solo_queuer_make_one_match_over_the_relay() {
    let mut w = World::new(3);
    for (i, n) in ["Velma", "Fred", "Daphne"].into_iter().enumerate() {
        w.post(i, presence(n));
    }
    w.run(300);
    assert!(form(&mut w, 0, 1), "a party of two");
    let ids = vec![w.all[0].id.clone(), w.all[1].id.clone()];
    queue_with(&mut w, 2, "Daphne", &["2v2"], 3 * 7919, None, 3);
    queue_with(&mut w, 0, "Velma", &["2v2"], 7919, Some(&ids), 3);
    queue_with(&mut w, 1, "Fred", &["2v2"], 2 * 7919, Some(&ids), 3);
    let f = all_found(&mut w, 20000).expect("all three found a match");
    assert!(f.iter().all(|m| m["match"] == f[0]["match"]), "one match: {f:?}");
    let mut all: Vec<String> = w.all.iter().map(|p| p.id.clone()).collect();
    all.sort();
    assert_eq!(humans(&f[0]), (all, 1), "the party, the solo queuer and a bot: {}", f[0]);
    assert!(w.all[..2].iter().all(|p| p.party_size() == 2), "the party stays");
}

#[test]
fn a_party_that_ends_while_it_queues_is_out_on_both_sides() {
    let mut w = World::new(2);
    w.post(0, presence("Velma"));
    w.post(1, presence("Fred"));
    w.run(300);
    assert!(form(&mut w, 0, 1), "a party of two");
    let ids = vec![w.all[0].id.clone(), w.all[1].id.clone()];
    queue_with(&mut w, 0, "Velma", &["2v2"], 7919, Some(&ids), 30);
    queue_with(&mut w, 1, "Fred", &["2v2"], 2 * 7919, Some(&ids), 30);
    let on = w.until(5000, |w| w.all.iter().all(|p| p.msgs("queue.state").iter().any(|m| m["on"] == true)));
    assert!(on, "both queued");
    w.post(1, json!({ "t": "party.leave" }));
    let out = w.until(5000, |w| w.all.iter().all(|p| off(p, "party")));
    w.run(1000);
    let states: Vec<Vec<Value>> = w.all.iter().map(|p| p.msgs("queue.state")).collect();
    assert!(out, "both pages told off party: {states:?}");
    for (i, p) in w.all.iter().enumerate() {
        let offs = p.msgs("queue.state").iter().filter(|m| m["on"] == false).count();
        assert_eq!(offs, 1, "player {i} told off once: {states:?}");
        assert!(p.msgs("queue.found").is_empty() && p.last("closed").is_none(), "no match, no session closed");
    }
}

#[test]
fn a_party_whose_member_never_joins_is_out_late() {
    let mut w = World::new(2);
    w.post(0, presence("Velma"));
    w.post(1, presence("Fred"));
    w.run(300);
    assert!(form(&mut w, 0, 1), "a party of two");
    let ids = vec![w.all[0].id.clone(), w.all[1].id.clone()];
    queue_with(&mut w, 0, "Velma", &["2v2"], 7919, Some(&ids), 30);
    let late = w.until(15000, |w| off(&w.all[0], "partyLate"));
    assert!(late, "the leader told off partyLate: {:?}", w.all[0].msgs("queue.state"));
    assert!(w.all[0].last("closed").is_none() && w.all[0].party_size() == 2, "the session and the party stay");
}

#[test]
fn a_party_of_one_queues_solo_and_its_ticket_stands_when_it_leaves_the_party() {
    let mut w = World::new(2);
    w.post(0, presence("Velma"));
    w.post(1, presence("Fred"));
    w.run(300);
    assert!(form(&mut w, 0, 1), "a party of two");
    // the member leaves: the leader leads a party of one, and queues solo
    w.post(1, json!({ "t": "party.leave" }));
    assert!(w.until(5000, |w| w.all[0].party_size() < 2), "the leader's party of one");
    let me = vec![w.all[0].id.clone()];
    queue_with(&mut w, 0, "Velma", &["2v2"], 7919, Some(&me), 60);
    let on = w.until(5000, |w| w.all[0].msgs("queue.state").iter().any(|m| m["on"] == true));
    assert!(on, "queued: {:?}", w.all[0].events);
    // the leader leaves the party of one too: the solo ticket stands
    w.post(0, json!({ "t": "party.leave" }));
    w.run(1500);
    let offs: Vec<Value> = w.all[0].msgs("queue.state").into_iter().filter(|m| m["on"] == false).collect();
    assert!(offs.is_empty(), "a solo ticket is not taken out by the party: {offs:?}");
}
