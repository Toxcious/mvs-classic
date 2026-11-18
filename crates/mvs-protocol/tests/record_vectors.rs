//! The match record's functions (record.rs) against the oracle's golden vectors (`protocol.json` group `record` of
//! the oracle's protocol vectors): every case's output equal, JSON compared by value with every
//! number as f64. A test that finds no vectors says so and passes.
//!
//! What the port's types cannot take is skipped, counted and shown: a buildRecord `at` or a record's `endedAt` that
//! is not a whole number (milliseconds, a u64; the vectors' 1.5). A NaN or an infinity of an input reads as a string
//! (a value that is no number, as the oracle's NaN is no whole number to any of these functions).

use indexmap::IndexSet;
use mvs_protocol::profile::{
    HistoryMode, MatchRecord, MatchRecordPlayer, MatchReportPlayer, ProfileTotals, TrackerCounts,
};
use mvs_protocol::record::{
    Agreed, FactPlayer, RecordFacts, Report, ReportRules, add_match, add_trackers, agree, build_record, empty_totals,
    match_trackers, read_report,
};
use mvs_protocol::vectors;
use serde_json::{Map, Value, json};

/// A vector value as the Rust functions take it: `$undef` left out, `$f64` a number (a whole one, -0 too, an integer;
/// a NaN or an infinity a string).
fn untag(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            if let Some(h) = o.get("$f64").and_then(Value::as_str) {
                let f = f64::from_bits(u64::from_str_radix(h, 16).expect("f64 bits"));
                #[allow(clippy::cast_possible_truncation)]
                return if f.is_finite() && f.fract() == 0.0 && f.abs() < 9e15 {
                    json!(f as i64)
                } else if f.is_finite() {
                    json!(f)
                } else {
                    json!(format!("$nonnumber {f}"))
                };
            }
            if o.contains_key("$undef") {
                return Value::Null;
            }
            Value::Object(
                o.iter()
                    .filter(|(_, x)| !x.as_object().is_some_and(|x| x.contains_key("$undef")))
                    .map(|(k, x)| (k.clone(), untag(x)))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(untag).collect()),
        _ => v.clone(),
    }
}

/// Every number as f64.
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().expect("a number")),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), norm(x))).collect()),
        _ => v.clone(),
    }
}

/// The oracle host's mapName (the vectors' mapRule).
fn map_rule(v: &Value) -> Option<String> {
    const OBJECT_NAMES: [&str; 12] = [
        "constructor",
        "__proto__",
        "toString",
        "toLocaleString",
        "valueOf",
        "hasOwnProperty",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "__defineGetter__",
        "__defineSetter__",
        "__lookupGetter__",
        "__lookupSetter__",
    ];
    let s = v.as_str()?;
    let ok = s.encode_utf16().count() <= 96
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !OBJECT_NAMES.contains(&s);
    (ok && s.len() >= 4 && s[..4].eq_ignore_ascii_case("map_")).then(|| s.to_string())
}

fn rules_of(v: &Value) -> ReportRules {
    ReportRules {
        slot_teams: v["slotTeams"].as_array().expect("slotTeams").iter().map(as_i32).collect(),
        teams: u32::try_from(v["teams"].as_u64().expect("teams")).expect("teams"),
        knockouts_to_win: as_i32(&v["knockoutsToWin"]),
    }
}

/// A whole number (a -0 as 0).
fn as_i32(v: &Value) -> i32 {
    let f = v.as_f64().unwrap_or_else(|| panic!("a number: {v}"));
    assert!(f.fract() == 0.0 && f.abs() < 2_147_483_648.0, "an i32: {v}");
    #[allow(clippy::cast_possible_truncation)]
    let n = f as i32;
    n
}

fn report_of(v: &Value) -> Report {
    Report {
        team: as_i32(&v["team"]),
        map: v["map"].as_str().unwrap_or("").to_string(),
        players: match &v["players"] {
            Value::Null => None,
            p => Some(serde_json::from_value::<Vec<MatchReportPlayer>>(p.clone()).expect("report players")),
        },
    }
}

fn report_json(r: &Report) -> Value {
    json!({ "team": r.team, "map": r.map, "players": r.players })
}

fn agreed_of(v: &Value) -> Agreed {
    Agreed {
        report: (!v["report"].is_null()).then(|| report_of(&v["report"])),
        checked: v["checked"].as_bool().expect("checked"),
    }
}

fn facts_of(v: &Value) -> RecordFacts {
    RecordFacts {
        id: v["id"].as_str().expect("id").to_string(),
        mode: HistoryMode(u8::try_from(v["mode"].as_u64().expect("mode")).expect("a mode")),
        map: v["map"].as_str().expect("map").to_string(),
        custom: v["custom"].as_bool().expect("custom"),
        players: v["players"]
            .as_array()
            .expect("players")
            .iter()
            .map(|p| FactPlayer {
                id: p["id"].as_str().expect("id").to_string(),
                name: p["name"].as_str().expect("name").to_string(),
                character: p["character"].as_str().expect("character").to_string(),
                team: as_i32(&p["team"]),
                slot: as_i32(&p["slot"]),
                bot: p["bot"].as_bool().expect("bot"),
            })
            .collect(),
    }
}

struct Tally {
    name: &'static str,
    equal: usize,
    skipped: usize,
    diffs: Vec<String>,
}

