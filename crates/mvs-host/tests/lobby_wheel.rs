//! A custom lobby's Chaos Wheel option on the core: the leader's `lobby.setOption` reaches the lobby's state, and the
//! started match's set-up carries the rule to its players.

mod common;

use common::{ManualClock, MemoryPeer};
use mvs_host::{Core, HOST_TIMES, HostOptions, HostRand, SessionKind, SystemRand};
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

    // a value that is no set of choices, or one outside the lists, is refused; choices are kept
    let on = json!({ "on": true, "seconds": 15, "warn": 5, "chaos": 3, "two": true, "off": 1 });
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": 3 }));
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": true }));
    let bad = json!({ "on": true, "seconds": 17, "warn": 5, "chaos": 3, "two": true, "off": 0 });
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": bad }));
    let refused = got("lobby.state");
    let mut found = Vec::new();
    refused.iter().for_each(|m| wheels(m, &mut found));
    assert!(found.is_empty(), "refused values: {found:?}");
    say(&mut core, json!({ "t": "lobby.setOption", "key": "wheel", "value": on }));
    let state = got("lobby.state");
    let last = state.last().expect("a state after the option");
    let mut found = Vec::new();
    wheels(last, &mut found);
    assert_eq!(found, [&on], "the state after the option: {last}");

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
    assert_eq!((&rule["seconds"], &rule["warn"]), (&json!(15), &json!(5)));
    assert_eq!((&rule["chaos"], &rule["two"]), (&json!("often"), &json!(true)));
    // (the first of the list left out of the pool)
    assert_eq!(rule["pool"].as_array().map(Vec::len), Some(mvs_mutators::ALL.len() - 1));
    assert!(!rule["pool"].as_array().expect("a pool").contains(&json!(mvs_mutators::ALL[0].key)));
}
