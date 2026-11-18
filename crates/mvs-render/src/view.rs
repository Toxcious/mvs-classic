//! The scene-wide rows the build's shaders read: FViewUniformShaderParameters (the View buffer, its layout the exe's
//! member table at 142d64000), the GPUScene primitive rows and the lightmap rows, and their GPU buffers.

#![allow(clippy::needless_range_loop)] // the index loops read as the matrices they fill

use std::sync::Arc;

use mvs_view::{Camera, FrameTime, M4};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use crate::Result;
use crate::gpu::{BufDesc, BufKind, Buffer, Gpu, Subresource, TexDesc, TexKind, TexViews, Texture, rows_bytes};

/// The View buffer's rows.
pub const VIEW_ROWS: usize = 232;
/// FPrimitiveSceneShaderData's rows a primitive (PrimitiveId x 36 + row).
pub const PRIMITIVE_ROWS: usize = 36;
/// FLightmapSceneShaderData's rows a lightmap (LightmapDataIndex x 15 + row).
pub const LIGHTMAP_ROWS: usize = 15;

/// The View struct's constant members by byte offset, ascending: the exe's member table (the
/// FViewUniformShaderParameters layout at 142d64000 and the members after it to 142d6e000). A member's size runs to
/// the next one's offset, within its row; an array or matrix member's element k is the row k past its offset.
const VIEW_MEMBERS: &[(&str, u16)] = &[
    ("TranslatedWorldToClip", 0),
    ("WorldToClip", 64),
    ("ClipToWorld", 128),
    ("TranslatedWorldToView", 192),
    ("ViewToTranslatedWorld", 256),
    ("TranslatedWorldToCameraView", 320),
    ("CameraViewToTranslatedWorld", 384),
    ("ViewToClip", 448),
    ("ViewToClipNoAA", 512),
    ("ClipToView", 576),
    ("ClipToTranslatedWorld", 640),
    ("SVPositionToTranslatedWorld", 704),
    ("ScreenToWorld", 768),
    ("ScreenToTranslatedWorld", 832),
    ("MobileMultiviewShadowTransform", 896),
    ("ViewForward", 960),
    ("ViewUp", 976),
    ("ViewRight", 992),
    ("HMDViewNoRollUp", 1008),
    ("HMDViewNoRollRight", 1024),
    ("InvDeviceZToWorldZTransform", 1040),
    ("ScreenPositionScaleBias", 1056),
    ("WorldCameraOrigin", 1072),
    ("TranslatedWorldCameraOrigin", 1088),
    ("WorldViewOrigin", 1104),
    ("PreViewTranslation", 1120),
    ("PrevProjection", 1136),
    ("PrevViewProj", 1200),
    ("PrevViewRotationProj", 1264),
    ("PrevViewToClip", 1328),
    ("PrevClipToView", 1392),
    ("PrevTranslatedWorldToClip", 1456),
    ("PrevTranslatedWorldToView", 1520),
    ("PrevViewToTranslatedWorld", 1584),
    ("PrevTranslatedWorldToCameraView", 1648),
    ("PrevCameraViewToTranslatedWorld", 1712),
    ("PrevWorldCameraOrigin", 1776),
    ("PrevWorldViewOrigin", 1792),
    ("PrevPreViewTranslation", 1808),
    ("PrevInvViewProj", 1824),
    ("PrevScreenToTranslatedWorld", 1888),
    ("ClipToPrevClip", 1952),
    ("TemporalAAJitter", 2016),
    ("GlobalClippingPlane", 2032),
    ("FieldOfViewWideAngles", 2048),
    ("PrevFieldOfViewWideAngles", 2056),
    ("ViewRectMin", 2064),
    ("ViewSizeAndInvSize", 2080),
    ("LightProbeSizeRatioAndInvSizeRatio", 2096),
    ("BufferSizeAndInvSize", 2112),
    ("BufferBilinearUVMinMax", 2128),
    ("ScreenToViewSpace", 2144),
    ("NumSceneColorMSAASamples", 2160),
    ("PreExposure", 2164),
    ("OneOverPreExposure", 2168),
    ("DiffuseOverrideParameter", 2176),
    ("SpecularOverrideParameter", 2192),
    ("NormalOverrideParameter", 2208),
    ("RoughnessOverrideParameter", 2224),
    ("PrevFrameGameTime", 2232),
    ("PrevFrameRealTime", 2236),
    ("OutOfBoundsMask", 2240),
    ("WorldCameraMovementSinceLastFrame", 2256),
    ("CullingSign", 2268),
    ("NearPlane", 2272),
    ("AdaptiveTessellationFactor", 2276),
    ("GameTime", 2280),
    ("RealTime", 2284),
    ("DeltaTime", 2288),
    ("MaterialTextureMipBias", 2292),
    ("MaterialTextureDerivativeMultiply", 2296),
    ("Random", 2300),
    ("FrameNumber", 2304),
    ("StateFrameIndexMod8", 2308),
    ("StateFrameIndex", 2312),
    ("DebugViewModeMask", 2316),
    ("CameraCut", 2320),
    ("UnlitViewmodeMask", 2324),
    ("DirectionalLightColor", 2336),
    ("DirectionalLightDirection", 2352),
    ("TranslucencyLightingVolumeMin", 2368),
    ("TranslucencyLightingVolumeInvSize", 2400),
    ("TemporalAAParams", 2432),
    ("CircleDOFParams", 2448),
    ("ForceDrawAllVelocities", 2464),
    ("DepthOfFieldSensorWidth", 2468),
    ("DepthOfFieldFocalDistance", 2472),
    ("DepthOfFieldScale", 2476),
    ("DepthOfFieldFocalLength", 2480),
    ("DepthOfFieldFocalRegion", 2484),
    ("DepthOfFieldNearTransitionRegion", 2488),
    ("DepthOfFieldFarTransitionRegion", 2492),
    ("MotionBlurNormalizedToPixel", 2496),
    ("bSubsurfacePostprocessEnabled", 2500),
    ("GeneralPurposeTweak", 2504),
    ("DemosaicVposOffset", 2508),
    ("IndirectLightingColorScale", 2512),
    ("AtmosphericFogSunPower", 2524),
    ("AtmosphericFogPower", 2528),
    ("AtmosphericFogDensityScale", 2532),
    ("AtmosphericFogDensityOffset", 2536),
    ("AtmosphericFogGroundOffset", 2540),
    ("AtmosphericFogDistanceScale", 2544),
    ("AtmosphericFogAltitudeScale", 2548),
    ("AtmosphericFogHeightScaleRayleigh", 2552),
    ("AtmosphericFogStartDistance", 2556),
    ("AtmosphericFogDistanceOffset", 2560),
    ("AtmosphericFogSunDiscScale", 2564),
    ("AtmosphereLightDirection", 2576),
    ("AtmosphereLightColor", 2608),
    ("AtmosphereLightColorGlobalPostTransmittance", 2640),
    ("AtmosphereLightDiscLuminance", 2672),
    ("AtmosphereLightDiscCosHalfApexAngle", 2704),
    ("SkyViewLutSizeAndInvSize", 2736),
    ("SkyWorldCameraOrigin", 2752),
    ("SkyPlanetCenterAndViewHeight", 2768),
    ("SkyViewLutReferential", 2784),
    ("SkyAtmosphereSkyLuminanceFactor", 2848),
    ("SkyAtmospherePresentInScene", 2864),
    ("SkyAtmosphereHeightFogContribution", 2868),
    ("SkyAtmosphereBottomRadiusKm", 2872),
    ("SkyAtmosphereTopRadiusKm", 2876),
    ("SkyAtmosphereCameraAerialPerspectiveVolumeSizeAndInvSize", 2880),
    ("SkyAtmosphereAerialPerspectiveStartDepthKm", 2896),
    ("SkyAtmosphereCameraAerialPerspectiveVolumeDepthResolution", 2900),
    ("SkyAtmosphereCameraAerialPerspectiveVolumeDepthResolutionInv", 2904),
    ("SkyAtmosphereCameraAerialPerspectiveVolumeDepthSliceLengthKm", 2908),
    ("SkyAtmosphereCameraAerialPerspectiveVolumeDepthSliceLengthKmInv", 2912),
    ("SkyAtmosphereApplyCameraAerialPerspectiveVolume", 2916),
    ("AtmosphericFogRenderMask", 2920),
    ("AtmosphericFogInscatterAltitudeSampleNum", 2924),
    ("NormalCurvatureToRoughnessScaleBias", 2928),
    ("RenderingReflectionCaptureMask", 2940),
    ("RealTimeReflectionCapture", 2944),
    ("RealTimeReflectionCapturePreExposure", 2948),
    ("AmbientCubemapTint", 2960),
    ("AmbientCubemapIntensity", 2976),
    ("SkyLightApplyPrecomputedBentNormalShadowingFlag", 2980),
    ("SkyLightAffectReflectionFlag", 2984),
    ("SkyLightAffectGlobalIlluminationFlag", 2988),
    ("SkyLightColor", 2992),
    ("MobileSkyIrradianceEnvironmentMap", 3008),
    ("MobilePreviewMode", 3120),
    ("HMDEyePaddingOffset", 3124),
    ("ReflectionCubemapMaxMip", 3128),
    ("ShowDecalsMask", 3132),
    ("DistanceFieldAOSpecularOcclusionMode", 3136),
    ("IndirectCapsuleSelfShadowingIntensity", 3140),
    ("ReflectionEnvironmentRoughnessMixingScaleBiasAndLargestWeight", 3152),
    ("StereoPassIndex", 3164),
    ("GlobalVolumeCenterAndExtent", 3168),
    ("GlobalVolumeWorldToUVAddAndMul", 3232),
    ("GlobalVolumeDimension", 3296),
    ("GlobalVolumeTexelSize", 3300),
    ("MaxGlobalDistance", 3304),
    ("CursorPosition", 3312),
    ("bCheckerboardSubsurfaceProfileRendering", 3320),
    ("VolumetricFogInvGridSize", 3328),
    ("VolumetricFogGridZParams", 3344),
    ("VolumetricFogSVPosToVolumeUV", 3360),
    ("VolumetricFogMaxDistance", 3368),
    ("VolumetricLightmapWorldToUVScale", 3376),
    ("VolumetricLightmapWorldToUVAdd", 3392),
    ("VolumetricLightmapIndirectionTextureSize", 3408),
    ("VolumetricLightmapBrickSize", 3420),
    ("VolumetricLightmapBrickTexelSize", 3424),
    ("StereoIPD", 3436),
    ("IndirectLightingCacheShowFlag", 3440),
    ("EyeToPixelSpreadAngle", 3444),
    ("WorldToVirtualTexture", 3456),
    ("XRPassthroughCameraUVs", 3520),
    ("VirtualTextureFeedbackStride", 3552),
    ("RuntimeVirtualTextureMipLevel", 3568),
    ("RuntimeVirtualTexturePackHeight", 3584),
    ("RuntimeVirtualTextureDebugParams", 3600),
    ("FarShadowStaticMeshLODBias", 3616),
    ("MinRoughness", 3620),
    ("HairRenderInfo", 3632),
    ("EnableSkyLight", 3648),
    ("HairRenderInfoBits", 3652),
    ("HairComponents", 3656),
];

