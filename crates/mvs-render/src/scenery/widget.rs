//! A block's world-space widgets (a WidgetComponent in World space, the plane geometry: The Court's JumboTron
//! scoreboard): the block says what the widget shows and where its quad
//! stands (`SceneryHost::set_widget`); the UI crate paints the widget into the target the renderer hands it
//! (`Renderer::scenery_widget`, `scenery_widget_target`); the renderer draws the quad with that picture in the
//! translucent pass. A widget no one painted is not drawn (render-frame has no UI).
// Unverified: the quad is drawn as its picture's colour (sRGB decoded) and alpha, unfogged, first in the separate
// translucency pass (render order -100000 in its translucent layer); the build's
// Widget3DPassThrough_Translucent material and its render target's gamma are not read. Both sides are drawn (the
// build's quad is one-sided; the camera never sees its back).

use std::sync::Arc;

use mvs_view::{Camera, M4};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

use super::host::SceneryHost;
use crate::Result;
use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, StateId, StateKey, TexDesc,
    TexKind, TexViews, Texture, dx,
};
use crate::lighting::compile;
use crate::renderer::Renderer;

/// What a world-space widget shows, by its widget class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidgetPaint {
    /// UI_SpaceJam_Scoreboard: its EnterAnim `enter` seconds in; each side (by TeamIndex: 0
    /// UI_SpaceJam_ScoreboardTeam_Right, 1 _Left) its score and its ScoreChangeAnim's seconds since the score last
    /// changed (None: never, the animation at its end). The painter holds a time to its animation's end.
    Scoreboard { enter: f64, scores: [i32; 2], changed: [Option<f64>; 2] },
}

/// A block's world-space widget as the last frame left it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldWidget {
    /// The block's name for it ("court.scoreboard").
    pub key: &'static str,
    /// The widget class (the hud bundle's).
    pub class: &'static str,
    /// The component's DrawSize: its render target's pixels.
    pub size: [u32; 2],
    /// The quad's UE transform (row-vector): the unit square of the component's local Y-Z plane, centred (pivot
    /// 0.5), its rows Y and Z at the draw size. The picture's top left is at (+Y, +Z): u along -Y, v along -Z
    /// (FWidget3DSceneProxy's vertices).
    pub place: M4,
    /// None: hidden (SetVisibility 2).
    pub paint: Option<WidgetPaint>,
}

/// A widget of the scenery's state and its picture (made at its first paint).
#[derive(Debug)]
pub(crate) struct SceneryWidget {
    pub(crate) spec: WorldWidget,
    pub(crate) target: Option<Arc<Texture>>,
}

impl SceneryHost<'_> {
    /// The block's widget `w.key` as this frame shows it (made at the first call).
    pub fn set_widget(&mut self, w: WorldWidget) {
        let ws = &mut self.state.widgets;
        match ws.iter_mut().find(|x| x.spec.key == w.key) {
            Some(x) => {
                if x.spec.size != w.size {
                    x.target = None;
                }
                x.spec = w;
            }
            None => ws.push(SceneryWidget { spec: w, target: None }),
        }
    }
}

/// The shown widgets that have a picture, as the frame's quads.
pub(crate) fn scenery_quads(widgets: &[SceneryWidget]) -> Vec<WidgetQuad> {
    widgets
        .iter()
        .filter(|w| w.spec.paint.is_some())
        .filter_map(|w| Some(WidgetQuad { place: w.spec.place, picture: w.target.clone()?, first: true }))
        .collect()
}

impl Renderer {
    /// The scenery's world-space widget `k` (in the order the blocks made them) as the last `scenery_frame` left
    /// it; None past the last. The UI paints the shown ones into their targets before `draw`.
    pub fn scenery_widget(&self, k: usize) -> Option<WorldWidget> {
        self.scenery.as_ref()?.state.widgets.get(k).map(|w| w.spec)
    }

