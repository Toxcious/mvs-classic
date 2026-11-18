//! A Wwise sound bank's tree, as CUE4Parse's WwiseReader reads it and the object kind writes it (an AkAudioEventData's
//! or AkAudioBank's `Data`). It follows CUE4Parse 1.2.2's WwiseReader and its
//! WriteJson (WwiseConverter, the hierarchy, action and plugin classes), with CUE4Parse's names and key order.
//!
//! The tree, as [`Value`]s: `{ Header, AKPluginList?, Hierarchies?, EnvSettings?, Platform?, GlobalSettings? }`.
//! `Hierarchies` is an array of `{ Type, Id, Data }` (`Id` a string); an enum is written
//! as its name text or JSON in the `enumJson` / `enumText` forms. Only what bank version 135 (the build's) reads is
//! ported: a bank of another version, a hierarchy type or a plugin no bank of the build holds is an error (CUE4Parse
//! falls back to a generic item or skips the plugin's bytes there; the port names it instead).
//!
//! What an earlier tree carried beside the JSON as hidden fields for the soundbank kind (an AkProp's raw u32, a
//! positioning's bits and 3D byte, an attenuation's curve map, a music playlist item's eRSType) is not in this tree:
//! [`read_bank_hidden`] gives it beside the tree, one [`ItemHidden`] per hierarchy item.

mod enums;
mod items;
mod plugins;

use enums::*;

use crate::archive::{Archive, ReadError, Result};
use crate::value::{Props, Value};

/// The bank version the build's banks are (BKHD's dwBankGeneratorVersion).
const VERSION: u32 = 135;

/// An object from its members in order; each value expression is evaluated (and read) in that order.
macro_rules! obj {
    ($($k:literal => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut p = $crate::value::Props::new();
        $( p.insert($k.to_owned(), $v); )*
        $crate::value::Value::Struct(p)
    }};
}
pub(crate) use obj;

/// What a hierarchy item holds that the tree leaves out (an earlier tree's non-enumerable fields).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemHidden {
    /// The node's property 70 as the bank holds it (the attenuation share set's id), the last of its bundle.
    pub att: Option<u32>,
    /// The node's positioning: its bits and its 3D byte (0 without one).
    pub pos: Option<(u8, u8)>,
    /// An attenuation's curve to use per purpose.
    pub curve_to_use: Option<[i8; 7]>,
    /// The eRSType of a music random / sequence container's playlist items, in the order the items are read (an item,
    /// then its children).
    pub rs_type: Vec<i32>,
}

impl ItemHidden {
    pub fn is_empty(&self) -> bool {
        *self == ItemHidden::default()
    }
}

/// The bank's tree from its bytes (the AkAssetData's bulk). `name`: for messages (`<pak path>:<export name>`). A
/// bank of another Wwise version, a hierarchy type or a chunk the port lacks, or an item that does not read to its
/// length is a parse error.
pub fn read_bank(bytes: &[u8], name: &str) -> Result<Value> {
    read_bank_hidden(bytes, name).map(|(tree, _)| tree)
}

