//! The post chain with the build's global shaders: custom depth and stencil, the legacy
//! atmospheric fog and the height fog, the stage's post-process material blendables (the team outline over the custom
//! stencil, through its skip), bloom (FDownsamplePS, FFilterPS), the tonemapper with its grading LUT (FLUTBlenderPS,
//! FTonemapPS) and FXAA into the frame's output.

mod bloom;
mod fog;
mod outline;
mod pass;
mod skip;
mod tonemap;
mod upscale;

pub(crate) use tonemap::DEFAULT_AA_LEVEL;

use mvs_data::stage::StageData;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::assets::Assets;
use crate::draw::execute;
use crate::gpu::{Gpu, Texture, dx};
use crate::material::PassKind;
use crate::renderer::Frame;
use crate::stagedraw::StageDraw;
use crate::targets::SceneTargets;
use crate::{Error, Result};

use bloom::{BLOOM_INTENSITY, Bloom};
use fog::{AtmospherePass, FogPass, HeightFog, fog_view};
use outline::Blendable;
use pass::{Quad, States, halton, target};

/// Whose settings a frame's post chain takes: the stage's (its fog, bloom and blendables), or a menu level's (the
/// defaults: no fog actor, no PostProcessVolume) with its camera's fade and VignetteIntensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PostView {
    pub stage: bool,
    /// The view's camera fade to black (0 none, 1 black).
    pub fade: f32,
    pub vignette: f32,
}

impl PostView {
    /// A match's frame.
    pub const STAGE: PostView = PostView { stage: true, fade: 0.0, vignette: tonemap::VIGNETTE_INTENSITY };

    /// A menu level's frame.
    pub fn menu(fade: f32, vignette: Option<f32>) -> PostView {
        PostView { stage: false, fade, vignette: vignette.unwrap_or(tonemap::VIGNETTE_INTENSITY) }
    }
}

/// What a stage sets on the post chain.
#[derive(Debug)]
struct StagePost {
    fog: Option<HeightFog>,
    bloom_intensity: f32,
    blendables: Vec<Blendable>,
}

impl Default for StagePost {
    fn default() -> StagePost {
        StagePost { fog: None, bloom_intensity: BLOOM_INTENSITY, blendables: Vec::new() }
    }
}

/// The post chain's targets and shaders.
#[derive(Debug)]
pub struct Post {
    /// The view's size: every pass up to FXAA runs at it (the frame's size scaled by r.ScreenPercentage).
    size: [u32; 2],
    /// The frame's size: the output's. Below it the view is upscaled to the output after FXAA.
    full: [u32; 2],
    quad: Quad,
    states: States,
    fog: FogPass,
    atmosphere: AtmospherePass,
    /// The View buffer the fogs read (fog.rs fog_view).
    fog_view: crate::gpu::Buffer,
    bloom: Bloom,
    tonemap: tonemap::Tonemap,
    stage: StagePost,
    /// The blendables' outputs (scene colour's format), two to alternate between.
    hdr: [Texture; 2],
    /// The tonemapper's output (PF_B8G8R8A8's bytes as RGBA), FXAA's input.
    ldr: Texture,
    /// FXAA's output at the view's size when the upscale follows (a resolution scale below 100).
    aa_out: Texture,
    upscale: upscale::Upscale,
    /// The frame's output when the caller gives none (the frame's size).
    own_out: Texture,
    /// Custom depth's stencil as a uint texture (X32_TYPELESS_G8X24_UINT), made at its first read.
    stencil: Option<ID3D11ShaderResourceView>,
    /// Custom depth was drawn this frame.
    custom_drawn: bool,
    /// Frames drawn, for the grain's Halton sequence.
    frame: u32,
    /// The outline's skip (skip.rs): its shaders, and its targets of the view's size.
    skip: skip::Skip,
    skip_targets: skip::SkipTargets,
    /// The blendables drawn through the skip (off: over every pixel, the skip's check).
    pub(crate) outline_skip: bool,
}

