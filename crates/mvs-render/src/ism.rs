//! The stage's foliage: FInstancedStaticMeshVertexFactory's instances (an InstanceVF buffer and its instance SRVs,
//! read by the cooked vertex shader's manual vertex fetch) of the scenery's FoliageInstancedStaticMeshComponents
//!
//!
//! The cooked vertex shader (TBasePassVS of the ISM map) takes the position stream alone, SV_InstanceID and
//! SV_VertexID; instance i (SV_InstanceID + InstanceOffset) reads VertexFetch_InstanceOriginBuffer[i] (the origin in
//! the component's space, w the instance's random), VertexFetch_InstanceTransformBuffer[3i..3i+3] (the X, Y and Z
//! axes times the scale, w the hit proxy colour and the selected bit), VertexFetch_InstanceLightmapBuffer[i] (the
//! lightmap UV bias in xy, the shadow map's in zw); the rest from LocalVF as the local factory, the Primitive buffer
//! for the component's transform, PrecomputedLightingBuffer for a lightmapped component, and the loose $Globals
//! (InstancingViewZCompareZero / One, InstancingViewZConstant, InstancingWorldViewOriginZero / One, InstancingOffset,
//! InstancingFadeOutParams, InstanceOffset).

use std::collections::HashMap;
use std::sync::Arc;

use mvs_view::{MaterialRef, MeshPose};
use serde::Deserialize;
use windows::Win32::Graphics::Direct3D11::ID3D11Buffer;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::draw::{Bound, DrawItem, MeshSlice, VfInput};
use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, InputElement, ShaderId, rows_bytes};
use crate::renderer::{Renderer, StaticMesh};
use crate::scenery::{InstanceLightmap, SceneryOut, lightmap_params, scenery_lightmap};
use crate::stage::{to_world, transform};
use crate::view::{LightmapRows, SceneBuffers};
use crate::{Error, Result};

/// One component's instances on the GPU: its InstanceVF buffer, the instance origin, transform and lightmap bias
/// SRVs, its loose parameters and its lightmap rows.
#[derive(Debug)]
pub struct IsmInstances {
    /// The instance count (the draw's).
    pub count: u32,
    /// InstanceVF's constants, its 0x30 bytes (NumCustomDataFloats at 0x20: 0).
    vf: Buffer,
    origin: Buffer,
    transform: Buffer,
    lightmap: Buffer,
    /// InstanceCustomDataBuffer: one float (no custom data).
    custom: Buffer,
    /// The vertex shader's loose $Globals.
    globals: Buffer,
    /// PrecomputedLightingBuffer: the component's lightmap rows (a lightmapped component).
    lighting: Option<Buffer>,
}

impl IsmInstances {
    /// The PrecomputedLightingBuffer of a lightmapped component.
    pub(crate) fn lighting(&self) -> Option<ID3D11Buffer> {
        self.lighting.as_ref().map(|b| b.buf.clone())
    }
}

/// A foliage component of the scenery kind's entry: its mesh, placement,
/// materials, lightmap, channels, and its instances' rows in the entry's body (count x 14 floats: three transform
/// rows and the lightmap UV bias).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneryFoliage {
    pub name: String,
    /// The mesh's key in the scenery's meshes.
    pub mesh: String,
    pub loc: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
    /// Per slot the material's draw reference (`?ctx=locallm` or `static`), None for a slot not drawn.
    pub materials: Vec<Option<String>>,
    /// The component's HQ lightmap (a component with build data).
    pub(crate) lm: Option<InstanceLightmap>,
    pub channels: [bool; 3],
    /// CastShadow: no shadow pass reads it.
    pub cast_shadow: bool,
    pub mobility: Option<String>,
    pub count: u32,
    /// The rows' byte offset in the entry's body.
    pub data: usize,
}

/// The floats an instance's row holds in the entry's body.
const ROW_FLOATS: usize = 14;

/// The $Globals rows of the ISM vertex shaders: rows 1-7 the float4 parameters, row 8 x InstanceOffset.
const GLOBAL_ROWS: usize = 9;

