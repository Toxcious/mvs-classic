//! The renderer: one frame from a FrameInput, offscreen into the scene targets and out to a texture, with its CPU
//! and GPU times.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use mvs_view::{FrameInput, M4, MaterialRef, MeshPose};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use mvs_vfx::draw::EmitterDraw;

use crate::assets::{Assets, LodBuffers, Mesh, MeshLod, MeshSection};
use crate::capture::Captures;
use crate::cmesh::{CMeshGeometry, ComponentMesh, ComponentMeshes, OwnGeometry};
use crate::decal::Decals;
use crate::draw::{DrawItem, DrawList, LightmapRef, LightmapTextures, MeshSlice, PassMask, VfInput, execute};
use crate::fx::FloorDecal;
use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, GpuTimer, Texture};
use crate::ism::IsmInstances;
use crate::landscape::Landscape;
use crate::lighting::Lighting;
use crate::lighting::shadow::SpotShadows;
use crate::lighting::tlv::Tlv;
use crate::material::{
    LightMapScene, LightMapSurface, MaterialId, MaterialInstance, Materials, Mobility, PassKind, TextureSource,
    VertexFactory, light_map_policy,
};
use crate::particles::Particles;
use crate::post::Post;
use crate::scene::LightSet;
use crate::scenery::{BlastBorder, LevelEmitter, SceneryRun};
use crate::targets::{SceneTargets, TargetId};
use crate::translucency::Translucency;
use crate::view::{
    LIGHTMAP_ROWS, LightmapParams, LightmapRows, PrimitiveParams, PrimitiveRows, SceneBuffers, VIEW_ROWS, ViewMembers,
    ViewParams, view_rows,
};
use crate::{Error, Result};

/// One frame's times, ms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameTimes {
    pub cpu: f64,
    /// The GPU's whole frame and its passes, from a frame a few frames back; None until one has been read.
    pub gpu: Option<f64>,
    pub passes: Vec<(&'static str, f64)>,
}

/// A lightmapped static mesh's lightmap: its rows and textures (the stage's scenery).
#[derive(Clone, Debug)]
pub struct StaticLightmap {
    pub params: LightmapParams,
    pub textures: Arc<LightmapTextures>,
    /// The shadow map is a texture (the distance-field shadows policy).
    pub shadow_map: bool,
}

/// A mesh of the stage that does not move: drawn every frame with its lightmap when it has one.
#[derive(Clone, Debug)]
pub struct StaticMesh {
    pub pose: MeshPose,
    pub lightmap: Option<StaticLightmap>,
    /// LightingChannelMask: channel k at bit k.
    pub channels: u8,
    /// An instanced static mesh component's instances (the foliage): drawn with FInstancedStaticMeshVertexFactory.
    pub instances: Option<Arc<IsmInstances>>,
    /// bReceivesDecals.
    pub receives_decals: bool,
    /// TranslucencySortPriority: its translucent sections' draw order key before their distance.
    pub sort: f32,
}

/// The passes besides the base pass a frame draws. Each is off until its module is written; render-frame's
/// `--passes` turns them on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Passes {
    /// PFGCamera's priority captures (capture.rs), before the base pass.
    pub captures: bool,
    /// Custom depth and stencil (post.rs): the outline's and the priority captures' input.
    pub custom_depth: bool,
    /// The deferred decals (decal.rs), after custom depth.
    pub decals: bool,
    /// The deferred lighting over the G-buffer into SceneColor (lighting.rs).
    pub lighting: bool,
    /// Height and atmospheric fog (post.rs).
    pub fog: bool,
    /// The translucent meshes and the separate translucency composite (translucency.rs).
    pub translucency: bool,
    /// The particle emitters and the distortion pass (particles.rs).
    pub particles: bool,
    /// Bloom, tonemap, FXAA, the outline, the copy to the frame's output (post.rs).
    pub post: bool,
}

impl Passes {
    pub const ALL: Passes = Passes {
        captures: true,
        custom_depth: true,
        decals: true,
        lighting: true,
        fog: true,
        translucency: true,
        particles: true,
        post: true,
    };
}

/// What a pass after the base pass reads and writes: the device, the store's assets, the frame's View and
/// GPUScene buffers, the scene targets, the material instances, the frame's draw items, the stage's lights and the
/// frame's input.
pub struct Frame<'a> {
    pub gpu: &'a Gpu,
    pub assets: &'a Assets,
    pub scene: &'a SceneBuffers,
    pub targets: &'a SceneTargets,
    pub materials: &'a mut Materials,
    pub list: &'a DrawList,
    /// The frame's GPUScene primitive rows, by the items' PrimitiveId.
    pub primitives: &'a PrimitiveRows,
    pub lights: &'a LightSet,
    pub input: &'a FrameInput<'a>,
}

/// Who a posed mesh belongs to, for its materials and bone buffer kept across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    Static(u32),
    Fighter(u32, u32),
    Actor(u32, u32),
    /// A landscape component, by its place in the stage's landscape (actors' components in order).
    Landscape(u32),
    /// A part of the blast box's drawn edge.
    BlastBorder(u32),
    /// An effect's component mesh, by its slot (crate::cmesh).
    Component(u32),
    /// A mesh drawn into custom depth alone, by its place in `Renderer::custom_depth_only`.
    CustomDepth(u32),
    /// A mesh of the menus' level (its planes, banner pieces and glow shells), by its place in the frame.
    MenuMesh(u32),
    /// A preview's view's mesh, by the preview's fighter id and the mesh's place in its pose.
    MenuFighter(u32, u32),
}

/// Whose scene a frame's passes draw: the stage's (its lights, view members, post settings and scenery), or the menus'
/// level (crate::menuscene).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewOf {
    Stage,
    Menu,
}

/// A component mesh's own geometry on the GPU: its buffers and their vertex and index room.
/// A component mesh slot's own geometry on the GPU: its buffers, their vertex and index room, and whether they hold a
/// colour stream.
type OwnMesh = (LodBuffers, u32, u32, bool);

/// Marks a frame's GPU timer takes.
const TIMER_MARKS: u32 = 16;

/// The Graphics screen's scalability settings (mvs_settings::GraphicsSettings' three).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scalability {
    /// sg.AntiAliasingQuality, 0..=4.
    pub aa: u8,
    /// sg.ResolutionQuality: the 3D view's percent of the frame, 10..=100.
    pub resolution_quality: f64,
    /// sg.TextureQuality, 0..=4.
    pub texture_quality: u8,
}

impl Default for Scalability {
    /// The settings' defaults (mvs_settings::GraphicsSettings): the start values the renderer draws without a call.
    fn default() -> Scalability {
        Scalability { aa: crate::post::DEFAULT_AA_LEVEL, resolution_quality: 100.0, texture_quality: 3 }
    }
}

/// The view's size for a frame of `full` pixels at r.ScreenPercentage `percent` (clamped to 10..=100): 100 is the frame;
/// below it CeilToInt(size x f) per axis with f = percent x 0.01 in float (PrepareViewRectsForRendering 141cd1aa0;
/// the target sizes).
pub fn view_size(full: [u32; 2], percent: f64) -> [u32; 2] {
    let p = percent.clamp(10.0, 100.0);
    if p >= 100.0 {
        return full;
    }
    let f = (p as f32) * 0.01f32;
    full.map(|v| (((v as f32) * f).ceil() as u32).clamp(1, v))
}

