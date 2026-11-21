//! The relay client against a running relay (the TS relay, `node relay/main.ts`, or `mvs-relay`) on this PC: sign-in,
//! a lobby made and joined, channel 0 both ways, a match refereed by the host's net thread, over UDP and over TCP
//! alone. The relay's address comes from `MVS_TEST_RELAY` (`127.0.0.1:<port>`); without it the test
//! says so and passes. The drop, the resume and the faults, the round trip's timing and a match of the relay's queue
//! core refereed on the relay run on an `mvs-relay` in this process.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use mvs_net::link::MatchLink;
use mvs_net::link::relay::P2P_MAX_PACKET;
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{CHANNEL_MESSAGES, ClientOptions, NetFaults, RelayClient, RelayEvent, RelayMatchLink};
use mvs_protocol::match_wire::{InputHeader, MAX_SLOTS, MSG_RULED, input_packet};
use mvs_protocol::p2p_frame::{FRAME_OPEN, MessageJoiner, Taken, signal, split_message};
use mvs_protocol::relay_wire::QUEUE_HOST;
use mvs_protocol::version::PROTOCOL;
use mvs_relay::server::{Options, start_relay};
use serde_json::{Value, json};

fn relay() -> Option<String> {
    let a = std::env::var("MVS_TEST_RELAY").ok();
    if a.is_none() {
        eprintln!("relay_client: no MVS_TEST_RELAY: nothing run");
    }
    a
}

fn client(addr: &str, epoch: Instant, udp_blocked: bool) -> RelayClient {
    let mut o = ClientOptions::new("mvs-test 0", epoch);
    o.udp_blocked = udp_blocked;
    RelayClient::connect(addr, &RelayKey::fresh(), &o).expect("signed in")
}