/// A component's instances made on the GPU from its rows in the entry's `body`.
// Unverified: the buffers are float4 (the kind's floats), where FStaticMeshInstanceBuffer may store the transform as
// half floats and the lightmap bias as 16-bit snorm; the instance's random (InstanceOrigin.w) is 0, as the kind does
// not hold PerInstanceRandom (the four foliage materials' pixel shaders do not read it); the hit proxy
// colour and selected bit (the transform rows' w) are 0, as a cooked game's.
pub fn build(gpu: &Gpu, f: &SceneryFoliage, body: &[u8]) -> Result<Arc<IsmInstances>> {
    let n = f.count as usize;
    let bytes = body.get(f.data..f.data + n * ROW_FLOATS * 4).ok_or_else(|| Error::Format {
        what: f.name.clone(),
        why: format!("{n} instances at {} past the body's {} bytes", f.data, body.len()),
    })?;
    let row: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let mut origin = Vec::with_capacity(n * 4);
    let mut axes = Vec::with_capacity(n * 12);
    let mut bias = Vec::with_capacity(n * 4);
    for d in row.as_chunks::<ROW_FLOATS>().0 {
        // The rows are the 3 x 4 matrix's (d[4r..4r+4] = the X, Y, Z axes' and the translation's r component).
        origin.extend([d[3], d[7], d[11], 0.0]);
        for a in 0..3 {
            axes.extend([d[a], d[4 + a], d[8 + a], 0.0]);
        }
        bias.extend([d[12], d[13], 0.0, 0.0]);
    }
    let typed = |v: &[f32], format| -> Result<Buffer> {
        let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        gpu.buffer(&BufDesc { kind: BufKind::Typed { format }, size: b.len() as u32, dynamic: false }, Some(&b))
    };
    let constant = |rows: &[[u32; 4]]| -> Result<Buffer> {
        gpu.buffer(
            &BufDesc { kind: BufKind::Constant, size: (rows.len() * 16) as u32, dynamic: false },
            Some(rows_bytes(rows)),
        )
    };
    let mut globals = [[0u32; 4]; GLOBAL_ROWS];
    // InstancingFadeOutParams: StartCullDistance 0, 1 / (End - Start) 0 (no cull distance), render selected and
    // unselected 1; every other parameter 0, so no instance fades or takes a LOD fade.
    // Unverified: HISM foliage's LOD fade and cull distance (the build's InstancingViewZCompare / Constant values) not
    // read; the port draws LOD 0 with no culling.
    globals[7] = [0.0f32, 0.0, 1.0, 1.0].map(f32::to_bits);
    let lighting = match &f.lm {
        Some(lm) => {
            let mut rows = LightmapRows::default();
            rows.push(&lightmap_params(lm, false));
            Some(constant(&rows.0[0])?)
        }
        None => None,
    };
    Ok(Arc::new(IsmInstances {
        count: f.count,
        vf: constant(&[[0; 4]; 3])?,
        origin: typed(&origin, DXGI_FORMAT_R32G32B32A32_FLOAT)?,
        transform: typed(&axes, DXGI_FORMAT_R32G32B32A32_FLOAT)?,
        lightmap: typed(&bias, DXGI_FORMAT_R32G32B32A32_FLOAT)?,
        custom: typed(&[0.0], DXGI_FORMAT_R32_FLOAT)?,
        globals: constant(&globals)?,
        lighting,
    }))
}

/// How a foliage slot draws: with the component's HQ lightmap (`static`) or as a movable component (`locallm`).
fn lightmapped(r: &Option<String>, what: &str) -> Result<Option<bool>> {
    let Some(r) = r else { return Ok(None) };
    match r.split_once("?ctx=").map(|(_, c)| c) {
        Some("static") => Ok(Some(true)),
        Some("locallm" | "local") => Ok(Some(false)),
        _ => Err(Error::Format { what: what.to_string(), why: format!("{r}: a draw context foliage does not draw") }),
    }
}

