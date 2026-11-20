//! The D3D11 device: textures, buffers, the build's shaders created from their DXBC, the fixed-function state
//! cache (UE's TStatic*State keys) and the GPU timer.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, MutexGuard};

use windows::Win32::Foundation::{HMODULE, S_FALSE};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::{Interface, PCSTR};

use crate::{Error, Result};

/// A failed D3D11 / DXGI call as the crate's error.
pub(crate) fn dx(what: impl Into<String>) -> impl FnOnce(windows::core::Error) -> Error {
    let what = what.into();
    move |source| Error::Device { what, source }
}

/// The device on one adapter and a context on it. The device is free-threaded: its creation methods (textures,
/// buffers, shaders, state objects, input layouts) run on any thread, into caches every handle on the device shares.
/// The context is not: the frame thread's Gpu holds the immediate context, a loader thread's one of its own
/// ([`Gpu::deferred`]), whose recorded commands the frame thread runs ([`Gpu::recorded`], [`Gpu::run_recorded`]).
#[derive(Debug)]
pub struct Gpu {
    pub dev: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    /// The adapter's description.
    pub adapter: String,
    /// The adapter's own video memory, bytes (DXGI's DedicatedVideoMemory; 0 for one that shares the system's).
    pub video_memory: u64,
    cache: Arc<Mutex<Cache>>,
}

/// A created shader by its stage.
#[derive(Debug)]
enum Stage {
    Vertex(ID3D11VertexShader),
    Pixel(ID3D11PixelShader),
    Geometry(ID3D11GeometryShader),
    Compute(ID3D11ComputeShader),
}

#[derive(Debug)]
struct Shader {
    stage: Stage,
    /// A vertex shader's DXBC, for its input layouts.
    dxbc: Option<Arc<[u8]>>,
}

#[derive(Debug)]
enum State {
    Blend(ID3D11BlendState),
    Raster(ID3D11RasterizerState),
    Depth(ID3D11DepthStencilState),
    Sampler(ID3D11SamplerState),
}

/// The device's caches: shaders by their code's SHA-1, state objects by key, input layouts by (vertex shader, the
/// layout's key).
#[derive(Debug, Default)]
struct Cache {
    shader_ids: HashMap<mvs_material::Sha1, ShaderId>,
    shaders: Vec<Shader>,
    state_ids: HashMap<StateKey, StateId>,
    states: Vec<State>,
    layouts: HashMap<(ShaderId, u32), ID3D11InputLayout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexKind {
    D2,
    /// Six faces a mip, `size[2]` 6.
    Cube,
    D3,
    /// `size[2]` slices.
    D2Array,
}

/// Which views a texture gets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TexViews {
    pub srv: bool,
    pub rtv: bool,
    pub dsv: bool,
    pub uav: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexDesc {
    pub kind: TexKind,
    /// Width, height, and depth / faces / slices.
    pub size: [u32; 3],
    pub mips: u32,
    /// The resource's format; a depth target's typeless one gets its views' formats by the device.
    pub format: DXGI_FORMAT,
    pub views: TexViews,
}

/// One subresource's bytes as D3D11_SUBRESOURCE_DATA takes them, in D3D's order (mip within face / slice).
#[derive(Clone, Copy, Debug)]
pub struct Subresource<'a> {
    pub data: &'a [u8],
    pub row_pitch: u32,
    pub slice_pitch: u32,
}

