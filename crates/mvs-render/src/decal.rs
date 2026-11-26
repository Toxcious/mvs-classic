//! The deferred decals with r.DBuffer False: the
//! stage's DecalActors and the effects' floor decals, a box each (FDeferredDecalVS / FDeferredDecalPS) drawn into
//! scene colour and the G-buffer by blend mode in DRS_BeforeLighting (ProcessAfterBasePass 141937520), so the lights
//! that follow light them.

use std::collections::HashMap;
use std::sync::Arc;

use mvs_d3d::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use mvs_d3d::Direct3D11::*;
use mvs_d3d::Dxgi::Common::*;
use mvs_material::{Frequency, PreshaderInputs};
use mvs_view::M4;

use crate::assets::Assets;
use crate::fx::FloorDecal;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, InputElement, RasterKey, SamplerKey, ShaderId,
    StateId, StateKey, StencilFace, StencilKey, Texture, dx,
};
use crate::material::{MaterialInstance, MaterialResource, TextureSource, view_sampler};
use crate::renderer::Frame;
use crate::scenery::StageDecal;
use crate::view::{VIEW_RESOURCES, identity, inverse, mul};
use crate::{Error, Result};

/// The base pass's stencil bit set where the primitive receives decals (bReceivesDecals): SetDepthStencilStateForBasePass
/// 14193d4a0 writes it as bit 7 of its reference; a DRS_BeforeLighting decal draws where it is set.
pub const RECEIVES_DECALS_BIT: u8 = 0x80;

/// FSceneTextureUniformParameters' members the decal pixel shaders read: SceneDepthTexture, PointClampSampler
/// (static init 140690280; post/fog.rs, post/outline.rs).
const SCENE_DEPTH_RESOURCE: u16 = 1;
const POINT_CLAMP_RESOURCE: u16 = 12;

/// FDeferredDecalVS's $Globals: FrustumComponentToClip (DeferredDecal.usf's globals in declaration order, the
/// pixel shader's before it).
const VS_FRUSTUM_COMPONENT_TO_CLIP: u32 = 224;
/// FDeferredDecalPS's $Globals byte offsets and sizes: SvPositionToDecal, DecalToWorld, WorldToDecal,
/// DecalOrientation, DecalParams.
const PS_GLOBALS: [(u32, u32); 5] = [(0, 64), (64, 64), (128, 64), (192, 12), (208, 8)];
const VS_ROWS: usize = 18;
const PS_ROWS: usize = 14;

/// UDecalComponent's FadeScreenSize (ctor 142955f30) and r.Decal.FadeScreenSizeMult's default (143a4f22c).
const FADE_SCREEN_SIZE: f32 = 0.01;
const FADE_SCREEN_SIZE_MULT: f32 = 1.0;

/// The unit cube's 12 triangles (the u16 data at 143f83d60) over its corners: corner k is (bit 2, bit 1, bit 0) of
/// k, each as -1 or 1.
const CUBE_INDICES: [u16; 36] =
    [0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6, 2, 1, 3, 7, 1, 7, 5];

/// EDecalBlendMode as the exe numbers it (the switch in 14191b920), for the modes the pass draws.
fn blend_number(mode: &str) -> Option<u8> {
    Some(match mode {
        "DBM_Translucent" => 0,
        "DBM_Emissive" => 3,
        "DBM_AlphaComposite" => 15,
        _ => return None,
    })
}

/// The render targets a decal draws into: the ERenderTargetMode ComputeRenderTargetMode 14191d920 gives the modes
/// drawn here (deferred shading), bound as the decal pass's parameters set them (14192af50). Decals of one mode in a
/// row of the sorted list share a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Targets {
    /// RTM_SceneColorAndGBufferWithNormal (0): SceneColor, GBufferA, GBufferB, GBufferC. DBM_Translucent with a normal.
    WithNormal,
    /// RTM_SceneColorAndGBufferNoNormal (1): SceneColor, GBufferB, GBufferC. DBM_Translucent without a normal,
    /// DBM_AlphaComposite.
    NoNormal,
    /// RTM_SceneColor (6): DBM_Emissive.
    SceneColor,
}

impl Targets {
    fn of(mode: u8, has_normal: bool) -> Targets {
        match mode {
            3 => Targets::SceneColor,
            0 if has_normal => Targets::WithNormal,
            _ => Targets::NoNormal,
        }
    }

    /// The pixel shader outputs of the mode: FDeferredDecalPS is compiled with DECAL_RENDERTARGET_COUNT targets.
    fn output_mask(self) -> u32 {
        match self {
            Targets::WithNormal => 0xf,
            Targets::NoNormal => 0x7,
            Targets::SceneColor => 0x1,
        }
    }

