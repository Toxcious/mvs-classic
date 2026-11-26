//! (the port's own) A look: a visual idea drawn into a real frame before it is built as a feature, the preview
//! binary's (`bin/preview.rs`). A look is a folder: `look.json` ([`LookFile`]) and one HLSL file of pixel shaders
//! that starts from `look/prelude.hlsl`. The renderer draws the look's shapes (world rectangles shaded by its
//! shaders, behind what stands in front of them) and its screen passes (shaders over the whole picture) into
//! SceneColor after the translucency and before the post chain, so bloom, tonemapping and outlines treat them as the
//! scene. The preview binary adds the look's lights, meshes, particles and camera to the frame itself.
//!
//! In a match the renderer draws no look but an approved one built as a feature: Rising Lava's (`lava.rs`).

use std::path::Path;
use std::sync::Arc;

use mvs_d3d::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP;
use mvs_d3d::Direct3D11::*;
use mvs_view::Camera;
use serde::Deserialize;

use crate::gpu::{
    BlendKey, BlendTarget, BufDesc, BufKind, Buffer, DepthKey, Gpu, RasterKey, SamplerKey, StateId, StateKey, TexDesc,
    TexKind, TexViews, Texture, dx,
};
use crate::lighting::compile;
use crate::renderer::Frame;
use crate::{Error, Result};

const PRELUDE: &str = include_str!("look/prelude.hlsl");

/// The store textures a look binds (Tex0..Tex7).
const TEXTURES: usize = 8;
/// The `params` rows the shaders read.
const PARAMS: usize = 8;

/// `look.json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookFile {
    /// The HLSL file beside it, its pixel shaders.
    pub shader: Option<String>,
    /// Store texture paths, bound as Tex0.. in order.
    #[serde(default)]
    pub textures: Vec<String>,
    /// Rows the shaders read as Params[0..8].
    #[serde(default)]
    pub params: Vec<[f32; 4]>,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    /// Pixel shaders run over the whole picture, in order; each one's output replaces the scene's colour.
    #[serde(default)]
    pub screen: Vec<String>,
    #[serde(default)]
    pub lights: Vec<LookLight>,
    #[serde(default)]
    pub meshes: Vec<LookMesh>,
    #[serde(default)]
    pub particles: Vec<LookParticles>,
    #[serde(default)]
    pub camera: LookCamera,
}

/// A rectangle in the world shaded by a pixel shader of the look's file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    /// The pixel shader: `float4 <entry>(ShapeIn i)`.
    pub entry: String,
    /// World centre (cm, UE axes: Z up).
    pub center: [f64; 3],
    /// Half axes (cm): +u is the shape's right edge, +v its top edge.
    pub u: [f64; 3],
    pub v: [f64; 3],
    #[serde(default)]
    pub blend: Blend,
    /// Its move (cm a second).
    #[serde(default)]
    pub velocity: [f64; 3],
}

/// How a shape's colour joins the scene's.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    /// Over the scene by its alpha.
    #[default]
    Alpha,
    /// Added to the scene (glows).
    Add,
    /// Replaces the scene.
    Opaque,
}

/// A point light added to the stage's.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookLight {
    pub at: [f64; 3],
    /// Linear colour times intensity.
    pub color: [f32; 3],
    /// Attenuation radius (cm).
    pub radius: f32,
    /// Its move (cm a second).
    #[serde(default)]
    pub velocity: [f64; 3],
}

/// A store mesh placed in the stage.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookMesh {
    pub mesh: String,
    /// Its sections' materials in slot order.
    pub materials: Vec<String>,
    pub at: [f64; 3],
    /// Pitch, yaw, roll (degrees).
    #[serde(default)]
    pub rotation: [f64; 3],
    #[serde(default = "one3")]
    pub scale: [f64; 3],
}

/// A store particle system spawned at the look's start.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookParticles {
    pub system: String,
    pub at: [f64; 3],
    #[serde(default)]
    pub rotation: [f64; 3],
    #[serde(default = "one3")]
    pub scale: [f64; 3],
}

