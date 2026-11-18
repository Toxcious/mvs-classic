// The deferred lighting pass: one full-screen triangle per lighting channel mask, the pixels of that mask picked by
// the stencil test, lit by the lights that share a channel with it and by the sky light's specular, added to
// SceneColor. The maths is the build's: the DefaultLit branch of the cooked FDeferredLightPS (directional and radial permutations, the capsule branch for a tube light) and
// FReflectionEnvironmentSkyLightingPS (the sphere capture permutation's composite where the stage has captures, the
// no-capture one's maths otherwise, which is the same with none). A spot light's dynamic shadow is read here from its
// map (lighting/shadow.rs), where the build projects it into LightAttenuation first.

#define MAX_CAPTURES 4

cbuffer Constants : register(b0)
{
    // Clip space (x, y, device z, 1) to translated world (world - WorldCameraOrigin), cm; row vectors.
    row_major float4x4 ClipToTranslatedWorld;
    // 2 / width, 2 / height: SV_Position to clip x, y.
    float4 InvSize;
    // The pass's lights: first directional, directional count, first radial, radial count.
    uint4 Ranges;
    // SkyLightParameters.x (the cube's mip count - 1), SkyLightCubemapBrightness, the sky light is on.
    float4 SkyParams;
    // The sky specular's SkyLightColor.
    float4 SkyColor;
    // Tiles across, the pass's first tile word, words per tile, the tile's size in pixels.
    uint4 Tiles;
    // The reflection captures: their count, ReflectionCubemapMaxMip.
    float4 CaptureInfo;
    // Per capture (ReflectionCapture's rows): PositionAndRadius (translated world, InfluenceRadius),
    // CaptureProperties (Brightness, ...), CaptureOffsetAndAverageBrightness.
    float4 Captures[MAX_CAPTURES * 3];
};

struct Light
{
    // Translated world position (radial), 1 / AttenuationRadius.
    float4 PositionInvRadius;
    // The unit vector towards the light (a spot light: against its axis), FalloffExponent (0: inverse squared).
    float4 DirectionExponent;
    // Linear colour x intensity, SpecularScale.
    float4 ColorSpecular;
    // SourceRadius, SoftSourceRadius, cos(outer cone), 1 / (cos(inner cone) - cos(outer cone)).
    float4 Source;
    // Tangent (a tube light's axis), SourceLength.
    float4 Tube;
    // x: the precomputed shadow factor's GBufferE channel + 1 (0: none); y: ShadowedBits; z: the dynamic shadow's
    // row in SpotShadows + 1 (0: none).
    float4 Shadow;
};

// A spot light's dynamic shadow (lighting/shadow.rs Plan::gpu_row).
struct SpotShadowRow
{
    // Translated world to (uv w, depth, w): uv in the map array's slice, depth as the map holds it (not divided).
    float4 ToShadow[4];
    // The slice, SoftTransitionScale.z, ShadowFadeFraction, ShadowSharpen.
    float4 Params;
    // 1 - ReceiverBias, the slice's size and its inverse.
    float4 Texels;
    // The map's rectangle inside its border, uv: what the build's shadow frustum volume bounds.
    float4 Bounds;
};

StructuredBuffer<Light> Lights : register(t0);
Texture2D<float> SceneDepth : register(t1);
Texture2D<float4> GBufferA : register(t2);
Texture2D<float4> GBufferB : register(t3);
Texture2D<float4> GBufferC : register(t4);
Texture2D<float4> GBufferE : register(t5);
TextureCube<float4> SkyCube : register(t6);
Texture2D<float2> PreIntegratedGF : register(t7);
// Per screen tile, the pass's radial lights whose sphere reaches it: bit j of word w is radial light 32 w + j.
StructuredBuffer<uint> TileLights : register(t8);
StructuredBuffer<SpotShadowRow> SpotShadows : register(t9);
Texture2DArray<float> ShadowMaps : register(t10);
// Deviation: one cube per capture where the build has one TextureCubeArray (ReflectionCubemap) indexed by
// CaptureProperties.y: the same samples.
TextureCube<float4> CaptureCube0 : register(t11);
TextureCube<float4> CaptureCube1 : register(t12);
TextureCube<float4> CaptureCube2 : register(t13);
TextureCube<float4> CaptureCube3 : register(t14);
SamplerState SkySampler : register(s0);
SamplerState ShadowSampler : register(s1);

