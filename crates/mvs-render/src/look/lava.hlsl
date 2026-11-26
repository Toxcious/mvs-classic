// Rising Lava: a cartoon lava sea under the stage. Raised crust plates drift on molten cracks, bubbles pop through,
// the stage glows orange from below, heat shimmers over the surface and embers rise from it (all in the world).
// Params[0]: surface Z at the start (cm), rise (cm/s). Params[1]: glow colour (rgb), glow reach (cm).

static const float3 CRUST_DARK = float3(0.030, 0.008, 0.006);
static const float3 CRUST_TOP = float3(0.16, 0.05, 0.035);
static const float3 MOLTEN = float3(2.4, 0.55, 0.05);
static const float3 HOT = float3(8.0, 3.6, 0.8);

float Level() {
    return Params[0].x + Params[0].y * Time;
}

// The flow's warp: plates drift one way and churn slowly.
float2 Flow(float2 p) {
    float2 drift = float2(Time * 0.05, Time * 0.018);
    float2 w = float2(Fbm(p * 0.45 + 3.1 + Time * 0.03, 3), Fbm(p * 0.45 + 9.7 - Time * 0.025, 3)) - 0.5;
    return p + drift + w * 1.5;
}

// The crust's height at `p` (0 crack .. 1 plate top) and the crack's closeness (1 on its line).
float2 Crust(float2 p) {
    float2 c = Cells(Flow(p), Time * 0.25);
    float edge = c.y - c.x;
    float plate = smoothstep(0.02, 0.22, edge);
    // Plates are domed, worn at their middle.
    float dome = plate * (0.75 + 0.25 * saturate(1 - c.x * 1.3));
    return float2(dome, saturate(1 - edge / 0.07));
}

float4 Lava(ShapeIn i) {
    float2 p = i.world.xy / 380.0;
    float2 cr = Crust(p);
    // The plate's slope by differences, for its shading.
    float e = 0.04;
    float hx = Crust(p + float2(e, 0)).x - cr.x;
    float hy = Crust(p + float2(0, e)).x - cr.x;
    float3 n = normalize(float3(-hx * 3.5, -hy * 3.5, e * 2));
    // Molten heat under the crust: patches that well up and sink.
    float heat = Fbm(p * 0.35 - Time * 0.05, 4);
    heat = saturate((heat - 0.40) * 3.2);
    // The crust lit by the sky from above-behind, banded as the game's toon light is.
    float3 toSky = normalize(float3(-0.5, 0.2, 0.85));
    float lit = saturate(dot(n, toSky));
    lit = floor(lit * 3 + 0.6) / 3;
    float3 crust = lerp(CRUST_DARK, CRUST_TOP, lit);
    // A plate's edge is thinner, so it glows from beneath.
    float thin = saturate(1 - cr.x * 1.25);
    crust += MOLTEN * 0.35 * thin * thin;
    // Where heat wells up the crust melts away.
    float melt = smoothstep(0.55, 0.85, heat + (1 - cr.x) * 0.3);
    float3 col = lerp(crust, MOLTEN * (0.7 + 0.5 * heat), melt);
    // Cracks: a bright line, a pulse running along them.
    float pulse = 0.75 + 0.25 * sin(Time * 2.2 + dot(i.world.xy, float2(0.004, 0.006)) + heat * 6);
    col = lerp(col, HOT * pulse, cr.y * cr.y);
    // Bubbles: in molten patches a dome swells and pops, its ring spreading.
    float2 bc = p * 1.3 + float2(Time * 0.05, 0);
    float2 cell = floor(bc);
    float2 at = 0.25 + 0.5 * Hash2(cell);
    float phase = frac(Time * (0.35 + 0.3 * Hash(cell + 5.7)) + Hash(cell + 1.3));
    float r = length(frac(bc) - at);
    float swell = smoothstep(0.75, 0.0, phase) * saturate(1 - r / (0.12 + 0.18 * phase));
    float ring = saturate(1 - abs(r - phase * 0.45) / 0.03) * smoothstep(1.0, 0.75, phase) * step(0.75, phase);
    float bubbly = step(0.45, Hash(cell + 9.1)) * melt;
    col += (MOLTEN * swell * 0.8 + HOT * ring * 0.6) * bubbly;
    // A hot lip where the lava meets the stage.
    float gap = SceneDepth(i.pos.xy) - i.depth;
    col = lerp(col, HOT * 0.9, saturate(1 - gap / 45.0) * 0.85);
    // Far lava sinks into a red haze: no hard back edge, no shimmering detail far off.
    float far = saturate((i.depth - 9000) / 6000);
    col = lerp(col, float3(0.5, 0.08, 0.02), far * 0.9);
    return float4(col, 1);
}