/// Changes to the match camera.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookCamera {
    /// Roll (degrees, positive turns the picture clockwise) and its swing: roll + swing x sin(2 pi t / period).
    #[serde(default)]
    pub roll: f64,
    #[serde(default)]
    pub swing: f64,
    #[serde(default = "default_period")]
    pub period: f64,
    /// Field of view scale: above 1 zooms in.
    #[serde(default = "one")]
    pub zoom: f64,
}

impl Default for LookCamera {
    fn default() -> LookCamera {
        LookCamera { roll: 0.0, swing: 0.0, period: default_period(), zoom: 1.0 }
    }
}

fn one() -> f64 {
    1.0
}

fn one3() -> [f64; 3] {
    [1.0; 3]
}

fn default_period() -> f64 {
    4.0
}

impl LookFile {
    pub fn read(path: &Path) -> Result<LookFile> {
        let text = std::fs::read_to_string(path).map_err(|e| bad(path, e))?;
        serde_json::from_str(&text).map_err(|e| bad(path, e))
    }
}

impl LookCamera {
    /// `cam` rolled and zoomed at look time `t`.
    pub fn apply(&self, cam: &mut Camera, t: f64) {
        let roll = self.roll + self.swing * (std::f64::consts::TAU * t / self.period.max(0.01)).sin();
        if roll != 0.0 {
            // View space is X right, Y up, Z forward, points taken as rows: a roll turns the view's X and Y columns.
            let (s, c) = roll.to_radians().sin_cos();
            for row in &mut cam.translated_world_to_view {
                let (x, y) = (row[0], row[1]);
                row[0] = x * c + y * s;
                row[1] = y * c - x * s;
            }
        }
        cam.view_to_clip[0][0] *= self.zoom;
        cam.view_to_clip[1][1] *= self.zoom;
    }

    pub fn moves(&self) -> bool {
        self.roll != 0.0 || self.swing != 0.0 || self.zoom != 1.0
    }
}

impl LookLight {
    /// The light as the lighting pass takes it, at look time `t`.
    pub fn light(&self, k: usize, t: f64) -> crate::scene::Light {
        crate::scene::Light {
            name: format!("look light {k}"),
            kind: crate::scene::LightKind::Point,
            position: [0, 1, 2].map(|i| self.at[i] + self.velocity[i] * t),
            direction: [0.0, 0.0, -1.0],
            tangent: [0.0, 0.0, -1.0],
            color: self.color,
            inv_radius: 1.0 / self.radius.max(1.0),
            // (not 0, which is inverse-squared: the colour then wants thousands; this fades to 0 at the radius)
            falloff_exponent: 2.0,
            // A point light's SpotAngles.
            spot_angles: [-2.0, 1.0],
            source_radius: 0.0,
            soft_source_radius: 0.0,
            source_length: 0.0,
            specular_scale: 1.0,
            channels: 1,
            shadow_channel: -1,
            cast_shadows: false,
            translucent: true,
        }
    }
}

fn bad(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::Format { what: path.display().to_string(), why: e.to_string() }
}

/// The HLSL a look of `file` compiles: the prelude, the shader file (its name and text), then each shape's entry
/// behind a test of the scene's depth (hidden where the scene stands in front). It depends on the shapes' entries
/// alone, not where they stand, so a look's source is the same every frame (`shaders.rs` lists it for Linux).
pub(crate) fn source(file: &LookFile, shader: Option<(&str, &str)>) -> String {
    let mut src = PRELUDE.to_string();
    if let Some((name, text)) = shader {
        src.push_str(&format!("#line 1 \"{name}\"\n"));
        src.push_str(text);
    }
    for (k, s) in file.shapes.iter().enumerate() {
        src.push_str(&format!(
            "\nfloat4 LookShape{k}(ShapeIn i) : SV_Target {{ if (SceneDepth(i.pos.xy) < i.depth - 1) discard; return {}(i); }}\n",
            s.entry
        ));
    }
    src
}

