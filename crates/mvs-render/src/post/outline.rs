//! A post-process material blendable (BL_BeforeTonemapping): the stage's PostProcessVolume material drawn with its
//! cooked FPostProcessMaterialVS / PS over scene colour (PostProcessInput_0) into a target of its own. The stages'
//! one blendable is the team outline, ColoredOutlines_Inst, which reads the custom stencil.

use mvs_material::PreshaderInputs;
use windows::Win32::Graphics::Direct3D11::{ID3D11DepthStencilView, ID3D11ShaderResourceView};

use super::pass::{Binds, Quad, Rows, Stage, States, constant_buffer, draw_rect, set_target, unbind, write};
use crate::assets::Assets;
use crate::gpu::{Buffer, Gpu, ShaderId, StateId, Texture};
use crate::material::MaterialInstance;
use crate::{Error, Result};

/// The SceneTexturesStruct resource the outline reads as the custom stencil (a uint texture, the stencil in .g).
// CustomStencilTexture: FSceneTextureUniformParameters' twelfth member (static init 140690280, fog.rs SCENE_DEPTH_RESOURCE).
const CUSTOM_STENCIL_RESOURCE: u16 = 11;

/// The material's parameters at their defaults (the instance's values), for its preshaders.
struct Defaults<'a>(&'a mvs_material::Material);

impl PreshaderInputs for Defaults<'_> {
    fn scalar(&self, k: usize) -> f64 {
        self.0.uniforms.scalars[k].value
    }
    fn vector(&self, k: usize) -> [f64; 4] {
        self.0.uniforms.vectors[k].value
    }
    fn texture_size(&self, k: usize) -> Option<[f64; 4]> {
        self.0.uniforms.texture_sizes.as_ref()?.get(k).copied().flatten()
    }
}

/// Where a pixel shader register's constant buffer comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cb {
    Globals,
    View,
    Material,
    Collection(usize),
    None,
}

#[derive(Debug)]
pub(super) struct Blendable {
    pub path: String,
    inst: MaterialInstance,
    vs: ShaderId,
    ps: ShaderId,
    /// The pixel shader's $Globals byte offsets: PostProcessOutput ViewportMin, ViewportSizeInverse;
    /// PostProcessInput_0 ViewportSize, UVViewportMin, UVViewportSize.
    globals: [u32; 5],
    cbs: Vec<Cb>,
    /// The custom stencil's register, PostProcessInput_0's.
    stencil_register: Option<u32>,
    input_register: u32,
    /// The pixel shader's sampler registers (PostProcessInput_0Sampler).
    samplers: u32,
    /// The largest of the instance's outline widths (its scalar parameters named ...Width).
    max_width: f32,
    ps_globals: Buffer,
    rect: Buffer,
}

impl Blendable {
    /// Its instance's buffers of the collection at `path` rewritten (a collection set at run time).
    pub fn set_collection(&mut self, gpu: &Gpu, path: &str, rows: &[[u32; 4]]) {
        self.inst.set_collection(gpu, path, rows);
    }

    pub fn new(gpu: &Gpu, assets: &Assets, path: &str) -> Result<Blendable> {
        let bad = |why: String| Error::Format { what: path.to_string(), why };
        let inst = MaterialInstance::new(gpu, assets, path)?;
        let m = inst.material.clone();
        if let Some(b) = &inst.buffer {
            let rows = m.uniform_rows(&Defaults(&m))?;
            let mut out = Rows::new((b.desc.size / 16) as usize);
            for (o, r) in out.0.iter_mut().zip(&rows) {
                *o = r.map(|x| (x as f32).to_bits());
            }
            gpu.write_rows(b, &out.0);
        }
        let (_, rows) = m
            .shaders
            .pass(None, None, &[("FPostProcessMaterialVS", 0), ("FPostProcessMaterialPS", 0)])
            .ok_or_else(|| bad("no FPostProcessMaterialVS / PS".into()))?;
        let (vs_sha, ps_sha) = (m.shaders.code(rows[0]), m.shaders.code(rows[1]));
        let vs_code = assets.code(vs_sha)?;
        let ps_code = assets.code(ps_sha)?;
        let vs = gpu.shader_by(vs_sha, &vs_code)?;
        let ps = gpu.shader_by(ps_sha, &ps_code)?;
        let params = m.shaders.params(rows[1]);
        let at = |member: u32| {
            params
                .bindings
                .parameters
                .iter()
                .find(|p| p.byte_offset == member)
                .map(|p| p.base_index)
                .ok_or_else(|| bad(format!("the pixel shader reads no parameter at member offset {member}")))
        };
        let globals = [at(64)?, at(88)?, at(192)?, at(208)?, at(224)?];
        let names = &ps_code.uniform_buffer_names;
        let cbs = names
            .iter()
            .enumerate()
            .map(|(r, n)| match n.as_str() {
                "" if r == 0 => Cb::Globals,
                "View" => Cb::View,
                "Material" => Cb::Material,
                n => n.strip_prefix("MaterialCollection").and_then(|k| k.parse().ok()).map_or(Cb::None, Cb::Collection),
            })
            .collect();
        let rt = &ps_code.resource_table;
        let mut stencil_register = None;
        let mut table = Vec::new();
        for map in [&rt.texture_map, &rt.srv_map] {
            for s in rt.slots(map)? {
                table.push(s.register as u32);
                let name = names.get(s.buffer as usize).map_or("", String::as_str);
                if name != "SceneTexturesStruct" || s.resource != CUSTOM_STENCIL_RESOURCE {
                    return Err(bad(format!("reads {name} resource {}, not the custom stencil", s.resource)));
                }
                stencil_register = Some(s.register as u32);
            }
        }
        // PostProcessInput_0: the parameter map's SRV outside the resource table.
        let input_register = params
            .parameter_map
            .srvs
            .iter()
            .map(|p| p.base_index)
            .find(|r| !table.contains(r))
            .ok_or_else(|| bad("no PostProcessInput_0 texture".into()))?;
        let max_width =
            m.uniforms.scalars.iter().filter(|s| s.name.ends_with("Width")).map(|s| s.value as f32).fold(0.0, f32::max);
        let samplers = params.parameter_map.texture_samplers.iter().map(|p| p.base_index + 1).max().unwrap_or(0);
        Ok(Blendable {
            path: path.to_string(),
            inst,
            vs,
            ps,
            globals,
            cbs,
            stencil_register,
            input_register,
            samplers,
            max_width,
            ps_globals: constant_buffer(gpu)?,
            rect: constant_buffer(gpu)?,
        })
    }

