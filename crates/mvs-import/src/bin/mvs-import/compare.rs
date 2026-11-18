//! `compare`: one kind's pack against the oracle store's, entry by entry, the differences grouped so a reader sees
//! what kind of thing differs and where.
//!
//! How a kind's entry is read is [`format_of`]: JSON, the json-bin container (the kind's magic, u32 header length,
//! the JSON header, padding to 16, the body: the header compared as JSON, the body by its bytes; an object entry
//! without a body is plain JSON), or raw bytes. JSON is compared by value: object key order ignored; numbers equal as
//! f64, else equal once both are rounded to f32 (counted as "float printing", a match); the keys [`left_out_of`] names
//! are left out. The content types are compared too.
//!
//! The gamedata kind is compared exactly ([`gamedata_entry`]): both entries decoded as its reader decodes them, every
//! number by its bits, every object's keys in order, every Float32Array by its bytes.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use mvs_data::value::{DecodeError, Value as GdValue, decode_entry};
use mvs_import::kinds;
use mvs_store::Pack;
use rayon::prelude::*;
use serde_json::Value;

/// How a kind's entries are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// The json-bin container with this magic, or plain JSON (an entry without a body).
    JsonBin(&'static [u8; 4]),
    Json,
    /// Bytes only (a texture's container, a font, an image).
    Raw,
    /// The gamedata kind's entries (`mvs_data::value`), compared exactly.
    GameData,
}

/// The format of each kind's entries (`None`: no such kind).
pub fn format_of(kind: &str) -> Option<Format> {
    Some(match kind {
        "object" => Format::JsonBin(kinds::object::OBJECT_MAGIC),
        "mesh" => Format::JsonBin(kinds::mesh::MESH_MAGIC),
        "animation" => Format::JsonBin(kinds::animation::ANIM_MAGIC),
        "lighting" => Format::JsonBin(kinds::lighting::LIGHTING_MAGIC),
        "shadermap" => Format::JsonBin(kinds::shadermap::SHADERMAP_MAGIC),
        "shader" => Format::JsonBin(kinds::shader::SHADER_MAGIC),
        "material" => Format::JsonBin(kinds::material::MATERIAL_MAGIC),
        "sky" => Format::JsonBin(kinds::sky::SKY_MAGIC),
        "atmosphere" => Format::JsonBin(kinds::atmosphere::ATMOSPHERE_MAGIC),
        "scenery" => Format::JsonBin(kinds::scenery::SCENERY_MAGIC),
        "skeleton" | "text" | "config" | "stage" | "stagedraw" | "stagelight" | "particles" | "ui" | "fighter"
        | "banner" | "literals" => Format::Json,
        "texture" | "font" | "image" => Format::Raw,
        "gamedata" => Format::GameData,
        _ => return None,
    })
}

/// The keys of a kind's JSON (its header's) the compare leaves out: what the port does not write by design. A key is a
/// path of members from the top, `.` between them, a member ending `[]` each element of the array it holds
/// (`instances[].bounds`). The atmosphere's `pixel` and `vertex` are the oracle's GLSL halves, which the native
/// renderer does not read; a scenery instance (under the header's `scenery`)'s `bounds` and `wpo` were for the oracle's
/// translated vertex shaders;
/// the fighter's `data` and `skinAudio` and the stage kind's `data` are the gamedata kind's.
pub fn left_out_of(kind: &str) -> &'static [&'static str] {
    match kind {
        "atmosphere" => &["pixel", "vertex"],
        "scenery" => &["scenery.instances[].bounds", "scenery.instances[].wpo"],
        "fighter" => &["data", "skinAudio", "blendSpaces"],
        "stage" => &["data"],
        _ => &[],
    }
}

/// Removes the member `path` names (a [`left_out_of`] key) from `v`, wherever it is.
fn leave_out(v: &mut Value, path: &str) {
    let (head, rest) = path.split_once('.').map_or((path, None), |(h, r)| (h, Some(r)));
    let Value::Object(m) = v else { return };
    match (head.strip_suffix("[]"), rest) {
        (None, None) => {
            m.remove(head);
        }
        (None, Some(rest)) => {
            if let Some(inner) = m.get_mut(head) {
                leave_out(inner, rest);
            }
        }
        (Some(key), rest) => {
            if let Some(Value::Array(list)) = m.get_mut(key) {
                match rest {
                    Some(rest) => list.iter_mut().for_each(|e| leave_out(e, rest)),
                    None => list.clear(),
                }
            }
        }
    }
}