/// Every (entry, target) a look of `file` compiles: its two vertex shaders, a pixel shader per shape, then one per
/// screen pass.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn entries(file: &LookFile) -> Vec<(String, &'static str)> {
    let mut e = vec![("ShapeVS".to_owned(), "vs_5_0"), ("ScreenVS".to_owned(), "vs_5_0")];
    e.extend((0..file.shapes.len()).map(|k| (format!("LookShape{k}"), "ps_5_0")));
    e.extend(file.screen.iter().map(|s| (s.clone(), "ps_5_0")));
    e
}

/// A shape's pixel shader and blend.
#[derive(Debug)]
struct ShapeDraw {
    ps: ID3D11PixelShader,
    blend: StateId,
}

/// A loaded look: its file, compiled shaders, textures and the frame's clock.
#[derive(Debug)]
pub struct Look {
    pub file: LookFile,
    /// Seconds since the look's start, set by the caller before each frame.
    pub time: f32,
    shape_vs: ID3D11VertexShader,
    screen_vs: ID3D11VertexShader,
    shapes: Vec<ShapeDraw>,
    screens: Vec<ID3D11PixelShader>,
    textures: Vec<Arc<Texture>>,
    constants: Buffer,
    /// SceneColor's copy the shaders read while SceneColor is drawn to.
    copy: Option<Texture>,
    raster: StateId,
    no_depth: StateId,
    opaque: StateId,
    wrap: StateId,
    clamp: StateId,
}

impl Look {
    /// The look in `dir`: `look.json` read, its shader file compiled against the prelude, its textures loaded.
    pub fn load(gpu: &Gpu, assets: &crate::assets::Assets, dir: &Path) -> Result<Look> {
        let file = LookFile::read(&dir.join("look.json"))?;
        let shader = match &file.shader {
            Some(name) => {
                let path = dir.join(name);
                Some((name.clone(), std::fs::read_to_string(&path).map_err(|e| bad(&path, e))?))
            }
            None => None,
        };
        Look::build(gpu, assets, file, shader.as_ref().map(|(n, s)| (n.as_str(), s.as_str())), dir)
    }

    /// The look of `file` with its shader's name and source: compiled against the prelude, its textures loaded.
    /// `dir` names it in an error.
    pub fn build(
        gpu: &Gpu,
        assets: &crate::assets::Assets,
        file: LookFile,
        shader: Option<(&str, &str)>,
        dir: &Path,
    ) -> Result<Look> {
        let src = source(&file, shader);
        if file.textures.len() > TEXTURES {
            return Err(bad(dir, format!("{} textures, at most {TEXTURES}", file.textures.len())));
        }
        if file.params.len() > PARAMS {
            return Err(bad(dir, format!("{} params rows, at most {PARAMS}", file.params.len())));
        }
        let vs = |entry: &str| -> Result<ID3D11VertexShader> {
            let code = compile(&src, entry, "vs_5_0")?;
            let mut s = None;
            // SAFETY: DXBC just compiled for this device's shader model.
            unsafe { gpu.dev.CreateVertexShader(&code, None, Some(&mut s)).map_err(dx("CreateVertexShader look"))? };
            Ok(s.expect("CreateVertexShader"))
        };
        let ps = |entry: &str| -> Result<ID3D11PixelShader> {
            let code = compile(&src, entry, "ps_5_0")?;
            let mut s = None;
            // SAFETY: as `vs`.
            unsafe { gpu.dev.CreatePixelShader(&code, None, Some(&mut s)).map_err(dx("CreatePixelShader look"))? };
            Ok(s.expect("CreatePixelShader"))
        };
        let blend_of = |b: Blend| {
            let mut key = BlendKey::OPAQUE;
            let (src, dst) = match b {
                Blend::Alpha => (D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_INV_SRC_ALPHA),
                Blend::Add => (D3D11_BLEND_ONE, D3D11_BLEND_ONE),
                Blend::Opaque => (D3D11_BLEND_ONE, D3D11_BLEND_ZERO),
            };
            key.targets[0] = BlendTarget {
                color_src: src.0 as u8,
                color_dst: dst.0 as u8,
                write_mask: (D3D11_COLOR_WRITE_ENABLE_RED.0
                    | D3D11_COLOR_WRITE_ENABLE_GREEN.0
                    | D3D11_COLOR_WRITE_ENABLE_BLUE.0) as u8,
                ..BlendTarget::OPAQUE
            };
            gpu.state(StateKey::Blend(key))
        };
        let shapes = (0..file.shapes.len())
            .map(|k| Ok(ShapeDraw { ps: ps(&format!("LookShape{k}"))?, blend: blend_of(file.shapes[k].blend) }))
            .collect::<Result<Vec<_>>>()?;
        let screens = file.screen.iter().map(|e| ps(e)).collect::<Result<Vec<_>>>()?;
        let textures = file.textures.iter().map(|p| assets.texture(p)).collect::<Result<Vec<_>>>()?;
        let sampler = |address| gpu.state(StateKey::Sampler(SamplerKey::of(D3D11_FILTER_MIN_MAG_MIP_LINEAR, address)));
        Ok(Look {
            time: 0.0,
            shape_vs: vs("ShapeVS")?,
            screen_vs: vs("ScreenVS")?,
            shapes,
            screens,
            textures,
            constants: gpu.buffer(&BufDesc { kind: BufKind::Constant, size: ROWS as u32 * 16, dynamic: true }, None)?,
            copy: None,
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
            opaque: blend_of(Blend::Opaque),
            wrap: sampler(D3D11_TEXTURE_ADDRESS_WRAP),
            clamp: sampler(D3D11_TEXTURE_ADDRESS_CLAMP),
            file,
        })
    }

