//! The combined draws (draws.rs) against the oracle's golden vectors (`protocol.json` group `draws` of
//! the oracle's protocol vectors). A test that finds no vectors says so and passes.
//!
//! The asks the port's `int(n: u64)` cannot take are read as the oracle treats them: 0, 1, a negative or NaN answer 0
//! with nothing drawn (`!(n > 1)`); a fraction over 1 is drawn word by word (`int(DRAW_LIMIT)`, one raw word) under
//! the oracle's own f64 bound and modulo; past 2^32 the oracle throws and nothing is drawn (skipped).

use mvs_host::draws::{CombinedDraws, DRAW_LIMIT, draw_of};
use mvs_protocol::vectors;
use serde_json::{Value, json};

/// A vector value: `$undef` as null, `$f64` as a number (NaN or an infinity a string, -0 as -0.0).
fn untag(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            if let Some(h) = o.get("$f64").and_then(Value::as_str) {
                let f = f64::from_bits(u64::from_str_radix(h, 16).expect("f64 bits"));
                return if f.is_finite() { json!(f) } else { json!(format!("$nonnumber {f}")) };
            }
            if o.contains_key("$undef") {
                return Value::Null;
            }
            v.clone()
        }
        _ => v.clone(),
    }
}

fn num(v: &Value) -> f64 {
    match untag(v) {
        Value::Number(n) => n.as_f64().expect("a number"),
        _ => f64::NAN,
    }
}

#[test]
fn draws_match_the_ts_vectors() {
    let Some(all) = vectors::read("protocol.json") else { return };
    let v = &all["draws"];
    assert_eq!(v["consts"]["DRAW_LIMIT"].as_u64(), Some(DRAW_LIMIT));
    let (mut equal, mut skipped, mut diffs) = (0, 0, Vec::new());
    for (i, c) in v["drawOf"].as_array().expect("drawOf").iter().enumerate() {
        let got = draw_of(&untag(&c["in"])).map_or(Value::Null, |d| json!(f64::from(d)));
        let want = match untag(&c["out"]) {
            Value::Number(n) => json!(n.as_f64().expect("a number")),
            w => w,
        };
        if got == want {
            equal += 1;
        } else {
            diffs.push(format!("drawOf {i}: {} -> port {got}, oracle {want}", c["in"]));
        }
    }
    for (g, c) in v["combinedDraws"].as_array().expect("combinedDraws").iter().enumerate() {
        let draws: Vec<u32> = c["draws"]
            .as_array()
            .expect("draws")
            .iter()
            .map(|d| u32::try_from(d.as_u64().expect("a draw")).expect("a u32"))
            .collect();
        let mut r = CombinedDraws::new(&draws);
        let answers = c["answers"].as_array().expect("answers");
        for (i, ask) in c["asks"].as_array().expect("asks").iter().enumerate() {
            let n = num(ask);
            let want = &answers[i];
            if n > 4_294_967_296.0 {
                assert!(want.get("throws").is_some(), "set {g} ask {i}: the oracle answered {n}");
                skipped += 1;
                continue;
            }
            if n.is_nan() || n <= 1.0 {
                r.int(0);
                if num(&want["out"]) == 0.0 {
                    equal += 1;
                } else {
                    diffs.push(format!("set {g} ask {i} ({n}): oracle {want}"));
                }
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let got = if n.fract() == 0.0 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                f64::from(r.int(n as u64))
            } else {
                let limit = DRAW_LIMIT as f64 - (DRAW_LIMIT as f64 % n);
                loop {
                    let x = f64::from(r.int(DRAW_LIMIT));
                    if x < limit {
                        break x % n;
                    }
                }
            };
            if got == num(&want["out"]) {
                equal += 1;
            } else {
                diffs.push(format!("set {g} ask {i} ({n}): port {got}, oracle {want}"));
            }
        }
    }
    eprintln!("draws vectors: {equal} equal, {} differ, {skipped} skipped", diffs.len());
    for d in diffs.iter().take(10) {
        eprintln!("{d}");
    }
    assert!(diffs.is_empty(), "draws differ from the oracle");
}