/// The View struct's resource members, by their index in its resource list (the member at byte 3664 + 8 x index).
// Unverified: 0, 1, 2-11, 31, 33, 34, 43, 48, 50 and 51 are read from the shaders' resource tables and the exe's
// member table; the rest are 4.26's FViewUniformShaderParameters order.
pub const VIEW_RESOURCES: &[&str] = &[
    "MaterialTextureBilinearWrapedSampler",
    "MaterialTextureBilinearClampedSampler",
    "VolumetricLightmapIndirectionTexture",
    "VolumetricLightmapBrickAmbientVector",
    "VolumetricLightmapBrickSHCoefficients0",
    "VolumetricLightmapBrickSHCoefficients1",
    "VolumetricLightmapBrickSHCoefficients2",
    "VolumetricLightmapBrickSHCoefficients3",
    "VolumetricLightmapBrickSHCoefficients4",
    "VolumetricLightmapBrickSHCoefficients5",
    "SkyBentNormalBrickTexture",
    "DirectionalLightShadowingBrickTexture",
    "VolumetricLightmapBrickAmbientVectorSampler",
    "VolumetricLightmapTextureSampler0",
    "VolumetricLightmapTextureSampler1",
    "VolumetricLightmapTextureSampler2",
    "VolumetricLightmapTextureSampler3",
    "VolumetricLightmapTextureSampler4",
    "VolumetricLightmapTextureSampler5",
    "SkyBentNormalTextureSampler",
    "DirectionalLightShadowingTextureSampler",
    "GlobalDistanceFieldTexture0",
    "GlobalDistanceFieldSampler0",
    "GlobalDistanceFieldTexture1",
    "GlobalDistanceFieldSampler1",
    "GlobalDistanceFieldTexture2",
    "GlobalDistanceFieldSampler2",
    "GlobalDistanceFieldTexture3",
    "GlobalDistanceFieldSampler3",
    "AtmosphereTransmittanceTexture",
    "AtmosphereTransmittanceTextureSampler",
    "AtmosphereIrradianceTexture",
    "AtmosphereIrradianceTextureSampler",
    "AtmosphereInscatterTexture",
    "AtmosphereInscatterTextureSampler",
    "PerlinNoiseGradientTexture",
    "PerlinNoiseGradientTextureSampler",
    "PerlinNoise3DTexture",
    "PerlinNoise3DTextureSampler",
    "SobolSamplingTexture",
    "SharedPointWrappedSampler",
    "SharedPointClampedSampler",
    "SharedBilinearWrappedSampler",
    "SharedBilinearClampedSampler",
    "SharedTrilinearWrappedSampler",
    "SharedTrilinearClampedSampler",
    "PreIntegratedBRDF",
    "PreIntegratedBRDFSampler",
    "PrimitiveSceneData",
    "PrimitiveSceneDataTexture",
    "LightmapSceneData",
    "SkyIrradianceEnvironmentMap",
    "TransmittanceLutTexture",
    "TransmittanceLutTextureSampler",
    "SkyViewLutTexture",
    "SkyViewLutTextureSampler",
    "CameraAerialPerspectiveVolume",
    "CameraAerialPerspectiveVolumeSampler",
    "HairScatteringLUTTexture",
    "HairScatteringLUTSampler",
];

