//! The store's particles kind: its index, a system's
//! entry with its emitters and modules, the distributions the modules sample, and a particle mesh's bounds from the
//! mesh kind. A module stays a JSON object here; the template reads its properties once (template.rs).

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result};

/// The entry layout this reader takes (PARTICLES_FORMAT_VERSION).
pub const PARTICLES_FORMAT_VERSION: u32 = 1;

fn bad(what: &str, why: impl Into<String>) -> Error {
    Error::Format { what: what.to_string(), why: why.into() }
}

/// The index entry: the owners of every system and particle mesh (the materials, decals and components are the
/// renderer's).
#[derive(Debug, Deserialize)]
pub struct ParticlesIndex {
    pub kind: String,
    pub version: u32,
    pub systems: IndexMap<String, SystemRef>,
    pub meshes: IndexMap<String, MeshRef>,
}

#[derive(Debug, Deserialize)]
pub struct SystemRef {
    /// The system's build object path.
    pub path: String,
    /// The parts that load it (a fighter, `base`).
    #[serde(default)]
    pub owners: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MeshRef {
    /// The mesh kind's entry path (the mesh's build object path).
    pub path: String,
    /// The one section an emitter draws (else every section).
    pub section: Option<u32>,
}

/// A system's entry.
#[derive(Debug, Deserialize)]
pub struct SystemEntry {
    pub kind: String,
    pub version: u32,
    pub path: String,
    pub system: SystemData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemData {
    pub emitters: Vec<EmitterData>,
    /// bUseFixedRelativeBoundingBox's FixedRelativeBoundingBox: min xyz, max xyz.
    pub fixed_box: Option<Vec<f64>>,
    #[serde(rename = "WarmupTime")]
    pub warmup_time: Option<f64>,
    #[serde(rename = "WarmupTickRate")]
    pub warmup_tick_rate: Option<f64>,
    #[serde(rename = "bOrientZAxisTowardCamera")]
    pub orient_z: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct EmitterData {
    pub name: String,
    pub modules: Vec<Value>,
    /// A mesh emitter's mesh: a key of the index's meshes.
    pub mesh: Option<String>,
    #[serde(rename = "bUseLegacySpawningBehavior")]
    pub legacy_spawning: Option<bool>,
    pub index: Option<i64>,
}

pub fn parse_index(bytes: &[u8]) -> Result<ParticlesIndex> {
    let ix: ParticlesIndex = serde_json::from_slice(bytes).map_err(|e| bad("particles index", e.to_string()))?;
    if ix.kind != "index" {
        return Err(bad("particles index", format!("kind {}", ix.kind)));
    }
    if ix.version != PARTICLES_FORMAT_VERSION {
        return Err(bad("particles index", format!("version {}, the reader's {PARTICLES_FORMAT_VERSION}", ix.version)));
    }
    Ok(ix)
}

pub fn parse_system(path: &str, bytes: &[u8]) -> Result<SystemEntry> {
    let s: SystemEntry = serde_json::from_slice(bytes).map_err(|e| bad(path, e.to_string()))?;
    if s.kind != "system" {
        return Err(bad(path, format!("kind {}, not a particle system entry", s.kind)));
    }
    if s.version != PARTICLES_FORMAT_VERSION {
        return Err(bad(path, format!("entry version {}, the reader's {PARTICLES_FORMAT_VERSION}", s.version)));
    }
    Ok(s)
}

/// A mesh entry's ExtendedBounds BoxExtent : the jsonbin header of the
/// mesh kind's MVM1 container.
pub fn mesh_extent(path: &str, bytes: &[u8]) -> Result<[f64; 3]> {
    if bytes.len() < 8 || &bytes[..4] != b"MVM1" {
        return Err(bad(path, "not an MVM1 mesh entry"));
    }
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let head = bytes.get(8..8 + len).ok_or_else(|| bad(path, "mesh header cut short"))?;
    #[derive(Deserialize)]
    struct Bounds {
        extent: [f64; 3],
    }
    #[derive(Deserialize)]
    struct Head {
        bounds: Bounds,
    }
    let h: Head = serde_json::from_slice(head).map_err(|e| bad(path, e.to_string()))?;
    Ok(h.bounds.extent)
}

// ---------- a module's properties, read the way JavaScript reads them ----------

/// The property, None for an absent or null one (the left side of JavaScript's `??`).
pub fn prop<'a>(m: &'a Value, k: &str) -> Option<&'a Value> {
    m.get(k).filter(|v| !v.is_null())
}

/// JavaScript truthiness of a value.
pub fn truthy_value(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `!!m[k]`.
pub fn truthy(m: &Value, k: &str) -> bool {
    prop(m, k).is_some_and(truthy_value)
}

/// `m[k] ?? d`, as a condition.
pub fn flag_or(m: &Value, k: &str, d: bool) -> bool {
    prop(m, k).map_or(d, truthy_value)
}

/// A value as a JavaScript number (a boolean is 0 or 1, anything else not a number NaN).
pub fn number_value(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

/// `m[k] ?? d`, as a number.
pub fn num_or(m: &Value, k: &str, d: f64) -> f64 {
    prop(m, k).map_or(d, number_value)
}

/// `String(m[k] ?? d)`.
pub fn str_or(m: &Value, k: &str, d: &str) -> String {
    match prop(m, k) {
        None => d.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => {
            a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect::<Vec<_>>().join(",")
        }
        Some(v) => v.to_string(),
    }
}

/// An `{X, Y, Z}` struct property's axes, each `?? d`.
pub fn xyz_or(m: &Value, k: &str, d: f64) -> [f64; 3] {
    match prop(m, k) {
        None => [d; 3],
        Some(v) => [num_or(v, "X", d), num_or(v, "Y", d), num_or(v, "Z", d)],
    }
}

// ---------- distributions ----------

/// A baked FRawDistribution lookup table.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub op: f64,
    pub n: f64,
    pub st: f64,
    pub sub: f64,
    pub ts: f64,
    pub tb: f64,
    pub v: Vec<f64>,
    pub lock: f64,
}

/// A DistributionVectorConstantCurve the build did not bake: 11 numbers a point (InVal, OutVal xyz, ArriveTangent
/// xyz, LeaveTangent xyz, mode 0 linear / 2 constant / 1 cubic) and its LockedAxes.
#[derive(Clone, Debug, Default)]
pub struct Curve {
    pub v: Vec<f64>,
    pub lock: f64,
}

/// One axis of a parameter distribution: its Constant, mode and ranges.
#[derive(Clone, Debug)]
pub struct ParamAxis {
    pub constant: f64,
    pub direct: bool,
    pub abs: bool,
    pub min_in: f64,
    pub max_in: f64,
    pub min_out: f64,
    pub g: f64,
}

/// A DistributionFloatParticleParameter (one axis) or DistributionVectorParticleParameter (three axes, read from
/// `Name.X` / `.Y` / `.Z` before `Name`).
#[derive(Clone, Debug)]
pub struct ParamDist {
    pub name: String,
    pub keys: Option<[String; 3]>,
    pub axes: Vec<ParamAxis>,
}

/// A module's distribution property: a table, an unbaked curve, a parameter distribution (which samples only with a
/// table beside it), or nothing (every value 0).
#[derive(Clone, Debug, Default)]
pub struct Dist {
    pub table: Option<Table>,
    pub curve: Option<Curve>,
    pub param: Option<ParamDist>,
}

fn param_axis(constant: f64, mode: &str, min_in: f64, max_in: f64, min_out: f64, max_out: f64) -> ParamAxis {
    ParamAxis {
        constant,
        direct: mode.ends_with("DPM_Direct"),
        abs: mode.ends_with("DPM_Abs"),
        min_in,
        max_in,
        min_out,
        g: if max_in > min_in { (max_out - min_out) / (max_in - min_in) } else { 0.0 },
    }
}

fn param_dist(p: &Value) -> ParamDist {
    let name = str_or(p, "ParameterName", "undefined");
    let constant = prop(p, "Constant");
    if let Some(c) = constant.filter(|c| c.is_object()) {
        let modes = prop(p, "ParamModes");
        let axes = (0..3)
            .map(|k| {
                let a = ["X", "Y", "Z"][k];
                let mode = modes.and_then(|m| m.get(k)).filter(|v| !v.is_null()).map_or_else(
                    || "DPM_Normal".to_string(),
                    |v| v.as_str().map_or_else(|| v.to_string(), str::to_string),
                );
                let sub = |key: &str, d: f64| prop(p, key).map_or(d, |v| num_or(v, a, d));
                param_axis(
                    num_or(c, a, 0.0),
                    &mode,
                    sub("MinInput", 0.0),
                    sub("MaxInput", 1.0),
                    sub("MinOutput", 0.0),
                    sub("MaxOutput", 1.0),
                )
            })
            .collect();
        return ParamDist { keys: Some([format!("{name}.X"), format!("{name}.Y"), format!("{name}.Z")]), name, axes };
    }
    let mode =
        if prop(p, "ParamMode").is_some() { str_or(p, "ParamMode", "") } else { str_or(p, "ParamModes", "DPM_Normal") };
    let constant = constant.map_or(0.0, number_value);
    let axis = param_axis(
        constant,
        &mode,
        num_or(p, "MinInput", 0.0),
        num_or(p, "MaxInput", 1.0),
        num_or(p, "MinOutput", 0.0),
        num_or(p, "MaxOutput", 1.0),
    );
    ParamDist { name, keys: None, axes: vec![axis] }
}

fn numbers(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map_or_else(Vec::new, |a| a.iter().map(number_value).collect())
}

/// A distribution property of a module (absent: every value 0).
pub fn dist(m: &Value, k: &str) -> Dist {
    prop(m, k).map_or_else(Dist::default, dist_of)
}

/// A distribution from its JSON object.
pub fn dist_of(d: &Value) -> Dist {
    let table = prop(d, "table").filter(|t| truthy_value(t)).map(|t| Table {
        op: num_or(t, "op", f64::NAN),
        n: num_or(t, "n", f64::NAN),
        st: num_or(t, "st", f64::NAN),
        sub: num_or(t, "sub", f64::NAN),
        ts: num_or(t, "ts", f64::NAN),
        tb: num_or(t, "tb", f64::NAN),
        v: numbers(prop(t, "v")),
        lock: num_or(t, "lock", 0.0),
    });
    let curve = prop(d, "curve")
        .filter(|c| truthy_value(c))
        .map(|c| Curve { v: numbers(prop(c, "v")), lock: num_or(c, "lock", 0.0) });
    let param = prop(d, "param").filter(|p| truthy_value(p)).map(param_dist);
    Dist { table, curve, param }
}

impl Dist {
    /// FDistributionLookupTable::GetRange's high end: the largest baked value, 0 without one.
    pub fn table_max(&self) -> f64 {
        match &self.table {
            Some(t) if !t.v.is_empty() => t.v.iter().copied().fold(f64::NEG_INFINITY, mvs_math::js::max),
            _ => 0.0,
        }
    }
}
