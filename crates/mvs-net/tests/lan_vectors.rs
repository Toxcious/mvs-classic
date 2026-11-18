//! The direct code and the LAN announcement against the oracle's golden vectors (`directCode` and `lanAnnounce` of
//! `protocol.json`, written by the oracle's protocol vectors from its direct code and LAN announcement).
//! A test that finds no vectors says so and passes.
//!
//! The vectors write a NaN or -0 as `{"$f64": bits}`, bytes as `{"$bytes": hex}`; an interface list is read in the
//! document's order (a map in the oracle: the order the system lists them), so the file is parsed a second time into
//! ordered maps.

use indexmap::IndexMap;
use mvs_net::lan::announce::{
    ANNOUNCE_EVERY_MS, ANNOUNCE_MAGIC, ANNOUNCE_MAX_BYTES, ANNOUNCE_V, Announcement, HEARD_FOR_MS, HEARD_MAX,
    SENDER_LOBBIES, SENDER_PACKETS, SENDERS_MAX, broadcast_targets, decode_announcement, encode_announcement,
};
use mvs_net::lan::direct_code::{
    DirectCode, Interfaces, Local, Nic, base32, decode_direct_code, direct_text, encode_direct_code, local_address,
    looks_direct, pick_addresses, port_of, unbase32,
};
use mvs_protocol::vectors;
use serde::Deserialize;
use serde_json::{Value, json};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A vector number: a plain one, or `{"$f64": bits}`.
fn num(v: &Value) -> f64 {
    match v.get("$f64").and_then(Value::as_str) {
        Some(bits) => f64::from_bits(u64::from_str_radix(bits, 16).expect("bits")),
        None => v.as_f64().expect("a number"),
    }
}

/// A number as the typed field the Rust takes (None: no such value fits, so the oracle's refusal is the answer).
fn whole(v: &Value) -> Option<u32> {
    let n = num(v);
    (n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n)).then_some(n as u32)
}

#[derive(Deserialize)]
struct NetCase {
    #[serde(rename = "in")]
    nets: IndexMap<String, Value>,
    loopback: bool,
    #[serde(default)]
    addresses: Vec<String>,
    out: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(rename = "directCode")]
    direct: DirectPart,
    #[serde(rename = "lanAnnounce")]
    lan: LanPart,
}

#[derive(Deserialize)]
struct DirectPart {
    #[serde(rename = "pickAddresses")]
    pick: Vec<NetCase>,
}

#[derive(Deserialize)]
struct LanPart {
    #[serde(rename = "broadcastTargets")]
    targets: Vec<NetCase>,
}

fn interfaces(nets: &IndexMap<String, Value>) -> Interfaces {
    nets.iter()
        .map(|(name, list)| {
            let nics = list
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| Nic {
                            address: x["address"].as_str().unwrap_or_default().to_owned(),
                            netmask: x["netmask"].as_str().unwrap_or_default().to_owned(),
                            v4: x["family"] == json!("IPv4") || x["family"] == json!(4),
                            internal: x["internal"].as_bool().unwrap_or(false),
                        })
                        .collect()
                })
                .unwrap_or_default();
            (name.clone(), nics)
        })
        .collect()
}

fn ordered() -> Option<File> {
    let path = vectors::dir().join("protocol.json");
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).expect("the vectors parse"))
}