    /// The picture of the widget `key` (RGBA8, its draw size, rows top first), cleared to transparent, for the UI
    /// to paint into: its alpha the coverage, its colour premultiplied by it (the Slate batches' blend over a clear
    /// target). From the first call on, the frames that show the widget draw its quad with it. None: no such widget.
    pub fn scenery_widget_target(&mut self, key: &str) -> Result<Option<Arc<Texture>>> {
        let Some(s) = &mut self.scenery else { return Ok(None) };
        let Some(w) = s.state.widgets.iter_mut().find(|w| w.spec.key == key) else { return Ok(None) };
        let gpu = &self.gpu;
        if w.target.is_none() {
            let [x, y] = w.spec.size;
            let desc = TexDesc {
                kind: TexKind::D2,
                size: [x, y, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            };
            w.target = Some(Arc::new(gpu.texture(&desc, &[])?));
        }
        let t = w.target.clone().expect("made above");
        // SAFETY: a render target view of this device's texture; the clear colour is four floats.
        unsafe { gpu.ctx.ClearRenderTargetView(t.rtv.as_ref().expect("a render target view"), &[0.0; 4]) };
        Ok(Some(t))
    }
}

/// An item of sort keys `key` (priority, after the meshes, view depth) draws before a priority 0 quad at view depth
/// `z`: a lower priority, or priority 0, not after the meshes and at least as far (a tie keeps the item first).
fn draws_before(key: (f32, bool, f32), z: f32) -> bool {
    let (sort, after, iz) = key;
    sort < 0.0 || (sort == 0.0 && !after && iz >= z)
}

/// The quad: its corners' clip positions from the constants (u, v: (0, 0), (0, 1), (1, 1), (1, 0)), two
/// triangles; the picture un-premultiplied, sRGB decoded (straight alpha, sRGB colour space).
pub(crate) const QUAD: &str = r"
cbuffer Quad : register(b0) { float4 Corner[4]; };
Texture2D<float4> Picture : register(t0);
SamplerState Linear : register(s0);
void MainVS(uint id : SV_VertexID, out float4 pos : SV_Position, out float2 uv : TEXCOORD0) {
    uint k = id < 3 ? id : (id == 3 ? 0 : id - 2);
    pos = Corner[k];
    uv = float2(k >= 2 ? 1 : 0, (k == 1 || k == 2) ? 1 : 0);
}
float4 MainPS(float4 pos : SV_Position, float2 uv : TEXCOORD0) : SV_Target0 {
    float4 c = Picture.Sample(Linear, uv);
    if (c.a <= 0) return float4(0, 0, 0, 0);
    float3 s = saturate(c.rgb / c.a);
    float3 lin = s <= 0.04045 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4);
    return float4(lin, c.a);
}
";

/// A world-space widget's quad in a frame: where it stands and its picture (the scenery's widgets, a menu level's).
#[derive(Clone, Debug)]
pub struct WidgetQuad {
    /// As `WorldWidget::place`: the unit square of the local Y-Z plane, its rows Y and Z at the picture's size.
    pub place: M4,
    /// RGBA8, the colour premultiplied by the alpha (Slate's batches over a target cleared to transparent).
    pub picture: Arc<Texture>,
    /// Drawn before every translucent item (the scenery's: render order -100000 in its translucent layer);
    /// else among the AfterDOF items at TranslucencySortPriority 0 by the view depth of its centre, far first (the
    /// menu levels': render order -view depth).
    pub first: bool,
}

/// A prepared quad: its corners' constants, picture, view depth and the index among the pass's sorted items it
/// draws before.
#[derive(Debug)]
struct Prepared {
    buf: Buffer,
    picture: Arc<Texture>,
    first: bool,
    view_z: f32,
    at: usize,
}