/// [`read_bank`] and, for each item of `Hierarchies` in order, what the tree leaves out of it (empty without a
/// HIRC chunk).
pub fn read_bank_hidden(bytes: &[u8], name: &str) -> Result<(Value, Vec<ItemHidden>)> {
    let mut hidden = Vec::new();
    let mut w = W { ar: Archive::new(bytes, name), hid: ItemHidden::default(), props70: None, positioning: None };
    let mut header: Option<Value> = None;
    let mut plugins: Option<Vec<(String, Value)>> = None;
    let mut hirc: Option<Vec<Value>> = None;
    let mut env: Option<Value> = None;
    let mut platform: Option<String> = None;
    let mut gs: Option<Value> = None;
    while w.ar.pos < w.ar.len() {
        let id = w.u32()?;
        let len = w.i32()?;
        let end = w.ar.pos as i64 + i64::from(len);
        if id == BKHD {
            let version = w.u32()?;
            if version != VERSION {
                return Err(perr(format!("{name}: a Wwise bank of version {version}, not {VERSION}")));
            }
            header = Some(obj! {
                "Version" => uint(version),
                "SoundBankId" => uint(w.u32()?),
                "LanguageId" => uint(w.u32()?),
                "FeedbackInBank" => Value::Bool(false),
                "AltValues" => EAltValuesFlags.text_value(w.u32()?),
                "ProjectId" => uint(w.u32()?),
                "SoundBankType" => EAkBankTypeEnum.text_value(0),
                "BankHash" => Value::Str(String::new()),
            });
        } else if header.is_none() {
            return Err(perr(format!("{name}: chunk {id:x} before the bank header")));
        } else if id == INIT {
            let n = w.i32()?;
            let mut list = Vec::new();
            for _ in 0..n {
                let k = w.u32()?;
                list.push((k.to_string(), Value::Str(w.ar.fstring()?)));
            }
            plugins = Some(list);
        } else if id == HIRC {
            let n = w.u32()?;
            hirc = Some(w.arr(i64::from(n), |w| {
                let ty = w.u8()?;
                let length = w.u32()?;
                let at = w.ar.pos;
                let Some(f) = items::item(ty) else {
                    return Err(perr(format!("{name}: hierarchy type {ty} at {at} is not ported")));
                };
                let id = w.u32()?;
                w.hid = ItemHidden::default();
                let data = f(w)?;
                hidden.push(std::mem::take(&mut w.hid));
                let type_name = EAKBKHircType.text(u32::from(ty));
                if w.ar.pos as u64 != at as u64 + u64::from(length) {
                    return Err(perr(format!(
                        "{name}: {type_name} {id} read {} of its {length} bytes",
                        w.ar.pos as i64 - at as i64
                    )));
                }
                Ok(obj! { "Type" => Value::Str(type_name), "Id" => Value::Str(id.to_string()), "Data" => data })
            })?);
        } else if id == STMG {
            gs = Some(global_settings(&mut w)?);
        } else if id == ENVS {
            // CAkEnvironmentsMgr: six conversion tables, each after a byte; the dump writes neither
            for _ in 0..6 {
                w.u8()?;
                conversion_table(&mut w)?;
            }
            env = Some(obj! { "AttenuationId" => Value::Int(0), "ConversionTableEntries" => Value::Null });
        } else if id == PLAT {
            let n = w.i32()?;
            platform = Some(w.latin1(n)?);
        } else if id != STID {
            return Err(perr(format!("{name}: chunk {id:x} is not ported")));
        }
        if w.ar.pos as i64 > end {
            return Err(perr(format!("{name}: chunk {id:x} read past its {len} bytes")));
        }
        // end >= pos >= 0 here; past the bank's end it ends the loop
        w.ar.pos = end as usize;
    }
    let Some(header) = header else {
        return Err(perr(format!("{name}: no bank header")));
    };
    let mut out = Props::new();
    out.insert("Header".to_owned(), header);
    if let Some(p) = plugins.filter(|p| !p.is_empty()) {
        out.insert("AKPluginList".to_owned(), Value::Struct(js_object(p)));
    }
    if let Some(h) = hirc.filter(|h| !h.is_empty()) {
        out.insert("Hierarchies".to_owned(), Value::Array(h));
    }
    if let Some(e) = env {
        out.insert("EnvSettings".to_owned(), e);
    }
    if let Some(p) = platform.filter(|p| !p.is_empty()) {
        out.insert("Platform".to_owned(), Value::Str(p));
    }
    if let Some(g) = gs {
        out.insert("GlobalSettings".to_owned(), g);
    }
    Ok((Value::Struct(out), hidden))
}

const fn chunk(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}
const BKHD: u32 = chunk(b"BKHD");
const INIT: u32 = chunk(b"INIT");
const HIRC: u32 = chunk(b"HIRC");
const STMG: u32 = chunk(b"STMG");
const ENVS: u32 = chunk(b"ENVS");
const PLAT: u32 = chunk(b"PLAT");
const STID: u32 = chunk(b"STID");

