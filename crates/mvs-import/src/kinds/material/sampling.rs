//! A texture's sampling (filter, address modes, LOD group) from its object and the texture LOD groups of the build's
//! device profile files.
//!
//! The texture's values come from the object (absent = the class default), the group's from the config files
//! (BaseDeviceProfiles with the project's DefaultDeviceProfiles laid over it).
//!
//! - The profile: WindowsNoEditor, its BaseProfileName chain (Windows) filling the groups it does not list.
//! - A profile's groups: its TextureLODGroups lines as the config combines them ([`config_values`]), one group per
//!   Group name, a later line replacing an earlier one; a group no profile of the chain lists takes FTextureLODGroup's
//!   defaults (aniso / point).
//! - The group's filter from its names (UTextureLODSettings::SetupLODGroup): aniso + point = anisotropicPoint, aniso +
//!   linear = anisotropicLinear, linear + point = bilinear, linear + linear = trilinear, point = point.
//! - The texture's filter (GetSamplerFilter): TF_Nearest point, TF_Bilinear bilinear, TF_Trilinear trilinear,
//!   TF_Default (or absent) its group's.
//! - Address: a Texture2D's AddressX / AddressY (TA_Wrap when absent); a TextureCube's sampler clamps on every axis.
//! - The class default group: World, but LightMapTexture2D Lightmap and ShadowMapTexture2D Shadowmap.
// Unverified: the device profile (WindowsNoEditor, UDeviceProfileManager::GetPlatformDeviceProfileName), the group
// filter rule, the texture filter rule, the cube clamp and the class default groups are the 4.26 source's, not read
// from the exe.

use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

use super::super::config::{IniFile, IniSection, ini};
use crate::kind::{Cx, KindError};
use crate::objview::{ObjView, string};

pub const BASE_DEVICE_PROFILES_INI: &str = "/Engine/Config/BaseDeviceProfiles";
pub const DEFAULT_DEVICE_PROFILES_INI: &str = "/MultiVersus/Config/DefaultDeviceProfiles";
/// The device profile the game samples by.
pub const TEXTURE_DEVICE_PROFILE: &str = "WindowsNoEditor";

/// The filter an entry names (`SamplerFilter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplerFilter {
    Point,
    Bilinear,
    Trilinear,
    AnisotropicPoint,
    AnisotropicLinear,
}

impl SamplerFilter {
    pub fn name(self) -> &'static str {
        match self {
            SamplerFilter::Point => "point",
            SamplerFilter::Bilinear => "bilinear",
            SamplerFilter::Trilinear => "trilinear",
            SamplerFilter::AnisotropicPoint => "anisotropicPoint",
            SamplerFilter::AnisotropicLinear => "anisotropicLinear",
        }
    }
}

/// The address mode an entry names (`SamplerAddress`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplerAddress {
    Wrap,
    Clamp,
    Mirror,
}

impl SamplerAddress {
    pub fn name(self) -> &'static str {
        match self {
            SamplerAddress::Wrap => "wrap",
            SamplerAddress::Clamp => "clamp",
            SamplerAddress::Mirror => "mirror",
        }
    }
}

/// One texture LOD group of a device profile.
#[derive(Clone, Debug)]
pub struct TextureLodGroup {
    /// `TEXTUREGROUP_World` ...
    pub group: String,
    pub lod_bias: f64,
    pub filter: SamplerFilter,
}

/// The groups of a device profile by their lower-cased `TEXTUREGROUP_` name.
pub type TextureLodGroups = IndexMap<String, TextureLodGroup>;

/// How the build samples a texture.
#[derive(Clone, Debug)]
pub struct TextureSampling {
    pub filter: SamplerFilter,
    pub address_x: SamplerAddress,
    pub address_y: SamplerAddress,
    /// The texture's group (its LODGroup, or the class default).
    pub group: TextureLodGroup,
}

/// FTextureLODGroup's defaults (4.26 source): what a group no profile lists gets.
const DEFAULT_LOD_BIAS: f64 = 0.0;
const DEFAULT_NUM_STREAMED_MIPS: f64 = -1.0;
const DEFAULT_MIN_LOD_SIZE: f64 = 1.0;
const DEFAULT_MAX_LOD_SIZE: f64 = 4096.0;
const DEFAULT_MIN_MAG_FILTER: &str = "aniso";
const DEFAULT_MIP_FILTER: &str = "point";

