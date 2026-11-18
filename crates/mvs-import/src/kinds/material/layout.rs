//! A material's uniform buffer layout with its preshaders evaluated (`mvs_material::preshader`), its textures, its
//! parameter collections and the shader rows of its map.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};

use mvs_material::preshader::{PreshaderInputs, eval};
use mvs_uasset::natives::frozen::{MaterialMapContent, ShaderMapContentRecord, ShaderRecord, UniformExpressionSet};
use mvs_uasset::value::Value;

use super::super::shadermap::{Js, Obj, write_array};
use super::resolve::{ResolvedMaterial, color};
use crate::kind::{Cx, KindError};
use crate::objview::{field, num, string};

type V4 = [f64; 4];
/// A material's scalar and vector parameters by name.
type Params = (Vec<(String, f64)>, Vec<(String, V4)>);
/// The evaluated vectors and scalars and the referenced textures whose size a preshader asked for.
type Evaluated = (Vec<V4>, Vec<f64>, BTreeSet<usize>);

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(why))
}

// ---------------------------------------------------------------- the shader rows

/// Bytes of one shader row.
pub const SHADER_ROW_BYTES: usize = 40;

/// One shader row: the hashed names as numbers (0: none).
#[derive(Clone, Copy, Debug)]
struct ShaderRow {
    map: u64,
    type_: u64,
    pipeline: u64,
    permutation: i32,
    frequency: u8,
    code: u32,
    params: u32,
}

fn hex64(h: &str) -> Result<u64, KindError> {
    u64::from_str_radix(h, 16).map_err(|_| KindError::Failed(format!("material: {h} is not a hashed name")))
}

/// The rows, codes and parameter records of shader maps: each FShader once per place it has, each code and each
/// distinct parameter record once.
pub struct RowBuilder {
    what: String,
    rows: Vec<ShaderRow>,
    /// Each distinct code's SHA-1 (upper-case hex).
    pub codes: Vec<String>,
    /// Each distinct parameter record, as its JSON.
    pub params: Vec<Vec<u8>>,
    code_at: HashMap<String, usize>,
    params_at: HashMap<Vec<u8>, usize>,
}

impl RowBuilder {
    pub fn new(what: &str) -> RowBuilder {
        RowBuilder {
            what: what.to_owned(),
            rows: Vec::new(),
            codes: Vec::new(),
            params: Vec::new(),
            code_at: HashMap::new(),
            params_at: HashMap::new(),
        }
    }

    fn row(
        &mut self,
        s: &ShaderRecord,
        map: Option<&str>,
        pipeline: Option<&str>,
        permutation: i32,
    ) -> Result<ShaderRow, KindError> {
        let what = &self.what;
        let Some(hash) = &s.hash else {
            return failed(format!(
                "material: {what}: a shader of type {} shares its map's code (bShareCode)",
                s.type_
            ));
        };
        if s.uniform_buffer_parameter_structs.len() != s.uniform_buffer_parameters.len() {
            return failed(format!(
                "material: {what}: {hash}: {} uniform buffer structs, {} parameters",
                s.uniform_buffer_parameter_structs.len(),
                s.uniform_buffer_parameters.len()
            ));
        }
        let code = match self.code_at.get(hash) {
            Some(&c) => c,
            None => {
                self.codes.push(hash.clone());
                self.code_at.insert(hash.clone(), self.codes.len() - 1);
                self.codes.len() - 1
            }
        };
        let mut rec = Vec::new();
        let mut o = Obj::new(&mut rec);
        let pairs: Vec<(&String, &u16)> =
            s.uniform_buffer_parameter_structs.iter().zip(&s.uniform_buffer_parameters).collect();
        write_array(o.key("uniformBuffers"), &pairs, |out, (h, r)| {
            out.push(b'[');
            h.js(out);
            out.push(b',');
            r.js(out);
            out.push(b']');
        });
        o.f("parameterMap", &s.parameter_map_info).f("bindings", &s.bindings);
        o.end();
        let params = match self.params_at.get(&rec) {
            Some(&p) => p,
            None => {
                self.params.push(rec.clone());
                self.params_at.insert(rec, self.params.len() - 1);
                self.params.len() - 1
            }
        };
        let row = ShaderRow {
            map: map.map_or(Ok(0), hex64)?,
            type_: hex64(&s.type_)?,
            pipeline: pipeline.map_or(Ok(0), hex64)?,
            permutation,
            frequency: u8::try_from(s.frequency & 0xff).expect("a byte"),
            code: u32::try_from(code).expect("under 2^32 codes"),
            params: u32::try_from(params).expect("under 2^32 records"),
        };
        Ok(row)
    }

