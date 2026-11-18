//! The team outline's skip (the port's own, frame cost; the picture is the same): the cooked ColoredOutlines pixel
//! shader runs only on the pixels its scan can change (render/post outline skip).
//!
//! The material changes a pixel only where its scan reads a custom stencil above 0, and every texel the scan reads lies
//! within R of the pixel on both axes (outline.rs scan_radius). So: the custom stencil reduced to one bit per TILE
//! square (any stencil above 0), dilated by ceil(R / TILE) tiles; the covered tiles marked 1 in a stencil of the
//! skip's own; the cooked pass drawn full screen under a stencil-equal test (the hardware's early stencil rejects the
//! rest) and the other pixels copied from its input, so the blendable's output is the same picture.

use windows::Win32::Graphics::Direct3D::{
    D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::pass::{Rows, constant_buffer, unbind, write};
use crate::Result;
use crate::gpu::{
    Buffer, DepthKey, Gpu, StateId, StateKey, StencilFace, StencilKey, TexDesc, TexKind, TexViews, Texture, dx,
};
use crate::lighting::compile;

/// A tile's side in pixels.
const TILE: u32 = 8;

/// The value the covered tiles' pixels hold in the skip's stencil.
const COVERED: u32 = 1;

const PARAMS: &str = "cbuffer Params : register(b0) { uint2 View; uint2 Count; int D; };\n";

/// One thread group per tile: 1 where a texel of the tile's has a custom stencil above 0.
const REDUCE: &str = r"
Texture2D<uint2> Stencil : register(t0);
RWTexture2D<uint> Tiles : register(u0);
groupshared uint Any;
[numthreads(8, 8, 1)]
void Main(uint3 g : SV_GroupID, uint3 t : SV_GroupThreadID, uint i : SV_GroupIndex) {
    if (i == 0) Any = 0;
    GroupMemoryBarrierWithGroupSync();
    uint2 p = min(g.xy * 8 + t.xy, View - 1);
    if (Stencil.Load(int3(p, 0)).g > 0) InterlockedOr(Any, 1);
    GroupMemoryBarrierWithGroupSync();
    if (i == 0) Tiles[g.xy] = Any;
}";

/// The tiles within D of a tile above 0.
const DILATE: &str = r"
Texture2D<uint> Tiles : register(t0);
RWTexture2D<uint> Covered : register(u0);
[numthreads(8, 8, 1)]
void Main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Count)) return;
    uint m = 0;
    for (int y = -D; y <= D; y++)
        for (int x = -D; x <= D; x++)
            m |= Tiles.Load(int3(clamp(int2(id.xy) + int2(x, y), 0, int2(Count) - 1), 0));
    Covered[id.xy] = m;
}";

/// A quad per tile (a 4-vertex strip, instanced), outside the view for a tile not covered.
const MARK: &str = r"
Texture2D<uint> Covered : register(t0);
float4 Main(uint v : SV_VertexID, uint i : SV_InstanceID) : SV_Position {
    uint2 t = uint2(i % Count.x, i / Count.x);
    if (Covered.Load(int3(t, 0)) == 0) return float4(2, 2, 2, 1);
    float2 c = min(float2(t + uint2(v & 1, v >> 1)) * 8.0, float2(View)) / float2(View);
    return float4(c.x * 2 - 1, 1 - c.y * 2, 0, 1);
}";

/// A triangle over the view, and the input's texel at each pixel.
const COPY: &str = r"
Texture2D<float4> Input : register(t0);
float4 Vertex(uint v : SV_VertexID) : SV_Position {
    float2 u = float2((v << 1) & 2, v & 2);
    return float4(u.x * 2 - 1, 1 - u.y * 2, 0, 1);
}
float4 Pixel(float4 p : SV_Position) : SV_Target { return Input.Load(int3(p.xy, 0)); }";

/// The tile counts of a view of `size`.
fn tile_count(size: [u32; 2]) -> [u32; 2] {
    size.map(|x| x.div_ceil(TILE))
}

/// The skip's targets of one view size: the tile masks and the covered tiles' stencil.
#[derive(Debug)]
pub(super) struct SkipTargets {
    size: [u32; 2],
    tiles: Texture,
    covered: Texture,
    /// D24_UNORM_S8_UINT of the view's size (its depth unused).
    _depth: ID3D11Texture2D,
    pub dsv: ID3D11DepthStencilView,
}