/// The scenery's foliage components as static meshes drawn with their instances: per component and kind of slot
/// (lightmapped or not) one StaticMesh showing those sections, its mesh's LOD 0 at the component's place.
// Deviation: LOD 0 only and no cull distance. Its two other deviations are not carried:
// the pixel shader and the world position offset are the cooked ISM pass's own. A component receives the decals when
// the scenery has none (the entry's receivers are its instances).
pub(crate) fn foliage_statics(r: &Renderer, scenery: &mut SceneryOut) -> Result<Vec<StaticMesh>> {
    let mut out = Vec::new();
    let mut cache = std::mem::take(&mut scenery.lightmap_cache);
    let meshes: &HashMap<String, String> = &scenery.meshes;
    for f in &scenery.foliage {
        let what = format!("{}: {}", f.name, f.mesh);
        let path = meshes
            .get(&f.mesh)
            .ok_or_else(|| Error::Format { what: f.name.clone(), why: format!("no mesh {}", f.mesh) })?;
        let mesh = r.assets().mesh(path)?;
        let instances = build(r.gpu(), f, &scenery.body)?;
        let slots = f.materials.iter().map(|m| lightmapped(m, &what)).collect::<Result<Vec<_>>>()?;
        let sections = &mesh.lods.first().ok_or_else(|| Error::Format { what: what.clone(), why: "no LOD".into() })?;
        let of = |k: usize| slots.get(sections.sections[k].material as usize).copied().flatten();
        for lit in [false, true] {
            let shown: Vec<bool> = (0..sections.sections.len()).map(|k| of(k) == Some(lit)).collect();
            if !shown.contains(&true) {
                continue;
            }
            let lightmap = if lit {
                let lm = f.lm.as_ref().ok_or_else(|| Error::Format {
                    what: what.clone(),
                    why: "a lightmapped slot without a lightmap".into(),
                })?;
                Some(scenery_lightmap(r.assets(), &scenery.lightmaps, &mut cache, lm, false, &what)?)
            } else {
                None
            };
            let pose = MeshPose {
                mesh: Arc::from(path.as_str()),
                to_world: to_world(&transform(f.loc, f.rot, f.scale)),
                materials: f
                    .materials
                    .iter()
                    .map(|m| {
                        let p = m.as_deref().map_or("", |p| p.split_once('?').map_or(p, |(p, _)| p));
                        MaterialRef { path: Arc::from(p), ..Default::default() }
                    })
                    .collect(),
                shown,
                ..Default::default()
            };
            let channels = f.channels.iter().enumerate().fold(0, |m, (k, on)| m | ((*on as u8) << k));
            let receives_decals = scenery.decals.is_empty();
            out.push(StaticMesh {
                pose,
                lightmap,
                channels,
                instances: Some(instances.clone()),
                receives_decals,
                sort: 0.0,
            });
        }
    }
    scenery.lightmap_cache = cache;
    Ok(out)
}

/// FInstancedStaticMeshVertexFactory's streams and input layout for `item`: the position stream alone (ATTRIBUTE0;
/// the instance and the rest are fetched by SV_InstanceID / SV_VertexID).
pub(crate) fn set_streams(
    gpu: &Gpu,
    scene: &SceneBuffers,
    item: &DrawItem,
    slice: &MeshSlice,
    vs: ShaderId,
) -> Result<()> {
    let _ = (scene, item);
    let b = &slice.mesh.lods[slice.lod as usize].buffers;
    let elems = [InputElement {
        semantic: c"ATTRIBUTE",
        index: 0,
        format: DXGI_FORMAT_R32G32B32_FLOAT,
        slot: 0,
        per_instance: false,
    }];
    gpu.set_input_layout(vs, 3, &elems)?;
    let vbs = [Some(b.position.buf.clone())];
    // SAFETY: one buffer of this device, one stride and offset.
    unsafe { gpu.ctx.IASetVertexBuffers(0, 1, Some(vbs.as_ptr()), Some([12u32].as_ptr()), Some([0u32].as_ptr())) };
    Ok(())
}

/// The InstanceVF buffer (`k` u16::MAX) or its resource `k` of `item`'s instances: 0 the origins, 1 the
/// transforms, 2 the lightmap biases, 3 the custom data.
pub(crate) fn resource(item: &DrawItem, k: u16) -> Bound {
    let VfInput::StaticInstances(i) = &item.vf_input else { return Bound::None };
    let srv = |b: &Buffer| b.srv.clone().map_or(Bound::None, Bound::Srv);
    match k {
        u16::MAX => Bound::Buffer(i.vf.buf.clone()),
        0 => srv(&i.origin),
        1 => srv(&i.transform),
        2 => srv(&i.lightmap),
        3 => srv(&i.custom),
        _ => Bound::None,
    }
}

/// The ISM vertex shader's loose $Globals of `item`.
pub(crate) fn globals(item: &DrawItem) -> Option<ID3D11Buffer> {
    match &item.vf_input {
        VfInput::StaticInstances(i) => Some(i.globals.buf.clone()),
        _ => None,
    }
}