    /// A map's shaders and pipelines; `map` is the row's map hash.
    pub fn add(&mut self, content: &ShaderMapContentRecord, map: Option<&str>) -> Result<(), KindError> {
        for s in &content.shaders {
            let r = self.row(s, map, None, s.permutation)?;
            self.rows.push(r);
        }
        for p in &content.shader_pipelines {
            for (f, s) in p.shaders.iter().enumerate() {
                let Some(s) = s else { continue };
                if usize::try_from(s.frequency).ok() != Some(f).filter(|f| *f < 5) {
                    return failed(format!(
                        "material: {}: pipeline {}'s shader {f} has frequency {}",
                        self.what, p.type_name, s.frequency
                    ));
                }
                let r = self.row(s, map, Some(&p.type_name), p.permutation_ids.get(f).copied().unwrap_or(0))?;
                self.rows.push(r);
            }
        }
        Ok(())
    }

    /// The rows section's bytes: u64 map, u64 type, u64 pipeline, i32 permutation,
    /// u8 frequency and three zero bytes, u32 code, u32 params, little-endian.
    pub fn shader_rows(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rows.len() * SHADER_ROW_BYTES);
        for r in &self.rows {
            out.extend_from_slice(&r.map.to_le_bytes());
            out.extend_from_slice(&r.type_.to_le_bytes());
            out.extend_from_slice(&r.pipeline.to_le_bytes());
            out.extend_from_slice(&r.permutation.to_le_bytes());
            out.extend_from_slice(&[r.frequency, 0, 0, 0]);
            out.extend_from_slice(&r.code.to_le_bytes());
            out.extend_from_slice(&r.params.to_le_bytes());
        }
        out
    }
}

// ---------------------------------------------------------------- the parameters

/// An array property's elements; empty for none.
fn array(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// The cached value of `name` among a cached entries struct's ParameterInfos (`cachedVal`).
fn cached_val<'v>(entries: Option<&Value>, values: Option<&'v Value>, name: &str, cx: &Cx<'_>) -> Option<&'v Value> {
    let Some(Value::Struct(e)) = entries else { return None };
    let at = array(field(e, "ParameterInfos")).iter().position(|p| {
        matches!(p, Value::Struct(s) if field(s, "Name").and_then(|n| string(cx.cache, n)).as_deref() == Some(name))
    })?;
    match values {
        Some(Value::Array(a)) => a.get(at),
        _ => None,
    }
}

/// The value of each uniform parameter the map's preshaders read: the nearest instance's, then (RefractionDepthBias)
/// the base's property, then the base Material's cached value of that name, then the map's default.
fn material_params(cx: &Cx<'_>, m: &ResolvedMaterial<'_>, ues: &UniformExpressionSet) -> Params {
    let cached = match m.prop("CachedExpressionData") {
        Some(Value::Struct(c)) => match field(c, "Parameters") {
            Some(Value::Struct(p)) => Some(p),
            _ => None,
        },
        _ => None,
    };
    let get = |k: &str| cached.and_then(|c| field(c, k));
    let scalars = ues
        .uniform_scalar_parameters
        .iter()
        .map(|p| {
            let name = &p.parameter_info.name;
            let value = if let Some(v) = m.scalars.get(name) {
                *v
            } else if name == "RefractionDepthBias" {
                m.prop("RefractionDepthBias").and_then(num).unwrap_or(0.0)
            } else {
                cached_val(get("RuntimeEntries"), get("ScalarValues"), name, cx)
                    .and_then(num)
                    .unwrap_or_else(|| f64::from(p.default_value))
            };
            (name.clone(), value)
        })
        .collect();
    let vectors = ues
        .uniform_vector_parameters
        .iter()
        .map(|p| {
            let name = &p.parameter_info.name;
            let value = if name == "SelectionColor" {
                [0.0; 4]
            } else if let Some(v) = m.vectors.get(name) {
                *v
            } else {
                cached_val(get("RuntimeEntries[1]"), get("VectorValues"), name, cx)
                    .and_then(color)
                    .unwrap_or_else(|| p.default_value.map(f64::from))
            };
            (name.clone(), value)
        })
        .collect();
    (scalars, vectors)
}

/// The values a preshader reads: the parameters and the sizes of the referenced textures it asks for.
struct Inputs<'a> {
    scalars: &'a [(String, f64)],
    vectors: &'a [(String, V4)],
    sizes: &'a [Option<V4>],
    asked: RefCell<BTreeSet<usize>>,
    /// The first parameter index a program read past the map's parameters.
    bad: Cell<Option<String>>,
}