fn perr(msg: String) -> ReadError {
    ReadError::parse(msg)
}

fn uint(v: impl Into<u64>) -> Value {
    Value::UInt(v.into())
}
fn int(v: impl Into<i64>) -> Value {
    Value::Int(v.into())
}
fn arr(v: Vec<Value>) -> Value {
    Value::Array(v)
}
fn empty() -> Value {
    Value::Array(Vec::new())
}

/// A JS object's own keys in the order JSON.stringify writes them: the array-index keys (canonical decimals below
/// 2^32 - 1) first in ascending order, then the rest in insertion order; a repeated key keeps its first place and
/// takes its last value. AKPluginList and a State's Settings are plain objects of this kind.
fn js_object(entries: Vec<(String, Value)>) -> Props {
    let index = |k: &str| -> Option<u32> {
        let ok = !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) && (k == "0" || !k.starts_with('0'));
        ok.then(|| k.parse::<u32>().ok()).flatten().filter(|&n| n != u32::MAX)
    };
    let mut p = Props::new();
    for (k, v) in entries {
        p.insert(k, v);
    }
    let (mut ix, rest): (Vec<_>, Vec<_>) = p.into_iter().partition(|(k, _)| index(k).is_some());
    ix.sort_by_key(|(k, _)| index(k));
    ix.into_iter().chain(rest).collect()
}

/// The bank reader: the byte reads and WwiseReader's own (Read7BitEncodedIntBE, arrays).
struct W<'a> {
    ar: Archive<'a>,
    /// The current item's hidden fields, as read so far.
    hid: ItemHidden,
    /// The last property bundle's property 70 and the last positioning's bits: a node's base reads them after its own.
    props70: Option<u32>,
    positioning: Option<(u8, u8)>,
}

impl W<'_> {
    fn u8(&mut self) -> Result<u8> {
        self.ar.u8()
    }
    fn i8(&mut self) -> Result<i8> {
        self.ar.i8()
    }
    fn u16(&mut self) -> Result<u16> {
        self.ar.u16()
    }
    fn i16(&mut self) -> Result<i16> {
        self.ar.i16()
    }
    fn u32(&mut self) -> Result<u32> {
        self.ar.u32()
    }
    fn i32(&mut self) -> Result<i32> {
        self.ar.i32()
    }
    fn f32(&mut self) -> Result<f32> {
        self.ar.f32()
    }
    /// A float as the tree writes it.
    fn f(&mut self) -> Result<Value> {
        Ok(Value::Float(self.ar.f32()?))
    }
    /// A double as the tree writes it.
    fn d(&mut self) -> Result<Value> {
        Ok(Value::Double(self.ar.f64()?))
    }
    /// A one-byte bool.
    fn b(&mut self) -> Result<bool> {
        Ok(self.ar.u8()? != 0)
    }
    fn bv(&mut self) -> Result<Value> {
        Ok(Value::Bool(self.b()?))
    }
    fn u8v(&mut self) -> Result<Value> {
        Ok(uint(self.u8()?))
    }
    fn u16v(&mut self) -> Result<Value> {
        Ok(uint(self.u16()?))
    }
    fn u32v(&mut self) -> Result<Value> {
        Ok(uint(self.u32()?))
    }
    fn i32v(&mut self) -> Result<Value> {
        Ok(int(self.i32()?))
    }
    /// Read7BitEncodedIntBE: big-endian groups of 7 bits, at most 10 bytes, wrapping to 32 bits.
    fn v7(&mut self) -> Result<i32> {
        let mut b = self.u8()?;
        let mut n = i32::from(b & 0x7f);
        let mut k = 0;
        while b & 0x80 != 0 {
            k += 1;
            if k >= 10 {
                return Err(perr(format!(
                    "{}: variable-length integer past 10 bytes at {}",
                    self.ar.name, self.ar.pos
                )));
            }
            b = self.u8()?;
            n = n.wrapping_shl(7) | i32::from(b & 0x7f);
        }
        Ok(n)
    }
    /// `n` items read by `f`; a negative count is an error.
    fn arr<T>(&mut self, n: i64, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        if n < 0 {
            return Err(perr(format!("{}: array of {n} at {}", self.ar.name, self.ar.pos)));
        }
        let mut out = Vec::with_capacity(usize::try_from(n).unwrap_or(0).min(self.ar.remaining()));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
    /// `n` items read by `f`, as an array value.
    fn arrv(&mut self, n: i64, f: impl FnMut(&mut Self) -> Result<Value>) -> Result<Value> {
        Ok(Value::Array(self.arr(n, f)?))
    }
    fn u32s(&mut self, n: u32) -> Result<Value> {
        self.arrv(i64::from(n), |w| w.u32v())
    }
    /// The next u32 without moving.
    fn peek_u32(&self) -> Result<u32> {
        let mut a = self.ar.clone();
        a.u32()
    }
    /// `n` bytes as Latin-1 text, the trailing NULs cut (a negative `n` fails as a read of it would).
    fn latin1(&mut self, n: i32) -> Result<String> {
        let Ok(n) = usize::try_from(n) else {
            return Err(perr(format!("{}: read of {n} at {} past end {}", self.ar.name, self.ar.pos, self.ar.len())));
        };
        let b = self.ar.bytes(n)?;
        let s: String = b.iter().map(|&c| char::from(c)).collect();
        Ok(s.trim_end_matches('\0').to_owned())
    }
}

