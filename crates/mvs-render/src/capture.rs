//! PFGCamera's priority captures: per capture N (1..=3) a view from the camera's
//! place at PRIORITY_CAPTURE_SIZE drawing the priority stack's first N fighters depth only, then their scene depth
//! in cm written to TargetId::PriorityCapture(N), which the fighters' materials read as LocalPlayerSceneDepth.
//!
//! The captures are SceneCaptureComponent2D_Priority1..3 (children of the CameraComponent; PRM_UseShowOnlyList,
//! SCS_SceneDepth, RTF_R16f, FOVAngle = CameraComponent.FieldOfView). Capture N shows the stack's indexes 0..N-1,
//! so the three share one depth buffer: the fighters of index N-1 are added to it before capture N is written.

use mvs_view::Camera;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::Result;
use crate::draw::{DrawItem, PassMask, execute};
use crate::gpu::{
    BlendKey, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, StateId, StateKey, TexDesc, TexKind, TexViews,
    Texture, dx,
};
use crate::lighting::compile;
use crate::material::PassKind;
use crate::renderer::Frame;
use crate::targets::{PRIORITY_CAPTURE_FAR, PRIORITY_CAPTURE_SIZE, TargetId};
use crate::view::{VIEW_ROWS, ViewMembers, ViewParams, view_rows};

