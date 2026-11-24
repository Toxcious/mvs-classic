//! The match wire and the relay wire against the oracle's golden vectors (`packets.json`, `frames.json`,
//! `datagrams.json` and `checks.json` of the oracle's wire recorder). A test that finds no vectors says so and passes.
//! The oracle's matches had four slots: its input headers are of four.

use mvs_protocol::match_wire::{
    self as mw, Checksum, InputHeader, RULED_HEADER, RUN_HEADER, RuledHeader, RunHeader, WireInput, packet_fits,
    packet_inputs,
};
use mvs_protocol::relay_wire::{self as rw, ClientMessage, ServerMessage};
use mvs_protocol::vectors;
use serde_json::{Value, json};

/// The oracle's slot count.
const SLOTS: usize = 4;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    unhex(s).try_into().expect("a fixed size")
}

fn u(v: &Value) -> u32 {
    u32::try_from(v.as_u64().expect("a number")).expect("a u32")
}

fn wire_inputs(v: &Value) -> Vec<WireInput> {
    v.as_array().expect("inputs").iter().map(|s| arr(s.as_str().expect("hex"))).collect()
}

/// JSON.stringify writes 250 where serde writes an f64 field as 250.0.
fn numbers(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("a number")),
        Value::Array(a) => Value::Array(a.into_iter().map(numbers).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, numbers(v))).collect()),
        v => v,
    }
}

/// forRuns' (slot, first, count, mask, end, at) of a ruled packet, from `runs()`.
fn runs_of(b: &[u8]) -> Vec<Value> {
    let mut at = RULED_HEADER as i64;
    mw::runs(b)
        .map(|r| {
            let h = r.head;
            let o = json!({ "slot": h.slot, "first": h.first, "count": h.count, "mask": h.mask, "end": h.end, "at": at + RUN_HEADER as i64 });
            let n = i64::from(h.count) - i64::from(mw::substituted(h.mask));
            at += RUN_HEADER as i64 + n * mw::INPUT_BYTES as i64;
            if n >= 0 {
                assert_eq!(r.inputs.len() as i64, n);
            }
            o
        })
        .collect()
}

/// An oracle input packet with the port's sequence byte after its acks: 0 (the oracle's had none); any other packet,
/// or one too short for the acks, as it is.
fn with_seq(mut b: Vec<u8>) -> Vec<u8> {
    if b.first() == Some(&mw::MSG_INPUT) && b.len() >= 11 + 4 * SLOTS {
        b.insert(11 + 4 * SLOTS, 0);
    }
    b
}

#[test]
fn packets() {
    let Some(v) = vectors::read("packets.json") else { return };
    let mut n = 0;
    for c in v["input"].as_array().unwrap() {
        let acks: Vec<u32> = c["acks"].as_array().unwrap().iter().map(u).collect();
        assert_eq!(acks.len(), SLOTS, "the oracle's acks");
        let h = InputHeader {
            slot: u8::try_from(u(&c["slot"])).unwrap(),
            first: u(&c["first"]),
            count: u8::try_from(u(&c["count"])).unwrap(),
            stamp: u(&c["stamp"]),
            acks: std::array::from_fn(|s| acks.get(s).copied().unwrap_or(0)),
            seq: 0,
        };
        let inputs = wire_inputs(&c["inputs"]);
        let bytes = with_seq(unhex(c["bytes"].as_str().unwrap()));
        assert_eq!(mw::input_packet(&h, SLOTS, &inputs), bytes, "input packet {n}");
        assert_eq!(packet_fits(&bytes, SLOTS), c["fits"].as_bool().unwrap(), "input fits {n}");
        assert_eq!(InputHeader::read(&bytes, SLOTS), Some(h), "input header {n}");
        assert_eq!(packet_inputs(&bytes, SLOTS), inputs.as_slice(), "input inputs {n}");
        n += 1;
    }
    for c in v["checksum"].as_array().unwrap() {
        let k = Checksum { slot: u8::try_from(u(&c["slot"])).unwrap(), ticks: u(&c["ticks"]), value: u(&c["value"]) };
        let bytes = unhex(c["bytes"].as_str().unwrap());
        assert_eq!(k.bytes().to_vec(), bytes, "checksum {n}");
        assert_eq!(Checksum::read(&bytes), Some(k), "checksum read {n}");
        assert_eq!(packet_fits(&bytes, SLOTS), c["fits"].as_bool().unwrap(), "checksum fits {n}");
        n += 1;
    }
    for c in v["ruled"].as_array().unwrap() {
        let runs = c["runs"].as_array().unwrap();
        let head = RuledHeader {
            runs: u8::try_from(runs.len()).unwrap(),
            clock: i32::try_from(c["clock"].as_i64().unwrap()).unwrap(),
            stamp: u(&c["stamp"]),
            held: u16::try_from(u(&c["held"])).unwrap(),
        };
        let mut b = vec![0u8; RULED_HEADER];
        head.write(&mut b);
        for r in runs {
            let rh = RunHeader {
                slot: u8::try_from(u(&r["slot"])).unwrap(),
                first: u(&r["first"]),
                count: u8::try_from(u(&r["count"])).unwrap(),
                mask: u(&r["mask"]),
                end: u(&r["end"]),
            };
            let at = b.len();
            b.resize(at + RUN_HEADER, 0);
            rh.write(&mut b[at..]);
            for i in wire_inputs(&r["inputs"]) {
                b.extend_from_slice(&i);
            }
        }
        let bytes = unhex(c["bytes"].as_str().unwrap());
        assert_eq!(b, bytes, "ruled packet {n}");
        assert_eq!(RuledHeader::read(&bytes), Some(head), "ruled header {n}");
        assert_eq!(packet_fits(&bytes, SLOTS), c["fits"].as_bool().unwrap(), "ruled fits {n}");
        assert_eq!(runs_of(&bytes), c["forRuns"].as_array().unwrap().clone(), "ruled runs {n}");
        // the runs' inputs are what was written
        for (r, want) in mw::runs(&bytes).zip(runs) {
            assert_eq!(r.inputs, wire_inputs(&want["inputs"]).as_slice(), "run inputs {n}");
        }
        n += 1;
    }
    let mut rejected = 0;
    for c in v["fits"].as_array().unwrap() {
        let bytes = with_seq(unhex(c["bytes"].as_str().unwrap()));
        let why = c["why"].as_str().unwrap();
        assert_eq!(packet_fits(&bytes, SLOTS), c["fits"].as_bool().unwrap(), "{why}: {}", c["bytes"]);
        if !packet_fits(&bytes, SLOTS) {
            rejected += 1;
        }
        if !c["forRuns"].is_null() {
            assert_eq!(runs_of(&bytes), c["forRuns"].as_array().unwrap().clone(), "{why}: forRuns");
        }
        n += 1;
    }
    for c in v["substituted"].as_array().unwrap() {
        assert_eq!(mw::substituted(u(&c["mask"])), u(&c["n"]), "substituted {}", c["mask"]);
        n += 1;
    }
    eprintln!("packets: {n} vectors ({rejected} rejected packets)");
}