#[test]
fn direct_code() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let d = &v["directCode"];
    let (mut wrote, mut threw) = (0, 0);
    for (i, x) in d["encodeDirectCode"].as_array().unwrap().iter().enumerate() {
        let inp = &x["in"];
        let loopback = x["loopback"].as_bool().unwrap();
        let port = port_of(num(&inp["port"]));
        let c = port.map(|port| DirectCode {
            addresses: inp["addresses"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect(),
            port,
            fingerprint: inp["fingerprint"].as_str().unwrap().to_owned(),
            lobby: inp["lobby"].as_str().unwrap().to_owned(),
        });
        let got =
            c.map_err(|e| e.to_string()).and_then(|c| encode_direct_code(&c, loopback).map_err(|e| e.to_string()));
        match (got, x.get("out"), x.get("throws")) {
            (Ok(g), Some(w), _) => {
                assert_eq!(&json!(g), w, "encodeDirectCode {i}: {x}");
                wrote += 1;
            }
            (Err(g), _, Some(w)) => {
                assert_eq!(&json!(g), w, "encodeDirectCode {i}: {x}");
                threw += 1;
            }
            // (the deviation of direct_code.rs encode_direct_code: a unicast address outside the private ranges is
            // written, the pick choosing which of the host's are; the oracle refuses it)
            (Ok(_), None, Some(w))
                if w.as_str().and_then(|w| w.strip_prefix("a direct code cannot hold the address ")).is_some_and(
                    |a| {
                        a.parse::<std::net::Ipv4Addr>().is_ok_and(|a| {
                            let o = a.octets();
                            o[0] != 0 && o[0] != 127 && o[0] < 224
                        })
                    },
                ) =>
            {
                wrote += 1;
            }
            (g, ..) => panic!("encodeDirectCode {i}: got {g:?}, want {x}"),
        }
    }
    let mut read = 0;
    for (i, x) in d["decodeDirectCode"].as_array().unwrap().iter().enumerate() {
        let got =
            decode_direct_code(x["in"].as_str().unwrap_or_default(), &Local::new(x["loopback"].as_bool().unwrap()));
        let got = match got {
            Ok(c) => {
                json!({"ok": true, "code": {"addresses": c.addresses, "port": c.port, "fingerprint": c.fingerprint, "lobby": c.lobby}})
            }
            Err(why) => json!({"ok": false, "why": why.log_text()}),
        };
        assert_eq!(got, x["out"], "decodeDirectCode {i}: {}", x["in"]);
        read += usize::from(got["ok"] == json!(true));
    }
    for (i, x) in d["base32"].as_array().unwrap().iter().enumerate() {
        let text = x["text"].as_str().unwrap();
        if let Some(b) = x.get("bytes").and_then(Value::as_str) {
            assert_eq!(base32(&hex(b)), text, "base32 {i}: {x}");
        }
        let back = unbase32(text).map_or(Value::Null, |b| json!({"$bytes": to_hex(&b)}));
        assert_eq!(back, x["back"], "unbase32 {i}: {x}");
    }
    for (i, x) in d["lanAddress"].as_array().unwrap().iter().enumerate() {
        let got = local_address(x["in"][0].as_str().unwrap(), &Local::new(x["in"][1].as_bool().unwrap()));
        assert_eq!(got, x["out"].as_bool().unwrap(), "lanAddress {i}: {x}");
    }
    for (i, x) in d["looksDirect"].as_array().unwrap().iter().enumerate() {
        assert_eq!(looks_direct(x["in"].as_str().unwrap()), x["out"].as_bool().unwrap(), "looksDirect {i}: {x}");
    }
    for (i, x) in d["directText"].as_array().unwrap().iter().enumerate() {
        assert_eq!(direct_text(x["in"].as_str().unwrap()), x["out"].as_str().unwrap(), "directText {i}: {x}");
    }
    let f = ordered().expect("the vectors are there");
    for (i, x) in f.direct.pick.iter().enumerate() {
        let got = pick_addresses(&interfaces(&x.nets), x.loopback);
        // (the deviation of direct_code.rs pick_addresses: a virtual network adapter's address before a VM's private
        // one; the oracle ranks them together)
        if !x.loopback && x.nets.keys().any(|n| n.to_lowercase().contains("tailscale")) {
            let (mut g, mut w) = (got.clone(), x.out.clone());
            g.sort();
            w.sort();
            assert_eq!(g, w, "pickAddresses {i}: the same addresses: {:?}", x.nets);
            continue;
        }
        assert_eq!(got, x.out, "pickAddresses {i}: {:?}", x.nets);
    }
    eprintln!("direct code: {wrote} written, {threw} refused, {read} read, {} picks", f.direct.pick.len());
}

#[test]
fn lan_announce() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let l = &v["lanAnnounce"];
    let c = &l["consts"];
    let magic: Vec<u8> = c["ANNOUNCE_MAGIC"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
    assert_eq!(magic, ANNOUNCE_MAGIC);
    assert_eq!(c["ANNOUNCE_V"].as_u64(), Some(u64::from(ANNOUNCE_V)));
    assert_eq!(c["ANNOUNCE_MAX_BYTES"].as_u64(), Some(ANNOUNCE_MAX_BYTES as u64));
    assert_eq!(c["ANNOUNCE_EVERY_MS"].as_f64(), Some(ANNOUNCE_EVERY_MS));
    assert_eq!(c["HEARD_FOR_MS"].as_f64(), Some(HEARD_FOR_MS));
    assert_eq!(c["HEARD_MAX"].as_u64(), Some(HEARD_MAX as u64));
    assert_eq!(c["SENDER_LOBBIES"].as_u64(), Some(SENDER_LOBBIES as u64));
    assert_eq!(c["SENDER_PACKETS"].as_u64(), Some(u64::from(SENDER_PACKETS)));
    assert_eq!(c["SENDERS_MAX"].as_u64(), Some(SENDERS_MAX as u64));

    let (mut made, mut refused) = (0, 0);
    for (i, x) in l["encodeAnnouncement"].as_array().unwrap().iter().enumerate() {
        let inp = &x["in"];
        // (a members or max the typed field cannot hold is one the oracle refuses too)
        let got = match (whole(&inp["members"]), whole(&inp["max"])) {
            (Some(members), Some(max)) => encode_announcement(&Announcement {
                code: inp["code"].as_str().unwrap().to_owned(),
                info: inp["info"].as_str().unwrap().to_owned(),
                members,
                max,
            }),
            _ => None,
        };
        match (&got, x["out"].get("$bytes").and_then(Value::as_str)) {
            (Some(g), Some(w)) => {
                assert_eq!(to_hex(g), w, "encodeAnnouncement {i}");
                made += 1;
            }
            (None, None) => {
                assert!(x["out"].is_null(), "encodeAnnouncement {i}: {x}");
                refused += 1;
            }
            _ => panic!("encodeAnnouncement {i}: got {got:?}"),
        }
    }
    let mut read = 0;
    for (i, x) in l["decodeAnnouncement"].as_array().unwrap().iter().enumerate() {
        let got = decode_announcement(&hex(x["in"].as_str().unwrap()));
        match (&got, &x["out"]) {
            (None, Value::Null) => {}
            (Some(g), w) => {
                assert_eq!(
                    &json!({"members": g.members, "max": g.max, "code": g.code, "info": g.info}),
                    w,
                    "decodeAnnouncement {i}"
                );
                read += 1;
            }
            (None, w) => panic!("decodeAnnouncement {i}: got none, want {w}"),
        }
    }
    let f = ordered().expect("the vectors are there");
    for (i, x) in f.lan.targets.iter().enumerate() {
        assert_eq!(broadcast_targets(&x.addresses, &interfaces(&x.nets), x.loopback), x.out, "broadcastTargets {i}");
    }
    eprintln!("lan announce: {made} made, {refused} refused, {read} read, {} target lists", f.lan.targets.len());
}
