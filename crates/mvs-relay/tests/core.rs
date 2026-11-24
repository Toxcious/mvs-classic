//! The core's rules (relay/test.ts, its first half): players in process and a clock the checks move on.

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use common::*;
use mvs_protocol::match_wire::{
    ALL_TICKS, INPUT_BYTES, INPUT_DELAY, InputHeader, MAX_SLOTS, MSG_INPUT, MSG_RULED, NO_END, RESUME_MS, SILENT_MS,
    TICK_MS, input_header, input_packet, packet_fits, runs,
};
use mvs_protocol::messages::HostWhy;
use mvs_protocol::referee::{Referee, RefereeOut, SlotNews};
use mvs_protocol::relay_wire::{RELAY_INFO_BYTES, SHORT_ALPHABET, SHORT_LEN, ServerMessage, WHO_LIMIT};
use mvs_relay::core::{CorePlayer, RING_FRAMES, Taken};
use mvs_relay::limits::{DEFAULT_LIMITS, RelayLimits};
use serde_json::{Value, json};

#[test]
fn search_listed_not_full_not_own_of_its_bucket_at_most_limit() {
    let mut w = world();
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let listed = w.create(&a, "b", 2, Some(5.0), None);
    let hidden = w.lobby(&a);
    w.create(&a, "other", 4, Some(1.0), None);
    let found: Vec<_> =
        w.search(&b, "b", 10, false).iter().map(|l| (l["id"].clone(), l["members"].clone(), l["at"].clone())).collect();
    assert_eq!(found, vec![(json!(listed), json!(1), json!(5.0))]);
    assert_eq!(w.search(&a, "b", 10, false), Vec::<Value>::new(), "not the searcher own");
    w.req(&b, json!({ "op": "join", "lobby": listed }));
    assert!(w.search(&c, "b", 10, false).is_empty(), "a full one");
    let full: Vec<_> = w.search(&c, "b", 10, true).iter().map(|l| (l["id"].clone(), l["members"].clone())).collect();
    assert_eq!(full, vec![(json!(listed), json!(2))], "a full one, in a search of the full ones too");
    assert_eq!(w.refusal(&c, json!({ "op": "join", "lobby": listed })), "EOS_Lobby_TooManyPlayers");
    assert_eq!(
        w.req(&c, json!({ "op": "join", "lobby": hidden })),
        json!({ "owner": "ra", "members": ["ra", "rc"] }),
        "an unlisted one is joined by its id"
    );
    for i in 0..3 {
        w.create(&c, "b", 4, Some(f64::from(i)), None);
    }
    assert_eq!(w.search(&b, "b", 2, false).len(), 2);
}

#[test]
fn list_the_owner_only_new_attributes_or_out_of_the_searches() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    let l = w.create(&a, "b", 4, Some(1.0), Some("one"));
    let at_info = |v: Vec<Value>| {
        v.iter().map(|x| (x["at"].as_f64().unwrap(), x["info"].as_str().unwrap().to_string())).collect::<Vec<_>>()
    };
    let r = w.search(&b, "b", 10, false);
    assert_eq!(at_info(r), vec![(1.0, "one".to_string())]);
    assert_eq!(w.refusal(&b, json!({ "op": "list", "lobby": l, "listed": null })), "EOS_Lobby_NotOwner");
    assert_eq!(w.refusal(&b, json!({ "op": "list", "lobby": "lnone", "listed": null })), "EOS_NotFound");
    let long = "x".repeat(RELAY_INFO_BYTES + 1);
    assert_eq!(
        w.refusal(&a, json!({ "op": "list", "lobby": l, "listed": { "at": 2, "info": long } })),
        "EOS_InvalidParameters"
    );
    assert_eq!(
        w.refusal(&a, json!({ "op": "create", "bucket": "b", "max": 2, "listed": { "at": 2, "info": 7 } })),
        "EOS_InvalidParameters"
    );
    w.req(&a, json!({ "op": "list", "lobby": l, "listed": { "at": 2, "info": "two" } }));
    let r = w.search(&b, "b", 10, false);
    assert_eq!(at_info(r), vec![(2.0, "two".to_string())]);
    w.req(&a, json!({ "op": "list", "lobby": l, "listed": null }));
    assert!(w.search(&b, "b", 10, false).is_empty());
    let hidden = w.lobby(&a);
    w.req(&a, json!({ "op": "list", "lobby": hidden, "listed": { "at": 3 } }));
    let r: Vec<_> = w.search(&b, "b", 10, false).iter().map(|x| (x["id"].clone(), x["info"].clone())).collect();
    assert_eq!(r, vec![(json!(hidden), json!(""))], "an unlisted one listed later, no info");
}

#[test]
fn join_and_leave_the_other_members_told_the_owner_leave_destroys_it() {
    let mut w = world();
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let l = w.lobby(&a);
    assert_eq!(w.refusal(&b, json!({ "op": "join", "lobby": "lnone" })), "EOS_NotFound");
    w.req(&b, json!({ "op": "join", "lobby": l }));
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra", "rb"])]);
    assert!(b.f.take().is_empty(), "the joiner has its answer");
    assert_eq!(
        w.req(&b, json!({ "op": "join", "lobby": l })),
        json!({ "owner": "ra", "members": ["ra", "rb"] }),
        "a second join answers the same"
    );
    assert!(a.f.take().is_empty());
    w.req(&c, json!({ "op": "join", "lobby": l }));
    a.f.take();
    b.f.take();
    w.req(&c, json!({ "op": "leave", "lobby": l }));
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra", "rb"])]);
    assert_eq!(b.f.take(), vec![lobby_msg(&l, &["ra", "rb"])]);
    assert!(c.f.take().is_empty());
    w.req(&a, json!({ "op": "leave", "lobby": l }));
    assert_eq!(b.f.take(), vec![closed_msg(&l, "destroyed")]);
    assert_eq!(w.core.lobbies(), 0);
    assert_eq!(w.refusal(&b, json!({ "op": "join", "lobby": l })), "EOS_NotFound");
}

#[test]
fn kick_the_owner_only_the_kicked_told_closed() {
    let mut w = world();
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let l = w.lobby(&a);
    w.req(&b, json!({ "op": "join", "lobby": l }));
    w.req(&c, json!({ "op": "join", "lobby": l }));
    a.f.take();
    b.f.take();
    w.req(&b, json!({ "op": "kick", "lobby": l, "member": "rc" }));
    assert!(c.f.take().is_empty(), "not the owner: nothing");
    w.req(&a, json!({ "op": "kick", "lobby": l, "member": "rc" }));
    assert_eq!(c.f.take(), vec![closed_msg(&l, "kicked")]);
    assert_eq!(b.f.take(), vec![lobby_msg(&l, &["ra", "rb"])]);
    assert!(a.f.take().is_empty());
    w.req(&c, json!({ "op": "join", "lobby": l }));
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra", "rb", "rc"])], "a kicked player may join again");
}

#[test]
fn links_opened_by_the_first_packet_closed_by_either_side() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    w.together(&[&a, &b]);
    w.packet(&a, 0, "rb", &[1, 2]);
    w.packet(&a, 1, "rb", &[3]);
    assert_eq!(b.f.packets(), vec![(0, "ra".into(), vec![1, 2]), (1, "ra".into(), vec![3])]);
    w.close_peer(&b, "ra");
    assert_eq!(a.f.take(), vec![peer_closed("rb")]);
    assert!(b.f.take().is_empty());
    w.close_peer(&b, "ra");
    assert!(a.f.take().is_empty(), "a closed link: nothing more");
    w.packet(&a, 0, "rnone", &[1]);
    assert_eq!(a.f.take(), vec![peer_closed("rnone")], "to no one on channel 0: the link is told closed");
    w.packet(&a, 1, "rnone", &[1]);
    w.packet(&a, 0, "ra", &[1]);
    assert_eq!(a.f.take(), vec![peer_closed("ra")], "not to itself");
    assert_eq!(a.f.npackets(), 0);
}

#[test]
fn the_packet_gate_a_shared_lobby_or_the_other_side_sent_first() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    w.packet(&a, 0, "rb", &[1]);
    w.packet(&a, 1, "rb", &[1]);
    assert_eq!(
        (b.f.npackets(), a.f.take(), b.f.take()),
        (0, vec![peer_closed("rb")], vec![]),
        "strangers: dropped, answered as to no one"
    );
    w.packet(&a, 0, "rnone", &[1]);
    assert_eq!(a.f.take(), vec![peer_closed("rnone")], "the same answer as for an id not connected");
    let l = w.together(&[&a, &b]);
    w.packet(&a, 0, "rb", &[2]);
    let firsts = |f: &Fake| f.packets().iter().map(|p| p.2[0]).collect::<Vec<_>>();
    assert_eq!(firsts(&b.f), vec![2], "in one lobby");
    w.req(&b, json!({ "op": "leave", "lobby": l }));
    a.f.take();
    w.packet(&b, 0, "ra", &[3]);
    assert_eq!(firsts(&a.f), vec![3], "out of the lobby, b may answer a, who sent it first");
    w.packet(&a, 1, "rb", &[4]);
    assert_eq!(firsts(&b.f), vec![2, 4], "and a b, who has sent it since");
    w.close_peer(&b, "ra");
    a.f.take();
    w.packet(&a, 0, "rb", &[5]);
    w.packet(&b, 0, "ra", &[5]);
    assert_eq!(
        (a.f.npackets(), b.f.npackets(), a.f.take(), b.f.take()),
        (1, 2, vec![peer_closed("rb")], vec![peer_closed("ra")]),
        "the link closed: strangers again"
    );
}