    /// The G-buffer targets bound after SceneColor (indices into SceneTargets::gbuffer).
    fn gbuffer(self) -> &'static [usize] {
        match self {
            Targets::WithNormal => &[0, 1, 2],
            Targets::NoNormal => &[1, 2],
            Targets::SceneColor => &[],
        }
    }
}

/// The blend state of a decal blend mode in DRS_BeforeLighting (GetDecalBlendState 14197dc70, the same switch inlined
/// in 141973bc0), every target's write mask RGB, each initializer read from its TStaticBlendState's creator. The
/// flag 14505248f (GSupportsSeparateRenderTargetBlendState) is set by FD3D11DynamicRHI's constructor (14185a470) at
/// feature level 11_0 and above, the build's only D3D11 path, so its states are the G-buffer ones:
/// - DBM_Emissive (144c3a418): RT0 Add SrcAlpha One, alpha Add One Zero.
/// - DBM_Translucent with a normal (144c3a3a0, 1419785e0): RT0 Add SrcAlpha One (the emissive), RT1..RT3 Add SrcAlpha
///   InvSrcAlpha (normal; metallic, specular, roughness; base colour); alpha Add Zero One on all.
/// - DBM_Translucent without a normal (144c3a3b8, 1419788e0): RT0 Add SrcAlpha One, RT1 and RT2 Add SrcAlpha
///   InvSrcAlpha; alpha Add Zero One.
/// - DBM_AlphaComposite (144c3a448, 1419782e0): RT0 Add One InvSrcAlpha, RT1 Add Zero One (GBufferB kept), RT2 and
///   RT3 Add One InvSrcAlpha (RT3 is not bound in its mode); alpha Add Zero One.
fn blend_key(mode: u8, targets: Targets) -> BlendKey {
    let rgb =
        (D3D11_COLOR_WRITE_ENABLE_RED.0 | D3D11_COLOR_WRITE_ENABLE_GREEN.0 | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8;
    let t = |src: D3D11_BLEND, dst: D3D11_BLEND, alpha_src: D3D11_BLEND, alpha_dst: D3D11_BLEND| BlendTarget {
        color_op: D3D11_BLEND_OP_ADD.0 as u8,
        color_src: src.0 as u8,
        color_dst: dst.0 as u8,
        alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
        alpha_src: alpha_src.0 as u8,
        alpha_dst: alpha_dst.0 as u8,
        write_mask: rgb,
    };
    let (zero, one) = (D3D11_BLEND_ZERO, D3D11_BLEND_ONE);
    let (src_alpha, inv_src_alpha) = (D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_INV_SRC_ALPHA);
    let mut out = [BlendTarget::OPAQUE; 8];
    match mode {
        3 => out[0] = t(src_alpha, one, one, zero),
        15 => {
            out[0] = t(one, inv_src_alpha, zero, one);
            out[1] = t(zero, one, zero, one);
            out[2] = t(one, inv_src_alpha, zero, one);
            out[3] = t(one, inv_src_alpha, zero, one);
        }
        _ => {
            out[0] = t(src_alpha, one, zero, one);
            for o in &mut out[1..=targets.gbuffer().len()] {
                *o = t(src_alpha, inv_src_alpha, zero, one);
            }
        }
    }
    BlendKey { targets: out, alpha_to_coverage: false }
}

/// What a pixel shader register reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Res {
    Globals,
    View(u16),
    Material(u16),
    Collection(usize),
    SceneDepth,
    PointClamp,
    None,
}

/// The material's parameters: the instance's scalars over the material's defaults.
struct Params<'a> {
    m: &'a mvs_material::Material,
    scalars: &'a [(usize, f64)],
}

impl PreshaderInputs for Params<'_> {
    fn scalar(&self, k: usize) -> f64 {
        self.scalars.iter().find(|s| s.0 == k).map_or(self.m.uniforms.scalars[k].value, |s| s.1)
    }
    fn vector(&self, k: usize) -> [f64; 4] {
        self.m.uniforms.vectors[k].value
    }
    fn texture_size(&self, k: usize) -> Option<[f64; 4]> {
        self.m.uniforms.texture_sizes.as_ref()?.get(k).copied().flatten()
    }
}

/// A decal material: its instance, FDeferredDecalPS and what its registers read.
#[derive(Debug)]
struct DecalMaterial {
    inst: MaterialInstance,
    ps: ShaderId,
    /// The final blend mode (the exe's number), the targets it draws into and its blend state.
    mode: u8,
    targets: Targets,
    blend: StateId,
    cbs: Vec<Res>,
    srvs: Vec<(u32, Res)>,
    samplers: Vec<(u32, Res)>,
    /// The scalars the Material buffer was last written with (parameter index, value).
    written: Option<Vec<(usize, f64)>>,
}