/// A View resource's index by its member name.
pub fn view_resource(name: &str) -> Option<usize> {
    VIEW_RESOURCES.iter().position(|n| *n == name)
}

/// A View member's byte offset and its size in components (1 to 4) by its name, `Name[k]` for an array's or a
/// matrix's row k. None: the struct holds no such member.
pub(crate) fn member(name: &str) -> Option<(usize, usize)> {
    let (base, k) = match name.split_once('[') {
        Some((b, rest)) => (b, rest.strip_suffix(']')?.parse::<usize>().ok()?),
        None => (name, 0),
    };
    member_row(base, k)
}

/// `member` of `Base[k]`.
fn member_row(base: &str, k: usize) -> Option<(usize, usize)> {
    let i = VIEW_MEMBERS.iter().position(|m| m.0 == base)?;
    let at = VIEW_MEMBERS[i].1 as usize + 16 * k;
    let next = VIEW_MEMBERS.get(i + 1).map_or(VIEW_ROWS * 16, |m| m.1 as usize);
    if at >= next {
        return None;
    }
    let size = (next - at).min(16 - at % 16) / 4;
    Some((at, size))
}

/// The View buffer's rows as they are written, by member name.
struct Rows<'a>(&'a mut [[u32; 4]; VIEW_ROWS]);

impl Rows<'_> {
    fn set(&mut self, name: &str, v: [u32; 4]) {
        let at = member(name).unwrap_or_else(|| panic!("View: no member {name}"));
        self.set_at(at, v);
    }

    /// The member at byte `at` of `size` components (`member`'s answer) set to `v`.
    fn set_at(&mut self, (at, size): (usize, usize), v: [u32; 4]) {
        let (row, c) = (at / 16, at % 16 / 4);
        self.0[row][c..c + size].copy_from_slice(&v[..size]);
    }

    fn f(&mut self, name: &str, v: [f32; 4]) {
        self.set(name, v.map(f32::to_bits));
    }

    fn f1(&mut self, name: &str, v: f32) {
        self.f(name, [v, 0.0, 0.0, 0.0]);
    }

    fn u1(&mut self, name: &str, v: u32) {
        self.set(name, [v, 0, 0, 0]);
    }

    fn v3(&mut self, name: &str, v: [f64; 3]) {
        self.f(name, [v[0] as f32, v[1] as f32, v[2] as f32, 0.0]);
    }

    fn mat(&mut self, name: &str, m: &M4) {
        for (i, r) in m.iter().enumerate() {
            let at = member_row(name, i).unwrap_or_else(|| panic!("View: no member {name}[{i}]"));
            self.set_at(at, r.map(|x| (x as f32).to_bits()));
        }
    }
}

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