#[derive(Debug)]
pub struct Renderer {
    pub(crate) gpu: Gpu,
    pub(crate) assets: Arc<Assets>,
    pub targets: SceneTargets,
    pub scene: SceneBuffers,
    pub materials: Materials,
    /// The View members the stage, the lighting and the post set.
    pub members: ViewMembers,
    /// The scene the light-map policy choice reads.
    pub lightmap_scene: LightMapScene,
    /// The stage's static meshes (set by the stage).
    pub statics: Vec<StaticMesh>,
    /// Meshes drawn into custom depth alone in the next `draw` (bRenderInMainPass off, each with its
    /// CustomDepthStencilValue), then emptied; a menu level's go through its frame (`MenuFrame::custom_depth`).
    pub custom_depth_only: Vec<MeshPose>,
    /// The frame has an item drawn into custom depth alone: its translucent passes read the custom stencil (cleared
    /// by `clear_items`).
    custom_depth_alone: bool,
    /// The stage's landscape (set by the stage).
    pub landscape: Option<Landscape>,
    /// The stage's Emitter actors (set by the stage), for `Fx::begin_level`.
    pub level_emitters: Vec<LevelEmitter>,
    /// The blast box's drawn edge (set by the stage, placed by `set_blast_box`).
    pub blast_border: Option<BlastBorder>,
    /// MPC TutorialEnvironmentDesat's Desaturation as the materials have it, and a value forced over the tutorial's
    /// (`force_tutorial_desaturation`: the pictures' hook).
    desaturation: f32,
    desaturation_forced: Option<f32>,
    /// The stage's scenery blocks and the state they change (set by the stage, stepped by `scenery_begin`,
    /// `scenery_tick` and `scenery_frame`).
    pub(crate) scenery: Option<SceneryRun>,
    /// The effects' floor decals of the frame (`Fx::decals`, set by the caller before `draw`).
    pub floor_decals: Vec<FloorDecal>,
    /// The effects' shown component meshes of the frame (`set_component_meshes`).
    components: Vec<(u32, u32, ComponentMesh)>,
    /// Per component mesh slot: the generation drawn last and the GPU mesh of an own geometry with its vertex and
    /// index room.
    component_gpu: HashMap<u32, (u32, Option<OwnMesh>)>,
    /// The stage's lights and sky (set by the stage, read by the lighting and the translucency).
    pub lights: LightSet,
    pub passes: Passes,
    /// The frame's size (the output's); the scene targets and the post chain run at the view's size inside it.
    pub(crate) output: [u32; 2],
    scalability: Scalability,
    captures: Captures,
    decals: Decals,
    lighting: Lighting,
    /// The menu levels' spot light shadow maps (lighting/shadow.rs).
    shadows: SpotShadows,
    translucency: Translucency,
    /// The translucency lighting volume (its static shadows set by the stage).
    pub(crate) tlv: Tlv,
    /// The menus' level behind their screens (`menu_*`).
    pub(crate) menu: crate::menuscene::MenuScenes,
    particles: Particles,
    /// The particle player's draws of the frame (kept between frames).
    emitter_draws: Vec<EmitterDraw>,
    post: Post,
    timer: GpuTimer,
    pub(crate) list: DrawList,
    primitives: PrimitiveRows,
    lightmaps: LightmapRows,
    view: Box<[[u32; 4]; VIEW_ROWS]>,
    /// Per (owner, slot): the material instance, the path it was made for and the names of the parameters the last
    /// MaterialRef set on it (`ref_names`).
    slots: HashMap<(Owner, u16), (Arc<str>, MaterialId, u64)>,
    bones: HashMap<Owner, Arc<Buffer>>,
    /// Per owner: its morph vertex buffer and what it was summed from (the mesh, the LOD and the weights).
    morphs: HashMap<Owner, MorphBuffer>,
    frame: u32,
    /// Buffers kept from frame to frame: the bytes written to a GPU buffer, the View members of the frame and a
    /// component mesh's pose.
    bytes: Vec<u8>,
    frame_members: ViewMembers,
    component_pose: MeshPose,
}

/// `n` material refs of a component mesh's one material and scalars, in `ms`'s room.
fn component_materials(ms: &mut Vec<MaterialRef>, n: usize, m: &ComponentMesh) {
    ms.truncate(n);
    ms.resize_with(n, MaterialRef::default);
    for r in ms {
        r.path.clone_from(&m.material);
        r.scalars.clone_from(&m.scalars);
        r.vectors.clear();
        r.textures.clear();
    }
}

/// A posed mesh's morph vertex buffer (FMorphVertexBuffer): per vertex of its LOD, FMorphGPUSkinVertex's
/// DeltaPosition and DeltaTangentZ (float3 each).
#[derive(Debug)]
struct MorphBuffer {
    buffer: Arc<Buffer>,
    mesh: String,
    lod: usize,
    weights: Vec<(Arc<str>, f32)>,
}

/// FMorphGPUSkinVertex's bytes.
const MORPH_STRIDE: usize = 24;
/// The weights a morph target is applied within (SkeletalRenderGPUSkin.cpp MinMorphTargetBlendWeight, SMALL_NUMBER,
/// and MaxMorphTargetBlendWeight).
// Unverified: 4.26's source, not read from the exe.
const MORPH_WEIGHTS: std::ops::RangeInclusive<f32> = 1e-8..=5.0;

/// The morph vertex buffer's values of `lod` with `weights` (FSkeletalMeshObjectGPUSkin::UpdateMorphVertexBuffer's
/// CPU sum): each active target's position and TangentZ deltas times its weight, the TangentZ sum divided by the
/// vertex's accumulated weight where that passes 1. None: no target of the mesh is active.
// Unverified: the CPU path of 4.26's source; the build's default on D3D11 (r.MorphTarget.Mode 1) sums on the GPU
// in fixed point, not read.
fn morph_deltas(mesh: &Mesh, lod: usize, weights: &[(Arc<str>, f32)]) -> Option<Vec<[f32; 6]>> {
    let n = mesh.lods.get(lod)?.vertices as usize;
    let mut out: Option<(Vec<[f32; 6]>, Vec<f32>)> = None;
    for (name, w) in weights {
        if !MORPH_WEIGHTS.contains(&w.abs()) {
            continue;
        }
        let Some(t) = mesh.morphs.iter().find(|m| *m.name == **name).and_then(|m| m.lods.get(lod)) else { continue };
        let (d, acc) = out.get_or_insert_with(|| (vec![[0.0; 6]; n], vec![0.0; n]));
        for (k, &v) in t.vertex.iter().enumerate() {
            let v = v as usize;
            for c in 0..3 {
                d[v][c] += t.position[k][c] * w;
                d[v][3 + c] += t.tangent_z[k][c] * w;
            }
            // Unverified: the accumulated weight is the absolute one (4.26's CPU morph update, MorphAbsWeight), not
            // read from the exe; no weight the port sets is negative.
            acc[v] += w.abs();
        }
    }
    let (mut d, acc) = out?;
    for (v, a) in d.iter_mut().zip(acc) {
        if a > 1.0 {
            for c in &mut v[3..] {
                *c /= a;
            }
        }
    }
    Some(d)
}

/// FMatrix3x4 (a row-vector matrix's three columns, each with its translation) as the row-vector M4.
pub fn matrix_of(m: &[f32; 12]) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for c in 0..3 {
        for row in 0..4 {
            r[row][c] = m[4 * c + row] as f64;
        }
    }
    r[3][3] = 1.0;
    r
}

/// A hash of the parameter names a MaterialRef sets (scalars, vectors, textures, in its order).
fn ref_names(r: &MaterialRef) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (n, _) in &r.scalars {
        n.hash(&mut h);
    }
    1u8.hash(&mut h);
    for (n, _) in &r.vectors {
        n.hash(&mut h);
    }
    2u8.hash(&mut h);
    for (n, _) in &r.textures {
        n.hash(&mut h);
    }
    h.finish() | 1
}

