//! The host cores a relay runs itself (`mvs_relay::hosted`): links by knock and bye, a hosted lobby's members only,
//! the queue's anyone, a core's kick, listing, end and panic, and a frozen leader's match and lobby.

mod common;

use common::*;
use mvs_protocol::match_wire::{INPUT_BYTES, MSG_INPUT, RESUME_MS, input_header};
use mvs_protocol::p2p_frame::split_message;
use mvs_protocol::relay_wire::QUEUE_HOST;
use mvs_relay::core::{BAD, Core, HOST_KNOCK_WAIT_MS, HOST_RELINK_MS};
use mvs_relay::limits::DEFAULT_LIMITS;
use serde_json::json;

fn hosting() -> World {
    World::hosting(DEFAULT_LIMITS, true)
}

/// A one-tick input of a slot of a match of `n` slots (the referee hears its human).
fn input(n: usize, slot: u8) -> Vec<u8> {
    let mut p = vec![0u8; input_header(n) + INPUT_BYTES];
    p[0] = MSG_INPUT;
    p[1] = slot;
    p[2..6].copy_from_slice(&2u32.to_le_bytes());
    p[6] = 1;
    p
}

fn bye_of(code: &str) -> serde_json::Value {
    json!({ "code": "host", "why": { "code": code } })
}

#[test]
fn a_hosted_lobby_links_its_members_by_a_knock() {
    let mut w = hosting();
    let a = w.join("a");
    let r = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 4, "host": true }));
    let (lobby, host) = (r["lobby"].as_str().unwrap().to_string(), r["host"].as_str().unwrap().to_string());
    assert_eq!(host, format!("{lobby}.host"));
    assert_eq!(w.core.hosts(), vec![host.clone()]);
    w.knock(&a, &host);
    w.say(&a, &host, "hello", json!({}));
    assert!(a.f.host_has("welcome"), "{:?}", a.f.host_kinds());
    w.say(&a, &host, "lobby.enter", json!({}));
    // a second knock on a live link does nothing
    a.f.clear_host();
    w.knock(&a, &host);
    assert!(a.f.host_kinds().is_empty());
    // a stranger is answered as for no one; channel 1 is dropped
    let s = w.join("s");
    w.knock(&s, &host);
    assert_eq!(s.f.take(), vec![peer_closed(&host)]);
    w.packet(&s, 1, &host, &[2]);
    assert!(s.f.take().is_empty());
    // a member's lobby leave: out of the core, which tells the others
    let b = w.join("b");
    w.req(&b, json!({ "op": "join", "lobby": lobby }));
    w.knock(&b, &host);
    w.say(&b, &host, "hello", json!({}));
    w.say(&b, &host, "lobby.join", json!({ "lobby": lobby }));
    assert!(b.f.host_has("lobby.state"));
    assert!(a.f.host_took("lobby.state").is_some_and(|m| m.to_string().contains("\"b\"")), "b in the core's lobby");
    a.f.clear_host();
    w.req(&b, json!({ "op": "leave", "lobby": lobby }));
    let state = a.f.host_took("lobby.state").expect("the core told the leader");
    assert!(!state.to_string().contains("\"b\""), "the core took it out: {state}");
    assert!(b.f.byes().is_empty(), "no bye to a member that left");
    b.f.take();
    w.say(&b, &host, "hello", json!({}));
    assert_eq!(b.f.take(), vec![peer_closed(&host)], "no longer a member");
}

#[test]
fn a_hosted_lobby_of_eight_seats_four_a_team_and_refuses_a_ninth() {
    let max = mvs_protocol::lobby_rules::MAX_LOBBY_PLAYERS;
    let mut w = hosting();
    let ps: Vec<P> = (0..=max).map(|i| w.join_at(&format!("p{i}"), &format!("10.0.0.{}", i + 1))).collect();
    let r = w.req(&ps[0], json!({ "op": "create", "bucket": "b", "max": max, "host": true }));
    let (lobby, host) = (r["lobby"].as_str().unwrap().to_string(), r["host"].as_str().unwrap().to_string());
    for (i, p) in ps[..max].iter().enumerate() {
        if i > 0 {
            w.req(p, json!({ "op": "join", "lobby": lobby }));
        }
        w.knock(p, &host);
        w.say(p, &host, "hello", json!({}));
        let t = if i == 0 { "lobby.enter" } else { "lobby.join" };
        w.say(p, &host, t, json!({ "lobby": lobby }));
        let f = p.f.clone();
        w.until("lobby.state", || f.host_has("lobby.state"));
    }
    let state = ps[max - 1].f.host_took("lobby.state").expect("the last one in sees the lobby");
    let teams: Vec<usize> =
        state["data"]["teams"].as_array().expect("teams").iter().map(|t| t.as_array().map_or(0, Vec::len)).collect();
    assert_eq!(teams, [4, 4], "a Versus of four a team: {state}");
    assert_eq!(w.refusal(&ps[max], json!({ "op": "join", "lobby": lobby })), mvs_relay::core::FULL, "a ninth");
}