impl PreshaderInputs for Inputs<'_> {
    fn scalar(&self, k: usize) -> f64 {
        match self.scalars.get(k) {
            Some(s) => s.1,
            None => {
                self.bad.set(Some(format!("scalar parameter {k} of {}", self.scalars.len())));
                0.0
            }
        }
    }

    fn vector(&self, k: usize) -> V4 {
        match self.vectors.get(k) {
            Some(v) => v.1,
            None => {
                self.bad.set(Some(format!("vector parameter {k} of {}", self.vectors.len())));
                [0.0; 4]
            }
        }
    }

    fn texture_size(&self, k: usize) -> Option<V4> {
        self.asked.borrow_mut().insert(k);
        Some(self.sizes.get(k).copied().flatten().unwrap_or([0.0; 4]))
    }
}

/// A texture parameter of the uniform expression set, in FillUniformBuffer's order (2D, cube, 2D array, volume).
pub struct LayoutTexture {
    pub name: String,
    pub type_: &'static str,
    pub texture_index: i32,
    pub sampler_source: u8,
}

/// The material uniform buffer as the material sets it.
pub struct Layout {
    pub size: u32,
    /// The buffer's float4 rows as evaluated: the vectors' rows then the scalars packed by four.
    pub values: Vec<f64>,
    pub scalar_params: Vec<(String, f64)>,
    pub vector_params: Vec<(String, V4)>,
    pub textures: Vec<LayoutTexture>,
    /// By referenced texture index, [SizeX, SizeY, 0, 0] for each one a TextureSize opcode reads; `None` when no
    /// preshader reads one.
    pub texture_sizes: Option<Vec<Option<V4>>>,
}

const TEXTURE_TYPES: [&str; 4] = ["2d", "cube", "array", "volume"];

/// The texture table the material's shader map indexes: the static-permutation instance's CachedReferencedTextures,
/// else the base's CachedExpressionData.ReferencedTextures.
fn referenced_textures(cx: &Cx<'_>, m: &ResolvedMaterial<'_>) -> Vec<Option<String>> {
    if let Some(r) = &m.ref_tex {
        return r.clone();
    }
    match m.prop("CachedExpressionData") {
        Some(Value::Struct(c)) => array(field(c, "ReferencedTextures")).iter().map(|v| string(cx.cache, v)).collect(),
        _ => Vec::new(),
    }
}

