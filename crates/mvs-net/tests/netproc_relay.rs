//! The net service over a relay (its relay half: relayChecks) on mvs-relay at 127.0.0.1: two
//! players with a service each, signed in to the relay; the custom lobby the relay hosts (its core on the relay, the
//! player who made it a member) joined by its code, the lobby to the match's go, the core's referee ruling the match
//! (its packets `from` -1 to both pages), a guest that falls silent told the other away and taken out once gone,
//! and the match's end, the sessions going on. Real sockets and the wall clock; every check is the page's side (the
//! the oracle's checks of the relay's own lists, matchRequests, have no reader here).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use mvs_net::backend::BackendKind;
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{ClientOptions, NetFaults, RelayClient, StatsReader};
use mvs_net::service::api::{NetEvent, NetRequest, SessionTarget};
use mvs_net::service::page::PageId;
use mvs_net::service::{BROWSE_AGAIN_MS, LIST_GAP_MS, NetService, ServiceConfig, ServiceOptions};
use mvs_protocol::match_wire::{INPUT_BYTES, INPUT_HEADER, MAX_SLOTS, MSG_INPUT, RESUME_MS, SILENT_MS};
use mvs_protocol::relay_wire::is_host_id;
use mvs_protocol::version::PROTOCOL;
use mvs_relay::server::{Options, start_relay};

/// A relay on 127.0.0.1 on a thread of its own (it runs to the test's end): its address.
fn relay() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async move {
            let r = start_relay(Options { host: "127.0.0.1".into(), port: 0, ..Options::default() }).expect("a relay");
            let _ = tx.send(r.port);
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }));
    });
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("the relay's port");
    format!("127.0.0.1:{port}")
}

/// One player: its service, its page, the page's events, the messages taken, its relay client's stats.
struct P {
    svc: NetService,
    stats: StatsReader,
    page: PageId,
    me: String,
    name: String,
    events: Vec<NetEvent>,
    taken: Vec<bool>,
}

impl P {
    fn new(address: &str, name: &str, epoch: Instant, faults: NetFaults) -> P {
        let now = ms(epoch);
        let mut svc = NetService::new(ServiceOptions { epoch, ..ServiceOptions::default() }, now);
        svc.configure(ServiceConfig { relay: Some(address.to_string()), ..ServiceConfig::default() }, now);
        let mut o = ClientOptions::new("mvs-test 0", epoch);
        o.faults = faults;
        let c = RelayClient::connect(address, &RelayKey::fresh(), &o).expect("signed in to the relay");
        let me = mvs_net::backend::Backend::me(&c).to_string();
        let stats = c.stats_reader();
        svc.start_backend(BackendKind::Relay, Box::new(c), now);
        let page = svc.open_page(now);
        P { svc, stats, page, me, name: name.to_string(), events: Vec::new(), taken: Vec::new() }
    }
    fn post(&mut self, m: NetRequest, epoch: Instant) {
        self.svc.take(self.page, m, ms(epoch));
    }
    fn say(&mut self, t: &str, body: Value, epoch: Instant) {
        //
        let mut m = body;
        if t == "hello" {
            m["proto"] = json!(PROTOCOL);
            m["app"] = json!("test");
            m["name"] = json!(self.name);
        }
        if t == "lobby.enter" || t == "lobby.join" {
            m["compat"] = json!("ctest.1");
        }
        m["t"] = json!(t);
        self.post(NetRequest::Msg(m.to_string()), epoch);
    }
    fn msgs(&self) -> Vec<(usize, Value)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                NetEvent::Msg(t) => serde_json::from_str(t).ok().map(|v| (i, v)),
                _ => None,
            })
            .collect()
    }
    fn has(&self, t: &str, test: impl Fn(&Value) -> bool) -> bool {
        self.msgs().iter().any(|(i, m)| !self.taken.get(*i).copied().unwrap_or(false) && m["t"] == t && test(m))
    }
    fn next(&mut self, t: &str) -> Value {
        let (i, m) = self
            .msgs()
            .into_iter()
            .find(|(i, m)| !self.taken.get(*i).copied().unwrap_or(false) && m["t"] == t)
            .unwrap_or_else(|| panic!("no {t} message"));
        if self.taken.len() <= i {
            self.taken.resize(i + 1, false);
        }
        self.taken[i] = true;
        m
    }
    fn packets(&self) -> Vec<i32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                NetEvent::Packets { from, .. } => Some(*from),
                _ => None,
            })
            .collect()
    }
    fn closed(&self) -> bool {
        self.events.iter().any(|e| matches!(e, NetEvent::Closed(_)))
    }
}

