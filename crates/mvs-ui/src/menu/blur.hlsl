// UBackgroundBlur's pass (menu/blur.rs): the rectangle of the picture so far taken down to the pass's size with its edge
// pixels repeated around it, the table's taps across and down, the result laid back over the rectangle.

cbuffer Blur : register(b0)
{
    // x0, y0 (the rectangle's top left on the target), sw, sh (its source size, pixels)
    float4 Rect;
    // fw, fh (the pass's picture), pad, taps (the centre and the pairs one side)
    float4 Dims;
    // the backdrop's size, the scratch textures' size
    float4 Sizes;
    // the step of one unit of a tap's offset in uv
    float4 Dir;
    // PW, PH (the padded picture), bw, bh (the rectangle on the target)
    float4 Pic;
    // (offset, weight)
    float4 Taps[130];
};

Texture2D Source : register(t0);
SamplerState Smp : register(s0);

struct VSOut { float4 pos : SV_Position; };

VSOut MainVS(uint id : SV_VertexID)
{
    VSOut o;
    float2 p = float2((id << 1) & 2, id & 2);
    o.pos = float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}

// The rectangle down to fw x fh, the picture clamped to its own edge pixels around it.
float4 DownPS(VSOut i) : SV_Target
{
    float2 p = i.pos.xy - Dims.z;
    p = clamp(p, float2(0.5, 0.5), Dims.xy - 0.5);
    float2 src = Rect.xy + p * (Rect.zw / Dims.xy);
    return float4(Source.SampleLevel(Smp, src / Sizes.xy, 0).rgb, 1);
}

// One pass of the table along Dir: the centre and each pair's two sides, the sum not normalised.
float4 BlurPS(VSOut i) : SV_Target
{
    float2 uv = i.pos.xy / Sizes.zw;
    float2 lo = 0.5 / Sizes.zw;
    float2 hi = (Pic.xy - 0.5) / Sizes.zw;
    float3 c = Taps[0].y * Source.SampleLevel(Smp, clamp(uv, lo, hi), 0).rgb;
    int n = (int)Dims.w;
    for (int k = 1; k < n; k++) {
        float2 o = Taps[k].x * Dir.xy;
        c += Taps[k].y * (Source.SampleLevel(Smp, clamp(uv + o, lo, hi), 0).rgb
                        + Source.SampleLevel(Smp, clamp(uv - o, lo, hi), 0).rgb);
    }
    return float4(saturate(c), 1);
}

// The pass's picture over the rectangle of the target.
float4 UpPS(VSOut i) : SV_Target
{
    float2 l = (i.pos.xy - Rect.xy) / Pic.zw;
    float2 src = Dims.z + l * Dims.xy;
    return float4(Source.SampleLevel(Smp, clamp(src, float2(0.5, 0.5), Pic.xy - 0.5) / Sizes.zw, 0).rgb, 1);
}