/// The material uniform buffer as the material sets it: each preshader evaluated over its parameters, the vectors'
/// rows then the scalars packed by four. The preshader bytes are the map's.
pub fn material_layout(cx: &Cx<'_>, m: &ResolvedMaterial<'_>, map: &MaterialMapContent) -> Result<Layout, KindError> {
    let ues = &map.record.material_compilation_output.uniform_expression_set;
    let pre = map.preshaders.as_slice();
    let (scalars, vectors) = material_params(cx, m, ues);
    // The TextureSize opcode (0x25) reads a referenced texture's resource size, [SizeX, SizeY, 0, 0] (SizeZ 0 for 2D),
    // by its index into the map's referenced textures; it names its parameter by index into the preshader name table,
    // which holds only None for the build's materials (a named one fails). A first run finds the indices the
    // preshaders ask for (the sizes given as 0), a second evaluates with their sizes; a texture the build does not
    // hold has no size and fails when a preshader asks for it.
    if ues.preshader_names.iter().any(|n| n != "None") {
        return failed(format!("drawmat: {}: named preshader texture parameters", m.path));
    }
    let eval_all = |sizes: &[Option<V4>]| -> Result<Evaluated, KindError> {
        let inputs = Inputs {
            scalars: &scalars,
            vectors: &vectors,
            sizes,
            asked: RefCell::new(BTreeSet::new()),
            bad: Cell::new(None),
        };
        let mut stack = Vec::with_capacity(16);
        let mut run = |h: &mvs_uasset::natives::frozen::UniformPreshaderHeader| -> Result<V4, KindError> {
            let (off, size) = (h.opcode_offset as usize, h.opcode_size as usize);
            let Some(code) = pre.get(off..off + size) else {
                return failed(format!(
                    "drawmat: {}: a preshader at {off}+{size} past the section's {}",
                    m.path,
                    pre.len()
                ));
            };
            let v =
                eval(code, &inputs, &mut stack).map_err(|e| KindError::Failed(format!("drawmat: {}: {e}", m.path)))?;
            if let Some(bad) = inputs.bad.take() {
                return failed(format!("drawmat: {}: a preshader reads {bad}", m.path));
            }
            Ok(v)
        };
        let vecs = ues.uniform_vector_preshaders.iter().map(&mut run).collect::<Result<Vec<_>, _>>()?;
        let mut scal = Vec::new();
        for h in &ues.uniform_scalar_preshaders {
            scal.push(run(h)?[0]);
        }
        Ok((vecs, scal, inputs.asked.into_inner()))
    };
    let (mut vecs, mut scal, asked) = eval_all(&[])?;
    let mut texture_sizes = None;
    if !asked.is_empty() {
        let refs = referenced_textures(cx, m);
        let mut sizes = Vec::with_capacity(refs.len());
        for (k, r) in refs.iter().enumerate() {
            if !asked.contains(&k) {
                sizes.push(None);
                continue;
            }
            let native = match r {
                Some(r) if !r.is_empty() && cx.exists(r) => cx.object(r)?.native()?,
                _ => None,
            };
            let dim = |key: &str| match native.as_ref().and_then(|n| n.get(key)) {
                Some(Value::Int(i)) => Some(*i as f64),
                Some(Value::UInt(u)) => Some(*u as f64),
                _ => None,
            };
            let (Some(x), Some(y)) = (dim("SizeX"), dim("SizeY")) else {
                return failed(format!(
                    "drawmat: {}: a preshader reads the size of referenced texture {k} ({}), which the build does not hold",
                    m.path,
                    r.as_deref().unwrap_or("null")
                ));
            };
            sizes.push(Some([x, y, 0.0, 0.0]));
        }
        for &k in &asked {
            if !matches!(sizes.get(k), Some(Some(_))) {
                return failed(format!(
                    "drawmat: {}: a preshader reads the size of referenced texture {k}, past the map's {}",
                    m.path,
                    refs.len()
                ));
            }
        }
        (vecs, scal, _) = eval_all(&sizes)?;
        texture_sizes = Some(sizes);
    }
    let mut values: Vec<f64> = Vec::new();
    for v in &vecs {
        values.extend_from_slice(v);
    }
    for k in (0..scal.len()).step_by(4) {
        values.extend((0..4).map(|i| scal.get(k + i).copied().unwrap_or(0.0)));
    }
    let size = ues.uniform_buffer_layout_initializer.constant_buffer_size;
    if (size as usize) < values.len() * 4 {
        return failed(format!("drawmat: {}: material buffer {} > {size}", m.path, values.len() * 4));
    }
    if ues.uniform_texture_parameters.get(4).is_some_and(|t| !t.is_empty())
        || !ues.vt_stacks.is_empty()
        || !ues.uniform_external_texture_parameters.is_empty()
    {
        return failed(format!("drawmat: {}: virtual or external textures", m.path));
    }
    let mut textures = Vec::new();
    for (i, type_) in TEXTURE_TYPES.iter().enumerate() {
        for t in ues.uniform_texture_parameters.get(i).map_or(&[][..], Vec::as_slice) {
            textures.push(LayoutTexture {
                name: t.parameter_info.name.clone(),
                type_,
                texture_index: t.texture_index,
                sampler_source: t.sampler_source,
            });
        }
    }
    Ok(Layout { size, values, scalar_params: scalars, vector_params: vectors, textures, texture_sizes })
}