/// A value shown in a sample is cut to this many characters.
const SHOWN: usize = 160;
/// Groups printed on stdout (the report holds all).
const TOP: usize = 40;

/// One difference: the cause, the JSON path as written and collapsed, and both values.
struct Diff {
    cause: &'static str,
    path: String,
    collapsed: String,
    ours: String,
    oracle: String,
}

/// An entry's outcome.
enum Outcome {
    Equal,
    /// Equal once numbers are rounded to f32.
    FloatOnly,
    Differ {
        class: String,
        diffs: Vec<Diff>,
    },
}

/// A decoded entry: its JSON (the container's header) and the container's body.
struct Decoded {
    json: Value,
    body: Option<Vec<u8>>,
}

fn decode(format: Format, bytes: &[u8]) -> Result<Decoded, String> {
    if format == Format::Raw {
        return Ok(Decoded { json: Value::Null, body: Some(bytes.to_vec()) });
    }
    if let Format::JsonBin(magic) = format
        && bytes.len() >= 8
        && &bytes[..4] == magic
    {
        let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let head = bytes.get(8..8 + len).ok_or("a container header past the entry")?;
        let json = serde_json::from_slice(head).map_err(|e| format!("container header: {e}"))?;
        let body_at = (8 + len + 15) & !15;
        return Ok(Decoded { json, body: Some(bytes.get(body_at..).unwrap_or_default().to_vec()) });
    }
    Ok(Decoded { json: serde_json::from_slice(bytes).map_err(|e| e.to_string())?, body: None })
}

fn show(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > SHOWN { format!("{}...", s.chars().take(SHOWN).collect::<String>()) } else { s }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

enum NumberMatch {
    Equal,
    /// Equal once both are rounded to f32 (a float printed at another precision).
    Float,
    Differ,
}

/// Two integers are equal only exactly; where either is a float (written with a fraction or exponent), equal as f64,
/// else equal once both are rounded to a finite f32.
fn number_match(x: &serde_json::Number, y: &serde_json::Number) -> NumberMatch {
    if !x.is_f64() && !y.is_f64() {
        let int = |n: &serde_json::Number| n.as_i64().map(i128::from).or_else(|| n.as_u64().map(i128::from));
        return if int(x) == int(y) { NumberMatch::Equal } else { NumberMatch::Differ };
    }
    let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
    if x == y {
        NumberMatch::Equal
    } else if (x as f32).is_finite() && x as f32 == y as f32 {
        NumberMatch::Float
    } else {
        NumberMatch::Differ
    }
}

/// Walks two values, recording each difference; `float` is set when numbers matched only once rounded to f32.
struct Walk {
    diffs: Vec<Diff>,
    float: bool,
}

impl Walk {
    fn push(&mut self, cause: &'static str, path: &str, collapsed: &str, ours: String, oracle: String) {
        self.diffs.push(Diff { cause, path: path.to_string(), collapsed: collapsed.to_string(), ours, oracle });
    }

    /// `path` as written, `collapsed` with array indexes and inner-object names as `*`; `inner` when `a` and `b`
    /// are the values of an `inner` key (their keys are inner objects' names).
    fn value(&mut self, a: &Value, b: &Value, path: &str, collapsed: &str, inner: bool) {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => match number_match(x, y) {
                NumberMatch::Equal => {}
                NumberMatch::Float => self.float = true,
                NumberMatch::Differ => self.push("number", path, collapsed, show(a), show(b)),
            },
            (Value::Object(x), Value::Object(y)) => {
                for (k, va) in x {
                    let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    let c = if inner {
                        format!("{collapsed}.*")
                    } else if collapsed.is_empty() {
                        k.clone()
                    } else {
                        format!("{collapsed}.{k}")
                    };
                    match y.get(k) {
                        Some(vb) => self.value(va, vb, &p, &c, !inner && k == "inner"),
                        None => self.push("key only in ours", &p, &c, show(va), String::new()),
                    }
                }
                for (k, vb) in y {
                    if !x.contains_key(k) {
                        let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                        let c = if inner {
                            format!("{collapsed}.*")
                        } else if collapsed.is_empty() {
                            k.clone()
                        } else {
                            format!("{collapsed}.{k}")
                        };
                        self.push("key only in the oracle", &p, &c, String::new(), show(vb));
                    }
                }
            }
            (Value::Array(x), Value::Array(y)) => {
                let c = format!("{collapsed}[*]");
                for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                    self.value(va, vb, &format!("{path}[{i}]"), &c, false);
                }
                if x.len() != y.len() {
                    self.push("array length", path, collapsed, x.len().to_string(), y.len().to_string());
                }
            }
            _ if a == b => {}
            _ if kind_of(a) != kind_of(b) => self.push("type", path, collapsed, show(a), show(b)),
            _ => self.push(kind_of(a), path, collapsed, show(a), show(b)),
        }
    }
}