impl Tally {
    fn new(name: &'static str) -> Tally {
        Tally { name, equal: 0, skipped: 0, diffs: Vec::new() }
    }

    /// A case the port's types cannot take: shown, so that only a fraction (or a NaN) of a whole-number field is.
    fn skip(&mut self, i: usize, c: &Value) {
        self.skipped += 1;
        eprintln!("record vectors: {} {i} skipped: {}", self.name, c.to_string().chars().take(300).collect::<String>());
    }

    fn check(&mut self, i: usize, got: &Value, want: &Value) {
        let (g, w) = (norm(got), norm(&untag(want)));
        if g == w {
            self.equal += 1;
        } else {
            let show = |v: &Value| v.to_string().chars().take(500).collect::<String>();
            self.diffs.push(format!("{} {i}:\n    port {}\n    oracle {}", self.name, show(&g), show(&w)));
        }
    }
}

#[test]
fn record_matches_the_ts_vectors() {
    let Some(all) = vectors::read("protocol.json") else { return };
    let v = &all["record"];
    let mut tallies = Vec::new();

    let mut t = Tally::new("readReport");
    for (i, c) in v["readReport"].as_array().expect("readReport").iter().enumerate() {
        let m: Map<String, Value> = untag(&c["in"]).as_object().cloned().expect("a message");
        let r = read_report(&m, &rules_of(&c["rules"]), &map_rule);
        t.check(i, &r.as_ref().map_or(Value::Null, report_json), &c["out"]);
    }
    tallies.push(t);

    let mut t = Tally::new("agree");
    for (i, c) in v["agree"].as_array().expect("agree").iter().enumerate() {
        let rs: Vec<Report> = untag(&c["in"]).as_array().expect("reports").iter().map(report_of).collect();
        let a = agree(&rs);
        t.check(i, &json!({ "report": a.report.as_ref().map(report_json), "checked": a.checked }), &c["out"]);
    }
    tallies.push(t);

    let mut t = Tally::new("buildRecord");
    for (i, c) in v["buildRecord"].as_array().expect("buildRecord").iter().enumerate() {
        let x = untag(&c["in"]);
        let Some(at) = x["at"].as_u64() else {
            t.skipped += 1;
            continue;
        };
        let r = build_record(&facts_of(&x["facts"]), &agreed_of(&x["agreed"]), at);
        t.check(i, &serde_json::to_value(&r).expect("json"), &c["out"]);
    }
    tallies.push(t);

    let mut t = Tally::new("emptyTotals");
    t.check(0, &serde_json::to_value(empty_totals()).expect("json"), &v["emptyTotals"]);
    tallies.push(t);

    let mut t = Tally::new("addMatch");
    for (i, c) in v["addMatch"].as_array().expect("addMatch").iter().enumerate() {
        let (Ok(totals), Ok(rec), Ok(p)) = (
            serde_json::from_value::<ProfileTotals>(untag(&c["totals"])),
            serde_json::from_value::<MatchRecord>(untag(&c["record"])),
            serde_json::from_value::<MatchRecordPlayer>(untag(&c["player"])),
        ) else {
            t.skip(i, c);
            continue;
        };
        let set_game = u32::try_from(c["setGame"].as_u64().expect("setGame")).expect("a game");
        let out = add_match(&totals, &rec, &p, set_game);
        t.check(i, &serde_json::to_value(&out).expect("json"), &c["out"]);
    }
    tallies.push(t);

    let mut t = Tally::new("matchTrackers");
    for (i, c) in v["matchTrackers"].as_array().expect("matchTrackers").iter().enumerate() {
        let Ok(rec) = serde_json::from_value::<MatchRecord>(untag(&c["record"])) else {
            t.skip(i, c);
            continue;
        };
        let report = untag(&c["report"]);
        let report = (!report.is_null()).then(|| report_of(&report));
        let left: IndexSet<i32> = c["left"].as_array().expect("left").iter().map(as_i32).collect();
        let out = match_trackers(&rec, report.as_ref(), &left);
        let entries: Vec<Value> = out.iter().map(|(k, x)| json!([k, serde_json::to_value(x).expect("json")])).collect();
        let want = match &c["out"] {
            Value::Object(o) if o.contains_key("$map") => o["$map"].clone(),
            w => w.clone(),
        };
        t.check(i, &Value::Array(entries), &want);
    }
    tallies.push(t);

    let mut t = Tally::new("addTrackers");
    for (i, c) in v["addTrackers"].as_array().expect("addTrackers").iter().enumerate() {
        let (Ok(counts), Ok(gain)) = (
            serde_json::from_value::<TrackerCounts>(untag(&c["counts"])),
            serde_json::from_value::<TrackerCounts>(untag(&c["gain"])),
        ) else {
            t.skip(i, c);
            continue;
        };
        t.check(i, &serde_json::to_value(add_trackers(&counts, &gain)).expect("json"), &c["out"]);
    }
    tallies.push(t);

    let mut diffs = 0;
    for t in &tallies {
        eprintln!("record vectors: {} {} equal, {} differ, {} skipped", t.name, t.equal, t.diffs.len(), t.skipped);
        for d in t.diffs.iter().take(5) {
            eprintln!("{d}");
        }
        diffs += t.diffs.len();
    }
    assert_eq!(diffs, 0, "record cases differ from the oracle");
}