/// The texture a material texture parameter binds: the instance chain's value of its name, else the shader map's
/// referenced texture at its index.
pub fn parameter_texture(
    cx: &Cx<'_>,
    m: &ResolvedMaterial<'_>,
    layout: &Layout,
    k: usize,
) -> Result<Option<String>, KindError> {
    let Some(t) = layout.textures.get(k) else {
        return failed(format!("drawmat: {}: no texture parameter {k}", m.path));
    };
    if let Some(Some(p)) = m.textures.get(&t.name)
        && !p.is_empty()
    {
        return Ok(Some(p.clone()));
    }
    Ok(usize::try_from(t.texture_index).ok().and_then(|i| referenced_textures(cx, m).into_iter().nth(i)).flatten())
}

// ---------------------------------------------------------------- the collections

const PREFERENCES_UI: &str =
    "/Game/Panda_Main/Blueprints/Structs/Preferences/Struct_Preferences_UI.Struct_Preferences_UI";
const CENTRAL_PALETTE: &str = "/Game/Panda_Main/UI/Prototype/Foundation/Types/CentralPalette.CentralPalette";
const PALETTE_PARAMS: [(&str, &str); 5] = [
    ("Ally", "ColorAlly"),
    ("Enemy", "ColorEnemy"),
    ("Me", "ColorMe"),
    ("Team3", "ColorTeam3"),
    ("Team4", "ColorTeam4"),
];

/// MPC_CentralPalette's vectors as a world has them at run time, not the collection's defaults: PreferencesManager
/// "Apply UI Material Colors" (@0-1246) sets ColorAlly, ColorEnemy, ColorMe, ColorTeam3 and ColorTeam4 to the `Game`
/// colour of the CentralPalette row whose ColorID is the ui-colorID-<name> preference; a match and the Lab call it from
/// PFGCamera ReceiveBeginPlay, the menu worlds from BP_CharacterPreview's. The entry takes Struct_Preferences_UI's
/// defaults: what a world draws with before the player has picked a colour. The game writes the player's picks over it
/// at run time.
fn palette_at_runtime(cx: &Cx<'_>) -> Result<Vec<(&'static str, V4)>, KindError> {
    let prefs = cx.object(PREFERENCES_UI)?.native()?.and_then(|n| match n.get("DefaultProperties") {
        Some(Value::Struct(p)) => Some(p.clone()),
        _ => None,
    });
    let rows = cx.object(CENTRAL_PALETTE)?.native()?.and_then(|n| match n.get("Rows") {
        Some(Value::Map(r)) => Some(r.clone()),
        _ => None,
    });
    let (Some(prefs), Some(rows)) = (prefs, rows) else {
        return failed("drawmat: the central palette: no preferences defaults or palette rows".into());
    };
    let mut out = Vec::new();
    for (pref, param) in PALETTE_PARAMS {
        let id = field(&prefs, &format!("ui-colorID-{pref}")).and_then(|v| string(cx.cache, v));
        let hit: Vec<&Value> = rows
            .iter()
            .filter_map(|(_, r)| match (r, &id) {
                (Value::Struct(s), Some(id))
                    if field(s, "ColorID").and_then(|c| string(cx.cache, c)).as_deref() == Some(id.as_str()) =>
                {
                    Some(r)
                }
                _ => None,
            })
            .collect();
        let (Some(id), [Value::Struct(row)]) = (&id, hit.as_slice()) else {
            return failed(format!(
                "drawmat: the central palette: {} rows with ColorID {} (ui-colorID-{pref})",
                hit.len(),
                id.as_deref().unwrap_or("undefined")
            ));
        };
        let Some(game) = field(row, "Game").and_then(color) else {
            return failed(format!("drawmat: the central palette: row {id} has no Game colour"));
        };
        out.push((param, game));
    }
    Ok(out)
}