fn class_of(v: &Value) -> String {
    v.get("class").and_then(Value::as_str).unwrap_or("?").to_string()
}

fn compare_entry(format: Format, left_out: &[&str], ours: &Pack, oracle: &Pack, path: &str, deep: bool) -> Outcome {
    let one = |cause: &'static str, a: String, b: String, class: String| Outcome::Differ {
        class,
        diffs: vec![Diff { cause, path: String::new(), collapsed: String::new(), ours: a, oracle: b }],
    };
    let (a, b) = match (ours.bytes(path), oracle.bytes(path)) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            let e = |r: Result<Vec<u8>, mvs_store::StoreError>| r.err().map(|e| e.to_string()).unwrap_or_default();
            return one("read", e(a), e(b), "?".into());
        }
    };
    let (ta, tb) = (ours.content_type(path).unwrap_or(""), oracle.content_type(path).unwrap_or(""));
    if ta != tb {
        return one("content type", ta.to_string(), tb.to_string(), "?".into());
    }
    if a == b && !deep {
        return Outcome::Equal;
    }
    if format == Format::GameData {
        return gamedata_entry(path, &a, &b);
    }
    let (mut da, mut db) = match (decode(format, &a), decode(format, &b)) {
        (Ok(x), Ok(y)) => (x, y),
        (x, y) => {
            let class = y.as_ref().map(|d| class_of(&d.json)).unwrap_or_else(|_| "?".into());
            let e = |r: Result<Decoded, String>| r.err().unwrap_or_default();
            return one("parse", e(x), e(y), class);
        }
    };
    for d in [&mut da, &mut db] {
        for k in left_out {
            leave_out(&mut d.json, k);
        }
    }
    let class = class_of(&db.json);
    let mut w = Walk { diffs: Vec::new(), float: false };
    match (&da.body, &db.body) {
        (None, None) => {}
        (Some(x), Some(y)) if x == y => {}
        (Some(x), Some(y)) => {
            w.push("body bytes", "(body)", "(body)", format!("{} bytes", x.len()), format!("{} bytes", y.len()))
        }
        (x, y) => {
            let k = |b: &Option<Vec<u8>>| if b.is_some() { "json-bin container" } else { "JSON" }.to_string();
            w.push("container", "(entry)", "(entry)", k(x), k(y));
        }
    }
    w.value(&da.json, &db.json, "", "", false);
    if !w.diffs.is_empty() {
        Outcome::Differ { class, diffs: w.diffs }
    } else if w.float {
        Outcome::FloatOnly
    } else {
        Outcome::Equal
    }
}