#[derive(Debug)]
pub struct Texture {
    pub res: ID3D11Resource,
    pub srv: Option<ID3D11ShaderResourceView>,
    pub rtv: Option<ID3D11RenderTargetView>,
    pub dsv: Option<ID3D11DepthStencilView>,
    pub uav: Option<ID3D11UnorderedAccessView>,
    pub desc: TexDesc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufKind {
    Vertex,
    Index16,
    Index32,
    /// A uniform buffer; `size` a multiple of 16.
    Constant,
    /// A StructuredBuffer with its SRV.
    Structured {
        stride: u32,
    },
    /// A Buffer<T> with its SRV in `format` (the vertex factories' manual fetch, GPUSkin's bone matrices).
    Typed {
        format: DXGI_FORMAT,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufDesc {
    pub kind: BufKind,
    /// Bytes.
    pub size: u32,
    /// Written each frame by the CPU (Map WRITE_DISCARD); else immutable.
    pub dynamic: bool,
}

/// A buffer and its view (a clone shares the GPU buffer).
#[derive(Clone, Debug)]
pub struct Buffer {
    pub buf: ID3D11Buffer,
    pub srv: Option<ID3D11ShaderResourceView>,
    pub desc: BufDesc,
}

/// A shader created from its DXBC, by the device's cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShaderId(pub u32);

/// A fixed-function state object, by the device's cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateId(pub u32);

/// One render target's blend (TStaticBlendState's per-target terms). Values are D3D11_BLEND_OP / D3D11_BLEND;
/// blending is on unless both are (Add, One, Zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendTarget {
    pub color_op: u8,
    pub color_src: u8,
    pub color_dst: u8,
    pub alpha_op: u8,
    pub alpha_src: u8,
    pub alpha_dst: u8,
    /// D3D11_COLOR_WRITE_ENABLE bits.
    pub write_mask: u8,
}

impl BlendTarget {
    /// No blending, every channel written (TStaticBlendState<>'s default target).
    pub const OPAQUE: BlendTarget = BlendTarget {
        color_op: D3D11_BLEND_OP_ADD.0 as u8,
        color_src: D3D11_BLEND_ONE.0 as u8,
        color_dst: D3D11_BLEND_ZERO.0 as u8,
        alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
        alpha_src: D3D11_BLEND_ONE.0 as u8,
        alpha_dst: D3D11_BLEND_ZERO.0 as u8,
        write_mask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendKey {
    pub targets: [BlendTarget; 8],
    pub alpha_to_coverage: bool,
}

impl BlendKey {
    pub const OPAQUE: BlendKey = BlendKey { targets: [BlendTarget::OPAQUE; 8], alpha_to_coverage: false };
}

/// TStaticRasterizerState. `fill` D3D11_FILL_MODE, `cull` D3D11_CULL_MODE.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterKey {
    pub fill: u8,
    pub cull: u8,
    pub front_ccw: bool,
    pub depth_bias: i32,
    /// SlopeScaledDepthBias's f32 bits.
    pub slope_scaled_depth_bias: u32,
}

/// One face's stencil: D3D11_COMPARISON_FUNC and D3D11_STENCIL_OPs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StencilFace {
    pub func: u8,
    pub fail: u8,
    pub depth_fail: u8,
    pub pass: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StencilKey {
    pub front: StencilFace,
    pub back: StencilFace,
    pub read_mask: u8,
    pub write_mask: u8,
}

/// TStaticDepthStencilState; the stencil reference is set at the draw. `depth_func` D3D11_COMPARISON_FUNC (the
/// depth is reversed: near 1, far 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DepthKey {
    pub depth_write: bool,
    pub depth_func: u8,
    pub stencil: Option<StencilKey>,
}

/// TStaticSamplerState. `filter` D3D11_FILTER, `address` D3D11_TEXTURE_ADDRESS_MODE (U, V, W), `comparison`
/// D3D11_COMPARISON_FUNC (0 none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerKey {
    pub filter: u32,
    pub address: [u8; 3],
    /// MipLODBias's f32 bits.
    pub mip_bias: u32,
    pub max_anisotropy: u8,
    pub comparison: u8,
    /// BorderColor's f32 bits.
    pub border: [u32; 4],
}

impl SamplerKey {
    /// A sampler of `filter` with every axis at `address`.
    pub fn of(filter: D3D11_FILTER, address: D3D11_TEXTURE_ADDRESS_MODE) -> SamplerKey {
        let a = address.0 as u8;
        SamplerKey {
            filter: filter.0 as u32,
            address: [a; 3],
            mip_bias: 0,
            max_anisotropy: 1,
            comparison: 0,
            border: [0; 4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateKey {
    Blend(BlendKey),
    Raster(RasterKey),
    Depth(DepthKey),
    Sampler(SamplerKey),
}

/// The views' formats of a depth target's typeless resource format: (DSV, SRV).
fn depth_views(f: DXGI_FORMAT) -> Option<(DXGI_FORMAT, DXGI_FORMAT)> {
    match f {
        DXGI_FORMAT_R32G8X24_TYPELESS => Some((DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS)),
        DXGI_FORMAT_R24G8_TYPELESS => Some((DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R24_UNORM_X8_TYPELESS)),
        DXGI_FORMAT_R32_TYPELESS => Some((DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R32_FLOAT)),
        DXGI_FORMAT_R16_TYPELESS => Some((DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_R16_UNORM)),
        _ => None,
    }
}

fn bool32(b: bool) -> windows::core::BOOL {
    b.into()
}

/// The commands a loader thread's deferred context recorded (the copies and writes of what it made).
#[derive(Debug)]
pub struct Recorded(ID3D11CommandList);

impl Gpu {
    /// The device on the first adapter whose description holds `adapter` (any case), or the default one; `debug`
    /// adds the D3D11 debug layer.
    pub fn new(adapter: Option<&str>, debug: bool) -> Result<Gpu> {
        // SAFETY: DXGI and D3D11 creation calls with valid out-pointers; the adapter outlives the call.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(dx("CreateDXGIFactory1"))?;
            let mut names = Vec::new();
            let mut found: Option<(IDXGIAdapter1, String, u64)> = None;
            let mut i = 0;
            while let Ok(a) = factory.EnumAdapters1(i) {
                i += 1;
                let d = a.GetDesc1().map_err(dx("GetDesc1"))?;
                let n = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                let name = String::from_utf16_lossy(&d.Description[..n]);
                let take = match adapter {
                    Some(want) => name.to_lowercase().contains(&want.to_lowercase()),
                    None => i == 1,
                };
                if found.is_none() && take {
                    found = Some((a, name.clone(), d.DedicatedVideoMemory as u64));
                }
                names.push(name);
            }
            let (a, name, video_memory) = found.ok_or_else(|| Error::Format {
                what: "adapter".into(),
                why: format!("no adapter's description holds {adapter:?}: {names:?}"),
            })?;
            let mut dev = None;
            let mut ctx = None;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            let flags = if debug { D3D11_CREATE_DEVICE_DEBUG } else { D3D11_CREATE_DEVICE_FLAG(0) };
            D3D11CreateDevice(
                &a,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                Some(&mut ctx),
            )
            .map_err(dx("D3D11CreateDevice"))?;
            Ok(Gpu {
                dev: dev.expect("D3D11CreateDevice gave no device"),
                ctx: ctx.expect("D3D11CreateDevice gave no context"),
                adapter: name,
                video_memory,
                cache: Arc::default(),
            })
        }
    }

    /// A handle on the same device, context and caches: what the asset loaders create with.
    pub(crate) fn share(&self) -> Gpu {
        Gpu {
            dev: self.dev.clone(),
            ctx: self.ctx.clone(),
            adapter: self.adapter.clone(),
            video_memory: self.video_memory,
            cache: self.cache.clone(),
        }
    }

    /// A handle on the same device and caches with a deferred context of its own, for a loader thread: what it makes
    /// lands in the shared caches; what it writes and copies on its context is recorded, for the frame thread to run
    /// ([`Gpu::recorded`], then [`Gpu::run_recorded`] on the device's own context) before what it made draws.
    pub fn deferred(&self) -> Result<Gpu> {
        let mut ctx = None;
        // SAFETY: a device method with a valid out-pointer (the device is free-threaded).
        unsafe { self.dev.CreateDeferredContext(0, Some(&mut ctx)).map_err(dx("CreateDeferredContext"))? };
        Ok(Gpu {
            dev: self.dev.clone(),
            ctx: ctx.expect("CreateDeferredContext gave no context"),
            adapter: self.adapter.clone(),
            video_memory: self.video_memory,
            cache: self.cache.clone(),
        })
    }

    /// A deferred context's recorded commands, as a list the frame thread runs ([`Gpu::run_recorded`]).
    pub fn recorded(&self) -> Result<Recorded> {
        let mut list = None;
        // SAFETY: a deferred context's method with a valid out-pointer, on the thread that recorded.
        unsafe { self.ctx.FinishCommandList(false, Some(&mut list)).map_err(dx("FinishCommandList"))? };
        Ok(Recorded(list.expect("FinishCommandList gave no list")))
    }

    /// A loader's recorded commands run on this context (the device's own: the frame thread's), its state kept.
    pub fn run_recorded(&self, r: &Recorded) {
        // SAFETY: a command list of this device, on the immediate context.
        unsafe { self.ctx.ExecuteCommandList(&r.0, true) };
    }

    /// The objects the device's caches hold for the process: shaders, state objects and input layouts.
    pub fn cached(&self) -> [usize; 3] {
        let c = self.cache();
        [c.shaders.len(), c.states.len(), c.layouts.len()]
    }

    /// The device's caches, locked (never held across a device call that creates).
    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A texture with its views as `desc` asks, filled from `mips` (empty: uninitialised, for a target).
    pub fn texture(&self, desc: &TexDesc, mips: &[Subresource]) -> Result<Texture> {
        let v = desc.views;
        let mut bind = 0u32;
        if v.srv {
            bind |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
        }
        if v.rtv {
            bind |= D3D11_BIND_RENDER_TARGET.0 as u32;
        }
        if v.dsv {
            bind |= D3D11_BIND_DEPTH_STENCIL.0 as u32;
        }
        if v.uav {
            bind |= D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        }
        let init: Vec<D3D11_SUBRESOURCE_DATA> = mips
            .iter()
            .map(|s| D3D11_SUBRESOURCE_DATA {
                pSysMem: s.data.as_ptr() as *const c_void,
                SysMemPitch: s.row_pitch,
                SysMemSlicePitch: s.slice_pitch,
            })
            .collect();
        let layers = match desc.kind {
            TexKind::D3 => 1,
            _ => desc.size[2].max(1),
        };
        if !init.is_empty() && init.len() != (desc.mips * layers) as usize {
            return Err(Error::Format {
                what: "texture".into(),
                why: format!("{} subresources for {} mips x {layers}", init.len(), desc.mips),
            });
        }
        let [w, h, d] = desc.size;
        let (block, block_bytes) = format_layout(desc.format)
            .ok_or_else(|| Error::Format { what: "texture".into(), why: format!("format {}", desc.format.0) })?;
        if block > 1 && (w % block != 0 || h % block != 0) {
            // D3D11 refuses a block-compressed texture whose top mip is not whole blocks.
            return Err(Error::Format {
                what: "texture".into(),
                why: format!("{w}x{h} is not whole {block}x{block} blocks"),
            });
        }
        for (k, s) in mips.iter().enumerate() {
            let mip = k as u32 % desc.mips.max(1);
            let (mw, mh) = ((w >> mip).max(1), (h >> mip).max(1));
            let row = mw.div_ceil(block) * block_bytes;
            let rows = mh.div_ceil(block);
            let slices = if desc.kind == TexKind::D3 { (d >> mip).max(1) } else { 1 };
            let need = if slices > 1 {
                (s.slice_pitch as usize) * (slices as usize - 1) + (s.row_pitch * (rows - 1) + row) as usize
            } else {
                (s.row_pitch * (rows - 1) + row) as usize
            };
            if s.row_pitch < row || (slices > 1 && s.slice_pitch < s.row_pitch * rows) || s.data.len() < need {
                return Err(Error::Format {
                    what: "texture".into(),
                    why: format!(
                        "subresource {k}: {} bytes, pitches {} / {}, for {mw}x{mh}x{slices}",
                        s.data.len(),
                        s.row_pitch,
                        s.slice_pitch
                    ),
                });
            }
        }
        let init_ptr = if init.is_empty() { None } else { Some(init.as_ptr()) };
        // SAFETY: the descriptions are filled above; `init` holds one entry per subresource and borrows `mips`,
        // alive through the call.
        let res: ID3D11Resource = unsafe {
            if desc.kind == TexKind::D3 {
                let td = D3D11_TEXTURE3D_DESC {
                    Width: w,
                    Height: h,
                    Depth: d,
                    MipLevels: desc.mips,
                    Format: desc.format,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut t = None;
                self.dev.CreateTexture3D(&td, init_ptr, Some(&mut t)).map_err(dx("CreateTexture3D"))?;
                t.expect("CreateTexture3D").cast().map_err(dx("cast"))?
            } else {
                let td = D3D11_TEXTURE2D_DESC {
                    Width: w,
                    Height: h,
                    MipLevels: desc.mips,
                    ArraySize: layers,
                    Format: desc.format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: if desc.kind == TexKind::Cube { D3D11_RESOURCE_MISC_TEXTURECUBE.0 as u32 } else { 0 },
                };
                let mut t = None;
                self.dev.CreateTexture2D(&td, init_ptr, Some(&mut t)).map_err(dx("CreateTexture2D"))?;
                t.expect("CreateTexture2D").cast().map_err(dx("cast"))?
            }
        };
        let depth = depth_views(desc.format);
        let mut out = Texture { res, srv: None, rtv: None, dsv: None, uav: None, desc: *desc };
        // SAFETY: views of the resource just made, with descriptions matching its kind and format.
        unsafe {
            if v.srv {
                let mut s = None;
                match depth {
                    Some((_, srv_format)) => {
                        let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
                            Format: srv_format,
                            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                                Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: desc.mips },
                            },
                        };
                        self.dev
                            .CreateShaderResourceView(&out.res, Some(&sd), Some(&mut s))
                            .map_err(dx("CreateShaderResourceView"))?;
                    }
                    None => self
                        .dev
                        .CreateShaderResourceView(&out.res, None, Some(&mut s))
                        .map_err(dx("CreateShaderResourceView"))?,
                }
                out.srv = s;
            }
            if v.rtv {
                let mut r = None;
                self.dev.CreateRenderTargetView(&out.res, None, Some(&mut r)).map_err(dx("CreateRenderTargetView"))?;
                out.rtv = r;
            }
            if v.dsv {
                let mut r = None;
                let dsv_format = depth.map_or(desc.format, |d| d.0);
                let dd = D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: dsv_format,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                    Flags: 0,
                    Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 } },
                };
                self.dev
                    .CreateDepthStencilView(&out.res, Some(&dd), Some(&mut r))
                    .map_err(dx("CreateDepthStencilView"))?;
                out.dsv = r;
            }
            if v.uav {
                let mut u = None;
                self.dev
                    .CreateUnorderedAccessView(&out.res, None, Some(&mut u))
                    .map_err(dx("CreateUnorderedAccessView"))?;
                out.uav = u;
            }
        }
        Ok(out)
    }

    /// A buffer, filled from `init` (an immutable one needs it).
    pub fn buffer(&self, desc: &BufDesc, init: Option<&[u8]>) -> Result<Buffer> {
        let (bind, misc, stride) = match desc.kind {
            BufKind::Vertex => (D3D11_BIND_VERTEX_BUFFER.0 as u32, 0, 0),
            BufKind::Index16 | BufKind::Index32 => (D3D11_BIND_INDEX_BUFFER.0 as u32, 0, 0),
            BufKind::Constant => (D3D11_BIND_CONSTANT_BUFFER.0 as u32, 0, 0),
            BufKind::Structured { stride } => {
                (D3D11_BIND_SHADER_RESOURCE.0 as u32, D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32, stride)
            }
            BufKind::Typed { .. } => (D3D11_BIND_SHADER_RESOURCE.0 as u32, 0, 0),
        };
        if !desc.dynamic && init.is_none() {
            return Err(Error::Format { what: "buffer".into(), why: "an immutable buffer with no contents".into() });
        }
        if let Some(b) = init
            && b.len() < desc.size as usize
        {
            return Err(Error::Format {
                what: "buffer".into(),
                why: format!("{} bytes for a buffer of {}", b.len(), desc.size),
            });
        }
        let bd = D3D11_BUFFER_DESC {
            ByteWidth: desc.size,
            Usage: if desc.dynamic { D3D11_USAGE_DYNAMIC } else { D3D11_USAGE_IMMUTABLE },
            BindFlags: bind,
            CPUAccessFlags: if desc.dynamic { D3D11_CPU_ACCESS_WRITE.0 as u32 } else { 0 },
            MiscFlags: misc,
            StructureByteStride: stride,
        };
        let data = init.map(|b| D3D11_SUBRESOURCE_DATA {
            pSysMem: b.as_ptr() as *const c_void,
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        });
        let mut b = None;
        // SAFETY: `data` points into `init`, alive through the call and at least `desc.size` bytes long.
        unsafe {
            self.dev
                .CreateBuffer(&bd, data.as_ref().map(|d| d as *const _), Some(&mut b))
                .map_err(dx("CreateBuffer"))?
        };
        let buf = b.expect("CreateBuffer");
        let srv = match desc.kind {
            BufKind::Structured { stride } => Some(self.buffer_srv(&buf, DXGI_FORMAT_UNKNOWN, desc.size / stride)?),
            BufKind::Typed { format } => {
                let n = format_bytes(format).ok_or_else(|| Error::Format {
                    what: "buffer".into(),
                    why: format!("typed view of format {}", format.0),
                })?;
                Some(self.buffer_srv(&buf, format, desc.size / n)?)
            }
            _ => None,
        };
        Ok(Buffer { buf, srv, desc: *desc })
    }

    fn buffer_srv(&self, buf: &ID3D11Buffer, format: DXGI_FORMAT, n: u32) -> Result<ID3D11ShaderResourceView> {
        let sd = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D_SRV_DIMENSION_BUFFER,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_SRV {
                    Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                    Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: n },
                },
            },
        };
        let mut s = None;
        // SAFETY: a view of a buffer made with the shader-resource bind flag.
        unsafe { self.dev.CreateShaderResourceView(buf, Some(&sd), Some(&mut s)).map_err(dx("buffer SRV"))? };
        Ok(s.expect("CreateShaderResourceView"))
    }

    /// A dynamic buffer's new contents (Map WRITE_DISCARD).
    pub fn write(&self, b: &Buffer, bytes: &[u8]) {
        assert!(b.desc.dynamic, "write to an immutable buffer");
        assert!(bytes.len() <= b.desc.size as usize, "{} bytes into a buffer of {}", bytes.len(), b.desc.size);
        // SAFETY: the mapped pointer covers the buffer's `size` bytes, at least `bytes.len()`; unmapped after.
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&b.buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).expect("Map WRITE_DISCARD");
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), m.pData as *mut u8, bytes.len());
            self.ctx.Unmap(&b.buf, 0);
        }
    }

    /// A dynamic uniform buffer's new rows (Map WRITE_DISCARD).
    pub fn write_rows(&self, b: &Buffer, rows: &[[u32; 4]]) {
        self.write(b, rows_bytes(rows));
    }

    /// The shader of `code`'s DXBC, unchanged, created once per code (by its SHA-1).
    pub fn shader(&self, code: &mvs_material::ShaderCode) -> Result<ShaderId> {
        self.shader_keyed(sha1_of(&code.dxbc), code)
    }

    /// The same, keyed by the code's store SHA-1 (no hash of the DXBC).
    pub fn shader_by(&self, sha: mvs_material::Sha1, code: &mvs_material::ShaderCode) -> Result<ShaderId> {
        self.shader_keyed(sha, code)
    }

    fn shader_keyed(&self, sha: mvs_material::Sha1, code: &mvs_material::ShaderCode) -> Result<ShaderId> {
        if let Some(&id) = self.cache().shader_ids.get(&sha) {
            return Ok(id);
        }
        use mvs_material::Frequency as F;
        if code.no_create {
            return Err(Error::Format { what: "shader".into(), why: "a DXIL code (NoCreate)".into() });
        }
        let b = &code.dxbc[..];
        // SAFETY: the DXBC as the build compiled it; D3D validates the container.
        let stage = unsafe {
            match code.frequency {
                F::Vertex => {
                    let mut s = None;
                    self.dev.CreateVertexShader(b, None, Some(&mut s)).map_err(dx("CreateVertexShader"))?;
                    Stage::Vertex(s.expect("CreateVertexShader"))
                }
                F::Pixel => {
                    let mut s = None;
                    self.dev.CreatePixelShader(b, None, Some(&mut s)).map_err(dx("CreatePixelShader"))?;
                    Stage::Pixel(s.expect("CreatePixelShader"))
                }
                F::Geometry => {
                    let mut s = None;
                    self.dev.CreateGeometryShader(b, None, Some(&mut s)).map_err(dx("CreateGeometryShader"))?;
                    Stage::Geometry(s.expect("CreateGeometryShader"))
                }
                F::Hull | F::Domain => {
                    return Err(Error::Format { what: "shader".into(), why: "a tessellation stage".into() });
                }
                F::Compute => {
                    let mut s = None;
                    self.dev.CreateComputeShader(b, None, Some(&mut s)).map_err(dx("CreateComputeShader"))?;
                    Stage::Compute(s.expect("CreateComputeShader"))
                }
                F::RayTracing(n) => {
                    return Err(Error::Format { what: "shader".into(), why: format!("ray tracing frequency {n}") });
                }
            }
        };
        let dxbc = matches!(stage, Stage::Vertex(_)).then(|| Arc::from(&code.dxbc[..]));
        let mut c = self.cache();
        // (another thread made it meanwhile: its id, this one dropped)
        if let Some(&id) = c.shader_ids.get(&sha) {
            return Ok(id);
        }
        let id = ShaderId(c.shaders.len() as u32);
        c.shaders.push(Shader { stage, dxbc });
        c.shader_ids.insert(sha, id);
        Ok(id)
    }

    /// Shaders created so far.
    pub fn shader_count(&self) -> usize {
        self.cache().shaders.len()
    }

    /// The caches' sizes: shaders, state objects, input layouts (what a warm-up's check watches).
    pub fn cache_counts(&self) -> [usize; 3] {
        let c = self.cache();
        [c.shaders.len(), c.states.len(), c.layouts.len()]
    }

    /// The state object of `key`, created once per key.
    pub fn state(&self, key: StateKey) -> StateId {
        if let Some(&id) = self.cache().state_ids.get(&key) {
            return id;
        }
        let state = self.create_state(&key).unwrap_or_else(|e| panic!("state {key:?}: {e}"));
        let mut c = self.cache();
        if let Some(&id) = c.state_ids.get(&key) {
            return id;
        }
        let id = StateId(c.states.len() as u32);
        c.states.push(state);
        c.state_ids.insert(key, id);
        id
    }

    fn create_state(&self, key: &StateKey) -> Result<State> {
        // SAFETY: state creation from descriptions filled here.
        unsafe {
            Ok(match key {
                StateKey::Blend(b) => {
                    let mut d = D3D11_BLEND_DESC {
                        AlphaToCoverageEnable: bool32(b.alpha_to_coverage),
                        IndependentBlendEnable: bool32(b.targets.iter().any(|t| *t != b.targets[0])),
                        ..Default::default()
                    };
                    for (o, t) in d.RenderTarget.iter_mut().zip(&b.targets) {
                        let off = t.color_op == D3D11_BLEND_OP_ADD.0 as u8
                            && t.color_src == D3D11_BLEND_ONE.0 as u8
                            && t.color_dst == D3D11_BLEND_ZERO.0 as u8
                            && t.alpha_op == D3D11_BLEND_OP_ADD.0 as u8
                            && t.alpha_src == D3D11_BLEND_ONE.0 as u8
                            && t.alpha_dst == D3D11_BLEND_ZERO.0 as u8;
                        *o = D3D11_RENDER_TARGET_BLEND_DESC {
                            BlendEnable: bool32(!off),
                            SrcBlend: D3D11_BLEND(t.color_src as i32),
                            DestBlend: D3D11_BLEND(t.color_dst as i32),
                            BlendOp: D3D11_BLEND_OP(t.color_op as i32),
                            SrcBlendAlpha: D3D11_BLEND(t.alpha_src as i32),
                            DestBlendAlpha: D3D11_BLEND(t.alpha_dst as i32),
                            BlendOpAlpha: D3D11_BLEND_OP(t.alpha_op as i32),
                            RenderTargetWriteMask: t.write_mask,
                        };
                    }
                    let mut s = None;
                    self.dev.CreateBlendState(&d, Some(&mut s)).map_err(dx("CreateBlendState"))?;
                    State::Blend(s.expect("CreateBlendState"))
                }
                StateKey::Raster(r) => {
                    // FD3D11DynamicRHI's rasterizer state: DepthClipEnable true. Scissor off: no pass of the
                    // renderer's sets a scissor rect (the viewport bounds every draw).
                    let d = D3D11_RASTERIZER_DESC {
                        FillMode: D3D11_FILL_MODE(r.fill as i32),
                        CullMode: D3D11_CULL_MODE(r.cull as i32),
                        FrontCounterClockwise: bool32(r.front_ccw),
                        DepthBias: r.depth_bias,
                        DepthBiasClamp: 0.0,
                        SlopeScaledDepthBias: f32::from_bits(r.slope_scaled_depth_bias),
                        DepthClipEnable: bool32(true),
                        ScissorEnable: bool32(false),
                        MultisampleEnable: bool32(false),
                        AntialiasedLineEnable: bool32(false),
                    };
                    let mut s = None;
                    self.dev.CreateRasterizerState(&d, Some(&mut s)).map_err(dx("CreateRasterizerState"))?;
                    State::Raster(s.expect("CreateRasterizerState"))
                }
                StateKey::Depth(k) => {
                    let face = |f: &StencilFace| D3D11_DEPTH_STENCILOP_DESC {
                        StencilFailOp: D3D11_STENCIL_OP(f.fail as i32),
                        StencilDepthFailOp: D3D11_STENCIL_OP(f.depth_fail as i32),
                        StencilPassOp: D3D11_STENCIL_OP(f.pass as i32),
                        StencilFunc: D3D11_COMPARISON_FUNC(f.func as i32),
                    };
                    let keep = StencilFace {
                        func: D3D11_COMPARISON_ALWAYS.0 as u8,
                        fail: D3D11_STENCIL_OP_KEEP.0 as u8,
                        depth_fail: D3D11_STENCIL_OP_KEEP.0 as u8,
                        pass: D3D11_STENCIL_OP_KEEP.0 as u8,
                    };
                    let st =
                        k.stencil.unwrap_or(StencilKey { front: keep, back: keep, read_mask: 0xff, write_mask: 0xff });
                    let d = D3D11_DEPTH_STENCIL_DESC {
                        DepthEnable: bool32(k.depth_func != D3D11_COMPARISON_ALWAYS.0 as u8 || k.depth_write),
                        DepthWriteMask: if k.depth_write {
                            D3D11_DEPTH_WRITE_MASK_ALL
                        } else {
                            D3D11_DEPTH_WRITE_MASK_ZERO
                        },
                        DepthFunc: D3D11_COMPARISON_FUNC(k.depth_func as i32),
                        StencilEnable: bool32(k.stencil.is_some()),
                        StencilReadMask: st.read_mask,
                        StencilWriteMask: st.write_mask,
                        FrontFace: face(&st.front),
                        BackFace: face(&st.back),
                    };
                    let mut s = None;
                    self.dev.CreateDepthStencilState(&d, Some(&mut s)).map_err(dx("CreateDepthStencilState"))?;
                    State::Depth(s.expect("CreateDepthStencilState"))
                }
                StateKey::Sampler(k) => {
                    let d = D3D11_SAMPLER_DESC {
                        Filter: D3D11_FILTER(k.filter as i32),
                        AddressU: D3D11_TEXTURE_ADDRESS_MODE(k.address[0] as i32),
                        AddressV: D3D11_TEXTURE_ADDRESS_MODE(k.address[1] as i32),
                        AddressW: D3D11_TEXTURE_ADDRESS_MODE(k.address[2] as i32),
                        MipLODBias: f32::from_bits(k.mip_bias),
                        MaxAnisotropy: k.max_anisotropy.max(1) as u32,
                        ComparisonFunc: if k.comparison == 0 {
                            D3D11_COMPARISON_NEVER
                        } else {
                            D3D11_COMPARISON_FUNC(k.comparison as i32)
                        },
                        BorderColor: k.border.map(f32::from_bits),
                        MinLOD: 0.0,
                        MaxLOD: f32::MAX,
                    };
                    let mut s = None;
                    self.dev.CreateSamplerState(&d, Some(&mut s)).map_err(dx("CreateSamplerState"))?;
                    State::Sampler(s.expect("CreateSamplerState"))
                }
            })
        }
    }

    /// Sets the vertex and pixel shaders (no pixel shader: None); the other stages are cleared.
    pub fn set_shaders(&self, vs: ShaderId, ps: Option<ShaderId>) {
        let c = self.cache();
        let Stage::Vertex(v) = &c.shaders[vs.0 as usize].stage else { panic!("shader {vs:?} is not a vertex shader") };
        // SAFETY: shaders made on this device.
        unsafe {
            self.ctx.VSSetShader(v, None);
            match ps.map(|p| &c.shaders[p.0 as usize].stage) {
                Some(Stage::Pixel(p)) => self.ctx.PSSetShader(p, None),
                None => self.ctx.PSSetShader(None, None),
                Some(_) => panic!("shader {ps:?} is not a pixel shader"),
            }
        }
    }

    /// Sets a geometry shader (None clears it).
    pub fn set_geometry_shader(&self, gs: Option<ShaderId>) {
        let c = self.cache();
        // SAFETY: a shader made on this device.
        unsafe {
            match gs.map(|g| &c.shaders[g.0 as usize].stage) {
                Some(Stage::Geometry(g)) => self.ctx.GSSetShader(g, None),
                None => self.ctx.GSSetShader(None, None),
                Some(_) => panic!("shader {gs:?} is not a geometry shader"),
            }
        }
    }

    /// Sets a compute shader (None clears it).
    pub fn set_compute_shader(&self, cs: Option<ShaderId>) {
        let c = self.cache();
        // SAFETY: a shader made on this device.
        unsafe {
            match cs.map(|g| &c.shaders[g.0 as usize].stage) {
                Some(Stage::Compute(g)) => self.ctx.CSSetShader(g, None),
                None => self.ctx.CSSetShader(None, None),
                Some(_) => panic!("shader {cs:?} is not a compute shader"),
            }
        }
    }

    /// Sets the blend, rasterizer and depth-stencil states and the stencil reference.
    pub fn set_states(&self, blend: StateId, raster: StateId, depth: StateId, stencil_ref: u32) {
        let c = self.cache();
        // SAFETY: states made on this device.
        unsafe {
            match &c.states[blend.0 as usize] {
                State::Blend(b) => self.ctx.OMSetBlendState(b, None, 0xffff_ffff),
                s => panic!("{blend:?} is not a blend state: {s:?}"),
            }
            match &c.states[raster.0 as usize] {
                State::Raster(r) => self.ctx.RSSetState(r),
                s => panic!("{raster:?} is not a rasterizer state: {s:?}"),
            }
            match &c.states[depth.0 as usize] {
                State::Depth(d) => self.ctx.OMSetDepthStencilState(d, stencil_ref),
                s => panic!("{depth:?} is not a depth-stencil state: {s:?}"),
            }
        }
    }

    /// The sampler object of a sampler state.
    pub fn sampler(&self, id: StateId) -> ID3D11SamplerState {
        match &self.cache().states[id.0 as usize] {
            State::Sampler(s) => s.clone(),
            s => panic!("{id:?} is not a sampler: {s:?}"),
        }
    }

    /// Sets the input layout of `elems` for vertex shader `vs`, created once per (vs, `key`): the caller's key
    /// names the element list.
    pub fn set_input_layout(&self, vs: ShaderId, key: u32, elems: &[InputElement]) -> Result<()> {
        if let Some(l) = self.cache().layouts.get(&(vs, key)) {
            // SAFETY: a layout made on this device.
            unsafe { self.ctx.IASetInputLayout(l) };
            return Ok(());
        }
        let mut l = None;
        {
            let code =
                self.cache().shaders[vs.0 as usize].dxbc.clone().expect("an input layout of a non-vertex shader");
            let descs: Vec<D3D11_INPUT_ELEMENT_DESC> = elems
                .iter()
                .map(|e| D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(e.semantic.as_ptr() as *const u8),
                    SemanticIndex: e.index,
                    Format: e.format,
                    InputSlot: e.slot,
                    AlignedByteOffset: 0,
                    InputSlotClass: if e.per_instance {
                        D3D11_INPUT_PER_INSTANCE_DATA
                    } else {
                        D3D11_INPUT_PER_VERTEX_DATA
                    },
                    InstanceDataStepRate: e.per_instance as u32,
                })
                .collect();
            // SAFETY: the element names point at 'static C strings (InputElement's), the code is the vertex
            // shader's DXBC.
            unsafe { self.dev.CreateInputLayout(&descs, &code, Some(&mut l)).map_err(dx("CreateInputLayout"))? };
        }
        let l = l.expect("CreateInputLayout");
        // SAFETY: as above.
        unsafe { self.ctx.IASetInputLayout(&l) };
        self.cache().layouts.entry((vs, key)).or_insert(l);
        Ok(())
    }

    pub fn viewport(&self, rect: [u32; 4]) {
        let vp = D3D11_VIEWPORT {
            TopLeftX: rect[0] as f32,
            TopLeftY: rect[1] as f32,
            Width: rect[2] as f32,
            Height: rect[3] as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        // SAFETY: a plain state call.
        unsafe { self.ctx.RSSetViewports(Some(&[vp])) };
    }

    /// A 2D texture's mip 0 (slice 0) read back as packed rows (an uncompressed format's texels).
    pub fn read_texture(&self, t: &Texture) -> Result<Vec<u8>> {
        let bpp = match format_layout(t.desc.format) {
            Some((1, b)) => b,
            _ => {
                return Err(Error::Format {
                    what: "read-back".into(),
                    why: format!("format {} is not read back", t.desc.format.0),
                });
            }
        };
        if t.desc.kind != TexKind::D2 {
            return Err(Error::Format { what: "read-back".into(), why: "not a 2D texture".into() });
        }
        let [w, h, _] = t.desc.size;
        let td = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: t.desc.format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut st = None;
        let row = (w * bpp) as usize;
        let mut out = vec![0u8; row * h as usize];
        // SAFETY: a staging copy of subresource 0, mapped for reading; each row copy stays within the mapped
        // RowPitch and `out`.
        unsafe {
            self.dev.CreateTexture2D(&td, None, Some(&mut st)).map_err(dx("CreateTexture2D staging"))?;
            let st = st.expect("CreateTexture2D");
            self.ctx.CopySubresourceRegion(&st, 0, 0, 0, 0, &t.res, 0, None);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&st, 0, D3D11_MAP_READ, 0, Some(&mut m)).map_err(dx("Map READ"))?;
            if (m.RowPitch as usize) < row {
                self.ctx.Unmap(&st, 0);
                return Err(Error::Format {
                    what: "read-back".into(),
                    why: format!("row pitch {} under a {row}-byte row", m.RowPitch),
                });
            }
            for y in 0..h as usize {
                let src = (m.pData as *const u8).add(y * m.RowPitch as usize);
                std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(y * row), row);
            }
            self.ctx.Unmap(&st, 0);
        }
        Ok(out)
    }

    fn query(&self, kind: D3D11_QUERY) -> Result<ID3D11Query> {
        let mut q = None;
        let desc = D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 };
        // SAFETY: a query description filled here.
        unsafe { self.dev.CreateQuery(&desc, Some(&mut q)).map_err(dx("CreateQuery"))? };
        Ok(q.expect("CreateQuery"))
    }

    /// A query's data if it is ready (the wrapper's GetData folds S_FALSE, "not ready", into Ok, so the vtable is
    /// called directly); `wait` spins until it is.
    fn query_data<T: Default>(&self, q: &ID3D11Query, wait: bool) -> Option<T> {
        let mut v = T::default();
        loop {
            // SAFETY: GetData writes at most size_of::<T>() bytes into `v`, the query's data size.
            let hr = unsafe {
                (Interface::vtable(&self.ctx).GetData)(
                    Interface::as_raw(&self.ctx),
                    q.as_raw(),
                    &mut v as *mut T as *mut c_void,
                    std::mem::size_of::<T>() as u32,
                    0,
                )
            };
            if hr != S_FALSE {
                return hr.is_ok().then_some(v);
            }
            if !wait {
                return None;
            }
        }
    }

    /// Waits for the GPU to finish what was sent so far (an event query).
    pub fn finish(&self) -> Result<()> {
        let q = self.query(D3D11_QUERY_EVENT)?;
        // SAFETY: plain context calls on a query of this device.
        unsafe {
            self.ctx.End(&q);
            self.ctx.Flush();
        }
        let _: Option<u32> = self.query_data(&q, true);
        Ok(())
    }

    /// The debug layer's messages since the last call (empty without the layer), each as "severity: text".
    pub fn debug_messages(&self) -> Vec<String> {
        let Ok(q) = self.dev.cast::<ID3D11InfoQueue>() else { return Vec::new() };
        let mut out = Vec::new();
        // SAFETY: GetMessage is called once for the size, then into a buffer of that size, 8-aligned for the
        // D3D11_MESSAGE header; the description pointer and length come from the filled message.
        unsafe {
            for k in 0..q.GetNumStoredMessages() {
                let mut len = 0usize;
                if q.GetMessage(k, None, &mut len).is_err() || len == 0 {
                    continue;
                }
                let mut buf = vec![0u64; len.div_ceil(8)];
                let m = buf.as_mut_ptr() as *mut D3D11_MESSAGE;
                if q.GetMessage(k, Some(m), &mut len).is_err() {
                    continue;
                }
                let m = &*m;
                let text = std::slice::from_raw_parts(m.pDescription, m.DescriptionByteLength.saturating_sub(1));
                let severity =
                    ["corruption", "error", "warning", "info", "message"].get(m.Severity.0 as usize).unwrap_or(&"?");
                out.push(format!("{severity}: {}", String::from_utf8_lossy(text)));
            }
            q.ClearStoredMessages();
        }
        out
    }
}