/// Waits up to 3 s for an event that `f` picks.
fn wait_for<T>(c: &RelayClient, mut f: impl FnMut(RelayEvent) -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        while let Some(e) = c.poll() {
            if let Some(t) = f(e) {
                return t;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no such event in time");
}

/// An input packet of `slot` with no inputs: the header alone (its clock and acknowledgements).
fn header(slot: u8, now: f64) -> Vec<u8> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a test's ms")]
    let stamp = now as u32;
    input_packet(&InputHeader { slot, first: 0, count: 0, stamp, acks: [0; MAX_SLOTS] }, &[])
}

/// Both sides' links get ruled packets for 600 ms while they send their headers.
fn ruled_both(epoch: Instant, a: &mut dyn MatchLink, b: &mut dyn MatchLink) {
    let (mut got_a, mut got_b) = (0, 0);
    let until = Instant::now() + Duration::from_millis(600);
    while Instant::now() < until {
        let now = epoch.elapsed().as_secs_f64() * 1000.0;
        a.send(&header(0, now), now);
        b.send(&header(1, now), now);
        while let Some(p) = a.recv(now) {
            assert_eq!(p[0], MSG_RULED);
            got_a += 1;
        }
        while let Some(p) = b.recv(now) {
            assert_eq!(p[0], MSG_RULED);
            got_b += 1;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(got_a > 3 && got_b > 3, "ruled packets: {got_a} and {got_b}");
}

fn run(addr: &str, b_udp_blocked: bool) {
    let epoch = Instant::now();
    let a = client(addr, epoch, false);
    let b = client(addr, epoch, b_udp_blocked);
    let lobby = a.create_lobby("mvs-test", 2).expect("created");
    let joined = b.join_lobby(&lobby).expect("joined");
    assert_eq!(joined.owner, a.me());
    wait_for(&a, |e| matches!(e, RelayEvent::Lobby { members: Some(m), .. } if m.len() == 2).then_some(()));
    b.send(a.me(), CHANNEL_MESSAGES, b"hi");
    let from = wait_for(&a, |e| match e {
        RelayEvent::Packet { from, channel: 0, bytes } if bytes == b"hi" => Some(from),
        _ => None,
    });
    assert_eq!(from, b.me());
    a.send(b.me(), CHANNEL_MESSAGES, b"there");
    wait_for(&b, |e| matches!(e, RelayEvent::Packet { bytes, .. } if bytes == b"there").then_some(()));
    // (a match the relay referees is a relay-hosted core's since wire 5)
    // a match the host's net thread referees
    let now = epoch.elapsed().as_secs_f64() * 1000.0;
    let mut la = a.host_match(vec![(0, a.me().to_owned()), (1, b.me().to_owned())], now + 50.0);
    let mut lb = b.join_match(a.me());
    ruled_both(epoch, &mut la, &mut lb);
    a.end_match();
    b.end_match();
    b.leave_lobby(&lobby).expect("left");
    a.shutdown();
    b.shutdown();
}

#[test]
fn a_lobby_and_its_matches_over_udp() {
    let Some(addr) = relay() else { return };
    run(&addr, false);
}

#[test]
fn a_lobby_and_its_matches_over_tcp_alone() {
    let Some(addr) = relay() else { return };
    run(&addr, true);
}

#[test]
fn a_bad_address_and_no_relay_are_said() {
    let epoch = Instant::now();
    let o = ClientOptions::new("mvs-test 0", epoch);
    let e = RelayClient::connect("not an address:x", &RelayKey::fresh(), &o).expect_err("refused");
    assert!(e.to_string().contains("not a relay address"), "{e}");
    let e = RelayClient::connect("127.0.0.1:1", &RelayKey::fresh(), &o).expect_err("nothing there");
    assert!(e.to_string().contains("nothing answers") || e.to_string().contains("did not answer"), "{e}");
}

/// The launcher's look at an address: the relay's wire version and UDP reach, and a failure for no relay.
#[test]
fn a_probe_finds_the_relay_and_its_udp() {
    use mvs_net::link::relay::{RelayProbe, probe_relay};
    let o = ClientOptions::new("mvs-test 0", Instant::now());
    assert!(matches!(probe_relay("127.0.0.1:1", &o), RelayProbe::Failed(_)));
    let Some(addr) = relay() else { return };
    match probe_relay(&addr, &o) {
        RelayProbe::Found { version, relay, udp, .. } => {
            assert_eq!(version, mvs_protocol::relay_wire::RELAY_WIRE_VERSION);
            assert!(!relay.is_empty() && udp, "{relay} {udp}");
        }
        RelayProbe::Failed(why) => panic!("not found: {why}"),
    }
}

/// Presence, the online list and an invite through the `Backend` calls (the ticket's presence pacing included).
#[test]
fn presence_who_and_invite() {
    use mvs_net::backend::{Answer, Ask, Backend, BackendEvent, RelayPresence};
    use mvs_protocol::relay_wire::PresenceStatus;
    fn ask(c: &mut RelayClient, a: Ask) -> Result<Answer, String> {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            while let Some(e) = Backend::poll(c, 0.0) {
                if let BackendEvent::Answered(t, r) = e
                    && t == a
                {
                    return r;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("no answer in time");
    }
    let Some(addr) = relay() else { return };
    let epoch = Instant::now();
    let mut a = client(&addr, epoch, false);
    let b = client(&addr, epoch, false);
    let mut c = client(&addr, epoch, false);
    let t = c.presence().expect("a relay has presence").set(RelayPresence {
        name: "Pat".to_owned(),
        icon: "ProfileIcon_Test".to_owned(),
        busy: false,
        listed: true,
        invites: true,
        status: Some(PresenceStatus::Lobby),
    });
    assert!(matches!(ask(&mut c, t), Ok(Answer::Done)));
    let t = a.presence().expect("presence").who(50);
    let Ok(Answer::Who(list)) = ask(&mut a, t) else { panic!("who") };
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(
        (list[0].id.as_str(), list[0].name.as_str(), list[0].icon.as_str(), list[0].busy, list[0].status),
        (c.me(), "Pat", "ProfileIcon_Test", false, Some(PresenceStatus::Lobby))
    );
    // an invite reaches the one invited
    let t = a.invite(b.me(), "come");
    assert!(matches!(ask(&mut a, t), Ok(Answer::Done)));
    let got = wait_for(&b, |e| match e {
        RelayEvent::Invite { from, payload } => Some((from, payload)),
        _ => None,
    });
    assert_eq!(got, (a.me().to_owned(), "come".to_owned()));
    a.shutdown();
    b.shutdown();
    c.shutdown();
}

// ---- the drop, the resume and the faults (an mvs-relay in this process)

/// A relay on 127.0.0.1 on a thread of its own (it runs to the test's end): its address.
fn local_relay() -> String {
    local_relay_with(mvs_relay::limits::RelayLimits::default())
}

/// [`local_relay`] under the limits `limits`.
fn local_relay_with(limits: mvs_relay::limits::RelayLimits) -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async move {
            let o = Options { host: "127.0.0.1".into(), port: 0, limits, ..Options::default() };
            let r = start_relay(o).expect("a relay");
            let _ = tx.send(r.port);
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }));
    });
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("the relay's port");
    format!("127.0.0.1:{port}")
}

/// A relay on 127.0.0.1 port `port` (0: any) with a key of its own made for this run, on a thread of its own: its port,
/// its log lines, and a sender that stops it (its sockets closed with no word to anyone, as a crashed relay's).
fn stoppable_relay(port: u16) -> (u16, mpsc::Receiver<String>, mpsc::Sender<()>) {
    let (tx, rx) = mpsc::channel();
    let (log_tx, log_rx) = mpsc::channel::<String>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async move {
            let log: mvs_relay::server::LogLine = Box::new(move |l| {
                let _ = log_tx.send(l.to_owned());
            });
            let o = Options { host: "127.0.0.1".into(), port, log: Some(log), ..Options::default() };
            let r = start_relay(o).expect("a relay");
            let _ = tx.send(r.port);
            while stop_rx.try_recv() == Err(mpsc::TryRecvError::Empty) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            drop(r);
        }));
        drop(local);
    });
    let port = rx.recv_timeout(Duration::from_secs(10)).expect("the relay's port");
    (port, log_rx, stop_tx)
}

