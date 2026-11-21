//! The relay over sockets on 127.0.0.1 ("the wire"): the handshake, the frames, the lobby calls and
//! packets, the UDP path, the peer handles, the limits on the wire and a refereed match, with a test client; and the
//! sealing of wire 7 (a tampered frame, a replayed datagram, a plain datagram, nothing readable after the start).

mod client;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use client::*;
use ed25519_dalek::Signer;
use mvs_protocol::match_wire::RESUME_MS;
use mvs_protocol::p2p_frame::{FRAME_OPEN, MessageJoiner, Taken as Frame, split_message};
use mvs_protocol::relay_wire::{
    FT_DATA, FT_JSON, FT_MATCH, MAX_FRAME, MAX_PEER_HANDLES, RELAY_WIRE_VERSION, ServerMessage, TOKEN_BYTES,
    UDP_FRESH_MS, UDP_LATE_MS, UDP_MATCH, UDP_PROBE, UDP_PROBE_ACK, UDP_PROBE_MS, frame, packet_frame, probe_datagram,
    read_packet,
};
use mvs_protocol::relay_wire::{Side, handshake_bytes, json_frame};
use mvs_protocol::version::PROTOCOL;
use mvs_relay::core::{CorePlayer, Session, Taken};
use mvs_relay::limits::{DEFAULT_LIMITS, RelayLimits};
use mvs_relay::server::{Options, Relay, key_id, start_relay};
use serde_json::{Value, json};

fn limits(over: &[(&str, f64)]) -> RelayLimits {
    let mut l = DEFAULT_LIMITS;
    for (k, v) in over {
        let (_, slot) = l.fields_mut().into_iter().find(|(n, _)| n == k).expect("a limit");
        *slot = *v;
    }
    l
}

type Logs = Rc<RefCell<Vec<String>>>;

/// A relay on 127.0.0.1, its log lines kept.
fn relay_with(l: RelayLimits) -> (Relay, Logs) {
    let logs: Logs = Rc::default();
    let into = logs.clone();
    let o = Options {
        host: "127.0.0.1".into(),
        limits: l,
        log: Some(Box::new(move |line| into.borrow_mut().push(line.to_string()))),
        ..Options::default()
    };
    (start_relay(o).unwrap(), logs)
}

/// The relay of most checks (every client here is on one address).
fn relay() -> (Relay, Logs) {
    relay_with(limits(&[("connectionsPerAddress", 100.0)]))
}

fn players(r: &Relay) -> usize {
    r.with_core(|c, _| c.players())
}

/// The players connected (not parked).
fn live(r: &Relay) -> usize {
    r.with_core(|c, _| c.players() - c.parked())
}

fn parked(r: &Relay) -> usize {
    r.with_core(|c, _| c.parked())
}

async fn client(r: &Relay) -> Rc<Client> {
    Client::new(r.port, None).await
}

#[test]
fn handshake_the_id_of_the_key_the_relay_proves_its_own() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        let id = a.sign_in(&relay.id).await;
        assert_eq!(id, key_id(&a.raw));
        assert!(
            id.len() == 33
                && id.starts_with('r')
                && id[1..].bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        );
        assert_eq!(a.welcome.borrow().as_ref().unwrap().udp, relay.port);
        assert_eq!(a.welcome.borrow().as_ref().unwrap().id, id);
        // (the welcome names the game protocol of the cores the relay hosts)
        assert_eq!(a.welcome.borrow().as_ref().unwrap().proto, Some(u64::from(PROTOCOL)));
        a.send(json!({ "t": "ping" }));
        a.msg("pong").await;
        a.end();
        relay.close().await;
    });
}

#[test]
fn handshake_a_bad_signature_another_version_a_message_before_it_a_late_hello() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.hello();
        a.msg("challenge").await;
        a.send(json!({ "t": "prove", "sig": b64(&a.key.sign(b"something else").to_bytes()) }));
        assert_eq!(a.refused().await, "bad signature");
        let b = client(&relay).await;
        b.hello_with(json!(RELAY_WIRE_VERSION + 1), None, None);
        assert_eq!(b.refused().await, format!("wire version not {RELAY_WIRE_VERSION}"));
        let c = client(&relay).await;
        c.send(json!({ "t": "req", "id": 1, "op": "search", "bucket": "b", "limit": 1 }));
        assert_eq!(c.refused().await, "bad message");
        let d = client(&relay).await;
        d.send(json!({ "t": "hello", "v": RELAY_WIRE_VERSION, "key": "short", "nonce": "x", "app": "a" }));
        assert_eq!(d.refused().await, "bad hello");
        let (quick, _) = relay_with(limits(&[("helloMs", 150.0)]));
        let e = client(&quick).await;
        assert_eq!(e.refused().await, "no hello in time");
        let f = client(&quick).await;
        f.hello();
        f.msg("challenge").await;
        assert_eq!(f.refused().await, "no hello in time", "the prove too");
        quick.close().await;
        assert_eq!(players(&relay), 0);
        relay.close().await;
    });
}

#[test]
fn handshake_neither_signature_passes_as_the_other_nor_for_another_relay() {
    run(async {
        let (relay, _) = relay();
        let x = client(&relay).await;
        x.hello();
        let relay_raw: [u8; 32] = unb64(&x.msg("challenge").await["key"]).try_into().unwrap();
        x.end();
        // a client claiming the relay's own key gives back the relay's challenge signature as its prove
        let a = client(&relay).await;
        a.hello_with(json!(RELAY_WIRE_VERSION), Some(&relay_raw), None);
        let ca = a.msg("challenge").await;
        a.send(json!({ "t": "prove", "sig": ca["sig"] }));
        assert_eq!(a.refused().await, "bad signature", "the challenge sig as a prove");
        // the relay as an oracle: the first connection's relay nonce sent as the second one's client nonce, the
        // second challenge's signature then given as the first one's prove
        let b1 = client(&relay).await;
        b1.hello_with(json!(RELAY_WIRE_VERSION), Some(&relay_raw), None);
        let c1 = b1.msg("challenge").await;
        let b2 = client(&relay).await;
        b2.hello_with(json!(RELAY_WIRE_VERSION), Some(&relay_raw), Some(unb64(&c1["nonce"]).try_into().unwrap()));
        let c2 = b2.msg("challenge").await;
        b1.send(json!({ "t": "prove", "sig": c2["sig"] }));
        assert_eq!(b1.refused().await, "bad signature", "the relay as an oracle");
        b2.end();
        // forwarding: a client's prove made for relay A (A's key in its transcript) given to relay B
        let (other, _) = relay_with(DEFAULT_LIMITS);
        let x = client(&other).await;
        x.hello();
        let other_raw: [u8; 32] = unb64(&x.msg("challenge").await["key"]).try_into().unwrap();
        x.end();
        let d = client(&relay).await;
        let nonce = d.hello();
        let cd = d.msg("challenge").await;
        d.prove(&d.transcript(nonce, &cd, Some(other_raw)));
        assert_eq!(d.refused().await, "bad signature", "a prove made for another relay");
        let e = Client::new(relay.port, Some(d.key.clone())).await;
        e.sign_in(&relay.id).await;
        e.end();
        other.close().await;
        relay.close().await;
    });
}

