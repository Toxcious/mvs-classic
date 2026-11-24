//! The post chain's shared parts: a global shader by its type, the loose ($Globals) rows it reads, the screen quad
//! the build's DrawRectangle and screen-pass shaders take, the states and samplers, a full-screen draw.

use std::sync::Arc;

use mvs_material::{Frequency, ShaderCode};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::assets::Assets;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, InputElement, RasterKey, SamplerKey, ShaderId,
    StateId, StateKey, TexDesc, TexKind, TexViews, Texture,
};
use crate::{Error, Result};

/// A global shader of the store's global shader cache, created, with its loose parameters' places.
#[derive(Debug)]
pub(super) struct Global {
    pub id: ShaderId,
    pub code: Arc<ShaderCode>,
    /// (the shader parameter struct's member offset, its byte offset in $Globals).
    pub loose: Vec<(u32, u32)>,
}

impl Global {
    /// The global shader `ty` permutation `perm` at `freq`.
    pub fn new(gpu: &Gpu, assets: &Assets, ty: &str, perm: i32, freq: Frequency) -> Result<Global> {
        let g = assets.global_shaders();
        let row = g.find(ty, perm, freq).ok_or_else(|| Error::Format {
            what: "global shader cache".into(),
            why: format!("no {ty} permutation {perm} at {freq:?}"),
        })?;
        let sha = g.shaders.code(row);
        let code = assets.code(sha)?;
        let id = gpu.shader_by(sha, &code)?;
        let loose = g.shaders.params(row).bindings.parameters.iter().map(|p| (p.byte_offset, p.base_index)).collect();
        Ok(Global { id, code, loose })
    }

    /// The $Globals byte offset of the parameter struct's member at `member`; None when the shader does not read it.
    pub fn find(&self, member: u32) -> Option<u32> {
        self.loose.iter().find(|l| l.0 == member).map(|l| l.1)
    }

    /// The $Globals byte offset of the parameter struct's member at `member` (a member the shader reads).
    pub fn at(&self, member: u32) -> u32 {
        self.loose
            .iter()
            .find(|l| l.0 == member)
            .unwrap_or_else(|| panic!("the shader reads no parameter at member offset {member}"))
            .1
    }
}

/// A $Globals or uniform buffer's rows being filled.
#[derive(Clone, Debug)]
pub(super) struct Rows(pub Vec<[u32; 4]>);

impl Rows {
    pub fn new(rows: usize) -> Rows {
        Rows(vec![[0; 4]; rows])
    }

    /// A whole row of floats.
    pub fn f(&mut self, row: usize, v: [f32; 4]) {
        self.0[row] = v.map(f32::to_bits);
    }