// ---- shared structs ---------------------------------------------------------------------------------------------

/// AkUnionValue's JSON (AkPropValueConverter): a float read as u32 when the bits are small, else the float.
fn union_value(u: u32) -> Value {
    if u > 0x1000_0000 { Value::Float(f32::from_bits(u)) } else { Value::Float(u as f32) }
}

fn props(w: &mut W) -> Result<Value> {
    let n = w.u8()?;
    let ids = w.arr(i64::from(n), |w| w.u8())?;
    let mut out = Vec::with_capacity(ids.len());
    w.props70 = None;
    for id in ids {
        let u = w.u32()?;
        if id == 70 {
            w.props70 = Some(u);
        }
        out.push(obj! { "Id" => uint(id), "Value" => union_value(u) });
    }
    Ok(arr(out))
}

fn prop_ranges(w: &mut W) -> Result<Value> {
    let n = w.u8()?;
    let ids = w.arr(i64::from(n), |w| w.u8())?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(obj! { "Id" => uint(id), "Min" => union_value(w.u32()?), "Max" => union_value(w.u32()?) });
    }
    Ok(arr(out))
}

fn graph_point(w: &mut W) -> Result<Value> {
    Ok(obj! { "From" => w.f()?, "To" => w.f()?, "Interpolation" => EAkCurveInterpolation.json(w.u32()?) })
}

fn conversion_table(w: &mut W) -> Result<Value> {
    let scaling = EAkCurveScaling.json(u32::from(w.u8()?));
    let size = w.u16()?;
    Ok(obj! {
        "Scaling" => scaling,
        "Size" => uint(size),
        "GraphPoints" => w.arrv(i64::from(size), graph_point)?,
    })
}

fn rtpc(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "RtpcId" => w.u32v()?,
        "RtpcType" => EAkGameSyncType.json(u32::from(w.u8()?)),
        "RtpcAccum" => EAkRtpcAccum.json(u32::from(w.u8()?)),
        "ParamId" => uint(w.v7()?.cast_unsigned()),
        "RtpcCurveId" => w.u32v()?,
        "ConversionTable" => conversion_table(w)?,
    })
}

fn rtpcs(w: &mut W) -> Result<Value> {
    let n = w.u16()?;
    w.arrv(i64::from(n), rtpc)
}

