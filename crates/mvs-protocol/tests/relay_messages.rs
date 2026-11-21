//! The relay's control messages read from and written as the JSON the oracle writes: the tags,
//! the field names, the optional fields left out.

use mvs_protocol::relay_wire::{ClientMessage, Listed, Request, ServerMessage};
use serde_json::{Value, json};

/// Every number as an f64: JSON.stringify writes 250 where serde writes an f64 field as 250.0, which JSON.parse reads
/// alike.
fn numbers(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.into_iter().map(numbers).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, numbers(v))).collect()),
        v => v,
    }
}

/// `text` reads as `m`, and `m` writes JSON equal to `text`'s.
fn both_ways<M>(text: &str, m: &M)
where
    M: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let read: M = serde_json::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(&read, m, "{text}");
    let written: Value = serde_json::to_value(m).unwrap();
    assert_eq!(numbers(written), numbers(serde_json::from_str::<Value>(text).unwrap()), "{text}");
}

#[test]
fn client_messages() {
    both_ways(
        r#"{"t":"hello","v":7,"key":"a2V5","nonce":"bm9uY2U=","kx":"a3g=","app":"mvs 1"}"#,
        &ClientMessage::Hello {
            v: 7,
            key: "a2V5".into(),
            nonce: "bm9uY2U=".into(),
            kx: "a3g=".into(),
            app: Some("mvs 1".into()),
            got: None,
        },
    );
    both_ways(
        r#"{"t":"hello","v":7,"key":"a","nonce":"b","kx":"c"}"#,
        &ClientMessage::Hello { v: 7, key: "a".into(), nonce: "b".into(), kx: "c".into(), app: None, got: None },
    );
    both_ways(r#"{"t":"prove","sig":"c2ln"}"#, &ClientMessage::Prove { sig: "c2ln".into() });
    both_ways(
        r#"{"t":"req","id":1,"op":"create","bucket":"versus","max":2,"listed":{"at":5,"info":"x"}}"#,
        &ClientMessage::Req {
            id: 1,
            req: Request::Create {
                bucket: "versus".into(),
                max: 2,
                listed: Some(Listed { at: 5.0, info: Some("x".into()), code: None }),
                host: false,
            },
        },
    );
    both_ways(
        r#"{"t":"req","id":4,"op":"create","bucket":"custom","max":4,"host":true}"#,
        &ClientMessage::Req {
            id: 4,
            req: Request::Create { bucket: "custom".into(), max: 4, listed: None, host: true },
        },
    );
    both_ways(
        r#"{"t":"req","id":2,"op":"list","lobby":"l1","listed":null}"#,
        &ClientMessage::Req { id: 2, req: Request::List { lobby: "l1".into(), listed: None } },
    );
    both_ways(r#"{"t":"peer","handle":4,"id":"r9"}"#, &ClientMessage::Peer { handle: 4, id: "r9".into() });
    both_ways(r#"{"t":"closePeer","id":"r9"}"#, &ClientMessage::ClosePeer { id: "r9".into() });
    both_ways(r#"{"t":"ping"}"#, &ClientMessage::Ping);
}

#[test]
fn server_messages() {
    both_ways(
        r#"{"t":"challenge","v":7,"nonce":"bg==","key":"aw==","kx":"eA==","sig":"cw=="}"#,
        &ServerMessage::Challenge {
            v: 7,
            nonce: "bg==".into(),
            key: "aw==".into(),
            kx: "eA==".into(),
            sig: "cw==".into(),
        },
    );
    both_ways(
        r#"{"t":"welcome","id":"rab","token":"dG9rZW4=","udp":7777}"#,
        &ServerMessage::Welcome { id: "rab".into(), token: "dG9rZW4=".into(), udp: 7777, resumed: None, proto: None },
    );
    both_ways(
        r#"{"t":"welcome","id":"rab","token":"dG9rZW4=","udp":7777,"resumed":2,"proto":28}"#,
        &ServerMessage::Welcome {
            id: "rab".into(),
            token: "dG9rZW4=".into(),
            udp: 7777,
            resumed: Some(2),
            proto: Some(28),
        },
    );
    both_ways(
        r#"{"t":"res","id":3,"ok":true,"value":{"lobby":"l1"}}"#,
        &ServerMessage::Res { id: 3, ok: true, value: Some(json!({ "lobby": "l1" })), error: None },
    );
    both_ways(
        r#"{"t":"res","id":3,"ok":false,"error":"EOS_NotFound"}"#,
        &ServerMessage::Res { id: 3, ok: false, value: None, error: Some("EOS_NotFound".into()) },
    );
    both_ways(
        r#"{"t":"lobby","lobby":"l1","closed":"EOS_Lobby_NotOwner"}"#,
        &ServerMessage::Lobby { lobby: "l1".into(), members: None, closed: Some("EOS_Lobby_NotOwner".into()) },
    );
    both_ways(
        r#"{"t":"lobby","lobby":"l1","members":["r1","r2"]}"#,
        &ServerMessage::Lobby { lobby: "l1".into(), members: Some(vec!["r1".into(), "r2".into()]), closed: None },
    );
    both_ways(
        r#"{"t":"listing","lobby":"l1","info":"x"}"#,
        &ServerMessage::Listing { lobby: "l1".into(), info: Some("x".into()) },
    );
    both_ways(r#"{"t":"listing","lobby":"l1"}"#, &ServerMessage::Listing { lobby: "l1".into(), info: None });
    both_ways(r#"{"t":"peerClosed","id":"r2"}"#, &ServerMessage::PeerClosed { id: "r2".into() });
    both_ways(r#"{"t":"pong"}"#, &ServerMessage::Pong);
}

#[test]
fn host_ids() {
    use mvs_protocol::relay_wire::{QUEUE_HOST, host_id, is_host_id, referee_id};
    assert_eq!(host_id("l0a1b"), "l0a1b.host");
    assert!(is_host_id(&host_id("l0a1b")));
    assert!(is_host_id(QUEUE_HOST));
    assert!(!is_host_id(".host"));
    assert!(!is_host_id(&format!("r{}", "0".repeat(32))));
    assert!(!is_host_id(&referee_id(&host_id("l0a1b"), 3)));
    assert!(!is_host_id(&referee_id(QUEUE_HOST, 3)));
}
