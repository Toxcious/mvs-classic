//! The lobby codes against the oracle's golden vectors (`codes` of `protocol.json`, written by
//! the oracle's protocol vectors from its codes). A test that finds no vectors says so and passes.
//!
//! The codes carry no words: a refusal is a [`CodeWhy`], and the table below is the oracle's text of each, so an oracle
//! line still checks the code.

use mvs_net::codes::{
    CodeWhy, EPIC_ID_MAX, LOBBY_CODE_VERSION, LobbyCode, RELAY_HOST_MAX, decode_lobby_code, encode_lobby_code,
    is_short, normal_relay, same_relay,
};
use mvs_protocol::relay_wire::{SHORT_ALPHABET, SHORT_LEN};
use mvs_protocol::vectors;
use serde_json::{Value, json};

/// The oracle's words for each reason.
fn ts_text(why: CodeWhy) -> String {
    match why {
        CodeWhy::Empty => "there is no lobby code".to_owned(),
        CodeWhy::UnknownKind => {
            "that is not a lobby code this app can read (mistyped, or made by a newer version of the app)".to_owned()
        }
        CodeWhy::Mistyped => "that lobby code is mistyped (a character is wrong, missing or extra)".to_owned(),
        CodeWhy::Version(n) => {
            format!("that lobby code is of another version ({n}): one of the two apps needs an update")
        }
        CodeWhy::Spelled => {
            "that lobby code is not written the way its host writes it: copy it from the host again".to_owned()
        }
        CodeWhy::DirectMistyped => "that direct code is mistyped (a character is wrong, missing or extra)".to_owned(),
        CodeWhy::DirectVersion(n) => {
            format!("that direct code is of another version ({n}): one of the two apps needs an update")
        }
        CodeWhy::DirectOutside => "that direct code names an address outside a local network".to_owned(),
    }
}

fn code_of(v: &Value) -> LobbyCode {
    let text = |k: &str| v[k].as_str().map(str::to_owned);
    match v["kind"].as_str().expect("a kind") {
        "epic" => LobbyCode::Epic { lobby: text("lobby").expect("lobby") },
        "relay" => LobbyCode::Relay { lobby: text("lobby").expect("lobby"), relay: text("relay") },
        _ => LobbyCode::Direct { code: text("code").expect("code") },
    }
}

fn code_json(c: &LobbyCode) -> Value {
    match c {
        LobbyCode::Epic { lobby } => json!({ "kind": "epic", "lobby": lobby }),
        LobbyCode::Relay { lobby, relay } => json!({ "kind": "relay", "lobby": lobby, "relay": relay }),
        LobbyCode::Direct { code } => json!({ "kind": "direct", "code": code }),
        // (the port's own kind: no oracle vector holds one)
        LobbyCode::Short { code } => json!({ "kind": "short", "code": code }),
    }
}

#[test]
fn lobby_codes() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let c = &v["codes"];
    assert_eq!(c["consts"]["LOBBY_CODE_VERSION"].as_u64(), Some(u64::from(LOBBY_CODE_VERSION)));
    assert_eq!(c["consts"]["RELAY_HOST_MAX"].as_u64(), Some(RELAY_HOST_MAX as u64));
    assert_eq!(c["consts"]["EPIC_ID_MAX"].as_u64(), Some(EPIC_ID_MAX as u64));

    for (i, x) in c["normalRelay"].as_array().unwrap().iter().enumerate() {
        let got = x["in"].as_str().map(normal_relay);
        let want = x["normalRelay"].as_str().map(str::to_owned);
        // (the vectors name a null address by null; it has no normal form)
        assert_eq!(got.unwrap_or(None), want, "normalRelay {i}: {}", x["in"]);
    }
    for (i, x) in c["sameRelay"].as_array().unwrap().iter().enumerate() {
        let a = x["in"][0].as_str();
        let b = x["in"][1].as_str();
        assert_eq!(same_relay(a, b), x["out"].as_bool().unwrap(), "sameRelay {i}: {}", x["in"]);
    }

    let mut threw = 0;
    for (i, x) in c["encodeLobbyCode"].as_array().unwrap().iter().enumerate() {
        let got = encode_lobby_code(&code_of(&x["in"]), x["defaultRelay"].as_str());
        match (&got, x.get("out")) {
            (Ok(g), Some(w)) => assert_eq!(&json!(g), w, "encodeLobbyCode {i}: {x}"),
            (Err(_), None) => {
                assert!(x.get("throws").is_some(), "encodeLobbyCode {i} threw in Rust only: {x}");
                threw += 1;
            }
            _ => panic!("encodeLobbyCode {i}: got {got:?}, want {x}"),
        }
    }

    let (mut ok, mut short) = (0, 0);
    for (i, x) in c["decodeLobbyCode"].as_array().unwrap().iter().enumerate() {
        // (a text that is no string reads as the empty text, as directText of a non-string does)
        let text = x["in"].as_str().unwrap_or_default();
        let got = decode_lobby_code(text, x["defaultRelay"].as_str());
        // (the port's short code: a text of its form the oracle read mistyped is one here, the deviation of codes.rs)
        let plain: String = text.split_whitespace().collect::<String>().to_lowercase();
        if is_short(&plain) {
            assert_eq!(
                x["out"]["ok"],
                json!(false),
                "decodeLobbyCode {i}: an oracle code of the short form: {}",
                x["in"]
            );
            assert_eq!(got, Ok(LobbyCode::Short { code: plain }), "decodeLobbyCode {i}");
            short += 1;
            continue;
        }
        let got_json = match &got {
            Ok(code) => json!({ "ok": true, "code": code_json(code) }),
            Err(why) => json!({ "ok": false, "why": ts_text(*why) }),
        };
        let want = x["out"].clone();
        assert_eq!(got_json, want, "decodeLobbyCode {i}: in {}", x["in"]);
        ok += usize::from(got.is_ok());
    }
    eprintln!("lobby codes: {threw} encodes refused, {ok} decodes read, {short} read as short codes");
}

#[test]
fn short_codes_read_before_the_kind_letter_a_bad_one_mistyped() {
    let d = Some("relay.example:7777");
    for t in ["k7m2pq", "EAEAWC", " d2x 9qa ", "rrrrrr", "222222"] {
        let plain: String = t.split_whitespace().collect::<String>().to_lowercase();
        assert_eq!(decode_lobby_code(t, d), Ok(LobbyCode::Short { code: plain.clone() }), "{t}");
        assert_eq!(decode_lobby_code(t, None), Ok(LobbyCode::Short { code: plain.clone() }), "{t}: no default relay");
        assert_eq!(encode_lobby_code(&LobbyCode::Short { code: plain.clone() }, d).as_deref(), Ok(plain.as_str()));
    }
    // a look-alike (i, l, o, 0, 1) or another character, in a text of a short code's length: mistyped
    for t in ["k7m2p0", "dabcd1", "eaeaio", "lllllo", "abc-de", "abcdeä"] {
        assert_eq!(decode_lobby_code(t, d), Err(CodeWhy::Mistyped), "{t}");
    }
    // a character missing or extra: the long forms' reading, as the oracle's
    assert_eq!(decode_lobby_code("k7m2p", d), Err(CodeWhy::UnknownKind));
    assert_eq!(decode_lobby_code("k7m2pqq", d), Err(CodeWhy::UnknownKind));
    assert_eq!(decode_lobby_code("eaeabeb", d), Err(CodeWhy::Mistyped));
    assert!(SHORT_ALPHABET.iter().all(|c| !b"ilo01".contains(c)) && SHORT_LEN == 6);
}