/// AkStateAwareChunk: its properties are read and dropped, its groups kept.
fn state_groups(w: &mut W) -> Result<Value> {
    let n = w.v7()?;
    w.arr(i64::from(n), |w| {
        w.v7()?;
        w.u8()?;
        w.u8()
    })?;
    let n = w.v7()?;
    w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "Id" => w.u32v()?,
            "GroupType" => EAkSyncType.json(u32::from(w.u8()?)),
            "States" => {
                let n = w.v7()?;
                w.arrv(i64::from(n), |w| {
                    Ok(obj! { "Id" => w.u32v()?, "StateInstanceId" => w.u32v()?, "Properties" => empty() })
                })?
            },
        })
    })
}

fn fx_params(w: &mut W) -> Result<Value> {
    let n = w.u8()?;
    if n == 0 {
        return Ok(obj! { "BypassAll" => Value::Bool(false), "Effects" => empty() });
    }
    Ok(obj! {
        "BypassAll" => w.bv()?,
        "Effects" => w.arrv(i64::from(n), |w| {
            Ok(obj! {
                "FXIndex" => w.u8v()?,
                "FXId" => w.u32v()?,
                "BitVector" => Value::Int(0),
                "IsShareSet" => w.bv()?,
                "IsRendered" => w.bv()?,
            })
        })?,
    })
}

fn positioning(w: &mut W) -> Result<Value> {
    let bits = w.u8()?;
    let mut o = Props::new();
    o.insert("BitsPositioning".to_owned(), EBitsPositioningFlags.json(u32::from(bits)));
    o.insert("PathMode".to_owned(), EAkPathMode.json(0));
    o.insert("IsLooping".to_owned(), Value::Bool(false));
    o.insert("TransitionTime".to_owned(), Value::Int(0));
    o.insert("Vertices".to_owned(), empty());
    o.insert("PlaylistItems".to_owned(), empty());
    o.insert("PlaylistRanges".to_owned(), empty());
    let mut bits_3d = 0;
    if bits & 1 != 0 && bits & 2 != 0 {
        bits_3d = w.u8()?;
        if (bits >> 5) & 3 != 0 {
            o.insert("PathMode".to_owned(), EAkPathMode.json(u32::from(w.u8()?)));
            o.insert("TransitionTime".to_owned(), w.i32v()?);
            let n = w.u32()?;
            let v = w.arrv(i64::from(n), |w| {
                Ok(obj! { "X" => w.f()?, "Y" => w.f()?, "Z" => w.f()?, "Duration" => w.i32v()? })
            })?;
            o.insert("Vertices".to_owned(), v);
            let n = i64::from(w.u32()?);
            let items = w.arrv(n, |w| Ok(obj! { "VerticesOffset" => w.u32v()?, "NumVertices" => w.u32v()? }))?;
            o.insert("PlaylistItems".to_owned(), items);
            let ranges = w.arrv(n, |w| Ok(obj! { "XRange" => w.f()?, "YRange" => w.f()?, "ZRange" => w.f()? }))?;
            o.insert("PlaylistRanges".to_owned(), ranges);
        }
    }
    w.positioning = Some((bits, bits_3d));
    Ok(Value::Struct(o))
}

fn aux_params(w: &mut W) -> Result<Value> {
    let flags = w.u8()?;
    let aux_ids = if flags & 8 != 0 { w.u32s(4)? } else { empty() };
    Ok(obj! {
        "OverrideGameAuxSends" => Value::Bool(false),
        "UseGameAuxSends" => Value::Bool(false),
        "OverrideUserAuxSends" => Value::Bool(false),
        "AuxParams" => EAuxParamsFlags.json(u32::from(flags)),
        "AuxIds" => aux_ids,
        "ReflectionsAuxBus" => w.u32v()?,
    })
}

