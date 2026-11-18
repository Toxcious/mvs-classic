//! The fogs over scene colour. The legacy atmospheric fog: the cooked TAtmosphericFogPS<E_DisableAll> with
//! FAtmosphericVS. The exponential height fog: the cooked
//! TExponentialHeightFogPS<HeightFog> with FHeightFogVS, (One, SrcAlpha) blended, no light shafts, inscattering cubemap,
//! directional inscattering or volumetric fog (render/post fogFrag).

use mvs_material::Frequency;
use mvs_material::material::TextureType;

use super::pass::{Binds, Global, Quad, Rows, Stage, States, constant_buffer, unbind, write};
use crate::assets::Assets;
use crate::gpu::{Buffer, Gpu};
use crate::renderer::Frame;
use crate::view::{VIEW_ROWS, member};
use crate::{Error, Result};

/// The View rows both fogs read: the frame's, with the InvDeviceZToWorldZTransform of a projection without a far
/// plane (CreateInvDeviceZToWorldZTransform of the build's FReversedZPerspectiveMatrix, [2][2] 0: its subtract value
/// -1e-8), so a pixel nothing covers (device Z 0) is 1e8 cm away, as in the build, not the camera's far plane.
// Deviation: the camera's projection has a far plane (mvs-view camera.rs), so a covered pixel's depth reads about
// 9e-8 x depth^2 cm too far (0.09% at 100 m) where the build's is exact.
pub(super) fn fog_view(view: &[[u32; 4]; VIEW_ROWS]) -> Rows {
    let (at, _) = member("InvDeviceZToWorldZTransform").expect("the View's InvDeviceZToWorldZTransform");
    let mut rows = view.to_vec();
    rows[at / 16][3] = (-1.0e-8f32).to_bits();
    Rows(rows)
}

/// The SceneTexturesStruct resource the fog reads as the scene depth: SceneDepthTexture, FSceneTextureUniformParameters'
/// second member (its static init 140690280: SceneColorTexture, SceneDepthTexture, GBufferA to F, GBufferVelocity,
/// ScreenSpaceAOTexture, CustomDepthTexture, CustomStencilTexture, PointClampSampler).
const SCENE_DEPTH_RESOURCE: u16 = 1;

/// FExponentialHeightFogSceneInfo's terms of one fog: Density and HeightFalloff (the component's / 1000), Height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FogData {
    density: f32,
    falloff: f32,
    height: f32,
}

/// A stage's height fog (UExponentialHeightFogComponent's values).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HeightFog {
    fogs: [FogData; 2],
    inscatter: [f32; 3],
    max_opacity: f32,
    start_distance: f32,
    cutoff_distance: f32,
}

impl HeightFog {
    pub fn of(f: &mvs_data::stage::Fog) -> HeightFog {
        let c = |k: usize| f.inscatter.get(k).copied().unwrap_or(0.0) as f32;
        // The second fog (SecondFogData: Batcave, M010) as the first: density and falloff / 1000, its height the
        // component's plus FogHeightOffset. None stored: density 0, which adds nothing.
        // Unverified: 4.26's FExponentialHeightFogSceneInfo constructor, not read in the exe.
        let second =
            f.second.as_ref().map_or(FogData { density: 0.0, falloff: 0.0, height: f.height as f32 }, |s| FogData {
                density: (s.density / 1000.0) as f32,
                falloff: (s.height_falloff / 1000.0) as f32,
                height: (f.height + s.height_offset) as f32,
            });
        HeightFog {
            fogs: [
                FogData {
                    density: (f.density / 1000.0) as f32,
                    falloff: (f.height_falloff / 1000.0) as f32,
                    height: f.height as f32,
                },
                second,
            ],
            inscatter: [c(0), c(1), c(2)],
            max_opacity: f.max_opacity as f32,
            start_distance: f.start_distance as f32,
            cutoff_distance: f.cutoff_distance as f32,
        }
    }

    /// FogStruct's rows for a camera at height `camera_z` (cm): FFogUniformParameters (static init 140665a00)
    /// ExponentialFogParameters, ExponentialFogParameters2, ExponentialFogColorParameter, ExponentialFogParameters3;
    /// the rest (directional inscattering, the inscattering cubemap, volumetric fog) left 0.
    // Unverified: the values are 4.26's SetupFogUniformParameters (the observer height cap, the collapsed density's
    // exponent clamp), not read from the exe.
    pub(super) fn rows(&self, camera_z: f64) -> Rows {
        // The observer's height is capped where the fog's density would leave float range.
        let max_observer = self
            .fogs
            .iter()
            .filter(|f| f.falloff > 0.0)
            .map(|f| f.height + 65536.0 / f.falloff)
            .fold(f32::MAX, f32::min);
        let z = (camera_z as f32).min(max_observer);
        let collapsed = |f: &FogData| f.density * (-f.falloff * (z - f.height)).clamp(-125.0, 126.0).exp2();
        let [a, b] = &self.fogs;
        let mut r = Rows::new(10);
        r.f(0, [collapsed(a), a.falloff, max_observer, self.start_distance]);
        r.f(1, [collapsed(b), b.falloff, b.density, b.height]);
        r.f(2, [self.inscatter[0], self.inscatter[1], self.inscatter[2], 1.0 - self.max_opacity]);
        r.f(3, [a.density, a.height, 0.0, self.cutoff_distance]);
        r
    }
}