/// A vertex input element: one stream (slot) read from its start, per vertex or per instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputElement {
    pub semantic: &'static std::ffi::CStr,
    pub index: u32,
    pub format: DXGI_FORMAT,
    pub slot: u32,
    pub per_instance: bool,
}

/// Rows as bytes.
pub fn rows_bytes(rows: &[[u32; 4]]) -> &[u8] {
    // SAFETY: [u32; 4] has no padding and u8 has no alignment; the length is the rows' bytes.
    unsafe { std::slice::from_raw_parts(rows.as_ptr() as *const u8, std::mem::size_of_val(rows)) }
}

/// A typed buffer element's bytes.
fn format_bytes(f: DXGI_FORMAT) -> Option<u32> {
    match format_layout(f)? {
        (1, b) => Some(b),
        _ => None,
    }
}

/// A format's block edge in pixels (1 for an uncompressed format) and bytes per block; None for a format the
/// renderer does not make.
pub fn format_layout(f: DXGI_FORMAT) -> Option<(u32, u32)> {
    Some(match f {
        DXGI_FORMAT_R32G32B32A32_FLOAT | DXGI_FORMAT_R32G32B32A32_UINT => (1, 16),
        DXGI_FORMAT_R32G32B32_FLOAT => (1, 12),
        DXGI_FORMAT_R32G32_FLOAT
        | DXGI_FORMAT_R16G16B16A16_FLOAT
        | DXGI_FORMAT_R16G16B16A16_UINT
        | DXGI_FORMAT_R16G16B16A16_SNORM
        | DXGI_FORMAT_R32G8X24_TYPELESS
        | DXGI_FORMAT_D32_FLOAT_S8X24_UINT
        | DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS => (1, 8),
        DXGI_FORMAT_R32_FLOAT
        | DXGI_FORMAT_R32_UINT
        | DXGI_FORMAT_R16G16_FLOAT
        | DXGI_FORMAT_R16G16_SNORM
        | DXGI_FORMAT_R16G16_UNORM
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        | DXGI_FORMAT_R8G8B8A8_SNORM
        | DXGI_FORMAT_R8G8B8A8_UINT
        | DXGI_FORMAT_B8G8R8A8_UNORM
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        | DXGI_FORMAT_R10G10B10A2_UNORM
        | DXGI_FORMAT_R11G11B10_FLOAT => (1, 4),
        DXGI_FORMAT_R16_UINT | DXGI_FORMAT_R16_FLOAT => (1, 2),
        DXGI_FORMAT_R8_UNORM => (1, 1),
        DXGI_FORMAT_BC1_UNORM | DXGI_FORMAT_BC1_UNORM_SRGB | DXGI_FORMAT_BC4_UNORM => (4, 8),
        DXGI_FORMAT_BC2_UNORM
        | DXGI_FORMAT_BC2_UNORM_SRGB
        | DXGI_FORMAT_BC3_UNORM
        | DXGI_FORMAT_BC3_UNORM_SRGB
        | DXGI_FORMAT_BC5_UNORM
        | DXGI_FORMAT_BC6H_UF16
        | DXGI_FORMAT_BC6H_SF16
        | DXGI_FORMAT_BC7_UNORM
        | DXGI_FORMAT_BC7_UNORM_SRGB => (4, 16),
        _ => return None,
    })
}