fn faulty(addr: &str, epoch: Instant, faults: NetFaults) -> RelayClient {
    let mut o = ClientOptions::new("mvs-test 0", epoch);
    o.faults = faults;
    RelayClient::connect(addr, &RelayKey::fresh(), &o).expect("signed in")
}

fn ms(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1000.0
}

/// Two players in a lobby of the first's.
fn lobby_of_two(a: &RelayClient, b: &RelayClient) {
    let lobby = a.create_lobby("mvs-test", 2).expect("created");
    b.join_lobby(&lobby).expect("joined");
    wait_for(a, |e| matches!(e, RelayEvent::Lobby { members: Some(m), .. } if m.len() == 2).then_some(()));
}

/// A match for `a` (slot 0) and `b` (slot 1), both links sending their headers every 16 ms for `for_ms`: the times
/// (ms of `epoch`) `a`'s link took a ruled packet, and how many `b`'s took. The events of either client go to `seen`.
/// Its referee is `b`'s net thread, so `a`'s packets cross the relay both ways (O2 prep: it was the relay's, by
/// match.open, which wire 5 drops; a later wire may put a relay-hosted core's referee back).
fn relay_match(
    epoch: Instant,
    a: &RelayClient,
    b: &RelayClient,
    for_ms: f64,
    seen: &mut Vec<RelayEvent>,
) -> (Vec<f64>, usize) {
    let humans = vec![(0, a.me().to_owned()), (1, b.me().to_owned())];
    let mut lb: RelayMatchLink = b.host_match(humans, ms(epoch) + 50.0);
    let mut la = a.join_match(b.me());
    let (mut at_a, mut got_b) = (Vec::new(), 0);
    let end = ms(epoch) + for_ms;
    while ms(epoch) < end {
        let now = ms(epoch);
        la.send(&header(0, now), now);
        lb.send(&header(1, now), now);
        while let Some(p) = la.recv(now) {
            assert_eq!(p[0], MSG_RULED);
            at_a.push(ms(epoch));
        }
        while lb.recv(now).is_some() {
            got_b += 1;
        }
        seen.extend(std::iter::from_fn(|| a.poll()));
        seen.extend(std::iter::from_fn(|| b.poll()));
        std::thread::sleep(Duration::from_millis(16));
    }
    a.end_match();
    b.end_match();
    (at_a, got_b)
}

fn signed_out(seen: &[RelayEvent]) -> Option<&str> {
    seen.iter().find_map(|e| match e {
        RelayEvent::SignedOut(why) => Some(why.as_str()),
        _ => None,
    })
}

fn has_bytes(seen: &[RelayEvent], b: &[u8]) -> usize {
    seen.iter().filter(|e| matches!(e, RelayEvent::Packet { bytes, .. } if bytes == b)).count()
}

/// A dropped socket starts a second connection at once: on a relay that kept the seat it is resumed, on one that did
/// not the client signs out with that reason, in either case well before the keepalive's 20 s.
/// The welcome names the game protocol of the relay's cores: the client and the launcher's look both read it.
#[test]
fn the_welcome_names_the_relays_game_protocol() {
    use mvs_net::link::relay::{RelayProbe, probe_relay};
    let addr = local_relay();
    let epoch = Instant::now();
    let c = client(&addr, epoch, false);
    assert_eq!(c.proto(), Some(PROTOCOL));
    c.shutdown();
    match probe_relay(&addr, &ClientOptions::new("mvs-test 0", epoch)) {
        RelayProbe::Found { proto, .. } => assert_eq!(proto, Some(PROTOCOL)),
        RelayProbe::Failed(why) => panic!("not found: {why}"),
    }
}