    /// Floats from byte offset `byte`.
    pub fn put(&mut self, byte: u32, v: &[f32]) {
        self.put_u(byte, &v.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    }

    /// Uints from byte offset `byte`.
    pub fn put_u(&mut self, byte: u32, v: &[u32]) {
        for (k, x) in v.iter().enumerate() {
            let w = byte as usize / 4 + k;
            self.0[w / 4][w % 4] = *x;
        }
    }
}

/// The rows of a constant buffer the passes rewrite before each draw.
pub(super) const CB_ROWS: usize = 128;

/// A dynamic constant buffer of `CB_ROWS` rows.
pub(super) fn constant_buffer(gpu: &Gpu) -> Result<Buffer> {
    gpu.buffer(&BufDesc { kind: BufKind::Constant, size: CB_ROWS as u32 * 16, dynamic: true }, None)
}

/// Writes `rows` into `b` (the rest of the buffer zero).
pub(super) fn write(gpu: &Gpu, b: &Buffer, rows: &Rows) {
    let mut all = rows.0.clone();
    all.resize((b.desc.size / 16) as usize, [0; 4]);
    gpu.write_rows(b, &all);
}

/// DrawRectangleParameters of a whole `target`-sized rectangle drawn from the unit quad over a `texture`-sized
/// input: PosScaleBias, UVScaleBias, InvTargetSizeAndTextureSize.
pub(super) fn draw_rect(target: [u32; 2], texture: [u32; 2]) -> Rows {
    let [w, h] = target.map(|x| x as f32);
    let [tw, th] = texture.map(|x| x as f32);
    let mut b = Rows::new(3);
    b.f(0, [w, h, 0.0, 0.0]);
    b.f(1, [tw, th, 0.0, 0.0]);
    b.f(2, [1.0 / w, 1.0 / h, 1.0 / tw, 1.0 / th]);
    b
}

/// GetScreenPassTextureViewportParameters (141d03d20) of a view filling its `size` texture: the bilinear UV range
/// [0.5 / E, (v - 0.5) / E] as (min.xy, max.zw).
pub(super) fn bilinear_range(size: [u32; 2]) -> [f32; 4] {
    let [w, h] = size.map(|x| x as f32);
    [0.5 / w, 0.5 / h, (w - 0.5) / w, (h - 0.5) / h]
}

/// FScreenPassVS's position alone (SV_POSITION in o0), the port's own: the cooked FScreenPassVS writes TEXCOORD0 to o0
/// and SV_POSITION to o1, which does not link to a pixel shader that reads SV_POSITION alone in v0 (FDownsamplePS:
/// the D3D11 debug layer's "Semantic 'SV_POSITION' is defined for mismatched hardware registers"). The same
/// DrawRectangle math as its disassembly: (pos x PosScaleBias.xy + .zw) x InvTargetSize, to clip with Y flipped,
/// z and w passed on.
// Deviation: the build draws its downsample with the cooked FScreenPassVS (AddDrawScreenPass's), whose linkage the
// driver resolves by the system value; the port's VS gives the same positions.
pub(crate) const SCREEN_POSITION_VS: &str = r"
cbuffer DrawRectangleParameters : register(b0) {
    float4 PosScaleBias;
    float4 UVScaleBias;
    float4 InvTargetSizeAndTextureSize;
};
float4 MainVS(float4 InPosition : ATTRIBUTE0, float2 InUV : ATTRIBUTE1) : SV_POSITION {
    float2 p = (InPosition.xy * PosScaleBias.xy + PosScaleBias.zw) * InvTargetSizeAndTextureSize.xy;
    p = (p * 2.0 - 1.0) * float2(1.0, -1.0);
    return float4(p, InPosition.zw);
}
";

/// The port's position-only screen vertex shader (SCREEN_POSITION_VS), compiled and made on `gpu`.
pub(super) fn screen_position_vs(gpu: &Gpu) -> Result<ShaderId> {
    let dxbc = crate::lighting::compile(SCREEN_POSITION_VS, "MainVS", "vs_5_0")?;
    let empty = mvs_material::shader::ResourceTable {
        bits: 0,
        srv_map: Vec::new(),
        sampler_map: Vec::new(),
        uav_map: Vec::new(),
        layout_hashes: Vec::new(),
        texture_map: Vec::new(),
    };
    gpu.shader(&ShaderCode {
        frequency: Frequency::Vertex,
        resource_table: empty,
        ray_entry_points: Vec::new(),
        global_uniform_buffer_used: false,
        output_mask: 0,
        uniform_buffer_names: Vec::new(),
        vendor_extensions: None,
        no_create: false,
        dxbc,
    })
}

const ATTRIBUTE: &std::ffi::CStr = c"ATTRIBUTE";

/// The input layout's key: ATTRIBUTE0 float4 in slot 0, ATTRIBUTE1 float2 in slot 1.
const LAYOUT_KEY: u32 = 0x706f_7374;

/// Two triangles over a rectangle: the unit square's corners (the DrawRectangle shaders' positions, 0 to 1),
/// the same in clip space (the screen-rectangle shaders' positions) and their UVs.
#[derive(Debug)]
pub(super) struct Quad {
    unit: Buffer,
    clip: Buffer,
    uv: Buffer,
}

impl Quad {
    pub fn new(gpu: &Gpu) -> Result<Quad> {
        let corners: [[f32; 2]; 6] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let bytes = |v: Vec<f32>| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let unit = bytes(corners.iter().flat_map(|c| [c[0], c[1], 0.0, 1.0]).collect());
        let clip = bytes(corners.iter().flat_map(|c| [c[0] * 2.0 - 1.0, 1.0 - c[1] * 2.0, 0.0, 1.0]).collect());
        let uv = bytes(corners.iter().flatten().copied().collect());
        let vb =
            |b: &[u8]| gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: b.len() as u32, dynamic: false }, Some(b));
        Ok(Quad { unit: vb(&unit)?, clip: vb(&clip)?, uv: vb(&uv)? })
    }

    /// Draws the quad `instances` times with `vs`'s input layout: `clip` the clip-space positions, else the unit
    /// square's.
    pub fn draw(&self, gpu: &Gpu, vs: ShaderId, clip: bool, instances: u32) -> Result<()> {
        let elems = [
            InputElement {
                semantic: ATTRIBUTE,
                index: 0,
                format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                slot: 0,
                per_instance: false,
            },
            InputElement {
                semantic: ATTRIBUTE,
                index: 1,
                format: DXGI_FORMAT_R32G32_FLOAT,
                slot: 1,
                per_instance: false,
            },
        ];
        gpu.set_input_layout(vs, LAYOUT_KEY, &elems)?;
        let pos = if clip { &self.clip } else { &self.unit };
        let vbs = [Some(pos.buf.clone()), Some(self.uv.buf.clone())];
        let strides = [16u32, 8];
        let offsets = [0u32; 2];
        // SAFETY: two vertex buffers of this device, their strides and offsets; six vertices in each.
        unsafe {
            gpu.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            gpu.ctx.IASetVertexBuffers(0, 2, Some(vbs.as_ptr()), Some(strides.as_ptr()), Some(offsets.as_ptr()));
            gpu.ctx.DrawInstanced(6, instances, 0, 0);
        }
        Ok(())
    }
}

/// The fixed-function states and samplers the passes use.
#[derive(Debug)]
pub(super) struct States {
    pub opaque: StateId,
    /// TStaticBlendState<CW_RGB, BO_Add, BF_One, BF_SourceAlpha>: the fog's.
    pub fog_blend: StateId,
    pub cull_none: StateId,
    /// No depth test or write.
    pub no_depth: StateId,
    pub point_clamp: StateId,
    pub bilinear_clamp: StateId,
}