#[derive(Debug)]
pub(super) struct FogPass {
    vs: Global,
    ps: Global,
    /// The pixel shader's scene depth register.
    depth_register: u32,
    globals: Buffer,
    fog: Buffer,
}

impl FogPass {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<FogPass> {
        let ps =
            Global::new(gpu, assets, "TExponentialHeightFogPS<EHeightFogFeature::HeightFog>", 0, Frequency::Pixel)?;
        let rt = &ps.code.resource_table;
        let names = &ps.code.uniform_buffer_names;
        let depth = rt.slots(&rt.texture_map)?.into_iter().find(|s| {
            names.get(s.buffer as usize).is_some_and(|n| n == "SceneTexturesStruct")
                && s.resource == SCENE_DEPTH_RESOURCE
        });
        let depth_register = depth
            .ok_or_else(|| Error::Format {
                what: "TExponentialHeightFogPS".into(),
                why: "reads no SceneTexturesStruct scene depth".into(),
            })?
            .register as u32;
        Ok(FogPass {
            vs: Global::new(gpu, assets, "FHeightFogVS", 0, Frequency::Vertex)?,
            ps,
            depth_register,
            globals: constant_buffer(gpu)?,
            fog: constant_buffer(gpu)?,
        })
    }

    /// The fog over `fr`'s scene colour.
    pub fn draw(&self, fr: &Frame, view: &Buffer, quad: &Quad, states: &States, fog: &HeightFog) -> Result<()> {
        let gpu = fr.gpu;
        let mut g = Rows::new(2);
        // bOnlyOnRenderedOpaque (a pixel at the far depth is left as it is), bUseLinearDepthTexture.
        // Unverified: their places are 4.26's parameter order; the cooked shader carries no names to look them up by.
        g.f(0, [1.0, 0.0, 0.0, 0.0]);
        write(gpu, &self.globals, &g);
        write(gpu, &self.fog, &fog.rows(fr.input.camera.origin[2]));
        gpu.set_shaders(self.vs.id, Some(self.ps.id));
        gpu.set_states(states.fog_blend, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(view)], ..Default::default() }.set(gpu, Stage::Vertex);
        let white = fr.assets.white(TextureType::Texture2d);
        // t0 the scene depth, t1 the light shafts' occlusion (none: white), t2 the linear depth (not read).
        let mut srvs = vec![white.srv.clone(); 3];
        srvs[self.depth_register as usize] = fr.targets.depth.srv.clone();
        Binds {
            cbs: vec![Some(&self.globals), Some(view), Some(&self.fog)],
            srvs,
            samplers: vec![states.point_clamp, states.bilinear_clamp, states.point_clamp],
        }
        .set(gpu, Stage::Pixel);
        let size = fr.targets.size;
        // SAFETY: scene colour's render target view, no depth bound (the pass reads depth).
        unsafe { gpu.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&fr.targets.scene_color.rtv)), None) };
        gpu.viewport([0, 0, size[0], size[1]]);
        quad.draw(gpu, self.vs.id, true, 1)?;
        unbind(gpu);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_rows() {
        let f = mvs_data::stage::Fog {
            density: 0.05,
            height_falloff: 2.0,
            inscatter: vec![0.1, 0.2, 0.3],
            start_distance: 0.0,
            height: 100.0,
            max_opacity: 1.0,
            cutoff_distance: 0.0,
            second: None,
        };
        let r = HeightFog::of(&f).rows(100.0);
        let row0 = r.0[0].map(f32::from_bits);
        assert_eq!(row0[0], 0.00005);
        assert_eq!(row0[1], 0.002);
        assert_eq!(row0[2], 100.0 + 65536.0 / 0.002);
        assert_eq!(r.0[2].map(f32::from_bits)[3], 0.0);
        assert_eq!(r.0[1].map(f32::from_bits)[0], 0.0);
        // Batcave's SecondFogData: density 0.02, the falloff and offset unstored (0.2, 0)
        let second = mvs_data::stage::SecondFog { density: 0.02, height_falloff: 0.2, height_offset: 50.0 };
        let r = HeightFog::of(&mvs_data::stage::Fog { second: Some(second), ..f }).rows(100.0);
        assert_eq!(r.0[1].map(f32::from_bits), [0.00002 * (-0.0002f32 * -50.0).exp2(), 0.0002, 0.00002, 150.0]);
    }

    #[test]
    fn fog_view_sky_depth() {
        let mut view = [[0u32; 4]; VIEW_ROWS];
        let (at, _) = member("InvDeviceZToWorldZTransform").unwrap();
        view[at / 16] = [0.0f32, 0.0, 0.1, -1.0e-7].map(f32::to_bits);
        let t = fog_view(&view).0[at / 16].map(f32::from_bits);
        // device Z 0 (nothing drawn): 1 / (0 x [2] - [3])
        assert_eq!(1.0 / (0.0 * t[2] - t[3]), 1.0e8);
        assert_eq!(t[2], 0.1);
    }
}