#[test]
fn a_message_with_no_link_is_answered_closed() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (_, host) = w.hosted_lobby(&[&a, &b]);
    w.bye(&b, &host);
    w.say(&b, &host, "hello", json!({}));
    assert_eq!(b.f.take(), vec![peer_closed(&host)]);
    // the leader's bye ends its core
    w.bye(&a, &host);
    assert!(w.core.hosts().is_empty());
}

#[test]
fn the_queue_core_starts_at_its_first_knock_and_takes_anyone() {
    let mut w = hosting();
    let a = w.join("a");
    assert!(w.core.hosts().is_empty());
    w.knock(&a, QUEUE_HOST);
    assert_eq!(w.core.hosts(), vec![QUEUE_HOST.to_string()]);
    w.say(&a, QUEUE_HOST, "hello", json!({}));
    assert!(a.f.host_has("welcome"), "{:?}", a.f.host_kinds());
}

#[test]
fn a_direct_hosts_relay_runs_no_queue() {
    let mut w = World::hosting(DEFAULT_LIMITS, false);
    let a = w.join("a");
    w.knock(&a, QUEUE_HOST);
    assert_eq!(a.f.take(), vec![peer_closed(QUEUE_HOST)]);
    assert!(w.core.hosts().is_empty());
}

#[test]
fn a_relay_with_no_host_cores_refuses_a_hosted_lobby() {
    let mut core = Core::new(&DEFAULT_LIMITS, None, Box::new(|_| {}));
    let (s, _) = core.join("a", "10.0.0.1", Box::new(Fake::default()), 0.0).unwrap();
    let r = core.request(&s, &json!({ "op": "create", "bucket": "b", "max": 4, "host": true }), 0.0);
    assert_eq!(r, Err(BAD));
    let mut w = hosting();
    let a = w.join("a");
    assert_eq!(w.refusal(&a, json!({ "op": "create", "bucket": "b", "max": 4, "host": 1 })), BAD);
    assert_eq!(w.core.lobbies(), 0);
}

#[test]
fn a_bye_closes_the_link_and_a_knock_opens_another_after_the_relink_wait() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    w.bye(&b, &host);
    let f = a.f.clone();
    w.until("the core's lobby without b", || {
        f.host_took("lobby.state").is_some_and(|m| !m.to_string().contains("\"b\""))
    });
    // (inside the relink wait a knock is dropped)
    w.knock(&b, &host);
    assert!(b.f.take().is_empty() && b.f.host_kinds().is_empty(), "{:?}", b.f.host_kinds());
    w.later(HOST_RELINK_MS);
    w.knock(&b, &host);
    w.say(&b, &host, "hello", json!({}));
    w.say(&b, &host, "lobby.join", json!({ "lobby": lobby }));
    assert!(b.f.host_has("lobby.state"), "{:?}", b.f.host_kinds());
}

#[test]
fn the_leaders_leave_ends_the_core_and_its_lobby() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    w.req(&a, json!({ "op": "leave", "lobby": lobby }));
    assert!(w.core.hosts().is_empty());
    assert_eq!(w.core.lobbies(), 0);
    assert_eq!(b.f.byes(), vec![(host.clone(), bye_of("host_left"))]);
    assert_eq!(b.f.take(), vec![closed_msg(&lobby, "destroyed")]);
    // a knock on a core that went
    w.knock(&b, &host);
    assert_eq!(b.f.take(), vec![peer_closed(&host)]);
}

#[test]
fn the_cores_kick_is_the_relays_its_bye_then_its_close() {
    let mut w = hosting();
    let (a, b, c) = (w.join("a"), w.join("b"), w.join("c"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b, &c]);
    w.say(&a, &host, "lobby.kickPlayer", json!({ "id": "b" }));
    assert!(b.f.host_has("lobby.kicked"), "{:?}", b.f.host_kinds());
    assert_eq!(b.f.byes(), vec![(host.clone(), bye_of("kicked"))]);
    assert_eq!(b.f.take(), vec![closed_msg(&lobby, "kicked")]);
    assert_eq!(c.f.take(), vec![lobby_msg(&lobby, &["a", "c"])]);
    assert_eq!(a.f.take(), vec![lobby_msg(&lobby, &["a", "c"])], "the leader told too: the kick was its core's");
    // out of the lobby: no way back to its core
    w.knock(&b, &host);
    assert_eq!(b.f.take(), vec![peer_closed(&host)]);
}

