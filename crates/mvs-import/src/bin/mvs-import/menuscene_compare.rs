//! `compare --kind menuscene`: the kind's pack against the oracle store's. The oracle's entries are the format-1
//! header, ours the format-4 one (mvs_material::menuscene); an entry is matched by its
//! name below the kinds' versioned folders. Both headers are read in their members' order and brought to one shape
//! before they are compared:
//! - the oracle's data goes under its entry kind's tag (`{"PreMatch": data}`, a preview's `Preview`), and the levels'
//!   `textures` tables, which format 2 dropped (the material kind holds a material's textures), are left out;
//! - the oracle MainMenu entry's `character` and `lights` (Shaggy's placement, HomescreenIdle and rig, which format 3
//!   moved to each skin's preview entry), our preview's `homescreenIdle` and our MainMenu entry's `events` (the
//!   background's Event Lookup Table; the oracle's has neither) are left out;
//! - a material (the oracle's GLSL material, ours a SceneMaterial) becomes `{"draw": <material name>}`, the oracle's
//!   name against the short name of the material our DrawRef names, with its parameter tracks where it has any (the
//!   oracle's glow keeps its forward tracks beside the material, format 2 in it);
//! - the header's `version` and `bindVersion` are left out (the formats' own), and so is a PreMatch piece's
//!   `customDepth.draw` (format 2's DrawRef of the pass, which the oracle drew with its own GLSL).
//!
//! Each of our entries is first read by the format's reader (`read_entry`: every DrawRef parses) and written again
//! (`entry_bytes`): the bytes must be the same.
//!
//! What remains must be equal, numbers by value and object members in order, but for two places whose order the
//! format does not keep: a sequence track's members (its `type` tag is written first) and a montage notify's `props`
//! (a serde_json value, its members sorted). A difference is printed by its path, array indices collapsed, counted per
//! path.

use std::collections::BTreeMap;
use std::path::Path;

use mvs_import::json::{Json, to_vec};
use mvs_import::kinds::menuscene::entry_bytes;
use mvs_material::draw::parse_draw_ref;
use mvs_material::jsonbin::JsonBin;
use mvs_material::menuscene::{MAGIC, read_entry};
use mvs_store::Pack;

type R<T> = Result<T, String>;

/// The entry's name below the versioned folder (`/MenuScene/b3s2o11/Preview/a/B.B` -> `Preview/a/B.B`).
fn suffix(p: &str) -> &str {
    let rest = p.strip_prefix("/MenuScene/").unwrap_or(p);
    rest.find('/').map_or(rest, |i| &rest[i + 1..])
}

fn header(bytes: &[u8], what: &str) -> R<Json> {
    let jb = JsonBin::split(MAGIC, bytes, what).map_err(|e| e.to_string())?;
    Json::parse(jb.header).map_err(|e| format!("{what}: {e}"))
}

fn short(p: &str) -> &str {
    let p = &p[p.rfind('/').map_or(0, |i| i + 1)..];
    let p = p.rfind('.').map_or(p, |i| &p[i + 1..]);
    p.rfind(':').map_or(p, |i| &p[i + 1..])
}

fn obj(v: &mut Json) -> Option<&mut indexmap::IndexMap<String, Json>> {
    match v {
        Json::Obj(m) => Some(m),
        _ => None,
    }
}

/// A material as `{"draw": name[, "tracks": ...]}`.
fn material(name: &str, tracks: Option<Json>) -> Json {
    let mut m = indexmap::IndexMap::new();
    m.insert("draw".to_owned(), Json::Str(name.to_owned()));
    if let Some(t) = tracks.filter(|t| !matches!(t, Json::Arr(a) if a.is_empty())) {
        m.insert("tracks".to_owned(), t);
    }
    Json::Obj(m)
}

