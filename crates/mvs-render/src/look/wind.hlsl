// (the port's own) Wind's gust, the approved look of `tools/looks/wind`: a warning (chevrons pulse at the upwind
// edge, a few wisps start), then the gust (streaks and dust race across the screen, the picture leans downwind).
// Params[0]: seconds since the warning started, the gust's push (0..1), its way (sim x: -1 or 1), the warning's
// length (s). Tex0: M010 wind line mask, Tex1: noise streaks (both in red).

float GustAmount() {
    return Params[0].y;
}

float WarnAmount() {
    float age = Params[0].x;
    float warn = Params[0].w;
    return smoothstep(0, 0.2, age) * (1 - smoothstep(warn + 0.2, warn + 0.6, age));
}

// The gust's way on the screen (+1 right): sim x is UE -Y, projected.
float ScreenWay() {
    float3 o = -CameraOrigin.xyz;
    float sx = ScreenUV(o + float3(0, -Params[0].z * 1000, 0)).x - ScreenUV(o).x;
    return sx >= 0 ? 1 : -1;
}

// A chevron pointing +x at `p` (units: its own size), 1 inside.
float Chevron(float2 p) {
    p.y = abs(p.y);
    float d = abs(p.x + p.y * 0.9 - 0.25) - 0.16;
    float inside = step(d, 0) * step(p.y, 0.55);
    return inside;
}

float ChevronEdge(float2 p, float grow) {
    p.y = abs(p.y);
    float d = abs(p.x + p.y * 0.9 - 0.25) - 0.16 - grow;
    return step(d, 0) * step(p.y, 0.55 + grow);
}

float4 Wind(ScreenIn i) : SV_Target {
    float dir = ScreenWay();
    float g = GustAmount();
    float w = WarnAmount();
    float aspect = Screen.x / Screen.y;
    // Screen place with x running downwind.
    float2 s = float2((dir > 0 ? i.uv.x : 1 - i.uv.x) * aspect, i.uv.y);

    // The picture leans a little downwind in the gust (a short smear, not a blur).
    float3 col = 0;
    float total = 0;
    for (int k = 0; k < 4; k++) {
        float wk = 1.0 - k * 0.25;
        col += SceneColor(i.uv - float2(dir * k * 0.0009 * g, 0)) * wk;
        total += wk;
    }
    col /= total;

    float lum = dot(col, float3(0.3, 0.59, 0.11));

    // Streaks: separate long thin lines in lanes, tapered at the tail, a slight wave along them, in two depths.
    float lines = 0;
    for (int l = 0; l < 2; l++) {
        float laneH = 0.045 - l * 0.012;
        float speed = 1.5 + l * 1.2;
        float lane = floor(s.y / laneH + l * 0.5);
        float pick = Hash(float2(lane, 7.3 + l));
        float len = 0.35 + 0.35 * Hash(float2(lane, 2.1 + l));
        float period = len * 2.2 + 0.6;
        float x = frac((s.x - Time * speed * (0.8 + 0.4 * pick)) / period + pick) * period;
        // 0 at the head, 1 at the tail.
        float along = x / len;
        float yc = (lane + 0.5 - l * 0.5) * laneH + 0.006 * sin(s.x * 18 + Time * 5 + pick * 20) + (pick - 0.5) * laneH * 0.5;
        float thick = (0.0042 + 0.002 * l) * (1 - along * 0.85);
        float streak = saturate(1 - abs(s.y - yc) / thick) * step(along, 1) * smoothstep(0, 0.06, along);
        streak *= step(0.45, Hash(float2(lane, 9.9 + l)));
        // A textured edge: the M010 wind streak mask breaks the streak slightly.
        streak *= 0.75 + 0.25 * Tex0.SampleLevel(Wrap, float2(x * 1.5, yc * 3), 0).r;
        lines += streak * (0.55 + 0.35 * l) * (1 - along * 0.6);
    }
    float3 streakCol = float3(1.25, 1.35, 1.5) * max(lum * 1.3, 0.45);
    // During the warning only a few thin wisps start.
    col = lerp(col, streakCol, saturate(lines * (g + w * 0.25)));

    // Dust: specks racing downwind with short tails, denser near the bottom.
    float dust = 0;
    for (int d = 0; d < 2; d++) {
        float size = 0.035 + 0.02 * d;
        float speed = 1.3 + 0.8 * d;
        float2 q = float2(s.x - Time * speed, s.y) / size + 31.7 * d;
        float2 cell = floor(q);
        float pick = Hash(cell + 4.1);
        float2 at = 0.2 + 0.6 * Hash2(cell);
        float2 o = frac(q) - at;
        o.y += 0.12 * sin(Time * 6 + pick * 40);
        float tail = saturate(-o.x / 0.9);
        float r = length(float2(o.x < 0 ? o.x * 0.12 : o.x, o.y));
        float speck = saturate(1 - r / (0.07 + 0.05 * pick)) * (o.x < 0 ? (1 - tail) : 1);
        dust += speck * step(0.72, pick) * (0.5 + 0.5 * smoothstep(0.2, 0.95, s.y));
    }
    float3 dustCol = float3(0.78, 0.66, 0.5) * max(lum * 1.2 + 0.25, 0.45);
    col = lerp(col, dustCol, saturate(dust) * g * 0.9);

    // The warning: three chevrons at the upwind edge, lit one after another, the whole set pulsing.
    float2 c = float2((dir > 0 ? i.uv.x : 1 - i.uv.x) * aspect, i.uv.y);
    float arrows = 0;
    float edges = 0;
    for (int a = 0; a < 3; a++) {
        float2 p = (c - float2(0.10 + a * 0.085, 0.5)) / 0.075;
        float lit = 0.35 + 0.65 * saturate(sin((Time * 3.2 - a * 0.55) * 3.1416));
        arrows += Chevron(p) * lit;
        edges += ChevronEdge(p, 0.06);
    }
    float pulse = 0.75 + 0.25 * sin(Time * 12.0);
    float show = w * pulse;
    col = lerp(col, float3(0.02, 0.03, 0.06), saturate(edges) * show * 0.9);
    col = lerp(col, float3(1.6, 1.9, 2.4), saturate(arrows) * show);
    // A thin band of air brightening at the upwind edge as the warning runs.
    float edgeGlow = exp(-c.x * 14) * w * 0.35;
    col += float3(0.5, 0.7, 1.0) * edgeGlow * max(lum, 0.3);

    return float4(col, 1);
}