/// The depth-to-scene-depth pass (the port's own): a full-screen triangle reading the capture's device Z and writing
/// ConvertFromDeviceZ's scene depth (cm), the far value where nothing was drawn.
const SOURCE: &str = r"
cbuffer Constants : register(b0) { float4 P; }
Texture2D<float> Depth : register(t0);
void MainVS(uint id : SV_VertexID, out float4 pos : SV_Position) {
    float2 uv = float2((id << 1) & 2, id & 2);
    pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float MainPS(float4 pos : SV_Position) : SV_Target {
    float z = Depth.Load(int3(pos.xy, 0));
    if (z <= 0) return P.z;
    return P.y / (z - P.x);
}
";

/// The capture pass: its depth target, shaders and states.
#[derive(Debug)]
pub struct Captures {
    depth: Texture,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    constants: Buffer,
    blend: StateId,
    raster: StateId,
    no_depth: StateId,
    /// The capture's View rows and the two the camera's rows are told apart by (kept between frames).
    rows: Box<[[[u32; 4]; VIEW_ROWS]; 3]>,
    /// The items of the capture being drawn, in the depth pass (kept between frames).
    items: Vec<DrawItem>,
}

impl Captures {
    pub fn new(gpu: &Gpu) -> Result<Captures> {
        let [w, h] = PRIORITY_CAPTURE_SIZE;
        let depth = gpu.texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format: DXGI_FORMAT_R32G8X24_TYPELESS,
                views: TexViews { srv: true, dsv: true, ..Default::default() },
            },
            &[],
        )?;
        let vs_code = compile(SOURCE, "MainVS", "vs_5_0")?;
        let ps_code = compile(SOURCE, "MainPS", "ps_5_0")?;
        let (mut vs, mut ps) = (None, None);
        // SAFETY: DXBC just compiled for this device's shader model.
        unsafe {
            gpu.dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(dx("CreateVertexShader capture"))?;
            gpu.dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(dx("CreatePixelShader capture"))?;
        }
        Ok(Captures {
            depth,
            vs: vs.expect("CreateVertexShader"),
            ps: ps.expect("CreatePixelShader"),
            constants: gpu.buffer(&BufDesc { kind: BufKind::Constant, size: 16, dynamic: true }, None)?,
            blend: gpu.state(StateKey::Blend(BlendKey::OPAQUE)),
            raster: gpu.state(StateKey::Raster(RasterKey {
                fill: D3D11_FILL_SOLID.0 as u8,
                cull: D3D11_CULL_NONE.0 as u8,
                front_ccw: false,
                depth_bias: 0,
                slope_scaled_depth_bias: 0,
            })),
            no_depth: gpu.state(StateKey::Depth(DepthKey {
                depth_write: false,
                depth_func: D3D11_COMPARISON_ALWAYS.0 as u8,
                stencil: None,
            })),
            rows: Box::new([[[0; 4]; VIEW_ROWS]; 3]),
            items: Vec::new(),
        })
    }

    /// The frame's captures, before the main view's base pass: the captures' View rows written, the three drawn,
    /// and `main_view`'s rows written back after the last.
    // The captures are enqueued before the main view of the same frame, with its poses (1429a1af0, 141cbf270).
    // Deviation: the build's SCS_SceneDepth renders the shown primitives' full deferred scene and copies its depth
    // through FSceneCapturePS permutation 3; the port draws their depth pass only (cooked TDepthOnlyVS and, for a
    // masked material, its FDepthOnlyPS), the same depth for opaque and masked sections, translucent ones left out.
    // Unverified: every section of the shown meshes is drawn, two-sided and unmasked, with its own depth material.
    pub fn draw(&mut self, fr: &mut Frame, main_view: &[[u32; 4]; VIEW_ROWS]) -> Result<()> {
        let gpu = fr.gpu;
        let [w, h] = PRIORITY_CAPTURE_SIZE;
        let cam = fr.input.camera;
        let capture = capture_camera(cam);
        // The capture's rows: the camera's terms from its own view, the rest (the stage's, the lighting's) as the
        // main view has them. A row that differs between the two views drawn with no members is the camera's.
        let at = |camera: Camera, rect: [u32; 4], buffer: [u32; 2]| ViewParams {
            camera,
            rect,
            buffer,
            time: fr.input.time,
            frame: 0,
            members: ViewMembers::default(),
        };
        let size = fr.targets.size;
        view_rows(&at(cam.clone(), [0, 0, size[0], size[1]], size), &mut self.rows[0]);
        view_rows(&at(capture.clone(), [0, 0, w, h], [w, h]), &mut self.rows[1]);
        let [main_bare, capture_bare, out] = &mut *self.rows;
        for k in 0..VIEW_ROWS {
            out[k] = if main_bare[k] == capture_bare[k] { main_view[k] } else { capture_bare[k] };
        }
        gpu.write_rows(&fr.scene.view, &self.rows[2]);
        let p = &capture.view_to_clip;
        gpu.write_rows(
            &self.constants,
            &[[p[2][2] as f32, p[3][2] as f32, PRIORITY_CAPTURE_FAR, 0.0].map(f32::to_bits)],
        );

        let dsv = self.depth.dsv.as_ref().expect("a depth view");
        // SAFETY: a view of this device's target.
        unsafe { gpu.ctx.ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0, 0.0, 0) };
        for n in 0..3u8 {
            // The fighters of stack index n join the depth buffer.
            self.items.clear();
            for &(s, i) in &fr.list.captures {
                if s == n {
                    let mut item = fr.list.opaque[i as usize].clone();
                    item.passes = PassMask::of(&[PassKind::DepthOnly]);
                    self.items.push(item);
                }
            }
            if !self.items.is_empty() {
                // SAFETY: the depth view of this device's target, no colour target.
                unsafe { gpu.ctx.OMSetRenderTargets(None, dsv) };
                gpu.viewport([0, 0, w, h]);
                execute(gpu, fr.assets, fr.scene, fr.targets, fr.materials, &self.items, PassKind::DepthOnly, None)?;
            }
            // Capture n + 1: the scene depth of the stack's indexes 0..=n.
            let target = fr.targets.get(TargetId::PriorityCapture(n + 1));
            gpu.viewport([0, 0, w, h]);
            gpu.set_geometry_shader(None);
            gpu.set_states(self.blend, self.raster, self.no_depth, 0);
            // SAFETY: shaders, views and buffers of this device; the depth target is unbound before its view is read,
            // and the SRV is unbound after the draw.
            unsafe {
                let ctx = &gpu.ctx;
                ctx.OMSetRenderTargets(Some(std::slice::from_ref(&target.rtv)), None);
                ctx.IASetInputLayout(None);
                ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                ctx.VSSetShader(&self.vs, None);
                ctx.PSSetShader(&self.ps, None);
                ctx.PSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
                ctx.PSSetShaderResources(0, Some(std::slice::from_ref(&self.depth.srv)));
                ctx.Draw(3, 0);
                ctx.PSSetShaderResources(0, Some(&[None]));
                ctx.OMSetRenderTargets(None, None);
            }
        }
        gpu.write_rows(&fr.scene.view, main_view);
        Ok(())
    }
}

/// The capture's camera: the main camera's place and horizontal field of view, at the capture's aspect.
fn capture_camera(main: &Camera) -> Camera {
    let [w, h] = PRIORITY_CAPTURE_SIZE;
    let mut c = main.clone();
    c.view_to_clip[1][1] = c.view_to_clip[0][0] * (w as f64 / h as f64);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_keeps_the_horizontal_fov() {
        let mut m = Camera {
            origin: [0.0; 3],
            translated_world_to_view: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]],
            view_to_clip: [[0.0; 4]; 4],
        };
        m.view_to_clip[0][0] = 1.0;
        m.view_to_clip[1][1] = 16.0 / 9.0;
        let c = capture_camera(&m);
        assert_eq!(c.view_to_clip[0][0], 1.0);
        assert!((c.view_to_clip[1][1] - 1024.0 / 578.0).abs() < 1e-12);
    }
}