    /// The look's shapes, then its screen passes, into SceneColor (after the translucency, before the post chain).
    pub(crate) fn draw(&mut self, fr: &Frame) -> Result<()> {
        if self.shapes.is_empty() && self.screens.is_empty() {
            return Ok(());
        }
        let gpu = fr.gpu;
        let t = fr.targets;
        let size = t.size;
        if self.copy.as_ref().is_none_or(|c| c.desc.size[..2] != size) {
            self.copy = Some(gpu.texture(
                &TexDesc {
                    kind: TexKind::D2,
                    size: [size[0], size[1], 1],
                    mips: 1,
                    format: t.scene_color.desc.format,
                    views: TexViews { srv: true, ..Default::default() },
                },
                &[],
            )?);
        }
        let copy = self.copy.as_ref().expect("made above");
        let cam = fr.input.camera;
        let mut rows = [[0f32; 4]; ROWS];
        let m = crate::lighting::translated_world_to_clip(cam);
        for (i, row) in m.iter().enumerate() {
            rows[i] = row.map(|v| v as f32);
        }
        rows[4] = [cam.origin[0] as f32, cam.origin[1] as f32, cam.origin[2] as f32, 0.0];
        let [w, h] = size.map(|v| v as f32);
        rows[5] = [w, h, 1.0 / w, 1.0 / h];
        let p = &cam.view_to_clip;
        rows[6] = [self.time, p[2][2] as f32, p[3][2] as f32, 0.0];
        for (k, r) in self.file.params.iter().enumerate() {
            rows[10 + k] = *r;
        }
        for (i, row) in crate::view::inverse(&m).iter().enumerate() {
            rows[10 + PARAMS + i] = row.map(|v| v as f32);
        }
        // t0 SceneColor's copy, t1 depth, t2..t9 the look's textures, t10 GBufferA (the world normal).
        let mut srvs: Vec<Option<ID3D11ShaderResourceView>> = vec![copy.srv.clone(), t.depth.srv.clone()];
        srvs.extend((0..TEXTURES).map(|k| self.textures.get(k).and_then(|x| x.srv.clone())));
        srvs.push(t.gbuffer[0].srv.clone());
        let samplers = [Some(gpu.sampler(self.wrap)), Some(gpu.sampler(self.clamp))];
        gpu.viewport([0, 0, size[0], size[1]]);
        gpu.set_geometry_shader(None);
        // SAFETY: SceneColor's copy (made with SceneColor's size and format) is taken before any draw reads it; the
        // targets, views, shaders and buffers are this device's, and the views are unbound after.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.CopyResource(&copy.res, &t.scene_color.res);
            ctx.OMSetRenderTargets(Some(std::slice::from_ref(&t.scene_color.rtv)), None);
            ctx.IASetInputLayout(None);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.PSSetShaderResources(0, Some(&srvs));
            ctx.PSSetSamplers(0, Some(&samplers));
        }
        for (k, s) in self.file.shapes.iter().enumerate() {
            let t = f64::from(self.time);
            let at = [0, 1, 2].map(|i| (s.center[i] + s.velocity[i] * t - cam.origin[i]) as f32);
            rows[7] = [at[0], at[1], at[2], 0.0];
            rows[8] = [s.u[0] as f32, s.u[1] as f32, s.u[2] as f32, 0.0];
            rows[9] = [s.v[0] as f32, s.v[1] as f32, s.v[2] as f32, 0.0];
            gpu.write_rows(&self.constants, &rows.map(|r| r.map(f32::to_bits)));
            gpu.set_states(self.shapes[k].blend, self.raster, self.no_depth, 0);
            // SAFETY: as above.
            unsafe {
                let ctx = &gpu.ctx;
                ctx.VSSetShader(&self.shape_vs, None);
                ctx.PSSetShader(&self.shapes[k].ps, None);
                ctx.VSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
                ctx.PSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
                ctx.Draw(4, 0);
            }
        }
        for ps in &self.screens {
            gpu.write_rows(&self.constants, &rows.map(|r| r.map(f32::to_bits)));
            gpu.set_states(self.opaque, self.raster, self.no_depth, 0);
            // SAFETY: as above; SceneColor is unbound while its copy is refreshed for the pass to read.
            unsafe {
                let ctx = &gpu.ctx;
                ctx.OMSetRenderTargets(None, None);
                ctx.CopyResource(&copy.res, &t.scene_color.res);
                ctx.OMSetRenderTargets(Some(std::slice::from_ref(&t.scene_color.rtv)), None);
                ctx.VSSetShader(&self.screen_vs, None);
                ctx.PSSetShader(ps, None);
                ctx.VSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
                ctx.PSSetConstantBuffers(0, Some(&[Some(self.constants.buf.clone())]));
                ctx.Draw(3, 0);
            }
        }
        let none: Vec<Option<ID3D11ShaderResourceView>> = vec![None; srvs.len()];
        // SAFETY: plain state calls.
        unsafe {
            gpu.ctx.PSSetShaderResources(0, Some(&none));
            gpu.ctx.OMSetRenderTargets(None, None);
        }
        Ok(())
    }
}

