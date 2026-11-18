//! The `sky` kind: per sky level (each stage's manifest `sky`, once, and the podium's), the level's sky light as the
//! engine processes it at load (`kinds/sky_compute.rs`): a json-bin container (SKY_MAGIC) whose header holds the
//! light's colour, SH and average brightness and whose body holds the filtered cube (an MVTX texture container,
//! FloatRGBA) and the pre-integrated GF table.
//! The layout is read by `mvs-render` (stage.rs load_sky).
//!
//! Reads the stages from the store's gamedata kind; the levels' sky light components through `Cx::object`, their
//! cubes from the texture kind's build in-process (`Texture.build`, its header read back, mip 0).

use mvs_uasset::value::Value;

use super::sky_compute::{
    GF_HEIGHT, GF_WIDTH, ProcessedSky, SkyCubeSource, SkyLightSettings, pre_integrated_gf, process_sky,
};
use super::texture::{
    TEXTURE_FORMATS, TEXTURE_KIND_VERSION, Texture, TextureHeader, TextureMip, format_code, read_header,
    texture_header_size, write_header,
};
use crate::game::{
    GAMEDATA, MAPS_ROOT, SKY_BIND_VERSION, SKY_KIND_VERSION, exported, sky_entry_path, sky_levels, stages,
};
use crate::jsonbin::{BodyRef, json_bin, lay_out_body};
use crate::kind::{Cx, Item, Kind, KindError};
use crate::kinds::lighting::{LIGHTING_MAGIC, Lighting};
use crate::kinds::object::OBJECT_KIND_VERSION;
use crate::objview::{field, num, short_name, string, truthy};
use crate::stage_levels::level_built_data;
use mvs_material::jsonbin::JsonBin;

/// The entry's magic.
pub const SKY_MAGIC: &[u8; 4] = b"MVSK";

/// The entry's layout (2: the reflection captures).
pub const SKY_FORMAT_VERSION: u32 = 2;

/// The entry's header.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u32,
    /// The level the sky light is in, below the maps root.
    level: String,
    /// The sky light component's object path and its cube's.
    light: String,
    cubemap: String,
    bind_version: u64,
    object_version: u64,
    texture_version: u64,
    average_brightness: f64,
    color: Vec<f64>,
    sh: Vec<Vec<f64>>,
    cube: BodyRef,
    gf: BodyRef,
    gf_size: [usize; 2],
    /// The sphere reflection captures of the world the sky level lights ([`captures`]).
    captures: Vec<Capture>,
}

/// A SphereReflectionCaptureComponent of the world, with its build data (FReflectionCaptureMapBuildData): what the
/// reflection pass's ReflectionCapture rows hold (PositionAndRadius, CaptureProperties.x, CaptureOffsetAndAverageBrightness).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Capture {
    /// Its level below the maps root and its object path.
    level: String,
    component: String,
    /// World position (RelativeLocation: the root component of an unattached actor), cm.
    position: [f64; 3],
    influence_radius: f64,
    brightness: f64,
    capture_offset: [f64; 3],
    /// The build data's AverageBrightness.
    average_brightness: f64,
    /// The build data's FullHDRCapturedData as a texture kind entry (FloatRGBA, a cube, every mip).
    cube: BodyRef,
}

/// A colour struct's members as numbers (`rgba` of `bindSky`).
fn rgba(level: &str, v: &Value, keys: &[&str]) -> Result<Vec<f64>, KindError> {
    let o = v.as_struct();
    keys.iter()
        .map(|k| {
            o.and_then(|o| field(o, k))
                .and_then(num)
                .ok_or_else(|| KindError::Failed(format!("sky {level}: a colour without {k}")))
        })
        .collect()
}