fn failed<T>(why: String) -> Result<T, KindError> {
    Err(KindError::Failed(format!("texture sampling: {why}")))
}

fn group_filter(group: &str, min_mag: &str, mip: &str) -> Result<SamplerFilter, KindError> {
    let (a, b) = (min_mag.to_lowercase(), mip.to_lowercase());
    let mipped = b == "point" || b == "linear";
    if a == "aniso" && mipped {
        return Ok(if b == "point" { SamplerFilter::AnisotropicPoint } else { SamplerFilter::AnisotropicLinear });
    }
    if a == "linear" && mipped {
        return Ok(if b == "point" { SamplerFilter::Bilinear } else { SamplerFilter::Trilinear });
    }
    if a == "point" {
        return Ok(SamplerFilter::Point);
    }
    failed(format!("{group}: no filter for MinMagFilter={min_mag}, MipFilter={mip}"))
}

/// A JavaScript `Number(text)` that is finite, else `None`.
fn js_number(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// One `(Group=...,MinMagFilter="aniso",...)` line's fields.
fn parse_group(line: &str) -> Result<TextureLodGroup, KindError> {
    let t = line.trim();
    if !t.starts_with('(') || !t.ends_with(')') {
        return failed(format!("not a TextureLODGroups struct: {line}"));
    }
    let mut f: IndexMap<String, String> = IndexMap::new();
    for part in t[1..t.len() - 1].split(',') {
        let Some(eq) = part.find('=') else { continue };
        let v = part[eq + 1..].trim();
        let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
        f.insert(part[..eq].trim().to_lowercase(), v.to_owned());
    }
    let Some(group) = f.get("group").filter(|g| !g.is_empty()) else {
        return failed(format!("a TextureLODGroups line without a Group: {line}"));
    };
    let num = |k: &str, d: f64| -> Result<f64, KindError> {
        match f.get(&k.to_lowercase()) {
            None => Ok(d),
            Some(v) => js_number(v).map_or_else(|| failed(format!("{group}: {k}={v} is not a number")), Ok),
        }
    };
    let lod_bias = num("LODBias", DEFAULT_LOD_BIAS)?;
    // (read for their checks: a value that is no number fails the profile)
    num("NumStreamedMips", DEFAULT_NUM_STREAMED_MIPS)?;
    num("MinLODSize", DEFAULT_MIN_LOD_SIZE)?;
    num("MaxLODSize", DEFAULT_MAX_LOD_SIZE)?;
    let min_mag = f.get("minmagfilter").map_or(DEFAULT_MIN_MAG_FILTER, String::as_str);
    let mip = f.get("mipfilter").map_or(DEFAULT_MIP_FILTER, String::as_str);
    Ok(TextureLodGroup { group: group.clone(), lod_bias, filter: group_filter(group, min_mag, mip)? })
}

/// The section named `name` (without regard to case, as UE finds a section).
fn config_section<'f>(file: &'f IniFile, name: &str) -> Option<&'f IniSection> {
    let k = name.to_lowercase();
    file.sections.iter().find(|s| s.name.to_lowercase() == k)
}

/// The values of `key` in `section` when the file is combined into an empty config, as FConfigFile::CombineFromBuffer
/// merges each line: keys compared without regard to case, values too; '' replaces the key's first value or adds one,
/// '+' adds unless an equal value is there, '.' adds, '-' removes the first equal value, '!' removes them all. In file
/// order; none for none. A file laid over a base starts from the base's values: `base`.
fn config_values(file: &IniFile, section: &str, key: &str, base: Vec<String>) -> Vec<String> {
    let mut out = base;
    let Some(s) = config_section(file, section) else { return out };
    let k = key.to_lowercase();
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    for e in &s.entries {
        let (op, name, value) = (e.0.as_str(), e.1.as_str(), e.2.as_str());
        if name.to_lowercase() != k {
            continue;
        }
        match op {
            "" => {
                if out.is_empty() {
                    out.push(value.to_owned());
                } else {
                    out[0] = value.to_owned();
                }
            }
            "+" => {
                if !out.iter().any(|v| same(v, value)) {
                    out.push(value.to_owned());
                }
            }
            "." => out.push(value.to_owned()),
            "-" => {
                if let Some(at) = out.iter().position(|v| same(v, value)) {
                    out.remove(at);
                }
            }
            _ => out.clear(),
        }
    }
    out
}