fn ms(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1000.0
}

/// Pumps both players for `ms` of wall time.
fn pump(ps: &mut [P], epoch: Instant, for_ms: f64) {
    let end = ms(epoch) + for_ms;
    while ms(epoch) < end {
        step(ps, epoch);
    }
}

fn step(ps: &mut [P], epoch: Instant) {
    for p in ps.iter_mut() {
        let now = ms(epoch);
        p.svc.pump(now);
        let page = p.page;
        p.events.extend(p.svc.events(page));
    }
    std::thread::sleep(Duration::from_millis(1));
}

/// Pumps until `cond` (at most 5 s).
fn until(ps: &mut [P], epoch: Instant, what: &str, cond: impl Fn(&[P]) -> bool) {
    let end = ms(epoch) + 5000.0;
    while !cond(ps) {
        assert!(ms(epoch) < end, "in time: {what}: {:?} / {:?}", ps[0].msgs(), ps[1].msgs());
        step(ps, epoch);
    }
}

/// A loadout as a page sends it with its lock.
fn loadout(i: usize) -> Value {
    let badges: Vec<Value> = (0..3)
        .map(|k| json!({ "slug": format!("st_{i}_{k}"), "asset": format!("ST_{i}_{k}"), "count": 100 + k }))
        .collect();
    json!({
        "taunts": [format!("Taunt_A{i}"), format!("Taunt_B{i}"), format!("Emote_{i}"), ""],
        "ringOut": format!("ROV_{i}"),
        "banner": { "slug": format!("banner-{i}"), "asset": format!("BD_{i}") },
        "badges": badges,
        "icon": { "slug": format!("icon-{i}"), "asset": format!("PI_{i}") },
    })
}

/// An input packet of `slot` at `tick` with one input whose last byte is `mark`, acknowledging every ruled tick
///.
fn input(slot: u8, tick: u32, mark: u8) -> Vec<u8> {
    let mut p = vec![0u8; INPUT_HEADER + INPUT_BYTES];
    p[0] = MSG_INPUT;
    p[1] = slot;
    p[2..6].copy_from_slice(&tick.to_le_bytes());
    for k in 0..MAX_SLOTS {
        p[11 + 4 * k..15 + 4 * k].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    }
    p[6] = 1;
    let last = p.len() - 1;
    p[last] = mark;
    p
}