// Embers on a vertical sheet: rising, swaying, fading with height. Additive.
float4 Embers(ShapeIn i) {
    float level = Level();
    float h = i.world.z - level;
    float3 col = 0;
    for (int k = 0; k < 2; k++) {
        float size = 70 + 40 * k;
        float speed = 90 + 50 * k;
        float2 q = float2(i.world.y, i.world.z - Time * speed) / size + 13.7 * k;
        float2 cell = floor(q);
        float pick = Hash(cell + 2.9);
        float2 at = 0.2 + 0.6 * Hash2(cell);
        at.x += 0.15 * sin(Time * (1.5 + pick * 2) + pick * 30);
        float d = length((frac(q) - at) * size);
        float core = saturate(1 - d / (4.5 + 3 * pick));
        float halo = saturate(1 - d / 22);
        float flick = 0.6 + 0.4 * sin(Time * (8 + pick * 9) + pick * 50);
        float on = step(0.80, pick) * flick;
        col += (float3(6, 2.3, 0.5) * core + float3(0.8, 0.22, 0.03) * halo * halo) * on;
    }
    float fade = saturate(h / 40) * exp(-max(h, 0) / 380);
    return float4(col * fade, 1);
}

// Over the scene: the lava's light on the stage by its height over the surface, and the heat's shimmer.
float4 Heat(ScreenIn i) : SV_Target {
    float level = Level();
    float reach = Params[1].w;
    float3 glow = Params[1].rgb;
    // The shimmer: pixels over the surface bend by a noise that rises with the heat (the world's, so it stays put).
    float3 P = SceneWorld(i.pos.xy, i.uv);
    float d = P.z - level;
    float near = saturate(1 - d / 350) * step(-30, d) * step(length(P - CameraOrigin.xyz), 60000);
    float2 q = float2(P.y / 90, (P.z - Time * 140) / 70);
    float2 uv = i.uv + (float2(Noise(q), Noise(q + 7.3)) - 0.5) * float2(0.0045, 0.003) * near;
    float3 col = SceneColor(uv);
    // The light: from below, strongest on faces turned down or toward the lava, flickering with the flow.
    float3 Pl = SceneWorld(i.pos.xy, i.uv);
    float dl = Pl.z - level;
    float3 n = SceneNormal(i.pos.xy);
    float facing = dot(n, n) > 0 ? saturate(0.5 - n.z * 0.9) + 0.3 : 0.6;
    float flick = 0.85 + 0.15 * Noise(float2(Pl.y / 600 + Time * 0.8, Time * 1.3));
    float l = exp(-max(dl, 0) / reach) * step(-20, dl) * facing * flick;
    l = pow(l, 1.4) * (1 - smoothstep(10500, 13500, length(Pl - CameraOrigin.xyz)));
    // Tinted toward the glow at about its own brightness (brightening alone turns white in the tonemapper).
    float lum = dot(col, float3(0.3, 0.59, 0.11));
    float3 warm = lum * glow * float3(3.2, 2.6, 2.0) + glow * 0.06;
    col = lerp(col, warm, saturate(l * 2.2)) * (1 + l * 0.8);
    return float4(col, 1);
}