impl Post {
    /// A chain whose passes run at `size` (the view) and whose output is `full` (the frame).
    pub fn new(gpu: &Gpu, assets: &Assets, size: [u32; 2], full: [u32; 2]) -> Result<Post> {
        let quad = Quad::new(gpu)?;
        let states = States::new(gpu);
        let tonemap = tonemap::Tonemap::new(gpu, assets, &quad, &states)?;
        let hdr = || target(gpu, size, DXGI_FORMAT_R16G16B16A16_FLOAT);
        let skip = skip::Skip::new(gpu, states.opaque, states.cull_none)?;
        Ok(Post {
            size,
            full,
            fog: FogPass::new(gpu, assets)?,
            atmosphere: AtmospherePass::new(gpu, assets)?,
            fog_view: gpu.buffer(
                &crate::gpu::BufDesc {
                    kind: crate::gpu::BufKind::Constant,
                    size: (crate::view::VIEW_ROWS * 16) as u32,
                    dynamic: true,
                },
                None,
            )?,
            bloom: Bloom::new(gpu, assets, size)?,
            tonemap,
            quad,
            states,
            stage: StagePost::default(),
            hdr: [hdr()?, hdr()?],
            ldr: target(gpu, size, DXGI_FORMAT_R8G8B8A8_UNORM)?,
            aa_out: target(gpu, size, DXGI_FORMAT_R8G8B8A8_UNORM)?,
            upscale: upscale::Upscale::default(),
            own_out: target(gpu, full, DXGI_FORMAT_R8G8B8A8_UNORM)?,
            stencil: None,
            custom_drawn: false,
            frame: 0,
            skip,
            skip_targets: skip::SkipTargets::new(gpu, size)?,
            outline_skip: true,
        })
    }

    /// The targets of the chain's own parts made at the view's size (the outline skip's); a resize remakes them.
    pub(crate) fn make_sized(&mut self, gpu: &Gpu, size: [u32; 2]) -> Result<()> {
        self.skip_targets = skip::SkipTargets::new(gpu, size)?;
        Ok(())
    }

    /// The stage's height fog, bloom intensity and post-process material blendables (its PostProcessVolumes'
    /// MaterialInstanceConstant and Material blendables of positive weight, in their order).
    // Unverified: every volume's blendables taken as unbound and at full weight (the stages' volumes are).
    // Unverified: each blendable drawn before the tonemapper (BL_BeforeTonemapping, the outline materials' location):
    // the material entry does not hold UMaterial::BlendableLocation, so another location is not refused.
    pub fn set_stage(&mut self, gpu: &Gpu, assets: &Assets, data: &StageData, draw: &StageDraw) -> Result<()> {
        let mut blendables: Vec<Blendable> = Vec::new();
        for b in draw.post_process.iter().flat_map(|v| &v.blendables) {
            let material = b.class.ends_with(".MaterialInstanceConstant") || b.class.ends_with(".Material");
            if material && b.weight > 0.0 && !blendables.iter().any(|x| x.path == b.path) {
                blendables.push(Blendable::new(gpu, assets, &b.path)?);
            }
        }
        self.stage = StagePost {
            fog: data.fog.as_ref().map(HeightFog::of),
            bloom_intensity: data.bloom_intensity.map_or(BLOOM_INTENSITY, |v| v as f32),
            blendables,
        };
        Ok(())
    }

    /// The stage's blendables' buffers of the collection at `path` rewritten (`Renderer::set_collection`).
    pub fn set_collection(&mut self, gpu: &Gpu, path: &str, rows: &[[u32; 4]]) {
        for b in &mut self.stage.blendables {
            b.set_collection(gpu, path, rows);
        }
    }