#[test]
fn invites_to_a_connected_id_a_payload_of_at_most_500_bytes() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    w.req(&a, json!({ "op": "invite", "to": "rb", "payload": "x".repeat(500) }));
    assert_eq!(b.f.take(), vec![invite_msg("ra", &"x".repeat(500))]);
    assert_eq!(w.refusal(&a, json!({ "op": "invite", "to": "rnone", "payload": "x" })), "EOS_NotFound");
    assert_eq!(
        w.refusal(&a, json!({ "op": "invite", "to": "rb", "payload": "x".repeat(501) })),
        "EOS_InvalidParameters"
    );
    assert_eq!(
        w.refusal(&a, json!({ "op": "invite", "to": "rb", "payload": "é".repeat(251) })),
        "EOS_InvalidParameters",
        "bytes, not characters"
    );
}

#[test]
fn bad_requests_are_refused_not_answered() {
    let mut w = world();
    let a = w.join("ra");
    for r in [
        json!({ "op": "create", "bucket": "", "max": 2 }),
        json!({ "op": "create", "bucket": "b", "max": 0 }),
        json!({ "op": "create", "bucket": "b", "max": 2.5 }),
        json!({ "op": "create", "bucket": "b", "max": 2, "listed": { "at": "x" } }),
        json!({ "op": "search", "bucket": "b", "limit": -1 }),
        json!({ "op": "what" }),
        json!({ "op": "invite", "to": 3, "payload": "x" }),
    ] {
        assert_eq!(w.refusal(&a, r.clone()), "EOS_InvalidParameters", "{r}");
    }
    assert_eq!(w.core.lobbies(), 0);
}

#[test]
fn disconnect_the_owner_a_member_a_linked_player() {
    let mut w = world();
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let l = w.lobby(&a);
    let m = w.lobby(&c);
    w.req(&b, json!({ "op": "join", "lobby": l }));
    w.req(&b, json!({ "op": "join", "lobby": m }));
    w.packet(&a, 0, "rb", &[1]);
    for p in [&a, &b, &c] {
        p.f.take();
    }
    w.leave(&b);
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra"]), peer_closed("rb")]);
    assert_eq!(c.f.take(), vec![lobby_msg(&m, &["rc"])]);
    assert_eq!(w.core.players(), 2);
    w.leave(&b);
    assert_eq!(w.refusal(&b, json!({ "op": "create", "bucket": "b", "max": 2 })), "gone");
    let d = w.join("rd");
    w.req(&d, json!({ "op": "join", "lobby": l }));
    a.f.take();
    w.leave(&a);
    assert_eq!(d.f.take(), vec![closed_msg(&l, "destroyed")]);
    assert!(w.search(&d, "b", 10, false).is_empty());
    assert_eq!(w.core.lobbies(), 1);
}

// Deviation from relay/test.ts: a second join of an id takes its seat over (the port's own rule); a fresh sign-in (no
// count in its hello) has a new seat, the old one's lobbies and links gone.
#[test]
fn a_second_join_of_an_id_takes_the_seat_over() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    let l = w.lobby(&a);
    w.req(&b, json!({ "op": "join", "lobby": l }));
    b.f.take();
    let (a2, took_over, resumed) = w.rejoin_at("ra", "10.0.0.9", None);
    assert!(took_over);
    assert_eq!(resumed, None, "a fresh sign-in: a new seat");
    assert_eq!(a.f.dropped().as_deref(), Some("resumed elsewhere"));
    assert_eq!(b.f.take(), vec![closed_msg(&l, "destroyed")]);
    w.leave(&a);
    assert_eq!(w.core.players(), 2, "the old session leaving does not take the new one out");
    w.together(&[&b, &a2]);
    w.packet(&a2, 0, "rb", &[7]);
    assert_eq!(b.f.packets().last().cloned(), Some((0, "ra".into(), vec![7])));
}

#[test]
fn limits_players_lobbies_per_player_members_per_lobby() {
    let mut w =
        World::new(RelayLimits { players: 2.0, lobbies_per_player: 2.0, members_per_lobby: 3.0, ..DEFAULT_LIMITS });
    let _a = w.join("ra");
    let b = w.join("rb");
    let t = w.t;
    assert_eq!(w.core.join("rc", "x", Box::new(Fake::default()), t).err().as_deref(), Some("relay full"));
    assert_eq!(w.join("ra").id, "ra", "a second join of a signed-in id is not refused when full");
    let a3 = w.join_at("ra", "x");
    assert_eq!(w.refusal(&a3, json!({ "op": "create", "bucket": "b", "max": 4 })), "EOS_InvalidParameters");
    let l1 = w.create(&a3, "b", 3, None, None);
    w.create(&a3, "b", 2, None, None);
    assert_eq!(w.refusal(&a3, json!({ "op": "create", "bucket": "b", "max": 2 })), "EOS_LimitExceeded");
    w.create(&b, "b", 2, None, None);
    w.create(&b, "b", 2, None, None);
    assert_eq!(w.refusal(&b, json!({ "op": "join", "lobby": l1 })), "EOS_LimitExceeded");
}

#[test]
fn limits_searches_refused_over_the_rate_dropped_at_three_times_it() {
    let mut w = World::new(RelayLimits { searches_per_minute: 3.0, ..DEFAULT_LIMITS });
    let a = w.join("ra");
    let s = json!({ "op": "search", "bucket": "b", "limit": 1 });
    for _ in 0..3 {
        w.search(&a, "b", 10, false);
    }
    w.wait(20000.0);
    w.search(&a, "b", 10, false);
    assert_eq!(w.refusal(&a, s.clone()), "EOS_LimitExceeded", "over the rate: refused");
    assert_eq!(a.f.dropped(), None, "not dropped");
    w.wait(20000.0);
    w.search(&a, "b", 10, false);
    assert_eq!(a.f.dropped(), None, "the rate refills");
    // asked 6 times of 9 a minute (a burst of 9), 6 back of the 40 s passed: 8 more refused, the 9th drops
    for _ in 0..8 {
        assert_eq!(w.refusal(&a, s.clone()), "EOS_LimitExceeded");
    }
    assert_eq!(a.f.dropped(), None);
    w.refusal(&a, s);
    assert_eq!(a.f.dropped().as_deref(), Some("too many searches"), "three times the rate asked: dropped");
    assert_eq!(w.core.players(), 0);
}

#[test]
fn limits_invites_per_player_refused_over_the_rate_dropped_at_three_times_it() {
    let mut w = World::new(RelayLimits {
        invites_per_minute: 2.0,
        invites_to_player: 100.0,
        invites_per_address: 100.0,
        ..DEFAULT_LIMITS
    });
    let b = w.join("rb");
    let others: Vec<P> =
        ["rc", "rd", "rf"].iter().enumerate().map(|(i, id)| w.join_at(id, &format!("10.0.1.{i}"))).collect();
    let inv = |to: &str| json!({ "op": "invite", "to": to, "payload": "" });
    w.req(&b, inv("rc"));
    w.req(&b, inv("rd"));
    assert_eq!(w.refusal(&b, inv("rf")), "EOS_LimitExceeded", "over the rate: refused");
    assert_eq!(b.f.dropped(), None, "not dropped");
    assert_eq!(others.iter().map(|o| o.f.take().len()).collect::<Vec<_>>(), vec![1, 1, 0]);
    w.wait(30000.0);
    w.req(&b, inv("rf"));
    assert_eq!(others[2].f.take().len(), 1, "the rate refills");
    // asked 4 times of 6 a minute (a burst of 6), 3 back of the 30 s passed: 5 more refused, the 6th drops
    for _ in 0..5 {
        assert_eq!(w.refusal(&b, inv("rf")), "EOS_LimitExceeded");
    }
    assert_eq!(b.f.dropped(), None);
    w.refusal(&b, inv("rf"));
    assert_eq!(b.f.dropped().as_deref(), Some("too many invites"));
}

#[test]
fn limits_one_invite_a_pair_per_invite_again_ms_invites_to_player() {
    let mut w = World::new(RelayLimits { invites_to_player: 4.0, invite_again_ms: 15000.0, ..DEFAULT_LIMITS });
    let v = w.join_at("rv", "10.0.9.9");
    let a = w.join_at("ra", "10.0.0.1");
    let inv = |to: &str| json!({ "op": "invite", "to": to, "payload": "x" });
    w.req(&a, inv("rv"));
    w.wait(14999.0);
    assert_eq!(w.refusal(&a, inv("rv")), "EOS_LimitExceeded", "the same pair within 15 s");
    w.wait(1.0);
    w.req(&a, inv("rv"));
    assert_eq!(v.f.take().len(), 2);
    // 4 a minute to one player, 2 at once: of the two 15 s apart, one is back; senders of other addresses, new ids each
    let s1 = w.join_at("r1", "10.0.0.2");
    let s2 = w.join_at("r2", "10.0.0.3");
    w.req(&s1, inv("rv"));
    assert_eq!(w.refusal(&s2, inv("rv")), "EOS_LimitExceeded", "the invited player has had its rate");
    assert_eq!(s2.f.dropped(), None, "a refused sender is not dropped");
    w.wait(15000.0);
    w.req(&s2, inv("rv"));
    assert_eq!(w.refusal(&s1, inv("rv")), "EOS_LimitExceeded");
    assert_eq!(v.f.take().len(), 2);
    // a refusal took nothing of the sender's own rates: s2 invites another at once
    let o = w.join_at("ro", "10.0.8.8");
    w.req(&s2, inv("ro"));
    assert_eq!(o.f.take().len(), 1);
}