fn client_or_server(side: &str, message: &Value) -> Vec<u8> {
    match side {
        "client" => rw::json_frame(
            &serde_json::from_value::<ClientMessage>(message.clone()).unwrap_or_else(|e| panic!("{message}: {e}")),
        ),
        _ => rw::json_frame(
            &serde_json::from_value::<ServerMessage>(message.clone()).unwrap_or_else(|e| panic!("{message}: {e}")),
        ),
    }
    .unwrap()
}

#[test]
fn frames() {
    let Some(v) = vectors::read("frames.json") else { return };
    let mut n = 0;
    for c in v["frames"].as_array().unwrap() {
        let ty = u8::try_from(u(&c["type"])).unwrap();
        assert_eq!(
            rw::frame(ty, &unhex(c["body"].as_str().unwrap())).unwrap(),
            unhex(c["bytes"].as_str().unwrap()),
            "frame {n}"
        );
        n += 1;
    }
    let long = &v["frameTooLong"];
    assert_eq!(
        rw::frame(rw::FT_DATA, &vec![0; usize::try_from(u(&long["bodyBytes"])).unwrap()]).is_err(),
        long["throws"].as_bool().unwrap()
    );
    for c in v["packets"].as_array().unwrap() {
        let ty = u8::try_from(u(&c["type"])).unwrap();
        let got = rw::packet_frame(ty, u(&c["handle"]), &unhex(c["packet"].as_str().unwrap())).unwrap();
        assert_eq!(got, unhex(c["bytes"].as_str().unwrap()), "packet frame {n}");
        n += 1;
    }
    for c in v["readPackets"].as_array().unwrap() {
        let body = unhex(c["body"].as_str().unwrap());
        let got = rw::read_packet(&body).map(|p| json!({ "handle": p.handle, "bytes": hex(p.bytes) }));
        assert_eq!(got.unwrap_or(Value::Null), c["read"], "readPacket {n}");
        n += 1;
    }
    for c in v["json"].as_array().unwrap() {
        // (the port's relay wire is its own from version 4: the oracle's match.silent is none of its messages; from
        // version 5 a player opens no match on the relay, so match.open, match.left and match.close are no requests;
        // from version 7 a hello and a challenge carry a kx the oracle's have not)
        let op = c["message"]["op"].as_str().unwrap_or("");
        let t = &c["message"]["t"];
        if t == "match.silent" || t == "hello" || t == "challenge" || op.starts_with("match.") {
            continue;
        }
        let bytes = client_or_server(c["side"].as_str().unwrap(), &c["message"]);
        // (compared as JSON: a field's place in the text is the writer's)
        // (the head's length is of Rust's text, which spells an f64 field 250.0 where JSON.stringify writes 250)
        assert_eq!(bytes[4], rw::FT_JSON);
        assert_eq!(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
            bytes.len() - 4,
            "json frame length {}",
            c["message"]
        );
        let got: Value = serde_json::from_slice(&bytes[5..]).unwrap();
        let want: Value = serde_json::from_slice(&unhex(c["bytes"].as_str().unwrap())[5..]).unwrap();
        assert_eq!(numbers(got), numbers(want), "json {}", c["message"]);
        let read = rw::read_json(&unhex(c["bytes"].as_str().unwrap())[5..]).map_or(Value::Null, Value::Object);
        assert_eq!(read, c["read"], "readJson {}", c["message"]);
        n += 1;
    }
    for c in v["readJsons"].as_array().unwrap() {
        let read = rw::read_json(&unhex(c["body"].as_str().unwrap())).map_or(Value::Null, Value::Object);
        assert_eq!(read, c["read"], "readJson {}", c["body"]);
        n += 1;
    }
    let mut chunks = 0;
    for (k, c) in v["readers"].as_array().unwrap().iter().enumerate() {
        let mut r = rw::FrameReader::new();
        for (p, push) in c["pushes"].as_array().unwrap().iter().enumerate() {
            let got = r.push(&unhex(push["chunk"].as_str().unwrap()));
            let got: Vec<Value> = got.iter().map(|f| json!({ "type": f.ty, "body": hex(&f.body) })).collect();
            assert_eq!(got, push["frames"].as_array().unwrap().clone(), "reader {k} push {p}: frames");
            assert_eq!(r.bad, push["bad"].as_bool().unwrap(), "reader {k} push {p}: bad");
            assert_eq!(r.pending(), usize::try_from(u(&push["pending"])).unwrap(), "reader {k} push {p}: pending");
            chunks += 1;
        }
        n += 1;
    }
    eprintln!("frames: {n} vectors ({chunks} reader pushes)");
}