/// The quads' shaders and states, and the frame's quads (`prepare`), drawn by the translucent pass.
#[derive(Debug)]
pub(crate) struct WidgetQuads {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    blend: StateId,
    raster: StateId,
    depth: StateId,
    sampler: ID3D11SamplerState,
    /// The frame's quads, in their draw order once `place_among` ran.
    frame: Vec<Prepared>,
    /// Constant buffers kept between frames.
    spare: Vec<Buffer>,
}

/// The clip position of the UE point `p` (ProjectWorldToScreen's matrices, mvs-ui hud.rs project).
fn clip_of(cam: &Camera, p: [f64; 3]) -> [f32; 4] {
    let t = [0, 1, 2].map(|i| p[i] - cam.origin[i]);
    let m = &cam.translated_world_to_view;
    let v: [f64; 4] = std::array::from_fn(|j| t[0] * m[0][j] + t[1] * m[1][j] + t[2] * m[2][j] + m[3][j]);
    let c = &cam.view_to_clip;
    std::array::from_fn(|j| (0..4).map(|k| v[k] * c[k][j]).sum::<f64>() as f32)
}

/// The quad's corners (u, v order of `QUAD`) in UE world space: local (0, y, z) for y, z = +-0.5 through `place`.
fn corners(place: &M4) -> [[f64; 3]; 4] {
    [(0.5, 0.5), (0.5, -0.5), (-0.5, -0.5), (-0.5, 0.5)]
        .map(|(y, z)| std::array::from_fn(|i| y * place[1][i] + z * place[2][i] + place[3][i]))
}

