//! Where the tests find the golden vectors and the referee tape that the oracle's wire and referee recorders
//! write: the folder [`ENV`] names, else `net` in the main tree's
//! scratch folder (a worktree reads the main tree's). They are written by our own code, never the game's. A test that
//! finds none says so and passes.

use std::path::{Path, PathBuf};

/// The environment variable naming the vectors' folder.
pub const ENV: &str = "MVS_NET_VECTORS";

/// The vectors' folder.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os(ENV) {
        return PathBuf::from(d);
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let common = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(here)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok());
    if let Some(main) = common.as_deref().map(str::trim).and_then(|c| Path::new(c).parent()) {
        return main.join(".scratch/net");
    }
    here.join("../../.scratch/net")
}

/// The vector file `name` of [`dir`], parsed; `None` with a note on stderr when it is not there (the test passes).
/// A file that is there and does not parse panics.
pub fn read(name: &str) -> Option<serde_json::Value> {
    let path = dir().join(name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("no vectors at {} (the oracle's wire recorder writes them): passed without them", path.display());
        return None;
    };
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The PROTOCOL the tapes (`host-tape.json`, `netproc-tape.json`) were recorded at.
pub const TAPE_PROTOCOL: u32 = 23;

/// A tape of [`dir`] as [`read`] gives it, with every `"proto"` (a hello's) and `"server"` (a refusal's) of
/// [`TAPE_PROTOCOL`] moved to this build's [`crate::version::PROTOCOL`] (in the tape's JSON, in the message texts it
/// holds and in the bytes it holds as `{ "$bytes": hex }`), so a new PROTOCOL needs no new tape. Another number (an
/// old game's hello) stays. The message texts' lobby mutator is moved to this build's set too ([`move_mutators`]).
pub fn read_tape(name: &str) -> Option<serde_json::Value> {
    let v = read(name)?;
    Some(move_protocol(v))
}

fn move_protocol(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    let now = crate::version::PROTOCOL;
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| {
                    let x = if PROTOCOL_KEYS.contains(&k.as_str()) && x.as_u64() == Some(u64::from(TAPE_PROTOCOL)) {
                        Value::from(now)
                    } else if k == "$bytes"
                        && let Some(h) = x.as_str()
                    {
                        Value::String(move_in_hex(h, now))
                    } else {
                        move_protocol(x)
                    };
                    (k, x)
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(move_protocol).collect()),
        Value::String(s) => {
            let s = move_in_text(&s, now);
            Value::String(move_mutators(&s).unwrap_or(s))
        }
        x => x,
    }
}

/// A message text of the tape's lobby, which had one mutator (`options.mutator` and the option `mutator`, an index
/// of `LOBBY_MUTATORS`, 0 none), with it as this build's set (`options.mutators` and the option `mutators`, a
/// `MutatorSet`'s bits); None for a text without one. An index past the list is moved to bits no set takes, so its
/// refusal stays.
fn move_mutators(s: &str) -> Option<String> {
    use crate::lobby_rules::{MUTATOR_CHOICES, MutatorSet};
    use serde_json::{Value, json};
    fn set_of(n: &Value) -> Value {
        match n.as_u64().and_then(|n| usize::try_from(n).ok()) {
            Some(n) if n < MUTATOR_CHOICES => json!(MutatorSet::default().with(n, true).0),
            Some(_) => json!(u32::MAX),
            None => n.clone(),
        }
    }
    fn widen(v: &mut Value) -> bool {
        match v {
            Value::Object(o) => {
                let mut changed = false;
                if o.get("key").and_then(Value::as_str) == Some("mutator") {
                    o.insert("key".into(), json!("mutators"));
                    if let Some(x) = o.get("value").map(set_of) {
                        o.insert("value".into(), x);
                    }
                    changed = true;
                }
                if let Some(n) = o.remove("mutator") {
                    o.insert("mutators".into(), set_of(&n));
                    changed = true;
                }
                o.values_mut().fold(changed, |c, x| widen(x) || c)
            }
            // (every element is widened: no stop at the first)
            Value::Array(a) => {
                let mut changed = false;
                for x in a {
                    changed |= widen(x);
                }
                changed
            }
            _ => false,
        }
    }
    if !s.contains("\"mutator\"") {
        return None;
    }
    let mut v: Value = serde_json::from_str(s).ok()?;
    widen(&mut v).then(|| v.to_string())
}

/// The keys of a message that hold the PROTOCOL.
const PROTOCOL_KEYS: [&str; 2] = ["proto", "server"];

/// A message text with each key of PROTOCOL_KEYS at TAPE_PROTOCOL (the number whole, not the start of a longer one)
/// moved to `now`; the rest of the text as it was.
fn move_in_text(s: &str, now: u32) -> String {
    PROTOCOL_KEYS.iter().fold(s.to_owned(), |s, k| move_key(&s, k, now))
}

fn move_key(s: &str, key: &str, now: u32) -> String {
    String::from_utf8(move_key_bytes(s.as_bytes(), key, now)).expect("a text with a number moved")
}

/// Bytes as hex with each key of PROTOCOL_KEYS at TAPE_PROTOCOL moved (a message in a frame); bytes that are not hex
/// stay.
fn move_in_hex(h: &str, now: u32) -> String {
    let b: Option<Vec<u8>> =
        (0..h.len() / 2).map(|i| h.get(2 * i..2 * i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect();
    let Some(b) = b.filter(|_| h.len().is_multiple_of(2)) else { return h.to_owned() };
    let b = PROTOCOL_KEYS.iter().fold(b, |b, k| move_key_bytes(&b, k, now));
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn move_key_bytes(s: &[u8], key: &str, now: u32) -> Vec<u8> {
    let from = format!("\"{key}\":{TAPE_PROTOCOL}").into_bytes();
    let to = format!("\"{key}\":{now}").into_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(&from) && !s.get(i + from.len()).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            out.extend_from_slice(&to);
            i += from.len();
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}