/// A shader code's cache key when the store's SHA-1 is not at hand: the container's own checksum (bytes 4..20, the
/// compiler's hash of the rest) and its length, in a Sha1's 20 bytes.
fn sha1_of(dxbc: &[u8]) -> mvs_material::Sha1 {
    let mut out = [0u8; 20];
    if dxbc.len() >= 20 {
        out[..16].copy_from_slice(&dxbc[4..20]);
    }
    out[16..].copy_from_slice(&(dxbc.len() as u32).to_le_bytes());
    mvs_material::Sha1(out)
}

/// Frames in flight the timer keeps queries for.
const TIMER_FRAMES: usize = 4;

#[derive(Debug)]
struct TimerFrame {
    disjoint: ID3D11Query,
    start: ID3D11Query,
    marks: Vec<(&'static str, ID3D11Query)>,
    used: usize,
    pending: bool,
}

/// GPU time between marks, read back a few frames late (disjoint query and timestamps).
#[derive(Debug)]
pub struct GpuTimer {
    frames: Vec<TimerFrame>,
    /// The frame being recorded, and the oldest one not read.
    at: usize,
    oldest: usize,
}

impl GpuTimer {
    /// A timer with room for `marks` marks a frame.
    pub fn new(gpu: &Gpu, marks: u32) -> Result<GpuTimer> {
        let mut frames = Vec::with_capacity(TIMER_FRAMES);
        for _ in 0..TIMER_FRAMES {
            let mut m = Vec::with_capacity(marks as usize);
            for _ in 0..marks {
                m.push(("", gpu.query(D3D11_QUERY_TIMESTAMP)?));
            }
            frames.push(TimerFrame {
                disjoint: gpu.query(D3D11_QUERY_TIMESTAMP_DISJOINT)?,
                start: gpu.query(D3D11_QUERY_TIMESTAMP)?,
                marks: m,
                used: 0,
                pending: false,
            });
        }
        Ok(GpuTimer { frames, at: 0, oldest: 0 })
    }