impl DecalMaterial {
    fn new(gpu: &Gpu, assets: &Assets, path: &str) -> Result<DecalMaterial> {
        let bad = |why: String| Error::Format { what: path.to_string(), why };
        let inst = MaterialInstance::new(gpu, assets, path)?;
        let m = inst.material.clone();
        if m.state.domain != "MD_DeferredDecal" {
            return Err(bad(format!("domain {}, not a deferred decal", m.state.domain)));
        }
        let mode = blend_number(&m.state.decal_blend_mode)
            .ok_or_else(|| bad(format!("{}: the decal pass's blend mode is not ported", m.state.decal_blend_mode)))?;
        let (_, rows) =
            m.shaders.pass(None, None, &[("FDeferredDecalPS", 0)]).ok_or_else(|| bad("no FDeferredDecalPS".into()))?;
        let sha = m.shaders.code(rows[0]);
        let code = assets.code(sha)?;
        let ps = gpu.shader_by(sha, &code)?;
        // The decal's bHasNormal (the material's normal connected) picks DBM_Translucent's targets; the cooked pixel
        // shader was compiled for those targets, so its outputs tell it.
        // Unverified: bHasNormal is taken from FDeferredDecalPS's outputs (four targets), not from the material's
        // normal input, which the store does not carry.
        let has_normal = code.output_mask == Targets::WithNormal.output_mask();
        let targets = Targets::of(mode, has_normal);
        if code.output_mask != targets.output_mask() {
            return Err(bad(format!("FDeferredDecalPS writes targets {:#x}, not {targets:?}'s", code.output_mask)));
        }
        for b in &m.shaders.params(rows[0]).parameter_map.loose_parameter_buffers {
            for p in &b.parameters {
                if !PS_GLOBALS.iter().any(|&(o, s)| o == p.base_index && s >= p.size) {
                    return Err(bad(format!("FDeferredDecalPS reads $Globals at {} ({} bytes)", p.base_index, p.size)));
                }
            }
        }
        let names = &code.uniform_buffer_names;
        let buffer = |r: usize| match names.get(r).map(String::as_str) {
            Some("") if r == 0 => Res::Globals,
            Some("View") => Res::View(0),
            Some("Material") => Res::Material(0),
            Some(n) => {
                n.strip_prefix("MaterialCollection").and_then(|k| k.parse().ok()).map_or(Res::None, Res::Collection)
            }
            None => Res::None,
        };
        let cbs = (0..names.len().max(1)).map(buffer).collect();
        let resource = |s: mvs_material::shader::TableSlot| -> Result<Res> {
            let name = names.get(s.buffer as usize).map_or("", String::as_str);
            Ok(match (name, s.resource) {
                ("View", k) => Res::View(k),
                ("Material", k) => Res::Material(k),
                ("SceneTexturesStruct", SCENE_DEPTH_RESOURCE) => Res::SceneDepth,
                ("SceneTexturesStruct", POINT_CLAMP_RESOURCE) => Res::PointClamp,
                (n, k) => return Err(bad(format!("FDeferredDecalPS reads {n} resource {k}, not bound by the pass"))),
            })
        };
        let rt = &code.resource_table;
        let mut srvs = Vec::new();
        for map in [&rt.texture_map, &rt.srv_map] {
            for s in rt.slots(map)? {
                srvs.push((s.register as u32, resource(s)?));
            }
        }
        let mut samplers = Vec::new();
        for s in rt.slots(&rt.sampler_map)? {
            samplers.push((s.register as u32, resource(s)?));
        }
        Ok(DecalMaterial {
            inst,
            ps,
            mode,
            targets,
            blend: gpu.state(StateKey::Blend(blend_key(mode, targets))),
            cbs,
            srvs,
            samplers,
            written: None,
        })
    }

    /// The Material buffer's rows over `scalars` (by name), written when they changed since the last write.
    fn set_scalars(&mut self, gpu: &Gpu, scalars: &[(&str, f64)]) -> Result<()> {
        let m = self.inst.material.clone();
        let set: Vec<(usize, f64)> = scalars
            .iter()
            .filter_map(|(n, v)| m.uniforms.scalars.iter().position(|s| s.name == *n).map(|k| (k, *v)))
            .collect();
        if self.written.as_ref() == Some(&set) {
            return Ok(());
        }
        if let Some(b) = &self.inst.buffer {
            let rows = m.uniform_rows(&Params { m: &m, scalars: &set })?;
            let mut out = vec![[0u32; 4]; (b.desc.size / 16) as usize];
            for (o, r) in out.iter_mut().zip(&rows) {
                *o = r.map(|x| (x as f32).to_bits());
            }
            gpu.write_rows(b, &out);
        }
        self.written = Some(set);
        Ok(())
    }
}

