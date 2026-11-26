// (the port's own) Black Hole, the approved look of `tools/looks/black-hole`: a dark core at a spot over the stage, a swirling glowing disc around it, the picture bending and
// twisting toward it, wisps of light spiralling in, and a dashed ring of the game's turning slowly further out.
// Params[0]: the hole's world place (cm, UE), core radius (cm). Params[1]: glow colour (rgb), spin (rad/s).
// Params[2].x: how far open (0..1).
// Tex0: T_RingGraphic_01 (red mask), Tex1: T_NoiseStringy02 (red mask).

float2 Rot(float2 p, float a) {
    float s = sin(a), c = cos(a);
    return float2(p.x * c - p.y * s, p.x * s + p.y * c);
}

float4 Lens(ScreenIn i) : SV_Target {
    float3 at = Params[0].xyz - CameraOrigin.xyz;
    float2 c = ScreenUV(at);
    float2 e = ScreenUV(at + float3(0, Params[0].w, 0));
    float aspect = Screen.x / Screen.y;
    // Opens with a small overshoot (and closes the same way), then breathes.
    float t = Time;
    float open = saturate(Params[2].x);
    float grow = open * open * (3 - 2 * open);
    grow *= 1 + 0.12 * sin(open * 3.1416);
    float breathe = 1 + 0.04 * sin(t * 2.6);
    float R = length((e - c) * float2(aspect, 1)) * grow * breathe;
    R = max(R, 1e-4);

    float2 p = (i.uv - c) * float2(aspect, 1);
    float dist = length(p);
    float d = dist / R;
    float ang = atan2(p.y, p.x);
    float spin = Params[1].w;

    // The bend: the picture pulled in toward the hole and twisted around it.
    float pull = 0.32 / max(d * d, 0.7);
    float twist = 0.9 / max(d * d, 0.9) * grow;
    float2 q = Rot(p, twist) * (1 - saturate(pull) * 0.7 * step(1, d));
    float2 uv = c + q / float2(aspect, 1);
    float3 col = SceneColor(uv);
    // Light near it dims and goes violet, as if pulled into it.
    float near = saturate(1.8 / max(d, 0.01) - 0.4);
    col = lerp(col, col * float3(0.55, 0.45, 0.85), saturate(near * 0.5));

    float3 glow = Params[1].rgb;
    float3 hot = float3(3.5, 3.0, 4.5);

    // The disc: swirling bands between 1.0 and 2.4 core radii, the inner edge hottest, turning faster inside.
    float sw = ang + spin * t / max(d * 0.7, 0.35) + 2.2 / max(d, 0.5);
    float2 pol = float2(cos(sw), sin(sw)) * (d * 1.6);
    float bands = Fbm(pol * 1.4 + d * 2.5, 4);
    bands = smoothstep(0.35, 0.8, bands);
    float disc = smoothstep(0.95, 1.12, d) * (1 - smoothstep(1.4, 2.5, d));
    float heat = saturate(1 - (d - 1.0) / 1.3);
    float3 discCol = lerp(glow, hot, heat * heat);
    col = lerp(col, discCol * (0.35 + 1.1 * bands), disc * (0.25 + 0.7 * bands) * grow);

    // The photon ring: a thin bright edge at the core.
    float ring = exp(-abs(d - 1.04) * 28);
    col += hot * ring * 1.2 * grow;

    // The core: black, with a faint violet sheen.
    float core = 1 - smoothstep(0.92, 1.02, d);
    col = lerp(col, glow * 0.02, core);

    // Wisps: light spiralling in from far out (the stringy noise mask, sampled along the spiral).
    float spiral = ang + 0.9 * log(max(d, 0.05)) * 3.0 + t * spin * 0.6;
    float2 wq = float2(spiral / 6.2831 * 3.0, log(max(d, 0.05)) * 0.9 + t * 0.55);
    float wisp = Tex1.SampleLevel(Wrap, wq, 0).r;
    wisp = smoothstep(0.45, 0.85, wisp) * smoothstep(1.2, 1.8, d) * (1 - smoothstep(3.0, 5.5, d));
    col += glow * wisp * 0.9 * grow;

    // The game's dashed ring, turning the other way, faint, further out.
    float2 rp = Rot(p / (R * 3.4), -t * 0.35) * 0.5 + 0.5;
    float dash = Tex0.SampleLevel(Clamp, saturate(rp), 0).r * step(abs(rp.x - 0.5), 0.5) * step(abs(rp.y - 0.5), 0.5);
    col += glow * dash * 0.8 * grow;

    // A soft halo.
    col += glow * 0.25 * exp(-max(d - 1, 0) * 1.2) * grow * (1 - core);
    return float4(col, 1);
}