float4 MainVS(uint id : SV_VertexID) : SV_Position
{
    float2 p = float2((id << 1) & 2, id & 2);
    return float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
}

// 4.26 sqrtFast: the bits halved plus the magic.
float SqrtFast(float x)
{
    return asfloat((asint(x) >> 1) + 0x1fbd1df5);
}

// The DefaultLit BxDF of FDeferredLightPS for one light: L the unit vector towards it (a tube light: towards the
// point of its segment closest to the reflection ray), Falloff its 1 / (d2 + 1) (0.5 for a directional light),
// InvDist the inverse distance to L's point (1 for a directional light), Src (SourceRadius, SoftSourceRadius,
// SpecularScale); NoLIn the irradiance's NoL (a tube light: the mean of its ends'), LineCos the cosine its segment
// subtends (1: not a tube).
float3 DeferredLight(float3 N, float3 V, float3 L, float Falloff, float InvDist, float3 Src, float3 Diffuse,
    float3 Spec, float a, float a2, float NoLIn, float LineCos)
{
    float NoVr = dot(N, V);
    float NoV = min(abs(NoVr) + 1e-5, 1.0);
    float SrcR = Src.x;
    float NoLr = dot(N, L), VoL = dot(V, L);
    float SinA = sqrt(clamp(Falloff * SrcR * SrcR, 0.0, 1.0));
    float NoL = NoLIn;
    if (SrcR > 0.0 && NoL < SinA)
    {
        float t = SinA + max(NoL, -SinA);
        NoL = t * t / (4.0 * SinA);
    }
    NoL = clamp(NoL, 0.0, 1.0);
    float SphS = clamp(SrcR * InvDist * (1.0 - a), 0.0, 1.0);
    float SphSoft = clamp(Src.y * InvDist, 0.0, 1.0);
    float NoH, VoH;
    float CosA = sqrt(1.0 - SphS * SphS);
    float RoL = 2.0 * NoVr * NoLr - VoL;
    if (SphS > 0.0 && RoL >= CosA)
    {
        NoH = 1.0;
        VoH = abs(NoVr);
    }
    else
    {
        float NoL2 = NoLr, VoL2 = VoL;
        if (SphS > 0.0)
        {
            // SphereMaxNoH with the Newton step.
            float rInvLengthT = SphS * rsqrt(1.0 - RoL * RoL);
            float NoTr = rInvLengthT * (NoVr - RoL * NoLr);
            float VoTr = rInvLengthT * (2.0 * NoVr * NoVr - 1.0 - RoL * VoL);
            float NxLoV = sqrt(clamp(1.0 - NoLr * NoLr - NoVr * NoVr - VoL * VoL + 2.0 * NoLr * NoVr * VoL, 0.0, 1.0));
            float NoBr = rInvLengthT * NxLoV, VoBr = rInvLengthT * NxLoV * 2.0 * NoVr;
            float NoLVTr = NoLr * CosA + NoVr + NoTr, VoLVTr = VoL * CosA + 1.0 + VoTr;
            float p = NoBr * VoLVTr, q = NoLVTr * VoLVTr, s = VoBr * NoLVTr;
            float xNum = q * (-0.5 * p + 0.25 * VoBr * NoLVTr);
            float xDenom = p * p + s * (s - 2.0 * p)
                + NoLVTr * ((NoLr * CosA + NoVr) * VoLVTr * VoLVTr + q * (-0.5 * (VoLVTr + VoL * CosA) - 0.5));
            float TwoX1 = 2.0 * xNum / (xDenom * xDenom + xNum * xNum);
            float SinT = TwoX1 * xDenom, CosT = 1.0 - TwoX1 * xNum;
            NoL2 = NoLr * CosA + CosT * NoTr + SinT * NoBr;
            VoL2 = VoL * CosA + CosT * VoTr + SinT * VoBr;
        }
        float InvLenH = rsqrt(2.0 + 2.0 * VoL2);
        NoH = clamp((NoL2 + NoVr) * InvLenH, 0.0, 1.0);
        VoH = clamp(InvLenH + InvLenH * VoL2, 0.0, 1.0);
    }
    float a2s = SphSoft > 0.0 ? min(a2 + SphSoft * SphSoft / (VoH * 3.6 + 0.4), 1.0) : a2;
    float Energy = SphS > 0.0 ? a2s / (a2s + 0.25 * SphS * (3.0 * SqrtFast(a2s) + SphS) / (VoH + 0.001)) : 1.0;
    if (LineCos < 1.0)
    {
        float sa2 = SphS > 0.0 ? a2s + 0.25 * SphS * (3.0 * SqrtFast(a2s) + SphS) / (VoH + 0.001) : a2s;
        float TanA = sqrt((1.0001 - LineCos) / (1.0 + LineCos));
        float la2 = sa2 + 0.25 * TanA * (3.0 * SqrtFast(sa2) + TanA) / (VoH + 0.001);
        Energy *= sqrt(sa2 / la2);
    }
    float d = (NoH * a2s - NoH) * NoH + 1.0;
    float D = a2s / (3.14159265 * d * d);
    float as = sqrt(a2s);
    float Vis = 0.5 / (NoL * (NoV * (1.0 - as) + as) + NoV * (NoL * (1.0 - as) + as));
    float Fc = pow(1.0 - VoH, 5.0);
    float3 F = clamp(50.0 * Spec.g, 0.0, 1.0) * Fc + (1.0 - Fc) * Spec;
    return NoL * (Diffuse * 0.318309886 + Energy * D * Vis * F * Src.z);
}

