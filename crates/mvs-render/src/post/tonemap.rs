//! The grading LUT (FLUTBlenderPS into a 32^3 volume through FWriteToSliceVS / GS), the tonemapper (FTonemapVS +
//! FTonemapPS) and FXAA (FFXAAVS + FFXAAPS), with this build's FPostProcessSettings defaults (render/post).

use mvs_material::Frequency;
use windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::pass::{
    Binds, Global, Quad, Rows, Stage, States, bilinear_range, constant_buffer, draw_rect, set_target, unbind, write,
};
use crate::Result;
use crate::assets::Assets;
use crate::gpu::{Buffer, Gpu, Subresource, TexDesc, TexKind, TexViews, Texture};

/// The grading LUT's side.
const LUT_SIZE: u32 = 32;
/// FPostProcessSettings VignetteIntensity's default (exe constructor FUN_142d3fd30, offset 0x400).
pub(crate) const VIGNETTE_INTENSITY: f32 = 0.4;
/// r.PostProcessAAQuality of each sg.AntiAliasingQuality level (BaseScalability's `[AntiAliasingQuality@<n>]`, the last
/// level `@Cine`).
const AA_QUALITY: [i32; 5] = [0, 2, 3, 4, 6];

/// FFXAAPS's permutation of a sg.AntiAliasingQuality level: GetFXAAQuality (141bb0130) = clamp(r.PostProcessAAQuality,
/// 0, 5); 0 is no FXAA pass (the view's method None), 1 and 2 the console shader, 3 to 5 FXAA 3.11 quality with
/// longer searches.
// Deviation: the exe starts at level 3 (cvar default 140728120, FQualityLevels::SetDefaults 142d5c770) and the
// first-run autodetect (PreferencesManager AutodetectGraphicsSettings, a GraphicsPresetByGPU row) replaces it; the
// port runs none, so the settings' default is level 3 (permutation 4).
pub(crate) fn fxaa_permutation(level: u8) -> i32 {
    AA_QUALITY[usize::from(level).min(AA_QUALITY.len() - 1)].clamp(0, 5)
}

/// The settings' default anti-aliasing level (mvs_settings::GraphicsSettings).
pub(crate) const DEFAULT_AA_LEVEL: u8 = 3;

/// The CombineLUTs settings buffer (FLUTBlenderPS's $Globals, 67 rows): this build's FPostProcessSettings defaults,
/// no overrides but the view's OverlayColor, black at `overlay` alpha (a camera fade; 0 none).
fn lut_settings(overlay: f32) -> Rows {
    let mut b = Rows::new(67);
    b.f(26, [0.0, 1.0, 0.0, 0.0]); // MappingPolynomial
    b.f(27, [1.0 / 2.2, 1.0, 1.0 / 2.2, 0.0]); // InverseGamma
    b.f(36, [0.88, 0.55, 0.26, 0.0]); // FilmSlope, FilmToe, FilmShoulder, FilmBlackClip
    b.f(37, [0.04, 0.0, 0.0, 0.0]); // FilmWhiteClip
    b.f(42, [0.0, 1.0, 1.0, 1.0]); // ColorScale
    b.f(43, [0.0, 0.0, 0.0, overlay]); // OverlayColor
    b.f(44, [0.0, 6500.0, 0.0, 0.0]); // the legacy curve flag, WhiteTemp, WhiteTint
    for s in 45..=64 {
        b.f(s, if (s - 45) % 5 == 4 { [0.0; 4] } else { [1.0; 4] }); // Saturation .. Offset per range
    }
    b.f(65, [0.09, 0.5, 0.0, 0.0]); // ColorCorrectionShadowsMax, HighlightsMin
    b.put_u(65 * 16 + 8, &[0, 0]); // OutputDevice (sRGB), OutputGamut
    b.f(66, [0.6, 1.0, 1.0, 0.0]); // BlueCorrection, ExpandGamut, ToneCurveAmount
    b
}