    pub fn begin_frame(&mut self, gpu: &Gpu) {
        let f = &mut self.frames[self.at];
        if f.pending {
            // The ring is full: the oldest frame is dropped unread.
            f.pending = false;
            self.oldest = (self.at + 1) % TIMER_FRAMES;
        }
        f.used = 0;
        // SAFETY: queries of this device.
        unsafe {
            gpu.ctx.Begin(&f.disjoint);
            gpu.ctx.End(&f.start);
        }
    }

    /// Ends the span named `name`, begun at the frame's start or its last mark.
    pub fn mark(&mut self, gpu: &Gpu, name: &'static str) {
        let f = &mut self.frames[self.at];
        let Some(slot) = f.marks.get_mut(f.used) else { panic!("GPU timer: more than {} marks", f.marks.len()) };
        slot.0 = name;
        // SAFETY: a query of this device.
        unsafe { gpu.ctx.End(&slot.1) };
        f.used += 1;
    }

    pub fn end_frame(&mut self, gpu: &Gpu) {
        let f = &mut self.frames[self.at];
        // SAFETY: a query of this device.
        unsafe { gpu.ctx.End(&f.disjoint) };
        f.pending = true;
        self.at = (self.at + 1) % TIMER_FRAMES;
    }

    /// The oldest finished frame's spans, ms, in mark order; None while none has finished.
    pub fn read(&mut self, gpu: &Gpu) -> Option<Vec<(&'static str, f64)>> {
        let f = &mut self.frames[self.oldest];
        if !f.pending {
            return None;
        }
        let d: D3D11_QUERY_DATA_TIMESTAMP_DISJOINT = gpu.query_data(&f.disjoint, false)?;
        let mut last: u64 = gpu.query_data(&f.start, false)?;
        let mut out = Vec::with_capacity(f.used);
        for (name, q) in &f.marks[..f.used] {
            let t: u64 = gpu.query_data(q, false)?;
            out.push((
                *name,
                if d.Disjoint.as_bool() { f64::NAN } else { (t - last) as f64 * 1e3 / d.Frequency as f64 },
            ));
            last = t;
        }
        f.pending = false;
        self.oldest = (self.oldest + 1) % TIMER_FRAMES;
        Some(out)
    }
}
