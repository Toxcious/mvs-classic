//! The scene targets of the deferred path: SceneColor, the G-buffer, the depth-stencil (reversed Z), custom depth,
//! separate translucency, and the camera's priority captures.

use mvs_d3d::Dxgi::Common::*;

use crate::Result;
use crate::gpu::{Gpu, TexDesc, TexKind, TexViews, Texture};

/// A priority capture's size (PFGCamera's SceneCaptureComponent2D_Priority1..3 render targets).
pub const PRIORITY_CAPTURE_SIZE: [u32; 2] = [1024, 578];
/// The value a priority capture holds where no shown primitive covers it: the far depth.
// Deviation: the build's scene depth there (about 1e8) overflows R16F to +INF (FSceneCapturePS permutation 3,
// 141c7ea10); the port keeps 65504, the largest finite half, so no material's arithmetic meets an infinity.
// The materials' cut (below 10,000 cm) reads both the same.
pub const PRIORITY_CAPTURE_FAR: f32 = 65504.0;

/// A scene target, as a material or a pass names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetId {
    SceneColor,
    GBufferA,
    GBufferB,
    GBufferC,
    GBufferD,
    GBufferE,
    SceneDepth,
    CustomDepth,
    SeparateTranslucency,
    /// The camera's priority capture N (1..=3): scene depth in cm of the priority stack's first N fighters.
    PriorityCapture(u8),
}

#[derive(Debug)]
pub struct SceneTargets {
    pub size: [u32; 2],
    pub scene_color: Texture,
    /// GBufferA to GBufferE.
    pub gbuffer: [Texture; 5],
    /// Depth-stencil, reversed Z (cleared to 0).
    pub depth: Texture,
    /// Custom depth-stencil (the stencil holds CustomDepthStencilValue).
    pub custom_depth: Texture,
    pub separate_translucency: Texture,
    /// The priority captures 1..=3: R16F, PRIORITY_CAPTURE_SIZE, cleared to PRIORITY_CAPTURE_FAR at creation.
    pub priority_captures: [Texture; 3],
}

impl SceneTargets {
    pub fn new(gpu: &Gpu, size: [u32; 2]) -> Result<SceneTargets> {
        let target = |format: DXGI_FORMAT| {
            gpu.texture(
                &TexDesc {
                    kind: TexKind::D2,
                    size: [size[0], size[1], 1],
                    mips: 1,
                    format,
                    views: TexViews { srv: true, rtv: true, ..Default::default() },
                },
                &[],
            )
        };
        let depth = || {
            gpu.texture(
                &TexDesc {
                    kind: TexKind::D2,
                    size: [size[0], size[1], 1],
                    mips: 1,
                    format: DXGI_FORMAT_R32G8X24_TYPELESS,
                    views: TexViews { srv: true, dsv: true, ..Default::default() },
                },
                &[],
            )
        };
        // FSceneRenderTargets's GBuffer allocation (exe 141bf1590), r.GBufferFormat 1: GBufferA PF_A2B10G10R10, B to E
        // PF_B8G8R8A8, GBufferC with TexCreate_SRGB (its views sRGB: the base pass writes linear, the lighting reads
        // linear). SceneColor (141bf1980): PF_FloatRGBA, the Epic / Cine effects quality's r.SceneColorFormat 4
        // (BaseScalability.ini gives Low to High PF_FloatRGB, R11G11B10); FloatRGBA is kept at every quality.
        // Unverified: depth PF_DepthStencil's format is 4.26's, not read from the exe.
        Ok(SceneTargets {
            size,
            scene_color: target(DXGI_FORMAT_R16G16B16A16_FLOAT)?,
            gbuffer: [
                target(DXGI_FORMAT_R10G10B10A2_UNORM)?,
                target(DXGI_FORMAT_B8G8R8A8_UNORM)?,
                target(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB)?,
                target(DXGI_FORMAT_B8G8R8A8_UNORM)?,
                target(DXGI_FORMAT_B8G8R8A8_UNORM)?,
            ],
            depth: depth()?,
            custom_depth: depth()?,
            separate_translucency: target(DXGI_FORMAT_R16G16B16A16_FLOAT)?,
            priority_captures: [priority_capture(gpu)?, priority_capture(gpu)?, priority_capture(gpu)?],
        })
    }

    pub fn get(&self, id: TargetId) -> &Texture {
        match id {
            TargetId::SceneColor => &self.scene_color,
            TargetId::GBufferA => &self.gbuffer[0],
            TargetId::GBufferB => &self.gbuffer[1],
            TargetId::GBufferC => &self.gbuffer[2],
            TargetId::GBufferD => &self.gbuffer[3],
            TargetId::GBufferE => &self.gbuffer[4],
            TargetId::SceneDepth => &self.depth,
            TargetId::CustomDepth => &self.custom_depth,
            TargetId::SeparateTranslucency => &self.separate_translucency,
            TargetId::PriorityCapture(n @ 1..=3) => &self.priority_captures[n as usize - 1],
            TargetId::PriorityCapture(n) => panic!("no priority capture {n}"),
        }
    }
}

/// A priority capture's target, cleared to the far depth (what the materials read until the capture is drawn).
fn priority_capture(gpu: &Gpu) -> Result<Texture> {
    let [w, h] = PRIORITY_CAPTURE_SIZE;
    let t = gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [w, h, 1],
            mips: 1,
            format: DXGI_FORMAT_R16_FLOAT,
            views: TexViews { srv: true, rtv: true, ..Default::default() },
        },
        &[],
    )?;
    let far = [PRIORITY_CAPTURE_FAR, 0.0, 0.0, 0.0];
    // SAFETY: the view is this device's target; the colour is four floats.
    unsafe { gpu.ctx.ClearRenderTargetView(t.rtv.as_ref().expect("a target view"), &far) };
    Ok(t)
}