/// A material parameter collection: its parameters' names and its buffer's rows (the scalars packed by four, then each
/// vector as a row): its defaults, MPC_CentralPalette's vectors as the game sets them.
pub struct Collection {
    pub path: String,
    pub scalars: Vec<Option<String>>,
    pub vectors: Vec<Option<String>>,
    pub values: Vec<f64>,
}

fn collection(cx: &Cx<'_>, path: &str) -> Result<Collection, KindError> {
    let set = if path.ends_with(".MPC_CentralPalette") { palette_at_runtime(cx)? } else { Vec::new() };
    let c = cx.object(path)?;
    let scalar_items = array(c.prop("ScalarParameters"));
    let s: Vec<f64> = scalar_items
        .iter()
        .map(|x| match x {
            Value::Struct(p) => field(p, "DefaultValue").and_then(num).unwrap_or(0.0),
            _ => 0.0,
        })
        .collect();
    let mut values = Vec::new();
    for k in (0..s.len()).step_by(4) {
        values.extend((0..4).map(|i| s.get(k + i).copied().unwrap_or(0.0)));
    }
    let name = |x: &Value, key: &str| match x {
        Value::Struct(p) => field(p, key).and_then(|n| string(cx.cache, n)),
        _ => None,
    };
    let vector_items = array(c.prop("VectorParameters"));
    for v in vector_items {
        let pname = name(v, "ParameterName");
        let x = pname.as_deref().and_then(|n| set.iter().find(|(k, _)| *k == n)).map(|(_, x)| *x);
        let x = match x {
            Some(x) => x,
            None => match v {
                Value::Struct(p) => match field(p, "DefaultValue").and_then(color) {
                    Some(x) => x,
                    None => return failed(format!("drawmat: {path}: a vector parameter has no default")),
                },
                _ => return failed(format!("drawmat: {path}: a vector parameter is no struct")),
            },
        };
        values.extend_from_slice(&x);
    }
    Ok(Collection {
        path: path.to_owned(),
        scalars: scalar_items.iter().map(|x| name(x, "ParameterName")).collect(),
        vectors: vector_items.iter().map(|x| name(x, "ParameterName")).collect(),
        values,
    })
}

/// The material's parameter collections in ParameterCollections order (their StateIds resolved through the base
/// Material's CachedExpressionData.ParameterCollectionInfos).
pub fn material_collections(
    cx: &Cx<'_>,
    m: &ResolvedMaterial<'_>,
    ues: &UniformExpressionSet,
) -> Result<Vec<Collection>, KindError> {
    let infos = match m.prop("CachedExpressionData") {
        Some(Value::Struct(c)) => array(field(c, "ParameterCollectionInfos")),
        _ => &[],
    };
    let mut out = Vec::new();
    for id in &ues.parameter_collections {
        let info = infos.iter().find_map(|i| match i {
            Value::Struct(s)
                if field(s, "StateId").and_then(|v| string(cx.cache, v)).as_deref() == Some(id.as_str()) =>
            {
                field(s, "ParameterCollection").and_then(|v| string(cx.cache, v))
            }
            _ => None,
        });
        let Some(info) = info else {
            return failed(format!("drawmat: {}: unknown parameter collection {id}", m.path));
        };
        out.push(collection(cx, &info)?);
    }
    Ok(out)
}
