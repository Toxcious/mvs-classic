//! The sealed relay wire's project vector (relay_seal): a whole start's exchange from fixed keys, nonces and kx secrets,
//! and what it seals. The bytes were made by this code and checked once against another implementation (Node's
//! crypto, OpenSSL: Ed25519, X25519, HKDF-SHA256, ChaCha20-Poly1305), so a change to any step of the derivation or
//! the layouts fails here.

use ed25519_dalek::SigningKey;
use mvs_protocol::relay_seal::{KxSecret, derive, shared, split, transcript_hash};
use mvs_protocol::relay_wire::{
    ClientMessage, FT_JSON, Side, Transcript, UDP_MATCH, handshake_bytes, json_frame, key_id, read_udp_body,
};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The vector's inputs: Ed25519 seeds, nonces and kx secrets of repeated bytes.
fn transcript() -> (Transcript, [u8; 32]) {
    let client_key = SigningKey::from_bytes(&[0x11; 32]).verifying_key().to_bytes();
    let relay_key = SigningKey::from_bytes(&[0x22; 32]).verifying_key().to_bytes();
    let client_kx = KxSecret::from_bytes([0x55; 32]).public();
    let relay_kx = KxSecret::from_bytes([0x66; 32]).public();
    let t =
        Transcript { client_key, relay_key, client_nonce: [0x33; 32], relay_nonce: [0x44; 32], client_kx, relay_kx };
    let s = shared(KxSecret::from_bytes([0x55; 32]), &relay_kx).expect("a usable kx");
    assert_eq!(Some(s), shared(KxSecret::from_bytes([0x66; 32]), &client_kx), "both sides' secret");
    (t, s)
}

#[test]
fn the_project_vector() {
    let (t, s) = transcript();
    let k = derive(&s, &t);
    let (mut tx, _, _) = split(&k, Side::Client);
    let mut ping = Vec::new();
    let plain = json_frame(&ClientMessage::Ping).unwrap();
    tx.seal_frame(&plain, &mut ping).unwrap();
    tx.seal_frame(&plain, &mut ping).unwrap();
    let mut d = Vec::new();
    for _ in 0..6 {
        tx.datagram_into(&mut d, &[0x77; 8], UDP_MATCH, &[&1u32.to_be_bytes(), b"abc"]);
    }
    let got = [
        ("client key", hex(&t.client_key)),
        ("relay key", hex(&t.relay_key)),
        ("client kx", hex(&t.client_kx)),
        ("relay kx", hex(&t.relay_kx)),
        ("T", hex(&t.bytes())),
        ("relay signs", hex(&handshake_bytes(Side::Relay, &t)[..20])),
        ("shared", hex(&s)),
        ("transcript hash", hex(&transcript_hash(&t))),
        ("c2r", hex(&k.c2r)),
        ("r2c", hex(&k.r2c)),
        ("ping 0 and 1", hex(&ping)),
        ("datagram 5", hex(&d)),
        ("client id", key_id(&t.client_key)),
    ];
    for (name, v) in &got {
        println!("{name}: {v}");
    }
    for ((name, v), want) in got.iter().zip(WANT) {
        assert_eq!(v, want, "{name}");
    }
}

#[test]
fn the_relay_opens_the_vector() {
    let (t, s) = transcript();
    let k = derive(&s, &t);
    let (mut tx, _, _) = split(&k, Side::Client);
    let (_, rx, mut rx_d) = split(&k, Side::Relay);
    let mut w = Vec::new();
    let plain = json_frame(&ClientMessage::Ping).unwrap();
    tx.seal_frame(&plain, &mut w).unwrap();
    tx.seal_frame(&plain, &mut w).unwrap();
    let mut r = mvs_protocol::relay_seal::SealedReader::new(rx, Vec::new());
    let frames = r.push(&w);
    assert_eq!(frames.len(), 2);
    assert!(frames.iter().all(|f| f.ty == FT_JSON && f.body == plain[5..]));
    let mut d = Vec::new();
    tx.datagram_into(&mut d, &[0x77; 8], UDP_MATCH, &[&1u32.to_be_bytes(), b"abc"]);
    let g = read_udp_body(rx_d.open(&mut d).unwrap()).unwrap();
    assert_eq!((g.kind, g.handle, g.bytes), (UDP_MATCH, 1, &b"abc"[..]));
}

const WANT: [&str; 13] = [
    "d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737",
    "a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f0",
    "38ab664bd86f77d7e66bdd9ae0792913a94fd8b33a1260027e4b46c1f4884c67",
    "219e4d800da968d2a5fcb009c784f4746c7138edb9ee4844b739e830b05cf424",
    "d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f03333333333333333333333333333333333333333333333333333333333333333444444444444444444444444444444444444444444444444444444444444444438ab664bd86f77d7e66bdd9ae0792913a94fd8b33a1260027e4b46c1f4884c67219e4d800da968d2a5fcb009c784f4746c7138edb9ee4844b739e830b05cf424",
    "6d76732d72656c61792d76323a72656c617900d0",
    "559ed2171b98fa0c0f417b78b5c5a43e971990d7605860038d940284d3245142",
    "924b4efe890ae128f47112c8db356f6e6c30abb2b8e09d333479272e815d228d",
    "febb105a2638b3f41915a24fea22c35b6fdf792a701766cfba72cefaade26947",
    "767ac366ec56055ccb17a01a6c24b44c7397e26a3e20037b728f30b04a91e387",
    "0000001d7482951b6ce6dd9acf11c5874ecaac7a955b662f88b01d60ccfdaaf7830000001d60cacd88d86b8095bfe1f45a4c2b6b90ca90e13e48308ea417efd92b96",
    "777777777777777700000000000000053baf2fa599478e36fae7c943e3433e0af8d3f70edad2bc21",
    "r10ba682c8ad13513971e8b56881aab8b",
];
