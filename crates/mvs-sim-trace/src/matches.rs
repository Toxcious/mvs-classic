//! The matches `sim-trace --record` steps from their definitions (`matches.json`, beside this crate's manifest) and
//! writes as traces: a baseline made from the player's own store, before a change, that a plain run after it compares
//! with.

use mvs_sim::InputState;
use mvs_trace::{Header, TraceWriter, Value};

use crate::inputs::{Combat, Listed, Masher};
use crate::run::{caught, step_caught};
use crate::{bots_for, frame_of, input_of, page_calls, world_for};

/// The match list, as committed.
const LIST: &str = include_str!("../matches.json");

/// Every match of the list, in its order: each one's definition as `matches.json` holds it.
pub fn all() -> Result<Vec<serde_json::Value>, String> {
    let v: serde_json::Value = serde_json::from_str(LIST).map_err(|e| format!("matches.json: {e}"))?;
    v["matches"].as_array().cloned().ok_or_else(|| "matches.json: no `matches` list".to_owned())
}

/// A player's source of input.
enum Source {
    Masher(Masher),
    Combat(Combat),
    Listed(Listed),
}

fn seed_at(spec: &serde_json::Value, key: &str, i: usize) -> Result<i64, String> {
    spec[key][i].as_i64().ok_or(format!("{key}[{i}]: not a seed"))
}

fn sources(spec: &serde_json::Value, players: usize) -> Result<Vec<Source>, String> {
    (0..players)
        .map(|i| {
            if spec.get("combat").is_some() {
                let seed = u32::try_from(seed_at(spec, "combat", i)?).map_err(|_| "combat: a seed out of range")?;
                Ok(Source::Combat(Combat::new(i, seed)))
            } else if spec.get("listed").is_some() {
                Ok(Source::Listed(Listed::new(&spec["listed"][i])?))
            } else {
                let seed = i32::try_from(seed_at(spec, "masher", i)?).map_err(|_| "masher: a seed out of range")?;
                Ok(Source::Masher(Masher::new(seed)))
            }
        })
        .collect()
}

/// Whether an event the frame records is the one `until` names: its type and each other key's value.
fn is_until(until: &serde_json::Value, e: &mvs_trace::EventRecord) -> bool {
    let Some(want) = until.as_object() else { return false };
    want.iter().all(|(k, v)| {
        if k == "type" {
            return v.as_str() == Some(e.kind.as_str());
        }
        match (e.fields.get(k), v) {
            (Some(Value::Str(s)), serde_json::Value::String(w)) => s == w,
            (Some(Value::Num(n)), serde_json::Value::Number(w)) => w.as_f64() == Some(*n),
            (Some(Value::Bool(b)), serde_json::Value::Bool(w)) => b == w,
            _ => false,
        }
    })
}

/// The header of a match recorded from `store`: its definition, set-up and seed, and the store's kinds.
fn header_of(spec: &serde_json::Value, store: &mvs_store::Store) -> Result<Header, String> {
    let m = store.manifest();
    let kinds: serde_json::Map<_, _> = m.kinds.iter().map(|(k, r)| (k.clone(), serde_json::json!(r.version))).collect();
    let calls = match spec.get("calls") {
        Some(c) => serde_json::from_value(c.clone()).map_err(|e| format!("calls: {e}"))?,
        None => Vec::new(),
    };
    Ok(Header {
        format: mvs_trace::file::FORMAT,
        name: spec["name"].as_str().ok_or("a match with no name")?.to_owned(),
        spec: spec.clone(),
        setup: spec["setup"].clone(),
        world_seed: spec["worldSeed"].as_f64().ok_or("worldSeed: not a number")?,
        stage: spec["stage"].as_str().map(str::to_owned),
        store: serde_json::json!({ "buildId": m.build_id, "kinds": kinds }),
        source: serde_json::json!({ "writer": "sim-trace --record" }),
        ticks: 0,
        stop: String::new(),
        hashes: Vec::new(),
        final_hash: None,
        calls,
        schema: Vec::new(),
    })
}

/// A recorded match: the trace's bytes, its ticks and why it stopped.
pub struct Recorded {
    pub bytes: Vec<u8>,
    pub ticks: usize,
    pub stop: String,
}

/// Steps the match `spec` defines on `store`'s data, each tick's inputs from its players' sources (a bot's from its
/// think, before the step as the app's loop does; a tutorial's calls into the World first), and writes each frame. A panic in
/// the sim ends the record with an error naming the tick.
pub fn record(spec: &serde_json::Value, store: &mvs_store::Store) -> Result<Recorded, String> {
    let mut header = header_of(spec, store)?;
    let ticks = usize::try_from(spec["ticks"].as_u64().ok_or("ticks: not a count")?).map_err(|e| e.to_string())?;
    let (mut w, book, mut bots) = caught(|| {
        let (w, book) = world_for(&header, store)?;
        let bots = bots_for(spec, &w);
        Ok::<_, String>((w, book, bots))
    })??;
    let players = w.setup.players.len();
    let mut sources = sources(spec, players)?;
    let until = spec.get("until");
    let mut out = TraceWriter::new();
    let mut stop = "ticks".to_owned();
    // (the fighting players read the World as it stood before this tick's step)
    let mut seen = frame_of(&w, &book, &[], &[]).frame.world;
    let mut n = 0;
    for t in 0..ticks {
        let mut inputs: Vec<InputState> = sources
            .iter_mut()
            .map(|s| match s {
                Source::Masher(m) => m.at(t),
                Source::Combat(c) => c.next(&seen),
                Source::Listed(l) => input_of(l.at(t), &w, &book),
            })
            .collect();
        caught(|| page_calls(&mut w, &header, t)).map_err(|e| format!("tick {t}: {e}"))?;
        for (slot, bot) in bots.iter_mut().enumerate() {
            if let Some(bot) = bot {
                let me = w.players[slot];
                inputs[slot] = caught(|| bot.think(&mut w, me)).map_err(|e| format!("tick {t}: {e}"))?;
            }
        }
        step_caught(&mut w, &inputs).map_err(|e| format!("tick {t}: {e}"))?;
        let written = frame_of(&w, &book, &inputs, &w.events);
        w.events.clear();
        out.push(&written.frame);
        n += 1;
        let done = until.is_some_and(|u| written.frame.world.events.iter().any(|e| is_until(u, e)));
        seen = written.frame.world;
        if done {
            stop = format!(
                "{} {}",
                until.and_then(|u| u["type"].as_str()).unwrap_or(""),
                until.map_or(String::new(), ToString::to_string)
            );
            break;
        }
    }
    header.stop.clone_from(&stop);
    Ok(Recorded { bytes: out.finish(header), ticks: n, stop })
}