#[test]
fn handshake_a_small_order_key_a_client_text_never_in_the_refusal_or_the_log() {
    run(async {
        let (relay, logs) = relay();
        let a = client(&relay).await;
        a.hello_with(json!(RELAY_WIRE_VERSION), Some(&[0u8; 32]), None);
        assert_eq!(a.refused().await, "bad key");
        // a kx of small order (the exchange's secret all zero): a bad key; none: a bad hello
        let a = client(&relay).await;
        a.send(json!({ "t": "hello", "v": RELAY_WIRE_VERSION, "key": b64(&a.raw), "nonce": b64(&[1u8; 32]), "kx": b64(&[0u8; 32]) }));
        assert_eq!(a.refused().await, "bad key");
        let a = client(&relay).await;
        a.send(json!({ "t": "hello", "v": RELAY_WIRE_VERSION, "key": b64(&a.raw), "nonce": b64(&[1u8; 32]) }));
        assert_eq!(a.refused().await, "bad hello");
        let b = client(&relay).await;
        let body = serde_json::to_vec(&json!({ "t": "hello", "v": "x".repeat(65515) })).unwrap();
        assert!(body.len() < MAX_FRAME, "the hello fits a frame");
        b.write(&frame(FT_JSON, &body).unwrap());
        assert_eq!(b.refused().await, format!("wire version not {RELAY_WIRE_VERSION}"));
        assert!(!logs.borrow().iter().any(|l| l.contains("xxxx")), "the log names it by kind");
        let c = client(&relay).await;
        c.sign_in(&relay.id).await;
        c.end();
        relay.close().await;
    });
}

#[test]
fn frames_too_long_garbage_an_unknown_type_bad_json() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.write(&(MAX_FRAME as u32 + 1).to_be_bytes());
        assert_eq!(a.refused().await, "frame too long");
        let b = client(&relay).await;
        b.write(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(b.refused().await, "frame too long");
        let c = client(&relay).await;
        c.write(&frame(9, &[0, 0, 0]).unwrap());
        assert_eq!(c.refused().await, "bad frame");
        let d = client(&relay).await;
        d.write(&frame(FT_JSON, b"{\"t\":").unwrap());
        assert_eq!(d.refused().await, "bad message");
        let e = client(&relay).await;
        e.sign_in(&relay.id).await;
        e.write(&frame(FT_DATA, &[0, 0]).unwrap());
        assert_eq!(e.refused().await, "bad frame", "a packet shorter than its handle");
        let f = client(&relay).await;
        f.sign_in(&relay.id).await;
        f.send(json!({ "t": "what" }));
        assert_eq!(f.refused().await, "bad message");
        let g = client(&relay).await;
        g.write(&[0, 0, 0, 0]);
        assert_eq!(g.refused().await, "frame too long", "a length of 0");
        relay.close().await;
    });
}

#[test]
fn lobby_calls_and_packets_over_the_wire() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        let b = client(&relay).await;
        let ida = a.sign_in(&relay.id).await;
        let idb = b.sign_in(&relay.id).await;
        let made = a.req(json!({ "op": "create", "bucket": "q", "max": 2, "listed": { "at": 7 } })).await;
        assert_eq!(made["ok"], true);
        let lobby = lobby_of(&made);
        let found = b.req(json!({ "op": "search", "bucket": "q", "limit": 5 })).await;
        assert_eq!(
            found,
            json!({ "t": "res", "id": 1, "ok": true, "value": { "lobbies": [{ "id": lobby, "owner": ida, "members": 1, "max": 2, "at": 7, "info": "" }] } })
        );
        assert_eq!(
            b.req(json!({ "op": "join", "lobby": lobby })).await["value"],
            json!({ "owner": ida, "members": [ida, idb] })
        );
        assert_eq!(a.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida, idb] }));
        assert_eq!(
            b.req(json!({ "op": "join", "lobby": "lnone" })).await,
            json!({ "t": "res", "id": 3, "ok": false, "error": "EOS_NotFound" })
        );
        // channel 0 on TCP, the handle told first
        a.send(json!({ "t": "peer", "handle": 40, "id": idb }));
        a.write(&packet_frame(FT_DATA, 40, &[1, 2, 3]).unwrap());
        a.write(&packet_frame(FT_DATA, 41, &[9]).unwrap());
        let peer = b.msg("peer").await;
        assert_eq!(peer["id"], ida);
        let p = b.next(is_ty(FT_DATA)).await;
        assert_eq!(unpack(&p.body), (peer["handle"].as_u64().unwrap() as u32, vec![1, 2, 3]));
        a.req(json!({ "op": "invite", "to": idb, "payload": "come" })).await;
        assert_eq!(b.msg("invite").await, json!({ "t": "invite", "from": ida, "payload": "come" }));
        assert!(!b.inner.borrow().got.iter().any(|g| g.ty == FT_DATA), "an unknown handle: dropped");
        // the receiver refuses the link: the sender is told
        b.send(json!({ "t": "closePeer", "id": ida }));
        assert_eq!(a.msg("peerClosed").await, json!({ "t": "peerClosed", "id": idb }));
        // a disconnect in a lobby: the owner told
        b.end();
        assert_eq!(a.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida] }));
        a.end();
        until("all gone", 2000, || relay.with_core(|c, _| c.players() == 0 && c.lobbies() == 0)).await;
        relay.close().await;
    });
}

/// a, b and c signed in to `r`, in one lobby of a's.
async fn trio(r: &Relay) -> (Rc<Client>, Rc<Client>, Rc<Client>, [String; 3]) {
    let (a, b, c) = (client(r).await, client(r).await, client(r).await);
    let ids = [a.sign_in(&r.id).await, b.sign_in(&r.id).await, c.sign_in(&r.id).await];
    let lobby = lobby_of(&a.req(json!({ "op": "create", "bucket": "u", "max": 4 })).await);
    b.req(json!({ "op": "join", "lobby": lobby })).await;
    c.req(json!({ "op": "join", "lobby": lobby })).await;
    for x in [&a, &b] {
        x.msg("lobby").await;
    }
    a.msg("lobby").await;
    (a, b, c, ids)
}