/// A gamedata entry against the oracle's (their bytes differ): both decoded by the reader's decoder, compared exactly,
/// the class the entry's kind of record (`Char`, `Stage`, `Index`, ...).
fn gamedata_entry(path: &str, a: &[u8], b: &[u8]) -> Outcome {
    let rest = path.strip_prefix("/GameData/").unwrap_or(path);
    let class = rest.split(['/', '.']).next().unwrap_or("?").to_string();
    let (va, vb) = match (decode_entry(a), decode_entry(b)) {
        (Ok(x), Ok(y)) => (x, y),
        (x, y) => {
            let e = |r: Result<GdValue, DecodeError>| r.err().map(|e| e.to_string()).unwrap_or_default();
            let diffs =
                vec![Diff { cause: "parse", path: String::new(), collapsed: String::new(), ours: e(x), oracle: e(y) }];
            return Outcome::Differ { class, diffs };
        }
    };
    let mut w = Walk { diffs: Vec::new(), float: false };
    gd_value(&mut w, &va, &vb, "", "");
    if w.diffs.is_empty() && a == b {
        return Outcome::Equal;
    }
    if w.diffs.is_empty() {
        // equal values in other bytes: the container's layout (padding, the sections' order)
        w.push("encoding", "(entry)", "(entry)", format!("{} bytes", a.len()), format!("{} bytes", b.len()));
    }
    Outcome::Differ { class, diffs: w.diffs }
}

fn cut(s: String) -> String {
    if s.chars().count() > SHOWN { format!("{}...", s.chars().take(SHOWN).collect::<String>()) } else { s }
}

/// A gamedata value as its entry's JSON writes it (a `$f32` tag naming its section), cut to [`SHOWN`].
fn gd_show(v: &GdValue) -> String {
    let e = mvs_data::value::encode(v).unwrap_or_default();
    let len = e.get(4..8).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    cut(String::from_utf8_lossy(e.get(8..8 + len).unwrap_or_default()).into_owned())
}

fn gd_kind(v: &GdValue) -> &'static str {
    match v {
        GdValue::Null => "null",
        GdValue::Bool(_) => "bool",
        GdValue::Num(_) => "number",
        GdValue::Str(_) => "string",
        GdValue::Arr(_) => "array",
        GdValue::Obj(_) => "object",
        GdValue::F32s(_) => "Float32Array",
    }
}

/// Walks two gamedata values exactly: a number by its bits (a NaN is one value, as the encoding writes it), an
/// object's keys and their order, a Float32Array by its floats' bits. `collapsed` has array indexes as `[*]`.
fn gd_value(w: &mut Walk, a: &GdValue, b: &GdValue, path: &str, collapsed: &str) {
    let member = |base: &str, k: &str| if base.is_empty() { k.to_string() } else { format!("{base}.{k}") };
    match (a, b) {
        (GdValue::Num(x), GdValue::Num(y)) => {
            if x.to_bits() != y.to_bits() && !(x.is_nan() && y.is_nan()) {
                let show = |n: &f64| format!("{} ({:016x})", gd_show(&GdValue::Num(*n)), n.to_bits());
                w.push("number", path, collapsed, show(x), show(y));
            }
        }
        (GdValue::Obj(x), GdValue::Obj(y)) => {
            for (k, va) in x {
                let (p, c) = (member(path, k), member(collapsed, k));
                match y.get(k) {
                    Some(vb) => gd_value(w, va, vb, &p, &c),
                    None => w.push("key only in ours", &p, &c, gd_show(va), String::new()),
                }
            }
            for (k, vb) in y {
                if !x.contains_key(k) {
                    w.push(
                        "key only in the oracle",
                        &member(path, k),
                        &member(collapsed, k),
                        String::new(),
                        gd_show(vb),
                    );
                }
            }
            let ka: Vec<&str> = x.keys().filter(|k| y.contains_key(*k)).map(String::as_str).collect();
            let kb: Vec<&str> = y.keys().filter(|k| x.contains_key(*k)).map(String::as_str).collect();
            if ka != kb {
                w.push("key order", path, collapsed, cut(ka.join(",")), cut(kb.join(",")));
            }
        }
        (GdValue::Arr(x), GdValue::Arr(y)) => {
            let c = format!("{collapsed}[*]");
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                gd_value(w, va, vb, &format!("{path}[{i}]"), &c);
            }
            if x.len() != y.len() {
                w.push("array length", path, collapsed, x.len().to_string(), y.len().to_string());
            }
        }
        (GdValue::F32s(x), GdValue::F32s(y)) => {
            let differ: Vec<usize> = (0..x.len().min(y.len())).filter(|&i| x[i].to_bits() != y[i].to_bits()).collect();
            if x.len() != y.len() {
                w.push("Float32Array length", path, collapsed, x.len().to_string(), y.len().to_string());
            } else if let Some(&i) = differ.first() {
                w.push(
                    "Float32Array floats",
                    path,
                    collapsed,
                    format!("{} differ; first [{i}] {:e} ({:08x})", differ.len(), x[i], x[i].to_bits()),
                    format!("[{i}] {:e} ({:08x})", y[i], y[i].to_bits()),
                );
            }
        }
        _ if a == b => {}
        _ if gd_kind(a) != gd_kind(b) => w.push("type", path, collapsed, gd_show(a), gd_show(b)),
        _ => w.push(gd_kind(a), path, collapsed, gd_show(a), gd_show(b)),
    }
}