/// The processed sky of a level's sky light (the first world-affecting SkyLightComponent in export order), and its
/// header (`bindSky`).
fn bind_sky(cx: &Cx<'_>, level: &str) -> Result<(Header, ProcessedSky), KindError> {
    let fail = |m: String| KindError::Failed(format!("sky {level}: {m}"));
    let world = cx.object(&format!("{MAPS_ROOT}{level}"))?;
    let comp = world
        .inner()?
        .into_iter()
        .find(|o| {
            short_name(o.class()) == "SkyLightComponent" && !matches!(o.prop("bAffectsWorld"), Some(Value::Bool(false)))
        })
        .ok_or_else(|| fail("no world-affecting SkyLightComponent".into()))?;
    let prop = |n: &str| comp.prop(n);
    let source_type = prop("SourceType").and_then(Value::as_str);
    let angle = prop("SourceCubemapAngle");
    if source_type != Some("SLS_SpecifiedCubemap") || angle.is_some_and(truthy) {
        return Err(fail(format!(
            "a sky light of source {}, angle {} (the capture is ported for a specified cubemap at angle 0)",
            source_type.unwrap_or("undefined"),
            angle.and_then(num).unwrap_or(0.0)
        )));
    }
    if let Some(m) = prop("Mobility")
        && m.as_str() != Some("EComponentMobility::Stationary")
    {
        return Err(fail(format!(
            "a {} sky light (the capture is ported for a stationary one)",
            m.as_str().unwrap_or("?")
        )));
    }
    let cubemap = prop("Cubemap").and_then(|v| string(comp.cache(), v)).ok_or_else(|| fail("no Cubemap".into()))?;
    // USkyLightComponent constructor (exe 1429a5ff9..): Mobility Stationary, bLowerHemisphereIsBlack 1,
    // CubemapResolution 128; LowerHemisphereColor (0, 0, 0, 1) and Intensity 1.
    let lower_hemisphere = match prop("LowerHemisphereColor") {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(v) => <[f64; 4]>::try_from(rgba(level, v, &["R", "G", "B", "A"])?).expect("four members"),
    };
    let light_color = match prop("LightColor") {
        None => None,
        Some(v) => Some(<[f64; 3]>::try_from(rgba(level, v, &["R", "G", "B"])?).expect("three members")),
    };
    let light = SkyLightSettings {
        resolution: prop("CubemapResolution").and_then(num).map_or(128, |r| r as u32),
        lower_hemisphere_is_black: prop("bLowerHemisphereIsBlack").and_then(Value::as_bool).unwrap_or(true),
        lower_hemisphere,
        light_color,
        intensity: prop("Intensity").and_then(num).unwrap_or(1.0),
    };
    let tex = Texture.build(cx, &Item::new(cubemap.clone()))?;
    let t = read_header(&tex)?;
    let format = TEXTURE_FORMATS[usize::from(t.format)];
    if !t.cube || t.width != t.height || (format != "FloatRGBA" && format != "DXT1") {
        return Err(fail(format!(
            "{cubemap} is a {} x {} {format}{} (FloatRGBA or DXT1 cubes are read)",
            t.width,
            t.height,
            if t.cube { " cube" } else { "" }
        )));
    }
    let m0 = t.mips.first().ok_or_else(|| fail(format!("{cubemap} has no mips")))?;
    let mip0 = &tex[m0.offset as usize..m0.offset as usize + m0.length as usize];
    let source = SkyCubeSource { format, size: u32::from(t.width), srgb: t.srgb, mip0 };
    let sky = process_sky(&light, &source)?;
    let header = Header {
        version: SKY_FORMAT_VERSION,
        level: level.to_owned(),
        light: comp.path().to_owned(),
        cubemap,
        bind_version: SKY_BIND_VERSION,
        object_version: u64::from(OBJECT_KIND_VERSION),
        texture_version: TEXTURE_KIND_VERSION,
        average_brightness: sky.average_brightness,
        color: sky.color.clone(),
        sh: sky.sh.clone(),
        cube: BodyRef::default(),
        gf: BodyRef::default(),
        gf_size: [GF_WIDTH, GF_HEIGHT],
        captures: Vec::new(),
    };
    Ok((header, sky))
}

/// The levels with reflection capture components of the stages the sky level `level` lights, in LabLevelsToLoad
/// order; the stages that share it must load the same ones (none: the podium's sky).
fn capture_levels(cx: &Cx<'_>, level: &str) -> Result<Vec<String>, KindError> {
    let st = stages(cx)?;
    let mut out: Option<(String, Vec<String>)> = None;
    for key in &st.keys {
        let m = &st.entries[key].manifest;
        if m.sky != level {
            continue;
        }
        let mut with = Vec::new();
        for (l, _) in &m.levels {
            let world = cx.object(&format!("{MAPS_ROOT}{l}"))?;
            if world.inner()?.iter().any(|o| short_name(o.class()).ends_with("ReflectionCaptureComponent")) {
                with.push(l.clone());
            }
        }
        match &out {
            Some((k, o)) if *o != with => {
                return Err(KindError::Failed(format!(
                    "sky {level}: stages {k} and {key} light it with reflection captures in {o:?} and {with:?}"
                )));
            }
            Some(_) => {}
            None => out = Some((key.clone(), with)),
        }
    }
    Ok(out.map(|(_, l)| l).unwrap_or_default())
}