#[test]
fn channel_1_by_udp_while_the_probes_say_the_client_hears_udp_else_over_tcp() {
    run(async {
        let (relay, _) = relay();
        let (a, b, c, [ida, idb, idc]) = trio(&relay).await;
        b.open_udp("127.0.0.1").await;
        a.open_udp("127.0.0.1").await;
        b.probe(true);
        let ack = b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        assert_eq!(ack.token, b.token());
        // a datagram with a token no one has is not answered
        let mut stranger = [0u8; TOKEN_BYTES];
        rand::fill(&mut stranger);
        a.udp_raw(&a.udp_sealed(&stranger, UDP_PROBE, &[&[1]]));
        a.probe(true);
        a.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        assert_eq!(a.datagrams(), 0);
        // a -> b on TCP: the first packet of a new handle comes on TCP, the next by UDP
        a.send(json!({ "t": "peer", "handle": 1, "id": idb }));
        a.write(&packet_frame(FT_MATCH, 1, &[5]).unwrap());
        a.write(&packet_frame(FT_MATCH, 1, &[6]).unwrap());
        let peer = b.msg("peer").await;
        assert_eq!(match_bytes(&b.next(is_ty(FT_MATCH)).await), [5]);
        let d = b.datagram(|d| d.kind == UDP_MATCH).await;
        assert_eq!(u64::from(d.handle), peer["handle"].as_u64().unwrap());
        assert_eq!(d.bytes, [6]);
        // b -> a by UDP; a probed too, so after its first (on TCP) it gets them by UDP
        b.send(json!({ "t": "peer", "handle": 3, "id": ida }));
        sleep(30).await;
        b.udp_send(UDP_MATCH, 3, &[7]);
        b.udp_send(UDP_MATCH, 3, &[8]);
        let pa = a.msg("peer").await;
        assert_eq!(unpack(&a.next(is_ty(FT_MATCH)).await.body), (pa["handle"].as_u64().unwrap() as u32, vec![7]));
        assert_eq!(a.datagram(|d| d.kind == UDP_MATCH).await.bytes, [8]);
        // b's probe says it hears no acknowledgement (its inbound UDP is blocked): a's packets to it come on TCP
        b.probe(false);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.write(&packet_frame(FT_MATCH, 1, &[9]).unwrap());
        assert_eq!(match_bytes(&b.next(is_ty(FT_MATCH)).await), [9], "a probe without the flag");
        // a client that never probed gets channel 1 over TCP
        b.send(json!({ "t": "peer", "handle": 4, "id": idc }));
        sleep(30).await;
        for x in [1, 2] {
            b.udp_send(UDP_MATCH, 4, &[x]);
        }
        c.msg("peer").await;
        assert_eq!(match_bytes(&c.next(is_ty(FT_MATCH)).await), [1]);
        assert_eq!(match_bytes(&c.next(is_ty(FT_MATCH)).await), [2]);
        // a probe from another IP than the TCP connection's (a spoofed source, a sniffed token) is not taken
        c.open_udp("127.0.0.2").await;
        c.probe(true);
        sleep(100).await;
        assert_eq!(c.datagrams(), 0, "not acknowledged");
        b.udp_send(UDP_MATCH, 4, &[3]);
        b.udp_send(UDP_MATCH, 4, &[4]);
        assert_eq!(match_bytes(&c.next(is_ty(FT_MATCH)).await), [3]);
        assert_eq!(match_bytes(&c.next(is_ty(FT_MATCH)).await), [4], "still on TCP");
        c.send(json!({ "t": "peer", "handle": 1, "id": ida }));
        sleep(30).await;
        c.udp_send(UDP_MATCH, 1, &[1]);
        sleep(50).await;
        c.write(&packet_frame(FT_MATCH, 1, &[2]).unwrap());
        a.msg("peer").await;
        assert_eq!(match_bytes(&a.next(is_ty(FT_MATCH)).await), [2], "a datagram from another IP is dropped");
        for x in [&a, &b, &c] {
            x.end();
        }
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn sealed_a_tampered_frame_closes_the_connection() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.sign_in(&relay.id).await;
        a.send(json!({ "t": "ping" }));
        a.msg("pong").await;
        let mut f = a.sealed_frame(FT_JSON, br#"{"t":"ping"}"#);
        f[6] ^= 1;
        a.write_raw(&f);
        assert_eq!(a.refused().await, "bad seal");
        assert!(!a.inner.borrow().bad_seal, "the refusal opens");
        // a plain frame after the prove is no sealed one either
        let b = client(&relay).await;
        b.sign_in(&relay.id).await;
        b.write_raw(&frame(FT_JSON, br#"{"t":"ping"}"#).unwrap());
        assert_eq!(b.refused().await, "bad seal");
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn sealed_frames_behind_the_prove_in_one_chunk_are_read_sealed() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        let nonce = a.hello();
        let c = a.msg("challenge").await;
        let t = a.transcript(nonce, &c, None);
        let mut chunk = json_frame(
            &json!({ "t": "prove", "sig": b64(&a.key.sign(&handshake_bytes(Side::Client, &t)).to_bytes()) }),
        )
        .unwrap();
        chunk.extend(a.sealed_frame(FT_JSON, br#"{"t":"ping"}"#));
        chunk.extend(a.sealed_frame(FT_JSON, br#"{"t":"ping"}"#));
        a.write_raw(&chunk);
        a.msg("welcome").await;
        a.msg("pong").await;
        a.msg("pong").await;
        assert!(!a.closed());
        a.inner.borrow_mut().sealing = true;
        a.end();
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn sealed_a_replayed_datagram_reaches_the_peer_once_and_is_acked_once() {
    run(async {
        let (relay, _) = relay();
        let (a, b, c, [ida, _, _]) = trio(&relay).await;
        b.open_udp("127.0.0.1").await;
        let probe = b.udp_sealed(&b.token(), UDP_PROBE, &[&[1]]);
        b.udp_raw(&probe);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        b.udp_raw(&probe);
        sleep(100).await;
        assert_eq!(b.datagrams(), 0, "the probe again: no second ack");
        b.send(json!({ "t": "peer", "handle": 3, "id": ida }));
        sleep(30).await;
        let d = b.udp_sealed(&b.token(), UDP_MATCH, &[&3u32.to_be_bytes(), &[7]]);
        b.udp_raw(&d);
        b.udp_raw(&d);
        a.msg("peer").await;
        assert_eq!(match_bytes(&a.next(is_ty(FT_MATCH)).await), [7]);
        sleep(100).await;
        assert!(!a.inner.borrow().got.iter().any(|g| g.ty == FT_MATCH), "the packet again: not passed on");
        // (the next counter still goes)
        b.udp_send(UDP_MATCH, 3, &[8]);
        assert_eq!(match_bytes(&a.next(is_ty(FT_MATCH)).await), [8]);
        for x in [&a, &b, &c] {
            x.end();
        }
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn sealed_a_plain_probe_with_a_valid_token_gets_no_ack() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.sign_in(&relay.id).await;
        a.open_udp("127.0.0.1").await;
        // wire 6's probe, and the same padded to a sealed datagram's length
        a.udp_raw(&probe_datagram(&a.token(), true));
        let mut long = probe_datagram(&a.token(), true);
        long.resize(40, 0);
        a.udp_raw(&long);
        sleep(150).await;
        assert_eq!(a.datagrams(), 0, "not acknowledged");
        a.probe(true);
        a.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.end();
        relay.close().await;
    });
}

#[test]
fn sealed_nothing_after_the_welcome_is_readable_on_the_socket() {
    run(async {
        let (relay, _) = relay();
        let (a, b, c, _) = trio(&relay).await;
        a.send(json!({ "t": "ping" }));
        a.msg("pong").await;
        for x in [&a, &b, &c] {
            let raw = x.inner.borrow().raw.clone();
            let has = |w: &[u8]| raw.windows(w.len()).any(|x| x == w);
            assert!(has(br#""t":"challenge""#), "the challenge is plain");
            assert!(!has(br#""t":"lobby""#), "a lobby message in clear");
            assert!(!has(br#""t":"welcome""#), "the welcome in clear");
            assert!(!has(br#""t":"res""#), "an answer in clear");
            assert!(!x.inner.borrow().bad_seal);
        }
        for x in [&a, &b, &c] {
            x.end();
        }
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn channel_1_a_probe_older_than_udp_fresh_ms_sends_it_back_to_tcp() {
    run(async {
        let t = Rc::new(Cell::new(0.0));
        let clock = t.clone();
        let o = Options { host: "127.0.0.1".into(), now: Some(Rc::new(move || clock.get())), ..Options::default() };
        let r = start_relay(o).unwrap();
        let (a, b, _c, ids) = trio(&r).await;
        b.open_udp("127.0.0.1").await;
        b.probe(true);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.send(json!({ "t": "peer", "handle": 1, "id": ids[1] }));
        for x in [1, 2] {
            a.write(&packet_frame(FT_MATCH, 1, &[x]).unwrap());
        }
        b.next(is_ty(FT_MATCH)).await;
        assert_eq!(b.datagram(|d| d.kind == UDP_MATCH).await.bytes, [2]);
        t.set(t.get() + UDP_FRESH_MS);
        a.write(&packet_frame(FT_MATCH, 1, &[3]).unwrap());
        assert_eq!(match_bytes(&b.next(is_ty(FT_MATCH)).await), [3]);
        b.probe(true);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.write(&packet_frame(FT_MATCH, 1, &[4]).unwrap());
        assert_eq!(b.datagram(|d| d.kind == UDP_MATCH).await.bytes, [4], "a new probe: UDP again");
        r.close().await;
    });
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<(Vec<Value>, usize)>>);

impl CorePlayer for Fake {
    fn send(&mut self, m: &ServerMessage) -> Taken {
        self.0.borrow_mut().0.push(serde_json::to_value(m).unwrap());
        Taken::Yes
    }
    fn packet(&mut self, _channel: u8, _from: &str, _bytes: &[u8]) -> Taken {
        self.0.borrow_mut().1 += 1;
        Taken::Yes
    }
    fn refuse(&mut self, _why: &str) {}
}

fn join_fake(r: &Relay, id: &str, f: &Fake) -> Session {
    r.with_core(|c, now| c.join(id, "127.0.0.1", Box::new(f.clone()), now)).unwrap().0
}

fn fake_lobby(r: &Relay, s: &Session, bucket: &str) -> String {
    r.with_core(|c, now| c.request(s, &json!({ "op": "create", "bucket": bucket, "max": 2 }), now)).unwrap()["lobby"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn peer_handles_1000_open_and_close_cycles_of_one_peer_and_of_1000_peers_lose_nothing() {
    run(async {
        let (r, _) = relay_with(limits(&[
            ("controlPerSecond", 1e6),
            ("players", 2000.0),
            ("connectionsPerAddress", 2000.0),
            ("joinsPerMinute", 1e6),
            ("joinsPerAddress", 1e6),
            ("lobbiesPerAddress", 1e6),
            // (b says nothing through the second loop, which can outlast the silence limit on a slow machine)
            ("silentMs", 1e9),
        ]));
        let (a, b) = (client(&r).await, client(&r).await);
        let ida = a.sign_in(&r.id).await;
        let idb = b.sign_in(&r.id).await;
        let lobby = lobby_of(&b.req(json!({ "op": "create", "bucket": "h", "max": 2 })).await);
        a.req(json!({ "op": "join", "lobby": lobby })).await;
        b.msg("lobby").await;
        let (mut ha, mut hb) = (0u32, 0u32);
        let mut seen = HashSet::new();
        for i in 0..1000u32 {
            let x = (i & 255) as u8;
            ha += 1;
            a.send(json!({ "t": "peer", "handle": ha, "id": idb }));
            a.write(&packet_frame(FT_DATA, ha, &[x]).unwrap());
            let p = b.msg("peer").await;
            let h = p["handle"].as_u64().unwrap() as u32;
            assert!(seen.insert(h), "a relay handle is never used twice");
            assert_eq!(unpack(&b.next(is_ty(FT_DATA)).await.body), (h, vec![x]));
            hb += 1;
            b.send(json!({ "t": "peer", "handle": hb, "id": ida }));
            b.write(&packet_frame(FT_MATCH, hb, &[x]).unwrap());
            let q = a.msg("peer").await;
            assert_eq!(unpack(&a.next(is_ty(FT_MATCH)).await.body), (q["handle"].as_u64().unwrap() as u32, vec![x]));
            if i % 2 == 1 {
                a.send(json!({ "t": "closePeer", "id": idb }));
                b.msg("peerClosed").await;
            } else {
                b.send(json!({ "t": "closePeer", "id": ida }));
                a.msg("peerClosed").await;
            }
        }
        // one player and 1000 others in turn, each in a lobby of its own that a joins and leaves
        for i in 0..1000u32 {
            let x = (i & 255) as u8;
            let f = Fake::default();
            let id = format!("rpeer{i}");
            let s = join_fake(&r, &id, &f);
            let l = fake_lobby(&r, &s, "h");
            a.req(json!({ "op": "join", "lobby": l })).await;
            ha += 1;
            a.send(json!({ "t": "peer", "handle": ha, "id": id }));
            a.write(&packet_frame(FT_DATA, ha, &[x]).unwrap());
            // (out of the lobby, the peer may still answer: a sent to it first)
            a.req(json!({ "op": "leave", "lobby": l })).await;
            assert_eq!(f.0.borrow().1, 1);
            r.with_core(|c, now| c.packet(&s, 0, &ida, &[x], now));
            let p = a.msg("peer").await;
            assert_eq!(unpack(&a.next(is_ty(FT_DATA)).await.body), (p["handle"].as_u64().unwrap() as u32, vec![x]));
            a.send(json!({ "t": "closePeer", "id": id }));
            r.with_core(|c, now| c.leave(&s, now));
        }
        assert!(!a.closed() && !b.closed(), "never refused");
        a.send(json!({ "t": "peer", "handle": ha, "id": idb }));
        assert_eq!(a.refused().await, "a peer handle used twice");
        for i in 0..MAX_PEER_HANDLES {
            hb += 1;
            b.send(json!({ "t": "peer", "handle": hb, "id": format!("rlive{i}") }));
        }
        b.send(json!({ "t": "ping" }));
        b.msg("pong").await;
        hb += 1;
        b.send(json!({ "t": "peer", "handle": hb, "id": format!("rlive{MAX_PEER_HANDLES}") }));
        assert_eq!(b.refused().await, "too many peers", "live handles past the bound");
        // the relay's own handles past the bound: that link closed, both sides told
        let c = client(&r).await;
        let idc = c.sign_in(&r.id).await;
        let mut others = Vec::new();
        for i in 0..=MAX_PEER_HANDLES {
            let f = Fake::default();
            let s = join_fake(&r, &format!("rout{i}"), &f);
            let l = fake_lobby(&r, &s, "o");
            c.req(json!({ "op": "join", "lobby": l })).await;
            r.with_core(|core, now| core.packet(&s, 0, &idc, &[1], now));
            if i < MAX_PEER_HANDLES {
                c.msg("peer").await;
            } else {
                assert_eq!(
                    c.msg("peerClosed").await,
                    json!({ "t": "peerClosed", "id": format!("rout{i}") }),
                    "the client told"
                );
            }
            c.req(json!({ "op": "leave", "lobby": l })).await;
            others.push((s, f));
        }
        let (last, last_f) = &others[MAX_PEER_HANDLES];
        assert!(last_f.0.borrow().0.iter().any(|m| m["t"] == "peerClosed" && m["id"] == idc), "the sender told");
        // its handle freed: once one link closes, the same sender gets a handle
        c.send(json!({ "t": "closePeer", "id": "rout0" }));
        c.send(json!({ "t": "ping" }));
        c.msg("pong").await;
        let l2 = fake_lobby(&r, last, "o");
        c.req(json!({ "op": "join", "lobby": l2 })).await;
        r.with_core(|core, now| core.packet(last, 0, &idc, &[2], now));
        assert_eq!(c.msg("peer").await["id"], format!("rout{MAX_PEER_HANDLES}"));
        assert!(!c.closed());
        r.close().await;
    });
}

/// A key's second connection takes its seat over (the port's own rule): resumed by the hello's count, the ring resent
/// from it under the new connection's handles, the seat's lobbies and links kept; the old connection refused; a fresh
/// sign-in of the key a new seat.
#[test]
fn a_second_connection_of_a_key_takes_the_seat_over() {
    run(async {
        let (relay, _) = relay();
        let (a, b) = (client(&relay).await, client(&relay).await);
        let ida = a.sign_in(&relay.id).await;
        let idb = b.sign_in(&relay.id).await;
        assert_eq!(b.resumed(), None, "a fresh sign-in");
        let lobby = lobby_of(&a.req(json!({ "op": "create", "bucket": "r", "max": 4 })).await);
        b.req(json!({ "op": "join", "lobby": lobby })).await;
        a.msg("lobby").await;
        // a -> b: two frames on channel 0; b -> a: one (the relay took one from b's seat)
        a.send(json!({ "t": "peer", "handle": 1, "id": idb }));
        a.write(&packet_frame(FT_DATA, 1, &[1]).unwrap());
        a.write(&packet_frame(FT_DATA, 1, &[2]).unwrap());
        b.msg("peer").await;
        b.next(is_ty(FT_DATA)).await;
        b.next(is_ty(FT_DATA)).await;
        b.send(json!({ "t": "peer", "handle": 1, "id": ida }));
        b.write(&packet_frame(FT_DATA, 1, &[9]).unwrap());
        a.msg("peer").await;
        assert_eq!(unpack(&a.next(is_ty(FT_DATA)).await.body).1, [9]);
        // b's connection drops without a bye: parked; a is told nothing
        b.cut();
        until("b parked", 2000, || parked(&relay) == 1).await;
        a.write(&packet_frame(FT_DATA, 1, &[3]).unwrap());
        a.write(&packet_frame(FT_MATCH, 1, &[4]).unwrap());
        let told = |g: &Got| g.m.as_ref().is_some_and(|m| m["t"] == "lobby" || m["t"] == "peerClosed");
        assert!(a.next_until_closed(200, told).await.is_none(), "a is told no leave");
        // its new connection: it counted one frame taken (the hello's got); the ring from there, named anew
        let b2 = Client::new(relay.port, Some(b.key.clone())).await;
        b2.sign_in_got(&relay.id, Some(1)).await;
        assert_eq!(b2.resumed(), Some(1), "the frames the relay took from the seat");
        let p = b2.msg("peer").await;
        assert_eq!(p["id"], ida);
        let h = p["handle"].as_u64().unwrap() as u32;
        assert_eq!(unpack(&b2.next(is_ty(FT_DATA)).await.body), (h, vec![2]));
        assert_eq!(unpack(&b2.next(is_ty(FT_DATA)).await.body), (h, vec![3]));
        assert_eq!(b2.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida, idb] }));
        assert_eq!(parked(&relay), 0);
        // its link stands: a takes its frame on a's handle for it, told no new peer
        b2.send(json!({ "t": "peer", "handle": 5, "id": ida }));
        b2.write(&packet_frame(FT_DATA, 5, &[8]).unwrap());
        assert_eq!(unpack(&a.next(is_ty(FT_DATA)).await.body).1, [8]);
        assert!(!a.inner.borrow().got.iter().any(|g| g.m.as_ref().is_some_and(|m| m["t"] == "peer")));
        // a third connection while the second is up: the second refused; it took all three, none resent
        let b3 = Client::new(relay.port, Some(b.key.clone())).await;
        b3.sign_in_got(&relay.id, Some(3)).await;
        assert_eq!(b2.refused().await, "resumed elsewhere");
        assert_eq!(b3.resumed(), Some(2));
        b3.msg("lobby").await;
        assert!(b3.next_until_closed(200, is_ty(FT_DATA)).await.is_none(), "nothing to resend");
        // a fresh sign-in of the key: a new seat, the old one's lobby and link gone
        let b4 = Client::new(relay.port, Some(b.key.clone())).await;
        b4.sign_in(&relay.id).await;
        assert_eq!(b3.refused().await, "resumed elsewhere");
        assert_eq!(b4.resumed(), None);
        assert_eq!(a.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida] }));
        assert_eq!(a.msg("peerClosed").await, json!({ "t": "peerClosed", "id": idb }));
        // a count that is no whole number: a bad hello
        let x = client(&relay).await;
        x.send(
            json!({ "t": "hello", "v": RELAY_WIRE_VERSION, "key": b64(&x.raw), "nonce": b64(&[0u8; 32]), "kx": b64(&[9u8; 32]), "got": "1" }),
        );
        assert_eq!(x.refused().await, "bad hello");
        a.end();
        b4.end();
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn a_seat_parked_for_resume_ms_leaves() {
    run(async {
        let t = Rc::new(Cell::new(0.0));
        let clock = t.clone();
        let o = Options {
            host: "127.0.0.1".into(),
            limits: limits(&[("connectionsPerAddress", 100.0), ("silentMs", 1e9)]),
            now: Some(Rc::new(move || clock.get())),
            ..Options::default()
        };
        let relay = start_relay(o).unwrap();
        let (a, b) = (client(&relay).await, client(&relay).await);
        let ida = a.sign_in(&relay.id).await;
        b.sign_in(&relay.id).await;
        let lobby = lobby_of(&a.req(json!({ "op": "create", "bucket": "r", "max": 4 })).await);
        b.req(json!({ "op": "join", "lobby": lobby })).await;
        a.msg("lobby").await;
        b.cut();
        until("b parked", 2000, || parked(&relay) == 1).await;
        t.set(RESUME_MS - 1.0);
        sleep(1200).await;
        assert_eq!(players(&relay), 2, "kept until RESUME_MS");
        t.set(RESUME_MS);
        assert_eq!(a.msg_in("lobby", 3000).await, json!({ "t": "lobby", "lobby": lobby, "members": [ida] }));
        assert_eq!(players(&relay), 1);
        let b2 = Client::new(relay.port, Some(b.key.clone())).await;
        b2.sign_in_got(&relay.id, Some(0)).await;
        assert_eq!(b2.resumed(), None, "no seat to resume");
        a.end();
        b2.end();
        relay.close().await;
    });
}

#[test]
fn channel_1_both_ways_while_the_probes_are_late_and_taken_once_when_sent_both_ways() {
    run(async {
        let t = Rc::new(Cell::new(0.0));
        let clock = t.clone();
        let o = Options {
            host: "127.0.0.1".into(),
            limits: limits(&[("connectionsPerAddress", 100.0)]),
            now: Some(Rc::new(move || clock.get())),
            ..Options::default()
        };
        let r = start_relay(o).unwrap();
        let (a, b, _c, ids) = trio(&r).await;
        b.open_udp("127.0.0.1").await;
        b.probe(true);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.send(json!({ "t": "peer", "handle": 1, "id": ids[1] }));
        for x in [1, 2] {
            a.write(&packet_frame(FT_MATCH, 1, &[x]).unwrap());
        }
        assert_eq!(match_bytes(&b.next(is_ty(FT_MATCH)).await), [1]);
        assert_eq!(b.datagram(|d| d.kind == UDP_MATCH).await.bytes, [2]);
        // the probe late: by UDP and on TCP
        t.set(t.get() + UDP_PROBE_MS + UDP_LATE_MS + 1.0);
        a.write(&packet_frame(FT_MATCH, 1, &[3]).unwrap());
        assert_eq!(b.datagram(|d| d.kind == UDP_MATCH).await.bytes, [3]);
        assert_eq!(match_bytes(&b.next(is_ty(FT_MATCH)).await), [3], "on TCP too");
        // a probe again: by UDP alone
        b.probe(true);
        b.datagram(|d| d.kind == UDP_PROBE_ACK).await;
        a.write(&packet_frame(FT_MATCH, 1, &[4]).unwrap());
        assert_eq!(b.datagram(|d| d.kind == UDP_MATCH).await.bytes, [4]);
        assert!(b.next_until_closed(300, is_ty(FT_MATCH)).await.is_none(), "not on TCP");
        // b sends one packet both ways: a takes it once
        b.send(json!({ "t": "peer", "handle": 3, "id": ids[0] }));
        sleep(30).await;
        b.udp_send(UDP_MATCH, 3, &[7]);
        b.write(&packet_frame(FT_MATCH, 3, &[7]).unwrap());
        a.msg("peer").await;
        assert_eq!(match_bytes(&a.next(is_ty(FT_MATCH)).await), [7]);
        assert!(a.next_until_closed(300, is_ty(FT_MATCH)).await.is_none(), "taken once");
        // (the same bytes again, one way: a packet of its own)
        b.write(&packet_frame(FT_MATCH, 3, &[7]).unwrap());
        assert_eq!(match_bytes(&a.next(is_ty(FT_MATCH)).await), [7]);
        r.close().await;
    });
}

#[test]
fn a_disconnect_in_each_state() {
    run(async {
        let (relay, logs) = relay();
        let a = client(&relay).await;
        a.end();
        let b = client(&relay).await;
        b.hello();
        b.msg("challenge").await;
        b.end();
        let (c, d) = (client(&relay).await, client(&relay).await);
        c.sign_in(&relay.id).await;
        d.sign_in(&relay.id).await;
        let lobby = lobby_of(&c.req(json!({ "op": "create", "bucket": "x", "max": 4 })).await);
        d.req(json!({ "op": "join", "lobby": lobby })).await;
        c.msg("lobby").await;
        d.send(json!({ "t": "peer", "handle": 1, "id": key_id(&c.raw) }));
        d.write(&packet_frame(FT_DATA, 1, &[1]).unwrap());
        c.next(is_ty(FT_DATA)).await;
        c.end();
        assert_eq!(d.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "closed": "destroyed" }));
        assert_eq!(d.msg("peerClosed").await, json!({ "t": "peerClosed", "id": key_id(&c.raw) }));
        d.end();
        until("all gone", 2000, || relay.with_core(|c, _| c.players() == 0 && c.lobbies() == 0)).await;
        assert!(logs.borrow().iter().any(|l| l.ends_with(" left")), "the ends logged");
        relay.close().await;
    });
}

#[test]
fn limits_on_the_wire_connections_per_address_control_frames_silence_a_slow_reader() {
    run(async {
        let (r, logs) = relay_with(limits(&[
            ("connectionsPerAddress", 2.0),
            ("controlPerSecond", 10.0),
            ("silentMs", 300.0),
            ("sendBufferBytes", 64.0 * 1024.0),
            ("messageBytesPerSecond", 1e9),
        ]));
        let (a, b) = (client(&r).await, client(&r).await);
        let c = client(&r).await;
        assert_eq!(c.refused().await, "too many connections from this address");
        a.end();
        b.end();
        sleep(300).await;
        let e = client(&r).await;
        e.sign_in(&r.id).await;
        for _ in 0..20 {
            e.send(json!({ "t": "ping" }));
        }
        assert_eq!(e.refused().await, "too many messages");
        let f = client(&r).await;
        f.sign_in(&r.id).await;
        let t0 = Instant::now();
        assert_eq!(f.refused().await, "silent");
        assert!(t0.elapsed() < Duration::from_secs(2));
        // a reader that reads nothing while another sends it data (once the relay has seen the two before go)
        sleep(200).await;
        let (g, h) = (client(&r).await, client(&r).await);
        g.sign_in(&r.id).await;
        let idh = h.sign_in(&r.id).await;
        let gl = lobby_of(&g.req(json!({ "op": "create", "bucket": "s", "max": 2 })).await);
        h.req(json!({ "op": "join", "lobby": gl })).await;
        g.msg("lobby").await;
        h.pause();
        g.send(json!({ "t": "peer", "handle": 1, "id": idh }));
        let big = vec![0u8; 1170];
        let keep_g = g.clone();
        let keep = tokio::task::spawn_local(async move {
            loop {
                sleep(100).await;
                keep_g.send(json!({ "t": "pong" }));
            }
        });
        let mut i = 0;
        while i < 4000 && live(&r) == 2 {
            g.write(&packet_frame(FT_DATA, 1, &big).unwrap());
            if i % 100 == 99 {
                sleep(5).await;
            }
            i += 1;
        }
        until("the slow reader dropped", 3000, || live(&r) == 1).await;
        keep.abort();
        assert_eq!(parked(&r), 1, "the slow reader's seat kept (the silent one, in no lobby, left)");
        assert!(g.next_until_closed(200, |m| m.m.as_ref().is_some_and(|m| m["t"] == "peerClosed")).await.is_none());
        assert!(
            logs.borrow().iter().any(|l| l.ends_with(&format!("{idh} dropped: reads too slowly"))),
            "dropped as a slow reader: {:?}",
            logs.borrow()
        );
        g.end();
        h.end();
        r.close().await;
    });
}

#[test]
fn a_reader_dropped_while_the_relay_is_blocked_writing_to_it_has_its_socket_freed() {
    run(async {
        // the kernel's buffers fill before the relay's own (16 MB) does: its writer is blocked when the drop comes
        let (r, logs) = relay_with(limits(&[
            ("sendBufferBytes", 16.0 * 1024.0 * 1024.0),
            ("messageBytesPerSecond", 1e12),
            ("matchBytesPerSecond", 1e12),
        ]));
        let (g, h) = (client(&r).await, client(&r).await);
        g.sign_in(&r.id).await;
        let idh = h.sign_in(&r.id).await;
        let gl = lobby_of(&g.req(json!({ "op": "create", "bucket": "s", "max": 2 })).await);
        h.req(json!({ "op": "join", "lobby": gl })).await;
        g.msg("lobby").await;
        h.pause();
        g.send(json!({ "t": "peer", "handle": 1, "id": idh }));
        let big = vec![0u8; 1170];
        let mut i = 0;
        while i < 60000 && live(&r) == 2 {
            g.write(&packet_frame(FT_DATA, 1, &big).unwrap());
            if i % 100 == 99 {
                sleep(1).await;
            }
            i += 1;
        }
        until("the slow reader dropped", 10000, || live(&r) == 1).await;
        assert!(
            logs.borrow().iter().any(|l| l.ends_with(&format!("{idh} dropped: reads too slowly"))),
            "dropped as a slow reader: {:?}",
            logs.borrow()
        );
        // it still reads nothing: the client sees the end, or its writes fail on the reset
        let gone = || h.closed() || h.inner.borrow().write_failed;
        for _ in 0..150 {
            if gone() {
                break;
            }
            h.send(json!({ "t": "pong" }));
            sleep(20).await;
        }
        assert!(gone(), "the relay still holds the socket of a reader that reads nothing");
        g.end();
        h.end();
        r.close().await;
    });
}

#[test]
fn json_as_js_reads_it_a_lone_surrogate_and_a_number_past_f64() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.sign_in(&relay.id).await;
        let res = |id: i64| move |g: &Got| g.m.as_ref().is_some_and(|m| m["t"] == "res" && m["id"] == id);
        // (JSON.parse reads 1e400 as Infinity, which no check takes)
        a.write(&frame(FT_JSON, br#"{"t":"req","id":1,"op":"create","bucket":"s","max":1e400}"#).unwrap());
        assert_eq!(a.next(res(1)).await.m.unwrap()["error"], "EOS_InvalidParameters");
        a.write(
            &frame(FT_JSON, br#"{"t":"req","id":2,"op":"create","bucket":"s","max":2,"listed":{"at":-1e999}}"#)
                .unwrap(),
        );
        assert_eq!(a.next(res(2)).await.m.unwrap()["error"], "EOS_InvalidParameters");
        // a lone surrogate in a text is a text
        a.write(&frame(FT_JSON, br#"{"t":"req","id":3,"op":"create","bucket":"s\ud83d","max":2}"#).unwrap());
        assert_eq!(a.next(res(3)).await.m.unwrap()["ok"], true);
        a.write(
            &frame(FT_JSON, br#"{"t":"req","id":4,"op":"create","bucket":"\ud83d\udc00\ud83d\ude00","max":2}"#)
                .unwrap(),
        );
        assert_eq!(a.next(res(4)).await.m.unwrap()["ok"], true);
        assert!(!a.closed());
        a.end();
        relay.close().await;
    });
}

/// A client's link to a host core: its handle told, its knock.
fn host_link(c: &Client, handle: u32, host: &str) {
    c.send(json!({ "t": "peer", "handle": handle, "id": host }));
    c.write(&packet_frame(FT_DATA, handle, &[FRAME_OPEN]).unwrap());
}

/// A message to a host core on its handle, split as a game splits it (the oracle game's hello and compat).
fn host_say(c: &Client, handle: u32, t: &str, body: Value) {
    let mut m = body;
    if t == "hello" {
        m["proto"] = json!(PROTOCOL);
        m["app"] = json!("test");
        m["name"] = json!(format!("N{handle}{}", c.raw[0]));
    }
    if t == "lobby.enter" || t == "lobby.join" {
        m["compat"] = json!("ctest.1");
    }
    m["t"] = json!(t);
    for piece in split_message(m.to_string().as_bytes(), 1170) {
        c.write(&packet_frame(FT_DATA, handle, &piece).unwrap());
    }
}

/// The messages a client has had from the host core `host` (its handle told by a peer message first), joined.
fn host_msgs(i: &Inner, host: &str) -> Vec<Value> {
    let (mut handle, mut joiner, mut out) = (None, MessageJoiner::new(), Vec::new());
    for g in &i.got {
        if let Some(m) = &g.m
            && m["t"] == "peer"
            && m["id"] == host
        {
            handle = m["handle"].as_u64().map(|h| h as u32);
        }
        if g.ty != FT_DATA {
            continue;
        }
        let p = read_packet(&g.body).unwrap();
        if Some(p.handle) != handle {
            continue;
        }
        if let Frame::Message(b) = joiner.take(p.bytes) {
            out.push(serde_json::from_slice(&b).unwrap());
        }
    }
    out
}

/// The last message of kind `t` a client has had from the host core, waited for.
async fn host_msg(c: &Client, host: &str, t: &str) -> Value {
    let got = c.wait(15_000, |i| host_msgs(i, host).into_iter().rfind(|m| m["t"] == t)).await;
    got.unwrap_or_else(|| panic!("no {t} from {host}; had {}", c.had()))
}

#[test]
fn a_match_refereed_on_the_relay_over_the_wire() {
    run(async {
        let (relay, _) = relay();
        let (a, b, c) = (client(&relay).await, client(&relay).await, client(&relay).await);
        let _ = [a.sign_in(&relay.id).await, b.sign_in(&relay.id).await, c.sign_in(&relay.id).await];
        // a's custom lobby the relay hosts, b in it; each linked to its core
        let r = a.req(json!({ "op": "create", "bucket": "u", "max": 4, "host": true })).await;
        let (lobby, host) = (lobby_of(&r), r["value"]["host"].as_str().unwrap().to_string());
        assert_eq!(host, format!("{lobby}.host"));
        // (b joins by its short code, answered with the lobby's id)
        let short = r["value"]["short"].as_str().expect("a short code").to_string();
        let j = b.req(json!({ "op": "join", "lobby": short })).await;
        assert_eq!(j["value"]["lobby"], json!(lobby), "{j}");
        for x in [&a, &b] {
            host_link(x, 7, &host);
            host_say(x, 7, "hello", json!({}));
            host_msg(x, &host, "welcome").await;
        }
        host_say(&a, 7, "lobby.enter", json!({}));
        host_msg(&a, &host, "lobby.state").await;
        host_say(&b, 7, "lobby.join", json!({ "lobby": lobby }));
        host_msg(&b, &host, "lobby.state").await;
        // c, in no lobby of it, is refused its core
        host_link(&c, 7, &host);
        assert_eq!(c.msg("peerClosed").await, json!({ "t": "peerClosed", "id": host }));
        // the match to its go
        host_say(&a, 7, "lobby.setTeamStyle", json!({ "style": 0 }));
        host_say(&b, 7, "lobby.setReady", json!({ "on": true }));
        sleep(100).await;
        host_say(&a, 7, "lobby.startMatch", json!({}));
        for x in [&a, &b] {
            host_msg(x, &host, "pm.begin").await;
            host_say(x, 7, "pm.lock", json!({ "character": "shaggy", "skin": "" }));
        }
        for x in [&a, &b] {
            host_msg(x, &host, "pm.all").await;
            host_say(x, 7, "pm.perks", json!({ "perks": [] }));
        }
        let mut slots = Vec::new();
        for x in [&a, &b] {
            slots.push(host_msg(x, &host, "match.start").await["slot"].as_u64().unwrap() as u8);
            host_say(x, 7, "match.loaded", json!({}));
        }
        let go = host_msg(&a, &host, "match.go").await;
        let rid = go["referee"].as_str().unwrap().to_string();
        assert!(rid.starts_with(&format!("{host}.m")), "the relay's referee of the core's match: {go}");
        assert_eq!(relay.with_core(|c, _| c.matches()), 1);
        let (sa, sb) = (slots[0], slots[1]);
        // (the peer-handle rule holds for the referee's id; the lobby-sharing rule does not stand in its way)
        for x in [&a, &b, &c] {
            x.send(json!({ "t": "peer", "handle": 9, "id": rid }));
        }
        a.write(&packet_frame(FT_MATCH, 9, &input(sa, 2, 11, 2)).unwrap());
        b.write(&packet_frame(FT_MATCH, 9, &input(sb, 2, 22, 2)).unwrap());
        c.write(&packet_frame(FT_MATCH, 9, &input(sa, 4, 33, 1)).unwrap());
        let (mut ha, mut hb, mut hc) = (0, 0, 0);
        let want = |r: &BTreeMap<(u8, u32), i32>| {
            r.get(&(sa, 2)) == Some(&11)
                && r.get(&(sa, 3)) == Some(&11)
                && r.get(&(sb, 2)) == Some(&22)
                && r.get(&(sb, 3)) == Some(&22)
        };
        until("both ruled", 2000, || want(&wire_ruled(&a, &rid, &mut ha)) && want(&wire_ruled(&b, &rid, &mut hb)))
            .await;
        assert!(ha > 0 && hb > 0, "the referee's handle told first");
        // (a clock packet or two later: still nothing of c's)
        sleep(600).await;
        let ra = wire_ruled(&a, &rid, &mut ha);
        assert!(ra.get(&(sa, 4)).is_none_or(|&v| v == -1), "the packet of a player not in the match dropped");
        assert!(wire_ruled(&c, &rid, &mut hc).is_empty(), "nothing to a player not in the match");
        // the leader leaves its lobby: its core, its match and the lobby go, b told each
        a.req(json!({ "op": "leave", "lobby": lobby })).await;
        assert_eq!(b.msg("peerClosed").await, json!({ "t": "peerClosed", "id": rid }));
        assert_eq!(b.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "closed": "destroyed" }));
        until("no match, core or lobby", 2000, || {
            relay.with_core(|c, _| c.matches() == 0 && c.hosts().is_empty() && c.lobbies() == 0)
        })
        .await;
        a.end();
        b.end();
        c.end();
        until("all gone", 2000, || players(&relay) == 0).await;
        relay.close().await;
    });
}

#[test]
fn a_client_of_the_old_wire_is_refused_with_the_reason() {
    run(async {
        let (relay, _) = relay();
        let a = client(&relay).await;
        a.hello_with(json!(1), None, None);
        assert_eq!(a.refused().await, format!("wire version not {RELAY_WIRE_VERSION}"));
        relay.close().await;
    });
}

#[test]
#[ignore = "listens on every address: Windows asks for a firewall rule per new test build; run with --ignored"]
fn a_relay_on_all_addresses_no_host_and_its_close() {
    run(async {
        let r = start_relay(Options::default()).unwrap();
        let a = Client::new(r.port, None).await;
        a.sign_in(&r.id).await;
        r.close().await;
        assert_eq!(a.refused().await, "relay stopping");
    });
}

#[test]
fn a_key_file_made_then_read_back_the_same_id() {
    let dir = std::env::temp_dir().join(format!("mvs-relay-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("mvs-relay.key");
    let _ = std::fs::remove_file(&f);
    let made = mvs_relay::server::load_key(Some(&f)).unwrap();
    let read = mvs_relay::server::load_key(Some(&f)).unwrap();
    assert_eq!(made.verifying_key(), read.verifying_key());
    assert!(std::fs::read_to_string(&f).unwrap().starts_with("-----BEGIN PRIVATE KEY-----"));
    std::fs::remove_file(&f).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}