/// The host's custom lobby to its match's go, each page given the match: the host's
/// slot, the guest's, the guest's id and the go's referee.
fn to_the_go(ps: &mut [P], epoch: Instant) -> (u8, u8, String, String) {
    step(ps, epoch);
    assert!(
        matches!(ps[0].events.first(), Some(NetEvent::Status { online: true, me, .. }) if *me == ps[0].me),
        "status: a page is told the sign-in at once: {:?}",
        ps[0].events.first()
    );

    // the lobby: the host's, the guest in it by its code, ready
    ps[0].post(NetRequest::Open(SessionTarget::Host), epoch);
    until(ps, epoch, "the host's server message", |ps| ps[0].has("server", |_| true));
    let code = ps[0]
        .events
        .iter()
        .find_map(|e| match e {
            NetEvent::Opened { code, .. } => Some(code.clone()),
            _ => None,
        })
        .expect("opened");
    ps[0].say("hello", json!({}), epoch);
    until(ps, epoch, "welcome", |ps| ps[0].has("welcome", |_| true));
    ps[0].next("welcome");
    ps[0].say("lobby.enter", json!({}), epoch);
    until(ps, epoch, "the host's lobby.state", |ps| ps[0].has("lobby.state", |_| true));
    ps[1].post(NetRequest::Open(SessionTarget::Join(code.clone())), epoch);
    until(ps, epoch, "the guest's server message", |ps| ps[1].has("server", |_| true));
    ps[1].say("hello", json!({}), epoch);
    until(ps, epoch, "the guest's welcome", |ps| ps[1].has("welcome", |_| true));
    ps[1].next("welcome");
    ps[1].say("lobby.join", json!({ "lobby": code }), epoch);
    until(ps, epoch, "the guest in the lobby", |ps| ps[1].has("lobby.state", |m| m["ev"] == "started"));
    let g = ps[1].me.clone();
    // (the core on the relay tells the maker on a link of its own: it may come after the guest's)
    until(ps, epoch, "join: the guest is in the core's lobby, under its relay id", |ps| {
        ps[0].msgs().iter().any(|(_, m)| m["t"] == "lobby.state" && m["data"].to_string().contains(&g))
    });
    ps[0].say("lobby.setTeamStyle", json!({ "style": 0 }), epoch);
    ps[0].say("lobby.setMaps", json!({ "maps": ["map_a"] }), epoch);
    ps[1].say("lobby.setReady", json!({ "on": true }), epoch);
    pump(ps, epoch, 100.0);

    // an invite: the other player's page is told the code and who sent it
    ps[0].post(NetRequest::Invite { to: g.clone(), name: "Velma".into() }, epoch);
    let host = ps[0].me.clone();
    until(ps, epoch, "the invite", |ps| {
        ps[1].events.iter().any(|e| {
            matches!(e, NetEvent::Invited { code: c, from_id, from_name, .. }
                if *c == code && *from_id == host && from_name == "Velma")
        })
    });

    // the match to its go
    ps[0].say("lobby.startMatch", json!({}), epoch);
    until(ps, epoch, "pm.begin", |ps| ps.iter().all(|p| p.has("pm.begin", |_| true)));
    for (i, p) in ps.iter_mut().enumerate() {
        p.next("pm.begin");
        p.say("pm.lock", json!({ "character": "shaggy", "skin": "", "loadout": loadout(i) }), epoch);
    }
    until(ps, epoch, "pm.all", |ps| ps.iter().all(|p| p.has("pm.all", |_| true)));
    for p in ps.iter_mut() {
        p.say("pm.perks", json!({ "perks": [] }), epoch);
    }
    until(ps, epoch, "match.start", |ps| ps.iter().all(|p| p.has("match.start", |_| true)));
    let starts: Vec<Value> = ps.iter_mut().map(|p| p.next("match.start")).collect();
    for p in ps.iter_mut() {
        p.say("match.loaded", json!({}), epoch);
    }
    until(ps, epoch, "match.go", |ps| ps.iter().all(|p| p.has("match.go", |_| true)));
    let gos: Vec<Value> = ps.iter_mut().map(|p| p.next("match.go")).collect();
    let slot = |i: usize| u8::try_from(starts[i]["slot"].as_u64().expect("a slot")).expect("a slot");
    let (sh, sg) = (slot(0), slot(1));
    let referee = gos[0]["referee"].as_str().expect("a referee").to_string();
    let of_core = referee.rsplit_once(".m").is_some_and(|(h, n)| is_host_id(h) && n.parse::<u32>().is_ok());
    assert!(
        gos.iter().all(|x| x["referee"] == referee.as_str()) && of_core,
        "relay referee: the go names the referee of the lobby's core on the relay, not a player: {gos:?}"
    );
    for (i, p) in ps.iter_mut().enumerate() {
        let peers =
            gos[i]["peers"].as_array().expect("peers").iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        let m = NetRequest::Match { peers, slot: u32::from(slot(i)), referee: referee.clone() };
        p.post(m, epoch);
    }
    assert!(
        gos.iter()
            .all(|x| x["peers"][usize::from(sh)] == ps[0].me.as_str() && x["peers"][usize::from(sg)] == g.as_str()),
        "match: the go names both players by slot"
    );
    (sh, sg, g, referee)
}

#[test]
fn the_relay_referees_the_hosts_match() {
    let address = relay();
    let epoch = Instant::now();
    let mut ps = vec![
        P::new(&address, "Host", epoch, NetFaults::default()),
        P::new(&address, "Guest", epoch, NetFaults::default()),
    ];
    let (sh, sg, _, _) = to_the_go(&mut ps, epoch);

    // both pages' packets go to the relay's referee, its packets to both pages, `from` -1
    ps[0].post(NetRequest::Send(input(sh, 2, 7)), epoch);
    ps[1].post(NetRequest::Send(input(sg, 2, 9)), epoch);
    pump(&mut ps, epoch, 100.0);
    assert!(!ps[0].packets().is_empty() && !ps[1].packets().is_empty(), "relay referee: both pages get its packets");
    assert!(ps.iter().all(|p| p.packets().iter().all(|f| *f == -1)), "relay referee: its packets come `from` -1");

    // the guest falls silent: away for the core's referee, which the core tells the others (the lobby's maker); gone
    // at RESUME_MS, and the core takes it out
    let rounds = ((RESUME_MS + 1000.0) / 100.0) as u32;
    let away_at = ((SILENT_MS + 1000.0) / 100.0) as u32;
    let away = |m: &Value| m["slot"] == u64::from(sg) && m["away"] == true;
    for t in 0..rounds {
        ps[0].post(NetRequest::Send(input(sh, 3 + t * 6, 2)), epoch);
        pump(&mut ps, epoch, 100.0);
        if t + 1 == away_at {
            assert!(ps[0].has("match.away", away), "relay referee: a guest silent at the relay is told the host away");
            assert!(!ps[1].has("match.left", |_| true), "relay referee: an away guest is not taken out");
        }
    }
    let left = |m: &Value| m["slot"] == u64::from(sg) && m["why"] == "disconnected";
    assert!(
        ps[1].has("match.left", left) && ps[0].has("match.left", left),
        "relay referee: a guest silent at the relay is told it is out, the host that it left"
    );

    // the match's end: the sessions go on
    for p in ps.iter_mut() {
        p.post(NetRequest::MatchEnd, epoch);
        p.say("match.leave", json!({}), epoch);
    }
    pump(&mut ps, epoch, 300.0);
    assert!(!ps[0].closed() && !ps[1].closed(), "relay referee: the sessions go on after the match's end");

    // the host leaves: the guest's session is over
    ps[0].post(NetRequest::Close, epoch);
    until(&mut ps, epoch, "the guest's session over", |ps| ps[1].closed());
}