#[test]
fn limits_invites_of_one_address_whatever_its_ids() {
    let mut w = World::new(RelayLimits { invites_per_address: 3.0, ..DEFAULT_LIMITS });
    let targets: Vec<P> = (0..4).map(|i| w.join_at(&format!("rt{i}"), &format!("10.0.5.{i}"))).collect();
    let inv = |to: &str| json!({ "op": "invite", "to": to, "payload": "" });
    // a new id each time, all of one IPv6 /64
    for (i, t) in targets.iter().take(3).enumerate() {
        let s = w.join_at(&format!("rs{i}"), &format!("2001:db8:1:2::{}", i + 1));
        w.req(&s, inv(&t.id));
    }
    let last = w.join_at("rs9", "2001:db8:1:2::99");
    assert_eq!(w.refusal(&last, inv("rt3")), "EOS_LimitExceeded");
    let other = w.join_at("rother", "2001:db8:1:3::1");
    w.req(&other, inv("rt3"));
    assert_eq!(
        targets.iter().map(|t| t.f.take().len()).collect::<Vec<_>>(),
        vec![1, 1, 1, 1],
        "another /64 is not counted"
    );
    w.wait(20000.0);
    w.req(&last, inv("rt2"));
    assert_eq!(targets[2].f.take().len(), 1, "the rate refills");
}

#[test]
fn limits_lobbies_owned_per_address_joins_per_player_and_per_address() {
    let mut w = World::new(RelayLimits {
        lobbies_per_address: 3.0,
        joins_per_minute: 2.0,
        joins_per_address: 3.0,
        ..DEFAULT_LIMITS
    });
    let a = w.join_at("ra", "10.0.0.1");
    let b = w.join_at("rb", "10.0.0.1");
    let l1 = w.lobby(&a);
    w.lobby(&a);
    w.lobby(&b);
    let cr = json!({ "op": "create", "bucket": "b", "max": 4 });
    assert_eq!(w.refusal(&b, cr.clone()), "EOS_LimitExceeded", "the address owns three");
    let c = w.join_at("rc", "10.0.0.2");
    let own = w.lobby(&c);
    w.req(&c, json!({ "op": "leave", "lobby": own }));
    w.req(&a, json!({ "op": "leave", "lobby": l1 }));
    w.lobby(&b);
    assert_eq!(w.refusal(&b, cr), "EOS_LimitExceeded", "a lobby destroyed is not counted");
    // joins: c's own two a minute; a lobby gone counts; a join of a lobby it is in does not
    let h = w.join_at("rh", "10.0.7.7");
    let hl = [w.lobby(&h), w.lobby(&h), w.lobby(&h)];
    let join = |l: &str| json!({ "op": "join", "lobby": l });
    w.req(&c, join(&hl[0]));
    w.req(&c, join(&hl[0]));
    assert_eq!(w.refusal(&c, join("lnone")), "EOS_NotFound");
    assert_eq!(w.refusal(&c, join(&hl[1])), "EOS_LimitExceeded", "the player's two");
    assert_eq!(c.f.dropped(), None, "refused, not dropped");
    // the address's three: a new id of c's address has one left
    let d = w.join_at("rd", "10.0.0.2");
    w.req(&d, join(&hl[1]));
    let e = w.join_at("re", "10.0.0.2");
    assert_eq!(w.refusal(&e, join(&hl[2])), "EOS_LimitExceeded", "the address's three");
    let g = w.join_at("rg", "10.0.0.3");
    w.req(&g, join(&hl[2]));
    w.wait(30000.0);
    w.req(&c, join(&hl[1]));
}

fn presence(name: &str) -> Value {
    json!({ "name": name, "icon": "", "busy": false, "listed": true, "invites": true })
}
fn with(mut v: Value, k: &str, x: Value) -> Value {
    v[k] = x;
    v
}
fn set_presence(w: &mut World, p: &P, pr: Value) {
    w.req(p, json!({ "op": "presence", "presence": pr }));
}
fn who(w: &mut World, p: &P, limit: f64) -> Vec<Value> {
    w.req(p, json!({ "op": "who", "limit": limit }))["players"].as_array().unwrap().clone()
}
fn names(v: &[Value]) -> Vec<String> {
    v.iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn presence_the_name_cleaned_and_checked_the_icon_an_asset_name() {
    let mut w = world();
    let a = w.join("ra");
    let b = w.join_at("rb", "10.0.0.2");
    let p = presence("Player");
    let mut bad = |pr: Option<Value>| {
        let mut r = json!({ "op": "presence" });
        if let Some(pr) = pr {
            r["presence"] = pr;
        }
        w.refusal(&a, r)
    };
    for (pr, what) in [
        (None, "undefined"),
        (Some(json!("x")), "a string"),
        (Some(with(p.clone(), "name", json!(3))), "a name not text"),
        (Some(with(p.clone(), "name", json!(""))), "no name"),
        (Some(with(p.clone(), "name", json!(" \u{202e} "))), "nothing left once cleaned"),
        (Some(with(p.clone(), "name", json!("f.u.c.k"))), "a word of the list"),
        (Some(with(p.clone(), "icon", json!("../x"))), "an icon path"),
        (Some(with(p.clone(), "icon", json!("constructor"))), "a name every object has"),
        (Some(with(p.clone(), "icon", json!("x".repeat(97)))), "an icon too long"),
        (Some(with(p.clone(), "busy", json!(1))), "busy"),
        (Some(with(p.clone(), "listed", json!("yes"))), "listed"),
        (Some(with(p.clone(), "invites", Value::Null)), "invites"),
    ] {
        assert_eq!(bad(pr), "EOS_InvalidParameters", "{what}");
    }
    assert!(who(&mut w, &b, f64::from(WHO_LIMIT)).is_empty(), "nothing refused was taken");
    let long = format!("  Long\u{200b}  name {}", "x".repeat(30));
    set_presence(&mut w, &a, with(presence(&long), "icon", json!("ProfileIcon_AS_RAM_Morty_Infini-Morty")));
    assert_eq!(
        who(&mut w, &b, f64::from(WHO_LIMIT)),
        vec![
            json!({ "id": "ra", "name": "Long name xxxxxxxxxx", "icon": "ProfileIcon_AS_RAM_Morty_Infini-Morty", "busy": false })
        ],
        "cleaned and cut to NAME_LIMIT"
    );
}

#[test]
fn presence_a_change_at_most_once_per_presence_every_ms() {
    let mut w = World::new(RelayLimits { presence_every_ms: 1000.0, ..DEFAULT_LIMITS });
    let a = w.join("ra");
    let b = w.join_at("rb", "10.0.0.2");
    set_presence(&mut w, &a, presence("One"));
    w.wait(999.0);
    set_presence(&mut w, &a, presence("One"));
    assert_eq!(w.refusal(&a, json!({ "op": "presence", "presence": presence("Two") })), "EOS_LimitExceeded");
    assert_eq!(a.f.dropped(), None, "not dropped");
    let got = who(&mut w, &b, 200.0);
    assert_eq!(names(&got), vec!["One"], "the refused one not taken");
    w.wait(1.0);
    set_presence(&mut w, &a, with(presence("One"), "busy", json!(true)));
    let got = who(&mut w, &b, 200.0);
    assert_eq!((got[0]["name"].clone(), got[0]["busy"].clone()), (json!("One"), json!(true)));
}

/// The status goes to the online list as it was given; none, or a word the relay does not know, is left out and the
/// presence still taken (an older or newer client).
#[test]
fn who_carries_a_status_and_leaves_out_none_or_an_unknown_one() {
    let mut w = World::new(RelayLimits { who_per_minute: 100.0, who_per_address: 100.0, ..DEFAULT_LIMITS });
    let a = w.join("ra");
    let b = w.join_at("rb", "10.0.0.2");
    for (status, want) in [
        (json!("2v2"), Some(json!("2v2"))),
        (json!("lobby"), Some(json!("lobby"))),
        (json!("spectating"), None),
        (json!(3), None),
        (Value::Null, None),
    ] {
        w.wait(DEFAULT_LIMITS.presence_every_ms);
        set_presence(&mut w, &a, with(presence("One"), "status", status.clone()));
        let got = who(&mut w, &b, 200.0);
        assert_eq!(got.len(), 1, "{status} taken");
        assert_eq!(got[0].get("status").cloned(), want, "{status}");
    }
}

#[test]
fn who_the_listed_players_but_the_asker_by_name_at_most_the_limit() {
    let mut w = World::new(RelayLimits { who_per_minute: 100.0, who_per_address: 100.0, ..DEFAULT_LIMITS });
    let asker = w.join_at("rz", "10.0.0.9");
    set_presence(&mut w, &asker, presence("Asker"));
    w.join_at("rnone", "10.0.0.8");
    for (i, name) in ["bob", "Alice", "carol", "Bob", "alice"].iter().enumerate() {
        let p = w.join_at(&format!("r{i}"), &format!("10.0.1.{i}"));
        set_presence(&mut w, &p, presence(name));
    }
    let hidden = w.join_at("rhidden", "10.0.0.7");
    set_presence(&mut w, &hidden, with(presence("Aaron"), "listed", json!(false)));
    let got = who(&mut w, &asker, 200.0);
    assert_eq!(
        names(&got),
        vec!["Alice", "alice", "Bob", "bob", "carol"],
        "not the asker, the unlisted or one with no presence"
    );
    for p in &got {
        let mut keys: Vec<&String> = p.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["busy", "icon", "id", "name"], "id, name, icon and busy only");
    }
    let two: Vec<_> = who(&mut w, &asker, 2.0).iter().map(|p| p["id"].clone()).collect();
    assert_eq!(two, vec![json!("r1"), json!("r4")]);
    for limit in [json!(0), json!(1.5), json!("x")] {
        assert_eq!(w.refusal(&asker, json!({ "op": "who", "limit": limit })), "EOS_InvalidParameters");
    }
    for i in 0..WHO_LIMIT + 5 {
        let p = w.join_at(&format!("rm{i}"), &format!("10.1.{}.{}", i >> 8, i & 255));
        set_presence(&mut w, &p, presence(&format!("m{i}")));
    }
    assert_eq!(who(&mut w, &asker, 1e9).len(), WHO_LIMIT as usize);
}