#[test]
fn datagrams() {
    let Some(v) = vectors::read("datagrams.json") else { return };
    let mut n = 0;
    for c in v["datagrams"].as_array().unwrap() {
        let got = rw::udp_datagram(
            &arr(c["token"].as_str().unwrap()),
            u8::try_from(u(&c["kind"])).unwrap(),
            u(&c["handle"]),
            &unhex(c["packet"].as_str().unwrap()),
        );
        assert_eq!(got, unhex(c["bytes"].as_str().unwrap()), "datagram {n}");
        n += 1;
    }
    for c in v["probes"].as_array().unwrap() {
        let got = rw::probe_datagram(&arr(c["token"].as_str().unwrap()), c["heard"].as_bool().unwrap());
        assert_eq!(got, unhex(c["bytes"].as_str().unwrap()), "probe {n}");
        n += 1;
    }
    for c in v["reads"].as_array().unwrap() {
        let b = unhex(c["bytes"].as_str().unwrap());
        let got = rw::read_udp(&b).map(|d| json!({ "token": hex(&d.token), "kind": d.kind, "handle": d.handle, "bytes": hex(d.bytes), "heard": d.heard }));
        assert_eq!(got.unwrap_or(Value::Null), c["read"], "readUdp {}", c["bytes"]);
        n += 1;
    }
    eprintln!("datagrams: {n} vectors");
}

#[test]
fn checks() {
    let Some(v) = vectors::read("checks.json") else { return };
    let mut n = 0;
    // (the oracle's handshake vectors are of wire 6: from wire 7 the start is the port's own, checked by
    // seal_vectors.rs)
    for c in v["refereeIds"].as_array().unwrap() {
        assert_eq!(rw::referee_id(c["owner"].as_str().unwrap(), u(&c["match"])), c["id"].as_str().unwrap());
        n += 1;
    }
    for c in v["addresses"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        assert_eq!(rw::normal_address(text).map_or(Value::Null, Value::String), c["normal"], "normalAddress {text:?}");
        let low = text.trim().to_lowercase();
        let groups = rw::ipv6_groups(&low).map_or(Value::Null, |g| json!(g));
        assert_eq!(groups, c["groups"], "ipv6Groups {text:?}");
        n += 1;
    }
    for c in v["relays"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        let got = rw::parse_address(text).map_or(Value::Null, |a| json!({ "host": a.host, "port": a.port }));
        assert_eq!(got, c["parsed"], "parseAddress {text:?}");
        n += 1;
    }
    let (mut bad, mut good) = (0, 0);
    for c in v["keys"].as_array().unwrap() {
        let k = unhex(c["key"].as_str().unwrap());
        assert_eq!(rw::bad_public_key(&k), c["bad"].as_bool().unwrap(), "badPublicKey {}: {}", c["why"], c["key"]);
        if c["bad"].as_bool().unwrap() {
            bad += 1
        } else {
            good += 1
        }
        n += 1;
    }
    eprintln!("checks: {n} vectors (keys: {good} usable, {bad} bad)");
}