/// The legacy AtmosphericFog pass: TAtmosphericFogPS<EAtmosphereRenderFlag::E_DisableAll> (no sun disk, no ground
/// scattering, no light shafts: the stages' RenderMask 3 without light shafts) with FAtmosphericVS, over the opaque
/// scene before the height fog. The shader reads the View's AtmosphericFog members (stage.rs atmosphere_members), the
/// scene depth (t0, SceneTexturesStruct's PointClampSampler s0) and the inscatter table (t1, s1).
// The exe's pass (141938d50, 14193d250) draws permutation 7 (E_DisableAll) when there is no light-shaft texture, with
// the blend One, SrcAlpha, colour writes RGB (144c3a058).
// Unverified: the order (before the height fog) is 4.26 RenderAtmosphere's.
#[derive(Debug)]
pub(super) struct AtmospherePass {
    vs: Global,
    ps: Global,
    depth_register: u32,
}

/// The View resource member the atmospheric fog's table is set under (stage.rs).
const INSCATTER: &str = "AtmosphereInscatterTexture";

impl AtmospherePass {
    pub fn new(gpu: &Gpu, assets: &Assets) -> Result<AtmospherePass> {
        let ps =
            Global::new(gpu, assets, "TAtmosphericFogPS<EAtmosphereRenderFlag::E_DisableAll>", 0, Frequency::Pixel)?;
        let rt = &ps.code.resource_table;
        let names = &ps.code.uniform_buffer_names;
        let depth = rt.slots(&rt.texture_map)?.into_iter().find(|s| {
            names.get(s.buffer as usize).is_some_and(|n| n == "SceneTexturesStruct")
                && s.resource == SCENE_DEPTH_RESOURCE
        });
        let depth_register = depth
            .ok_or_else(|| Error::Format {
                what: "TAtmosphericFogPS".into(),
                why: "reads no SceneTexturesStruct scene depth".into(),
            })?
            .register as u32;
        Ok(AtmospherePass { vs: Global::new(gpu, assets, "FAtmosphericVS", 0, Frequency::Vertex)?, ps, depth_register })
    }

    /// The atmospheric fog over `fr`'s scene colour when the stage set its table; false without one.
    pub fn draw(&self, fr: &Frame, view: &Buffer, quad: &Quad, states: &States) -> Result<bool> {
        let Some((_, table)) = fr.scene.view_textures.iter().rev().find(|(n, _)| *n == INSCATTER) else {
            return Ok(false);
        };
        let gpu = fr.gpu;
        gpu.set_shaders(self.vs.id, Some(self.ps.id));
        gpu.set_states(states.fog_blend, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(view)], ..Default::default() }.set(gpu, Stage::Vertex);
        // t0 the scene depth, t1 the inscatter table (the depth's register checked, the table's the parameter map's).
        let mut srvs = vec![None, table.srv.clone()];
        srvs[self.depth_register as usize] = fr.targets.depth.srv.clone();
        // Unverified: the table's sampler bilinear and clamped (4.26's AtmosphereInscatterTextureSampler).
        Binds { cbs: vec![Some(view)], srvs, samplers: vec![states.point_clamp, states.bilinear_clamp] }
            .set(gpu, Stage::Pixel);
        let size = fr.targets.size;
        // SAFETY: scene colour's render target view, no depth bound (the pass reads depth).
        unsafe { gpu.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&fr.targets.scene_color.rtv)), None) };
        gpu.viewport([0, 0, size[0], size[1]]);
        quad.draw(gpu, self.vs.id, true, 1)?;
        unbind(gpu);
        Ok(true)
    }
}