#[test]
fn who_a_presence_is_forgotten_at_the_connections_end_and_a_second_join() {
    let mut w = world();
    let asker = w.join_at("rz", "10.0.0.9");
    let a = w.join("ra");
    set_presence(&mut w, &a, presence("A"));
    let b = w.join_at("rb", "10.0.0.2");
    set_presence(&mut w, &b, presence("B"));
    let ids = |v: Vec<Value>| v.iter().map(|p| p["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let got = who(&mut w, &asker, 200.0);
    assert_eq!(ids(got), vec!["ra", "rb"]);
    w.leave(&a);
    let got = who(&mut w, &asker, 200.0);
    assert_eq!(ids(got), vec!["rb"]);
    w.join_at("rb", "10.0.0.3");
    assert!(who(&mut w, &asker, 200.0).is_empty(), "a new connection of the id has no presence yet");
}

#[test]
fn limits_who_per_player_and_per_address_never_dropped() {
    let mut w = World::new(RelayLimits { who_per_minute: 2.0, who_per_address: 3.0, ..DEFAULT_LIMITS });
    let a = w.join_at("ra", "10.0.0.1");
    let q = json!({ "op": "who", "limit": 10 });
    who(&mut w, &a, 200.0);
    who(&mut w, &a, 200.0);
    for _ in 0..20 {
        assert_eq!(w.refusal(&a, q.clone()), "EOS_LimitExceeded", "the player's two");
    }
    assert_eq!(a.f.dropped(), None, "never dropped");
    // a new id of the address has one left of its three; a /64 of IPv6 is one address
    let b = w.join_at("rb", "10.0.0.1");
    who(&mut w, &b, 200.0);
    let c = w.join_at("rc", "10.0.0.1");
    assert_eq!(w.refusal(&c, q.clone()), "EOS_LimitExceeded", "the address's three");
    let d = w.join_at("rd", "10.0.0.2");
    who(&mut w, &d, 200.0);
    for (i, x) in ["r6a", "r6b", "r6c"].iter().enumerate() {
        let p = w.join_at(x, &format!("2001:db8:1:2::{}", i + 1));
        w.req(&p, json!({ "op": "who", "limit": 1 }));
    }
    let e = w.join_at("r6d", "2001:db8:1:2::4");
    assert_eq!(w.refusal(&e, json!({ "op": "who", "limit": 1 })), "EOS_LimitExceeded", "one /64");
    w.wait(30000.0);
    who(&mut w, &a, 200.0);
    assert_eq!(w.refusal(&a, q), "EOS_LimitExceeded", "one back in half a minute");
}

#[test]
fn invites_to_a_player_whose_presence_takes_none_answered_as_to_no_one() {
    let mut w = World::new(RelayLimits {
        invites_per_minute: 2.0,
        invites_to_player: 100.0,
        invites_per_address: 100.0,
        ..DEFAULT_LIMITS
    });
    let a = w.join("ra");
    let v = w.join_at("rv", "10.0.0.2");
    let inv = |to: &str| json!({ "op": "invite", "to": to, "payload": "x" });
    // no presence yet: invited as before
    w.req(&a, inv("rv"));
    assert_eq!(v.f.take().len(), 1);
    w.wait(60000.0);
    set_presence(&mut w, &v, with(presence("Player"), "invites", json!(false)));
    let closed = w.refusal(&a, inv("rv"));
    let none = w.refusal(&a, inv("rnone"));
    assert_eq!(closed, "EOS_NotFound");
    assert_eq!(closed, none);
    assert_eq!(
        w.refusal(&a, json!({ "op": "invite", "to": "rv", "payload": "x".repeat(501) })),
        none,
        "checked before the payload"
    );
    assert!(v.f.take().is_empty(), "nothing sent");
    // neither took from the sender's rate (two a minute): two more invites go
    let o1 = w.join_at("ro1", "10.0.0.3");
    let o2 = w.join_at("ro2", "10.0.0.4");
    w.req(&a, inv("ro1"));
    w.req(&a, inv("ro2"));
    assert_eq!(o1.f.take().len() + o2.f.take().len(), 2);
    // invites again: taken
    w.wait(60000.0);
    set_presence(&mut w, &v, presence("Player"));
    w.req(&a, inv("rv"));
    assert_eq!(v.f.take().len(), 1);
}

#[test]
fn limits_bytes_per_second_per_channel_packet_size() {
    let mut w = World::new(RelayLimits {
        message_bytes_per_second: 100.0,
        match_bytes_per_second: 100.0,
        packet_bytes: 50.0,
        ..DEFAULT_LIMITS
    });
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    w.together(&[&a, &b, &c]);
    let p40 = [0u8; 40];
    for _ in 0..6 {
        w.packet(&a, 1, "rb", &p40);
    }
    assert_eq!(b.f.npackets(), 5, "two seconds of the rate; channel 1 past it is lost");
    assert_eq!(a.f.dropped(), None);
    w.wait(1000.0);
    w.packet(&a, 1, "rb", &p40);
    assert_eq!(b.f.npackets(), 6, "the rate refills");
    for _ in 0..5 {
        w.packet(&a, 0, "rb", &p40);
    }
    assert_eq!(b.f.npackets(), 11);
    w.packet(&a, 0, "rb", &p40);
    assert_eq!(a.f.dropped().as_deref(), Some("too much data"), "channel 0 past it drops the sender");
    w.packet(&c, 1, "rb", &[0u8; 51]);
    assert_eq!(c.f.dropped().as_deref(), Some("packet too long"));
    assert!(w.logs.borrow().iter().any(|l| l.ends_with("dropped: packet too long")));
}

// ---- the referees of the matches a host core opens

/// An input packet of a slot for `count` ticks from `tick`, each input's last byte `mark`; acknowledging every ruled
/// tick (so that the referee never finds its peer behind); `n` the match's slots.
fn input(n: usize, slot: u8, tick: u32, mark: u8, count: u8) -> Vec<u8> {
    let mut p = vec![0u8; input_header(n) + usize::from(count) * INPUT_BYTES];
    p[0] = MSG_INPUT;
    p[1] = slot;
    p[2..6].copy_from_slice(&tick.to_le_bytes());
    p[6] = count;
    for k in 0..n {
        p[11 + 4 * k..15 + 4 * k].copy_from_slice(&ALL_TICKS.to_le_bytes());
    }
    for k in 0..usize::from(count) {
        p[input_header(n) + (k + 1) * INPUT_BYTES - 1] = mark;
    }
    p
}

/// What a player was sent by the referee `rid`, the ruled packets' inputs together: by (slot, tick) each input's last
/// byte (a substitution -1), and the slots' ends.
#[derive(Default)]
struct FromReferee {
    inputs: BTreeMap<(u8, u32), i32>,
    ends: BTreeMap<u8, u32>,
}

fn from_referee(f: &Fake, rid: &str) -> FromReferee {
    let mut out = FromReferee::default();
    for (channel, from, b) in f.packets() {
        if from != rid || channel != 1 || b[0] != MSG_RULED || !packet_fits(&b, MAX_SLOTS) {
            continue;
        }
        for run in runs(&b) {
            let h = run.head;
            out.ends.insert(h.slot, h.end);
            let mut at = 0;
            for k in 0..u32::from(h.count) {
                if h.mask & (1 << k) != 0 {
                    out.inputs.insert((h.slot, h.first + k), -1);
                } else {
                    out.inputs.insert((h.slot, h.first + k), i32::from(run.inputs[at][INPUT_BYTES - 1]));
                    at += 1;
                }
            }
        }
    }
    out
}

/// A relay that hosts a's custom lobby (ra leads, rb and rc in it), rd connected in none; the checks run its pump.
struct RefWorld {
    w: World,
    a: P,
    b: P,
    c: P,
    d: P,
    lobby: String,
    host: String,
}

fn ref_world(limits: RelayLimits) -> RefWorld {
    let mut w = World::hosting(limits, false);
    let (a, b, c, d) = (w.join("ra"), w.join("rb"), w.join("rc"), w.join("rd"));
    let (lobby, host) = w.hosted_lobby(&[&a, &b, &c]);
    for x in [&a, &b, &c, &d] {
        x.f.take();
    }
    RefWorld { w, a, b, c, d, lobby, host }
}

/// The hosted lobby's match of `ps` to its go, its players' packets so far cleared.
fn open(w: &mut World, host: &str, ps: &[&P]) -> HostedMatch {
    let m = w.run_to_the_go(host, ps);
    for p in ps {
        p.f.clear_packets();
    }
    m
}

/// [`ref_world`]'s match of ra, rb and rc to its go.
fn ref_match() -> (RefWorld, HostedMatch) {
    let mut r = ref_world(DEFAULT_LIMITS);
    let (a, b, c, host) = (r.a.clone(), r.b.clone(), r.c.clone(), r.host.clone());
    let m = open(&mut r.w, &host, &[&a, &b, &c]);
    (r, m)
}

/// The relay's `matches`: at that many live referees its cores start no more (`MatchReferee::room`: a
/// lobby.startMatch refused) until one ends.
#[test]
fn the_relays_match_bound_refuses_a_start_until_a_match_ends() {
    let mut w = World::hosting(RelayLimits { matches: 1.0, ..DEFAULT_LIMITS }, false);
    let (a, b, c, d) = (w.join("ra"), w.join("rb"), w.join("rc"), w.join_at("rd", "10.0.0.2"));
    let (_, h1) = w.hosted_lobby(&[&a, &b]);
    let (_, h2) = w.hosted_lobby(&[&c, &d]);
    let m = open(&mut w, &h1, &[&a, &b]);
    assert_eq!(w.core.matches(), 1);
    w.say(&c, &h2, "lobby.setTeamStyle", json!({ "style": 0 }));
    w.say(&d, &h2, "lobby.setReady", json!({ "on": true }));
    w.later(100.0);
    w.say(&c, &h2, "lobby.startMatch", json!({}));
    let f = c.f.clone();
    w.until("lobby.startFailed", || f.host_has("lobby.startFailed"));
    assert_eq!(w.core.matches(), 1);
    // the match's humans gone: its core ends it, the relay has room again
    w.leave(&a);
    w.leave(&b);
    assert_eq!(w.core.matches(), 0, "{m:?}");
    let m2 = open(&mut w, &h2, &[&c, &d]);
    assert_eq!(w.core.matches(), 1, "{m2:?}");
}

#[test]
fn a_human_out_its_close_peer_its_lobby_leave_a_kick_a_leave() {
    let (RefWorld { mut w, a, b, c, lobby, host, .. }, m) = ref_match();
    let (rid, sb) = (m.rid.clone(), m.slots[1]);
    w.later(100.0);
    // its own closePeer: out, not told back, no silent, the others go on
    w.close_peer(&b, &rid);
    assert!(b.f.take().is_empty());
    b.f.clear_packets();
    w.later(100.0);
    assert_eq!(b.f.npackets(), 0, "sent nothing more");
    assert_ne!(from_referee(&a.f, &rid).ends.get(&sb), Some(&NO_END), "its stream ended for the others");
    assert!(from_referee(&a.f, &rid).ends.contains_key(&sb));
    assert!(a.f.take().is_empty(), "no one told");
    assert_eq!(w.core.matches(), 1);
    w.close_peer(&b, &rid);
    w.close_peer(&b, &format!("{host}.m99"));
    // a lobby leave: out of the core, so out of its match and told
    w.req(&c, json!({ "op": "leave", "lobby": lobby }));
    assert_eq!(c.f.take(), vec![peer_closed(&rid)]);
    c.f.clear_packets();
    w.later(100.0);
    assert_eq!(c.f.npackets(), 0);
    // the last human out ends it
    w.close_peer(&a, &rid);
    assert_eq!(w.core.matches(), 0);
    // a kick by the lobby's owner on the relay
    let (RefWorld { mut w, a, b, lobby, .. }, m) = ref_match();
    w.req(&a, json!({ "op": "kick", "lobby": lobby, "member": "rb" }));
    let closed: Vec<Value> = b.f.take().into_iter().filter(|m| m["t"] == "peerClosed").collect();
    assert_eq!(closed, vec![peer_closed(&m.rid)]);
    // a leave of the relay: out of everything
    let (RefWorld { mut w, a, b, c, lobby, .. }, m) = ref_match();
    w.leave(&c);
    w.later(100.0);
    let c2 = w.join("rc");
    w.later(100.0);
    assert_eq!(c2.f.npackets(), 0, "sent nothing");
    assert_eq!(w.core.matches(), 1, "the others' match goes on");
    // the leader's leave of its lobby: the core ends, its match with it
    w.req(&a, json!({ "op": "leave", "lobby": lobby }));
    assert_eq!(w.core.matches(), 0);
    let closed: Vec<Value> = b.f.take().into_iter().filter(|m| m["t"] == "peerClosed").collect();
    assert_eq!(closed, vec![peer_closed(&m.rid)]);
}

// Deviation from relay/test.ts: there a player's sink calls the core back (the owner leaves in a send to b); here a
// sink never does, and a player lost in a send is the recipient itself (its reader too slow), which is parked (the
// port's own seat rule): the leader lost at the referee's first send keeps its match, and is sent nothing more.
#[test]
fn a_player_lost_at_a_send_is_parked_and_sent_nothing_more() {
    let (RefWorld { mut w, a, b, c, .. }, m) = ref_match();
    let rid = m.rid.clone();
    a.f.0.borrow_mut().lose_from = Some(rid.clone());
    w.packet(&a, 1, &rid, &input(m.n, m.slots[0], 2, 1, 1));
    assert_eq!((w.core.matches(), w.core.parked()), (1, 1));
    assert_eq!((a.f.npackets(), b.f.npackets(), c.f.npackets()), (1, 1, 1));
    w.later(1000.0);
    assert_eq!(a.f.npackets(), 1, "the parked leader is sent nothing more");
    assert!(b.f.npackets() > 1 && c.f.npackets() > 1, "the others go on");
    assert!(b.f.take().is_empty() && c.f.take().is_empty(), "no one told");
    // a human lost at a send: the others go on
    let (RefWorld { w: mut w2, a: a2, b: b2, c: c2, .. }, m) = ref_match();
    let wid = m.rid.clone();
    b2.f.0.borrow_mut().lose_from = Some(wid.clone());
    w2.packet(&a2, 1, &wid, &input(m.n, m.slots[0], 2, 1, 1));
    assert_eq!(w2.core.matches(), 1);
    assert_eq!(b2.f.npackets(), 1);
    assert_eq!(c2.f.npackets(), 1);
    w2.later(100.0);
    assert_eq!(b2.f.npackets(), 1, "the gone one sent nothing more");
}

#[test]
fn a_refereed_match_inputs_in_the_ruled_streams_out_to_every_human() {
    let mut w = World::hosting(DEFAULT_LIMITS, false);
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let (_, host) = w.hosted_lobby(&[&a, &b]);
    let m = open(&mut w, &host, &[&a, &b]);
    let (rid, sa, sb) = (m.rid.clone(), m.slots[0], m.slots[1]);
    assert_eq!(rid, format!("{host}.m{}", m.r#match), "the referee is the relay's, of the host core's match");
    w.packet(&a, 1, &rid, &input(m.n, sa, 2, 11, 3));
    w.packet(&b, 1, &rid, &input(m.n, sb, 2, 22, 2));
    for x in [&a, &b] {
        let r = from_referee(&x.f, &rid);
        assert_eq!([2, 3, 4].map(|t| r.inputs.get(&(sa, t)).copied()), [Some(11); 3], "a's slot ruled at once");
        assert_eq!([2, 3].map(|t| r.inputs.get(&(sb, t)).copied()), [Some(22); 2], "b's slot ruled at once");
        assert!(x.f.packets().iter().all(|p| p.0 == 1 && p.1 == rid), "on channel 1 from the referee");
    }
    assert_eq!(c.f.npackets(), 0, "nothing to a player not in the match");
    // (the slot is the sender's: a packet that names another slot, or one from a player not in the match, is dropped)
    let before = (a.f.npackets(), b.f.npackets());
    w.packet(&b, 1, &rid, &input(m.n, sa, 5, 33, 1));
    w.packet(&c, 1, &rid, &input(m.n, sa, 5, 44, 1));
    assert_eq!((a.f.npackets(), b.f.npackets()), before, "dropped");
    assert!(c.f.take().is_empty(), "a channel 1 one unanswered");
    w.packet(&c, 0, &rid, &[1]);
    assert_eq!(c.f.take(), vec![peer_closed(&rid)], "a channel 0 one answered as to no one");
    w.packet(&b, 0, &rid, &[1]);
    assert_eq!(b.f.take(), vec![peer_closed(&rid)], "channel 0 is no referee's, a human's either");
    // the pumps: substitutions once the deadline passes, the clock packets
    w.later(1000.0);
    let r = from_referee(&a.f, &rid);
    assert_eq!(r.inputs.get(&(sb, 4)), Some(&-1), "a tick past its deadline substituted");
    assert_eq!(r.inputs.get(&(sa, 5)), Some(&-1));
}
#[test]
fn a_cores_leaver_and_end_reach_its_referee() {
    let (RefWorld { mut w, a, b, c, lobby, host, .. }, m) = ref_match();
    let rid = m.rid.clone();
    let (sa, sb, sc) = (m.slots[0], m.slots[1], m.slots[2]);
    w.later(100.0);
    w.packet(&a, 1, &rid, &input(m.n, sa, 2, 1, 6));
    w.packet(&b, 1, &rid, &input(m.n, sb, 2, 2, 6));
    w.packet(&c, 1, &rid, &input(m.n, sc, 2, 3, 6));
    for x in [&a, &b, &c] {
        x.f.clear_packets();
    }
    // the core's leaver (match.leave): its stream ends at the ruled line for the others; it is sent nothing more and
    // told the referee closed
    w.say(&c, &host, "match.leave", json!({}));
    assert_eq!(c.f.take(), vec![peer_closed(&rid)]);
    w.later(1.0);
    assert_eq!(from_referee(&b.f, &rid).ends.get(&sc), Some(&8), "the leaver's end told at the next pump");
    c.f.clear_packets();
    w.packet(&c, 1, &rid, &input(m.n, sc, 8, 9, 1));
    w.later(1000.0);
    assert_eq!(c.f.npackets(), 0, "the leaver is sent nothing");
    assert!(!from_referee(&a.f, &rid).inputs.contains_key(&(sc, 8)), "its packets are dropped");
    // the core's end: its links told it, its referees closed, its lobby destroyed
    for x in [&a, &b, &c] {
        x.f.take();
        x.f.byes();
    }
    let ended = w.core.with_host(&host, |core, now| core.end(HostWhy::Owner { text: "over".into() }, now));
    assert_eq!(ended, Some(()));
    assert_eq!((w.core.matches(), w.core.lobbies(), w.core.hosts().len()), (0, 0, 0));
    let bye = json!({ "code": "host", "why": { "code": "owner", "text": "over" } });
    for x in [&a, &b, &c] {
        assert_eq!(x.f.byes(), vec![(host.clone(), bye.clone())], "{} told the core's end", x.id);
    }
    for x in [&a, &b] {
        assert_eq!(x.f.take(), vec![peer_closed(&rid), closed_msg(&lobby, "destroyed")]);
    }
    assert_eq!(c.f.take(), vec![closed_msg(&lobby, "destroyed")], "the leaver was told before");
    a.f.clear_packets();
    w.packet(&a, 1, &rid, &input(m.n, sa, 8, 1, 1));
    assert_eq!(a.f.npackets(), 0, "a packet to a closed referee goes nowhere");
}

#[test]
fn a_player_out_of_peer_handles_closes_that_link() {
    struct NoRoom(Rc<RefCell<Vec<Value>>>);
    impl CorePlayer for NoRoom {
        fn send(&mut self, m: &ServerMessage) -> Taken {
            self.0.borrow_mut().push(serde_json::to_value(m).unwrap());
            Taken::Yes
        }
        fn packet(&mut self, _: u8, _: &str, _: &[u8]) -> Taken {
            Taken::NoHandle
        }
        fn refuse(&mut self, _: &str) {}
    }
    let mut w = world();
    let a = w.join("ra");
    let got = Rc::new(RefCell::new(Vec::new()));
    let s = w.core.join("rb", "10.0.0.1", Box::new(NoRoom(got.clone())), w.t).unwrap().0;
    let b = P { f: Fake::default(), s, id: "rb".into() };
    w.together(&[&a, &b]);
    got.borrow_mut().clear();
    w.packet(&a, 0, "rb", &[1]);
    assert_eq!(a.f.take(), vec![peer_closed("rb")], "the sender told its link closed");
    assert_eq!(*got.borrow(), vec![peer_closed("ra")], "the player out of handles told too");
}

#[test]
fn addresses_the_per_address_limit_counts_ipv6_by_64() {
    use mvs_relay::limits::address_group;
    assert_eq!(address_group("2001:db8:1:2:3:4:5:6"), address_group("2001:db8:1:2::9"));
    assert_ne!(address_group("2001:db8:1:2::1"), address_group("2001:db8:1:3::1"));
    assert_eq!(address_group("::ffff:192.0.2.7"), "192.0.2.7");
    assert_eq!(address_group("192.0.2.7"), "192.0.2.7");
    assert_eq!(address_group("not an address"), "not an address");
}

/// The cut of b (a's hosted lobby, its match, a link with a, c and the host core) and its seat taken back inside
/// RESUME_MS: kept and resent what it is owed.
#[test]
fn a_cut_and_a_rejoin_inside_the_window_keep_the_seat() {
    let (RefWorld { mut w, a, b, c, d, lobby, .. }, m) = ref_match();
    let rid = m.rid.clone();
    w.packet(&a, 0, "rb", &[1]);
    w.packet(&a, 0, "rb", &[2]);
    w.packet(&b, 0, "ra", &[9]);
    w.core.took_data(&b.s);
    b.f.clear_packets();
    // (b took every frame but a's last before the cut: that was lost on the way)
    let got = b.f.got0() - 1;
    w.cut(&b);
    assert_eq!((w.core.players(), w.core.parked()), (4, 1));
    // parked: nothing sent; channel 0 kept in its ring, channel 1 lost; a link closed and its lobby's change held
    w.packet(&a, 0, "rb", &[3]);
    w.packet(&a, 1, "rb", &[4]);
    w.packet(&c, 0, "rb", &[5]);
    w.close_peer(&c, "rb");
    w.req(&d, json!({ "op": "join", "lobby": lobby }));
    w.later(1000.0);
    assert_eq!(b.f.npackets(), 0, "a parked seat is sent nothing");
    assert!(b.f.take().is_empty());
    let told = a.f.take();
    assert!(told.iter().all(|m| m["t"] == "lobby"), "the others are told no leave: {told:?}");
    w.wait(RESUME_MS - 2000.0);
    w.core.expire(w.t);
    assert_eq!((w.core.players(), w.core.lobbies(), w.core.matches()), (4, 1, 1), "still parked");
    // the ring from the frame b did not take on
    let (b2, took_over, resumed) = w.rejoin("rb", Some(got));
    assert!(took_over);
    assert_eq!(resumed, Some(1), "the frames the relay took from the seat");
    assert_eq!(b.f.dropped(), None, "a parked seat's connection is not told");
    assert_eq!(w.core.parked(), 0);
    assert_eq!(
        b2.f.packets(),
        vec![(0, "ra".into(), vec![2]), (0, "ra".into(), vec![3])],
        "the ring from the hello's count on, but c's frame: its link closed (a resend opens no link)"
    );
    assert_eq!(b2.f.take(), vec![peer_closed("rc"), lobby_msg(&lobby, &["ra", "rb", "rc", "rd"])]);
    // the seat's lobbies, links and matches are the new connection's
    b2.f.clear_packets();
    w.later(100.0);
    assert!(from_referee(&b2.f, &rid).inputs.len() + b2.f.npackets() > 0, "the referee sends to it again");
    b2.f.clear_packets();
    w.packet(&a, 0, "rb", &[6]);
    w.packet(&b2, 0, "ra", &[7]);
    assert_eq!(b2.f.packets(), vec![(0, "ra".into(), vec![6])]);
    assert_eq!(a.f.packets().last().cloned(), Some((0, "rb".into(), vec![7])));
    assert_eq!(w.refusal(&b, json!({ "op": "leave", "lobby": lobby })), "gone", "the old session is no more");
    assert_eq!((w.core.players(), w.core.lobbies(), w.core.matches()), (4, 1, 1));
    // a connected seat taken over: its connection told; a count past what the seat was sent, a new seat
    let (b3, _, resumed) = w.rejoin("rb", Some(got));
    assert_eq!(resumed, Some(1));
    assert_eq!(b2.f.dropped().as_deref(), Some("resumed elsewhere"));
    assert_eq!(b3.f.npackets(), 3, "a's frames from the count on but c's");
    let (_, _, resumed) = w.rejoin("rb", Some(1_000_000));
    assert_eq!(resumed, None, "a count the ring cannot cover");
    assert_eq!(a.f.take().last().cloned(), Some(peer_closed("rb")), "the old seat left");
}

#[test]
fn a_rejoin_after_the_window_or_past_the_ring_is_a_new_seat_a_bye_leaves_at_once() {
    let mut w = world();
    let (a, b) = (w.join("ra"), w.join("rb"));
    let l = w.lobby(&a);
    w.req(&b, json!({ "op": "join", "lobby": l }));
    w.packet(&a, 0, "rb", &[1]);
    a.f.take();
    w.cut(&b);
    w.wait(RESUME_MS - 1.0);
    w.core.expire(w.t);
    assert_eq!(w.core.players(), 2);
    w.wait(1.0);
    w.core.expire(w.t);
    assert_eq!((w.core.players(), w.core.parked()), (1, 0), "gone at RESUME_MS");
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra"]), peer_closed("rb")]);
    let (b2, took_over, resumed) = w.rejoin("rb", Some(1));
    assert_eq!((took_over, resumed), (false, None), "no seat");
    assert!(b2.f.take().is_empty());
    // a gap the ring no longer covers: the old seat leaves, the connection has a new one
    w.req(&b2, json!({ "op": "join", "lobby": l }));
    a.f.take();
    w.cut(&b2);
    for k in 0..=RING_FRAMES {
        w.packet(&a, 0, "rb", &[(k & 255) as u8]);
    }
    let (b3, took_over, resumed) = w.rejoin("rb", Some(0));
    assert_eq!((took_over, resumed), (true, None));
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra"]), peer_closed("rb")]);
    assert_eq!(b3.f.npackets(), 0);
    assert!(b3.f.take().is_empty());
    assert_eq!(w.core.players(), 2);
    // a bye: out of everything at once
    w.req(&b3, json!({ "op": "join", "lobby": l }));
    a.f.take();
    w.leave(&b3);
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra"])]);
    assert_eq!(w.core.players(), 1);
}

/// The leader parked mid-match: its match goes on, the core told its slot away (the others told by the core), and
/// back once its resumed seat sends again.
#[test]
fn a_leader_parked_mid_match_is_away_to_the_core_and_back_at_its_resume() {
    let (RefWorld { mut w, a, b, c, .. }, m) = ref_match();
    let rid = m.rid.clone();
    let (sa, sb, sc) = (m.slots[0], m.slots[1], m.slots[2]);
    w.cut(&a);
    for _ in 0..(SILENT_MS / 500.0) as usize + 4 {
        w.later(500.0);
        w.packet(&b, 1, &rid, &input(m.n, sb, 2, 2, 1));
        w.packet(&c, 1, &rid, &input(m.n, sc, 2, 3, 1));
    }
    assert!(a.f.take().is_empty(), "a parked seat is told nothing");
    assert_eq!(w.core.matches(), 1, "its match goes on");
    let away = |f: &Fake| -> Vec<Value> {
        let mut out = Vec::new();
        while let Some(m) = f.host_took("match.away") {
            out.push(m);
        }
        out
    };
    for f in [&b.f, &c.f] {
        assert_eq!(away(f), vec![json!({ "t": "match.away", "slot": sa, "away": true })], "the core told the others");
    }
    let (a2, _, resumed) = w.rejoin("ra", Some(a.f.got0()));
    assert!(resumed.is_some(), "the seat resumed");
    for _ in 0..5 {
        w.packet(&a2, 1, &rid, &input(m.n, sa, 2, 1, 1));
        w.packet(&b, 1, &rid, &input(m.n, sb, 2, 2, 1));
        w.packet(&c, 1, &rid, &input(m.n, sc, 2, 3, 1));
        w.later(100.0);
    }
    for f in [&b.f, &c.f] {
        assert_eq!(away(f), vec![json!({ "t": "match.away", "slot": sa, "away": false })], "back, told once");
    }
    assert!(!from_referee(&a2.f, &rid).inputs.is_empty(), "the referee sends to the leader again");
}

/// A human silent short of RESUME_MS, then sending again: away and back to the core, never gone.
#[test]
fn a_slot_back_before_resume_ms_stays_in_the_match() {
    let (RefWorld { mut w, a, b, c, .. }, m) = ref_match();
    let rid = m.rid.clone();
    let (sa, sb, sc) = (m.slots[0], m.slots[1], m.slots[2]);
    for _ in 0..((RESUME_MS - 2000.0) / 500.0) as usize {
        w.later(500.0);
        w.packet(&a, 1, &rid, &input(m.n, sa, 2, 1, 1));
        w.packet(&c, 1, &rid, &input(m.n, sc, 2, 3, 1));
    }
    assert!(a.f.host_has("match.away"), "b away to the core, the others told");
    for _ in 0..(RESUME_MS / 500.0) as usize {
        w.later(500.0);
        for (x, s) in [(&a, sa), (&b, sb), (&c, sc)] {
            w.packet(x, 1, &rid, &input(m.n, s, 2, 1, 1));
        }
    }
    assert!(b.f.take().is_empty(), "b is not out of the match");
    assert!(!w.logs.borrow().iter().any(|l| l.contains("match silent")), "never gone: {:?}", w.logs.borrow());
    assert_eq!(w.core.matches(), 1);
}

/// A human silent for RESUME_MS is gone to the core: out of its match (match.left), so out of the referee's, told its
/// referee closed; the others go on.
#[test]
fn a_gone_slot_is_out_of_the_cores_match_and_the_referees() {
    let (RefWorld { mut w, a, b, c, host, .. }, m) = ref_match();
    let rid = m.rid.clone();
    let (sa, sc) = (m.slots[0], m.slots[2]);
    for _ in 0..(RESUME_MS / 500.0) as usize + 4 {
        w.later(500.0);
        w.packet(&a, 1, &rid, &input(m.n, sa, 2, 1, 1));
        w.packet(&c, 1, &rid, &input(m.n, sc, 2, 3, 1));
    }
    assert!(w.logs.borrow().iter().any(|l| *l == format!("{host} match silent Nrb")), "{:?}", w.logs.borrow());
    assert!(b.f.host_has("match.left"), "the core told it out");
    assert_eq!(b.f.take(), vec![peer_closed(&rid)], "out of the referee's");
    b.f.clear_packets();
    w.later(500.0);
    assert_eq!(b.f.npackets(), 0, "sent nothing more");
    assert_eq!(w.core.matches(), 1, "the others go on");
}

/// A seat with nothing to resume leaves at once; parked seats hold their address's places until they expire.
#[test]
fn an_idle_seat_leaves_at_once_and_parked_seats_count_to_their_address() {
    let mut w = world();
    let a = w.join("ra");
    let x = w.join_at("rx", "10.0.0.9");
    w.cut(&x);
    assert_eq!((w.core.players(), w.core.parked()), (1, 0), "in no lobby, link or match: gone");
    let mut xs = Vec::new();
    for k in 0..4 {
        let p = w.join_at(&format!("rx{k}"), "10.0.0.9");
        xs.push(p);
    }
    let l = w.create(&a, "b", 8, None, None);
    for p in &xs {
        w.req(p, json!({ "op": "join", "lobby": l }));
    }
    for p in &xs {
        w.cut(p);
    }
    assert_eq!(w.core.parked(), 4);
    let f = Fake::default();
    let refused = w.core.join("rnew", "10.0.0.9", Box::new(f), w.t).err();
    assert_eq!(refused.as_deref(), Some("too many connections from this address"));
    let (_, took_over, resumed) = w.rejoin_at("rx0", "10.0.0.9", Some(0));
    assert_eq!((took_over, resumed), (true, Some(0)), "a resume is never refused for its address");
    w.wait(RESUME_MS);
    w.core.expire(w.t);
    assert_eq!(w.core.parked(), 0);
    w.join_at("rnew", "10.0.0.9");
}

/// A closing written to a connection already dead is told again at the resume, from the hello's count on; an invite
/// held while parked once only; a link opened again is owed no closing.
#[test]
fn closings_are_told_again_at_a_resume_from_the_hello_s_count() {
    let mut w = world();
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    let l = w.together(&[&a, &b, &c]);
    let l2 = w.together(&[&c, &b]);
    w.packet(&a, 0, "rb", &[1]);
    // b's connection is dead already: a's lobby closes (written, lost), then c's frame
    w.leave(&a);
    w.packet(&c, 0, "rb", &[2]);
    w.cut(&b);
    w.req(&c, json!({ "op": "invite", "to": "rb", "payload": "p" }));
    assert_eq!(w.core.parked(), 1, "in c's lobby: kept");
    let (b2, _, resumed) = w.rejoin("rb", Some(1));
    assert_eq!(resumed, Some(0));
    assert_eq!(b2.f.packets(), vec![(0, "rc".into(), vec![2])]);
    assert_eq!(
        b2.f.take(),
        vec![closed_msg(&l, "destroyed"), peer_closed("ra"), invite_msg("rc", "p"), lobby_msg(&l2, &["rc", "rb"])]
    );
    // read past the closing (frame 1 came after it): not told again, nor the invite
    w.cut(&b2);
    let (b3, _, _) = w.rejoin("rb", Some(2));
    assert_eq!(b3.f.take(), vec![lobby_msg(&l2, &["rc", "rb"])]);
    // a link closed, then open again: owed no closing
    w.close_peer(&c, "rb");
    w.packet(&c, 0, "rb", &[3]);
    w.cut(&b3);
    let (b4, _, _) = w.rejoin("rb", Some(2));
    assert_eq!(b4.f.take(), vec![lobby_msg(&l2, &["rc", "rb"])]);
    assert_eq!(b4.f.packets(), vec![(0, "rc".into(), vec![3])]);
}

/// A drop on the way of a resend parks the seat again, what it is owed kept in its order; a frame no handle is left
/// for in a resend is out of the ring again, as a live one is.
#[test]
fn a_drop_or_no_handle_in_a_resend_keeps_the_count_and_the_order() {
    let mut w = World::new(RelayLimits {
        invites_per_minute: 600.0,
        invites_to_player: 600.0,
        invite_again_ms: 1.0,
        ..DEFAULT_LIMITS
    });
    let (a, b, c) = (w.join("ra"), w.join("rb"), w.join("rc"));
    w.together(&[&a, &b, &c]);
    w.cut(&b);
    for k in 1..=3 {
        w.wait(1.0);
        w.req(&a, json!({ "op": "invite", "to": "rb", "payload": format!("{k}") }));
    }
    let lossy = Fake::default();
    lossy.0.borrow_mut().lose_msgs_at = Some(1);
    let (b2, _) = w.rejoin_with("rb", Some(0), lossy);
    assert_eq!(b2.f.take(), vec![invite_msg("ra", "1")]);
    assert_eq!(w.core.parked(), 1, "parked again");
    let (b3, _, _) = w.rejoin("rb", Some(0));
    let got = b3.f.take();
    assert_eq!(got[..2], [invite_msg("ra", "2"), invite_msg("ra", "3")], "in their order, the first not again");
    // c's frame finds no handle left: out of the ring, c's link closed; a's later frame is frame 2
    w.cut(&b3);
    w.packet(&a, 0, "rb", &[1]);
    w.packet(&c, 0, "rb", &[2]);
    w.packet(&a, 0, "rb", &[3]);
    let full = Fake::default();
    full.0.borrow_mut().no_handle_from = Some("rc".into());
    let (b4, _) = w.rejoin_with("rb", Some(0), full);
    assert_eq!(b4.f.packets(), vec![(0, "ra".into(), vec![1]), (0, "ra".into(), vec![3])]);
    assert!(b4.f.take().contains(&peer_closed("rc")));
    assert_eq!(c.f.take().last().cloned(), Some(peer_closed("rb")));
    w.cut(&b4);
    let (b5, _, resumed) = w.rejoin("rb", Some(2));
    assert_eq!(resumed, Some(0));
    assert!(b5.f.packets().is_empty(), "two frames sent, not three");
}

/// A core that keeps no seats (a direct host's guests): a second connection of an id is a new seat.
#[test]
fn a_core_that_keeps_no_seats_resumes_none() {
    let mut w = world();
    w.core.keep_no_seats();
    let (a, b) = (w.join("ra"), w.join("rb"));
    let l = w.together(&[&a, &b]);
    w.packet(&a, 0, "rb", &[1]);
    let (_, took_over, resumed) = w.rejoin("rb", Some(0));
    assert_eq!((took_over, resumed), (true, None));
    assert_eq!(b.f.dropped().as_deref(), Some("resumed elsewhere"));
    assert_eq!(a.f.take(), vec![lobby_msg(&l, &["ra"]), peer_closed("rb")]);
}

#[test]
fn a_hosted_lobby_has_a_short_code_joined_by_it_past_the_join_charge_and_let_go_with_it() {
    let mut w = World::hosting(RelayLimits { joins_per_minute: 3.0, joins_per_address: 4.0, ..DEFAULT_LIMITS }, false);
    let (a, b, c) = (w.join_at("ra", "10.0.0.1"), w.join_at("rb", "10.0.0.2"), w.join_at("rc", "10.0.0.3"));
    let r = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 4, "host": true }));
    let (lobby, short) = (r["lobby"].as_str().unwrap().to_string(), r["short"].as_str().unwrap().to_string());
    assert!(short.len() == SHORT_LEN && short.bytes().all(|x| SHORT_ALPHABET.contains(&x)), "{short}");
    assert_eq!(w.core.lobby_of_short(&short), Some(lobby.as_str()));
    let plain = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 4 }));
    assert_eq!(plain.get("short"), None, "a lobby with no host core has none");
    // a join by the short code: answered with the lobby's id; by the id: without it
    assert_eq!(
        w.req(&b, json!({ "op": "join", "lobby": short })),
        json!({ "owner": "ra", "members": ["ra", "rb"], "lobby": lobby })
    );
    assert_eq!(w.req(&b, json!({ "op": "join", "lobby": lobby })), json!({ "owner": "ra", "members": ["ra", "rb"] }));
    // every guess is a join of the buckets, an unknown code's too: two unknown and the right one spend the player's
    // three, then the right code is refused as well
    let wrong: Vec<&str> = ["aaaaaa", "bbbbbb", "cccccc"].into_iter().filter(|x| *x != short).take(2).collect();
    for x in &wrong {
        assert_eq!(w.refusal(&c, json!({ "op": "join", "lobby": x })), "EOS_NotFound");
    }
    w.req(&c, json!({ "op": "join", "lobby": short }));
    w.req(&c, json!({ "op": "leave", "lobby": lobby }));
    assert_eq!(w.refusal(&c, json!({ "op": "join", "lobby": short })), "EOS_LimitExceeded");
    // and the address's: a new id of c's address has the rest of it
    let d = w.join_at("rd", "10.0.0.3");
    assert_eq!(w.refusal(&d, json!({ "op": "join", "lobby": wrong[0] })), "EOS_NotFound");
    assert_eq!(w.refusal(&d, json!({ "op": "join", "lobby": short })), "EOS_LimitExceeded");
    // the owner's leave ends the lobby and its code
    w.req(&a, json!({ "op": "leave", "lobby": lobby }));
    assert_eq!(w.core.lobby_of_short(&short), None);
    w.wait(60000.0);
    assert_eq!(w.refusal(&c, json!({ "op": "join", "lobby": short })), "EOS_NotFound");
}