    /// `fr.list.custom_depth` into custom depth and stencil (each item's CustomDepthStencilValue).
    pub fn custom_depth(&mut self, fr: &mut Frame) -> Result<()> {
        let gpu = fr.gpu;
        let dsv = fr.targets.custom_depth.dsv.as_ref().expect("custom depth's view");
        // SAFETY: custom depth's depth-stencil view, no colour target.
        unsafe {
            gpu.ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0, 0.0, 0);
            gpu.ctx.OMSetRenderTargets(None, dsv);
        }
        let size = fr.targets.size;
        gpu.viewport([0, 0, size[0], size[1]]);
        // Deviation: coplanar writers draw in the list's order (the later one's stencil wins at equal depth), where the
        // build sorts the pass's commands by the opaque sort key (shader hashes).
        execute(
            gpu,
            fr.assets,
            fr.scene,
            fr.targets,
            fr.materials,
            &fr.list.custom_depth,
            PassKind::CustomDepth,
            None,
        )?;
        pass::unbind(gpu);
        self.custom_drawn = true;
        Ok(())
    }

    /// Custom depth's stencil as a uint texture, made at its first read.
    pub(crate) fn stencil_srv(&mut self, gpu: &Gpu, targets: &SceneTargets) -> Result<ID3D11ShaderResourceView> {
        stencil_of(&mut self.stencil, gpu, targets)
    }

    /// FogStruct's rows (ExponentialFogParameters .. ExponentialFogParameters3 and the rest 0) of the stage's height
    /// fog for a camera at height `camera_z`; None without a fog.
    pub(crate) fn fog_rows(&self, camera_z: f64) -> Option<Vec<[u32; 4]>> {
        self.stage.fog.as_ref().map(|f| f.rows(camera_z).0)
    }

    /// The atmospheric fog (when the stage has one) and the height fog over SceneColor; `view` the frame's View
    /// rows.
    pub fn fog(&mut self, fr: &mut Frame, view: &[[u32; 4]; crate::view::VIEW_ROWS]) -> Result<()> {
        fr.gpu.write_rows(&self.fog_view, &fog_view(view).0);
        self.atmosphere.draw(fr, &self.fog_view, &self.quad, &self.states)?;
        if let Some(f) = &self.stage.fog {
            self.fog.draw(fr, &self.fog_view, &self.quad, &self.states, f)?;
        }
        Ok(())
    }

    /// The frame's size.
    pub(crate) fn full(&self) -> [u32; 2] {
        self.full
    }

    /// The anti-aliasing level's FXAA permutation (sg.AntiAliasingQuality; level 0: no FXAA pass).
    pub(crate) fn set_aa(&mut self, gpu: &Gpu, assets: &Assets, level: u8) -> Result<()> {
        self.tonemap.set_aa(gpu, assets, level)
    }

    /// The blendables, bloom, the tonemapper and FXAA from SceneColor to `out` (an RGBA8 target of the frame's
    /// size; none: the chain's own), through the upscale pass when the view is below the frame. `view` names whose
    /// settings apply (a menu level's: the defaults, no blendables).
    pub(crate) fn draw(&mut self, fr: &mut Frame, out: Option<&Texture>, view: &PostView) -> Result<()> {
        let gpu = fr.gpu;
        let out = out.unwrap_or(&self.own_out);
        if [out.desc.size[0], out.desc.size[1]] != self.full || out.desc.format != DXGI_FORMAT_R8G8B8A8_UNORM {
            return Err(Error::Format {
                what: "post output".into(),
                why: format!("{:?} format {} for an RGBA8 target of {:?}", out.desc.size, out.desc.format.0, self.size),
            });
        }
        if !self.custom_drawn {
            // No custom depth pass this frame: the stencil the blendables read is empty.
            let dsv = fr.targets.custom_depth.dsv.as_ref().expect("custom depth's view");
            // SAFETY: custom depth's depth-stencil view.
            unsafe { gpu.ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0, 0.0, 0) };
        }
        self.custom_drawn = false;
        let stencil = stencil_of(&mut self.stencil, gpu, fr.targets)?;
        let mut scene = &fr.targets.scene_color;
        let menu_post = StagePost::default();
        let settings = if view.stage { &self.stage } else { &menu_post };
        // Deviation: the blendables run through the outline skip (skip.rs), the port's own: the same picture at a
        // fraction of the cost (the cooked scan runs on the pixels near a custom stencil only).
        let skip = self.outline_skip
            && !settings.blendables.is_empty()
            && settings.blendables.iter().all(Blendable::skippable);
        if skip {
            let radius = settings.blendables.iter().map(|b| b.scan_radius(self.size)).max().unwrap_or(0);
            self.skip.mark(gpu, &self.skip_targets, &stencil, radius)?;
        }
        for (k, b) in settings.blendables.iter().enumerate() {
            let dst = &self.hdr[k % 2];
            if skip {
                let test = Some((&self.skip_targets.dsv, self.skip.equal));
                b.draw(gpu, &self.quad, &self.states, &fr.scene.view, scene, &stencil, dst, test)?;
                self.skip.copy_uncovered(gpu, &self.skip_targets, scene, dst);
            } else {
                b.draw(gpu, &self.quad, &self.states, &fr.scene.view, scene, &stencil, dst, None)?;
            }
            scene = dst;
        }
        let bloom = self.bloom.draw(gpu, fr.assets, &self.quad, &self.states, scene)?;
        self.tonemap.set_overlay(gpu, &self.quad, &self.states, view.fade)?;
        let n = self.frame % 7;
        self.frame = self.frame.wrapping_add(1);
        self.tonemap.tonemap(
            gpu,
            &self.quad,
            &self.states,
            &fr.scene.view,
            scene,
            Some(bloom),
            settings.bloom_intensity,
            [halton(n, 2), halton(n, 3)],
            view.vignette,
            &self.ldr,
        )?;
        // tonemap -> FXAA (none at level 0) -> upscale (a view below the frame): the last of them writes `out`
        // (AddPostProcessingPasses 141ba7290).
        let up = self.size != self.full;
        let aa = self.tonemap.has_fxaa();
        if aa {
            self.tonemap.fxaa(gpu, &self.quad, &self.states, &self.ldr, if up { &self.aa_out } else { out })?;
        }
        if up {
            let input = if aa { &self.aa_out } else { &self.ldr };
            self.upscale.draw(gpu, fr.assets, &self.quad, &self.states, input, out)?;
        } else if !aa {
            // SAFETY: two textures of this device, the same size and format; the tonemapper's picture copied to the
            // output.
            unsafe { gpu.ctx.CopyResource(&out.res, &self.ldr.res) };
        }
        Ok(())
    }

    /// The chain's own output (the last frame's picture when `draw` was given no target).
    pub fn output(&self) -> &Texture {
        &self.own_out
    }

    /// The targets made again at `size` (the bloom's chain, the blendables', the tonemapper's, the own output and
    /// `make_sized`'s); the stage's settings and the shaders kept. Custom depth's stencil view is made again at its
    /// next read.
    pub(crate) fn resize(&mut self, gpu: &Gpu, assets: &Assets, size: [u32; 2], full: [u32; 2]) -> Result<()> {
        let hdr = || target(gpu, size, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.bloom = Bloom::new(gpu, assets, size)?;
        self.hdr = [hdr()?, hdr()?];
        self.ldr = target(gpu, size, DXGI_FORMAT_R8G8B8A8_UNORM)?;
        self.aa_out = target(gpu, size, DXGI_FORMAT_R8G8B8A8_UNORM)?;
        self.own_out = target(gpu, full, DXGI_FORMAT_R8G8B8A8_UNORM)?;
        self.stencil = None;
        self.make_sized(gpu, size)?;
        self.size = size;
        self.full = full;
        Ok(())
    }
}