/// The sphere reflection captures of the world the sky level lights, each with its cube as a texture kind entry: every
/// SphereReflectionCaptureComponent of [`capture_levels`] in export order, its build data from its level's registry.
fn captures(cx: &Cx<'_>, level: &str) -> Result<Vec<(Capture, Vec<u8>)>, KindError> {
    let mut out = Vec::new();
    for l in capture_levels(cx, level)? {
        let fail = |m: String| KindError::Failed(format!("sky {level}: {l}: {m}"));
        let world = cx.object(&format!("{MAPS_ROOT}{l}"))?;
        let objects = world.inner()?;
        let registry =
            level_built_data(&objects).ok_or_else(|| fail("reflection captures without build data".into()))?;
        let bytes = Lighting.build(cx, &Item::new(registry.clone()))?;
        let what = format!("lighting {registry}");
        let jb = JsonBin::split(LIGHTING_MAGIC, &bytes, &what).map_err(|e| fail(e.to_string()))?;
        let built: serde_json::Value = jb.parse(&what).map_err(|e| fail(e.to_string()))?;
        for comp in objects.iter().filter(|o| short_name(o.class()).ends_with("ReflectionCaptureComponent")) {
            let class = short_name(comp.class());
            if class != "SphereReflectionCaptureComponent" {
                return Err(fail(format!("{}: a {class} (sphere captures are ported)", comp.path())));
            }
            let fail = |m: &str| fail(format!("{}: {m}", comp.path()));
            if comp.prop("AttachParent").is_some() {
                return Err(fail("an attached capture"));
            }
            let vec3 = |name: &str| -> Result<[f64; 3], KindError> {
                match comp.prop(name) {
                    None => Ok([0.0; 3]),
                    Some(v) => {
                        let s = v.as_struct().ok_or_else(|| fail(&format!("{name} is no struct")))?;
                        let c = |k: &str| field(s, k).and_then(num).unwrap_or(0.0);
                        Ok([c("X"), c("Y"), c("Z")])
                    }
                }
            };
            let scalar = |name: &str, d: f64| match comp.prop(name) {
                None => Ok(d),
                Some(v) => num(v).ok_or_else(|| fail(&format!("{name} is no number"))),
            };
            let guid = comp
                .prop("MapBuildDataId")
                .and_then(Value::as_str)
                .map(|g| g.replace('-', ""))
                .ok_or_else(|| fail("no MapBuildDataId"))?;
            let b = built
                .get("reflectionCaptures")
                .and_then(|c| c.get(&guid))
                .ok_or_else(|| fail(&format!("no build data {guid} in {registry}")))?;
            let size =
                b.get("CubemapSize").and_then(serde_json::Value::as_u64).ok_or_else(|| fail("no CubemapSize"))?;
            let average_brightness = b
                .get("AverageBrightness")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| fail("no AverageBrightness"))?;
            let r: mvs_material::jsonbin::BodyRef = b
                .get("FullHDRCapturedData")
                .and_then(|r| serde_json::from_value(r.clone()).ok())
                .ok_or_else(|| fail("no FullHDRCapturedData"))?;
            let texels = jb.section(r, &what).map_err(|e| fail(&e.to_string()))?;
            let size = u32::try_from(size).map_err(|_| fail("a CubemapSize past u32"))?;
            let cube = cube_entry(size, texels).map_err(|m| fail(&m))?;
            out.push((
                Capture {
                    level: l.clone(),
                    component: comp.path().to_owned(),
                    position: vec3("RelativeLocation")?,
                    // Unverified: the class defaults InfluenceRadius 3000 and Brightness 1 are 4.26's
                    // (USphereReflectionCaptureComponent / UReflectionCaptureComponent constructors, not read in the exe).
                    influence_radius: scalar("InfluenceRadius", 3000.0)?,
                    brightness: scalar("Brightness", 1.0)?,
                    capture_offset: vec3("CaptureOffset")?,
                    average_brightness,
                    cube: BodyRef::default(),
                },
                cube,
            ));
        }
    }
    Ok(out)
}

/// A cube of `size` with every mip down to 1 x 1 as a texture kind entry (FloatRGBA), from `texels`: the mips in
/// order, each its six faces in order, rows of FFloat16Color (FullHDRCapturedData's order: each face's mean colour holds
/// from mip to mip in M011's capture).
fn cube_entry(size: u32, texels: &[u8]) -> Result<Vec<u8>, String> {
    if !size.is_power_of_two() {
        return Err(format!("a cube of size {size}"));
    }
    let count = size.trailing_zeros() + 1;
    let mut at = texture_header_size(count as usize);
    let mut mips = Vec::new();
    for m in 0..count {
        let n = size >> m;
        let length = 6 * n * n * 8;
        let side = u16::try_from(n).map_err(|_| format!("a cube of size {size}"))?;
        mips.push(TextureMip {
            width: side,
            height: side,
            offset: u32::try_from(at).expect("an entry under 4 GB"),
            length,
        });
        at = (at + length as usize + 15) & !15;
    }
    let total: usize = mips.iter().map(|m| m.length as usize).sum();
    if total != texels.len() {
        return Err(format!("{} bytes of texels for a {size} cube's mips of {total}", texels.len()));
    }
    let mut cube = vec![0u8; at];
    let side = u16::try_from(size).map_err(|_| format!("a cube of size {size}"))?;
    let h = TextureHeader {
        format: format_code("FloatRGBA").expect("a texture format"),
        srgb: false,
        cube: true,
        width: side,
        height: side,
        depth: 6,
        streamed: 0,
        mips,
    };
    write_header(&h, &mut cube);
    let mut from = 0;
    for m in &h.mips {
        let (o, len) = (m.offset as usize, m.length as usize);
        cube[o..o + len].copy_from_slice(&texels[from..from + len]);
        from += len;
    }
    Ok(cube)
}