/// The guest's link cut for `cut_s` half a second after its go, both sending their inputs every 100 ms: the match
/// goes on through it (no one taken out, no session closed), the guest's pages get the referee's packets again after
/// it, and the guest's client came back on a second connection where the cut passed RESUME_TRY_MS. The match is the
/// service's (no route given to the client): the cut and the stall are found from its channel 1.
fn through_a_cut(cut_s: f64, resumes: u64) {
    let address = relay();
    let epoch = Instant::now();
    let faults = NetFaults { cut_s, cut_at_s: 0.5, ..NetFaults::default() };
    let mut ps = vec![P::new(&address, "Host", epoch, NetFaults::default()), P::new(&address, "Guest", epoch, faults)];
    let (sh, sg, _, _) = to_the_go(&mut ps, epoch);
    let go = ms(epoch);
    let (mut t, mut in_cut) = (0u32, None);
    while ms(epoch) < go + (cut_s + 2.0) * 1000.0 {
        ps[0].post(NetRequest::Send(input(sh, 2 + t * 6, 2)), epoch);
        ps[1].post(NetRequest::Send(input(sg, 2 + t * 6, 3)), epoch);
        pump(&mut ps, epoch, 100.0);
        t += 1;
        if in_cut.is_none() && ms(epoch) > go + (0.5 + cut_s) * 1000.0 - 200.0 {
            in_cut = Some(ps[1].packets().len());
        }
    }
    let in_cut = in_cut.expect("a count in the cut");
    let s = ps[1].stats.get();
    assert_eq!((s.cuts, s.resumes), (1, resumes), "the cut fired, and the resumes: {s:?}");
    assert!(ps[1].packets().len() > in_cut + 5, "the guest's pages ruled again after the cut");
    assert!(
        !ps.iter().any(|p| p.has("match.left", |_| true) || p.closed()),
        "no one taken out, no session closed by the cut"
    );
}

#[test]
fn a_short_cut_in_the_relays_match() {
    through_a_cut(1.5, 0);
}

#[test]
fn a_cut_past_resume_try_in_the_relays_match() {
    through_a_cut(4.0, 1);
}

/// The browse's lobbies with `code`, once its answer came (`id` the browse's).
fn browsed(p: &mut P, id: u32, code: &str, epoch: Instant) -> usize {
    p.post(NetRequest::Browse { id }, epoch);
    let end = ms(epoch) + 5000.0;
    loop {
        let found = p.events.iter().find_map(|e| match e {
            NetEvent::Browsed { id: i, lobbies, .. } if *i == id => {
                Some(lobbies.iter().filter(|l| l.code == code).count())
            }
            _ => None,
        });
        if let Some(n) = found {
            return n;
        }
        assert!(ms(epoch) < end, "in time: browse {id}");
        step(std::slice::from_mut(p), epoch);
    }
}

