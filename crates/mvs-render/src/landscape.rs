//! The stage's landscape: FLandscapeVertexFactory's shared grid (1024 float4 vertices) and its five LOD index
//! lists, per component a LandscapeParameters buffer, a primitive and lightmap row and its material instance, and the
//! per-view section LOD and LOD bias buffers.
//!
//! Every component draws with the cooked base pass of its material (TBasePassVSTLightMapPolicyHQ and its pixel
//! shader in the FLandscapeVertexFactory map), as the build draws one mesh batch per component. The vertex shader
//! reads the component's LOD and its four neighbours' from the section LOD buffer (LandscapeContinuousLODParameters
//! t0) at the cells of the grid that buffer covers, takes floor(own LOD) as the batch its vertices belong to, and
//! morphs between that batch's grid and the next and towards the neighbours' LODs at the edges.

use std::sync::Arc;

use indexmap::IndexMap;
use mvs_view::{Camera, M4};
use serde::Deserialize;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::Result;
use crate::assets::{LodBuffers, Mesh, MeshLod, MeshSection};
use crate::draw::{Bound, DrawItem, DrawList, LightmapRef, LightmapTextures, MeshSlice, PassMask, VfInput};
use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, InputElement, SamplerKey, ShaderId, StateKey, Texture};
use crate::material::{
    LightMapPolicy, LightMapScene, LightMapSurface, MaterialId, MaterialInstance, MaterialResource, Mobility, PassKind,
    Source, TextureSource, VertexFactory, light_map_policy,
};
use crate::renderer::Renderer;
use crate::scenery::{InstanceLightmap, LightmapCache, scenery_lightmap};
use crate::stagedraw::StageDraw;
use crate::view::{LightmapParams, LightmapRows, PrimitiveParams, PrimitiveRows, SceneBuffers, mul};

/// The scenery kind's `landscape`: the heightmaps, the weightmaps and the
/// landscape actors with their components.
#[derive(Clone, Debug, Deserialize)]
pub struct SceneryLandscape {
    /// The level's heightmap textures, by object path (the texture kind).
    pub(crate) heightmaps: Vec<String>,
    /// The level's weightmap textures, by object path.
    pub(crate) weightmaps: Vec<String>,
    actors: Vec<SceneryLandscapeActor>,
}

/// An ALandscape: its root's place and scale (no rotation), its render system's section grid (Min x, Min y, Size x,
/// Size y in components) and LastLOD.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneryLandscapeActor {
    name: String,
    loc: [f64; 3],
    scale: [f64; 3],
    grid: [i32; 4],
    #[serde(rename = "lastLOD")]
    last_lod: i32,
    components: Vec<SceneryLandscapeComponent>,
}

/// A ULandscapeComponent (one subsection of 31 quads): SectionBase in quads and its cell (SectionBase / 31), its
/// heightmap (an index into `heightmaps`) and HeightmapScaleBias, its WeightmapScaleBias, CachedLocalBox (min xyz,
/// max xyz; quads, heights in local units), its bounds sphere in the world (cm), its material (the static permutation
/// its MaterialInstances[0] is a child of, a `?ctx=landscape` draw reference) with the child's LayerMask_* vectors
/// and Weightmap0 (an index into `weightmaps`), its HQ lightmap and its lighting channels.
#[derive(Clone, Debug, Deserialize)]
struct SceneryLandscapeComponent {
    name: String,
    base: [i32; 2],
    cell: [i32; 2],
    hm: usize,
    hsb: [f32; 4],
    wsb: [f32; 4],
    #[serde(rename = "box")]
    bounds: [f64; 6],
    sphere: [f64; 4],
    material: String,
    #[serde(default)]
    vectors: IndexMap<String, [f32; 4]>,
    weightmap: usize,
    lm: InstanceLightmap,
    channels: [bool; 3],
}

/// ComponentSizeQuads and SubsectionSizeQuads (the reader refuses any other).
const QUADS: i32 = 31;
/// SubsectionSizeVerts.
const VERTS: i32 = QUADS + 1;
/// The LOD batches: (32 >> L) - 1 quads a side.
const BATCHES: usize = 5;