impl SkipTargets {
    pub fn new(gpu: &Gpu, size: [u32; 2]) -> Result<SkipTargets> {
        let [tx, ty] = tile_count(size);
        let mask = || {
            gpu.texture(
                &TexDesc {
                    kind: TexKind::D2,
                    size: [tx, ty, 1],
                    mips: 1,
                    format: DXGI_FORMAT_R32_UINT,
                    views: TexViews { srv: true, uav: true, ..Default::default() },
                },
                &[],
            )
        };
        let td = D3D11_TEXTURE2D_DESC {
            Width: size[0],
            Height: size[1],
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let (mut t, mut v) = (None, None);
        // SAFETY: a texture of the description above and its view (the format's own) on this device.
        unsafe {
            gpu.dev.CreateTexture2D(&td, None, Some(&mut t)).map_err(dx("CreateTexture2D outline stencil"))?;
            let t = t.as_ref().expect("CreateTexture2D");
            gpu.dev.CreateDepthStencilView(t, None, Some(&mut v)).map_err(dx("CreateDepthStencilView outline"))?;
        }
        Ok(SkipTargets {
            size,
            tiles: mask()?,
            covered: mask()?,
            _depth: t.expect("CreateTexture2D"),
            dsv: v.expect("CreateDepthStencilView"),
        })
    }
}

/// The skip's shaders and states.
#[derive(Debug)]
pub(super) struct Skip {
    reduce: ID3D11ComputeShader,
    dilate: ID3D11ComputeShader,
    mark: ID3D11VertexShader,
    copy_vs: ID3D11VertexShader,
    copy_ps: ID3D11PixelShader,
    /// Stencil always replaced by COVERED.
    mark_state: StateId,
    /// Stencil equal to COVERED: the cooked pass's.
    pub equal: StateId,
    /// Stencil not equal to COVERED: the copy's.
    not_equal: StateId,
    opaque: StateId,
    cull_none: StateId,
    params: Buffer,
}

impl Skip {
    pub fn new(gpu: &Gpu, opaque: StateId, cull_none: StateId) -> Result<Skip> {
        let src = |body: &str| format!("{PARAMS}{body}");
        let reduce = compile(&src(REDUCE), "Main", "cs_5_0")?;
        let dilate = compile(&src(DILATE), "Main", "cs_5_0")?;
        let mark = compile(&src(MARK), "Main", "vs_5_0")?;
        let copy_vs = compile(COPY, "Vertex", "vs_5_0")?;
        let copy_ps = compile(COPY, "Pixel", "ps_5_0")?;
        let (mut r, mut d, mut m, mut cv, mut cp) = (None, None, None, None, None);
        // SAFETY: DXBC compiled above by d3dcompiler, created on this device.
        unsafe {
            gpu.dev.CreateComputeShader(&reduce, None, Some(&mut r)).map_err(dx("CreateComputeShader reduce"))?;
            gpu.dev.CreateComputeShader(&dilate, None, Some(&mut d)).map_err(dx("CreateComputeShader dilate"))?;
            gpu.dev.CreateVertexShader(&mark, None, Some(&mut m)).map_err(dx("CreateVertexShader mark"))?;
            gpu.dev.CreateVertexShader(&copy_vs, None, Some(&mut cv)).map_err(dx("CreateVertexShader copy"))?;
            gpu.dev.CreatePixelShader(&copy_ps, None, Some(&mut cp)).map_err(dx("CreatePixelShader copy"))?;
        }
        let face = |func: D3D11_COMPARISON_FUNC, pass: D3D11_STENCIL_OP| StencilFace {
            func: func.0 as u8,
            fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
            pass: pass.0 as u8,
        };
        let state = |func, pass, write_mask| {
            let f = face(func, pass);
            gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: Some(StencilKey { front: f, back: f, read_mask: 0xff, write_mask }),
            }))
        };
        Ok(Skip {
            reduce: r.expect("CreateComputeShader"),
            dilate: d.expect("CreateComputeShader"),
            mark: m.expect("CreateVertexShader"),
            copy_vs: cv.expect("CreateVertexShader"),
            copy_ps: cp.expect("CreatePixelShader"),
            mark_state: state(D3D11_COMPARISON_ALWAYS, D3D11_STENCIL_OP_REPLACE, 0xff),
            equal: state(D3D11_COMPARISON_EQUAL, D3D11_STENCIL_OP_KEEP, 0),
            not_equal: state(D3D11_COMPARISON_NOT_EQUAL, D3D11_STENCIL_OP_KEEP, 0),
            opaque,
            cull_none,
            params: constant_buffer(gpu)?,
        })
    }

    /// The covered tiles of `custom_stencil` (the custom stencil's uint view) for a scan of `radius` pixels, marked
    /// in `t`'s stencil.
    pub fn mark(
        &self,
        gpu: &Gpu,
        t: &SkipTargets,
        custom_stencil: &ID3D11ShaderResourceView,
        radius: u32,
    ) -> Result<()> {
        let [tx, ty] = tile_count(t.size);
        let mut p = Rows::new(2);
        p.put_u(0, &[t.size[0], t.size[1], tx, ty, radius.div_ceil(TILE)]);
        write(gpu, &self.params, &p);
        let ctx = &gpu.ctx;
        let none_srv: [Option<ID3D11ShaderResourceView>; 1] = [None];
        let none_uav: [Option<ID3D11UnorderedAccessView>; 1] = [None];
        // SAFETY: shaders, views and buffers of this device; each UAV is unbound before its texture is read.
        unsafe {
            ctx.CSSetConstantBuffers(0, Some(&[Some(self.params.buf.clone())]));
            ctx.CSSetShader(&self.reduce, None);
            ctx.CSSetShaderResources(0, Some(&[Some(custom_stencil.clone())]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some([t.tiles.uav.clone()].as_ptr()), None);
            ctx.Dispatch(tx, ty, 1);
            ctx.CSSetUnorderedAccessViews(0, 1, Some(none_uav.as_ptr()), None);
            ctx.CSSetShader(&self.dilate, None);
            ctx.CSSetShaderResources(0, Some(std::slice::from_ref(&t.tiles.srv)));
            ctx.CSSetUnorderedAccessViews(0, 1, Some([t.covered.uav.clone()].as_ptr()), None);
            ctx.Dispatch(tx.div_ceil(8), ty.div_ceil(8), 1);
            ctx.CSSetUnorderedAccessViews(0, 1, Some(none_uav.as_ptr()), None);
            ctx.CSSetShaderResources(0, Some(&none_srv));
            ctx.CSSetShader(None, None);

            ctx.ClearDepthStencilView(&t.dsv, D3D11_CLEAR_STENCIL.0, 0.0, 0);
            ctx.OMSetRenderTargets(None, &t.dsv);
        }
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        gpu.set_states(self.opaque, self.cull_none, self.mark_state, COVERED);
        // SAFETY: as above; no vertex buffers (the shader makes its positions from the vertex and instance ids).
        unsafe {
            ctx.VSSetShader(&self.mark, None);
            ctx.PSSetShader(None, None);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.params.buf.clone())]));
            ctx.VSSetShaderResources(0, Some(std::slice::from_ref(&t.covered.srv)));
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.DrawInstanced(4, tx * ty, 0, 0);
        }
        unbind(gpu);
        Ok(())
    }

    /// `input`'s pixels outside the covered tiles into `out` (both `t`'s size).
    pub fn copy_uncovered(&self, gpu: &Gpu, t: &SkipTargets, input: &Texture, out: &Texture) {
        gpu.set_states(self.opaque, self.cull_none, self.not_equal, COVERED);
        let ctx = &gpu.ctx;
        // SAFETY: views and shaders of this device; `out` is not `input`.
        unsafe {
            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&out.rtv)), &t.dsv);
            ctx.VSSetShader(&self.copy_vs, None);
            ctx.PSSetShader(&self.copy_ps, None);
            ctx.PSSetShaderResources(0, Some(std::slice::from_ref(&input.srv)));
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        }
        gpu.viewport([0, 0, t.size[0], t.size[1]]);
        // SAFETY: a draw of three vertices made from their ids.
        unsafe { ctx.Draw(3, 0) };
        unbind(gpu);
    }
}