    /// A bound of the scan's reach in pixels at a view of `size` (scan_radius).
    pub fn scan_radius(&self, size: [u32; 2]) -> u32 {
        scan_radius(size, self.max_width)
    }

    /// Whether the skip may draw this blendable on the covered tiles only: it reads the custom stencil and its
    /// widths were found (else it draws over every pixel).
    pub fn skippable(&self) -> bool {
        self.stencil_register.is_some() && self.max_width > 0.0
    }

    /// The material over `input` into `out` (both the view's size); `stencil` the custom stencil's view; `test` a
    /// depth-stencil view and its state the draw is tested against (the skip's covered tiles), with reference 1.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        gpu: &Gpu,
        quad: &Quad,
        states: &States,
        view: &Buffer,
        input: &Texture,
        stencil: &ID3D11ShaderResourceView,
        out: &Texture,
        test: Option<(&ID3D11DepthStencilView, StateId)>,
    ) -> Result<()> {
        let size = [out.desc.size[0], out.desc.size[1]];
        let [w, h] = size.map(|x| x as f32);
        let mut g = Rows::new(super::pass::CB_ROWS);
        let [out_min, out_inv, in_size, in_uv_min, in_uv_size] = self.globals;
        g.put_u(out_min, &[0, 0]);
        g.put(out_inv, &[1.0 / w, 1.0 / h]);
        g.put(in_size, &[w, h]);
        g.put(in_uv_min, &[0.0, 0.0]);
        g.put(in_uv_size, &[1.0, 1.0]);
        write(gpu, &self.ps_globals, &g);
        write(gpu, &self.rect, &draw_rect(size, size));
        let cbs: Vec<Option<&Buffer>> = self
            .cbs
            .iter()
            .map(|c| match c {
                Cb::Globals => Some(&self.ps_globals),
                Cb::View => Some(view),
                Cb::Material => self.inst.buffer.as_ref(),
                Cb::Collection(k) => self.inst.collections.get(*k),
                Cb::None => None,
            })
            .collect();
        let n = self.input_register.max(self.stencil_register.unwrap_or(0)) as usize + 1;
        let mut srvs: Vec<Option<ID3D11ShaderResourceView>> = vec![None; n];
        srvs[self.input_register as usize] = input.srv.clone();
        if let Some(r) = self.stencil_register {
            srvs[r as usize] = Some(stencil.clone());
        }
        // Unverified: PostProcessInput_0's sampler point and clamped (4.26's PostProcessMaterial input), not read
        // from the exe.
        let samplers = vec![states.point_clamp; self.samplers as usize];
        gpu.set_shaders(self.vs, Some(self.ps));
        match test {
            Some((_, depth)) => gpu.set_states(states.opaque, states.cull_none, depth, 1),
            None => gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0),
        }
        Binds { cbs: vec![Some(&self.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        Binds { cbs, srvs, samplers }.set(gpu, Stage::Pixel);
        match test {
            Some((dsv, _)) => {
                // SAFETY: a render target view and a depth-stencil view of this device, both of `size`.
                unsafe { gpu.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&out.rtv)), dsv) };
                gpu.viewport([0, 0, size[0], size[1]]);
            }
            None => set_target(gpu, out, size),
        }
        quad.draw(gpu, self.vs, false, 1)?;
        unbind(gpu);
        Ok(())
    }
}

/// A bound of the outline scan's reach in pixels at a view of `size` for the instance's largest width: every custom
/// stencil texel the material reads for a pixel lies within it on both axes (ColoredOutlines PS lines 43-70,
/// render/post outlineScan): M the view's larger side, K = M / 2560 and R = round(K + ceil(max width)) from
/// M = 1843.2, else K = M / 4096 and R = max(round(K ceil(max width)), 1). Taken up (ceil, not round): a tie's
/// rounding is the GPU's (the shader's mad may be fused).
fn scan_radius(size: [u32; 2], max_width: f32) -> u32 {
    let m = size[0].max(size[1]) as f32;
    let top = max_width.ceil();
    let r = if m >= 1843.2 { m * (1.0 / 2560.0) + top } else { (m * (1.0 / 4096.0) * top).max(1.0) };
    r.ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::scan_radius;

    /// render/post outlineScan's R (1280: 2, 1920: 8, 2560: 8) or one above where the bound takes it up.
    #[test]
    fn scan_radius_bounds_the_page() {
        assert_eq!(scan_radius([1920, 1080], 6.5), 8);
        assert_eq!(scan_radius([2560, 1440], 6.5), 8);
        assert_eq!(scan_radius([1280, 720], 6.5), 3);
        assert_eq!(scan_radius([640, 360], 6.5), 2);
    }
}