#[test]
fn a_core_that_closes_a_link_kicks_its_member_from_the_lobby() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    // (a hello of another protocol: the core closes the link)
    let hello = json!({ "t": "hello", "proto": 0, "name": "Nb", "app": "test" }).to_string();
    for piece in split_message(hello.as_bytes(), 1170) {
        w.packet(&b, 0, &host, &piece);
    }
    assert_eq!(b.f.byes(), vec![(host.clone(), bye_of("protocol"))]);
    assert_eq!(b.f.take(), vec![closed_msg(&lobby, "kicked")]);
    assert_eq!(a.f.take(), vec![lobby_msg(&lobby, &["a"])]);
    // its lobby's member no more: refused
    w.knock(&b, &host);
    assert_eq!(b.f.take(), vec![peer_closed(&host)]);
    // on the queue's: what it sends on the closed link is dropped, a knock opens another
    w.knock(&b, QUEUE_HOST);
    for piece in split_message(hello.as_bytes(), 1170) {
        w.packet(&b, 0, QUEUE_HOST, &piece);
    }
    assert_eq!(b.f.byes(), vec![(QUEUE_HOST.to_string(), bye_of("protocol"))]);
    b.f.clear_host();
    w.say(&b, QUEUE_HOST, "hello", json!({}));
    assert!(b.f.take().is_empty() && b.f.host_kinds().is_empty(), "dropped on the closed link");
    w.later(HOST_RELINK_MS);
    w.knock(&b, QUEUE_HOST);
    w.say(&b, QUEUE_HOST, "hello", json!({}));
    assert!(b.f.host_has("welcome"), "a new link: {:?}", b.f.host_kinds());
}

#[test]
fn the_cores_listing_is_its_lobbys_and_told_its_owner() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let r = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 4, "host": true, "listed": { "at": 1 } }));
    let (lobby, host) = (r["lobby"].as_str().unwrap().to_string(), r["host"].as_str().unwrap().to_string());
    w.knock(&a, &host);
    w.say(&a, &host, "hello", json!({}));
    w.say(&a, &host, "lobby.enter", json!({}));
    a.f.take();
    w.say(&a, &host, "lobby.setOption", json!({ "key": "public", "value": true }));
    let told = a.f.take();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!((told[0]["t"].as_str(), told[0]["lobby"].as_str()), (Some("listing"), Some(lobby.as_str())));
    let info = told[0]["info"].as_str().expect("a public lobby's text").to_string();
    let found = w.search(&b, "b", 10, false);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["info"].as_str(), Some(info.as_str()), "a search shows the core's text");
    w.say(&a, &host, "lobby.setOption", json!({ "key": "public", "value": false }));
    assert_eq!(a.f.take(), vec![json!({ "t": "listing", "lobby": lobby })], "private: no text");
}

/// A knock and a bye over and over: no new link (and so no new budgets of the core's) inside the relink wait.
#[test]
fn a_knock_bye_loop_gets_no_new_link_inside_the_relink_wait() {
    let mut w = hosting();
    let a = w.join("a");
    w.knock(&a, QUEUE_HOST);
    w.say(&a, QUEUE_HOST, "hello", json!({}));
    assert!(a.f.host_has("welcome"));
    a.f.clear_host();
    for _ in 0..100 {
        w.bye(&a, QUEUE_HOST);
        w.knock(&a, QUEUE_HOST);
        w.say(&a, QUEUE_HOST, "hello", json!({}));
        w.later(10.0);
    }
    assert!(a.f.host_kinds().is_empty(), "no link the loop made: {:?}", a.f.host_kinds());
    w.later(HOST_RELINK_MS);
    w.knock(&a, QUEUE_HOST);
    w.say(&a, QUEUE_HOST, "hello", json!({}));
    assert_eq!(a.f.host_kinds(), vec!["server", "welcome"], "after the wait a new link");
}