#[test]
fn a_dropped_socket_tries_a_second_connection_at_once() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults::default());
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    a.cut_socket();
    let until = Instant::now() + Duration::from_secs(3);
    let mut out = None;
    while Instant::now() < until && a.stats().resumes == 0 && out.is_none() {
        while let Some(e) = a.poll() {
            if let RelayEvent::SignedOut(why) = e {
                out = Some(why);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    match out {
        Some(why) => assert!(why.contains("did not keep"), "signed out for another reason: {why}"),
        None => assert_eq!(a.stats().resumes, 1, "neither resumed nor signed out in 3 s"),
    }
}

/// A client that signed in with no pin is pinned for its resume to the key its relay proved: when another key answers
/// at that address after a drop, the second connection stops at its challenge (it never signs in there) and the
/// client stays down, its seat not taken by a stranger relay's "did not keep" welcome.
#[test]
fn a_resume_is_pinned_to_the_key_the_start_proved() {
    let (port, _log_a, stop_a) = stoppable_relay(0);
    let addr = format!("127.0.0.1:{port}");
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults::default());
    let first = a.relay_id();
    stop_a.send(()).expect("the first relay");
    // (the same port: the first relay's sockets are closed once its thread drops them)
    let until = Instant::now() + Duration::from_secs(5);
    let (log_b, _stop_b) = loop {
        let bound = std::panic::catch_unwind(|| stoppable_relay(port));
        match bound {
            Ok((_, log, stop)) => break (log, stop),
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => panic!("the port was not free again"),
        }
    };
    let mut seen = Vec::new();
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        seen.extend(std::iter::from_fn(|| a.poll()));
        std::thread::sleep(Duration::from_millis(10));
    }
    let lines: Vec<String> = log_b.try_iter().collect();
    assert!(lines.iter().any(|l| l.contains("127.0.0.1")), "the client tried the second relay: {lines:?}");
    assert!(!lines.iter().any(|l| l.ends_with(" in")), "signed in to another key: {lines:?}");
    assert_eq!(signed_out(&seen), None, "a stranger's welcome ended it");
    assert_eq!(a.stats().resumes, 0);
    assert_eq!(a.relay_id(), first);
}

/// A socket dropped in a lobby: the second connection takes the seat back, and what either side sent on channel 0
/// while it was down arrives, once.
#[test]
fn a_dropped_socket_takes_its_seat_back() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults::default());
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    b.send(a.me(), CHANNEL_MESSAGES, b"one");
    wait_for(&a, |e| matches!(e, RelayEvent::Packet { bytes, .. } if bytes == b"one").then_some(()));
    a.cut_socket();
    b.send(a.me(), CHANNEL_MESSAGES, b"two");
    a.send(b.me(), CHANNEL_MESSAGES, b"three");
    let mut at_a = Vec::new();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until && has_bytes(&at_a, b"two") == 0 {
        at_a.extend(std::iter::from_fn(|| a.poll()));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(signed_out(&at_a), None);
    assert_eq!(a.stats().resumes, 1);
    wait_for(&b, |e| matches!(e, RelayEvent::Packet { bytes, .. } if bytes == b"three").then_some(()));
    // (once: nothing more comes)
    std::thread::sleep(Duration::from_millis(300));
    at_a.extend(std::iter::from_fn(|| a.poll()));
    assert_eq!(has_bytes(&at_a, b"two"), 1, "{at_a:?}");
    assert!(!at_a.iter().any(|e| matches!(e, RelayEvent::Lobby { closed: Some(_), .. })), "the lobby stays: {at_a:?}");
    a.shutdown();
    b.shutdown();
}

/// A link that stands still in a match (a 3 s cut): at RESUME_TRY_MS without the referee a second connection starts,
/// goes once the cut is over, and takes the seat over; the referee's packets come again.
#[test]
fn a_second_connection_takes_over_a_stalled_first() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults { cut_s: 3.0, cut_at_s: 0.5, ..NetFaults::default() });
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let go = ms(epoch);
    let mut seen = Vec::new();
    let (at_a, got_b) = relay_match(epoch, &a, &b, 6000.0, &mut seen);
    assert_eq!(signed_out(&seen), None, "{seen:?}");
    assert_eq!(a.stats().resumes, 1);
    assert!(got_b > 100, "b ruled throughout: {got_b}");
    let after = at_a.iter().filter(|t| **t > go + 4000.0).count();
    assert!(after > 20, "a ruled again after the cut: {after}");
}

/// A cut shorter than RESUME_TRY_MS holds the match's packets and lets them go again, with no second connection.
#[test]
fn a_short_cut_holds_the_link_and_lets_it_go() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults { cut_s: 1.0, cut_at_s: 0.5, ..NetFaults::default() });
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let go = ms(epoch);
    let mut seen = Vec::new();
    let (at_a, _) = relay_match(epoch, &a, &b, 2500.0, &mut seen);
    assert_eq!(signed_out(&seen), None, "{seen:?}");
    assert_eq!(a.stats().resumes, 0);
    // (the go is a little before the link's own: a margin at either end)
    let held = at_a.iter().filter(|t| **t > go + 600.0 && **t < go + 1450.0).count();
    let after = at_a.iter().filter(|t| **t > go + 1600.0).count();
    assert_eq!(held, 0, "nothing in the cut: {at_a:?}");
    assert!(after > 10, "ruled packets after the cut: {after}");
}

