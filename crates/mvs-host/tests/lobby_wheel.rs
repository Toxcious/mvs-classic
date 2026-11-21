//! A custom lobby's Chaos Wheel option on the core: the leader's `lobby.setOption` reaches the lobby's state, and the
//! started match's set-up carries the rule to its players.

mod common;

use common::{ManualClock, MemoryPeer};
use mvs_host::{Core, HOST_TIMES, HostOptions, HostRand, SessionKind, SystemRand};
use mvs_protocol::lobby_rules::{WHEEL_MUTATORS, WHEEL_SECONDS, WHEEL_WARN};
use mvs_protocol::version::PROTOCOL;
use serde_json::{Value, json};

/// Every `wheel` value under `v`, at any depth.
fn wheels<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if k == "wheel" {
                    out.push(x);
                }
                wheels(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| wheels(x, out)),
        _ => {}
    }
}

#[test]
fn the_option_reaches_the_state_and_the_set_up() {
    let rand: Box<dyn HostRand> = Box::new(SystemRand);
    let mut core = Core::new(
        HostOptions {
            kind: SessionKind::Custom,
            host: "p0".to_string(),
            name: None,
            lobby_id: Some("L1".to_string()),
            times: HOST_TIMES,
            referee: None,
            rand,
            log: Some(Box::new(|_| {})),
            paths: false,
        },
        0.0,
    );
    let clock = ManualClock { t: 0.0 };
    let (peer, log) = MemoryPeer::new("p0", "");
    let key = core.connect(Box::new(peer), clock.t);
    let say = |core: &mut Core, m: Value| core.message(key, &m.to_string(), clock.t);
    say(&mut core, json!({ "t": "hello", "proto": PROTOCOL, "name": "Leader" }));
    say(&mut core, json!({ "t": "lobby.enter", "compat": "c1" }));
    let got =
        |t: &str| -> Vec<Value> { log.lock().expect("log").got.iter().filter(|m| m["t"] == t).cloned().collect() };
    // off: the state does not hold the key
    let state = got("lobby.state");
    let mut found = Vec::new();
    state.iter().for_each(|m| wheels(m, &mut found));
    assert!(!state.is_empty() && found.is_empty(), "a new lobby's state: {state:?}");

    // a value that is no bool is refused; a bool is kept
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": 3 }));
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": true }));
    let state = got("lobby.state");
    let last = state.last().expect("a state after the option");
    let mut found = Vec::new();
    wheels(last, &mut found);
    assert_eq!(found, [&json!(true)], "the state after the option: {last}");

    // a 1v1 against a bot, started and through its pre-match screen: the set-up the players get holds the rule
    say(&mut core, json!({ "t": "lobby.setTeamStyle", "style": 0 }));
    say(&mut core, json!({ "t": "lobby.addBot", "team": 1, "character": "shaggy", "skin": "", "difficulty": 4 }));
    say(&mut core, json!({ "t": "lobby.setReady", "on": true }));
    log.lock().expect("log").got.clear();
    say(&mut core, json!({ "t": "lobby.startMatch" }));
    say(&mut core, json!({ "t": "pm.lock", "character": "shaggy", "skin": "" }));
    // (the bot's pick is locked when the pick's time is up)
    let later = 600_000.0;
    core.pump(later);
    core.message(key, &json!({ "t": "pm.perks", "perks": [] }).to_string(), later);
    let all = log.lock().expect("log").got.clone();
    let mut rules = Vec::new();
    all.iter().for_each(|m| wheels(m, &mut rules));
    let rule = rules.iter().find(|r| r.is_object()).unwrap_or_else(|| {
        panic!("no set-up with the rule after the start: {:?}", all.iter().map(|m| m["t"].clone()).collect::<Vec<_>>())
    });
    assert_eq!(rule["seconds"], json!(WHEEL_SECONDS));
    assert_eq!(rule["warn"], json!(WHEEL_WARN));
    assert_eq!(rule["pool"].as_array().map(Vec::len), Some(WHEEL_MUTATORS.len()));
}