// The light attenuation's surface shadow of a spot light's dynamic shadow at P (translated world), N the pixel's
// normal, LightPos the light's place: FShadowProjectionPS5falsefalsefalse (6EDAFF54) without its subsurface
// transmission (the DefaultLit pixels this pass lights take none), then the B8G8R8A8 store of its square root and the
// square FDeferredLightPS takes of it (C481DECD lines 219-220).
// Unverified: the per-object fade ($Globals +448 / +452) is taken as off (1) for a whole-scene shadow.
float SpotShadow(SpotShadowRow s, float3 P, float3 N, float3 LightPos)
{
    float4 sp = P.x * s.ToShadow[0] + P.y * s.ToShadow[1] + P.z * s.ToShadow[2] + s.ToShadow[3];
    if (sp.w <= 0.0)
        return 1.0;
    float2 uv = sp.xy / sp.w;
    if (any(uv < s.Bounds.xy) || any(uv > s.Bounds.zw))
        return 1.0;
    float z = min(sp.z, 0.99999);
    float3 L = normalize(LightPos - P);
    float TS = s.Params.y * lerp(s.Texels.x, 1.0, saturate(dot(N, L)));
    // The 6 x 6 texels about the snapped centre by nine Gather4, the edge texels weighted bilinearly: a 5 x 5 PCF.
    float2 t = uv * s.Texels.y - 0.5;
    float2 f = frac(t);
    float2 c = (floor(t) + 0.5) * s.Texels.z;
    float3 at = float3(c, s.Params.x);
    float Sum = 0.0;
    [unroll] for (int j = 0; j < 3; j++)
    {
        [unroll] for (int i = 0; i < 3; i++)
        {
            float4 d = ShadowMaps.GatherRed(ShadowSampler, at, int2(2 * i - 2, 2 * j - 2));
            float4 v = saturate(d * TS - (z * TS - 1.0));
            // Gather4's order: x (0, 1), y (1, 1), z (1, 0), w (0, 0) of its 2 x 2.
            float x0 = i == 0 ? 1.0 - f.x : 1.0, x1 = i == 2 ? f.x : 1.0;
            float y0 = j == 0 ? 1.0 - f.y : 1.0, y1 = j == 2 ? f.y : 1.0;
            Sum += v.w * x0 * y0 + v.z * x1 * y0 + v.x * x0 * y1 + v.y * x1 * y1;
        }
    }
    float S = saturate((Sum * 0.04 - 0.5) * s.Params.w + 0.5);
    float a = lerp(1.0, S * S, s.Params.z);
    float q = round(sqrt(a) * 255.0) / 255.0;
    return q * q;
}

