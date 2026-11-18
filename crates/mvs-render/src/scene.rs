//! The stage's scenery (stage, scenery, landscape, foliage, volumetric lightmap, decals): its meshes placed with their
//! cooked materials, lightmaps and shadow maps, the volumetric lightmap, the sky, and the lights the lighting pass
//! reads.

use std::sync::Arc;

use crate::gpu::Texture;
use crate::view::ViewMembers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

/// A light as the deferred light buffer takes it (FLightShaderParameters' terms).
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub name: String,
    pub kind: LightKind,
    /// World, cm (point and spot).
    pub position: [f64; 3],
    /// The direction the light travels (directional), or the spot's axis.
    pub direction: [f32; 3],
    pub tangent: [f32; 3],
    /// Linear colour times intensity.
    pub color: [f32; 3],
    pub inv_radius: f32,
    pub falloff_exponent: f32,
    /// SpotAngles: cos outer, 1 / (cos inner - cos outer).
    pub spot_angles: [f32; 2],
    pub source_radius: f32,
    pub soft_source_radius: f32,
    pub source_length: f32,
    pub specular_scale: f32,
    /// Lighting channels: channel k at bit k.
    pub channels: u8,
    /// The precomputed shadow factor's channel of GBufferE (a stationary light's ShadowMapChannel), -1 for none.
    pub shadow_channel: i8,
    /// A movable light with CastShadows: its dynamic shadow is drawn (lighting/shadow.rs; the menu rigs' spot lights).
    pub cast_shadows: bool,
    /// It lights translucency (the translucency lighting volume and the forward light data).
    pub translucent: bool,
}

/// The stage's sky light.
#[derive(Clone, Debug)]
pub struct SkyLight {
    /// SkyLightColor: linear colour times intensity.
    pub color: [f32; 3],
    /// The filtered capture cube (all mips) and its brightness.
    pub cube: Arc<Texture>,
    pub brightness: f32,
    /// The irradiance SH (SkyIrradianceEnvironmentMap's seven rows).
    pub irradiance: [[f32; 4]; 7],
    pub pre_integrated_gf: Arc<Texture>,
    /// The world's sphere reflection captures, in the composite's order (lighting.rs).
    pub captures: Vec<ReflectionCapture>,
}

/// A SphereReflectionCaptureComponent with its build data: what the reflection pass's ReflectionCapture rows hold.
#[derive(Clone, Debug)]
pub struct ReflectionCapture {
    /// World, cm.
    pub position: [f64; 3],
    pub influence_radius: f32,
    pub brightness: f32,
    pub capture_offset: [f32; 3],
    pub average_brightness: f32,
    /// FullHDRCapturedData: a FloatRGBA cube, every mip.
    pub cube: Arc<Texture>,
}

/// What the stage gives the lighting pass.
#[derive(Clone, Debug, Default)]
pub struct LightSet {
    pub directional: Vec<Light>,
    pub radial: Vec<Light>,
    pub sky: Option<SkyLight>,
    /// The View members and textures the stage sets: the sky light's, the volumetric lightmap's, the fog's.
    pub view: ViewMembers,
}