#[test]
fn a_core_that_gives_no_shorts_hosts_with_none() {
    let mut w = World::hosting(DEFAULT_LIMITS, false);
    w.core.give_no_shorts();
    let a = w.join("ra");
    let r = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 4, "host": true }));
    assert!(r["host"].is_string() && r.get("short").is_none(), "{r}");
}

/// Short-code misses are charged to the joiner's /48 (its IPv4 address alone): two /64s of one /48 spend
/// SHORT_MISSES_PER_MINUTE together, then no short code is read for that /48, a right one too; another /48 joins.
#[test]
fn short_code_misses_are_charged_per_48() {
    use mvs_relay::core::SHORT_MISSES_PER_MINUTE;
    let many = RelayLimits { joins_per_minute: 100.0, joins_per_address: 100.0, ..DEFAULT_LIMITS };
    let mut w = World::hosting(many, false);
    let a = w.join_at("ra", "10.0.0.1");
    let r = w.req(&a, json!({ "op": "create", "bucket": "b", "max": 8, "host": true }));
    let short = r["short"].as_str().unwrap().to_string();
    let (b, c) = (w.join_at("rb", "2001:db8:aa:1::5"), w.join_at("rc", "2001:db8:aa:2::5"));
    let wrong: Vec<String> = SHORT_ALPHABET
        .iter()
        .map(|x| String::from_utf8(vec![*x; SHORT_LEN]).unwrap())
        .filter(|x| *x != short)
        .take(SHORT_MISSES_PER_MINUTE as usize)
        .collect();
    for (i, x) in wrong.iter().enumerate() {
        let p = if i % 2 == 0 { &b } else { &c };
        assert_eq!(w.refusal(p, json!({ "op": "join", "lobby": x })), "EOS_NotFound", "miss {i}");
    }
    let d = w.join_at("rd", "2001:db8:aa:3::5");
    assert_eq!(w.refusal(&d, json!({ "op": "join", "lobby": short })), "EOS_LimitExceeded", "the /48 is spent");
    let e = w.join_at("re", "2001:db8:bb:1::5");
    assert_eq!(w.req(&e, json!({ "op": "join", "lobby": short }))["owner"], "ra", "another /48");
}

