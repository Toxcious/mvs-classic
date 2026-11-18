//! The relay's client side against a relay of another build (the oracle's relay), on 127.0.0.1:
//! the handshake and its refusals, a lobby's create, search, join and leave, an invite, packets on channel 0 (TCP) and
//! channel 1 (UDP once probed, TCP before), a closed link and the online list. (A match the oracle relay referees is
//! opened by its host's match.open, an op this relay has not: here a match is a host core's, the relay's own, so no
//! build but this one has one to cross.) The oracle relay predates the sealed wire (wire 7, the port's own): this check
//! runs only against a relay that seals. Ignored unless run by the oracle's relay cross check, which starts that relay
//! (connectionsPerAddress 100) and gives its port and id: MVS_RELAY_PORT, MVS_RELAY_ID.

mod client;

use client::*;
use mvs_protocol::relay_wire::{FT_DATA, FT_MATCH, RELAY_WIRE_VERSION, UDP_MATCH, UDP_PROBE_ACK, packet_frame};
use serde_json::json;

#[test]
#[ignore = "run by the oracle's relay cross check against a relay it starts"]
fn against_an_outside_relay() {
    let port: u16 = std::env::var("MVS_RELAY_PORT").expect("MVS_RELAY_PORT").parse().unwrap();
    let rid_relay = std::env::var("MVS_RELAY_ID").expect("MVS_RELAY_ID");
    run(async move {
        // the handshake's refusals
        let x = Client::new(port, None).await;
        x.hello_with(json!(RELAY_WIRE_VERSION - 1), None, None);
        assert_eq!(x.refused().await, format!("wire version not {RELAY_WIRE_VERSION}"));
        let x = Client::new(port, None).await;
        let nonce = x.hello();
        let c = x.msg("challenge").await;
        let mut t = x.transcript(nonce, &c, None);
        t.relay_nonce[0] ^= 1;
        x.prove(&t);
        assert_eq!(x.refused().await, "bad signature");

        // three players, a's lobby listed
        let (a, b, c) = (Client::new(port, None).await, Client::new(port, None).await, Client::new(port, None).await);
        let ida = a.sign_in(&rid_relay).await;
        let idb = b.sign_in(&rid_relay).await;
        let idc = c.sign_in(&rid_relay).await;
        let made =
            a.req(json!({ "op": "create", "bucket": "cross", "max": 4, "listed": { "at": 3, "info": "x" } })).await;
        let lobby = lobby_of(&made);
        assert_eq!(
            b.req(json!({ "op": "search", "bucket": "cross", "limit": 5 })).await["value"],
            json!({ "lobbies": [{ "id": lobby, "owner": ida, "members": 1, "max": 4, "at": 3, "info": "x" }] })
        );
        assert_eq!(
            b.req(json!({ "op": "join", "lobby": lobby })).await["value"],
            json!({ "owner": ida, "members": [ida, idb] })
        );
        assert_eq!(a.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida, idb] }));
        c.req(json!({ "op": "join", "lobby": lobby })).await;
        a.msg("lobby").await;
        b.msg("lobby").await;
        assert_eq!(b.req(json!({ "op": "join", "lobby": "lnone" })).await["error"], "EOS_NotFound");

        // an invite; the online list
        a.req(json!({ "op": "invite", "to": idb, "payload": "come" })).await;
        assert_eq!(b.msg("invite").await, json!({ "t": "invite", "from": ida, "payload": "come" }));
        let presence =
            json!({ "name": "Pat", "icon": "ProfileIcon_Test", "busy": false, "listed": true, "invites": true });
        assert_eq!(c.req(json!({ "op": "presence", "presence": presence })).await["ok"], true);
        assert_eq!(
            a.req(json!({ "op": "who", "limit": 50 })).await["value"],
            json!({ "players": [{ "id": idc, "name": "Pat", "icon": "ProfileIcon_Test", "busy": false }] })
        );

        // channel 0 on TCP, the handle told first; channel 1 on TCP until b probes, then by UDP
        a.send(json!({ "t": "peer", "handle": 7, "id": idb }));
        a.write(&packet_frame(FT_DATA, 7, &[1, 2, 3]).unwrap());
        let peer = b.msg("peer").await;
        assert_eq!(peer["id"], ida);
        let h = peer["handle"].as_u64().unwrap() as u32;
        assert_eq!(unpack(&b.next(is_ty(FT_DATA)).await.body), (h, vec![1, 2, 3]));
        a.write(&packet_frame(FT_MATCH, 7, &[4]).unwrap());
        assert_eq!(unpack(&b.next(is_ty(FT_MATCH)).await.body), (h, vec![4]));
        b.open_udp("127.0.0.1").await;
        b.probe(true);
        assert_eq!(b.datagram(|d| d.kind == UDP_PROBE_ACK).await.token, b.token());
        a.write(&packet_frame(FT_MATCH, 7, &[5]).unwrap());
        let d = b.datagram(|d| d.kind == UDP_MATCH).await;
        assert_eq!((d.handle, d.bytes), (h, vec![5]));
        // b -> a by UDP (a never probed: it gets it on TCP)
        b.send(json!({ "t": "peer", "handle": 2, "id": ida }));
        sleep(30).await;
        b.udp_send(UDP_MATCH, 2, &[6]);
        let pa = a.msg("peer").await;
        assert_eq!(unpack(&a.next(is_ty(FT_MATCH)).await.body), (pa["handle"].as_u64().unwrap() as u32, vec![6]));
        // b closes the link: a told
        b.send(json!({ "t": "closePeer", "id": ida }));
        assert_eq!(a.msg("peerClosed").await, json!({ "t": "peerClosed", "id": idb }));

        // a member leaves, then the owner's end destroys the lobby
        c.req(json!({ "op": "leave", "lobby": lobby })).await;
        for x in [&a, &b] {
            assert_eq!(x.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "members": [ida, idb] }));
        }
        a.end();
        assert_eq!(b.msg("lobby").await, json!({ "t": "lobby", "lobby": lobby, "closed": "destroyed" }));
        b.send(json!({ "t": "ping" }));
        b.msg("pong").await;
        b.end();
        c.end();
    });
}