/// One decal of the frame: its material, ComponentToWorld (the box's axes scaled by its half extents, UE world),
/// its material's scalars and its place in the list before the sort.
struct Drawn<'a> {
    material: &'a str,
    to_world: M4,
    scalars: Vec<(&'static str, f64)>,
    n: usize,
}

/// The decal pass: its shaders and states, the decal materials made so far and the stage's decals.
#[derive(Debug)]
pub struct Decals {
    vs: ShaderId,
    cube: Buffer,
    cube_index: Buffer,
    vs_globals: Buffer,
    ps_globals: Buffer,
    /// By material object path, with the assets' load mark each was last used at (`trim`).
    materials: HashMap<String, (DecalMaterial, u64)>,
    /// A floor decal's material name to its object path in the material kind (made at the first floor decal).
    names: Option<Arc<HashMap<String, String>>>,
    /// The stage's decals: material path and ComponentToWorld.
    stage: Vec<(String, M4)>,
    /// Depth tested near-or-equal (outside the box) or always (inside), both with the receive-decal stencil test.
    depth_outside: StateId,
    depth_inside: StateId,
    /// The read-only view of the scene depth-stencil, by its resource.
    read_only: Option<(usize, ID3D11DepthStencilView)>,
}

impl Decals {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<Decals> {
        let g = assets.global_shaders();
        let row = g
            .find("FDeferredDecalVS", 0, Frequency::Vertex)
            .ok_or_else(|| Error::Format { what: "global shaders".into(), why: "no FDeferredDecalVS".into() })?;
        let sha = g.shaders.code(row);
        let code = assets.code(sha)?;
        let vs = gpu.shader_by(sha, &code)?;
        for b in &g.shaders.params(row).parameter_map.loose_parameter_buffers {
            if b.parameters.iter().any(|p| p.base_index != VS_FRUSTUM_COMPONENT_TO_CLIP || p.size > 64) {
                return Err(Error::Format {
                    what: "FDeferredDecalVS".into(),
                    why: format!("$Globals {:?}, not FrustumComponentToClip alone", b.parameters),
                });
            }
        }
        let corners: Vec<u8> = (0..8u32)
            .flat_map(|k| {
                let c = |bit: u32| if k >> bit & 1 == 1 { 1.0f32 } else { -1.0 };
                [c(2), c(1), c(0), 1.0]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        let indices: Vec<u8> = CUBE_INDICES.iter().flat_map(|i| i.to_le_bytes()).collect();
        let cube =
            gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: corners.len() as u32, dynamic: false }, Some(&corners))?;
        let cube_index = gpu
            .buffer(&BufDesc { kind: BufKind::Index16, size: indices.len() as u32, dynamic: false }, Some(&indices))?;
        let constants = |rows: usize| {
            gpu.buffer(&BufDesc { kind: BufKind::Constant, size: (rows * 16) as u32, dynamic: true }, None)
        };
        // The stage-2 states without the pre-stencil (1418fc690 outside, 1418fc8a0 inside): no depth write, stencil
        // Equal with every op Keep.
        // The test is the stage-2 one, (stencil & 0x81) == 0x80: the receive-decals bit set, the sandbox bit 0 clear.
        // Deviation: the pre-stencil pass (14193ca50, an optimisation that marks the box's pixels in the sandbox bit
        // first) is not drawn, so the sandbox bit stays 0 and the test passes on every receiving pixel in the box.
        let equal = StencilFace {
            func: D3D11_COMPARISON_EQUAL.0 as u8,
            fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            pass: D3D11_STENCIL_OP_KEEP.0 as u8,
        };
        let stencil = Some(StencilKey { front: equal, back: equal, read_mask: RECEIVES_DECALS_BIT | 1, write_mask: 0 });
        let depth = |func: D3D11_COMPARISON_FUNC| {
            gpu.state(StateKey::Depth(DepthKey { depth_write: false, depth_func: func.0 as u8, stencil }))
        };
        Ok(Decals {
            vs,
            cube,
            cube_index,
            vs_globals: constants(VS_ROWS)?,
            ps_globals: constants(PS_ROWS)?,
            materials: HashMap::new(),
            names: None,
            stage: Vec::new(),
            depth_outside: depth(D3D11_COMPARISON_GREATER_EQUAL),
            depth_inside: depth(D3D11_COMPARISON_ALWAYS),
            read_only: None,
        })
    }

    /// The stage's DecalActors (replacing the last stage's).
    pub fn set_stage(&mut self, gpu: &Gpu, assets: &Assets, decals: &[StageDecal]) -> Result<()> {
        self.stage.clear();
        for d in decals {
            // (the scenery entry names the material with its draw context)
            let path = d.material.split('?').next().unwrap_or(&d.material);
            self.material(gpu, assets, path)?;
            self.stage.push((path.to_string(), crate::stage::transform(d.loc, d.rot, d.half)));
        }
        Ok(())
    }