fn determinant3(m: &M4) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// A posed mesh's GPUScene rows: its transform and the bounds moved by it (the local box's centre transformed, its
/// extent by the absolute matrix).
fn primitive_of(mesh: &Mesh, m: &M4, channels: u8, receives_decals: bool, lightmap_index: u32) -> PrimitiveParams {
    let [o, e] = mesh.bounds.map(|v| v.map(|x| x as f64));
    let mut origin = [0.0; 3];
    let mut extent = [0.0; 3];
    for c in 0..3 {
        origin[c] = m[3][c] + (0..3).map(|r| o[r] * m[r][c]).sum::<f64>();
        extent[c] = (0..3).map(|r| (e[r] * m[r][c]).abs()).sum::<f64>();
    }
    let lo = [0, 1, 2].map(|i| mesh.bounds[0][i] - mesh.bounds[1][i]);
    let hi = [0, 1, 2].map(|i| mesh.bounds[0][i] + mesh.bounds[1][i]);
    PrimitiveParams {
        local_to_world: *m,
        previous_local_to_world: *m,
        actor_world_position: [m[3][0], m[3][1], m[3][2]],
        bounds_origin: origin,
        bounds_extent: extent,
        bounds_radius: (extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt(),
        local_bounds_min: lo,
        local_bounds_max: hi,
        lighting_channels: channels,
        receives_decals,
        use_single_sample_shadow: false,
        use_volumetric_lightmap_shadow: false,
        lightmap_index,
        lightmap_uv_index: mesh.lightmap_uv,
        custom_data: [[0.0; 4]; 9],
    }
}

/// A renderer's GPU parts, built from borrowed GPU and assets (the GPU moves in only once every part is made).
struct RendererParts {
    targets: SceneTargets,
    scene: SceneBuffers,
    timer: GpuTimer,
    captures: Captures,
    decals: Decals,
    lighting: Lighting,
    translucency: Translucency,
    tlv: Tlv,
    particles: Particles,
    post: Post,
    members: ViewMembers,
}

impl RendererParts {
    fn new(gpu: &Gpu, assets: &Assets, size: [u32; 2]) -> Result<RendererParts> {
        Ok(RendererParts {
            targets: SceneTargets::new(gpu, size)?,
            scene: SceneBuffers::new(gpu)?,
            timer: GpuTimer::new(gpu, TIMER_MARKS)?,
            captures: Captures::new(gpu)?,
            decals: Decals::new(gpu, assets)?,
            lighting: Lighting::new(gpu, assets)?,
            translucency: Translucency::new(gpu, assets)?,
            tlv: Tlv::new(gpu, assets)?,
            particles: Particles::new(gpu, assets)?,
            post: Post::new(gpu, assets, size, size)?,
            members: crate::view::system_members(gpu)?,
        })
    }
}
impl Renderer {
    /// A renderer drawing `size` pixels.
    pub fn new(gpu: Gpu, assets: Assets, size: [u32; 2]) -> Result<Renderer> {
        Renderer::try_new(gpu, assets, size).map_err(|b| b.0)
    }

    /// A renderer drawing `size` pixels; on a failure the GPU and the assets handed back with the error.
    pub fn try_new(
        gpu: Gpu,
        assets: Assets,
        size: [u32; 2],
    ) -> std::result::Result<Renderer, Box<(Error, Gpu, Assets)>> {
        match RendererParts::new(&gpu, &assets, size) {
            Ok(p) => Ok(Renderer::from_parts(gpu, assets, size, p)),
            Err(e) => Err(Box::new((e, gpu, assets))),
        }
    }

    fn from_parts(gpu: Gpu, assets: Assets, size: [u32; 2], p: RendererParts) -> Renderer {
        let RendererParts {
            targets,
            scene,
            timer,
            captures,
            decals,
            lighting,
            translucency,
            tlv,
            particles,
            post,
            members,
        } = p;
        Renderer {
            targets,
            scene,
            timer,
            lights: LightSet::default(),
            passes: Passes::default(),
            captures,
            decals,
            lighting,
            shadows: SpotShadows::default(),
            translucency,
            tlv,
            particles,
            emitter_draws: Vec::new(),
            post,
            output: size,
            menu: Default::default(),
            scalability: Scalability::default(),
            gpu,
            assets: Arc::new(assets),
            materials: Materials::default(),
            members,
            lightmap_scene: LightMapScene::STAGE,
            statics: Vec::new(),
            custom_depth_only: Vec::new(),
            custom_depth_alone: false,
            landscape: None,
            level_emitters: Vec::new(),
            blast_border: None,
            desaturation: 0.0,
            desaturation_forced: None,
            scenery: None,
            floor_decals: Vec::new(),
            components: Vec::new(),
            component_gpu: HashMap::new(),
            list: DrawList::default(),
            primitives: PrimitiveRows::default(),
            lightmaps: LightmapRows::default(),
            view: Box::new([[0; 4]; VIEW_ROWS]),
            slots: HashMap::new(),
            bones: HashMap::new(),
            morphs: HashMap::new(),
            frame: 0,
            bytes: Vec::new(),
            frame_members: ViewMembers::default(),
            component_pose: MeshPose::default(),
        }
    }

    /// The post chain's stage settings: its height fog, bloom intensity and post-process material blendables.
    pub(crate) fn set_post_stage(
        &mut self,
        data: &mvs_data::stage::StageData,
        draw: &crate::stagedraw::StageDraw,
    ) -> Result<()> {
        self.post.set_stage(&self.gpu, &self.assets, data, draw)
    }

    /// A material parameter collection's vectors set at run time (MPC_CentralPalette):
    /// every material instance reading the collection at `path` takes `rows`, those made later
    /// included.
    pub fn set_collection(&mut self, path: &str, rows: &[[f32; 4]]) {
        let bits: Vec<[u32; 4]> = rows.iter().map(|r| r.map(f32::to_bits)).collect();
        self.assets.set_collection(path, &bits);
        self.materials.set_collection(&self.gpu, path, rows);
        self.post.set_collection(&self.gpu, path, &bits);
    }

    /// SetScalarParameterValue(TutorialEnvironmentDesat, Desaturation, `v`):
    /// every material that reads the collection takes it, those made later included. A
    /// value forced by `force_tutorial_desaturation` wins.
    pub fn set_tutorial_desaturation(&mut self, v: f32) {
        let v = self.desaturation_forced.unwrap_or(v);
        if v.to_bits() == self.desaturation.to_bits() {
            return;
        }
        self.desaturation = v;
        self.set_collection(mvs_view::skins::TUTORIAL_DESAT, &[[v, 0.0, 0.0, 0.0]]);
    }

    /// A dev hook for pictures (render-frame `--desaturation`): the Desaturation every later
    /// `set_tutorial_desaturation` is replaced by; None gives the tutorial's back.
    pub fn force_tutorial_desaturation(&mut self, v: Option<f32>) {
        self.desaturation_forced = v;
        self.set_tutorial_desaturation(v.unwrap_or(0.0));
    }

    /// The decal pass's stage DecalActors.
    pub(crate) fn set_decal_stage(&mut self, decals: &[crate::scenery::StageDecal]) -> Result<()> {
        self.decals.set_stage(&self.gpu, &self.assets, decals)
    }

    /// The blast box as it is now (World.blast): the drawn edge follows it (the FreeForAll shrink, a reset world's
    /// box).
    pub fn set_blast_box(&mut self, b: &mvs_sim::world::BlastBounds) {
        if let Some(border) = &mut self.blast_border {
            border.place(b);
        }
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The renderer let go, its GPU kept (the store closed for an import that switches its packs).
    pub fn into_gpu(self) -> Gpu {
        self.gpu
    }

    /// The scalability settings applied (the Graphics page; Scalability.ini's sg groups): FXAA from anti-aliasing
    /// quality 1 up (its permutation by level), the 3D view drawn at `resolution_quality` percent of the frame and
    /// scaled up, the textures' anisotropy by texture quality (samplers made after the call).
    // Deviation: the texture quality changes the anisotropy only (texture_quality.rs): the pool is not
    // modelled, so no level cuts a mip.
    pub fn set_scalability(&mut self, s: Scalability) -> Result<()> {
        if s.aa > 4 || s.texture_quality > 4 || !(10.0..=100.0).contains(&s.resolution_quality) {
            return Err(Error::Format { what: "scalability".into(), why: format!("{s:?} out of range") });
        }
        if s.aa != self.scalability.aa {
            self.post.set_aa(&self.gpu, &self.assets, s.aa)?;
        }
        crate::texture_quality::set_level(s.texture_quality);
        self.scalability = s;
        self.fit()
    }

    /// The frame's size changed: the scene targets, the post chain, the lighting's and the translucency's targets
    /// made again at the view's size inside `size`.
    // (the lighting's and the particles' depth views are keyed by the depth texture and follow it; the translucency's
    // modulate target and the distortion targets follow the scene targets' size at their next draw)
    pub fn resize(&mut self, size: [u32; 2]) -> Result<()> {
        self.output = size.map(|v| v.max(1));
        self.fit()
    }

    /// The targets at the view's size for the output and the resolution quality, made again when either moved.
    fn fit(&mut self) -> Result<()> {
        let view = view_size(self.output, self.scalability.resolution_quality);
        if view == self.targets.size && self.output == self.post_full() {
            return Ok(());
        }
        if view != self.targets.size {
            self.targets = SceneTargets::new(&self.gpu, view)?;
        }
        self.post.resize(&self.gpu, &self.assets, view, self.output)?;
        Ok(())
    }

    fn post_full(&self) -> [u32; 2] {
        self.post.full()
    }

    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    /// The assets, shared with a loader thread's job (a match's HUD warm).
    pub fn assets_shared(&self) -> Arc<Assets> {
        self.assets.clone()
    }

    /// The particle pass's materials and mesh streams made so far (crate::warm's census).
    pub(crate) fn particles_counts(&self) -> [usize; 2] {
        self.particles.counts()
    }

    /// The decal pass's warm-up of the floor decals' materials `names` (crate::warm): the lines of what failed.
    pub(crate) fn warm_decals(&mut self, names: &[String]) -> Vec<String> {
        self.decals.warm(&self.gpu, &self.assets, names)
    }

    /// The decal pass's material names, if made (a loader's warm-up job takes them).
    pub(crate) fn decal_names(&self) -> Option<Arc<HashMap<String, String>>> {
        self.decals.names()
    }

    /// What a loader's warm-up made for the particle and decal passes, taken in (crate::warm).
    /// Lets go the particle and decal materials and the mesh streams nothing has used since the last call (a
    /// fighter's or stage's effects no longer played), before the assets' trim, which can then let their meshes
    /// and textures go: at a match's install, after its warm-up was taken in. How many of each went.
    pub fn trim_effects(&mut self) -> [usize; 3] {
        let [materials, meshes] = self.particles.trim();
        [materials, meshes, self.decals.trim()]
    }

    pub(crate) fn adopt_warm(&mut self, p: crate::particles::ParticlesMade, d: crate::decal::DecalsMade) {
        self.particles.adopt(p);
        self.decals.adopt(d);
    }

    /// The particle pass's warm-up of `emitters` (crate::warm): the lines of what failed.
    pub(crate) fn warm_particles(&mut self, emitters: &[(Arc<str>, mvs_vfx::draw::EmitterKind)]) -> Vec<String> {
        self.particles.warm(&self.gpu, &self.assets, emitters)
    }

    /// The material instance of a mesh slot, made on first use and when its material changes; the parameters set
    /// on it each frame.
    /// `inst` as `owner`'s material: in the id the owner had from an earlier stage, else a new one, so a stage set
    /// again does not grow the instances.
    pub(crate) fn keep_material(&mut self, owner: Owner, inst: MaterialInstance) -> MaterialId {
        if let Some((_, id, _)) = self.slots.get(&(owner, 0)) {
            let id = *id;
            self.materials.instances[id.0 as usize] = inst;
            return id;
        }
        let id = self.materials.add(inst);
        self.slots.insert((owner, 0), (Arc::from(""), id, 0));
        id
    }

    fn material(&mut self, owner: Owner, slot: u16, r: Option<&MaterialRef>, mesh: &Mesh) -> Result<MaterialId> {
        let path: Arc<str> = match r {
            Some(r) if !r.path.is_empty() => r.path.clone(),
            _ => mesh.materials.get(slot as usize).cloned().unwrap_or_else(|| Arc::from("")),
        };
        if path.is_empty() {
            return Err(Error::Format { what: mesh.path.clone(), why: format!("slot {slot} has no material") });
        }
        // An owner slot may pass from one mesh to another of the same material (a menu frame's meshes are owned by
        // their place in its list): the values the last MaterialRef set and this one does not are the material's own
        // again, so a banner's mask copy does not keep the trim's StencilValue and UseTrim.
        let names = r.map_or(0, ref_names);
        let id = match self.slots.get_mut(&(owner, slot)) {
            Some((p, id, n)) if *p == path => {
                if *n != names {
                    *n = names;
                    self.materials.instances[id.0 as usize].reset_parameters(&self.assets)?;
                }
                *id
            }
            Some((_, id, _)) => {
                let id = *id;
                self.materials.instances[id.0 as usize] = MaterialInstance::new(&self.gpu, &self.assets, &path)?;
                self.slots.insert((owner, slot), (path, id, names));
                id
            }
            None => {
                let inst = MaterialInstance::new(&self.gpu, &self.assets, &path)?;
                let id = self.materials.add(inst);
                self.slots.insert((owner, slot), (path, id, names));
                id
            }
        };
        if let Some(r) = r {
            let inst = self.materials.get_mut(id);
            for (n, v) in &r.scalars {
                inst.set_scalar(n, *v);
            }
            for (n, v) in &r.vectors {
                inst.set_vector(n, *v);
            }
            for (n, t) in &r.textures {
                // A render target the game fills: PFGCamera's priority captures; a tape's `<runtime>` (a texture
                // made at run time) keeps the material's own.
                let source = match mvs_view::pose::PRIORITY_CAPTURES.iter().position(|p| **p == **t) {
                    Some(k) => TextureSource::Target(TargetId::PriorityCapture(k as u8 + 1)),
                    None if &**t == "<runtime>" => continue,
                    None if t.starts_with('<') => {
                        return Err(Error::Format {
                            what: mesh.path.clone(),
                            why: format!("{n}: no render target {t}"),
                        });
                    }
                    None => TextureSource::Asset(self.assets.texture(t)?),
                };
                inst.set_texture(n, source);
            }
        }
        Ok(id)
    }

    /// A posed mesh's draw items (a section a material slot; the shown ones), its primitive and lightmap rows;
    /// `instances`: an instanced static mesh component's (drawn with FInstancedStaticMeshVertexFactory).
    pub(crate) fn add_pose(
        &mut self,
        owner: Owner,
        p: &MeshPose,
        lightmap: Option<&StaticLightmap>,
        channels: u8,
        receives_decals: bool,
        instances: Option<&Arc<IsmInstances>>,
    ) -> Result<()> {
        let mesh = self.assets.mesh(&p.mesh)?;
        self.add_mesh_pose(owner, mesh, p, lightmap, channels, receives_decals, instances)
    }

    /// The owner's morph vertex buffer for `weights` on `mesh`'s LOD `lod`, summed again when they changed; None when
    /// no target of the mesh is active.
    fn morph_buffer(
        &mut self,
        owner: Owner,
        mesh: &Mesh,
        lod: usize,
        weights: &[(Arc<str>, f32)],
    ) -> Result<Option<Arc<Buffer>>> {
        if weights.is_empty() || mesh.morphs.is_empty() {
            return Ok(None);
        }
        if let Some(m) = self.morphs.get(&owner)
            && m.mesh == mesh.path
            && m.lod == lod
            && m.weights == weights
        {
            return Ok(Some(m.buffer.clone()));
        }
        let Some(d) = morph_deltas(mesh, lod, weights) else { return Ok(None) };
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.clear();
        bytes.extend(d.iter().flatten().flat_map(|f| f.to_le_bytes()));
        debug_assert_eq!(bytes.len(), d.len() * MORPH_STRIDE);
        let size = bytes.len();
        let buffer = match self.morphs.get(&owner) {
            Some(m) if m.buffer.desc.size as usize == size => m.buffer.clone(),
            _ => match self.gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: size as u32, dynamic: true }, None) {
                Ok(b) => Arc::new(b),
                Err(e) => {
                    self.bytes = bytes;
                    return Err(e);
                }
            },
        };
        self.gpu.write(&buffer, &bytes);
        self.bytes = bytes;
        match self.morphs.get_mut(&owner) {
            Some(m) => {
                m.buffer = buffer.clone();
                m.mesh.clone_from(&mesh.path);
                m.lod = lod;
                m.weights.clear();
                m.weights.extend_from_slice(weights);
            }
            None => {
                let m = MorphBuffer { buffer: buffer.clone(), mesh: mesh.path.clone(), lod, weights: weights.to_vec() };
                self.morphs.insert(owner, m);
            }
        }
        Ok(Some(buffer))
    }

    /// The opaque items from `from` on cast the menu levels' spot shadows (lighting/shadow.rs): each section with
    /// bCastShadow draws in the shadow depth pass where its material has one, a masked material's own shaders, an
    /// opaque one's the default material's. AddSubjectPrimitive 141d41650 takes opaque and masked relevance only (the
    /// translucent sections cast none here); the light's interaction (lighting channels, bounds) is tested per light
    /// in SpotShadows::draw.
    // The interaction's HasShadow (flags +0x34 bit 0, ctor 141cb0c70) for a primitive without static lighting (a
    // skeletal mesh) is the light's CastShadows && CastDynamicShadows (the rig's SpotLight::cast_shadows) and the
    // primitive's CastShadow && bCastDynamicShadow; bit 7 (bSelfShadowOnly) keeps it out, and IsShadowCast 142cfd410
    // takes a hidden primitive only with bCastHiddenShadow. BP_CharacterPreview's skeletal mesh components store none
    // of CastShadow, bCastDynamicShadow, bSelfShadowOnly or bCastHiddenShadow (true, true, false, false) and nothing
    // in its graph sets them, so each shown preview's meshes cast; the caller adds only the shown previews.
    // Unverified: the per-section bCastShadow filter is 4.26 source (the mesh proxy's section draw), not read in the exe.
    pub(crate) fn cast_shadows_from(&mut self, from: usize) {
        for it in &mut self.list.opaque[from..] {
            if let Some(s) = &it.mesh
                && !s.mesh.lods[s.lod as usize].sections[s.section as usize].cast_shadow
            {
                continue;
            }
            let inst = &self.materials.instances[it.material.0 as usize];
            if inst.has_shadow_depth_pass(&self.assets, it.vf) {
                it.passes = it.passes.with(PassKind::ShadowDepth);
            } else {
                crate::fx::warn_once(format!("{}: no shadow depth pass for {:?}", inst.material.path, it.vf));
            }
        }
    }

    /// `add_pose` with the mesh given (`p.mesh` not read).
    #[allow(clippy::too_many_arguments)]
    fn add_mesh_pose(
        &mut self,
        owner: Owner,
        mesh: Arc<Mesh>,
        p: &MeshPose,
        lightmap: Option<&StaticLightmap>,
        channels: u8,
        receives_decals: bool,
        instances: Option<&Arc<IsmInstances>>,
    ) -> Result<()> {
        let lod = (p.lod as usize).min(mesh.lods.len().saturating_sub(1));
        let m = matrix_of(&p.to_world);
        let skinned = !p.bones.is_empty();
        let vf_input = if skinned {
            if p.bones.len() != mesh.bones as usize {
                return Err(Error::Format {
                    what: mesh.path.clone(),
                    why: format!("{} bones posed for {}", p.bones.len(), mesh.bones),
                });
            }
            let size = p.bones.len() * 48;
            let b = match self.bones.get(&owner) {
                Some(b) if b.desc.size as usize == size => b.clone(),
                _ => {
                    let b = Arc::new(self.gpu.buffer(
                        &BufDesc {
                            kind: BufKind::Typed { format: DXGI_FORMAT_R32G32B32A32_FLOAT },
                            size: size as u32,
                            dynamic: true,
                        },
                        None,
                    )?);
                    self.bones.insert(owner, b.clone());
                    b
                }
            };
            self.bytes.clear();
            self.bytes.extend(p.bones.iter().flatten().flat_map(|f| f.to_le_bytes()));
            self.gpu.write(&b, &self.bytes);
            VfInput::Bones { bones: b, morph: self.morph_buffer(owner, &mesh, lod, &p.morphs)? }
        } else if let Some(i) = instances {
            VfInput::StaticInstances(i.clone())
        } else {
            VfInput::None
        };
        let morphed = matches!(&vf_input, VfInput::Bones { morph: Some(_), .. });
        let vf = if skinned {
            VertexFactory::GpuSkin { extra_influences: mesh.extra_influences, morph: false }
        } else if instances.is_some() {
            VertexFactory::InstancedStatic
        } else {
            VertexFactory::Local
        };
        let instance_count = instances.map_or(1, |i| i.count);
        let lightmap_ref =
            lightmap.map(|l| LightmapRef { row: self.lightmaps.push(&l.params), textures: l.textures.clone() });
        let primitive = self.primitives.push(&primitive_of(
            &mesh,
            &m,
            channels,
            receives_decals,
            lightmap_ref.as_ref().map_or(0, |l| l.row),
        ));
        let mirrored = determinant3(&m) < 0.0;
        for (k, s) in mesh.lods[lod].sections.iter().enumerate() {
            if !p.shown.get(k).copied().unwrap_or(true) || s.triangles == 0 {
                continue;
            }
            let material = self.material(owner, s.material, p.materials.get(s.material as usize), &mesh)?;
            // FSkeletalMeshObjectGPUSkin::GetSkinVertexFactory: the morph factory while a target is active, for a
            // section whose material has its shaders.
            // Unverified: a section whose material has no TGPUSkinMorphVertexFactory map (Velma_PoliceCarWindow_MAT on
            // the police car) is drawn unmorphed with the plain factory; the build's proxy may give it the default
            // material or leave it undrawn (not read), and its vertices move with the rest.
            let vf = match vf {
                VertexFactory::GpuSkin { extra_influences, .. } if morphed => {
                    let map =
                        mvs_material::hashed_name(VertexFactory::GpuSkin { extra_influences, morph: true }.type_name());
                    let set = &self.materials.instances[material.0 as usize].material.shaders;
                    let has = set.rows.iter().any(|r| r.map == map);
                    VertexFactory::GpuSkin { extra_influences, morph: has }
                }
                vf => vf,
            };
            let st = &self.materials.instances[material.0 as usize].material.state;
            let translucent = st.blend != "BLEND_Opaque" && st.blend != "BLEND_Masked";
            // FCustomDepthPassMeshProcessor: a translucent section writes custom depth only with
            // bAllowCustomDepthWrites (the armor copies get none).
            let custom_depth = p.custom_stencil > 0 && (!translucent || st.allow_translucent_custom_depth_writes);
            // A skinned or movable mesh has no light-cache interface's
            // lightmap (the volumetric policy); a lightmapped static one takes the lightmap.
            let surface = LightMapSurface {
                lit: st.shading_model != "MSM_Unlit",
                translucent,
                light_cache: !skinned,
                cast_volumetric_translucent_shadow: false,
                texture_light_map: lightmap.is_some(),
                hq_interaction: true,
                texture_shadow_map: lightmap.is_some_and(|l| l.shadow_map),
                mobility: if lightmap.is_some() { Mobility::Static } else { Mobility::Movable },
                preview_or_forced_volumetric: false,
                indirect_cache: None,
            };
            let policy = light_map_policy(&surface, &self.lightmap_scene);
            let mut passes = PassMask::of(&[if translucent { PassKind::Translucent } else { PassKind::Base }]);
            if custom_depth {
                passes = passes.with(PassKind::CustomDepth);
            }
            let item = DrawItem {
                mesh: Some(MeshSlice { mesh: mesh.clone(), lod: lod as u8, section: k as u16 }),
                vf,
                vf_input: vf_input.clone(),
                material,
                policy,
                primitive,
                lightmap: lightmap_ref.clone(),
                passes,
                custom_stencil: p.custom_stencil,
                lighting_channels: channels | if receives_decals { crate::decal::RECEIVES_DECALS_BIT } else { 0 },
                mirrored,
                sort_key: 0.0,
                after_meshes: false,
                instances: instance_count,
            };
            if custom_depth {
                self.list.custom_depth.push(item.clone());
            }
            if translucent {
                self.list.translucent.push(item);
            } else {
                self.list.opaque.push(item);
            }
        }
        Ok(())
    }

    /// A pose into the custom depth list alone (bRenderInMainPass off; a section that writes no custom depth draws
    /// nothing); the frame's translucent passes then read the custom stencil (`custom_depth_alone`).
    pub(crate) fn add_custom_depth_pose(&mut self, owner: Owner, p: &MeshPose) -> Result<()> {
        let (opaque, translucent, custom) =
            (self.list.opaque.len(), self.list.translucent.len(), self.list.custom_depth.len());
        self.add_pose(owner, p, None, 1, false, None)?;
        self.list.opaque.truncate(opaque);
        self.list.translucent.truncate(translucent);
        self.custom_depth_alone |= self.list.custom_depth.len() > custom;
        Ok(())
    }

    /// The effects' component meshes to draw in the next frames (`Fx::components`, set by the caller before `draw`):
    /// the shown ones are kept; an own geometry's GPU buffers are written at the draw.
    pub fn set_component_meshes(&mut self, c: &ComponentMeshes) {
        let mut n = 0;
        for (id, m) in c.iter().filter(|(_, m)| m.shown) {
            match self.components.get_mut(n) {
                Some(e) => {
                    (e.0, e.1) = (id.slot, id.age);
                    e.2.clone_from(m);
                }
                None => self.components.push((id.slot, id.age, m.clone())),
            }
            n += 1;
        }
        self.components.truncate(n);
    }

    /// The frame's component meshes' draw items (after the fighters'): a slot taken by a new mesh makes its material
    /// instance anew; an own geometry goes to the slot's dynamic buffers (made again when it outgrows them).
    fn add_components(&mut self) -> Result<()> {
        let components = std::mem::take(&mut self.components);
        let mut p = std::mem::take(&mut self.component_pose);
        let r = (|| {
            for (slot, age, m) in &components {
                let owner = Owner::Component(*slot);
                let kept = self.component_gpu.entry(*slot).or_insert((*age, None));
                if kept.0 != *age {
                    *kept = (*age, None);
                    for ((o, _), s) in self.slots.iter_mut() {
                        if *o == owner {
                            s.0 = Arc::from("");
                        }
                    }
                }
                let from = self.list.translucent.len();
                // (the pose: the component's transform and its one material on every slot; add_mesh_pose reads no
                // mesh path)
                match &m.geometry {
                    CMeshGeometry::Static { mesh, to_world } => {
                        let mesh = self.assets.mesh(mesh)?;
                        p.to_world = *to_world;
                        component_materials(&mut p.materials, mesh.materials.len().max(1), m);
                        self.add_mesh_pose(owner, mesh, &p, None, 1, false, None)?;
                    }
                    CMeshGeometry::Own(g) => {
                        if g.indices.is_empty() || g.positions.is_empty() {
                            continue;
                        }
                        let mesh = self.own_mesh(*slot, &m.material, g)?;
                        p.to_world = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
                        component_materials(&mut p.materials, 1, m);
                        self.add_mesh_pose(owner, mesh, &p, None, 1, false, None)?;
                    }
                }
                // (render order 999: after the translucent meshes of its priority; cmesh.rs)
                for it in &mut self.list.translucent[from..] {
                    it.sort_key = m.sort_priority.unwrap_or(0) as f32;
                    it.after_meshes = !m.by_depth;
                }
            }
            Ok(())
        })();
        self.components = components;
        self.component_pose = p;
        r
    }

    /// A component mesh's own geometry on the GPU (FLocalVertexFactory's streams: the positions, the UVs, the packed
    /// tangents, the colours; UE world, so the identity transform), its bounds the vertices'. Without tangents the
    /// stream is zeros; without colours every vertex reads the one white colour (ColorIndexMask 0).
    // Unverified: zero tangents and the one white colour for a cable and a spline mesh of no colour stream (the
    // geometry is position, uv, colour 1 only); the build's cable component builds its own.
    fn own_mesh(&mut self, slot: u32, material: &Arc<str>, g: &OwnGeometry) -> Result<Arc<Mesh>> {
        let (positions, uvs, indices) = (&g.positions, &g.uvs, &g.indices);
        let (nv, ni) = (positions.len() as u32, indices.len() as u32);
        let colored = g.colors.len() == positions.len();
        let kept = &mut self.component_gpu.get_mut(&slot).expect("the slot's record").1;
        if !matches!(kept, Some((_, v, i, c)) if *v >= nv && *i >= ni && *c == colored) {
            let (vcap, icap) = (nv.next_power_of_two().max(64), ni.next_power_of_two().max(192));
            let gpu = &self.gpu;
            let dynamic = |kind: BufKind, size: u32| gpu.buffer(&BufDesc { kind, size, dynamic: true }, None);
            let color = if colored {
                dynamic(BufKind::Typed { format: DXGI_FORMAT_R8G8B8A8_UNORM }, vcap * 4)?
            } else {
                gpu.buffer(
                    &BufDesc { kind: BufKind::Typed { format: DXGI_FORMAT_R8G8B8A8_UNORM }, size: 4, dynamic: false },
                    Some(&[255; 4]),
                )?
            };
            // (ColorIndexMask: every vertex its own colour, else the one white colour; one UV)
            let mask = if colored { u32::MAX } else { 0 };
            let buffers = LodBuffers {
                position: dynamic(BufKind::Vertex, vcap * 12)?,
                index: dynamic(BufKind::Index32, icap * 4)?,
                tex_coords: Some(dynamic(BufKind::Typed { format: DXGI_FORMAT_R32G32_FLOAT }, vcap * 8)?),
                num_tex_coords: 1,
                tangents: Some(dynamic(BufKind::Typed { format: DXGI_FORMAT_R8G8B8A8_SNORM }, vcap * 8)?),
                color: Some(color),
                local_vf: Some(gpu.buffer(
                    &BufDesc { kind: BufKind::Constant, size: 32, dynamic: false },
                    Some(crate::gpu::rows_bytes(&[[mask, 1, 0, 0], [0; 4]])),
                )?),
                skin: Vec::new(),
            };
            *kept = Some((buffers, vcap, icap, colored));
        }
        let b = &kept.as_ref().expect("made above").0;
        // VertexFetch_PackedTangentsBuffer: TangentX, TangentZ per vertex (FPackedNormal: x 127, rounded)
        let snorm = |v: f32| ((v.clamp(-1.0, 1.0) * 127.0).round() as i8).cast_unsigned();
        let v = &mut self.bytes;
        v.clear();
        v.resize(positions.len() * 8, 0);
        if g.tangent_x.len() == positions.len() && g.tangent_z.len() == positions.len() {
            for (k, t) in v.as_chunks_mut::<8>().0.iter_mut().enumerate() {
                let (x, z) = (g.tangent_x[k], g.tangent_z[k]);
                *t = [x[0], x[1], x[2], 0.0, z[0], z[1], z[2], z[3]].map(snorm);
            }
        }
        self.gpu.write(b.tangents.as_ref().expect("made above"), v);
        if colored {
            self.gpu.write(b.color.as_ref().expect("made above"), g.colors.as_flattened());
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in positions {
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
        v.clear();
        v.extend(positions.iter().flatten().flat_map(|f| f.to_le_bytes()));
        self.gpu.write(&b.position, v);
        v.clear();
        for k in 0..positions.len() {
            let t = uvs.get(k).copied().unwrap_or([0.0; 2]);
            v.extend_from_slice(&t[0].to_le_bytes());
            v.extend_from_slice(&t[1].to_le_bytes());
        }
        self.gpu.write(b.tex_coords.as_ref().expect("made above"), v);
        v.clear();
        v.extend(indices.iter().flat_map(|i| i.to_le_bytes()));
        self.gpu.write(&b.index, v);
        let sections = vec![MeshSection { material: 0, first_index: 0, triangles: ni / 3, cast_shadow: true }];
        Ok(Arc::new(Mesh {
            path: format!("<component mesh {slot}>"),
            lods: vec![MeshLod { sections, vertices: nv, buffers: b.clone() }],
            materials: vec![material.clone()],
            bones: 0,
            extra_influences: false,
            bounds: [[0, 1, 2].map(|c| (lo[c] + hi[c]) * 0.5), [0, 1, 2].map(|c| (hi[c] - lo[c]) * 0.5)],
            lightmap_uv: 0,
            morphs: Vec::new(),
        }))
    }

    /// Draws `f` with `vfx`'s emitters (its draws from the camera at the frame's alpha); the final picture goes to
    /// `out` (an RGBA8 target of the renderer's size) when given and the post pass is on.
    // The pass order: the priority captures, the base pass into the G-buffer (the opaque and masked particles last in
    // it), custom depth, the deferred decals, the deferred lighting into SceneColor, the fog (height and atmospheric),
    // the scene colour copy and the translucent meshes, the translucent particles, the translucent meshes of a sort
    // priority above 0 (the blast border's frame), the distortion, the separate translucency composite, the post chain
    // to `out`. Each pass besides the base pass runs when `self.passes` turns it on. The UI is not the renderer's: the caller draws it over `out` after.
    // Unverified: the translucent meshes and the particles drawn as groups, each sorted in itself, where the build
    // sorts every translucent primitive together.
    pub fn draw(
        &mut self,
        f: &FrameInput,
        vfx: Option<&mut mvs_vfx::Player>,
        out: Option<&Texture>,
    ) -> Result<FrameTimes> {
        let start = Instant::now();
        self.clear_items();
        let mut statics = std::mem::take(&mut self.statics);
        // (the scenery's meshes as its blocks and animations left them)
        if let Some(s) = &self.scenery {
            s.apply(&mut statics);
        }
        let r = (|| {
            for (i, s) in statics.iter().enumerate() {
                // (a hidden one: none of its sections shown)
                if !s.pose.shown.is_empty() && !s.pose.shown.contains(&true) {
                    continue;
                }
                let instances = s.instances.as_ref();
                let owner = Owner::Static(i as u32);
                let from = self.list.translucent.len();
                self.add_pose(owner, &s.pose, s.lightmap.as_ref(), s.channels, s.receives_decals, instances)?;
                // Its translucent sections take its TranslucencySortPriority (above 0 they draw after the
                // particles, as the blast border's frame; no scenery instance of the build has one).
                for it in &mut self.list.translucent[from..] {
                    it.sort_key = s.sort;
                }
            }
            Ok::<(), Error>(())
        })();
        self.statics = statics;
        r?;
        // The blast box's edge: its translucent sections take the part's TranslucencySortPriority.
        if let Some(border) = self.blast_border.take() {
            let r = (|| {
                for (k, (pose, sort)) in border.parts.iter().enumerate().filter(|_| border.placed()) {
                    let from = self.list.translucent.len();
                    self.add_pose(Owner::BlastBorder(k as u32), pose, None, 1, true, None)?;
                    for it in &mut self.list.translucent[from..] {
                        it.sort_key = *sort;
                    }
                }
                Ok::<(), Error>(())
            })();
            self.blast_border = Some(border);
            r?;
        }
        if let Some(l) = &mut self.landscape {
            l.add_items(&self.gpu, f.camera, &mut self.list, &mut self.primitives, &mut self.lightmaps)?;
        }
        self.add_fighters(f)?;
        self.add_components()?;
        let poses = std::mem::take(&mut self.custom_depth_only);
        let r = (|| {
            for (i, p) in poses.iter().enumerate() {
                self.add_custom_depth_pose(Owner::CustomDepth(i as u32), p)?;
            }
            Ok::<(), Error>(())
        })();
        r?;
        self.draw_items(f, vfx, out, start, ViewOf::Stage)
    }

    /// The frame's draw list, primitive and lightmap rows emptied (a frame's first step).
    pub(crate) fn clear_items(&mut self) {
        self.list.clear();
        self.primitives.0.clear();
        self.lightmaps.0.clear();
        self.custom_depth_alone = false;
    }

    /// The passes over the frame's items (`draw` from its base pass on): `of` names the scene whose lights, View
    /// members, post settings and scenery widgets they read; `start` is when the frame's CPU time began.
    pub(crate) fn draw_items(
        &mut self,
        f: &FrameInput,
        vfx: Option<&mut mvs_vfx::Player>,
        out: Option<&Texture>,
        start: Instant,
        of: ViewOf,
    ) -> Result<FrameTimes> {
        let menu = of == ViewOf::Menu;
        let passes = if menu { crate::menuscene::MENU_PASSES } else { self.passes };
        if passes.particles {
            // (the player takes last frame's draws back as its spare buffers: no clear before it)
            if let Some(player) = vfx {
                let cam = f.camera;
                player.set_view(cam.origin, [0, 1, 2].map(|i| cam.translated_world_to_view[i][2]));
                // (the beams fall back on the camera's up axis, its view Y)
                player.set_camera_up([0, 1, 2].map(|i| cam.translated_world_to_view[i][1]));
                // (the Collision modules' LOD factor: the horizontal FOV / 90; ViewToClip[0][0] = 1 / tan(FOV / 2))
                player.set_lod_factor(2.0 * (1.0 / cam.view_to_clip[0][0]).atan().to_degrees() / 90.0);
                player.draws(f.camera.origin, f.alpha, &mut self.emitter_draws);
                self.particles.set_params(player.draw_params());
            } else {
                self.emitter_draws.clear();
                self.particles.set_params(&[]);
            }
            self.particles.prepare(&self.gpu, &self.assets, &self.emitter_draws)?;
        }
        if self.lightmaps.0.is_empty() {
            self.lightmaps.push(&LightmapParams::default());
        }
        debug_assert!(self.lightmaps.0.iter().all(|l| l.len() == LIGHTMAP_ROWS));
        let size = self.targets.size;
        let mut members = std::mem::take(&mut self.frame_members);
        if menu {
            members = self.menu.members(&self.members);
        } else {
            members.rows.clone_from(&self.members.rows);
            members.textures.clone_from(&self.members.textures);
        }
        self.tlv.set_view(f.camera, &mut members);
        let params = ViewParams {
            camera: f.camera.clone(),
            rect: [0, 0, size[0], size[1]],
            buffer: size,
            time: f.time,
            frame: self.frame,
            members,
        };
        view_rows(&params, &mut self.view);
        self.scene.view_textures.clone_from(&params.members.textures);
        self.frame_members = params.members;
        self.scene.upload(&self.gpu, &self.view, &self.primitives, &self.lightmaps)?;

        let gpu = &self.gpu;
        self.timer.begin_frame(gpu);
        // FogStruct's rows of the stage's height fog for this camera (the translucent passes' and the particles'); the
        // menus' levels have no fog actor.
        let fog = if menu { None } else { self.post.fog_rows(f.camera.origin[2]) };
        let lights = if menu { &self.menu.lights } else { &self.lights };
        let mut fr = Frame {
            gpu,
            assets: &self.assets,
            scene: &self.scene,
            targets: &self.targets,
            materials: &mut self.materials,
            list: &self.list,
            primitives: &self.primitives,
            lights,
            input: f,
        };
        if passes.captures {
            self.captures.draw(&mut fr, &self.view)?;
            self.timer.mark(gpu, "captures");
        }
        let t = &self.targets;
        let g = &t.gbuffer;
        let rtvs: [Option<ID3D11RenderTargetView>; 6] =
            [&t.scene_color, &g[0], &g[1], &g[2], &g[3], &g[4]].map(|x| x.rtv.clone());
        // SAFETY: views of this device's targets; the clear colours are four floats.
        unsafe {
            for r in rtvs.iter().flatten() {
                gpu.ctx.ClearRenderTargetView(r, &[0.0; 4]);
            }
            let dsv = t.depth.dsv.as_ref().expect("a depth view");
            gpu.ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0, 0.0, 0);
            gpu.ctx.OMSetRenderTargets(Some(&rtvs), dsv);
        }
        gpu.viewport([0, 0, size[0], size[1]]);
        execute(gpu, &self.assets, &self.scene, t, fr.materials, &self.list.opaque, PassKind::Base, None)?;
        // (the opaque and masked particles in the base pass, on the targets it bound)
        if passes.particles {
            self.particles.draw_opaque(&mut fr)?;
        }
        self.timer.mark(gpu, "base pass");
        // SAFETY: unbinds the targets.
        unsafe { gpu.ctx.OMSetRenderTargets(None, None) };
        if passes.custom_depth {
            self.post.custom_depth(&mut fr)?;
            self.timer.mark(gpu, "custom depth");
        }
        if passes.decals {
            self.decals.draw(&mut fr, &self.floor_decals)?;
            self.timer.mark(gpu, "decals");
        }
        if passes.lighting {
            if menu {
                self.shadows.draw(&mut fr)?;
                self.timer.mark(gpu, "spot shadows");
            } else {
                self.shadows.release();
            }
            self.lighting.draw(&mut fr, Some(&self.shadows))?;
            self.timer.mark(gpu, "lighting");
        }
        if passes.fog {
            self.post.fog(&mut fr, &self.view)?;
            self.timer.mark(gpu, "fog");
        }
        let stencil = if passes.custom_depth && self.custom_depth_alone {
            Some(self.post.stencil_srv(gpu, &self.targets)?)
        } else {
            None
        };
        self.translucency.set_custom_stencil(stencil);
        if passes.translucency || passes.particles {
            let shadows = if menu { self.menu.tlv_shadows() } else { crate::lighting::tlv::TlvShadows::Stage };
            self.tlv.draw(&fr, shadows)?;
            self.translucency.set_volume(self.tlv.srvs());
            self.timer.mark(gpu, "translucency volume");
        }
        if passes.translucency {
            let stage_quads;
            let quads: &[crate::scenery::WidgetQuad] = match &self.scenery {
                _ if menu => &self.menu.quads,
                Some(s) => {
                    stage_quads = crate::scenery::widget::scenery_quads(&s.state.widgets);
                    &stage_quads
                }
                None => &[],
            };
            let cam = f.camera;
            let z = |p: [f64; 3]| crate::translucency::view_z(&cam.translated_world_to_view, cam.origin, p);
            self.translucency.widgets.prepare(gpu, quads, cam, z)?;
            self.translucency.draw(&mut fr, fog.as_deref())?;
            self.timer.mark(gpu, "translucency");
        }
        if passes.particles {
            // (without the separate translucency composite, its items go straight into SceneColor; the
            // TranslucentBasePass they bind is the translucent passes', filled here when those do not run)
            if passes.translucency {
                self.particles.draw(&mut fr, self.translucency.pass_input())?;
            } else {
                self.translucency.fill_pass(&fr, fog.as_deref())?;
                self.particles.draw_scene_color(&mut fr, self.translucency.pass_input())?;
            }
        }
        // The translucent meshes sorted after the particles (a TranslucencySortPriority above theirs).
        if passes.translucency {
            self.translucency.draw_late(&mut fr)?;
        }
        if passes.particles {
            self.particles.distortion(&mut fr)?;
            self.timer.mark(gpu, "particles");
        }
        if passes.translucency {
            self.translucency.composite(&mut fr)?;
            self.timer.mark(gpu, "translucency composite");
        }
        if passes.post {
            let view = if menu { self.menu.post_view() } else { crate::post::PostView::STAGE };
            self.post.draw(&mut fr, out, &view)?;
            self.timer.mark(gpu, "post");
        }
        self.timer.end_frame(gpu);
        self.frame = self.frame.wrapping_add(1);
        let cpu = start.elapsed().as_secs_f64() * 1e3;
        let mut times = FrameTimes { cpu, gpu: None, passes: Vec::new() };
        if let Some(p) = self.timer.read(gpu) {
            times.gpu = Some(p.iter().map(|x| x.1).sum());
            times.passes = p;
        }
        Ok(times)
    }

    /// `t`'s pixels (a 2D texture of an uncompressed format, its texels' bytes as stored), rows top first.
    pub fn read_back(&self, t: &Texture) -> Result<Vec<u8>> {
        self.gpu.read_texture(t)
    }

    /// The items of the last frame drawn.
    pub fn draw_list(&self) -> &DrawList {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_of_takes_columns() {
        // The 3x4's columns: x' = p.x + 10, y' = -p.y + 20, z' = p.z + 30 (a mirror in Y).
        let m = matrix_of(&[1.0, 0.0, 0.0, 10.0, 0.0, -1.0, 0.0, 20.0, 0.0, 0.0, 1.0, 30.0]);
        assert_eq!(m[3], [10.0, 20.0, 30.0, 1.0]);
        assert_eq!(m[1], [0.0, -1.0, 0.0, 0.0]);
        assert!(determinant3(&m) < 0.0);
    }
}