/// A member of a hosted lobby that never links to its core (or whose link ended and that does not link again) is
/// kicked by the relay; the leader never is.
#[test]
fn a_member_that_never_links_is_kicked_after_the_knock_wait() {
    let mut w = hosting();
    let (a, b, c) = (w.join("a"), w.join("b"), w.join("c"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    w.req(&c, json!({ "op": "join", "lobby": lobby }));
    c.f.take();
    a.f.take();
    w.later(HOST_KNOCK_WAIT_MS - 100.0);
    assert!(c.f.take().is_empty(), "not yet");
    w.later(200.0);
    assert_eq!(c.f.take(), vec![closed_msg(&lobby, "kicked")]);
    assert_eq!(a.f.take(), vec![lobby_msg(&lobby, &["a", "b"])]);
    assert!(w.logs.borrow().iter().any(|l| *l == format!("{host} member never linked: kicked c")), "{:?}", w.logs);
    // a bye: kicked at the end of its relink wait and the knock wait, unless it links again
    w.bye(&b, &host);
    w.later(HOST_RELINK_MS + HOST_KNOCK_WAIT_MS - 100.0);
    assert!(!b.f.take().contains(&closed_msg(&lobby, "kicked")));
    w.later(200.0);
    assert!(b.f.take().contains(&closed_msg(&lobby, "kicked")));
    // one that links in time stays; the leader stays
    let d = w.join("d");
    w.req(&d, json!({ "op": "join", "lobby": lobby }));
    w.later(1000.0);
    w.knock(&d, &host);
    w.later(HOST_RELINK_MS + HOST_KNOCK_WAIT_MS + 1000.0);
    assert!(!d.f.take().contains(&closed_msg(&lobby, "kicked")));
    assert!(!a.f.take().contains(&closed_msg(&lobby, "kicked")));
    assert_eq!(w.core.hosts(), vec![host]);
}

/// A core that ends while a member's seat is parked: the bye it was sent is not replayed (its link is gone), so the
/// resume tells it peerClosed for the core.
#[test]
fn a_parked_member_is_owed_the_closed_core_at_its_resume() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    let got = b.f.got0();
    w.cut(&b);
    w.req(&a, json!({ "op": "leave", "lobby": lobby }));
    let (b2, _, resumed) = w.rejoin("b", Some(got));
    assert!(resumed.is_some(), "its seat resumed");
    let told = b2.f.take();
    assert!(told.contains(&peer_closed(&host)), "{told:?}");
    assert!(told.contains(&closed_msg(&lobby, "destroyed")), "{told:?}");
}

/// A panic carrying out a core's step (a bug in the relay, not the core) ends that core as a panic in it does: its
/// outputs are not left undone, its match not left counted.
#[test]
fn a_panic_carrying_out_a_cores_step_ends_that_core() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    w.run_to_the_go(&host, &[&a, &b]);
    assert_eq!(w.core.matches(), 1);
    b.f.0.borrow_mut().panic_at_host = true;
    // (a hello with another name: the core answers b)
    w.say(&b, &host, "hello", json!({ "name": "Other" }));
    w.later(1000.0);
    assert!(w.logs.borrow().iter().any(|l| *l == format!("{host} host: internal error: a panic")), "{:?}", w.logs);
    assert!(w.core.hosts().is_empty());
    assert_eq!((w.core.matches(), w.core.lobbies()), (0, 0));
    assert!(a.f.take().contains(&closed_msg(&lobby, "destroyed")));
    // the relay's match bound has its room back
    let (c, d) = (w.join("c"), w.join("d"));
    let (_, h2) = w.hosted_lobby(&[&c, &d]);
    w.run_to_the_go(&h2, &[&c, &d]);
    assert_eq!(w.core.matches(), 1);
}

#[test]
fn a_panic_in_a_core_ends_that_core_alone() {
    let mut w = hosting();
    let (a, b, c, d) = (w.join("a"), w.join("b"), w.join("c"), w.join("d"));
    let (l1, h1) = w.hosted_lobby(&[&a, &b]);
    let (_, h2) = w.hosted_lobby(&[&c, &d]);
    let m = w.run_to_the_go(&h1, &[&a, &b]);
    let r: Option<()> = w.core.with_host(&h1, |_, _| panic!("a core's bug"));
    assert_eq!(r, None);
    assert!(w.logs.borrow().iter().any(|l| *l == format!("{h1} host: internal error: a panic")), "{:?}", w.logs);
    assert_eq!(w.core.hosts(), vec![h2.clone()], "the other core goes on");
    assert_eq!(w.core.matches(), 0, "its match ended");
    for x in [&a, &b] {
        assert_eq!(x.f.byes(), vec![(h1.clone(), bye_of("ended"))], "{} told", x.id);
        assert_eq!(x.f.take(), vec![peer_closed(&m.rid), closed_msg(&l1, "destroyed")]);
    }
    assert!(c.f.byes().is_empty());
    c.f.clear_host();
    w.say(&c, &h2, "lobby.setTeamStyle", json!({ "style": 0 }));
    assert!(c.f.host_has("lobby.state"), "the other core answers: {:?}", c.f.host_kinds());
}