/// The tonemapper's $Globals (68 rows, FTonemapVS's and FTonemapPS's): `size` the output's, `bloom` the bloom
/// level's size, `grain` GrainRandomFull, `vignette` VignetteIntensity.
fn tonemap_globals(size: [u32; 2], bloom: [u32; 2], bloom_intensity: f32, grain: [f32; 2], vignette: f32) -> Rows {
    let mut b = Rows::new(68);
    let [w, h] = size.map(|x| x as f32);
    let r = bilinear_range(bloom);
    b.f(37, [0.0, 2.0, -2.0, 0.0]); // the viewport UV to screen position scale (.yz)
    b.f(38, [1.0 / w, 1.0 / h, 1.0, 1.0]);
    b.f(39, [1.0, -1.0, 0.0, 0.0]); // and bias
    b.f(50, [0.0, 0.0, r[0], r[1]]); // the bloom UV's clamp min (.zw)
    b.f(51, [r[2], r[3], 0.0, 0.0]); // and max (.xy)
    b.f(54, [0.0, 0.0, w, h]); // the output viewport's size (.zw)
    b.f(55, [1.0 / w, 1.0 / h, 0.0, 0.0]); // and its inverse
    b.f(58, [0.0, 0.0, 1.0, 1.0]); // the scene UV to bloom UV scale (.zw)
    b.f(59, [0.0; 4]); // and bias
    b.f(60, [1.0, 1.0, 1.0, 0.0]); // ColorScale0
    b.f(61, [bloom_intensity; 4]); // ColorScale1 (BloomIntensity)
    b.f(62, [vignette, 0.0, 0.0, 0.0]);
    b.f(63, [grain[0], grain[1], 0.0, 0.0]); // GrainRandomFull
    b.f(65, [0.0, 1.0, 0.0, 0.0]); // .x the HDR output branch off, .y the fixed exposure (1)
    b.f(66, [0.0; 4]); // BloomDirtMaskTint x intensity
    b.f(67, [0.0, 0.0, 1.0, 1.0]); // the screen position transform the dirt mask takes
    b
}

#[derive(Debug)]
pub(super) struct Tonemap {
    /// The grading LUT (PF_A2B10G10R10).
    pub lut: Texture,
    /// The OverlayColor alpha the LUT was drawn with.
    overlay: f32,
    lut_vs: Global,
    lut_gs: Global,
    lut_ps: Global,
    tone_vs: Global,
    tone_ps: Global,
    fxaa_vs: Global,
    /// FFXAAPS of the level's permutation (none: no FXAA pass), with the permutation.
    fxaa_ps: Option<(i32, Global)>,
    /// The bloom dirt mask (none: black).
    black: Texture,
    vs_globals: Buffer,
    ps_globals: Buffer,
    rect: Buffer,
}