/// The inverse by Gauss-Jordan with partial pivoting.
pub fn inverse(m: &M4) -> M4 {
    let mut a = *m;
    let mut r = identity();
    for c in 0..4 {
        let p = (c..4).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap();
        a.swap(c, p);
        r.swap(c, p);
        let d = a[c][c];
        for j in 0..4 {
            a[c][j] /= d;
            r[c][j] /= d;
        }
        for i in 0..4 {
            if i != c {
                let f = a[i][c];
                for j in 0..4 {
                    a[i][j] -= f * a[c][j];
                    r[i][j] -= f * r[c][j];
                }
            }
        }
    }
    r
}

pub fn identity() -> M4 {
    let mut r = [[0.0; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    r
}

/// View members set by the scene, the lighting and the post (sky light, fog, volumetric lightmap, exposure), by
/// the member's name in the View struct; an array member's element k is `Name[k]`. Every other member is the
/// camera's, the view rect's and the time's, or the build's default. `textures`: the View's textures by member
/// name (the volumetric lightmap's, the sky's), bound by the shaders' resource tables.
#[derive(Clone, Debug, Default)]
pub struct ViewMembers {
    pub rows: Vec<(&'static str, [u32; 4])>,
    pub textures: Vec<(&'static str, Arc<Texture>)>,
}

impl ViewMembers {
    pub fn set_f(&mut self, name: &'static str, v: [f32; 4]) {
        self.rows.push((name, v.map(f32::to_bits)));
    }

    pub fn set_u(&mut self, name: &'static str, v: [u32; 4]) {
        self.rows.push((name, v));
    }

    pub fn set_texture(&mut self, name: &'static str, t: Arc<Texture>) {
        self.textures.push((name, t));
    }

    /// The texture set for a View resource member, by name.
    pub fn texture(&self, name: &str) -> Option<&Arc<Texture>> {
        self.textures.iter().rev().find(|(n, _)| *n == name).map(|(_, t)| t)
    }
}

/// The View textures the engine makes itself (the system textures), set once for every view: PerlinNoiseGradient,
/// what a material's texture-based noise samples (Batcave_WaterPlane_MIC_Inst's).
pub fn system_members(gpu: &Gpu) -> Result<ViewMembers> {
    let texels = perlin_noise_gradient();
    let data: Vec<u8> = texels.iter().flat_map(|t| t.to_le_bytes()).collect();
    let t = gpu.texture(
        &TexDesc {
            kind: TexKind::D2,
            size: [PERLIN_SIZE, PERLIN_SIZE, 1],
            mips: 1,
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            views: TexViews { srv: true, ..Default::default() },
        },
        &[Subresource { data: &data, row_pitch: PERLIN_SIZE * 4, slice_pitch: PERLIN_SIZE * PERLIN_SIZE * 4 }],
    )?;
    let mut m = ViewMembers::default();
    m.set_texture("PerlinNoiseGradientTexture", Arc::new(t));
    Ok(m)
}

/// PerlinNoiseGradient's width and height.
const PERLIN_SIZE: u32 = 128;

/// PerlinNoiseGradient's texels (PF_B8G8R8A8, row by row) as FSystemTextures::InitializeCommonTextures fills them
/// (141d8a8bc): an FRandomStream seeded 12345 (seed x 196314165 + 907633515, the fraction the seed's top 23 bits
/// as a float in [1, 2) less 1, in float32), each texel gradtable[(u32)(fraction x 12.0f)] of the 12 cube edge
/// mid points at 144c3bf88 (0x00 is -1, 0x88 about 0.067, 0xff 1).
fn perlin_noise_gradient() -> Vec<u32> {
    const GRADIENTS: [u32; 12] = [
        0x0088_ffff,
        0x00ff_88ff,
        0x00ff_ff88,
        0x0088_ff00,
        0x00ff_8800,
        0x00ff_0088,
        0x0088_00ff,
        0x0000_88ff,
        0x0000_ff88,
        0x0088_0000,
        0x0000_8800,
        0x0000_0088,
    ];
    let mut seed: u32 = 12345;
    (0..PERLIN_SIZE * PERLIN_SIZE)
        .map(|_| {
            seed = seed.wrapping_mul(196_314_165).wrapping_add(907_633_515);
            let fraction = f32::from_bits(0x3f80_0000 | (seed >> 9)) - 1.0;
            // (cvttss2si: toward zero; the largest fraction times 12.0f rounds to 11.999998)
            GRADIENTS[(fraction * 12.0) as usize]
        })
        .collect()
}

/// What the View buffer is built from.
#[derive(Clone, Debug)]
pub struct ViewParams {
    pub camera: Camera,
    /// ViewRectMin and the view's size, pixels: x, y, width, height.
    pub rect: [u32; 4],
    /// The scene targets' size (BufferSizeAndInvSize).
    pub buffer: [u32; 2],
    pub time: FrameTime,
    /// FrameNumber.
    pub frame: u32,
    pub members: ViewMembers,
}

/// The view's axes from TranslatedWorldToView (its columns: right, up, forward).
fn axes(r: &M4) -> [[f64; 3]; 3] {
    let col = |c: usize| [r[0][c], r[1][c], r[2][c]];
    [col(0), col(1), col(2)]
}

/// The View buffer's rows of `v` (FViewInfo::SetupUniformBufferParameters' terms for one view with no history:
/// the previous frame's matrices are this frame's). Panics on a member name the struct does not hold.
// Unverified: the defaults past the camera, the rect and the time (the override parameters, the reflection mixing,
// MinRoughness, the light probe ratio) are 4.26's console variable defaults, as the native trial set them.
pub fn view_rows(v: &ViewParams, out: &mut [[u32; 4]; VIEW_ROWS]) {
    *out = [[0; 4]; VIEW_ROWS];
    let mut b = Rows(out);
    let cam = &v.camera;
    let [rx, ry, w, h] = v.rect.map(|x| x as f64);
    let [bw, bh] = v.buffer.map(|x| x as f64);
    let r = cam.translated_world_to_view;
    let rt = inverse(&r);
    let proj = cam.view_to_clip;
    let tvp = mul(&r, &proj);
    let mut pre = identity();
    for i in 0..3 {
        pre[3][i] = -cam.origin[i];
    }
    let vp = mul(&pre, &tvp);
    let clip_to_tw = inverse(&tvp);
    // ScreenToClip (FViewMatrices): (x w, y w, w, 1) -> clip, from the projection's M[2][2] and M[3][2].
    let mut screen_to_clip = identity();
    screen_to_clip[2] = [0.0, 0.0, proj[2][2], 1.0];
    screen_to_clip[3] = [0.0, 0.0, proj[3][2], 0.0];
    // SVPosition (pixels in the buffer) -> clip over the view rect.
    let mut sv_to_clip = identity();
    sv_to_clip[0][0] = 2.0 / w;
    sv_to_clip[1][1] = -2.0 / h;
    sv_to_clip[3] = [-1.0 - 2.0 * rx / w, 1.0 + 2.0 * ry / h, 0.0, 1.0];
    let [right, up, forward] = axes(&r);
    // (each member and its Prev twin)
    for k in 0..2 {
        let n = |names: [&'static str; 2]| names[k];
        b.mat(n(["TranslatedWorldToView", "PrevTranslatedWorldToView"]), &r);
        b.mat(n(["ViewToTranslatedWorld", "PrevViewToTranslatedWorld"]), &rt);
        b.mat(n(["TranslatedWorldToCameraView", "PrevTranslatedWorldToCameraView"]), &r);
        b.mat(n(["CameraViewToTranslatedWorld", "PrevCameraViewToTranslatedWorld"]), &rt);
        b.mat(n(["ViewToClip", "PrevViewToClip"]), &proj);
        b.mat(n(["ClipToView", "PrevClipToView"]), &inverse(&proj));
        b.mat(n(["TranslatedWorldToClip", "PrevTranslatedWorldToClip"]), &tvp);
        b.mat(n(["ScreenToTranslatedWorld", "PrevScreenToTranslatedWorld"]), &mul(&screen_to_clip, &clip_to_tw));
        b.v3(n(["PreViewTranslation", "PrevPreViewTranslation"]), [-cam.origin[0], -cam.origin[1], -cam.origin[2]]);
        b.v3(n(["WorldCameraOrigin", "PrevWorldCameraOrigin"]), cam.origin);
        b.v3(n(["WorldViewOrigin", "PrevWorldViewOrigin"]), cam.origin);
    }
    b.mat("WorldToClip", &vp);
    b.mat("ClipToWorld", &inverse(&vp));
    b.mat("ViewToClipNoAA", &proj);
    b.mat("ClipToTranslatedWorld", &clip_to_tw);
    b.mat("SVPositionToTranslatedWorld", &mul(&sv_to_clip, &clip_to_tw));
    b.mat("ScreenToWorld", &mul(&screen_to_clip, &inverse(&vp)));
    b.mat("PrevProjection", &proj);
    b.mat("PrevViewProj", &vp);
    b.mat("PrevViewRotationProj", &tvp);
    b.mat("PrevInvViewProj", &inverse(&vp));
    b.mat("ClipToPrevClip", &identity());
    b.v3("ViewForward", forward);
    b.v3("ViewUp", up);
    b.v3("ViewRight", right);
    b.v3("TranslatedWorldCameraOrigin", [0.0; 3]);
    // InvDeviceZToWorldZTransform (CreateInvDeviceZToWorldZTransform): depth = 1 / (z [2] - [3]).
    let (mul22, add32) = (proj[2][2], proj[3][2]);
    b.f("InvDeviceZToWorldZTransform", [0.0, 0.0, (1.0 / add32) as f32, (mul22 / add32) as f32]);
    b.f(
        "ScreenPositionScaleBias",
        [(w / bw / 2.0) as f32, (-h / bh / 2.0) as f32, ((h / 2.0 + ry) / bh) as f32, ((w / 2.0 + rx) / bw) as f32],
    );
    b.f(
        "FieldOfViewWideAngles",
        [(2.0 * (1.0 / proj[0][0]).atan()) as f32, (2.0 * (1.0 / proj[1][1]).atan()) as f32, 0.0, 0.0],
    );
    b.f(
        "PrevFieldOfViewWideAngles",
        [(2.0 * (1.0 / proj[0][0]).atan()) as f32, (2.0 * (1.0 / proj[1][1]).atan()) as f32, 0.0, 0.0],
    );
    b.f("ViewRectMin", [rx as f32, ry as f32, 0.0, 0.0]);
    b.f("ViewSizeAndInvSize", [w as f32, h as f32, (1.0 / w) as f32, (1.0 / h) as f32]);
    b.f("LightProbeSizeRatioAndInvSizeRatio", [1.0; 4]);
    b.f("BufferSizeAndInvSize", [bw as f32, bh as f32, (1.0 / bw) as f32, (1.0 / bh) as f32]);
    b.f(
        "BufferBilinearUVMinMax",
        [
            ((rx + 0.5) / bw) as f32,
            ((ry + 0.5) / bh) as f32,
            ((rx + w - 0.5) / bw) as f32,
            ((ry + h - 0.5) / bh) as f32,
        ],
    );
    b.u1("NumSceneColorMSAASamples", 1);
    b.f1("PreExposure", 1.0);
    b.f1("OneOverPreExposure", 1.0);
    b.f("DiffuseOverrideParameter", [0.0, 0.0, 0.0, 1.0]);
    b.f("SpecularOverrideParameter", [0.0, 0.0, 0.0, 1.0]);
    b.f("NormalOverrideParameter", [0.0, 0.0, 0.0, 1.0]);
    b.f("RoughnessOverrideParameter", [0.0, 1.0, 0.0, 0.0]);
    b.f1("PrevFrameGameTime", v.time.game - v.time.delta);
    b.f1("PrevFrameRealTime", v.time.real - v.time.delta);
    b.f1("CullingSign", 1.0);
    // UE's reversed-Z perspective: M[3][2] is the near plane.
    b.f1("NearPlane", proj[3][2] as f32);
    b.f1("GameTime", v.time.game);
    b.f1("RealTime", v.time.real);
    b.f1("DeltaTime", v.time.delta);
    b.f1("MaterialTextureDerivativeMultiply", 1.0);
    b.u1("FrameNumber", v.frame);
    b.u1("StateFrameIndexMod8", v.frame % 8);
    b.u1("StateFrameIndex", v.frame);
    b.f("IndirectLightingColorScale", [1.0, 1.0, 1.0, 0.0]);
    b.f1("ShowDecalsMask", 1.0);
    b.f1("IndirectLightingCacheShowFlag", 1.0);
    // ReflectionEnvironmentRoughnessMixingScaleBiasAndLargestWeight: r.ReflectionEnvironmentBeginMixingRoughness
    // 0.1, EndMixingRoughness 0.3 (scale 1 / (end - begin), bias -begin x scale), LightmapMixLargestWeight 10000.
    b.f("ReflectionEnvironmentRoughnessMixingScaleBiasAndLargestWeight", [5.0, -0.5, 10000.0, 0.0]);
    b.f1("MinRoughness", 0.02);
    for (name, row) in &v.members.rows {
        b.set(name, *row);
    }
}

/// The scale of each of a matrix's three axes (rows), FMatrix::GetScaleVector.
fn scale_of(m: &M4) -> [f64; 3] {
    [0, 1, 2].map(|i| (m[i][0] * m[i][0] + m[i][1] * m[i][1] + m[i][2] * m[i][2]).sqrt())
}

/// The 3x3 part's determinant (FMatrix::RotDeterminant).
fn rot_determinant(m: &M4) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// What a primitive's GPUScene rows are built from; the rest (WorldToLocal, the orientation, the non-uniform
/// scale, the determinant's sign) is derived.
// The rows are the exe's FPrimitiveUniformShaderParameters member table (142d6b600): PrimitiveRows::push.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveParams {
    pub local_to_world: M4,
    pub previous_local_to_world: M4,
    pub actor_world_position: [f64; 3],
    /// ObjectBounds' world origin and box extent, and its sphere radius, cm.
    pub bounds_origin: [f64; 3],
    pub bounds_extent: [f64; 3],
    pub bounds_radius: f64,
    pub local_bounds_min: [f32; 3],
    pub local_bounds_max: [f32; 3],
    /// LightingChannelMask: channel k at bit k.
    pub lighting_channels: u8,
    /// DecalReceiverMask: the primitive receives decals.
    pub receives_decals: bool,
    pub use_single_sample_shadow: bool,
    pub use_volumetric_lightmap_shadow: bool,
    /// LightmapDataIndex: its first lightmap row block.
    pub lightmap_index: u32,
    pub lightmap_uv_index: u32,
    pub custom_data: [[f32; 4]; 9],
}

/// The GPUScene primitive rows of a frame, PrimitiveId by PrimitiveId.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveRows(pub Vec<[[u32; 4]; PRIMITIVE_ROWS]>);

fn f4(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

fn f3w(v: [f64; 3], w: f32) -> [u32; 4] {
    f4([v[0] as f32, v[1] as f32, v[2] as f32, w])
}

impl PrimitiveRows {
    /// Adds `p`'s rows; returns its PrimitiveId.
    // The rows are the exe's FPrimitiveUniformShaderParameters member table (142d6b600..142d6e000): LocalToWorld
    // 0-3, InvNonUniformScaleAndDeterminantSign 4, ObjectWorldPositionAndRadius 5, WorldToLocal 6-9,
    // PreviousLocalToWorld 10-13, PreviousWorldToLocal 14-17, ActorWorldPosition 18 (.w
    // UseSingleSampleShadowFromStationaryLights), ObjectBounds 19 (.w LpvBiasMultiplier), DecalReceiverMask,
    // PerObjectGBufferData, UseVolumetricLightmapShadowFromStationaryLights and DrawsVelocity 20, ObjectOrientation
    // 21, NonUniformScale 22, LocalObjectBoundsMin 23 (.w LightingChannelMask), LocalObjectBoundsMax 24 (.w
    // LightmapDataIndex), PreSkinnedLocalBoundsMin 25 (.w SingleCaptureIndex), PreSkinnedLocalBoundsMax 26 (.w
    // OutputVelocity), CustomPrimitiveData 27-35. The struct holds no LightmapUVIndex: `lightmap_uv_index` is not
    // written (the LocalVF buffer carries LightMapCoordinateIndex).
    // Unverified: the derived terms (the scale's w, the orientation, PerObjectGBufferData 0, DrawsVelocity 1) are
    // 4.26's GetPrimitiveUniformShaderParameters.
    pub fn push(&mut self, p: &PrimitiveParams) -> u32 {
        let id = u32::try_from(self.0.len()).expect("primitives past u32");
        let mut r = [[0u32; 4]; PRIMITIVE_ROWS];
        let m = &p.local_to_world;
        let mat = |r: &mut [[u32; 4]; PRIMITIVE_ROWS], at: usize, m: &M4| {
            for i in 0..4 {
                r[at + i] = f4(m[i].map(|x| x as f32));
            }
        };
        let s = scale_of(m);
        let sign = if rot_determinant(m) < 0.0 { -1.0 } else { 1.0 };
        // GetPrimitiveUniformShaderParameters: 1 / scale where the scale is over KINDA_SMALL_NUMBER, else 0.
        let inv = |x: f64| if x > 1e-4 { (1.0 / x) as f32 } else { 0.0 };
        mat(&mut r, 0, m);
        r[4] = f4([inv(s[0]), inv(s[1]), inv(s[2]), sign]);
        r[5] = f3w(p.bounds_origin, p.bounds_radius as f32);
        mat(&mut r, 6, &inverse(m));
        mat(&mut r, 10, &p.previous_local_to_world);
        mat(&mut r, 14, &inverse(&p.previous_local_to_world));
        r[18] = f3w(p.actor_world_position, if p.use_single_sample_shadow { 1.0 } else { 0.0 });
        r[19] = f3w(p.bounds_extent, 1.0);
        r[20] = f4([
            if p.receives_decals { 1.0 } else { 0.0 },
            0.0,
            if p.use_volumetric_lightmap_shadow { 1.0 } else { 0.0 },
            1.0,
        ]);
        let z = [m[2][0], m[2][1], m[2][2]];
        let zl = (z[0] * z[0] + z[1] * z[1] + z[2] * z[2]).sqrt();
        let z = if zl > 0.0 { z.map(|c| c / zl) } else { [0.0, 0.0, 1.0] };
        r[21] = f3w(z, 0.0);
        r[22] = f3w(s, s[0].max(s[1]).max(s[2]) as f32);
        let lb = |v: [f32; 3], w: u32| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), w];
        r[23] = lb(p.local_bounds_min, p.lighting_channels as u32);
        r[24] = lb(p.local_bounds_max, p.lightmap_index);
        r[25] = lb(p.local_bounds_min, 0);
        r[26] = lb(p.local_bounds_max, 0);
        for (k, c) in p.custom_data.iter().enumerate() {
            r[27 + k] = f4(*c);
        }
        self.0.push(r);
        id
    }
}

/// A lightmap's rows (FLightmapSceneShaderData; the virtual texture rows stay 0).
// Unverified: the member list is 4.26's; the exe's layout is not read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LightmapParams {
    pub static_shadow_map_masks: [f32; 4],
    pub inv_uniform_penumbra_sizes: [f32; 4],
    pub lightmap_coordinate_scale_bias: [f32; 4],
    pub shadow_map_coordinate_scale_bias: [f32; 4],
    pub lightmap_scale: [[f32; 4]; 2],
    pub lightmap_add: [[f32; 4]; 2],
}

/// The lightmap rows of a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightmapRows(pub Vec<[[u32; 4]; LIGHTMAP_ROWS]>);

impl LightmapRows {
    /// Adds `p`'s rows; returns its LightmapDataIndex.
    pub fn push(&mut self, p: &LightmapParams) -> u32 {
        let id = u32::try_from(self.0.len()).expect("lightmaps past u32");
        let mut r = [[0u32; 4]; LIGHTMAP_ROWS];
        r[0] = f4(p.static_shadow_map_masks);
        r[1] = f4(p.inv_uniform_penumbra_sizes);
        r[2] = f4(p.lightmap_coordinate_scale_bias);
        r[3] = f4(p.shadow_map_coordinate_scale_bias);
        r[4] = f4(p.lightmap_scale[0]);
        r[5] = f4(p.lightmap_scale[1]);
        r[6] = f4(p.lightmap_add[0]);
        r[7] = f4(p.lightmap_add[1]);
        self.0.push(r);
        id
    }
}

/// PrimitiveIds the shared instance stream holds (a draw binds it at its id: one instance reads its own).
pub const MAX_PRIMITIVES: u32 = 1 << 16;

/// The scene-wide buffers on the GPU: the View uniform buffer and the primitive and lightmap structured buffers.
#[derive(Debug)]
pub struct SceneBuffers {
    pub view: Buffer,
    pub primitives: Buffer,
    pub lightmaps: Buffer,
    /// The instance stream of PrimitiveIds 0.. (FLocalVertexFactory's ATTRIBUTE13): a draw binds it at offset
    /// 4 x PrimitiveId.
    pub primitive_ids: Buffer,
    /// 64 zero bytes: the vertex stream a factory's unused attribute reads (stride 0).
    pub zeros: Buffer,
    /// 64 bytes of 0xff: GNullColorVertexBuffer, the colour stream of a mesh without one (stride 0, white).
    pub white: Buffer,
    /// The Primitive uniform buffer a draw whose shaders read it gets (its GPUScene rows), rewritten per draw.
    pub primitive_buffer: Buffer,
    /// The frame's primitive rows, for the Primitive buffer.
    pub primitive_rows: PrimitiveRows,
    /// GPUSkin's loose ($Globals) buffer: NumBoneInfluences (row 1 y) 4 and 8.
    pub skin_globals: [Buffer; 2],
    /// The View's textures (by member name) the frame binds.
    pub view_textures: Vec<(&'static str, Arc<Texture>)>,
}

fn structured(gpu: &Gpu, rows: usize) -> Result<Buffer> {
    gpu.buffer(&BufDesc { kind: BufKind::Structured { stride: 16 }, size: (rows * 16) as u32, dynamic: true }, None)
}

impl SceneBuffers {
    pub fn new(gpu: &Gpu) -> Result<SceneBuffers> {
        let ids: Vec<u32> = (0..MAX_PRIMITIVES).collect();
        let ids: Vec<u8> = ids.iter().flat_map(|i| i.to_le_bytes()).collect();
        Ok(SceneBuffers {
            view: gpu
                .buffer(&BufDesc { kind: BufKind::Constant, size: (VIEW_ROWS * 16) as u32, dynamic: true }, None)?,
            primitives: structured(gpu, 64 * PRIMITIVE_ROWS)?,
            lightmaps: structured(gpu, 16 * LIGHTMAP_ROWS)?,
            primitive_ids: gpu
                .buffer(&BufDesc { kind: BufKind::Vertex, size: ids.len() as u32, dynamic: false }, Some(&ids))?,
            zeros: gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: 64, dynamic: false }, Some(&[0; 64]))?,
            white: gpu.buffer(&BufDesc { kind: BufKind::Vertex, size: 64, dynamic: false }, Some(&[0xff; 64]))?,
            primitive_buffer: gpu.buffer(
                &BufDesc { kind: BufKind::Constant, size: (PRIMITIVE_ROWS * 16) as u32, dynamic: true },
                None,
            )?,
            primitive_rows: PrimitiveRows::default(),
            // Unverified: NumBoneInfluencesParam's place ($Globals row 1 y, read as uint against 4) is taken from the
            // GPU skin vertex shaders' code, not from their parameter map.
            skin_globals: [4u32, 8]
                .map(|n| {
                    let rows = [[0u32; 4], [0, n, 0, 0]];
                    gpu.buffer(&BufDesc { kind: BufKind::Constant, size: 32, dynamic: false }, Some(rows_bytes(&rows)))
                })
                .into_iter()
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .expect("two buffers"),
            view_textures: Vec::new(),
        })
    }

    /// The frame's rows to the GPU (the structured buffers grow as needed).
    pub fn upload(
        &mut self,
        gpu: &Gpu,
        view: &[[u32; 4]; VIEW_ROWS],
        primitives: &PrimitiveRows,
        lightmaps: &LightmapRows,
    ) -> Result<()> {
        gpu.write_rows(&self.view, view);
        assert!(primitives.0.len() <= MAX_PRIMITIVES as usize, "{} primitives", primitives.0.len());
        let p: &[[u32; 4]] = primitives.0.as_flattened();
        if p.len() * 16 > self.primitives.desc.size as usize {
            self.primitives = structured(gpu, p.len().next_power_of_two())?;
        }
        if !p.is_empty() {
            gpu.write(&self.primitives, rows_bytes(p));
        }
        self.primitive_rows.0.clone_from(&primitives.0);
        let l: &[[u32; 4]] = lightmaps.0.as_flattened();
        if l.len() * 16 > self.lightmaps.desc.size as usize {
            self.lightmaps = structured(gpu, l.len().next_power_of_two())?;
        }
        if !l.is_empty() {
            gpu.write(&self.lightmaps, rows_bytes(l));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_texels() {
        let t = perlin_noise_gradient();
        assert_eq!(t.len(), 128 * 128);
        assert_eq!(t[..4], [0x00ff_0088, 0x0000_88ff, 0x0088_00ff, 0x0000_0088]);
        assert_eq!(t[t.len() - 1], 0x0000_88ff);
    }

    #[test]
    fn members_by_name() {
        assert_eq!(member("TranslatedWorldToClip"), Some((0, 4)));
        assert_eq!(member("TranslatedWorldToClip[3]"), Some((48, 4)));
        assert_eq!(member("TranslatedWorldToClip[4]"), None);
        assert_eq!(member("PreExposure"), Some((135 * 16 + 4, 1)));
        // Padding to the next member counts as the member's.
        assert_eq!(member("MinRoughness"), Some((226 * 16 + 4, 3)));
        assert_eq!(member("AtmosphereLightDirection[1]"), Some((162 * 16, 4)));
        assert_eq!(member("MobileSkyIrradianceEnvironmentMap[6]"), Some((194 * 16, 4)));
        assert_eq!(member("NoSuchMember"), None);
    }

    #[test]
    fn primitive_rows() {
        let mut m = identity();
        m[0][0] = 2.0;
        m[3] = [10.0, 20.0, 30.0, 1.0];
        let p = PrimitiveParams {
            local_to_world: m,
            previous_local_to_world: m,
            actor_world_position: [10.0, 20.0, 30.0],
            bounds_origin: [0.0; 3],
            bounds_extent: [1.0; 3],
            bounds_radius: 1.0,
            local_bounds_min: [-1.0; 3],
            local_bounds_max: [1.0; 3],
            lighting_channels: 1,
            receives_decals: true,
            use_single_sample_shadow: false,
            use_volumetric_lightmap_shadow: false,
            lightmap_index: 7,
            lightmap_uv_index: 1,
            custom_data: [[0.0; 4]; 9],
        };
        let mut rows = PrimitiveRows::default();
        assert_eq!(rows.push(&p), 0);
        let r = &rows.0[0];
        assert_eq!(r[3], f4([10.0, 20.0, 30.0, 1.0]));
        assert_eq!(r[4], f4([0.5, 1.0, 1.0, 1.0]));
        assert_eq!(r[9], f4([-5.0, -20.0, -30.0, 1.0]));
        assert_eq!(r[24][3], 7);
        assert_eq!(r[23][3], 1);
    }
}
