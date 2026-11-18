//! Direct play over real sockets on 127.0.0.1 (joinChecks and hostChecks, and the
//! code and the local player's round trips): a host with its own player on its core, joiners by its code, the guest
//! rules of its listener, the players' limit. The checks of that file that run through the net service are the
//! service's; the one that makes the host's core fail its first join (a claimed address that then fails on the network)
//! needs a core of the test's own and is not ported. Last, a LAN custom lobby through the net service: the lobby's
//! core on the direct host's relay, its maker and a guest by its code both members of it.

use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mvs_net::backend::{Answer, Backend, BackendEvent, Channel, LobbyChange};
use mvs_net::lan::direct::{
    DirectHost, DirectHostOptions, DirectJoinOptions, DirectJoinWhy, DirectLink, begin_join, join_direct,
    open_direct_host,
};
use mvs_net::lan::direct_code::{DirectCode, DirectWhy, Local, decode_direct_code, encode_direct_code};
use mvs_net::link::relay::key::{RelayKey, key_fingerprint};
use mvs_net::link::relay::{ClientOptions, RelayClient, RelayEvent, StartWith};
use mvs_net::service::api::{DirectOptions, LanOptions, NetEvent, NetRequest, SessionTarget};
use mvs_net::service::page::PageId;
use mvs_net::service::{NetService, ServiceOptions};
use mvs_protocol::relay_wire::Request;
use mvs_protocol::version::PROTOCOL;
use serde_json::{Value, json};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("mvs-direct-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).expect("a folder");
        Dir(d)
    }

    fn key(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("an address").port()
}

fn host_options(dir: &Dir, name: &str, port: u16) -> DirectHostOptions {
    DirectHostOptions {
        key_file: dir.key(name),
        max: 4,
        join_within_ms: None,
        port,
        loopback: true,
        log: None,
        interfaces: None,
    }
}

fn client_options(hello_ms: Option<f64>) -> ClientOptions {
    let mut o = ClientOptions::new("test test", Instant::now());
    if let Some(h) = hello_ms {
        o.times.hello_ms = h;
    }
    o
}

fn join_options(dir: &Dir, name: &str, hello_ms: Option<f64>, loopback: bool) -> DirectJoinOptions {
    DirectJoinOptions { key_file: dir.key(name), loopback, client: client_options(hello_ms), cancel: None }
}

fn code(addresses: &[&str], port: u16, fingerprint: &str, lobby: &str) -> String {
    let c = DirectCode {
        addresses: addresses.iter().map(|a| (*a).to_owned()).collect(),
        port,
        fingerprint: fingerprint.to_owned(),
        lobby: lobby.to_owned(),
    };
    encode_direct_code(&c, true).expect("a code")
}