    fn material(&mut self, gpu: &Gpu, assets: &Assets, path: &str) -> Result<()> {
        let mark = assets.mark();
        match self.materials.get_mut(path) {
            Some((_, used)) => *used = mark,
            None => {
                self.materials.insert(path.to_string(), (DecalMaterial::new(gpu, assets, path)?, mark));
            }
        }
        Ok(())
    }

    /// The warm-up (crate::warm) of the floor decals of materials `names` (export names or object paths): each
    /// material made, the cube's input layout and its raster states. The lines of what failed.
    pub(crate) fn warm(&mut self, gpu: &Gpu, assets: &Assets, names: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for n in names {
            // (a name with no material of the kind is none an effect can draw: the scan of the classes finds it)
            let Ok(p) = self.floor_path(assets, n) else { continue };
            if let Err(e) = self.material(gpu, assets, &p) {
                out.push(format!("the decal {n}: {e}"));
            }
        }
        if let Err(e) = gpu.set_input_layout(self.vs, 0, &[CUBE_POSITION]) {
            out.push(format!("the decals' input layout: {e}"));
        }
        for clockwise in [false, true] {
            gpu.state(StateKey::Raster(cube_raster(clockwise)));
        }
        out
    }

    /// A floor decal's material (named by its export name, as the effects' data gives it) as an object path.
    fn floor_path(&mut self, assets: &Assets, name: &str) -> Result<String> {
        if name.starts_with('/') {
            return Ok(name.to_string());
        }
        if self.names.is_none() {
            self.names = Some(Arc::new(material_names(assets)?));
        }
        floor_path_in(self.names.as_deref().expect("set above"), name)
    }

    /// The material names the floor decals are found by, if made already (a loader's warm-up takes them).
    pub(crate) fn names(&self) -> Option<Arc<HashMap<String, String>>> {
        self.names.clone()
    }

    /// What a loader thread's warm-up made (`warm_off_thread`) taken in: the materials this pass has not made yet,
    /// and the names when it had none.
    pub(crate) fn adopt(&mut self, made: DecalsMade, mark: u64) {
        for (path, m) in made.materials {
            self.materials.entry(path).or_insert((m, mark)).1 = mark;
        }
        if self.names.is_none() {
            self.names = made.names;
        }
    }

    /// Lets go the materials nothing has used since the assets' load `mark` and the stage does not draw (a floor
    /// decal of a fighter or stage no longer played), so the textures they hold can go at the assets' trim. The
    /// materials let go.
    pub(crate) fn trim(&mut self, mark: u64) -> usize {
        let before = self.materials.len();
        let stage = &self.stage;
        self.materials.retain(|path, (_, used)| *used >= mark || stage.iter().any(|(p, _)| p == path));
        before - self.materials.len()
    }
}

/// The floor decals' materials a loader thread made, for `Decals::adopt`.
#[derive(Debug, Default)]
pub(crate) struct DecalsMade {
    materials: Vec<(String, DecalMaterial)>,
    names: Option<Arc<HashMap<String, String>>>,
}

/// Every material of the material kind by its export name.
fn material_names(assets: &Assets) -> Result<HashMap<String, String>> {
    let pack = assets.store().kind("material")?;
    Ok(pack.paths().filter_map(|p| p.rsplit_once('.').map(|(_, n)| (n.to_string(), p.to_string()))).collect())
}

fn floor_path_in(names: &HashMap<String, String>, name: &str) -> Result<String> {
    if name.starts_with('/') {
        return Ok(name.to_string());
    }
    names
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Format { what: name.to_string(), why: "no decal material of that name".into() })
}

/// The decal warm-up's materials made on a loader thread (`gpu` a deferred one): what `Decals::warm` would make,
/// for `Decals::adopt`. One that fails is left for the frame thread's warm-up to report.
pub(crate) fn warm_off_thread(
    gpu: &Gpu,
    assets: &Assets,
    names: Option<Arc<HashMap<String, String>>>,
    list: &[String],
) -> DecalsMade {
    let names = match names {
        Some(n) => Some(n),
        None if list.iter().any(|n| !n.starts_with('/')) => material_names(assets).ok().map(Arc::new),
        None => None,
    };
    let mut materials: Vec<(String, DecalMaterial)> = Vec::new();
    for n in list {
        let path = match &names {
            Some(m) => floor_path_in(m, n),
            None => floor_path_in(&HashMap::new(), n),
        };
        let Ok(p) = path else { continue };
        if materials.iter().any(|(q, _)| *q == p) {
            continue;
        }
        if let Ok(m) = DecalMaterial::new(gpu, assets, &p) {
            materials.push((p, m));
        }
    }
    DecalsMade { materials, names }
}