/// The landscape's LOD buffers of one actor: LandscapeContinuousLODParameters (the grid's Min and Size), the section
/// LOD buffer (a float per cell, written per view) and the section LOD bias buffer.
#[derive(Debug)]
struct LodBinds {
    params: Buffer,
    lods: Buffer,
    biases: Buffer,
}

/// A component's binds: its LandscapeParameters buffer and resources, its PrecomputedLightingBuffer, and the LOD
/// buffers of its actor.
#[derive(Debug)]
pub struct ComponentBinds {
    params: Buffer,
    precomputed: Buffer,
    heightmap: Arc<Texture>,
    /// HeightmapTextureSampler (and XYOffsetmapTextureSampler): 144e66d18, point / clamp.
    point: ID3D11SamplerState,
    /// NormalmapTextureSampler: 144e69308, bilinear / clamp.
    bilinear: ID3D11SamplerState,
    lod: Arc<LodBinds>,
}

/// A component as drawn: its binds, material instance, rows and where its LOD goes.
#[derive(Debug)]
struct Component {
    binds: Arc<ComponentBinds>,
    material: MaterialId,
    primitive: PrimitiveParams,
    lightmap: LightmapParams,
    textures: Arc<LightmapTextures>,
    channels: u8,
    /// The bounds sphere in the world: centre (cm), radius.
    sphere: [f64; 4],
    /// Its cell's index in the actor's section LOD buffer.
    cell: usize,
}

#[derive(Debug)]
struct Actor {
    lod: Arc<LodBinds>,
    /// The section LODs of the grid, per view (a cell with no component: 0).
    // Unverified: the grid as the components' bounding rectangle with an empty cell at 0 is assumed.
    lods: Vec<f32>,
    last_lod: i32,
    components: Vec<Component>,
}

/// The stage's landscapes on the GPU.
#[derive(Debug, Default)]
pub struct Landscape {
    /// The shared grid as a mesh: LOD L the batch L index list over the grid's top-left (q + 1)^2 vertices.
    grid: Option<Arc<Mesh>>,
    actors: Vec<Actor>,
    /// The components' light-map policy.
    policy: Option<LightMapPolicy>,
}