impl WidgetQuads {
    pub(crate) fn new(gpu: &Gpu) -> Result<WidgetQuads> {
        let vs_code = compile(QUAD, "MainVS", "vs_5_0")?;
        let ps_code = compile(QUAD, "MainPS", "ps_5_0")?;
        let (mut vs, mut ps) = (None, None);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader widget"))?;
            gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader widget"))?;
        }
        // BLEND_Translucent's state (SetTranslucentRenderState 14193f5a0): colour SrcAlpha / InvSrcAlpha, alpha Zero /
        // InvSrcAlpha (the separate target's alpha the scene's transmittance).
        // Unverified: the build's text material is not read; its blend mode is taken as BLEND_Translucent.
        let t = BlendTarget {
            color_op: D3D11_BLEND_OP_ADD.0 as u8,
            color_src: D3D11_BLEND_SRC_ALPHA.0 as u8,
            color_dst: D3D11_BLEND_INV_SRC_ALPHA.0 as u8,
            alpha_op: D3D11_BLEND_OP_ADD.0 as u8,
            alpha_src: D3D11_BLEND_ZERO.0 as u8,
            alpha_dst: D3D11_BLEND_INV_SRC_ALPHA.0 as u8,
            write_mask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let mut targets = [BlendTarget::OPAQUE; 8];
        targets[0] = t;
        let sampler =
            gpu.state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP)));
        Ok(WidgetQuads {
            vs: vs.expect("CreateVertexShader"),
            ps: ps.expect("CreatePixelShader"),
            blend: gpu.state(StateKey::Blend(BlendKey { targets, alpha_to_coverage: false })),
            raster: gpu.state(StateKey::Raster(RasterKey {
                fill: D3D11_FILL_SOLID.0 as u8,
                cull: D3D11_CULL_NONE.0 as u8,
                front_ccw: false,
                depth_bias: 0,
                slope_scaled_depth_bias: 0,
            })),
            // the translucent meshes' test (reversed Z), no write
            depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_GREATER_EQUAL.0 as u8,
                stencil: None,
            })),
            sampler: gpu.sampler(sampler),
            frame: Vec::new(),
            spare: Vec::new(),
        })
    }

    /// The frame's quads seen from `cam`; `view_z` gives a UE point's view depth (the translucent sort's).
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        quads: &[WidgetQuad],
        cam: &Camera,
        view_z: impl Fn([f64; 3]) -> f32,
    ) -> Result<()> {
        self.spare.extend(self.frame.drain(..).map(|p| p.buf));
        for q in quads {
            let rows = corners(&q.place).map(|p| clip_of(cam, p).map(f32::to_bits));
            let buf = match self.spare.pop() {
                Some(b) => b,
                None => gpu.buffer(&BufDesc { kind: BufKind::Constant, size: 64, dynamic: true }, None)?,
            };
            gpu.write_rows(&buf, &rows);
            let centre = [q.place[3][0], q.place[3][1], q.place[3][2]];
            let view_z = view_z(centre);
            self.frame.push(Prepared { buf, picture: q.picture.clone(), first: q.first, view_z, at: 0 });
        }
        Ok(())
    }

    /// Each quad's place among the AfterDOF pass's sorted items (`before` an item's priority, its after-meshes flag
    /// and view depth: the sort's keys), the quads in their draw order: the first ones, then far to near.
    pub(crate) fn place_among<T>(&mut self, items: &[T], key: impl Fn(&T) -> (f32, bool, f32)) {
        // (a stable sort: equal quads keep the order they were given in)
        self.frame.sort_by(|a, b| b.first.cmp(&a.first).then(b.view_z.total_cmp(&a.view_z)));
        for q in &mut self.frame {
            q.at = if q.first { 0 } else { items.partition_point(|it| draws_before(key(it), q.view_z)) };
        }
    }

    /// The indices among the pass's items the quads draw before, ascending (each once).
    pub(crate) fn places(&self) -> Vec<usize> {
        let mut at: Vec<usize> = self.frame.iter().map(|q| q.at).collect();
        at.dedup();
        at
    }

    /// The frame's quads that draw before the pass's item `at` into the bound target (the separate translucency
    /// colour) and read-only depth.
    pub(crate) fn draw(&self, gpu: &Gpu, at: usize) {
        if !self.frame.iter().any(|q| q.at == at) {
            return;
        }
        gpu.set_states(self.blend, self.raster, self.depth, 0);
        // SAFETY: shaders, buffers and views of this device; the picture is unbound before the return.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            for q in self.frame.iter().filter(|q| q.at == at) {
                ctx.VSSetConstantBuffers(0, Some(&[Some(q.buf.buf.clone())]));
                ctx.PSSetShaderResources(0, Some(std::slice::from_ref(&q.picture.srv)));
                ctx.Draw(6, 0);
            }
            ctx.PSSetShaderResources(0, Some(&[None]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quad_compiles() {
        assert!(!compile(QUAD, "MainVS", "vs_5_0").expect("vs").is_empty());
        assert!(!compile(QUAD, "MainPS", "ps_5_0").expect("ps").is_empty());
    }

    /// A quad among the sorted items: after the lower priorities and the farther items of priority 0, before the
    /// nearer ones, the after-meshes ones and the higher priorities.
    #[test]
    fn quads_sort_among_the_items() {
        // (priority, after meshes, view depth) in the pass's order
        let items =
            [(-20.0, false, 900.0), (0.0, false, 800.0), (0.0, false, 500.0), (0.0, true, 900.0), (1.0, false, 900.0)];
        let at = |z: f32| items.partition_point(|&k| draws_before(k, z));
        assert_eq!(at(1000.0), 1);
        assert_eq!(at(800.0), 2);
        assert_eq!(at(600.0), 2);
        assert_eq!(at(100.0), 3);
    }

    /// The corners in u, v order: (0, 0) at (+Y, +Z), (1, 1) at (-Y, -Z).
    #[test]
    fn corners_follow_the_proxy() {
        let place = [[1.0, 0.0, 0.0, 0.0], [0.0, 950.0, 0.0, 0.0], [0.0, 0.0, 950.0, 0.0], [10.0, 20.0, 30.0, 1.0]];
        let c = corners(&place);
        assert_eq!(c[0], [10.0, 495.0, 505.0]);
        assert_eq!(c[2], [10.0, -455.0, -445.0]);
    }
}