impl Tonemap {
    /// The shaders and the LUT, drawn with the defaults every view takes (drawn again when a camera fade changes it).
    pub fn new(gpu: &Gpu, assets: &Assets, quad: &Quad, states: &States) -> Result<Tonemap> {
        let lut = gpu.texture(
            &TexDesc {
                kind: TexKind::D3,
                size: [LUT_SIZE; 3],
                mips: 1,
                format: DXGI_FORMAT_R10G10B10A2_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[],
        )?;
        let black = gpu.texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [1, 1, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, ..Default::default() },
            },
            &[Subresource { data: &[0, 0, 0, 0], row_pitch: 4, slice_pitch: 4 }],
        )?;
        let t = Tonemap {
            lut,
            overlay: 0.0,
            lut_vs: Global::new(gpu, assets, "FWriteToSliceVS", 0, Frequency::Vertex)?,
            lut_gs: Global::new(gpu, assets, "FWriteToSliceGS", 0, Frequency::Geometry)?,
            lut_ps: Global::new(gpu, assets, "FLUTBlenderPS", 0, Frequency::Pixel)?,
            tone_vs: Global::new(gpu, assets, "FTonemapVS", 0, Frequency::Vertex)?,
            // Unverified: permutation 521 (sRGB output, grain and vignette on, no other feature), not
            // read from the exe's permutation choice.
            tone_ps: Global::new(gpu, assets, "FTonemapPS", 521, Frequency::Pixel)?,
            fxaa_vs: Global::new(gpu, assets, "FFXAAVS", 0, Frequency::Vertex)?,
            fxaa_ps: Some((
                fxaa_permutation(DEFAULT_AA_LEVEL),
                Global::new(gpu, assets, "FFXAAPS", fxaa_permutation(DEFAULT_AA_LEVEL), Frequency::Pixel)?,
            )),
            black,
            vs_globals: constant_buffer(gpu)?,
            ps_globals: constant_buffer(gpu)?,
            rect: constant_buffer(gpu)?,
        };
        t.draw_lut(gpu, quad, states)?;
        Ok(t)
    }

    /// The view's camera fade (black at `alpha`): the LUT drawn again when it changed (CombineLUTs mixes
    /// OverlayColor in, after the film curve).
    pub fn set_overlay(&mut self, gpu: &Gpu, quad: &Quad, states: &States, alpha: f32) -> Result<()> {
        if alpha == self.overlay {
            return Ok(());
        }
        self.overlay = alpha;
        self.draw_lut(gpu, quad, states)
    }

    /// FLUTBlenderPS over every slice of the volume: FWriteToSliceVS's instance is the slice, FWriteToSliceGS sends
    /// it to the target's array index.
    fn draw_lut(&self, gpu: &Gpu, quad: &Quad, states: &States) -> Result<()> {
        let (vs, gs, ps) = (&self.lut_vs, &self.lut_gs, &self.lut_ps);
        let mut slice = Rows::new(2);
        slice.put_u(0, &[0]); // MinZ
        slice.f(1, [1.0, 1.0, 0.0, 0.0]); // UVScaleBias
        write(gpu, &self.vs_globals, &slice);
        write(gpu, &self.ps_globals, &lut_settings(self.overlay));
        gpu.set_shaders(vs.id, Some(ps.id));
        gpu.set_geometry_shader(Some(gs.id));
        gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(&self.vs_globals)], ..Default::default() }.set(gpu, Stage::Vertex);
        Binds { cbs: vec![Some(&self.ps_globals)], ..Default::default() }.set(gpu, Stage::Pixel);
        set_target(gpu, &self.lut, [LUT_SIZE, LUT_SIZE]);
        quad.draw(gpu, vs.id, true, LUT_SIZE)?;
        gpu.set_geometry_shader(None);
        unbind(gpu);
        Ok(())
    }

    /// The tonemapper from `scene` (HDR) and `bloom` into `ldr` (sRGB, its luma in alpha for FXAA), with the view's
    /// VignetteIntensity `vignette`.
    #[allow(clippy::too_many_arguments)]
    pub fn tonemap(
        &self,
        gpu: &Gpu,
        quad: &Quad,
        states: &States,
        view: &Buffer,
        scene: &Texture,
        bloom: Option<&Texture>,
        bloom_intensity: f32,
        grain: [f32; 2],
        vignette: f32,
        ldr: &Texture,
    ) -> Result<()> {
        let size = [ldr.desc.size[0], ldr.desc.size[1]];
        let bloom_size = bloom.map_or(size, |b| [b.desc.size[0], b.desc.size[1]]);
        let g = tonemap_globals(size, bloom_size, if bloom.is_some() { bloom_intensity } else { 0.0 }, grain, vignette);
        write(gpu, &self.ps_globals, &g);
        write(gpu, &self.rect, &draw_rect(size, size));
        gpu.set_shaders(self.tone_vs.id, Some(self.tone_ps.id));
        gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(&self.ps_globals), Some(&self.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        let black: Option<ID3D11ShaderResourceView> = self.black.srv.clone();
        Binds {
            cbs: vec![Some(&self.ps_globals), Some(view)],
            srvs: vec![scene.srv.clone(), bloom.map_or(black.clone(), |b| b.srv.clone()), black, self.lut.srv.clone()],
            samplers: vec![states.bilinear_clamp; 4],
        }
        .set(gpu, Stage::Pixel);
        set_target(gpu, ldr, size);
        quad.draw(gpu, self.tone_vs.id, false, 1)?;
        unbind(gpu);
        Ok(())
    }

    /// FFXAAPS of anti-aliasing `level`'s permutation (level 0: no FXAA pass).
    pub fn set_aa(&mut self, gpu: &Gpu, assets: &Assets, level: u8) -> Result<()> {
        let perm = fxaa_permutation(level);
        if perm == 0 {
            self.fxaa_ps = None;
        } else if self.fxaa_ps.as_ref().map(|f| f.0) != Some(perm) {
            self.fxaa_ps = Some((perm, Global::new(gpu, assets, "FFXAAPS", perm, Frequency::Pixel)?));
        }
        Ok(())
    }

    /// Whether the level draws an FXAA pass.
    pub fn has_fxaa(&self) -> bool {
        self.fxaa_ps.is_some()
    }

    /// FXAA from `ldr` into `out`: the pass constants AddFXAAPass (141b47370) writes whatever the permutation (the
    /// shader reads the ones it has): the quality shaders' subpix 0.75, edge threshold 0.166 and its minimum 0.0833,
    /// the console shader's frame offsets (-0.5, 0.5 and -2, 2 of the output's texel), edge sharpness 8, threshold
    /// 0.125 and minimum 0.05. Without an FXAA pass (level 0) nothing is drawn.
    pub fn fxaa(&self, gpu: &Gpu, quad: &Quad, states: &States, ldr: &Texture, out: &Texture) -> Result<()> {
        let Some((_, ps)) = &self.fxaa_ps else { return Ok(()) };
        let size = [out.desc.size[0], out.desc.size[1]];
        let inv = [1.0 / size[0] as f32, 1.0 / size[1] as f32];
        let mut vs = Rows::new(1);
        vs.put(self.fxaa_vs.at(8), &inv);
        let mut pr = Rows::new(16);
        let r = bilinear_range(size);
        let mut put = |member: u32, v: &[f32]| {
            if let Some(at) = ps.find(member) {
                pr.put(at, v);
            }
        };
        put(8, &inv);
        put(96, &r[..2]);
        put(104, &r[2..]);
        put(128, &[-0.5 * inv[0], -0.5 * inv[1], 0.5 * inv[0], 0.5 * inv[1]]);
        put(144, &[-2.0 * inv[0], -2.0 * inv[1], 2.0 * inv[0], 2.0 * inv[1]]);
        put(160, &[0.75]);
        put(164, &[0.166]);
        put(168, &[0.0833]);
        put(172, &[8.0]);
        put(176, &[0.125]);
        put(180, &[0.05]);
        write(gpu, &self.vs_globals, &vs);
        write(gpu, &self.ps_globals, &pr);
        write(gpu, &self.rect, &draw_rect(size, size));
        gpu.set_shaders(self.fxaa_vs.id, Some(ps.id));
        gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(&self.vs_globals), Some(&self.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        Binds { cbs: vec![Some(&self.ps_globals)], srvs: vec![ldr.srv.clone()], samplers: vec![states.bilinear_clamp] }
            .set(gpu, Stage::Pixel);
        set_target(gpu, out, size);
        quad.draw(gpu, self.fxaa_vs.id, false, 1)?;
        unbind(gpu);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_pick_permutations() {
        // sg.AntiAliasingQuality 0..4 -> r.PostProcessAAQuality 0 / 2 / 3 / 4 / 6, clamped to the FXAA shaders' 0..5.
        let perms: Vec<i32> = (0..5).map(fxaa_permutation).collect();
        assert_eq!(perms, [0, 2, 3, 4, 5]);
        assert_eq!(fxaa_permutation(DEFAULT_AA_LEVEL), 4);
    }
}