/// FLandscapeSharedBuffers (14238f360): the 32 x 32 vertices (x, y, SubX, SubY) as float4, x fastest (1423a3580),
/// and per LOD L (q = (32 >> L) - 1) the quads y outer, x inner, each the triangles (i00, i11, i10), (i00, i01, i11)
/// over the top-left (q + 1)^2 vertices of the grid (142387ed0, 16-bit).
fn grid(gpu: &Gpu) -> Result<Mesh> {
    let mut v = Vec::with_capacity((VERTS * VERTS * 16) as usize);
    for y in 0..VERTS {
        for x in 0..VERTS {
            for c in [x as f32, y as f32, 0.0, 0.0] {
                v.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    let position = gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: v.len() as u32, dynamic: false }, Some(&v))?;
    let mut lods = Vec::with_capacity(BATCHES);
    for l in 0..BATCHES {
        let q = (VERTS >> l) - 1;
        let mut index: Vec<u8> = Vec::with_capacity((q * q * 12) as usize);
        for y in 0..q {
            for x in 0..q {
                let i00 = (x + y * VERTS) as u16;
                let (i10, i01) = (i00 + 1, i00 + VERTS as u16);
                let i11 = i01 + 1;
                for i in [i00, i11, i10, i00, i01, i11] {
                    index.extend_from_slice(&i.to_le_bytes());
                }
            }
        }
        let index =
            gpu.buffer(&BufDesc { kind: BufKind::Index16, size: index.len() as u32, dynamic: false }, Some(&index))?;
        lods.push(MeshLod {
            sections: vec![MeshSection {
                material: 0,
                first_index: 0,
                triangles: (2 * q * q) as u32,
                cast_shadow: true,
            }],
            vertices: ((q + 1) * (q + 1)) as u32,
            buffers: LodBuffers {
                position: Buffer { buf: position.buf.clone(), srv: None, desc: position.desc },
                index,
                tex_coords: None,
                num_tex_coords: 0,
                tangents: None,
                color: None,
                local_vf: None,
                skin: Vec::new(),
            },
        });
    }
    Ok(Mesh {
        path: "FLandscapeSharedBuffers".into(),
        lods,
        materials: Vec::new(),
        bones: 0,
        extra_influences: false,
        bounds: [[15.5, 15.5, 0.0], [15.5, 15.5, 0.0]],
        lightmap_uv: 0,
        morphs: Vec::new(),
    })
}

/// LandscapeLightmapScaleBias (FLandscapeComponentSceneProxy::OnTransformChanged 1423a3c00): (ScaleX, ScaleY, BiasY,
/// BiasX) of a component of one subsection at StaticLightingResolution `res` (the component's 0: the proxy's 1.0)
/// and StaticLightingLOD 0, in float.
// Unverified: the steps (the lightmap's padded size rounded to a power of two, the ratio of the used part) are 4.26's
// GetTerrainExpandPatchCount; the exe's values for The Court (patch expand 4 from PF_DXT1's block, ratio 0.967742,
// scale 0.0241935, bias 0.0967742) agree with them.
fn lightmap_scale_bias(res: f32) -> [f32; 4] {
    // PF_DXT1's BlockSizeX (144c47b50); StaticLightingLOD 0.
    let expand = ((4.0 / res) as i32).max(1);
    let desired0 = ((VERTS as f32 * res) as i32).min(4096);
    let current = (((2 * expand + VERTS) as f32 * res) as i32).min(4096);
    let mut desired = desired0;
    if current > desired {
        let prior = 1 << (31 - desired.leading_zeros());
        desired = if current * current <= (prior * prior) << 1 { prior } else { prior << 1 };
    }
    let dest = (desired as f32 / current as f32 * (QUADS as f32 * res)) as i32;
    let ratio = dest as f32 / (QUADS as f32 * res) * current as f32 / desired as f32;
    let lod_scale = ratio / (VERTS + 2 * expand) as f32;
    let scale = lod_scale * (VERTS - 1) as f32 / QUADS as f32;
    let bias = expand as f32 * lod_scale;
    [scale, scale, bias, bias]
}

fn f4(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

/// A uniform buffer of `rows`, made once.
fn constant(gpu: &Gpu, rows: &[[u32; 4]]) -> Result<Buffer> {
    let bytes = crate::gpu::rows_bytes(rows);
    gpu.buffer(&BufDesc { kind: BufKind::Constant, size: bytes.len() as u32, dynamic: false }, Some(bytes))
}

impl Landscape {
    /// The landscape of the stage: the grid and index buffers, the components' buffers, rows and material instances
    /// (in `r.materials`). `lightmaps`: the scenery's lightmap texture paths, by a lightmap's `tex`.
    pub fn new(r: &mut Renderer, l: &SceneryLandscape, lightmaps: &[String], draw: &StageDraw) -> Result<Landscape> {
        let _ = draw;
        let gpu = r.gpu();
        let heightmaps = l.heightmaps.iter().map(|p| r.assets().texture(p)).collect::<Result<Vec<_>>>()?;
        let weightmaps = l.weightmaps.iter().map(|p| r.assets().texture(p)).collect::<Result<Vec<_>>>()?;
        let point = gpu.sampler(
            gpu.state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP))),
        );
        let bilinear = gpu.sampler(gpu.state(StateKey::Sampler(SamplerKey::of(
            D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
            D3D11_TEXTURE_ADDRESS_CLAMP,
        ))));
        // The weightmap's own sampler: TA_Clamp and TEXTUREGROUP_Terrain_Weightmap (SF_AnisotropicPoint).
        // Unverified: the group's filter is DefaultDeviceProfiles.ini's, its anisotropy the world group's 8; D3D11 has
        // no anisotropic filter with point mips, taken as D3D11_FILTER_ANISOTROPIC (the 4.26 D3D11 RHI's mapping).
        let weight_sampler = {
            let mut k = SamplerKey::of(D3D11_FILTER_ANISOTROPIC, D3D11_TEXTURE_ADDRESS_CLAMP);
            k.max_anisotropy = crate::texture_quality::max_anisotropy();
            gpu.state(StateKey::Sampler(k))
        };
        // StaticLightingResolution: the components' 0, so the proxy's 1.0 (ALandscapeProxy ctor 14237d520).
        let lmsb = lightmap_scale_bias(1.0);
        // A component's lightmap is a texture lightmap of a Static primitive: the HQ policy where the scene allows it.
        let surface = LightMapSurface {
            lit: true,
            translucent: false,
            light_cache: true,
            cast_volumetric_translucent_shadow: false,
            texture_light_map: true,
            hq_interaction: true,
            texture_shadow_map: false,
            mobility: Mobility::Static,
            preview_or_forced_volumetric: false,
            indirect_cache: None,
        };
        // Unverified: the STAGE scene's policy, not the one set_stage computes: a stage with a landscape is taken to
        // have its build data (true of The Court, the only one).
        let policy = Some(light_map_policy(&surface, &LightMapScene::STAGE));
        let mut cache = LightmapCache::new();
        let mut actors = Vec::with_capacity(l.actors.len());
        let mut owner = 0u32;
        for a in &l.actors {
            let [min_x, min_y, size_x, size_y] = a.grid;
            let cells = (size_x * size_y) as usize;
            let gpu = r.gpu();
            let lod = Arc::new(LodBinds {
                params: constant(gpu, &[[min_x, min_y, size_x, size_y].map(|x| x as u32)])?,
                lods: gpu.buffer(
                    &BufDesc {
                        kind: BufKind::Typed { format: DXGI_FORMAT_R32_FLOAT },
                        size: (cells * 4) as u32,
                        dynamic: true,
                    },
                    None,
                )?,
                // SectionLODBias (FUN_142393900): the heightmap's mips less its resident mips, 0 with every mip in.
                // Unverified: whether the build's streamer has the big heightmaps' top mips in from the first frame.
                biases: gpu.buffer(
                    &BufDesc {
                        kind: BufKind::Typed { format: DXGI_FORMAT_R32_FLOAT },
                        size: (cells * 4) as u32,
                        dynamic: false,
                    },
                    Some(&vec![0; cells * 4]),
                )?,
            });
            let mut components = Vec::with_capacity(a.components.len());
            for c in &a.components {
                let what = format!("{} {}", a.name, c.name);
                let at = |k: usize, list: &[Arc<Texture>], kind: &str| {
                    list.get(k)
                        .cloned()
                        .ok_or_else(|| crate::Error::Format { what: what.clone(), why: format!("no {kind} {k}") })
                };
                let heightmap = at(c.hm, &heightmaps, "heightmap")?;
                let weightmap = at(c.weightmap, &weightmaps, "weightmap")?;
                let lm = scenery_lightmap(r.assets(), lightmaps, &mut cache, &c.lm, false, &what)?;
                // The component's LocalToWorld: the root's scale, its place moved by SectionBase (its
                // RelativeLocation) under that scale.
                let s = a.scale;
                let t = [a.loc[0] + s[0] * c.base[0] as f64, a.loc[1] + s[1] * c.base[1] as f64, a.loc[2]];
                let m: M4 =
                    [[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0], [t[0], t[1], t[2], 1.0]];
                let b = c.bounds;
                let lo = [b[0], b[1], b[2]];
                let hi = [b[3], b[4], b[5]];
                let origin: [f64; 3] = std::array::from_fn(|k| t[k] + s[k] * (lo[k] + hi[k]) / 2.0);
                let extent: [f64; 3] = std::array::from_fn(|k| (s[k] * (hi[k] - lo[k]) / 2.0).abs());
                let channels = c.channels.iter().enumerate().fold(0u8, |m, (k, on)| m | ((*on as u8) << k));
                let primitive = PrimitiveParams {
                    local_to_world: m,
                    previous_local_to_world: m,
                    actor_world_position: t,
                    bounds_origin: origin,
                    bounds_extent: extent,
                    bounds_radius: (extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt(),
                    local_bounds_min: lo.map(|x| x as f32),
                    local_bounds_max: hi.map(|x| x as f32),
                    lighting_channels: channels,
                    receives_decals: true,
                    use_single_sample_shadow: false,
                    use_volumetric_lightmap_shadow: false,
                    lightmap_index: 0,
                    lightmap_uv_index: 0,
                    custom_data: [[0.0; 4]; 9],
                };
                // LandscapeParameters (OnTransformChanged 1423a3c00): ComponentBase, SubsectionSizeVerts,
                // NumSubsections; LastLOD; the stored scale-biases; LandscapeLightmapScaleBias;
                // SubsectionSizeVertsLayerUVPan; SubsectionOffsetParams; LightmapSubsectionOffsetParams;
                // BlendableLayerMask; LocalToWorldNoScaling.
                // Unverified: SubsectionOffsetParams' WeightmapSubsectionOffset (z) is not in the scenery kind and
                // BlendableLayerMask is 0; neither is read with one subsection by these shaders.
                let hm = heightmap.desc.size;
                let mut rows = [[0u32; 4]; 13];
                rows[0] = [c.cell[0], c.cell[1], VERTS, 1].map(|x| x as u32);
                rows[1] = [a.last_lod as u32, 0, 0, 0];
                rows[2] = f4(c.hsb);
                rows[3] = f4(c.wsb);
                rows[4] = f4(lmsb);
                rows[5] = f4([VERTS as f32, 1.0 / QUADS as f32, c.base[0] as f32, c.base[1] as f32]);
                rows[6] = f4([VERTS as f32 / hm[0] as f32, VERTS as f32 / hm[1] as f32, 0.0, QUADS as f32]);
                rows[7] = f4([QUADS as f32 * lmsb[0], QUADS as f32 * lmsb[1], 0.0, 0.0]);
                rows[9] = f4([1.0, 0.0, 0.0, 0.0]);
                rows[10] = f4([0.0, 1.0, 0.0, 0.0]);
                rows[11] = f4([0.0, 0.0, 1.0, 0.0]);
                rows[12] = f4([t[0] as f32, t[1] as f32, t[2] as f32, 1.0]);
                let gpu = r.gpu();
                let params = constant(gpu, &rows)?;
                let mut pl = LightmapRows::default();
                pl.push(&lm.params);
                let precomputed = constant(gpu, &pl.0[0])?;
                // The component's MaterialInstanceConstant: a child of the static permutation with its Weightmap0
                // and LayerMask_* values.
                let path = c.material.split_once('?').map_or(c.material.as_str(), |(p, _)| p);
                let mut inst = MaterialInstance::new(gpu, r.assets(), path)?;
                for (n, v) in &c.vectors {
                    inst.set_vector(n, *v);
                }
                let k =
                    inst.material.uniforms.textures.iter().position(|t| t.name == "Weightmap0").ok_or_else(|| {
                        crate::Error::Format { what: what.clone(), why: format!("{path} has no Weightmap0") }
                    })?;
                inst.set_texture("Weightmap0", TextureSource::Asset(weightmap));
                inst.resources[2 * k + 1] = MaterialResource::Sampler(weight_sampler);
                let binds = Arc::new(ComponentBinds {
                    params,
                    precomputed,
                    heightmap,
                    point: point.clone(),
                    bilinear: bilinear.clone(),
                    lod: lod.clone(),
                });
                let material = r.keep_material(crate::renderer::Owner::Landscape(owner), inst);
                owner += 1;
                let cell = ((c.cell[1] - min_y) * size_x + (c.cell[0] - min_x)) as usize;
                if cell >= cells {
                    return Err(crate::Error::Format { what, why: format!("cell {:?} off the grid", c.cell) });
                }
                components.push(Component {
                    binds,
                    material,
                    primitive,
                    lightmap: lm.params,
                    textures: lm.textures,
                    channels,
                    sphere: c.sphere,
                    cell,
                });
            }
            actors.push(Actor { lod, lods: vec![0.0; cells], last_lod: a.last_lod, components });
        }
        Ok(Landscape { grid: Some(Arc::new(grid(r.gpu())?)), actors, policy })
    }

    /// The frame's items: each visible component at its LOD batch, with its primitive and lightmap rows.
    pub fn add_items(
        &mut self,
        gpu: &Gpu,
        camera: &Camera,
        list: &mut DrawList,
        primitives: &mut PrimitiveRows,
        lightmaps: &mut LightmapRows,
    ) -> Result<()> {
        let (Some(grid), Some(policy)) = (&self.grid, self.policy) else { return Ok(()) };
        let p = &camera.view_to_clip;
        // ComputeBoundsScreenRadiusSquared (142d477f0): max(0.5 P00, 0.5 P11).
        let screen = (0.5 * p[0][0]).max(0.5 * p[1][1]);
        let planes = frustum(&mul(&camera.translated_world_to_view, p));
        for a in &mut self.actors {
            // ComputeSectionsLODForView (142398470), per component of one subsection: the screen radius squared of
            // its bounds sphere over (LODDistanceFactor x r.StaticMeshLODDistanceScale)^2, both 1; LastLOD at or
            // below the last LOD's screen size squared, a linear ramp between LOD 0's and LOD 1's, then logarithmic
            // in the distribution scalar 3^2; no ForcedLOD; at least 0.
            // Unverified: LODDistanceFactor 1 (the 4.26 source's per-view factor, not read).
            let last_sq = 0.16 / 9f64.powi(a.last_lod - 1);
            for c in &a.components {
                let [ox, oy, oz, r] = c.sphere;
                let o = camera.origin;
                let d2 = (ox - o[0]).powi(2) + (oy - o[1]).powi(2) + (oz - o[2]).powi(2);
                let s = (screen * r).powi(2) / d2.max(1.0);
                let lod = if s <= last_sq {
                    a.last_lod as f64
                } else if s > 0.16 {
                    (0.25 - s.min(0.25)) / (0.25 - 0.16)
                } else {
                    1.0 + (0.16 / s).ln() / 9f64.ln()
                };
                a.lods[c.cell] = lod.max(0.0) as f32;
            }
            let bytes: Vec<u8> = a.lods.iter().flat_map(|x| x.to_le_bytes()).collect();
            gpu.write(&a.lod.lods, &bytes);
            for c in &a.components {
                // The batch: floor(own LOD), as the cooked vertex shader takes it from the same buffer.
                // The exe draws the batch of floor(LOD) (14239e3f0, 1423a0760).
                let batch = a.lods[c.cell].floor() as usize;
                if batch >= BATCHES || !in_frustum(&planes, camera.origin, c.sphere) {
                    continue;
                }
                let row = lightmaps.push(&c.lightmap);
                let primitive = primitives.push(&PrimitiveParams { lightmap_index: row, ..c.primitive.clone() });
                list.opaque.push(DrawItem {
                    mesh: Some(MeshSlice { mesh: grid.clone(), lod: batch as u8, section: 0 }),
                    vf: VertexFactory::Landscape,
                    vf_input: VfInput::Landscape(c.binds.clone()),
                    material: c.material,
                    policy,
                    primitive,
                    lightmap: Some(LightmapRef { row, textures: c.textures.clone() }),
                    passes: PassMask::of(&[PassKind::Base]),
                    custom_stencil: 0,
                    lighting_channels: c.channels | crate::decal::RECEIVES_DECALS_BIT,
                    mirrored: false,
                    sort_key: 0.0,
                    after_meshes: false,
                    instances: 1,
                });
            }
        }
        Ok(())
    }
}