impl States {
    pub fn new(gpu: &Gpu) -> States {
        let mut fog = BlendKey::OPAQUE;
        fog.targets[0] = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_ONE.0 as u8,
            color_dst: D3D11_BLEND_SRC_ALPHA.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ONE.0 as u8,
            alpha_dst: D3D11_BLEND_ZERO.0 as u8,
            write_mask: (D3D11_COLOR_WRITE_ENABLE_RED.0
                | D3D11_COLOR_WRITE_ENABLE_GREEN.0
                | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8,
        };
        States {
            opaque: gpu.state(StateKey::Blend(BlendKey::OPAQUE)),
            fog_blend: gpu.state(StateKey::Blend(fog)),
            cull_none: gpu.state(StateKey::Raster(RasterKey {
                fill: D3D11_FILL_SOLID.0 as u8,
                cull: D3D11_CULL_NONE.0 as u8,
                front_ccw: true,
                depth_bias: 0,
                slope_scaled_depth_bias: 0,
            })),
            no_depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: None,
            })),
            point_clamp: gpu
                .state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP))),
            bilinear_clamp: gpu.state(StateKey::Sampler(SamplerKey::of(
                D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                D3D11_TEXTURE_ADDRESS_CLAMP,
            ))),
        }
    }
}

/// A 2D colour target of `size` in `format`, read and drawn.
pub(super) fn target(gpu: &Gpu, size: [u32; 2], format: DXGI_FORMAT) -> Result<Texture> {
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
}

/// A shader stage's resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Vertex,
    Pixel,
}

/// What a draw binds at one stage, by register.
#[derive(Default)]
pub(super) struct Binds<'a> {
    pub cbs: Vec<Option<&'a Buffer>>,
    pub srvs: Vec<Option<ID3D11ShaderResourceView>>,
    pub samplers: Vec<StateId>,
}

impl Binds<'_> {
    pub fn set(&self, gpu: &Gpu, stage: Stage) {
        let cbs: Vec<Option<ID3D11Buffer>> = self.cbs.iter().map(|b| b.map(|b| b.buf.clone())).collect();
        let smp: Vec<Option<ID3D11SamplerState>> = self.samplers.iter().map(|s| Some(gpu.sampler(*s))).collect();
        let ctx = &gpu.ctx;
        // SAFETY: buffers, views and samplers of this device.
        unsafe {
            match stage {
                Stage::Vertex => {
                    if !cbs.is_empty() {
                        ctx.VSSetConstantBuffers(0, Some(&cbs));
                    }
                    if !self.srvs.is_empty() {
                        ctx.VSSetShaderResources(0, Some(&self.srvs));
                    }
                    if !smp.is_empty() {
                        ctx.VSSetSamplers(0, Some(&smp));
                    }
                }
                Stage::Pixel => {
                    if !cbs.is_empty() {
                        ctx.PSSetConstantBuffers(0, Some(&cbs));
                    }
                    if !self.srvs.is_empty() {
                        ctx.PSSetShaderResources(0, Some(&self.srvs));
                    }
                    if !smp.is_empty() {
                        ctx.PSSetSamplers(0, Some(&smp));
                    }
                }
            }
        }
    }
}

/// Binds `rtv` alone (no depth) with a viewport of `size`.
pub(super) fn set_target(gpu: &Gpu, rtv: &Texture, size: [u32; 2]) {
    // SAFETY: a render target view of this device.
    unsafe { gpu.ctx.OMSetRenderTargets(Some(std::slice::from_ref(&rtv.rtv)), None) };
    gpu.viewport([0, 0, size[0], size[1]]);
}

/// Unbinds the pixel shader's resources and the targets (a target read by the next pass is not left bound).
pub(super) fn unbind(gpu: &Gpu) {
    let none: [Option<ID3D11ShaderResourceView>; 8] = Default::default();
    // SAFETY: plain state calls.
    unsafe {
        gpu.ctx.PSSetShaderResources(0, Some(&none));
        gpu.ctx.VSSetShaderResources(0, Some(&none));
        gpu.ctx.OMSetRenderTargets(None, None);
    }
}

/// Halton(index, base): the radical inverse.
pub(super) fn halton(mut index: u32, base: u32) -> f32 {
    let (mut r, mut f) = (0.0f64, 1.0 / base as f64);
    while index > 0 {
        r += f * (index % base) as f64;
        index /= base;
        f /= base as f64;
    }
    r as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_put_at_bytes() {
        let mut r = Rows::new(3);
        r.put(24, &[1.0, 2.0]);
        r.put_u(32, &[7]);
        assert_eq!(r.0[1], [0, 0, 1.0f32.to_bits(), 2.0f32.to_bits()]);
        assert_eq!(r.0[2][0], 7);
    }

    #[test]
    fn halton_sequence() {
        assert_eq!(halton(0, 2), 0.0);
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(3, 2), 0.75);
        assert!((halton(2, 3) - 2.0 / 3.0).abs() < 1e-6);
    }
}