/// The Look buffer's rows (prelude.hlsl): WorldToClip 4, CameraOrigin, Screen, TimeDepth, Center, AxisU, AxisV,
/// Params 8, ClipToWorld 4.
const ROWS: usize = 22;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_file_reads_with_its_defaults() {
        let f: LookFile = serde_json::from_str(
            r#"{"shader": "a.hlsl", "shapes": [{"entry": "A", "center": [0, 0, 0], "u": [0, 1, 0], "v": [1, 0, 0]}],
                "lights": [{"at": [0, 0, 0], "color": [1, 1, 1], "radius": 100}]}"#,
        )
        .unwrap();
        assert_eq!(f.shapes[0].blend, Blend::Alpha);
        assert_eq!(f.shapes[0].velocity, [0.0; 3]);
        assert_eq!(f.camera.zoom, 1.0);
        assert!(!f.camera.moves());
        assert!(serde_json::from_str::<LookFile>(r#"{"shapez": []}"#).is_err());
    }

    #[test]
    fn a_roll_turns_right_and_up_and_keeps_forward() {
        let mut cam = Camera {
            origin: [0.0; 3],
            translated_world_to_view: [
                [0.0, 0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            view_to_clip: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 10.0, 0.0]],
        };
        LookCamera { roll: 90.0, zoom: 2.0, ..Default::default() }.apply(&mut cam, 0.0);
        // World Y (the view's right) now reads as the view's down; world Z (up) as its right.
        let m = &cam.translated_world_to_view;
        assert!((m[1][1] + 1.0).abs() < 1e-9 && m[1][0].abs() < 1e-9);
        assert!((m[2][0] - 1.0).abs() < 1e-9);
        assert_eq!(m[0][2], 1.0);
        assert_eq!(cam.view_to_clip[0][0], 2.0);
    }
}