fn adv_settings(w: &mut W) -> Result<Value> {
    Ok(obj! {
        "AdvSettingsFlags" => EAkAdvSettingsFlags.json(u32::from(w.u8()?)),
        "VirtualQueueBehavior" => EAkVirtualQueueBehavior.json(u32::from(w.u8()?)),
        "MaxNumInstance" => w.u16v()?,
        "IsGlobalLimit" => Value::Bool(false),
        "BelowThresholdBehavior" => w.u8v()?,
        "HdrEnvelopeFlags" => EAkHdrEnvelopeFlags.json(u32::from(w.u8()?)),
    })
}

/// BaseHierarchy (NodeBaseParams): its fields as its WriteJson writes them.
fn base_hierarchy(w: &mut W) -> Result<Value> {
    let override_fx = w.bv()?;
    let fx = fx_params(w)?;
    let override_attachment = w.bv()?;
    let override_bus = w.u32()?;
    let direct_parent = w.u32()?;
    let midi = w.u8()?;
    let prop_bundle = obj! { "Props" => props(w)?, "PropRanges" => prop_ranges(w)? };
    w.hid.att = w.props70;
    let positioning = positioning(w)?;
    w.hid.pos = w.positioning;
    let aux = aux_params(w)?;
    let adv = adv_settings(w)?;
    let state_groups = state_groups(w)?;
    let rtpc_list = rtpcs(w)?;
    let mut o = Props::new();
    o.insert("OverrideFx".to_owned(), override_fx);
    o.insert("FxParams".to_owned(), fx);
    o.insert("OverrideParentMetadataFlag".to_owned(), Value::Bool(false));
    o.insert("FxChunks".to_owned(), empty());
    o.insert("OverrideAttachmentParams".to_owned(), override_attachment);
    if override_bus != 0 {
        o.insert("OverrideBusId".to_owned(), uint(override_bus));
    }
    if direct_parent != 0 {
        o.insert("DirectParentId".to_owned(), uint(direct_parent));
    }
    o.insert("Priority".to_owned(), Value::Bool(false));
    o.insert("PriorityOverrideParent".to_owned(), Value::Bool(midi == 1));
    o.insert("PriorityApplyDistFactor".to_owned(), Value::Bool(midi == 2));
    o.insert("DistOffset".to_owned(), Value::Int(0));
    o.insert("MidiBehaviorFlags".to_owned(), EMidiBehaviorFlags.text_value(u32::from(midi)));
    o.insert("PropBundle".to_owned(), prop_bundle);
    o.insert("PositioningParams".to_owned(), positioning);
    o.insert("AuxParams".to_owned(), aux);
    o.insert("AdvSettingsParams".to_owned(), adv);
    // BaseHierarchy's own fields of these names stay at their defaults in v135 (AdvSettingsParams holds the read ones)
    o.insert("VirtualQueueBehavior".to_owned(), EAkVirtualQueueBehavior.text_value(0));
    o.insert("MaxNumInstance".to_owned(), Value::Int(0));
    o.insert("BelowThresholdBehavior".to_owned(), EAkBelowThresholdBehavior.text_value(0));
    o.insert("HdrEnvelopeFlags".to_owned(), EHdrEnvelopeFlags.text_value(0));
    o.insert("StateGroups".to_owned(), state_groups);
    o.insert("RtpcList".to_owned(), rtpc_list);
    Ok(Value::Struct(o))
}