/// The groups of `profile` from the device profile files (the project's laid over the engine's).
pub fn texture_lod_groups(base: &IniFile, project: &IniFile, profile: &str) -> Result<TextureLodGroups, KindError> {
    let mut out = TextureLodGroups::new();
    let mut seen = IndexSet::new();
    let mut name = Some(profile.to_owned());
    while let Some(n) = name {
        if !seen.insert(n.to_lowercase()) {
            return failed(format!("device profile {n} is its own base"));
        }
        let section = format!("{n} DeviceProfile");
        let lines = config_values(
            project,
            &section,
            "TextureLODGroups",
            config_values(base, &section, "TextureLODGroups", Vec::new()),
        );
        let mut own: IndexMap<String, TextureLodGroup> = IndexMap::new();
        for l in &lines {
            let g = parse_group(l)?;
            own.insert(g.group.to_lowercase(), g);
        }
        // a group the child lists is the child's; the base fills the rest
        for (k, g) in own {
            out.entry(k).or_insert(g);
        }
        if config_section(base, &section).is_none() && config_section(project, &section).is_none() {
            return failed(format!("no device profile {n} in the config"));
        }
        let base_name = config_values(
            project,
            &section,
            "BaseProfileName",
            config_values(base, &section, "BaseProfileName", Vec::new()),
        );
        name = base_name.into_iter().next().filter(|s| !s.is_empty());
    }
    Ok(out)
}

/// The run's groups of the game's device profile (a failed read is the run's).
struct GroupsSlot(Result<Arc<TextureLodGroups>, String>);

/// The groups of the game's device profile from the build's two config files, read once per run.
pub fn load_texture_lod_groups(cx: &Cx<'_>) -> Result<Arc<TextureLodGroups>, KindError> {
    let slot = cx.shared.get_or_init(|| {
        GroupsSlot(
            (|| {
                let base = ini(cx, BASE_DEVICE_PROFILES_INI)?;
                let project = ini(cx, DEFAULT_DEVICE_PROFILES_INI)?;
                texture_lod_groups(&base, &project, TEXTURE_DEVICE_PROFILE)
            })()
            .map(Arc::new)
            .map_err(|e| e.to_string()),
        )
    });
    slot.0.clone().map_err(KindError::Failed)
}

/// A group by name (any case): the profile's, or FTextureLODGroup's defaults for one it does not list.
fn texture_lod_group(groups: &TextureLodGroups, name: &str) -> Result<TextureLodGroup, KindError> {
    if let Some(g) = groups.get(&name.to_lowercase()) {
        return Ok(g.clone());
    }
    Ok(TextureLodGroup {
        group: name.to_owned(),
        lod_bias: DEFAULT_LOD_BIAS,
        filter: group_filter(name, DEFAULT_MIN_MAG_FILTER, DEFAULT_MIP_FILTER)?,
    })
}

/// The class default LODGroup.
fn default_group(class: &str) -> &'static str {
    match class {
        "/Script/Engine.LightMapTexture2D" => "TEXTUREGROUP_Lightmap",
        "/Script/Engine.ShadowMapTexture2D" => "TEXTUREGROUP_Shadowmap",
        _ => "TEXTUREGROUP_World",
    }
}

/// An enum property's value without its qualifier (`TextureFilter::TF_Trilinear` -> `TF_Trilinear`); `None` when
/// absent or null.
fn enum_value(obj: &ObjView<'_>, key: &str) -> Result<Option<String>, KindError> {
    use mvs_uasset::value::Value;
    let Some(v) = obj.prop(key) else { return Ok(None) };
    if matches!(v, Value::Null | Value::Ref(None)) {
        return Ok(None);
    }
    let Some(s) = string(obj.cache(), v) else {
        return failed(format!("{}: {key} is not an enum value ({v:?})", obj.path()));
    };
    Ok(Some(s[s.rfind(':').map_or(0, |i| i + 1)..].to_owned()))
}