/// Datagrams all lost: UDP never answers, so channel 1 goes on TCP alone and the match is ruled as ever.
#[test]
fn all_datagrams_lost_leaves_tcp() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults { loss: 1.0, ..NetFaults::default() });
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let mut seen = Vec::new();
    let (at_a, got_b) = relay_match(epoch, &a, &b, 1500.0, &mut seen);
    let s = a.stats();
    assert_eq!((s.udp, s.both), (0, 0), "{s:?}");
    assert!(s.tcp_match > 20 && at_a.len() > 20 && got_b > 20, "{s:?} {} {got_b}", at_a.len());
}

/// A delay holds channel 0 that long on the way in.
#[test]
fn a_delay_holds_channel_0() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults { delay_ms: 150.0, ..NetFaults::default() });
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let sent = Instant::now();
    b.send(a.me(), CHANNEL_MESSAGES, b"late");
    wait_for(&a, |e| matches!(e, RelayEvent::Packet { bytes, .. } if bytes == b"late").then_some(()));
    let took = sent.elapsed().as_secs_f64() * 1000.0;
    assert!(took >= 150.0, "taken after {took} ms");
}

/// A link cut for 1.8 s: once no acknowledgement has come for UDP_PROBE_MS + UDP_LATE_MS, channel 1 goes on UDP and
/// TCP both (UDP still on: the last acknowledgement is not UDP_HEARD_MS old), and on UDP alone once they come again.
#[test]
fn channel_1_goes_both_ways_while_a_probe_is_late() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults { cut_s: 1.8, cut_at_s: 0.5, ..NetFaults::default() });
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let mut seen = Vec::new();
    let (at_a, _) = relay_match(epoch, &a, &b, 3500.0, &mut seen);
    let s = a.stats();
    assert!(s.both > 10 && s.udp > s.both, "both ways at times, UDP alone at others: {s:?}");
    assert!(at_a.len() > 20, "ruled: {}", at_a.len());
}