/// A group of differences: (class, collapsed JSON path, cause).
#[derive(Default)]
struct Group {
    /// Differences counted.
    count: u64,
    /// Entries that have one.
    entries: u64,
    /// One sample: the entry, its JSON path, ours and the oracle's.
    sample: Option<(String, String, String, String)>,
}

#[derive(Default)]
struct Tally {
    equal: u64,
    float_only: u64,
    differing: u64,
    groups: HashMap<(String, String, &'static str), Group>,
}

impl Tally {
    fn merge(mut self, o: Tally) -> Tally {
        self.equal += o.equal;
        self.float_only += o.float_only;
        self.differing += o.differing;
        for (k, g) in o.groups {
            let mine = self.groups.entry(k).or_default();
            mine.count += g.count;
            mine.entries += g.entries;
            // the sample of the entry first in path order, whatever the threads' order
            if mine.sample.is_none() || g.sample.as_ref().is_some_and(|s| s.0 < mine.sample.as_ref().unwrap().0) {
                mine.sample = g.sample;
            }
        }
        self
    }
}

/// The paths only one side has, and the tally of the paths both have.
struct Compared<'a> {
    only_ours: Vec<&'a str>,
    only_oracle: Vec<&'a str>,
    both: usize,
    tally: Tally,
}

/// Compares every entry of `ours` with `oracle`'s (on all cores). `deep` decodes an entry even when its bytes are
/// equal (a timing of the compare itself).
fn compare<'a>(format: Format, left_out: &[&str], ours: &'a Pack, oracle: &'a Pack, deep: bool) -> Compared<'a> {
    let mut only_ours: Vec<&str> = ours.paths().filter(|p| !oracle.has(p)).collect();
    let mut only_oracle: Vec<&str> = oracle.paths().filter(|p| !ours.has(p)).collect();
    only_ours.sort_unstable();
    only_oracle.sort_unstable();
    let both: Vec<&str> = ours.paths().filter(|p| oracle.has(p)).collect();

    let tally = both
        .par_iter()
        .fold(Tally::default, |mut t, &path| {
            match compare_entry(format, left_out, ours, oracle, path, deep) {
                Outcome::Equal => t.equal += 1,
                Outcome::FloatOnly => t.float_only += 1,
                Outcome::Differ { class, diffs } => {
                    t.differing += 1;
                    let mut seen: HashMap<(String, &'static str), ()> = HashMap::new();
                    for d in diffs {
                        let key = (class.clone(), d.collapsed.clone(), d.cause);
                        let g = t.groups.entry(key).or_default();
                        g.count += 1;
                        if seen.insert((d.collapsed, d.cause), ()).is_none() {
                            g.entries += 1;
                        }
                        if g.sample.as_ref().is_none_or(|s| path < s.0.as_str()) {
                            g.sample = Some((path.to_string(), d.path, d.ours, d.oracle));
                        }
                    }
                }
            }
            t
        })
        .reduce(Tally::default, Tally::merge);
    Compared { only_ours, only_oracle, both: both.len(), tally }
}

/// Compares `ours` with `oracle`; prints the summary and the top groups, and writes everything to `report`.
pub fn run(
    format: Format,
    left_out: &[&str],
    ours: &Pack,
    oracle: &Pack,
    report: Option<&Path>,
    deep: bool,
) -> Result<(), String> {
    let Compared { only_ours, only_oracle, both, tally } = compare(format, left_out, ours, oracle, deep);
    let mut groups: Vec<_> = tally.groups.into_iter().collect();
    groups.sort_by(|a, b| b.1.entries.cmp(&a.1.entries).then(b.1.count.cmp(&a.1.count)).then(a.0.cmp(&b.0)));
    let mut summary = String::new();
    let _ = writeln!(
        summary,
        "entries: ours {}, oracle {}; only in ours {}, only in the oracle {}",
        ours.len(),
        oracle.len(),
        only_ours.len(),
        only_oracle.len()
    );
    let _ = writeln!(
        summary,
        "compared {}: equal {}, equal up to float printing {}, differing {}; {} groups of differences",
        both,
        tally.equal,
        tally.float_only,
        tally.differing,
        groups.len()
    );
    let line = |out: &mut String, (k, g): &((String, String, &'static str), Group)| {
        let _ =
            writeln!(out, "{:>7} entries {:>8} diffs  {}  {}  [{}]", g.entries, g.count, k.0, display_path(&k.1), k.2);
        if let Some((entry, path, a, b)) = &g.sample {
            let _ = writeln!(
                out,
                "        e.g. {entry} {}\n          ours:   {a}\n          oracle: {b}",
                display_path(path)
            );
        }
    };
    print!("{summary}");
    let mut by_class: BTreeMap<&str, u64> = BTreeMap::new();
    for (k, g) in &groups {
        *by_class.entry(&k.0).or_default() += g.entries;
    }
    for g in groups.iter().take(TOP) {
        let mut s = String::new();
        line(&mut s, g);
        print!("{s}");
    }
    if groups.len() > TOP {
        println!("... {} more groups{}", groups.len() - TOP, if report.is_some() { " (in the report)" } else { "" });
    }
    for (what, list) in [("only in ours", &only_ours), ("only in the oracle", &only_oracle)] {
        if !list.is_empty() {
            println!(
                "{what}: {}{}",
                list.iter().take(5).copied().collect::<Vec<_>>().join(", "),
                if list.len() > 5 { ", ..." } else { "" }
            );
        }
    }
    if let Some(file) = report {
        let mut out = summary.clone();
        out.push_str("\nentries with a difference per class (summed over groups):\n");
        let mut classes: Vec<_> = by_class.into_iter().collect();
        classes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (c, n) in classes {
            let _ = writeln!(out, "{n:>8}  {c}");
        }
        out.push_str("\ngroups:\n");
        for g in &groups {
            line(&mut out, g);
        }
        for (what, list) in [("only in ours", &only_ours), ("only in the oracle", &only_oracle)] {
            let _ = writeln!(out, "\n{what} ({}):", list.len());
            for p in list.iter() {
                let _ = writeln!(out, "  {p}");
            }
        }
        std::fs::write(file, out).map_err(|e| format!("{}: {e}", file.display()))?;
        println!("report: {}", file.display());
    }
    Ok(())
}

fn display_path(p: &str) -> &str {
    if p.is_empty() { "(root)" } else { p }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_import::kind::Kind;
    use serde_json::json;

    const MAGIC: &[u8; 4] = kinds::object::OBJECT_MAGIC;
    const FORMAT: Format = Format::JsonBin(MAGIC);

    fn walk(a: Value, b: Value) -> Walk {
        let mut w = Walk { diffs: Vec::new(), float: false };
        w.value(&a, &b, "", "", false);
        w
    }

    #[test]
    fn json_by_value() {
        let w = walk(json!({"a": 1, "b": [1.0, 2]}), json!({"b": [1, 2.0], "a": 1.0}));
        assert!(w.diffs.is_empty() && !w.float);
        let w = walk(json!({"x": 0.1}), json!({"x": 0.100000001}));
        assert!(w.diffs.is_empty() && w.float);
        // 0.1 as an f64 against the f32 0.1 written at f64 precision: a float printed two ways
        let w = walk(json!(0.1), json!(0.10000000149011612));
        assert!(w.diffs.is_empty() && w.float);
        // two ids above 2^24 one apart round to one f32 but are integers: different
        let w = walk(json!(16_777_217u32), json!(16_777_218u32));
        assert_eq!(w.diffs.len(), 1);
        assert!(!w.float);
        // integer against float: by value
        let w = walk(json!(3), json!(3.0));
        assert!(w.diffs.is_empty() && !w.float);
        // past f32's range both round to inf: a difference
        let w = walk(json!(1e39), json!(2e39));
        assert_eq!(w.diffs.len(), 1);
        let w = walk(
            json!({"class": "C", "inner": {"N_1": {"properties": {"P": [1, 2]}}}}),
            json!({"class": "C", "inner": {"N_1": {"properties": {"P": [1, 3, 4]}}}, "extra": true}),
        );
        let got: Vec<(&str, &str, &str)> =
            w.diffs.iter().map(|d| (d.cause, d.path.as_str(), d.collapsed.as_str())).collect();
        assert_eq!(
            got,
            [
                ("number", "inner.N_1.properties.P[1]", "inner.*.properties.P[*]"),
                ("array length", "inner.N_1.properties.P", "inner.*.properties.P"),
                ("key only in the oracle", "extra", "extra"),
            ]
        );
    }

    /// Two hand-made packs with known differences.
    #[test]
    fn packs_with_known_differences() {
        use mvs_store::writer::PackWriter;
        let dir = std::env::temp_dir().join(format!("mvs-import-compare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let container = |head: &str, body: &[u8]| {
            let mut e = MAGIC.to_vec();
            e.extend_from_slice(&(head.len() as u32).to_le_bytes());
            e.extend_from_slice(head.as_bytes());
            e.resize((e.len() + 15) & !15, 0);
            e.extend_from_slice(body);
            e
        };
        let ours: Vec<(&str, Vec<u8>)> = vec![
            ("/Game/Same.Same", br#"{"class":"C","a":1}"#.to_vec()),
            ("/Game/Order.Order", br#"{"b":[1,2],"class":"C","a":1}"#.to_vec()),
            ("/Game/Float.Float", br#"{"class":"C","x":0.30000001192092896}"#.to_vec()),
            ("/Game/Num.Num", br#"{"class":"C","properties":{"P":[1,2]}}"#.to_vec()),
            ("/Game/Num2.Num2", br#"{"class":"C","properties":{"P":[5,2]}}"#.to_vec()),
            ("/Game/Body.Body", container(r#"{"class":"M"}"#, &[1, 2, 3])),
            ("/Game/OnlyOurs.OnlyOurs", b"{}".to_vec()),
            ("/Game/Type.Type", br#"{"class":"C"}"#.to_vec()),
        ];
        let oracle: Vec<(&str, Vec<u8>)> = vec![
            ("/Game/Same.Same", br#"{"class":"C","a":1}"#.to_vec()),
            ("/Game/Order.Order", br#"{"a":1.0,"class":"C","b":[1,2]}"#.to_vec()),
            ("/Game/Float.Float", br#"{"class":"C","x":0.3}"#.to_vec()),
            ("/Game/Num.Num", br#"{"class":"C","properties":{"P":[1,3]}}"#.to_vec()),
            ("/Game/Num2.Num2", br#"{"class":"C","properties":{"P":[1,2]},"inner":{"A":{"k":1}}}"#.to_vec()),
            ("/Game/Body.Body", container(r#"{"class":"M"}"#, &[1, 2, 4])),
            ("/Game/OnlyOracle.OnlyOracle", b"{}".to_vec()),
            ("/Game/Type.Type", br#"{"class":"C"}"#.to_vec()),
        ];
        let write = |name: &str, list: &[(&str, Vec<u8>)]| {
            let f = dir.join(name);
            let mut w = PackWriter::create(&f).unwrap();
            for (p, b) in list {
                let ty = if name == "ours.pack" && p.contains("Type") {
                    "application/octet-stream"
                } else {
                    kinds::object::Object.content_type(b)
                };
                w.add(p, ty, b).unwrap();
            }
            w.finish().unwrap();
            Pack::open("object", &f).unwrap()
        };
        let (a, b) = (write("ours.pack", &ours), write("oracle.pack", &oracle));
        let c = compare(FORMAT, &[], &a, &b, false);
        assert_eq!(c.only_ours, ["/Game/OnlyOurs.OnlyOurs"]);
        assert_eq!(c.only_oracle, ["/Game/OnlyOracle.OnlyOracle"]);
        assert_eq!((c.both, c.tally.equal, c.tally.float_only, c.tally.differing), (7, 2, 1, 4));
        let mut groups: Vec<(String, String, &str, u64, u64)> =
            c.tally.groups.iter().map(|(k, g)| (k.0.clone(), k.1.clone(), k.2, g.entries, g.count)).collect();
        groups.sort();
        assert_eq!(
            groups,
            [
                ("?".into(), "".into(), "content type", 1, 1),
                ("C".into(), "inner".into(), "key only in the oracle", 1, 1),
                ("C".into(), "properties.P[*]".into(), "number", 2, 2),
                ("M".into(), "(body)".into(), "body bytes", 1, 1),
            ]
        );
        let sample = &c.tally.groups[&("C".to_string(), "properties.P[*]".to_string(), "number")].sample;
        assert_eq!(
            sample.as_ref().map(|s| (s.0.as_str(), s.1.as_str(), s.2.as_str(), s.3.as_str())),
            Some(("/Game/Num.Num", "properties.P[1]", "2", "3"))
        );
        // a key left out (top level) is in no group
        let c = compare(FORMAT, &["inner"], &a, &b, false);
        assert_eq!(c.tally.differing, 4);
        assert!(!c.tally.groups.keys().any(|k| k.1 == "inner"));
        drop((a, b));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two gamedata entries that JSON would call equal differ by bits, key order and a Float32Array's float.
    #[test]
    fn gamedata_exactly() {
        use mvs_data::value::{Props, Value as V, encode};
        let entry = |zero: f64, order: [&str; 2], last: f32| {
            let mut inner = Props::new();
            for k in order {
                inner.insert(k.into(), V::Num(1.0));
            }
            let mut p = Props::new();
            p.insert("z".into(), V::Num(zero));
            p.insert("same".into(), V::Arr(vec![V::Num(0.1), V::Num(f64::NAN)]));
            p.insert("inner".into(), V::Obj(inner));
            p.insert("frames".into(), V::F32s(vec![1.0, last]));
            encode(&V::Obj(p)).unwrap()
        };
        let a = entry(-0.0, ["a", "b"], 2.0);
        let b = entry(0.0, ["b", "a"], f32::from_bits(2f32.to_bits() + 1));
        let Outcome::Differ { class, diffs } = gamedata_entry("/GameData/Char/x.x", &a, &b) else { panic!("equal") };
        assert_eq!(class, "Char");
        let got: Vec<(&str, &str)> = diffs.iter().map(|d| (d.cause, d.path.as_str())).collect();
        assert_eq!(got, [("number", "z"), ("key order", "inner"), ("Float32Array floats", "frames")]);
        assert!(matches!(gamedata_entry("/GameData/Index.Index", &a, &a), Outcome::Equal));
        // the same values in other bytes (a section's padding cut)
        assert!(matches!(
            gamedata_entry("/GameData/Index.Index", &a, &a[..a.len() - 4]),
            Outcome::Differ { diffs, .. } if diffs[0].cause == "encoding"
        ));
    }

    #[test]
    fn nested_left_out() {
        let mut v = json!({"instances": [{"a": 1, "bounds": [1], "wpo": true}, {"a": 2}], "data": {"x": 1}, "keep": 1});
        for k in ["instances[].bounds", "instances[].wpo", "data", "missing.deep", "keep[].x"] {
            leave_out(&mut v, k);
        }
        assert_eq!(v, json!({"instances": [{"a": 1}, {"a": 2}], "keep": 1}));
        assert_eq!(left_out_of("scenery"), ["scenery.instances[].bounds", "scenery.instances[].wpo"]);
    }

    #[test]
    fn containers() {
        let mut e = MAGIC.to_vec();
        let head = br#"{"class":"X"}"#;
        e.extend_from_slice(&(head.len() as u32).to_le_bytes());
        e.extend_from_slice(head);
        e.resize((e.len() + 15) & !15, 0);
        e.extend_from_slice(&[1, 2, 3]);
        let d = decode(FORMAT, &e).unwrap();
        assert_eq!(d.json, json!({"class": "X"}));
        assert_eq!(d.body.as_deref(), Some(&[1u8, 2, 3][..]));
    }
}