/// The oracle's materials (objects with `pre` and `shader`) by name.
fn oracle_materials(v: &mut Json) {
    match v {
        Json::Arr(a) => a.iter_mut().for_each(oracle_materials),
        Json::Obj(m) => {
            if m.contains_key("pre") && m.contains_key("shader") {
                let n = m.get("name").and_then(Json::as_str).unwrap_or_default().to_owned();
                *v = material(&n, None);
                return;
            }
            m.values_mut().for_each(oracle_materials);
        }
        _ => {}
    }
}

/// Our SceneMaterials (objects with a `draw` DrawRef) by their material's short name.
fn our_materials(v: &mut Json) {
    match v {
        Json::Arr(a) => a.iter_mut().for_each(our_materials),
        Json::Obj(m) if m.contains_key("stencil") => {
            // a piece's customDepth: the oracle has no draw for its pass
            m.shift_remove("draw");
        }
        Json::Obj(m) => {
            if let Some(d) = m.get("draw").and_then(Json::as_str).and_then(parse_draw_ref) {
                let n = short(d.material).to_owned();
                let t = m.get("tracks").cloned();
                *v = material(&n, t);
                return;
            }
            m.values_mut().for_each(our_materials);
        }
        _ => {}
    }
}

fn oracle_shape(mut h: Json) -> R<Json> {
    let m = obj(&mut h).ok_or("oracle header is not an object")?;
    m.shift_remove("version");
    m.shift_remove("bindVersion");
    let name = m.get("name").and_then(Json::as_str).unwrap_or_default().to_owned();
    let tag = if name.starts_with("Preview/") { "Preview".to_owned() } else { name };
    let mut data = m.shift_remove("data").ok_or("oracle header has no data")?;
    if let Some(d) = obj(&mut data) {
        if tag != "Preview" {
            d.shift_remove("textures");
        }
        if tag == "MainMenu" {
            d.shift_remove("character");
            d.shift_remove("lights");
        }
        // the glow's forward tracks go into its material
        if let Some(g) = d.get_mut("glow").and_then(obj) {
            let tracks = g.shift_remove("tracks");
            oracle_materials(&mut data);
            if let Some(g) = obj(&mut data).and_then(|d| d.get_mut("glow")).and_then(obj)
                && let Some(Json::Obj(mat)) = g.get_mut("material")
                && let Some(t) = tracks
            {
                mat.insert("tracks".to_owned(), t);
            }
        }
    }
    oracle_materials(&mut data);
    let mut tagged = indexmap::IndexMap::new();
    tagged.insert(tag, data);
    m.insert("data".to_owned(), Json::Obj(tagged));
    Ok(h)
}

fn our_shape(mut h: Json) -> R<Json> {
    let m = obj(&mut h).ok_or("our header is not an object")?;
    m.shift_remove("version");
    m.shift_remove("bindVersion");
    if let Some(d) = m.get_mut("data") {
        if let Some(p) = obj(d).and_then(|d| d.get_mut("Preview")).and_then(obj) {
            p.shift_remove("homescreenIdle");
        }
        if let Some(p) = obj(d).and_then(|d| d.get_mut("MainMenu")).and_then(obj) {
            p.shift_remove("events");
        }
        our_materials(d);
    }
    Ok(h)
}

fn collapse(path: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in path.chars() {
        match c {
            '[' => {
                out.push_str("[]");
                skip = true;
            }
            ']' => skip = false,
            _ if skip => {}
            _ => out.push(c),
        }
    }
    out
}

/// Whether an object's member order is not compared at `path` (the module's note).
fn order_free(path: &str) -> bool {
    let c = collapse(path);
    c.contains(".notifies[].props") || (c.starts_with(".data.PreMatch.sequences.") && c.ends_with(".tracks[]"))
}