/// The entry's bytes: the header, then the cube as a texture kind entry (FloatRGBA, a cube, every mip), then the
/// PreIntegratedGF table (`skyEntry`), then each reflection capture's cube.
fn sky_entry(mut header: Header, sky: &ProcessedSky, captures: Vec<(Capture, Vec<u8>)>) -> Result<Vec<u8>, KindError> {
    let (rows, cubes): (Vec<Capture>, Vec<Vec<u8>>) = captures.into_iter().unzip();
    let mut at = texture_header_size(sky.mips as usize);
    let mut mips = Vec::new();
    for m in 0..sky.mips {
        let n = sky.size >> m;
        let length = 6 * n * n * 8;
        let side = u16::try_from(n).expect("a cube size");
        mips.push(TextureMip {
            width: side,
            height: side,
            offset: u32::try_from(at).expect("an entry under 4 GB"),
            length,
        });
        at = (at + length as usize + 15) & !15;
    }
    let mut cube = vec![0u8; at];
    let size = u16::try_from(sky.size).expect("a cube size");
    let h = TextureHeader {
        format: format_code("FloatRGBA").expect("a texture format"),
        srgb: false,
        cube: true,
        width: size,
        height: size,
        depth: 6,
        streamed: 0,
        mips,
    };
    write_header(&h, &mut cube);
    let texels: Vec<u8> = sky.cube.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut from = 0;
    for m in &h.mips {
        let (o, len) = (m.offset as usize, m.length as usize);
        cube[o..o + len].copy_from_slice(&texels[from..from + len]);
        from += len;
    }
    if from != texels.len() {
        return Err(KindError::Failed(format!(
            "sky {}: {} bytes of texels for mips of {from}",
            header.level,
            texels.len()
        )));
    }
    let gf: Vec<u8> = pre_integrated_gf().iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut sections = vec![&cube[..], &gf[..]];
    sections.extend(cubes.iter().map(|c| &c[..]));
    let (refs, _) = lay_out_body(&sections);
    header.cube = refs[0];
    header.gf = refs[1];
    header.captures = rows.into_iter().zip(&refs[2..]).map(|(c, r)| Capture { cube: *r, ..c }).collect();
    json_bin(SKY_MAGIC, &header, &sections, &refs).map_err(|e| KindError::Failed(e.to_string()))
}

/// The levels the kind holds: each stage's sky level once, and the podium's.
fn levels(cx: &Cx<'_>) -> Result<Vec<String>, KindError> {
    let st = stages(cx)?;
    Ok(sky_levels(st.keys.iter().map(|k| st.entries[k].manifest.sky.as_str())))
}

/// The `sky` kind.
pub struct Sky;

impl Kind for Sky {
    fn id(&self) -> &'static str {
        "sky"
    }

    fn version(&self) -> u64 {
        SKY_KIND_VERSION
    }

    fn content_type(&self, _: &[u8]) -> &'static str {
        "application/octet-stream"
    }

    /// Each sky level's entry (`game::sky_levels` over the stages' manifests, `exported(sky_entry_path(level))`).
    fn list(&self, cx: &Cx<'_>) -> Result<Vec<Item>, KindError> {
        Ok(levels(cx)?.iter().map(|l| Item::new(exported(&sky_entry_path(l)))).collect())
    }

    fn build(&self, cx: &Cx<'_>, item: &Item) -> Result<Vec<u8>, KindError> {
        let level = levels(cx)?
            .into_iter()
            .find(|l| exported(&sky_entry_path(l)) == item.path)
            .ok_or_else(|| KindError::NotFound(format!("sky-import: no sky level for {}", item.path)))?;
        let (header, sky) = bind_sky(cx, &level)?;
        let captures = captures(cx, &level)?;
        sky_entry(header, &sky, captures)
    }

    fn reads(&self) -> &'static [(&'static str, u64)] {
        &[GAMEDATA]
    }
}