// ---- a game that referees its own match over the relay

/// What a game's referee sends, by slot.
#[derive(Default)]
struct Sends(Vec<(usize, Vec<u8>)>);

impl RefereeOut for Sends {
    fn send(&mut self, slot: usize, bytes: &[u8]) {
        self.0.push((slot, bytes.to_vec()));
    }
    fn news(&mut self, _: usize, _: SlotNews) {}
}

/// A human of a game-refereed match: what it has of each stream (the first tick it lacks, the ticks ahead of it), and
/// its input packets sent.
struct Human {
    slot: usize,
    have: Vec<u32>,
    early: Vec<BTreeMap<u32, ()>>,
    seq: u8,
}

impl Human {
    fn take(&mut self, b: &[u8]) {
        for run in runs(b) {
            let s = usize::from(run.head.slot);
            for k in 0..u32::from(run.head.count) {
                if run.head.first + k >= self.have[s] {
                    self.early[s].insert(run.head.first + k, ());
                }
            }
            while self.early[s].remove(&self.have[s]).is_some() {
                self.have[s] += 1;
            }
        }
    }

    /// Its input packet for `tick`, acknowledging what it has.
    fn packet(&mut self, n: usize, tick: i64) -> Vec<u8> {
        let mut acks = [0u32; MAX_SLOTS];
        acks[..n].copy_from_slice(&self.have);
        let first = u32::try_from(tick.max(0)).unwrap() + INPUT_DELAY;
        #[allow(clippy::cast_possible_truncation, reason = "a test's slots")]
        let h = InputHeader { slot: self.slot as u8, first, count: 1, stamp: 0, acks, seq: self.seq };
        self.seq = self.seq.wrapping_add(1);
        input_packet(&h, n, &[[self.slot as u8 + 1; INPUT_BYTES]])
    }
}

