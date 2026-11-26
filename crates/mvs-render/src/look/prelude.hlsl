// (the port's own) What every look's shader file starts with: its buffer, the scene's colour and depth, the store
// textures it names, two samplers, noise helpers and the two vertex shaders. A look's file adds pixel shaders only.

cbuffer Look : register(b0) {
    // Translated world (world - CameraOrigin, cm, Z up) to clip.
    row_major float4x4 WorldToClip;
    float4 CameraOrigin;
    // Width, height, 1 / width, 1 / height of the scene targets.
    float4 Screen;
    // The look's clock (s from its start), then the device-Z-to-depth terms.
    float4 TimeDepth;
    // The shape drawn: its centre (translated world) and half axes (cm).
    float4 Center;
    float4 AxisU;
    float4 AxisV;
    // The look file's `params`.
    float4 Params[4];
    // Clip to translated world, WorldToClip's inverse.
    row_major float4x4 ClipToWorld;
};

Texture2D SceneColorTex : register(t0);
Texture2D<float> SceneDepthTex : register(t1);
Texture2D Tex0 : register(t2);
Texture2D Tex1 : register(t3);
Texture2D Tex2 : register(t4);
Texture2D Tex3 : register(t5);
Texture2D Tex4 : register(t6);
Texture2D Tex5 : register(t7);
Texture2D Tex6 : register(t8);
Texture2D Tex7 : register(t9);
// GBufferA: the world normal as n * 0.5 + 0.5 (rgb).
Texture2D SceneNormalTex : register(t10);
SamplerState Wrap : register(s0);
SamplerState Clamp : register(s1);

#define Time (TimeDepth.x)

// The scene's depth (cm along the view) at pixel `px`; far where nothing was drawn.
float SceneDepth(float2 px) {
    float z = SceneDepthTex.Load(int3(px, 0));
    return z <= 0 ? 1e9 : TimeDepth.z / (z - TimeDepth.y);
}

// The scene's world place (cm) at pixel `px` (its uv `uv`); far where nothing was drawn.
float3 SceneWorld(float2 px, float2 uv) {
    float z = SceneDepthTex.Load(int3(px, 0));
    float4 w = mul(float4(uv.x * 2 - 1, 1 - uv.y * 2, max(z, 1e-7), 1), ClipToWorld);
    return w.xyz / w.w + CameraOrigin.xyz;
}

// The scene's world normal at pixel `px` (0 where no G-buffer was written).
float3 SceneNormal(float2 px) {
    float3 n = SceneNormalTex.Load(int3(px, 0)).rgb * 2 - 1;
    return dot(n, n) > 0.01 ? normalize(n) : 0;
}

// The scene's colour (linear, before the post chain) at `uv`.
float3 SceneColor(float2 uv) {
    return SceneColorTex.SampleLevel(Clamp, uv, 0).rgb;
}

// A translated world point's screen UV.
float2 ScreenUV(float3 translated) {
    float4 c = mul(float4(translated, 1), WorldToClip);
    return float2(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
}

float Hash(float2 p) {
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float2 Hash2(float2 p) {
    return float2(Hash(p), Hash(p + 17.31));
}

// Value noise, 0..1.
float Noise(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3 - 2 * f);
    return lerp(lerp(Hash(i), Hash(i + float2(1, 0)), u.x), lerp(Hash(i + float2(0, 1)), Hash(i + 1), u.x), u.y);
}

// Fractal noise of `octaves` octaves, 0..1.
float Fbm(float2 p, int octaves) {
    float sum = 0, amp = 0.5, total = 0;
    for (int k = 0; k < octaves; k++) {
        sum += Noise(p) * amp;
        total += amp;
        p = p * 2.03 + 11.7;
        amp *= 0.5;
    }
    return sum / total;
}

// Cellular noise: the distances to the nearest and second nearest cell points (x, y); the points drift with `t`.
float2 Cells(float2 p, float t) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 d = 8;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            float2 g = float2(x, y);
            float2 o = Hash2(i + g);
            o = 0.5 + 0.4 * sin(t + 6.2831 * o);
            float e = length(g + o - f);
            if (e < d.x) {
                d.y = d.x;
                d.x = e;
            } else if (e < d.y) {
                d.y = e;
            }
        }
    }
    return d;
}

struct ShapeIn {
    float4 pos : SV_Position;
    // 0..1 across the shape, v from its top edge (+AxisV) down.
    float2 uv : TEXCOORD0;
    // The pixel's world place (cm).
    float3 world : TEXCOORD1;
    // Its depth along the view (cm), to compare with SceneDepth.
    float depth : TEXCOORD2;
};

struct ScreenIn {
    float4 pos : SV_Position;
    float2 uv : TEXCOORD0;
};

ShapeIn ShapeVS(uint id : SV_VertexID) {
    ShapeIn o;
    float2 uv = float2(id & 1, id >> 1);
    float3 p = Center.xyz + AxisU.xyz * (uv.x * 2 - 1) + AxisV.xyz * (1 - uv.y * 2);
    o.pos = mul(float4(p, 1), WorldToClip);
    o.uv = uv;
    o.world = p + CameraOrigin.xyz;
    o.depth = o.pos.w;
    return o;
}

ScreenIn ScreenVS(uint id : SV_VertexID) {
    ScreenIn o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
    o.uv = uv;
    return o;
}