#[test]
fn a_public_lobby_on_the_relay_is_browsed_and_a_private_one_is_not() {
    let address = relay();
    let epoch = Instant::now();
    let mut ps = vec![
        P::new(&address, "Host", epoch, NetFaults::default()),
        P::new(&address, "Guest", epoch, NetFaults::default()),
    ];
    step(&mut ps, epoch);
    ps[0].post(NetRequest::Open(SessionTarget::Host), epoch);
    until(&mut ps, epoch, "the host's server message", |ps| ps[0].has("server", |_| true));
    let code = ps[0]
        .events
        .iter()
        .find_map(|e| match e {
            NetEvent::Opened { code, .. } => Some(code.clone()),
            _ => None,
        })
        .expect("opened");
    ps[0].say("hello", json!({}), epoch);
    until(&mut ps, epoch, "welcome", |ps| ps[0].has("welcome", |_| true));
    ps[0].say("lobby.enter", json!({}), epoch);
    until(&mut ps, epoch, "the host's lobby.state", |ps| ps[0].has("lobby.state", |_| true));
    pump(&mut ps, epoch, 300.0);
    assert_eq!(browsed(&mut ps[1], 1, &code, epoch), 0, "a private lobby is not browsed");

    // the leader sets it public: the core's listing goes to the relay's list, the other service finds it (each
    // browse past BROWSE_AGAIN_MS, which answers a browse from the last search)
    ps[0].say("lobby.setOption", json!({ "key": "public", "value": true }), epoch);
    pump(&mut ps, epoch, BROWSE_AGAIN_MS + 200.0);
    assert_eq!(browsed(&mut ps[1], 2, &code, epoch), 1, "a public lobby is browsed by its code");

    // private again: unlisted once the listing's gap is over
    ps[0].say("lobby.setOption", json!({ "key": "public", "value": false }), epoch);
    pump(&mut ps, epoch, BROWSE_AGAIN_MS.max(LIST_GAP_MS) + 200.0);
    assert_eq!(browsed(&mut ps[1], 3, &code, epoch), 0, "a lobby made private is gone from the browse");
}

/// A lobby set public is on the relay's list well within LIST_GAP_MS (the gap holds back only the updates after the
/// first): a player of its own searches the relay every 100 ms from the leader's setOption. Measured at about 150 ms
/// on 127.0.0.1 (the first search found nothing, the second found it); a browse then answers in a few ms.
#[test]
fn a_lobby_set_public_is_on_the_relay_at_once() {
    use mvs_net::backend::{Answer, Backend, BackendEvent};
    use mvs_net::service::CUSTOM_BUCKET;
    let address = relay();
    let epoch = Instant::now();
    let mut ps = vec![P::new(&address, "Host", epoch, NetFaults::default())];
    let mut probe =
        RelayClient::connect(&address, &RelayKey::fresh(), &ClientOptions::new("mvs-test 0", epoch)).expect("a probe");
    step(&mut ps, epoch);
    ps[0].post(NetRequest::Open(SessionTarget::Host), epoch);
    until_one(&mut ps, epoch, "the host's server message", |p| p.has("server", |_| true));
    ps[0].say("hello", json!({}), epoch);
    until_one(&mut ps, epoch, "welcome", |p| p.has("welcome", |_| true));
    ps[0].say("lobby.enter", json!({}), epoch);
    until_one(&mut ps, epoch, "the host's lobby.state", |p| p.has("lobby.state", |_| true));
    pump(&mut ps, epoch, 300.0);

    let at = ms(epoch);
    ps[0].say("lobby.setOption", json!({ "key": "public", "value": true }), epoch);
    let mut next = at;
    loop {
        let waited = ms(epoch) - at;
        assert!(waited < LIST_GAP_MS / 2.0, "the lobby set public is not on the relay after {waited} ms");
        step(&mut ps, epoch);
        if ms(epoch) < next {
            continue;
        }
        next += 100.0;
        let ask = probe.search_lobbies(CUSTOM_BUCKET, 100, true);
        let found = loop {
            step(&mut ps, epoch);
            match Backend::poll(&mut probe, ms(epoch)) {
                Some(BackendEvent::Answered(a, Ok(Answer::Found(x)))) if a == ask => break x.len(),
                Some(BackendEvent::Answered(a, r)) if a == ask => panic!("the probe's search: {r:?}"),
                _ => assert!(ms(epoch) - at < LIST_GAP_MS, "the probe's search is answered"),
            }
        };
        if found == 1 {
            break;
        }
    }
}

/// Pumps until `cond` holds of the first player (at most 5 s).
fn until_one(ps: &mut [P], epoch: Instant, what: &str, cond: impl Fn(&P) -> bool) {
    let end = ms(epoch) + 5000.0;
    while !cond(&ps[0]) {
        assert!(ms(epoch) < end, "in time: {what}: {:?}", ps[0].msgs());
        step(ps, epoch);
    }
}