/// A stream of channel-0 frames to a player whose client opens a second connection twice while the first goes on
/// (frames read on the old connection after each hello, which the relay sends again): every frame comes once, in
/// order, and the second resume's count is the relay's (no gap, no sign-out).
#[test]
fn two_resumes_count_each_frame_once() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults::default());
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let mut seen = Vec::new();
    let mut n = 0u32;
    let mut stream = |seen: &mut Vec<RelayEvent>, k: u32| {
        for _ in 0..k {
            b.send(a.me(), CHANNEL_MESSAGES, format!("n{n}").as_bytes());
            n += 1;
            seen.extend(std::iter::from_fn(|| a.poll()));
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    stream(&mut seen, 30);
    for k in 1..=2 {
        a.resume_now();
        let until = Instant::now() + Duration::from_secs(5);
        while a.stats().resumes < k && Instant::now() < until {
            stream(&mut seen, 1);
        }
        assert_eq!(a.stats().resumes, k, "resume {k}");
        stream(&mut seen, 30);
    }
    let sent = n;
    let until = Instant::now() + Duration::from_secs(2);
    let got = |seen: &[RelayEvent]| {
        seen.iter()
            .filter_map(|e| match e {
                RelayEvent::Packet { bytes, .. } => std::str::from_utf8(bytes).ok()?.strip_prefix('n')?.parse().ok(),
                _ => None,
            })
            .collect::<Vec<u32>>()
    };
    while Instant::now() < until && got(&seen).len() < sent as usize {
        seen.extend(std::iter::from_fn(|| a.poll()));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(signed_out(&seen), None);
    assert_eq!(got(&seen), (0..sent).collect::<Vec<_>>(), "each frame once, in order");
    a.shutdown();
    b.shutdown();
}

/// A client that ends while its second connection is under way: the start is cancelled, or its welcome leaves with
/// a bye; the seat is never kept, so the other member sees the player leave (its lobby close) at once.
#[test]
fn a_second_connection_never_outlives_the_client() {
    let addr = local_relay();
    for wait_ms in [0, 1, 3, 8, 20] {
        let epoch = Instant::now();
        let a = faulty(&addr, epoch, NetFaults::default());
        let b = faulty(&addr, epoch, NetFaults::default());
        lobby_of_two(&a, &b);
        a.resume_now();
        std::thread::sleep(Duration::from_millis(wait_ms));
        a.shutdown();
        // (the owner gone: its lobby closes)
        wait_for(&b, |e| matches!(e, RelayEvent::Lobby { closed: Some(_), .. }).then_some(()));
        b.shutdown();
    }
}

/// A hosted match whose only other human falls silent while both links are healthy: the relay answers the stall's
/// ping, so the host opens no second connection.
#[test]
fn a_silent_peer_on_a_healthy_link_is_no_stall() {
    let addr = local_relay();
    let epoch = Instant::now();
    let a = faulty(&addr, epoch, NetFaults::default());
    let b = faulty(&addr, epoch, NetFaults::default());
    lobby_of_two(&a, &b);
    let humans = vec![(0, a.me().to_owned()), (1, b.me().to_owned())];
    let start = ms(epoch);
    let mut la = a.host_match(humans, start + 50.0);
    let mut lb = b.join_match(a.me());
    let mut heard = 0;
    while ms(epoch) < start + 5500.0 {
        let now = ms(epoch);
        la.send(&header(0, now), now);
        if now < start + 500.0 {
            lb.send(&header(1, now), now);
        }
        while la.recv(now).is_some() {}
        while lb.recv(now).is_some() {
            heard += 1;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(heard > 0, "the guest was ruled before it fell silent");
    assert_eq!(a.stats().resumes, 0, "no second connection on a healthy link");
    a.end_match();
    b.end_match();
}

/// The smallest round trip is timed on UDP (a probe to its acknowledgement) and, with UDP off, on TCP (a ping to its
/// pong): none before the first answer, then a small one on loopback.
#[test]
fn the_round_trip_is_timed_by_probe_and_by_ping() {
    let addr = local_relay();
    let epoch = Instant::now();
    let rtt_of = |c: &RelayClient| {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Some(ms) = c.rtt() {
                return ms;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no round trip timed");
    };
    let by_udp = client(&addr, epoch, false);
    let ms = rtt_of(&by_udp);
    assert!((0.0..500.0).contains(&ms), "by probe: {ms}");
    let mut o = ClientOptions::new("mvs-test 0", epoch);
    o.udp_blocked = true;
    o.times.ping_ms = 100.0;
    let by_tcp = RelayClient::connect(&addr, &RelayKey::fresh(), &o).expect("signed in");
    assert_eq!(by_tcp.rtt(), None, "nothing timed before the first ping");
    let ms = rtt_of(&by_tcp);
    assert!((0.0..500.0).contains(&ms), "by ping: {ms}");
    by_udp.shutdown();
    by_tcp.shutdown();
}

/// A player's link to the relay's queue core, on channel 0 by the p2p frame rules.
struct QueueLink<'a> {
    c: &'a RelayClient,
    joiner: MessageJoiner,
    got: Vec<Value>,
}

impl<'a> QueueLink<'a> {
    /// The knock: the core answers `server`.
    fn open(c: &'a RelayClient) -> QueueLink<'a> {
        c.send(QUEUE_HOST, CHANNEL_MESSAGES, &signal(FRAME_OPEN, &[]));
        let mut l = QueueLink { c, joiner: MessageJoiner::new(), got: Vec::new() };
        l.wait("server");
        l
    }

    fn say(&self, m: &Value) {
        for p in split_message(m.to_string().as_bytes(), P2P_MAX_PACKET) {
            self.c.send(QUEUE_HOST, CHANNEL_MESSAGES, &p);
        }
    }

    /// The core's next message of type `t` (up to 3 s), the others before it kept in `got`.
    fn wait(&mut self, t: &str) -> Value {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            while let Some(e) = self.c.poll() {
                let RelayEvent::Packet { from, channel: 0, bytes } = e else { continue };
                assert_eq!(from, QUEUE_HOST);
                if let Taken::Message(b) = self.joiner.take(&bytes) {
                    let m: Value = serde_json::from_slice(&b).expect("the core's JSON");
                    if m["t"] == t {
                        return m;
                    }
                    self.got.push(m);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("no {t} from the queue core in time (got {:?})", self.got);
    }
}

/// A steady 1.5 s round trip on UDP (a delay of 750 ms each way, probes every second, so one is always out when the
/// last is acknowledged) is timed as 1.5 s, not as the second between two probes less it.
#[test]
fn a_long_steady_round_trip_is_timed_whole() {
    let addr = local_relay();
    let epoch = Instant::now();
    let c = faulty(&addr, epoch, NetFaults { delay_ms: 750.0, ..NetFaults::default() });
    std::thread::sleep(Duration::from_millis(4500));
    let ms = c.rtt().expect("timed");
    assert!((1450.0..1700.0).contains(&ms), "{ms}");
    c.shutdown();
}

/// Two players queue 1v1 at the relay's queue core (no lobby of their own), take its pre-match and play the match
/// its referee on the relay rules: the relay-hosted match of wire 5.
#[test]
fn a_queue_match_of_the_relays_core_is_refereed_on_the_relay() {
    let addr = local_relay();
    let epoch = Instant::now();
    let (a, b) = (client(&addr, epoch, false), client(&addr, epoch, false));
    let mut la = QueueLink::open(&a);
    let mut lb = QueueLink::open(&b);
    for (n, l) in [&mut la, &mut lb].into_iter().enumerate() {
        l.say(&json!({ "t": "hello", "proto": PROTOCOL, "name": format!("Tester{n}") }));
        l.wait("welcome");
        let rtt = l.c.rtt();
        l.say(&json!({ "t": "queue.join", "modes": ["1v1"], "character": "", "skin": "", "compat": "c1", "rtt": rtt }));
    }
    let (fa, fb) = (la.wait("queue.found"), lb.wait("queue.found"));
    assert_eq!(fa["match"], fb["match"]);
    for l in [&mut la, &mut lb] {
        l.say(&json!({ "t": "pm.perks", "perks": [] }));
    }
    for l in [&mut la, &mut lb] {
        l.wait("match.start");
        l.say(&json!({ "t": "match.loaded" }));
    }
    let (ga, gb) = (la.wait("match.go"), lb.wait("match.go"));
    let referee = ga["referee"].as_str().expect("a referee").to_owned();
    assert_eq!(gb["referee"], referee.as_str());
    assert!(referee.starts_with("queue.m"), "{referee}");
    // (slot 0 is the first of the found match's players)
    let slot_a = fa["me"].as_u64().expect("a slot");
    let (mut ma, mut mb) = (a.join_match(&referee), b.join_match(&referee));
    if slot_a == 0 {
        ruled_both(epoch, &mut ma, &mut mb);
    } else {
        ruled_both(epoch, &mut mb, &mut ma);
    }
    la.say(&json!({ "t": "match.leave" }));
    lb.wait("match.left");
    a.end_match();
    b.end_match();
    a.shutdown();
    b.shutdown();
}

/// One player's net service on `relay` (None: signed in nowhere) with the release's relay `default`.
struct Svc {
    svc: mvs_net::service::NetService,
    page: mvs_net::service::page::PageId,
    events: Vec<mvs_net::service::api::NetEvent>,
    epoch: Instant,
}

impl Svc {
    fn new(relay: Option<&str>, default: Option<&str>, epoch: Instant) -> Svc {
        use mvs_net::service::{NetService, ServiceConfig, ServiceOptions};
        let now = ms(epoch);
        let mut svc = NetService::new(ServiceOptions { epoch, ..ServiceOptions::default() }, now);
        let config = ServiceConfig {
            relay: relay.map(str::to_owned),
            default_relay: default.map(str::to_owned),
            ..ServiceConfig::default()
        };
        svc.configure(config, now);
        if let Some(r) = relay {
            let c = RelayClient::connect(r, &RelayKey::fresh(), &ClientOptions::new("mvs-test 0", epoch)).expect("in");
            svc.start_backend(mvs_net::backend::BackendKind::Relay, Box::new(c), now);
        }
        let page = svc.open_page(now);
        Svc { svc, page, events: Vec::new(), epoch }
    }
    fn step(&mut self) {
        let now = ms(self.epoch);
        self.svc.pump(now);
        let page = self.page;
        self.events.extend(self.svc.events(page));
    }
    fn open(&mut self, t: mvs_net::service::api::SessionTarget) {
        self.events.clear();
        let now = ms(self.epoch);
        self.svc.take(self.page, mvs_net::service::api::NetRequest::Open(t), now);
    }
    fn say(&mut self, m: Value) {
        let now = ms(self.epoch);
        self.svc.take(self.page, mvs_net::service::api::NetRequest::Msg(m.to_string()), now);
    }
    /// The page's opened code or its closed why, waited for (5 s).
    fn outcome(&mut self, others: &mut [&mut Svc]) -> Result<String, mvs_net::service::api::ClosedWhy> {
        use mvs_net::service::api::NetEvent;
        let end = ms(self.epoch) + 5000.0;
        loop {
            self.step();
            for o in others.iter_mut() {
                o.step();
            }
            for e in &self.events {
                match e {
                    NetEvent::Opened { code, .. } => return Ok(code.clone()),
                    // (the page's session before this one, ended by the open)
                    NetEvent::Closed(mvs_net::service::api::ClosedWhy::Replaced) => {}
                    NetEvent::Closed(why) => return Err(why.clone()),
                    _ => {}
                }
            }
            assert!(ms(self.epoch) < end, "no outcome: {:?}", self.events);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn has(&self, t: &str, test: impl Fn(&Value) -> bool) -> bool {
        self.events.iter().any(|e| match e {
            mvs_net::service::api::NetEvent::Msg(m) => {
                serde_json::from_str::<Value>(m).is_ok_and(|v| v["t"] == t && test(&v))
            }
            _ => false,
        })
    }
}

/// Pumps `ps` until `cond` (5 s).
fn until(ps: &mut [&mut Svc], what: &str, cond: impl Fn(&[&mut Svc]) -> bool) {
    let epoch = ps[0].epoch;
    let end = ms(epoch) + 5000.0;
    while !cond(ps) {
        assert!(ms(epoch) < end, "in time: {what}");
        for p in ps.iter_mut() {
            p.step();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn codes_of_every_kind_joined_from_every_setting() {
    use mvs_net::codes::{LobbyCode, decode_lobby_code, encode_lobby_code, is_short};
    use mvs_net::lan::direct_code::{DirectCode, encode_direct_code};
    use mvs_net::service::api::{ClosedWhy, JoinWhy, OffWhy, SessionTarget};
    let epoch = Instant::now();
    // (six players on this PC's one address)
    let r = local_relay_with(mvs_relay::limits::RelayLimits { connections_per_address: 8.0, ..Default::default() });
    let other = "relay.example:7777";
    let failed = |why| Err(ClosedWhy::JoinFailed { why });

    // on the release's relay: a hosted lobby's code is its short code, joined by it, the lobby's id adopted
    let mut host = Svc::new(Some(&r), Some(&r), epoch);
    host.step();
    host.open(SessionTarget::Host);
    let short = host.outcome(&mut []).expect("hosted");
    assert!(is_short(&short), "{short}");
    host.say(json!({ "t": "hello", "proto": PROTOCOL, "app": "test", "name": "H" }));
    host.say(json!({ "t": "lobby.enter", "compat": "ctest.1" }));
    let mut g = Svc::new(Some(&r), Some(&r), epoch);
    g.step();
    g.open(SessionTarget::Join(short.to_uppercase()));
    assert_eq!(g.outcome(&mut [&mut host]), Ok(short.clone()), "the page shows the short code");
    g.say(json!({ "t": "hello", "proto": PROTOCOL, "app": "test", "name": "G" }));
    g.say(json!({ "t": "lobby.join", "lobby": short, "compat": "ctest.1" }));
    until(&mut [&mut g, &mut host], "the guest in the core's lobby", |ps| {
        ps[0].has("lobby.state", |m| m["ev"] == "started")
    });
    // a short code no lobby has, and one mistyped
    let mut g2 = Svc::new(Some(&r), Some(&r), epoch);
    g2.step();
    let unknown = if short == "aaaaaa" { "bbbbbb" } else { "aaaaaa" };
    g2.open(SessionTarget::Join(unknown.into()));
    assert_eq!(g2.outcome(&mut [&mut host]), failed(JoinWhy::Gone));
    g2.open(SessionTarget::Join("k7m2p0".into()));
    let mistyped = JoinWhy::BadCode { why: mvs_net::codes::CodeWhy::Mistyped };
    assert_eq!(g2.outcome(&mut []), failed(mistyped));

    // the long forms from the release's relay: an Epic code needs Epic, a direct code direct play
    let e = encode_lobby_code(&LobbyCode::Epic { lobby: "ab".into() }, Some(&r)).unwrap();
    g2.open(SessionTarget::Join(e));
    assert!(matches!(g2.outcome(&mut []), Err(ClosedWhy::JoinFailed { why: JoinWhy::EpicOff { .. } })));
    let dc = DirectCode {
        addresses: vec!["127.0.0.1".into()],
        port: 7777,
        fingerprint: "0123456789abcdef".into(),
        lobby: format!("l{}", "7".repeat(20)),
    };
    let d = encode_direct_code(&dc, true).unwrap();
    g2.open(SessionTarget::Join(d));
    assert_eq!(g2.outcome(&mut []), failed(JoinWhy::NoDirect));

    // on another relay than the release's: a hosted lobby's code is the long relay code, joined by it; a short code
    // names the release's relay
    let mut h2 = Svc::new(Some(&r), Some(other), epoch);
    h2.step();
    h2.open(SessionTarget::Host);
    let long = h2.outcome(&mut []).expect("hosted");
    let read = decode_lobby_code(&long, Some(other)).expect("a code");
    assert!(matches!(&read, LobbyCode::Relay { relay: Some(_), .. }), "{long}");
    let mut g3 = Svc::new(Some(&r), Some(other), epoch);
    g3.step();
    g3.open(SessionTarget::Join(long.clone()));
    assert_eq!(g3.outcome(&mut [&mut h2]), Ok(long.clone()));
    g3.open(SessionTarget::Join(short.clone()));
    assert_eq!(g3.outcome(&mut []), failed(JoinWhy::ShortOtherRelay));
    // that lobby's long code in the release's relay's form (it is on r), from the release's relay: joined too
    let LobbyCode::Relay { lobby, .. } = read else { unreachable!() };
    let r_short = encode_lobby_code(&LobbyCode::Relay { lobby, relay: None }, Some(&r)).unwrap();
    g2.open(SessionTarget::Join(r_short.clone()));
    assert_eq!(g2.outcome(&mut [&mut h2]), Ok(r_short));

    // signed in nowhere: the release's relay is off; with no release's relay at all, none
    let mut off = Svc::new(None, Some(&r), epoch);
    off.open(SessionTarget::Join(short.clone()));
    assert!(
        matches!(
            off.outcome(&mut []),
            Err(ClosedWhy::JoinFailed { why: JoinWhy::RelayOff { why: OffWhy::NotOn | OffWhy::Off, .. } })
        ),
        "{:?}",
        off.events
    );
    let mut none = Svc::new(None, None, epoch);
    none.open(SessionTarget::Join(short));
    assert_eq!(none.outcome(&mut []), failed(JoinWhy::NoDefaultRelay));
}