/// STMG: the init bank's global settings.
fn global_settings(w: &mut W) -> Result<Value> {
    let volume_threshold = w.f()?;
    let max_voices = w.u16v()?;
    let max_dangerous = w.u16v()?;
    let n = w.i32()?;
    let state_groups = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "StateGroupId" => w.u32v()?,
            "DefaultTransitionTime" => w.u32v()?,
            "StateTransitions" => {
                let n = w.u32()?;
                w.arrv(i64::from(n), |w| {
                    Ok(obj! { "StateFrom" => w.u32v()?, "StateTo" => w.u32v()?, "TransitionTime" => w.u32v()? })
                })?
            },
        })
    })?;
    let n = w.i32()?;
    let switch_groups = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "SwitchGroupId" => w.u32v()?,
            "RtpcId" => w.u32v()?,
            "RtpcType" => EAkGameSyncType.json(u32::from(w.u8()?)),
            "GraphPoints" => {
                let n = w.u32()?;
                w.arrv(i64::from(n), |w| {
                    Ok(obj! {
                        "From" => w.f()?,
                        "To" => w.u32v()?,
                        "Interp" => EAkCurveInterpolation.json(w.u32()?),
                    })
                })?
            },
        })
    })?;
    let n = w.i32()?;
    let ramping = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "RtpcId" => w.u32v()?,
            "Value" => w.f()?,
            "RampType" => EAkTransitionRampingType.json(w.u32()?),
            "RampUp" => w.f()?,
            "RampDown" => w.f()?,
            "BindToBuiltInParam" => EAkBuiltInParam.json(u32::from(w.u8()?)),
        })
    })?;
    let n = w.i32()?;
    let acoustics = w.arrv(i64::from(n), |w| {
        Ok(obj! {
            "Id" => w.u32v()?,
            "AbsorptionOffset" => w.f()?,
            "AbsorptionLow" => w.f()?,
            "AbsorptionMidLow" => w.f()?,
            "AbsorptionMidHigh" => w.f()?,
            "AbsorptionHigh" => w.f()?,
            "Scattering" => w.f()?,
        })
    })?;
    Ok(obj! {
        "FilterBehavior" => EAkFilterBehavior.text_value(0),
        "VolumeThreshold" => volume_threshold,
        "MaxNumVoicesLimitInternal" => max_voices,
        "MaxNumDangerousVirtVoicesLimitInternal" => max_dangerous,
        "HSFEmphasis" => Value::Int(0),
        "StateGroups" => state_groups,
        "SwitchGroups" => switch_groups,
        "RTPCRampingParams" => ramping,
        "VirtualAcoustics" => acoustics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_write_as_dotnet_and_newtonsoft() {
        // a flags enum: greedy from the top, joined ascending; bits left over give the number
        assert_eq!(EBankSourceFlags.text(0), "None");
        assert_eq!(EBankSourceFlags.text(129), "IsLanguageSpecific, HasSource");
        assert_eq!(EBankSourceFlags.text(16), "16");
        assert!(matches!(EAuxParamsFlags.json(64), Value::UInt(64)));
        // a plain enum with a repeated value: the binary search's pick
        assert_eq!(EAkCurveInterpolation.text(8), "Exp3");
        assert_eq!(EAkCurveInterpolation.text(42), "42");
        assert!(matches!(EAkPathMode.json(1), Value::Str(ref s) if s == "StepRandom"));
    }

    #[test]
    fn plugin_keys_order_as_a_js_object() {
        let p = js_object(vec![
            ("7".to_owned(), Value::Null),
            ("b".to_owned(), Value::Null),
            ("3".to_owned(), Value::Null),
            ("4294967295".to_owned(), Value::Null),
            ("03".to_owned(), Value::Null),
            ("7".to_owned(), Value::Bool(true)),
        ]);
        let keys: Vec<&str> = p.keys().map(String::as_str).collect();
        assert_eq!(keys, ["3", "7", "b", "4294967295", "03"]);
        assert!(matches!(p["7"], Value::Bool(true)));
    }

    #[test]
    fn a_bank_of_another_version_is_an_error() {
        let mut b = b"BKHD".to_vec();
        b.extend_from_slice(&20i32.to_le_bytes());
        b.extend_from_slice(&134u32.to_le_bytes());
        b.extend_from_slice(&[0; 16]);
        let e = read_bank(&b, "x").unwrap_err();
        assert_eq!(e.to_string(), "x: a Wwise bank of version 134, not 135");
        let e = read_bank(b"HIRC\0\0\0\0", "x").unwrap_err();
        assert_eq!(e.to_string(), "x: chunk 43524948 before the bank header");
    }
}