/// A game refereeing an eight-slot match over the relay (as the dev link's host does) sends each of the seven others
/// the ruled streams of all: with one packet in twenty lost each way, its sends past the losses it repairs stay within
/// the relay's match bytes limit (the limit's rate for each player it sends), and none is dropped there.
#[test]
fn a_game_refereeing_eight_slots_over_the_relay_is_not_cut_by_its_bytes_limit() {
    let n = MAX_SLOTS;
    let mut w = World::new(DEFAULT_LIMITS);
    let ps: Vec<P> = (0..n).map(|i| w.join_at(&format!("r{i}"), &format!("10.0.0.{}", i + 1))).collect();
    w.together(&ps.iter().collect::<Vec<_>>());
    let mut r = Referee::new(n, &(0..n).collect::<Vec<_>>(), w.t);
    let mut hs: Vec<Human> = (0..n)
        .map(|slot| Human { slot, have: vec![INPUT_DELAY; n], early: vec![BTreeMap::new(); n], seq: 0 })
        .collect();
    // (a lost packet: one in twenty, by a fixed sequence)
    let mut seed: u64 = 7;
    let mut lost = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33).is_multiple_of(20)
    };
    let (mut sent, mut got, mut bytes) = (0usize, 0usize, 0usize);
    let ticks: i32 = 900;
    for _ in 0..ticks {
        w.wait(TICK_MS);
        let now = w.t;
        let mut out = Sends::default();
        // (the referee game's own input, and what came to it through the relay, each pumped as it comes)
        let own = hs[0].packet(n, r.tick(now));
        r.receive(0, &own, now, &mut out);
        r.pump(now, &mut out);
        for (_, from, b) in ps[0].f.packets() {
            let slot: usize = from[1..].parse().unwrap();
            if !lost() {
                r.receive(slot, &b, now, &mut out);
                r.pump(now, &mut out);
            }
        }
        ps[0].f.clear_packets();
        r.pump(now, &mut out);
        for (slot, b) in out.0 {
            if slot == 0 {
                hs[0].take(&b);
                continue;
            }
            sent += 1;
            bytes += b.len();
            w.packet(&ps[0], 1, &ps[slot].id, &b);
        }
        for slot in 1..n {
            for (_, _, b) in ps[slot].f.packets() {
                if !lost() {
                    hs[slot].take(&b);
                }
            }
            got += ps[slot].f.npackets();
            ps[slot].f.clear_packets();
            let p = hs[slot].packet(n, r.tick(now));
            w.packet(&ps[slot], 1, "r0", &p);
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "a test's counts")]
    let rate = bytes as f64 / (f64::from(ticks) * TICK_MS / 1000.0);
    eprintln!("the referee game sent {sent} packets, {rate:.0} bytes a second; {got} came through");
    assert_eq!(got, sent, "packets the relay dropped over the match bytes limit");
    assert!(rate > DEFAULT_LIMITS.match_bytes_per_second, "more than one player's limit, {rate:.0} B/s");
    let line = r.tick(w.t) - 30;
    assert!(hs.iter().all(|h| h.have.iter().all(|&t| i64::from(t) >= line)), "every stream ruled through");
}