// Unverified: the culling (each component's world bounds sphere against the side and near planes, no far plane) is
// UE's FBoxSphereBounds test, not read in the exe.
/// The side planes of a row-vector translated-world-to-clip matrix (reversed Z: no far plane), each (normal, d)
/// with the normal of unit length: a point p is inside when n . p + d >= 0 for each.
fn frustum(m: &M4) -> [[f64; 4]; 5] {
    let col = |j: usize| [m[0][j], m[1][j], m[2][j], m[3][j]];
    let (x, y, z, w) = (col(0), col(1), col(2), col(3));
    let plane = |a: [f64; 4], sign: f64, b: [f64; 4]| {
        let p: [f64; 4] = std::array::from_fn(|k| a[k] + sign * b[k]);
        let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        p.map(|c| c / n)
    };
    // Left, right, bottom, top, and the near plane (reversed Z: clip z <= w).
    [plane(w, 1.0, x), plane(w, -1.0, x), plane(w, 1.0, y), plane(w, -1.0, y), plane(w, -1.0, z)]
}

fn in_frustum(planes: &[[f64; 4]; 5], origin: [f64; 3], s: [f64; 4]) -> bool {
    let c = [s[0] - origin[0], s[1] - origin[1], s[2] - origin[2]];
    planes.iter().all(|p| p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] >= -s[3])
}