/// Custom depth's stencil view kept in `slot`, made at its first read.
fn stencil_of(
    slot: &mut Option<ID3D11ShaderResourceView>,
    gpu: &Gpu,
    targets: &SceneTargets,
) -> Result<ID3D11ShaderResourceView> {
    if let Some(s) = slot {
        return Ok(s.clone());
    }
    let s = stencil_view(gpu, &targets.custom_depth)?;
    *slot = Some(s.clone());
    Ok(s)
}

/// A depth-stencil texture's stencil as a uint texture (its .g).
fn stencil_view(gpu: &Gpu, t: &Texture) -> Result<ID3D11ShaderResourceView> {
    if t.desc.format != DXGI_FORMAT_R32G8X24_TYPELESS {
        return Err(Error::Format { what: "custom depth".into(), why: format!("format {}", t.desc.format.0) });
    }
    let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_X32_TYPELESS_G8X24_UINT,
        ViewDimension: windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 },
        },
    };
    let mut s = None;
    // SAFETY: a view of a depth-stencil texture of this device in its stencil format.
    unsafe { gpu.dev.CreateShaderResourceView(&t.res, Some(&sd), Some(&mut s)).map_err(dx("stencil SRV"))? };
    Ok(s.expect("CreateShaderResourceView"))
}