/// How the build samples the texture `obj` (a Texture2D, TextureCube, LightMapTexture2D or ShadowMapTexture2D object)
/// on the game's device profile.
pub fn texture_sampling(obj: &ObjView<'_>, groups: &TextureLodGroups) -> Result<TextureSampling, KindError> {
    let group_name = enum_value(obj, "LODGroup")?;
    let group = texture_lod_group(groups, group_name.as_deref().unwrap_or_else(|| default_group(obj.class())))?;
    let tf = enum_value(obj, "Filter")?.unwrap_or_else(|| "TF_Default".to_owned());
    let own = match tf.as_str() {
        "TF_Default" => None,
        "TF_Nearest" => Some(SamplerFilter::Point),
        "TF_Bilinear" => Some(SamplerFilter::Bilinear),
        "TF_Trilinear" => Some(SamplerFilter::Trilinear),
        _ => return failed(format!("{}: no filter {tf}", obj.path())),
    };
    let filter = own.unwrap_or(group.filter);
    let address = |key: &str| -> Result<SamplerAddress, KindError> {
        if obj.class() == "/Script/Engine.TextureCube" {
            return Ok(SamplerAddress::Clamp);
        }
        let v = enum_value(obj, key)?.unwrap_or_else(|| "TA_Wrap".to_owned());
        match v.as_str() {
            "TA_Wrap" => Ok(SamplerAddress::Wrap),
            "TA_Clamp" => Ok(SamplerAddress::Clamp),
            "TA_Mirror" => Ok(SamplerAddress::Mirror),
            _ => failed(format!("{}: no address mode {v}", obj.path())),
        }
    };
    Ok(TextureSampling { filter, address_x: address("AddressX")?, address_y: address("AddressY")?, group })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::config::{IniEntry, parse_ini};

    #[test]
    fn group_lines_and_filters() {
        let g =
            parse_group("(Group=TEXTUREGROUP_World,MinMagFilter=aniso,MipFilter=point,NumStreamedMips=-1)").unwrap();
        assert_eq!(
            (g.group.as_str(), g.filter, g.lod_bias),
            ("TEXTUREGROUP_World", SamplerFilter::AnisotropicPoint, 0.0)
        );
        let g = parse_group("(Group=TEXTUREGROUP_UI,MinMagFilter=\"linear\",MipFilter=linear,LODBias=1)").unwrap();
        assert_eq!((g.filter, g.lod_bias), (SamplerFilter::Trilinear, 1.0));
        assert!(parse_group("(Group=X,MinMagFilter=weird)").is_err());
        assert!(parse_group("Group=X").is_err());
        assert!(parse_group("(Group=X,LODBias=a)").is_err());
    }

    #[test]
    fn chain_and_merge() {
        let base = parse_ini(
            "[Windows DeviceProfile]\n+TextureLODGroups=(Group=TEXTUREGROUP_World,MinMagFilter=aniso,MipFilter=point)\n\
             +TextureLODGroups=(Group=TEXTUREGROUP_UI,MinMagFilter=linear,MipFilter=linear)\n\
             [WindowsNoEditor DeviceProfile]\nBaseProfileName=Windows\n",
        );
        let project = parse_ini(
            "[WindowsNoEditor DeviceProfile]\n+TextureLODGroups=(Group=TEXTUREGROUP_World,MinMagFilter=linear,MipFilter=point)\n",
        );
        let groups = texture_lod_groups(&base, &project, TEXTURE_DEVICE_PROFILE).unwrap();
        assert_eq!(groups["texturegroup_world"].filter, SamplerFilter::Bilinear);
        assert_eq!(groups["texturegroup_ui"].filter, SamplerFilter::Trilinear);
        assert!(texture_lod_groups(&base, &project, "Nope").is_err());
        let defaulted = texture_lod_group(&groups, "TEXTUREGROUP_Other").unwrap();
        assert_eq!(defaulted.filter, SamplerFilter::AnisotropicPoint);
    }

    #[test]
    fn config_values_ops() {
        let f = IniFile {
            sections: vec![IniSection {
                name: "S".into(),
                entries: vec![
                    IniEntry("+".into(), "K".into(), "a".into()),
                    IniEntry("+".into(), "k".into(), "A".into()),
                    IniEntry(".".into(), "K".into(), "b".into()),
                    IniEntry("-".into(), "K".into(), "a".into()),
                ],
            }],
        };
        assert_eq!(config_values(&f, "s", "K", vec![]), ["b"]);
        assert_eq!(config_values(&f, "s", "K", vec!["x".into()]), ["x", "b"]);
    }
}