/// FLandscapeVertexFactory's stream: ATTRIBUTE0, the grid's float4 (x, y, SubX, SubY), stride 16.
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
        format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        slot: 0,
        per_instance: false,
    }];
    // (the layout's key among the vertex shader's layouts: the factories' others are 1 and odd hashes | 2)
    gpu.set_input_layout(vs, 0x4c41_4e44, &elems)?;
    let vbs = [Some(b.position.buf.clone())];
    // SAFETY: one buffer of this device, its stride and offset.
    unsafe { gpu.ctx.IASetVertexBuffers(0, 1, Some(vbs.as_ptr()), Some(&16), Some(&0)) };
    Ok(())
}

/// `item`'s LandscapeParameters (Source::LandscapeVf) or LOD (Source::LandscapeLod) buffer (resource u16::MAX) or
/// resource, and its PrecomputedLightingBuffer.
// LandscapeParameters' resources: HeightmapTexture 0, HeightmapTextureSampler 1, NormalmapTexture 2 (the heightmap),
// NormalmapTextureSampler 3, XYOffsetmapTexture 4 and its sampler 5 (not read by FLandscapeVertexFactory's shaders);
// LandscapeContinuousLODParameters': the section LOD buffer 0, the section LOD bias buffer 1.
pub(crate) fn resource(item: &DrawItem, s: Source) -> Bound {
    let VfInput::Landscape(b) = &item.vf_input else { panic!("a {:?} item with no landscape binds", item.vf) };
    let srv = |t: &Option<ID3D11ShaderResourceView>| t.clone().map_or(Bound::None, Bound::Srv);
    match s {
        Source::LandscapeVf(u16::MAX) => Bound::Buffer(b.params.buf.clone()),
        Source::LandscapeVf(0 | 2) => srv(&b.heightmap.srv),
        Source::LandscapeVf(1 | 5) => Bound::Sampler(b.point.clone()),
        Source::LandscapeVf(3) => Bound::Sampler(b.bilinear.clone()),
        Source::LandscapeLod(u16::MAX) => Bound::Buffer(b.lod.params.buf.clone()),
        Source::LandscapeLod(0) => srv(&b.lod.lods.srv),
        Source::LandscapeLod(1) => srv(&b.lod.biases.srv),
        Source::PrecomputedLighting => Bound::Buffer(b.precomputed.buf.clone()),
        _ => Bound::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LandscapeLightmapScaleBias of a Court component: the exe's values (1423a3c00).
    #[test]
    fn court_lightmap_scale_bias() {
        let [sx, sy, by, bx] = lightmap_scale_bias(1.0);
        for (v, want) in [(sx, 0.0241935), (sy, 0.0241935), (by, 0.0967742), (bx, 0.0967742)] {
            assert!((v - want).abs() < 1e-7, "{v} {want}");
        }
    }

    /// The scenery kind's Court landscape reads: two actors, 285 components on their grids.
    #[test]
    fn court_landscape_reads() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no store: nothing to check");
            return;
        };
        let Ok(draw) = StageDraw::load(&store, "m010") else {
            eprintln!("no m010 stagedraw entry: nothing to check");
            return;
        };
        let bytes = store.kind("scenery").unwrap().bytes(&draw.scenery.entry).unwrap();
        #[derive(Deserialize)]
        struct H {
            scenery: S,
        }
        #[derive(Deserialize)]
        struct S {
            landscape: SceneryLandscape,
        }
        let h: H = mvs_material::jsonbin::JsonBin::split(b"MVSC", &bytes, "m010").unwrap().parse("m010").unwrap();
        let l = h.scenery.landscape;
        assert_eq!(l.actors.len(), 2);
        assert_eq!(l.actors.iter().map(|a| a.components.len()).sum::<usize>(), 285);
        for a in &l.actors {
            let [mx, my, sx, sy] = a.grid;
            assert!(
                a.components.iter().all(|c| (mx..mx + sx).contains(&c.cell[0]) && (my..my + sy).contains(&c.cell[1]))
            );
        }
    }
}