/// The differences of two values: (path, ours, oracle).
fn walk(a: &Json, b: &Json, path: &str, out: &mut Vec<(String, String)>) {
    let show = |v: &Json| {
        let s = String::from_utf8(to_vec(v).unwrap_or_default()).unwrap_or_default();
        if s.len() > 160 { format!("{}...", &s[..s.floor_char_boundary(160)]) } else { s }
    };
    match (a, b) {
        (Json::Obj(x), Json::Obj(y)) => {
            if !x.keys().eq(y.keys()) {
                let (kx, ky): (Vec<_>, Vec<_>) = (x.keys().collect(), y.keys().collect());
                let mut sx = kx.clone();
                let mut sy = ky.clone();
                sx.sort();
                sy.sort();
                if sx != sy || !order_free(path) {
                    let what = if sx == sy { "member order" } else { "members" };
                    out.push((format!("{path} ({what})"), format!("{kx:?} vs oracle {ky:?}")));
                }
            }
            for (k, v) in x {
                if let Some(w) = y.get(k) {
                    walk(v, w, &format!("{path}.{k}"), out);
                }
            }
        }
        (Json::Arr(x), Json::Arr(y)) => {
            if x.len() != y.len() {
                out.push((format!("{path} (length)"), format!("{} vs oracle {}", x.len(), y.len())));
            }
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                walk(v, w, &format!("{path}[{i}]"), out);
            }
        }
        (Json::Num(x), Json::Num(y)) if x == y || (x.is_nan() && y.is_nan()) => {}
        _ if a == b => {}
        _ => out.push((path.to_owned(), format!("{} vs oracle {}", show(a), show(b)))),
    }
}

pub fn run(ours: &Pack, oracle: &Pack, report: Option<&Path>) -> R<()> {
    let mine: BTreeMap<&str, &str> = ours.paths().map(|p| (suffix(p), p)).collect();
    let mut paths: Vec<&str> = oracle.paths().collect();
    paths.sort_unstable();
    let (mut equal, mut differ) = (0usize, 0usize);
    let mut only_oracle = Vec::new();
    let mut groups: BTreeMap<String, (usize, String, String)> = BTreeMap::new();
    let mut lines = Vec::new();
    for p in &paths {
        let Some(our_path) = mine.get(suffix(p)) else {
            only_oracle.push(*p);
            continue;
        };
        let bytes = ours.bytes(our_path).map_err(|e| format!("{our_path}: {e}"))?;
        // our entry reads as format 2 (every DrawRef parses) and writes back to the same bytes
        let read = read_entry(&bytes, our_path).map_err(|e| e.to_string())?;
        if entry_bytes(&read).map_err(|e| e.to_string())? != bytes {
            return Err(format!("{our_path}: the entry read and written again differs"));
        }
        let a = our_shape(header(&bytes, our_path)?)?;
        let b = oracle_shape(header(&oracle.bytes(p).map_err(|e| format!("{p}: {e}"))?, p)?)?;
        let mut out = Vec::new();
        walk(&a, &b, "", &mut out);
        if out.is_empty() {
            equal += 1;
            continue;
        }
        differ += 1;
        for (path, what) in out {
            lines.push(format!("{}  {path}: {what}", suffix(p)));
            let g = groups.entry(collapse(&path)).or_insert((0, suffix(p).to_owned(), what));
            g.0 += 1;
        }
    }
    let only_ours: Vec<&&str> =
        mine.iter().filter(|(s, _)| !paths.iter().any(|p| suffix(p) == **s)).map(|(_, p)| p).collect();
    println!(
        "menuscene: {} oracle entries, {equal} equal, {differ} differ, {} only in the oracle, {} only ours",
        paths.len(),
        only_oracle.len(),
        only_ours.len()
    );
    for p in &only_oracle {
        println!("  only in the oracle: {p}");
    }
    for p in &only_ours {
        println!("  only ours: {p}");
    }
    for (path, (n, first, what)) in &groups {
        println!("  {n:5}  {path}  (first {first}: {what})");
    }
    if let Some(r) = report {
        std::fs::write(r, lines.join("\n")).map_err(|e| format!("{}: {e}", r.display()))?;
    }
    if differ > 0 || !only_oracle.is_empty() || !only_ours.is_empty() {
        return Err("menuscene: the packs differ".into());
    }
    Ok(())
}