impl Decals {
    /// The stage's decals and the effects' `floor` decals, after the base pass and custom depth, before the
    /// lighting, scene depth and stencil read-only.
    pub fn draw(&mut self, fr: &mut Frame, floor: &[FloorDecal]) -> Result<()> {
        let gpu = fr.gpu;
        let dsv = self.read_only_depth(gpu, &fr.targets.depth)?;
        let mut floor_paths = Vec::with_capacity(floor.len());
        for d in floor {
            let path = self.floor_path(fr.assets, &d.material)?;
            self.material(gpu, fr.assets, &path)?;
            floor_paths.push(path);
        }
        let mut list: Vec<Drawn> = Vec::with_capacity(self.stage.len() + floor.len());
        for (path, m) in &self.stage {
            list.push(Drawn { material: path, to_world: *m, scalars: Vec::new(), n: list.len() });
        }
        for (d, path) in floor.iter().zip(&floor_paths) {
            let m: M4 = std::array::from_fn(|i| {
                if i < 3 {
                    std::array::from_fn(|c| if c < 3 { d.rows[3 * i + c] * d.extent[i] } else { 0.0 })
                } else {
                    [d.loc[0], d.loc[1], d.loc[2], 1.0]
                }
            });
            // The decal's dynamic instance: Alpha its timeline and Dissolve
            // where a graph sets them, Team the owner's team where
            // there is one (a tutorial's ownerless fire wall keeps its material's).
            let mut scalars: Vec<(&str, f64)> = d.team.map(|t| ("Team", t)).into_iter().collect();
            if let Some(a) = d.alpha {
                scalars.insert(0, ("Alpha", a));
            }
            if let Some(v) = d.dissolve {
                scalars.push(("Dissolve", v));
            }
            list.push(Drawn { material: path, to_world: m, scalars, n: list.len() });
        }
        if list.is_empty() {
            return Ok(());
        }
        // The sort (141966a00): SortOrder ascending, then the decals that write a normal first, then the final blend
        // mode descending; the exe then breaks ties by material proxy and component.
        // Unverified: SortOrder is not in the scenery entry (taken 0, the component default), and ties keep the
        // stage's decals first in their order, then the floor decals (no material or proxy key). The normal key is
        // bHasNormal as the targets give it, so it is false for DBM_AlphaComposite and DBM_Emissive, whose outputs do
        // not show a connected normal (the exe keys on the material's bHasNormal whatever the mode).
        list.sort_by_key(|d| {
            let m = &self.materials[d.material].0;
            (m.targets != Targets::WithNormal, std::cmp::Reverse(m.mode), d.n)
        });

        let cam = fr.input.camera;
        let t = fr.targets;
        let [w, h] = t.size;
        let proj = cam.view_to_clip;
        let tvp = mul(&cam.translated_world_to_view, &proj);
        // SVPosition -> clip over the view rect (the whole target).
        let mut sv_to_clip = identity();
        sv_to_clip[0][0] = 2.0 / w as f64;
        sv_to_clip[1][1] = -2.0 / h as f64;
        sv_to_clip[3] = [-1.0, 1.0, 0.0, 1.0];
        let sv_to_tw = mul(&sv_to_clip, &inverse(&tvp));
        // UE's reversed-Z perspective: M[3][2] is the near plane.
        let near = proj[3][2];
        let perspective = proj[3][3] < 1.0;

        gpu.viewport([0, 0, w, h]);
        gpu.set_geometry_shader(None);
        gpu.set_input_layout(self.vs, 0, &[CUBE_POSITION])?;
        // SAFETY: views and buffers of this device; the targets and views bound here are unbound before the return.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetVertexBuffers(0, 1, Some(&Some(self.cube.buf.clone())), Some(&16), Some(&0));
            ctx.IASetIndexBuffer(&self.cube_index.buf, DXGI_FORMAT_R16_UINT, 0);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.vs_globals.buf.clone())]));
        }
        let mut max_srv = 1;
        let mut bound: Option<Targets> = None;
        for d in &list {
            // A run of one render target mode shares a pass (141920950): its targets bound once.
            let targets = self.materials[d.material].0.targets;
            if bound != Some(targets) {
                let rtvs: Vec<Option<ID3D11RenderTargetView>> = std::iter::once(t.scene_color.rtv.clone())
                    .chain(targets.gbuffer().iter().map(|&k| t.gbuffer[k].rtv.clone()))
                    .collect();
                // SAFETY: this device's targets, with the depth-stencil read-only while the decals read its depth.
                unsafe { gpu.ctx.OMSetRenderTargets(Some(&rtvs), &dsv) };
                bound = Some(targets);
            }
            let m = d.to_world;
            let loc = [m[3][0], m[3][1], m[3][2]];
            let axis_len = |i: usize| (m[i][0] * m[i][0] + m[i][1] * m[i][1] + m[i][2] * m[i][2]).sqrt();
            // ConservativeRadius: the length of the scaled axes together.
            let radius = (0..3).map(|i| axis_len(i).powi(2)).sum::<f64>().sqrt();
            let dist = [0, 1, 2].map(|c| cam.origin[c] - loc[c]).iter().map(|x| x * x).sum::<f64>().sqrt();
            // The screen-size fade (BuildVisibleDecalList 141974ec0), float32.
            let fade = if perspective {
                let max_axis = (0..3).map(axis_len).fold(0.0, f64::max) as f32;
                let size = FADE_SCREEN_SIZE_MULT * max_axis / dist as f32;
                let coeff = FADE_SCREEN_SIZE * (2.0 / proj[0][0] as f32) / w as f32 * 600.0;
                ((size - coeff) * (2.0 / coeff)).min(1.0)
            } else {
                1.0
            };
            if fade <= 0.0 || radius < 1e-8 {
                continue;
            }
            // Unverified: the floor decals take UDecalComponent's FadeScreenSize; the effects' decal components'
            // own values are not read.
            let inside = dist * dist < (radius * 1.05 + 2.0 * near).powi(2);
            let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
            // bClockwise = bInsideDecal XOR (the scale's product < 0) (View.bReverseCulling is false): CM_CW
            // (144f90558) or CM_CCW (144f90570). Outside the box the faces toward the camera draw (tested near-or-equal
            // against the scene), inside the far ones (depth Always).
            // The 4.26 D3D11 RHI maps CM_CCW to D3D11_CULL_FRONT; this cube's winding is the opposite of
            // FUnitCubeVertexBuffer's (corner 0 at (1, 1, 1)), so here CM_CCW culls the D3D11 back faces and CM_CW the
            // front, the same faces drawn.
            // Unverified: the build cube's winding is the 4.26 source's, not read in the exe.
            let clockwise = inside != (det < 0.0);
            let raster = gpu.state(StateKey::Raster(cube_raster(clockwise)));
            // ComponentToWorld in translated world (PreViewTranslation added to its origin).
            let mut translated = m;
            for c in 0..3 {
                translated[3][c] = loc[c] - cam.origin[c];
            }
            let mut vs_rows = [[0u32; 4]; VS_ROWS];
            put_m4(&mut vs_rows, VS_FRUSTUM_COMPONENT_TO_CLIP, &mul(&translated, &tvp));
            gpu.write_rows(&self.vs_globals, &vs_rows);
            // SvPositionToDecal = SvPositionToClip x InvViewProjection x WorldToDecal, taken through translated world.
            let mut ps_rows = [[0u32; 4]; PS_ROWS];
            put_m4(&mut ps_rows, PS_GLOBALS[0].0, &mul(&sv_to_tw, &inverse(&translated)));
            put_m4(&mut ps_rows, PS_GLOBALS[1].0, &m);
            put_m4(&mut ps_rows, PS_GLOBALS[2].0, &inverse(&m));
            let x = axis_len(0);
            put(&mut ps_rows, PS_GLOBALS[3].0, &[m[0][0] / x, m[0][1] / x, m[0][2] / x].map(|v| v as f32));
            // DecalParams: FadeAlpha, LifetimeAlpha (1 with no fade duration: UDecalComponent's defaults).
            put(&mut ps_rows, PS_GLOBALS[4].0, &[fade, 1.0]);
            gpu.write_rows(&self.ps_globals, &ps_rows);

            let (mat, used) = self.materials.get_mut(d.material).expect("made above");
            *used = fr.assets.mark();
            mat.set_scalars(gpu, &d.scalars)?;
            let cbs: Vec<Option<ID3D11Buffer>> = mat
                .cbs
                .iter()
                .map(|r| match r {
                    Res::Globals => Some(self.ps_globals.buf.clone()),
                    Res::View(_) => Some(fr.scene.view.buf.clone()),
                    Res::Material(_) => mat.inst.buffer.as_ref().map(|b| b.buf.clone()),
                    Res::Collection(k) => mat.inst.collections.get(*k).map(|b| b.buf.clone()),
                    _ => None,
                })
                .collect();
            let n = mat.srvs.iter().map(|s| s.0 as usize + 1).max().unwrap_or(0);
            max_srv = max_srv.max(n);
            let mut srvs: Vec<Option<ID3D11ShaderResourceView>> = vec![None; n];
            for &(r, s) in &mat.srvs {
                srvs[r as usize] = match s {
                    Res::SceneDepth => t.depth.srv.clone(),
                    Res::View(k) => VIEW_RESOURCES.get(k as usize).and_then(|name| {
                        fr.scene.view_textures.iter().rev().find(|(v, _)| v == name).and_then(|(_, t)| t.srv.clone())
                    }),
                    Res::Material(k) => match mat.inst.resources.get(k as usize) {
                        Some(MaterialResource::Texture(TextureSource::Asset(t))) => t.srv.clone(),
                        _ => None,
                    },
                    _ => None,
                };
            }
            let n = mat.samplers.iter().map(|s| s.0 as usize + 1).max().unwrap_or(0);
            let mut samplers: Vec<Option<ID3D11SamplerState>> = vec![None; n];
            for &(r, s) in &mat.samplers {
                let id = match s {
                    Res::View(k) => {
                        VIEW_RESOURCES.get(k as usize).map(|name| gpu.state(StateKey::Sampler(view_sampler(name))))
                    }
                    // Unverified: PointClampSampler point-filtered and clamped (4.26's scene texture sampler).
                    Res::PointClamp => Some(gpu.state(StateKey::Sampler(SamplerKey::of(
                        D3D11_FILTER_MIN_MAG_MIP_POINT,
                        D3D11_TEXTURE_ADDRESS_CLAMP,
                    )))),
                    Res::Material(k) => match mat.inst.resources.get(k as usize) {
                        Some(MaterialResource::Sampler(id)) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                samplers[r as usize] = id.map(|id| gpu.sampler(id));
            }
            gpu.set_shaders(self.vs, Some(mat.ps));
            let depth = if inside { self.depth_inside } else { self.depth_outside };
            gpu.set_states(mat.blend, raster, depth, RECEIVES_DECALS_BIT as u32);
            // SAFETY: the pipeline set above; the resources are this device's.
            unsafe {
                let ctx = &gpu.ctx;
                ctx.PSSetConstantBuffers(0, Some(&cbs));
                ctx.PSSetShaderResources(0, Some(&srvs));
                ctx.PSSetSamplers(0, Some(&samplers));
                ctx.DrawIndexed(CUBE_INDICES.len() as u32, 0, 0);
            }
        }
        let none: Vec<Option<ID3D11ShaderResourceView>> = vec![None; max_srv];
        // SAFETY: unbinds what was bound above.
        unsafe {
            gpu.ctx.PSSetShaderResources(0, Some(&none));
            gpu.ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// The read-only view of the scene depth-stencil `depth`, made once per resource.
    fn read_only_depth(&mut self, gpu: &Gpu, depth: &Texture) -> Result<ID3D11DepthStencilView> {
        use mvs_d3d::core::Interface;
        let key = depth.res.as_raw() as usize;
        if let Some((k, v)) = &self.read_only
            && *k == key
        {
            return Ok(v.clone());
        }
        let format = match depth.desc.format {
            DXGI_FORMAT_R32G8X24_TYPELESS => DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
            DXGI_FORMAT_R24G8_TYPELESS => DXGI_FORMAT_D24_UNORM_S8_UINT,
            f => return Err(Error::Format { what: "scene depth".into(), why: format!("format {}", f.0) }),
        };
        let dd = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            Flags: (D3D11_DSV_READ_ONLY_DEPTH.0 | D3D11_DSV_READ_ONLY_STENCIL.0) as u32,
            Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
        };
        let mut v = None;
        // SAFETY: a view of the depth target's resource in its depth format.
        unsafe { gpu.dev.CreateDepthStencilView(&depth.res, Some(&dd), Some(&mut v)).map_err(dx("read-only DSV"))? };
        let v = v.expect("CreateDepthStencilView");
        self.read_only = Some((key, v.clone()));
        Ok(v)
    }
}

/// The decal cube's one vertex element.
const CUBE_POSITION: InputElement = InputElement {
    semantic: c"ATTRIBUTE",
    index: 0,
    format: DXGI_FORMAT_R32G32B32A32_FLOAT,
    slot: 0,
    per_instance: false,
};

/// The cube's raster state: CM_CW culls the front faces, CM_CCW the back (see the draw).
fn cube_raster(clockwise: bool) -> RasterKey {
    RasterKey {
        fill: D3D11_FILL_SOLID.0 as u8,
        cull: if clockwise { D3D11_CULL_FRONT } else { D3D11_CULL_BACK }.0 as u8,
        front_ccw: true,
        depth_bias: 0,
        slope_scaled_depth_bias: 0,
    }
}

/// Floats into constant buffer rows at byte offset `at`.
fn put(rows: &mut [[u32; 4]], at: u32, v: &[f32]) {
    for (i, x) in v.iter().enumerate() {
        let k = at as usize / 4 + i;
        rows[k / 4][k % 4] = x.to_bits();
    }
}

/// A row-vector M4 into constant buffer rows at byte offset `at`, row by row (UE's FMatrix upload).
fn put_m4(rows: &mut [[u32; 4]], at: u32, m: &M4) {
    for (i, r) in m.iter().enumerate() {
        put(rows, at + 16 * i as u32, &r.map(|x| x as f32));
    }
}