// The PreIntegratedGF table's bilinear read at uv, clamped to its texels.
float2 PreIntegrated(float2 uv)
{
    uint w, h;
    PreIntegratedGF.GetDimensions(w, h);
    int2 size = int2(w, h), hi = size - 1;
    float2 p = clamp(uv, 0.0, 1.0) * float2(size) - 0.5;
    int2 i = int2(floor(p));
    float2 f = p - floor(p);
    float2 t00 = PreIntegratedGF.Load(int3(clamp(i, 0, hi), 0));
    float2 t10 = PreIntegratedGF.Load(int3(clamp(i + int2(1, 0), 0, hi), 0));
    float2 t01 = PreIntegratedGF.Load(int3(clamp(i + int2(0, 1), 0, hi), 0));
    float2 t11 = PreIntegratedGF.Load(int3(clamp(i + int2(1, 1), 0, hi), 0));
    return lerp(lerp(t00, t10, f.x), lerp(t01, t11, f.x), f.y);
}

float4 MainPS(float4 Svp : SV_Position) : SV_Target0
{
    int3 px = int3(Svp.xy, 0);
    float4 g = GBufferB.Load(px);
    // GBufferB.a: the shading model in the low four bits; 0 is unlit (and the cleared background).
    uint GBits = uint(g.a * 255.0 + 0.5);
    uint Model = GBits & 15;
    if (Model == 0)
        discard;
    // DecodeGBufferData: the precomputed shadow factors are GBufferE unless SKIP_PRECSHADOW (bit 5 of GBufferB.a),
    // then 0 with ZERO_PRECSHADOW (bit 6), else 1.
    float4 PrecShadow = (GBits & 32) != 0 ? ((GBits & 64) != 0 ? 0.0 : 1.0) : GBufferE.Load(px);
    float z = SceneDepth.Load(px);
    float4 tw = mul(float4(Svp.x * InvSize.x - 1.0, 1.0 - Svp.y * InvSize.y, z, 1.0), ClipToTranslatedWorld);
    float3 P = tw.xyz / tw.w;
    float3 N = normalize(GBufferA.Load(px).xyz * 2.0 - 1.0);
    float4 c = GBufferC.Load(px);
    float3 Base = c.rgb;
    float3 Diffuse = Base - Base * g.x;
    float3 Spec = lerp((0.08 * g.y).xxx, Base, g.x);
    float Rough = max(g.z, 0.02);
    float a = Rough * Rough, a2 = a * a;
    float3 V = normalize(-P);
    float NoVr = dot(N, V);
    float3 Lit = 0;

    for (uint i = Ranges.x; i < Ranges.x + Ranges.y; i++)
    {
        Light l = Lights[i];
        float3 L = l.DirectionExponent.xyz;
        float3 Src = float3(l.Source.xy, l.ColorSpecular.w);
        Lit += l.ColorSpecular.rgb * DeferredLight(N, V, L, 0.5, 1.0, Src, Diffuse, Spec, a, a2, dot(N, L), 1.0);
    }

    uint2 Tile = uint2(Svp.xy) / Tiles.w;
    uint TileAt = Tiles.y + (Tile.y * Tiles.x + Tile.x) * Tiles.z;
    for (uint w = 0; w < Tiles.z; w++)
    {
    uint Bits = TileLights[TileAt + w];
    while (Bits != 0)
    {
        uint j = firstbitlow(Bits);
        Bits &= Bits - 1;
        Light l = Lights[Ranges.z + 32 * w + j];
        float3 ToL = l.PositionInvRadius.xyz - P;
        float d2 = dot(ToL, ToL);
        float InvRadius = l.PositionInvRadius.w;
        float m = d2 * InvRadius * InvRadius;
        // (the page's own, frame cost) at the light's radius and past it the LightMask is 0 by either falloff.
        if (m >= 1.0)
            continue;
        float InvDist = rsqrt(d2);
        float3 L = ToL * InvDist;
        float Cone = clamp((dot(L, l.DirectionExponent.xyz) - l.Source.z) * l.Source.w, 0.0, 1.0);
        float Exponent = l.DirectionExponent.w;
        bool InvSquared = Exponent == 0.0;
        m = InvSquared ? max(1.0 - m * m, 0.0) : 1.0 - min(m, 1.0);
        m = (InvSquared ? m * m : pow(m, Exponent)) * Cone * Cone;
        // FDeferredLightPS: a light with ShadowedBits takes its precomputed factor (by channel, 1 without one) times
        // its dynamic shadow's attenuation (1 where it has no map); a light without takes the screen-space AO.
        // Unverified: the AO is taken as 1 (the port draws no screen-space AO).
        if (l.Shadow.y != 0.0)
        {
            uint Channel = uint(l.Shadow.x);
            if (Channel > 0)
                m *= PrecShadow[Channel - 1];
            uint Row = uint(l.Shadow.z);
            if (Row > 0 && m > 0.0)
                m *= SpotShadow(SpotShadows[Row - 1], P, N, l.PositionInvRadius.xyz);
        }
        if (m > 0.0)
        {
            float Falloff = 1.0 / (d2 + 1.0);
            float NoLIn = dot(N, L), LineCos = 1.0;
            float Len = l.Tube.w;
            if (Len > 0.0)
            {
                float3 T = l.Tube.xyz;
                float3 L0 = ToL - 0.5 * Len * T, L1 = ToL + 0.5 * Len * T;
                float il0 = rsqrt(dot(L0, L0)), il1 = rsqrt(dot(L1, L1)), il01 = il0 * il1;
                LineCos = dot(L0, L1) * il01;
                Falloff = il01 / (LineCos * 0.5 + 0.5 + il01);
                NoLIn = 0.5 * (dot(N, L0) * il0 + dot(N, L1) * il1);
                float3 R = 2.0 * NoVr * N - V;
                float3 L01 = L1 - L0;
                float B = dot(R, L01);
                float3 Pc = L0 + clamp(dot(L0, B * R - L01) / (Len * Len - B * B), 0.0, 1.0) * L01;
                InvDist = rsqrt(dot(Pc, Pc));
                L = Pc * InvDist;
            }
            float3 Src = float3(l.Source.xy, l.ColorSpecular.w);
            Lit += l.ColorSpecular.rgb * (m * (InvSquared ? Falloff : 1.0))
                * DeferredLight(N, V, L, Falloff, InvDist, Src, Diffuse, Spec, a, a2, NoLIn, LineCos);
        }
    }
    }

    if (SkyParams.z > 0.0)
    {
        // IndirectIrradiance from GBufferC's alpha.
        float ii = exp2(16.0 * floor(saturate(c.a) * 255.0 + 0.5) / 255.0 - 8.0) - 0.003906;
        float ra = g.z * g.z;
        float3 Rs = lerp(N, 2.0 * NoVr * N - V, (1.0 - ra) * (sqrt(1.0 - ra) + ra));
        float LevelFrom1x1 = 1.0 - 1.2 * log2(max(g.z, 0.001));
        // The captures' composite (FReflectionEnvironmentSkyLightingPS p10, the sphere capture loop): each capture
        // whose influence sphere holds the pixel, in the list's order (smallest InfluenceRadius first), under the
        // ones before it. Ibl.a is the coverage left, Avg the composited AverageBrightness and its weight left.
        float4 Ibl = float4(0.0, 0.0, 0.0, 1.0);
        float2 Avg = float2(0.0, 1.0);
        float CapLevel = CaptureInfo.y - 1.0 - LevelFrom1x1;
        [unroll]
        for (uint k = 0; k < MAX_CAPTURES; k++)
        {
            if (k < uint(CaptureInfo.x) && Ibl.a >= 0.001)
            {
                float4 PosRad = Captures[3 * k];
                float3 Cv = P - PosRad.xyz;
                float D2 = dot(Cv, Cv);
                float D = sqrt(D2);
                if (D < PosRad.w)
                {
                    // GetLookupVectorForSphereCapture: the ray from the pixel meets the influence sphere; the cube is
                    // looked up from the capture (moved by CaptureOffset) toward that point.
                    float Nd = saturate(D / PosRad.w);
                    float B = dot(Rs, Cv);
                    float Q = B * B - (D2 - PosRad.w * PosRad.w);
                    float Alpha = 0.0;
                    float3 Look = Rs;
                    if (Q >= 0.0)
                    {
                        Look = Cv + (sqrt(Q) - B) * Rs - Captures[3 * k + 2].xyz;
                        float x = max(Nd * 2.5 - 1.5, 0.0);
                        Alpha = 1.0 - x * x * (3.0 - 2.0 * x);
                    }
                    float4 S;
                    if (k == 0)
                        S = CaptureCube0.SampleLevel(SkySampler, Look, CapLevel);
                    else if (k == 1)
                        S = CaptureCube1.SampleLevel(SkySampler, Look, CapLevel);
                    else if (k == 2)
                        S = CaptureCube2.SampleLevel(SkySampler, Look, CapLevel);
                    else
                        S = CaptureCube3.SampleLevel(SkySampler, Look, CapLevel);
                    Ibl.rgb += S.rgb * Captures[3 * k + 1].x * Alpha * Ibl.a;
                    Ibl.a *= 1.0 - S.a * Alpha;
                    Avg.x += Captures[3 * k + 2].w * Alpha * Avg.y;
                    Avg.y *= 1.0 - Alpha;
                }
            }
        }
        // Unverified: View.IndirectLightingColorScale (cb0[157], which scales the captures and their
        // AverageBrightness) is taken as 1, the port's View value.
        // The stationary sky light (ReflectionStruct's dynamic flag 0) fills the coverage left, its average
        // brightness the weight left.
        float3 Sky = SkyCube.SampleLevel(SkySampler, Rs, SkyParams.x - 1.0 - LevelFrom1x1).rgb * SkyColor.rgb;
        float SkyAvg = dot(SkyColor.rgb, float3(0.3, 0.59, 0.11)) * SkyParams.y;
        Ibl.rgb += Ibl.a * Sky;
        Avg.x += SkyAvg * Avg.y;
        float MixS = clamp(g.z * 5.0 - 0.5, 0.0, 1.0);
        MixS = MixS * MixS * (3.0 - 2.0 * MixS);
        Ibl.rgb *= 1.0 + MixS * (min(ii / max(Avg.x, 1e-4), 10000.0) - 1.0);
        float2 ab = PreIntegrated(float2(clamp(NoVr, 0.0, 1.0), g.z));
        Lit += max(Ibl.rgb * (Spec * ab.x + clamp(50.0 * Spec.g, 0.0, 1.0) * ab.y), 0.0);
    }
    return float4(Lit, 0.0);
}