/// A leader frozen in a custom match (its connection gone, its seat parked): away, then gone at RESUME_MS, which
/// ends the 1v1 for the other; its seat then expires and its lobby ends with it.
#[test]
fn a_frozen_leader_is_gone_from_its_match_then_its_seat_and_lobby_end() {
    let mut w = hosting();
    let (a, b) = (w.join("a"), w.join("b"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b]);
    let m = w.run_to_the_go(&host, &[&a, &b]);
    w.packet(&a, 1, &m.rid, &input(m.n, m.slots[0]));
    w.cut(&a);
    let mut away_at = None;
    let mut ended_at = None;
    let start = w.t;
    while w.core.lobbies() > 0 {
        assert!(w.t - start < 2.0 * RESUME_MS + 10_000.0, "the lobby ends in time");
        w.later(250.0);
        w.core.expire(w.t);
        if w.core.matches() > 0 {
            w.packet(&b, 1, &m.rid, &input(m.n, m.slots[1]));
        }
        if away_at.is_none() && b.f.host_has("match.away") {
            away_at = Some(w.t - start);
        }
        if ended_at.is_none() && w.core.matches() == 0 {
            ended_at = Some(w.t - start);
        }
    }
    let (away_at, ended_at) = (away_at.expect("away"), ended_at.expect("the match ended"));
    assert!(away_at < ended_at, "away {away_at} before the end {ended_at}");
    assert!(ended_at <= RESUME_MS + 1000.0, "gone at RESUME_MS: {ended_at}");
    assert!(w.logs.borrow().iter().any(|l| *l == format!("{host} match silent Na")), "{:?}", w.logs);
    let told = b.f.take();
    assert!(told.contains(&peer_closed(&m.rid)), "b out of the referee's: {told:?}");
    assert_eq!(told.last(), Some(&closed_msg(&lobby, "destroyed")), "{told:?}");
    assert_eq!(b.f.byes(), vec![(host.clone(), bye_of("host_left"))]);
    assert_eq!((w.core.hosts().len(), w.core.players()), (0, 1));
}

/// The relay full (`players`: 500) of queuers of its queue core: every ticket found, each pair's match at its go
/// (`matches`: 250, the relay's bound reached and no more), in the time a relay has.
#[test]
fn five_hundred_tickets_reach_their_matches_in_time() {
    let mut w = hosting();
    let (started, t0) = (std::time::Instant::now(), w.t);
    let ps: Vec<P> = (0..500).map(|i| w.join_at(&format!("q{i}"), &format!("10.{}.{}.1", i / 200, i % 200))).collect();
    for p in &ps {
        w.knock(p, QUEUE_HOST);
        w.say(p, QUEUE_HOST, "hello", json!({}));
        let join = json!({ "compat": "ctest.1", "mode": "1v1", "character": "shaggy", "skin": "" });
        w.say(p, QUEUE_HOST, "queue.join", join);
    }
    let joined = started.elapsed();
    let fs: Vec<Fake> = ps.iter().map(|p| p.f.clone()).collect();
    w.until("queue.found", || fs.iter().all(|f| f.host_has("queue.found")));
    let found = started.elapsed();
    for p in &ps {
        w.say(p, QUEUE_HOST, "pm.perks", json!({ "perks": [] }));
    }
    w.until("match.start", || fs.iter().all(|f| f.host_has("match.start")));
    for p in &ps {
        w.say(p, QUEUE_HOST, "match.loaded", json!({}));
    }
    w.until("match.go", || fs.iter().all(|f| f.host_has("match.go")));
    let go = started.elapsed();
    assert_eq!(w.core.matches(), 250);
    let rids: std::collections::HashSet<String> =
        fs.iter().map(|f| f.host_took("match.go").unwrap()["referee"].as_str().unwrap().to_string()).collect();
    assert_eq!(rids.len(), 250, "a referee per pair");
    let waited = w.t - t0;
    eprintln!(
        "500 tickets: joined {joined:?}, all found {found:?}, all at their go {go:?} ({waited} ms of the relay's)"
    );
    assert!(go.as_secs_f64() < 5.0, "500 tickets to their go in {go:?}");
}