/// Polls a backend up to `ms` for an event `f` picks; the events before it are dropped.
fn wait<T>(b: &mut dyn Backend, ms: u64, mut f: impl FnMut(BackendEvent) -> Option<T>) -> Option<T> {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        while let Some(e) = b.poll(0.0) {
            if let Some(t) = f(e) {
                return Some(t);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn answer(b: &mut dyn Backend, ask: mvs_net::backend::Ask) -> Result<Answer, String> {
    wait(b, 5000, |e| match e {
        BackendEvent::Answered(a, r) if a == ask => Some(r),
        _ => None,
    })
    .expect("an answer in time")
}

fn create_lobby(host: &mut DirectHost) -> String {
    let b = host.backend.as_mut().expect("the host's player");
    let ask = b.create_lobby("custom", 4, None);
    match answer(b, ask) {
        Ok(Answer::Created(l)) => l,
        other => panic!("create: {other:?}"),
    }
}

/// Whether the client was signed out within `ms`.
fn signed_out(c: &RelayClient, ms: u64) -> bool {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        while let Some(e) = c.poll() {
            if matches!(e, RelayEvent::SignedOut(_)) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn stranger(addr: &str, fp: &str, key: &RelayKey) -> Result<RelayClient, String> {
    let x = StartWith { host_key: Some(fp.to_owned()), ..StartWith::default() };
    RelayClient::connect_with(addr, key, &client_options(Some(2000.0)), &x).map_err(|e| e.to_string())
}

#[test]
fn port_in_use_goes_to_the_next() {
    let dir = Dir::new("port");
    let port = free_port();
    let _holder = TcpListener::bind(("127.0.0.1", port)).expect("held");
    let mut host = open_direct_host(host_options(&dir, "host2", port)).expect("listening");
    assert!(host.port > port, "the held port is skipped");
    let lobby = create_lobby(&mut host);
    let read = decode_direct_code(&host.code(&lobby).expect("a code"), &Local::new(true)).expect("a code");
    assert_eq!(read.port, host.port);
    assert_eq!(read.lobby, lobby);
    host.close();
}

#[test]
fn joins_race_and_refuse() {
    let dir = Dir::new("join");
    let port = free_port();
    let mut host = open_direct_host(host_options(&dir, "host3", port)).expect("listening");
    let lobby = create_lobby(&mut host);
    let fp = key_fingerprint(RelayKey::load(&dir.key("host3")).expect("a key").raw());
    let host_id = host.me.clone();

    // a dead address (no host answers there) and the live one: the live one wins at once
    let dead = code(&["10.255.255.1", "127.0.0.1"], port, &fp, &lobby);
    let t0 = Instant::now();
    let mut l1: DirectLink = join_direct(&dead, join_options(&dir, "j1", Some(4000.0), true)).expect("joined");
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!((l1.host.as_str(), l1.lobby.as_str()), (host_id.as_str(), lobby.as_str()));
    // a joiner's link has no online list
    assert!(l1.client.presence().is_none());
    l1.client.shutdown();

    // a refused address (nothing on 127.0.0.2) and the live one
    let refused = code(&["127.0.0.2", "127.0.0.1"], port, &fp, &lobby);
    let l2 = join_direct(&refused, join_options(&dir, "j2", None, true)).expect("joined");
    assert_eq!(l2.host, host_id);
    l2.client.shutdown();

    // the same host on both addresses: one sign-in, which stays (the second try is closed before it signs in)
    let twice = code(&["127.0.0.1", "127.0.0.1"], port, &fp, &lobby);
    let mut l3 = join_direct(&twice, join_options(&dir, "j3", None, true)).expect("joined");
    let ask = Backend::join_lobby(&mut l3.client, &lobby);
    assert!(matches!(answer(&mut l3.client, ask), Ok(Answer::Joined { .. })));
    std::thread::sleep(Duration::from_millis(300));
    assert!(wait(&mut l3.client, 100, |e| matches!(e, BackendEvent::SignedOut(_)).then_some(())).is_none());

    // only dead addresses: each one's reason, within the deadline
    let none = code(&["10.255.255.1", "127.0.0.2"], port, &fp, &lobby);
    let w = join_direct(&none, join_options(&dir, "j4", Some(800.0), true)).err().expect("refused");
    let DirectJoinWhy::Unreachable(each) = &w else { panic!("{w:?}") };
    assert!(
        each.len() == 2 && each[0].0 == format!("10.255.255.1:{port}") && each[1].0 == format!("127.0.0.2:{port}"),
        "{w:?}"
    );
    assert!(w.log_text().contains("a network error"));

    // a host whose key is not the code's
    let other = key_fingerprint(RelayKey::fresh().raw());
    let wrong = code(&["127.0.0.1"], port, &other, &lobby);
    let w = join_direct(&wrong, join_options(&dir, "j5", None, true)).err().expect("refused");
    assert_eq!(w, DirectJoinWhy::WrongHost);

    // a mistyped code; a loopback code without the test flag
    let mut mistyped = twice.clone();
    mistyped.pop();
    mistyped.push(if twice.ends_with('a') { 'b' } else { 'a' });
    let w6 = join_direct(&mistyped, join_options(&dir, "j6", None, true)).err().expect("refused");
    let w7 = join_direct(&twice, join_options(&dir, "j7", None, false)).err().expect("refused");
    assert_eq!(w6, DirectJoinWhy::Code(DirectWhy::Mistyped));
    assert_eq!(w7, DirectJoinWhy::Code(DirectWhy::Outside));

    // a stranger on the listener (the host's key proved): it joins only by the lobby id; it makes and finds none
    let addr = format!("127.0.0.1:{port}");
    let s = stranger(&addr, &fp, &RelayKey::fresh()).expect("signed in");
    let create = s.request(Request::Create { bucket: "custom".to_owned(), max: 4, listed: None, host: false });
    let search = s.request(Request::Search { bucket: "custom".to_owned(), limit: 10, full: None });
    let guess = s.join_lobby(&format!("l{}", "0".repeat(20)));
    assert!(create.is_err() && search.is_err() && guess.is_err(), "{create:?} {search:?} {guess:?}");
    s.shutdown();

    // a connection under the host's own id is refused (the host's own player stays)
    let own = stranger(&addr, &fp, &RelayKey::load(&dir.key("host3")).expect("the host's key"));
    assert!(own.is_err());
    std::thread::sleep(Duration::from_millis(100));
    let b = host.backend.as_mut().expect("player");
    assert!(wait(b, 100, |e| matches!(e, BackendEvent::SignedOut(_)).then_some(())).is_none());

    l3.client.shutdown();
    host.close();
}

#[test]
fn host_rules_and_limits() {
    let dir = Dir::new("host");
    // a player in no lobby is dropped after the join's time; one in the lobby stays
    let port = free_port();
    let mut o = host_options(&dir, "host5", port);
    o.join_within_ms = Some(400.0);
    let mut host = open_direct_host(o).expect("listening");
    let lobby = create_lobby(&mut host);
    let fp = key_fingerprint(RelayKey::load(&dir.key("host5")).expect("a key").raw());
    let addr = format!("127.0.0.1:{port}");
    let idle = stranger(&addr, &fp, &RelayKey::fresh()).expect("signed in");
    let member = stranger(&addr, &fp, &RelayKey::fresh()).expect("signed in");
    member.join_lobby(&lobby).expect("joined");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(signed_out(&idle, 100), "a player in no lobby is dropped");
    assert!(!signed_out(&member, 100), "a member stays");
    // left: dropped in its turn
    member.leave_lobby(&lobby).expect("left");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(signed_out(&member, 100), "a member that left is dropped after the same time");

    // the players' limit: twice the lobby's players, the host's own one of them
    let many: Vec<Result<RelayClient, String>> = (0..9).map(|_| stranger(&addr, &fp, &RelayKey::fresh())).collect();
    let ok = many.iter().filter(|r| r.is_ok()).count();
    let eighth = many[7].as_ref().err().cloned().unwrap_or_default();
    assert!(
        ok == 7 && eighth.contains("full") && many[8].is_err(),
        "{ok} {:?}",
        many.iter().map(|r| r.as_ref().err()).collect::<Vec<_>>()
    );
    drop(many);
    host.close();
}

/// The host's own player and a joiner through a lobby: the joiner's join is told to the host, packets go both ways
/// once each side accepts the other, and a kick ends the joiner's membership.
#[test]
fn host_and_guest_talk() {
    let dir = Dir::new("talk");
    let port = free_port();
    let mut host = open_direct_host(host_options(&dir, "host7", port)).expect("listening");
    let lobby = create_lobby(&mut host);
    let fp = key_fingerprint(RelayKey::load(&dir.key("host7")).expect("a key").raw());
    let mut g =
        join_direct(&code(&["127.0.0.1"], port, &fp, &lobby), join_options(&dir, "g7", None, true)).expect("joined");
    let ask = Backend::join_lobby(&mut g.client, &lobby);
    let Ok(Answer::Joined { owner, members, .. }) = answer(&mut g.client, ask) else { panic!("join") };
    assert_eq!(owner, host.me);
    assert!(members.contains(&host.me));
    let guest_id = Backend::me(&g.client).to_owned();
    let hb = host.backend.as_mut().expect("player");
    let told = wait(hb, 3000, |e| match e {
        BackendEvent::Lobby(LobbyChange::Members { members, .. }) if members.contains(&guest_id) => Some(members),
        _ => None,
    });
    assert!(told.is_some(), "the host's player is told of the join");
    // the host sends first: the guest takes it (a member of its lobby), the host accepts the guest's answer
    hb.set_accept([guest_id.clone()].into_iter().collect());
    assert!(hb.send(&guest_id, Channel::Messages, b"hello guest"));
    let got = wait(&mut g.client, 3000, |e| match e {
        BackendEvent::Packet { from, bytes, .. } => Some((from, bytes)),
        _ => None,
    });
    assert_eq!(got, Some((host.me.clone(), b"hello guest".to_vec())));
    assert!(Backend::send(&mut g.client, &host.me, Channel::Messages, b"hello host"));
    let hb = host.backend.as_mut().expect("player");
    let got = wait(hb, 3000, |e| match e {
        BackendEvent::Packet { from, bytes, .. } => Some((from, bytes)),
        _ => None,
    });
    assert_eq!(got, Some((guest_id.clone(), b"hello host".to_vec())));
    assert_eq!(hb.relayed(&guest_id), Some(false));
    // a kick
    hb.kick(&lobby, &guest_id);
    let closed = wait(&mut g.client, 3000, |e| match e {
        BackendEvent::Lobby(LobbyChange::Closed { lobby: l, .. }) => Some(l),
        _ => None,
    });
    assert_eq!(closed.as_deref(), Some(lobby.as_str()));
    g.client.shutdown();
    host.close();
}

/// A host closed on purpose ends its own player without a sign-out; a call after it is refused, not left waiting.
#[test]
fn a_closed_host_is_quiet_and_refuses_calls() {
    let dir = Dir::new("closed");
    let port = free_port();
    let mut host = open_direct_host(host_options(&dir, "host8", port)).expect("listening");
    let mut b = host.backend.take().expect("player");
    let ask = b.create_lobby("custom", 4, None);
    assert!(matches!(answer(&mut b, ask), Ok(Answer::Created(_))));
    host.close();
    std::thread::sleep(Duration::from_millis(300));
    let ask = b.search_lobbies("custom", 5, false);
    let mut out: Vec<BackendEvent> = Vec::new();
    while let Some(e) = b.poll(0.0) {
        out.push(e);
    }
    assert!(out.iter().all(|e| !matches!(e, BackendEvent::SignedOut(_))), "{out:?}");
    assert!(
        out.iter().any(|e| matches!(e, BackendEvent::Answered(a, Err(w)) if *a == ask && w.contains("has closed"))),
        "{out:?}"
    );
}

/// A join in the air ends at once when it is cancelled, and the polled result says so.
#[test]
fn a_join_in_the_air_is_cancelled() {
    let dir = Dir::new("cancel");
    let c = code(&["10.255.255.1"], 7778, "0123456789abcdef", &format!("l{}", "0".repeat(20)));
    let t0 = Instant::now();
    let mut p = begin_join(c, join_options(&dir, "jc", Some(8000.0), true));
    assert!(p.poll().is_none(), "the call returns at once");
    std::thread::sleep(Duration::from_millis(150));
    p.cancel();
    let until = Instant::now() + Duration::from_secs(2);
    let mut got = None;
    while got.is_none() && Instant::now() < until {
        got = p.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(got.expect("a result").err(), Some(DirectJoinWhy::Cancelled));
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
}

/// A player on LAN through its net service: the service, its page, the page's events.
struct LanPlayer {
    svc: NetService,
    page: PageId,
    events: Vec<NetEvent>,
}

impl LanPlayer {
    fn new(dir: &Dir, name: &str, port: u16, epoch: Instant) -> LanPlayer {
        let now = since(epoch);
        let mut svc = NetService::new(ServiceOptions { epoch, ..ServiceOptions::default() }, now);
        let direct =
            DirectOptions { key_file: dir.key(name), product: "test".into(), version: "test".into(), loopback: true };
        // (the launcher's direct options, which a join by a direct code reads)
        svc.set_direct(Some(direct.clone()));
        svc.start_lan(LanOptions { direct, port }, now);
        let page = svc.open_page(now);
        LanPlayer { svc, page, events: Vec::new() }
    }

    fn post(&mut self, m: NetRequest, epoch: Instant) {
        self.svc.take(self.page, m, since(epoch));
    }

    fn say(&mut self, m: Value, epoch: Instant) {
        self.post(NetRequest::Msg(m.to_string()), epoch);
    }

    fn msgs(&self, t: &str) -> Vec<Value> {
        self.events
            .iter()
            .filter_map(|e| match e {
                NetEvent::Msg(x) => serde_json::from_str::<Value>(x).ok().filter(|m| m["t"] == t),
                _ => None,
            })
            .collect()
    }
}

fn since(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1000.0
}

/// Pumps every player until `cond` (at most 5 s); whether it came.
fn lan_until(ps: &mut [LanPlayer], epoch: Instant, cond: impl Fn(&[LanPlayer]) -> bool) -> bool {
    let end = since(epoch) + 5000.0;
    while !cond(ps) {
        if since(epoch) > end {
            return false;
        }
        for p in ps.iter_mut() {
            p.svc.pump(since(epoch));
            let page = p.page;
            p.events.extend(p.svc.events(page));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

#[test]
fn a_lan_custom_lobby_is_hosted_by_the_direct_hosts_relay() {
    let dir = Dir::new("lan-custom");
    let epoch = Instant::now();
    let mut ps = vec![
        LanPlayer::new(&dir, "host.key", free_port(), epoch),
        LanPlayer::new(&dir, "guest.key", free_port(), epoch),
    ];
    let hello = |name: &str| json!({ "t": "hello", "proto": PROTOCOL, "app": "test", "name": name });

    // the maker: the direct host started, the lobby's core on it, the maker its member
    ps[0].post(NetRequest::Open(SessionTarget::Host), epoch);
    assert!(lan_until(&mut ps, epoch, |ps| !ps[0].msgs("server").is_empty()), "the maker's server: {:?}", ps[0].events);
    let code = ps[0]
        .events
        .iter()
        .find_map(|e| match e {
            NetEvent::Opened { code, .. } => Some(code.clone()),
            _ => None,
        })
        .expect("opened");
    ps[0].say(hello("Velma"), epoch);
    assert!(lan_until(&mut ps, epoch, |ps| !ps[0].msgs("welcome").is_empty()), "the maker's welcome");
    ps[0].say(json!({ "t": "lobby.enter", "compat": "ctest.1" }), epoch);
    assert!(lan_until(&mut ps, epoch, |ps| !ps[0].msgs("lobby.state").is_empty()), "the maker's lobby.state");

    // the guest by the code: a member of the same core
    ps[1].post(NetRequest::Open(SessionTarget::Join(code.clone())), epoch);
    assert!(lan_until(&mut ps, epoch, |ps| !ps[1].msgs("server").is_empty()), "the guest's server: {:?}", ps[1].events);
    ps[1].say(hello("Fred"), epoch);
    assert!(lan_until(&mut ps, epoch, |ps| !ps[1].msgs("welcome").is_empty()), "the guest's welcome");
    // (the page's join names the code, as a pasted code; the service tells the core its lobby id)
    ps[1].say(json!({ "t": "lobby.join", "lobby": code, "compat": "ctest.1" }), epoch);
    let joined = lan_until(&mut ps, epoch, |ps| ps[1].msgs("lobby.state").iter().any(|m| m["ev"] == "started"));
    assert!(joined, "the guest in the lobby: {:?}", ps[1].events);
    let g = ps[1]
        .events
        .iter()
        .find_map(|e| match e {
            NetEvent::Opened { me, .. } => Some(me.clone()),
            _ => None,
        })
        .expect("the guest opened");
    let both =
        lan_until(&mut ps, epoch, |ps| ps[0].msgs("lobby.state").iter().any(|m| m["data"].to_string().contains(&g)));
    assert!(both, "the maker told of the guest: {:?}", ps[0].msgs("lobby.state").last());

    // the maker leaves: its direct host ends, the guest's session with it
    ps[0].post(NetRequest::Close, epoch);
    let over = lan_until(&mut ps, epoch, |ps| ps[1].events.iter().any(|e| matches!(e, NetEvent::Closed(_))));
    assert!(over, "the guest's session over");
}
