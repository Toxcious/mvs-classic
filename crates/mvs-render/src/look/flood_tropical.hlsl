// Flood A, clear tropical water: a turquoise flood that rises from below the stage and settles over the main floor.
// One screen pass ray-casts the water as a box (its front face at x = Params[0].w, just in front of the fighters,
// its top the wavy surface): what is seen through it is refracted, tinted and hazed by the path in water and the
// depth below the surface, caustics ripple over submerged faces, light shafts fall from the surface, bubbles rise in
// streams, and a foam line runs along the waterline.
// Params[0]: surface Z at the start (cm), settled Z (cm), rise time (s; 0 = already settled), front face X (cm).
// Params[1]: wave amplitude (cm), wave speed scale, clarity (absorption length along the view, cm), depth haze (cm).
// The game's water (`flood.rs`) sets a rise time under 0: the surface is then Params[0].x, and Params[2] is how far
// risen (0..1), the seconds since the rise began, the stage's middle (UE Y, cm); Params[4..7] are the splashes where
// fighters cross the surface (UE Y, seconds since, strength, seed; none while the seconds are under 0).

static const float3 SHALLOW = float3(0.01, 0.38, 0.52);
static const float3 DEEP = float3(0.003, 0.10, 0.22);
static const float3 TRANSMIT = float3(0.38, 0.86, 1.0);
static const float3 FOAM = float3(1.4, 1.7, 1.7);
static const float3 SHEEN = float3(0.35, 0.85, 0.90);
static const float3 CAUSTIC = float3(0.8, 1.4, 1.5);
// The sky the far surface reflects (near the horizon, overhead) and the sun's streak.
static const float3 SKY_LOW = float3(0.36, 0.80, 1.0);
static const float3 SKY_HIGH = float3(0.12, 0.42, 0.90);
static const float3 SUN = float3(2.2, 2.1, 1.8);

// The pixel's view angle (radians) and how flat the view runs to the water (|d.z|), set at the pass's start: every
// pattern is faded out before it gets finer than about two pixels where it is drawn.
static float PixAngle;
static float Flat;

// A pixel's footprint (cm) at view distance t: across the view, and along the view on the water's top.
float2 FootAt(float t) {
    float a = t * PixAngle;
    return float2(a, a / max(Flat, 0.02));
}

// How much of a pattern of wavelengths lam (cm, across and along) survives a footprint fp: 1 down to 0 by 2 px.
float Keep(float2 lam, float2 fp) {
    return 1 - smoothstep(0.2, 0.5, max(fp.x / lam.x, fp.y / lam.y));
}

// Whether the game drives the water (else the preview's rise from Time).
bool Game() {
    return Params[0].z < 0;
}

// How far the rise is, 0..1, eased in and out.
float Risen() {
    if (Game()) {
        return Params[2].x;
    }
    float k = Params[0].z <= 0 ? 1 : saturate(Time / Params[0].z);
    return k * k * (3 - 2 * k);
}

float Level() {
    return Game() ? Params[0].x : lerp(Params[0].x, Params[0].y, Risen());
}

// A wave with sharp crests and round troughs.
float Crest(float x) {
    float s = 0.5 + 0.5 * sin(x);
    return s * s * 2 - 0.7;
}

// The surface's height over its level at world (y, x), band-limited to the footprint fp.
float Wave(float2 p, float2 fp) {
    float t = Time * Params[1].y;
    // Choppier while it rises.
    float a = Params[1].x * (1 + 1.2 * (1 - Risen()));
    float w = 0.50 * lerp(0.05, Crest(p.x / 300 - t * 1.4 + p.y * 0.0011), Keep(float2(940, 2850), fp));
    w += 0.30 * sin(p.x / 130 + t * 2.2 + p.y * 0.0035) * Keep(float2(817, 1795), fp);
    w += 0.12 * sin(p.x / 55 - t * 3.6 - p.y * 0.006) * Keep(float2(345, 1047), fp);
    w += (Noise(float2(p.x / 110 - t * 0.9, p.y / 160 + t * 0.4)) - 0.5) * 0.45 * Keep(float2(110, 160), fp);
    return w * a;
}

float Surface(float3 w, float2 fp) {
    return Level() + Wave(w.yx, fp);
}

// The surface's slope (d/dy, d/dx) at world (y, x).
float2 Slope(float2 p, float2 fp) {
    float2 e = max(6, fp);
    float h = Wave(p, fp);
    return float2(Wave(p + float2(e.x, 0), fp) - h, Wave(p + float2(0, e.y), fp) - h) / e;
}

// One bubble at offset `o` (cm) from its middle, radius `r`, its edge `soft` cm wide: body, rim, highlight.
float3 Bubble(float2 o, float r, float soft) {
    float d = length(o);
    float rim = smoothstep(r + soft, r - soft * 0.4, d) * smoothstep(r * 0.45, r * 0.95, d);
    float body = smoothstep(r + soft, r - soft, d);
    float hi = smoothstep(r * 0.42, r * 0.08, length(o - float2(-r * 0.35, r * 0.38)));
    return float3(body, rim, hi);
}

// Bubble streams on a sheet, at sheet point b = (y, z). Each slot `span` cm wide holds a stream by chance `dens`,
// anywhere in it, with its own start depth, speed and push, emission gap, size, growth, wobble, sideways current,
// spread and bursts (a slow noise of the emission time switches it on and off); each bubble has its own lifetime,
// size, emission jitter and place in the spread, and a burst's peak puffs extra bubbles around it (a plume). A
// bubble's age is solved from its height, so only the three nearest emissions of each stream are drawn.
// k: x = gap scale, y = size scale, z = spread (cm), w = edge softness (cm).
float3 Streams(float2 b, float seed, float span, float dens, float4 k) {
    float3 acc = 0;
    float slot0 = floor(b.x / span);
    for (int j = -1; j <= 1; j++) {
        float slot = slot0 + j;
        float2 sid = float2(slot, seed * 13.7);
        if (Hash(sid) > dens) {
            continue;
        }
        float x0 = (slot + Hash(sid + 1.3)) * span;
        float z0 = lerp(-1300, -150, Hash(sid + 2.7));
        float v = lerp(45, 105, Hash(sid + 3.1));
        float push = lerp(4, 22, Hash(sid + 3.9));
        float gap = lerp(0.10, 0.55, Hash(sid + 4.9)) * k.x;
        float r0 = lerp(2.2, 5.5, Hash(sid + 5.3)) * k.y;
        float grow = lerp(0.04, 0.16, Hash(sid + 5.8));
        float wobA = lerp(2, 12, Hash(sid + 6.1));
        float wobF = lerp(1.2, 4.5, Hash(sid + 7.7));
        float drift = lerp(-8, 16, Hash(sid + 8.3));
        float burst = lerp(0.12, 0.45, Hash(sid + 9.2));
        // The age a bubble here would have: z = z0 + v a + push a^2 / 2.
        float dz = b.y - z0;
        if (dz < -20) {
            continue;
        }
        float a = (-v + sqrt(v * v + 2 * push * max(dz, 0))) / push;
        float k0 = floor((Time - a) / gap);
        for (int m = -1; m <= 1; m++) {
            float e = k0 + m;
            float2 kid = float2(e, slot * 7.1 + seed * 3.3);
            if (Hash(kid + 0.7) < 0.22) {
                continue;
            }
            float te = (e + 0.9 * (Hash(kid) - 0.5)) * gap;
            float age = Time - te;
            float gate = Noise(float2(te * burst + slot * 3.3, seed * 5.1 + slot));
            float life = lerp(3, 14, Hash(kid + 2.2));
            if (age < 0 || gate < (k.x > 2 ? 0.30 : 0.46) || age > life) {
                continue;
            }
            float z = z0 + v * age + push * age * age * 0.5;
            float x = x0 + drift * age + sin(age * wobF + Hash(kid + 4.4) * 6.28) * wobA * min(age, 1.5)
                + (Hash(kid + 5.5) - 0.5) * k.z * (0.4 + 0.6 * Hash(kid + 6.6));
            float r = r0 * lerp(0.55, 1.6, Hash(kid + 3.9)) * (1 + grow * age);
            float on = smoothstep(0, 0.3, age) * smoothstep(life, life - 0.4, age);
            acc = max(acc, Bubble(float2(b.x - x, b.y - z), r, k.w) * on);
            // A burst's peak: a puff of small ones around it.
            if (gate > 0.72) {
                for (int q = 0; q < 2; q++) {
                    float2 pid = kid + 11.1 * (q + 1);
                    float2 off = (Hash2(pid) - 0.5) * float2(34, 40) * (1 + age * 0.15);
                    float rq = r * lerp(0.35, 0.7, Hash(pid + 1.9));
                    acc = max(acc, Bubble(float2(b.x - x, b.y - z) - off, rq, k.w) * on);
                }
            }
        }
    }
    return saturate(acc);
}

float Splashes(float3 C, float3 d, float tP, float level, float px);
float SplashFoam(float2 p, float fp);

float4 Water(ScreenIn i) : SV_Target {
    float level = Level();
    float front = Params[0].w;
    float3 C = CameraOrigin.xyz;
    float3 P = SceneWorld(i.pos.xy, i.uv);
    float3 ray = P - C;
    float tP = min(length(ray), 300000);
    float3 d = normalize(ray);
    PixAngle = max(length(ddx(d)), length(ddy(d)));
    Flat = abs(d.z);
    P = C + d * tP;
    float2 fpP = FootAt(tP);
    float hP = Surface(P, fpP);

    // Where the view enters the water: the front face, else the top.
    float tIn = 1e9;
    int via = 0;
    float tf = (front - C.x) / min(d.x, -1e-5);
    float3 F = C + d * tf;
    float hF = Surface(F, FootAt(tf));
    if (tf > 0 && F.z < hF) {
        tIn = tf;
        via = 1;
    } else if (d.z < -1e-5) {
        float tt = (level - C.z) / d.z;
        float3 T = C + d * tt;
        // One refinement onto the waves.
        tt = (Surface(T, FootAt(tt)) - C.z) / d.z;
        tIn = max(tt, tf);
        via = 2;
    }
    // A point under the surface in front of the front face is in water too.
    if (P.z < hP && tIn >= tP) {
        tIn = tP;
        via = 1;
    }
    bool under = tIn < tP + 0.5;
    float3 E = C + d * min(tIn, tP);
    float3 n = SceneNormal(i.pos.xy);
    // The footprint on the scene's face, stretched as the face turns away.
    float fpS = fpP.x / max(dot(n, n) > 0 ? abs(dot(d, n)) : 1, 0.08);

    float3 col;
    if (!under) {
        col = SceneColor(i.uv);
        // Ripple light thrown up onto faces just over the water.
        float above = P.z - hP;
        if (above > 0 && above < 160 && dot(n, n) > 0) {
            float2 c = Cells(float2(P.y / 55, P.z / 22 - Time * 0.6), Time * 1.4);
            float rip = saturate(1 - (c.y - c.x) / 0.10) * Keep(float2(55, 22), fpS);
            col += col * CAUSTIC * rip * saturate(n.x) * 0.35 * (1 - above / 160) * Risen();
        }
    } else {
        float path = min(tP - tIn, 40000);
        float depth = max(hP - P.z, 0);
        // Refraction: the wave slope where the view enters bends what is behind, more the more water is between.
        float2 fpE = FootAt(min(tIn, tP));
        float2 s = Slope(E.yx, fpE);
        float2 fine = (float2(Noise(P.yz / 30 + Time * 1.2), Noise(P.yz / 30 - Time * 1.1 + 9.1)) - 0.5)
            * Keep(float2(30, 30), fpP.xx);
        float bend = saturate(path / 400) * 0.6 + 0.4;
        float2 uv = i.uv + (float2(-s.x, s.y) * 0.006 * (via == 2 ? 1 : 0.4) + fine * 0.0035) * bend;
        float3 scene = SceneColor(uv);

        // Caustics: a drifting net of light, strongest on faces turned up, fading with depth.
        float3 caus = 0;
        if (dot(n, n) > 0) {
            float up = saturate(n.z);
            float2 pf = P.xy / 120 + float2(Noise(P.xy / 300 + Time * 0.2), Noise(P.xy / 300 - Time * 0.17)) * 1.2;
            float2 c1 = Cells(pf, Time * 1.3);
            float2 c2 = Cells(pf * 1.7 + 4.3, -Time * 1.1);
            float net = saturate(1 - (c1.y - c1.x) / 0.08) + 0.6 * saturate(1 - (c2.y - c2.x) / 0.07);
            float2 ps = float2(P.y, P.z) / 95;
            float2 c3 = Cells(ps + Noise(ps * 0.4 + Time * 0.3), Time * 1.5);
            float side = saturate(1 - (c3.y - c3.x) / 0.10);
            float k = (net * up + side * (1 - up) * 0.45) * exp(-depth / 450) * smoothstep(0, 15, depth)
                * Keep(float2(60, 60), fpS.xx);
            caus = CAUSTIC * k;
        }
        float3 tint = lerp(1, TRANSMIT, saturate(depth / 260 + path / 3000));
        float3 lit = scene * tint * (1 + caus * 0.9) + caus * 0.02;
        // A push of saturation: tinted pale stone reads turquoise, not grey.
        float ll = dot(lit, float3(0.3, 0.59, 0.11));
        lit = max(ll + (lit - ll) * lerp(1, 1.45, saturate(depth / 120)), 0);

        // Haze: by the path in water and the depth under the surface.
        float hazePath = 1 - exp(-path / Params[1].z);
        float hazeDepth = 1 - exp(-depth / Params[1].w);
        float haze = saturate(hazePath * 0.55 + hazeDepth * 0.75);
        float3 water = lerp(SHALLOW, DEEP, saturate(hazeDepth * 1.1));
        col = lerp(lit, water, haze * lerp(0.88, 1, hazePath * hazeDepth));

        // Light shafts slanting down from the surface.
        float sx = (P.y + depth * 0.32) / 210 + Time * 0.06;
        float sh = Noise(float2(sx, Time * 0.12)) * Noise(float2(sx * 2.3 + 3.1, Time * 0.2));
        float shaft = smoothstep(0.25, 0.6, sh) * exp(-depth / 700) * smoothstep(0, 40, depth);
        col += SHEEN * shaft * 0.10;

        // The top seen from above: clear near, a turquoise sheen growing far off, long wave bands and few glints.
        if (via == 2) {
            float behind = front - E.x;
            float far = smoothstep(300, 9000, behind);
            // Slow broad swells, kept only while they are wider than the footprint.
            float band = Noise(float2(E.y / 1400 + Time * 0.04, E.x / 700 - Time * 0.12));
            band = lerp(0.5, band, Keep(float2(1400, 700), fpE));
            float swell = Noise(float2(E.y / 420 - Time * 0.10, E.x / 260 + Time * 0.25));
            swell = lerp(0.5, swell, Keep(float2(420, 260), fpE));
            float3 body = lerp(SHALLOW * 1.15, SHEEN * 0.75, saturate(0.15 + 0.45 * band + 0.35 * swell));
            float3 nrm = normalize(float3(-s.y * 1.6, -s.x * 1.6, 1));
            float3 r = reflect(d, nrm);
            r.z = abs(r.z);
            float3 sky = lerp(SKY_LOW, SKY_HIGH, saturate(r.z * 2.5));
            float cosv = saturate(-dot(d, nrm));
            float fres = 0.02 + 0.98 * pow(1 - cosv, 5);
            float3 top = lerp(body, sky * 0.8, saturate(fres * 0.42));
            col = lerp(col, top, 0.12 + 0.72 * far);
            // The sun's glitter path: a soft column on the water where the reflection points at a sun low ahead,
            // broken by the filtered waves near and smooth far off, so it smears instead of sparkling.
            float across = (r.y / max(-r.x, 0.2) - 0.035) / 0.035;
            float up = (r.z - 0.045) / 0.05;
            float path = exp(-across * across) * exp(-up * up);
            float sheenLine = 0.6 + 0.4 * swell;
            col += SUN * path * sheenLine * far * 0.45;
            col = lerp(col, FOAM, SplashFoam(E.xy, fpE.y) * 0.85);
        }
    }

    // Foam: along the front face's edge (the waterline across the picture) and where the surface meets the stage.
    float below = hF - F.z;
    if (tf > 0 && tf < tP + 1) {
        float bits = Noise(float2(F.y / 16 - Time * 0.9, Time * 0.6));
        float lip = smoothstep(-1.5, 0.3, below) * smoothstep(4.5 + 3 * bits, 2, below);
        float froth = smoothstep(-1, 1, below) * smoothstep(14 * bits + 3, 0, below)
            * step(0.5, Noise(F.yz / 7 + float2(-Time * 1.4, Time * 0.3)));
        col = lerp(col, FOAM, saturate(lip + froth * 0.6));
        // A band of light just under the line.
        col += SHEEN * 0.30 * smoothstep(0, 5, below) * exp(-max(below, 0) / 22);
    }
    float meet = hP - P.z;
    if (P.x < front && tP < 100000) {
        float bits = lerp(0.5, Noise(float2(P.y / 14 + Time * 0.7, P.x / 14)), Keep(float2(14, 14), fpP.xx));
        float wide = max(4 + 5 * bits, fpS * 2.5);
        float ring = smoothstep(-max(2, fpS), 0, meet) * smoothstep(wide, 0, meet);
        col = lerp(col, FOAM, ring * 0.8 * lerp(0.5, 1, Keep(float2(20, 20), fpS.xx)));
    }

    // Bubbles on three sheets in the water (near, middle, far: smaller, softer and fainter with distance), under
    // the surface and popping at it. Each holds a few streams and a sparse scatter of loose ones.
    float3 sheet[3] = { float3(front - 40, 1, 1), float3(-550, 0.85, 0.75), float3(-1700, 0.7, 0.5) };
    for (int k = 0; k < 3; k++) {
        float tb = (sheet[k].x - C.x) / min(d.x, -1e-5);
        if (tb > 0 && tb < tP) {
            float3 B = C + d * tb;
            float hb = Surface(B, FootAt(tb));
            if (B.z < hb - 3) {
                float sz = sheet[k].y;
                float soft = max(0.8 + 0.8 * k, FootAt(tb).x);
                float2 bp = float2(B.y, B.z);
                float3 bb = Streams(bp, k + 1, 750 + 150 * k, 0.65, float4(1, sz, 22, soft));
                bb = max(bb, Streams(bp + float2(0, 37), k + 7, 220, 0.55, float4(4, sz * 0.9, 150, soft)));
                float fade = smoothstep(hb - 3, hb - 25, B.z) * exp(-max(hb - B.z, 0) / 1100) * sheet[k].z;
                col = lerp(col, col * 1.25 + SHEEN * 0.12, bb.x * fade * 0.5);
                col = lerp(col, FOAM, (bb.y * 0.7 + bb.z) * fade);
            }
        }
    }

    // Splashes where fighters cross the surface.
    col = lerp(col, FOAM, Splashes(C, d, tP, level, FootAt(max(-C.x / min(d.x, -1e-5), 1)).x));
    return float4(col, 1);
}

// The splashes' white (0..1) on the ray from C along d to the scene at tP: each a crown of drops thrown up from the
// fighters' plane (UE X 0) and falling back, and a mound of white water bursting up and collapsing, on the waves
// there (the foam on the top is SplashFoam's). Drops are hashed afresh per splash (its seed), so no two look alike.
// px: a pixel's size (cm) on the plane.
float Splashes(float3 C, float3 d, float tP, float level, float px) {
    float s = -C.x / min(d.x, -1e-5);
    if (!Game() || s <= 0 || s > tP + 60) {
        return 0;
    }
    float3 Q = C + d * s;
    // (from the waves there, so the white sits on them)
    float up = Q.z - level - Wave(Q.yx, FootAt(s));
    float white = 0;
    for (int k = 0; k < 4; k++) {
        float4 sp = Params[4 + k];
        float age = sp.y;
        float dy = Q.y - sp.x;
        if (age < 0 || age > 1.6 || abs(dy) > 520 || up < -30 || up > 460) {
            continue;
        }
        float str = sp.z;
        // The mound: up then down in about 0.6 s, its top broken by noise.
        float wid = 28 + 40 * str + 160 * age;
        float hgt = (40 + 120 * str) * sin(saturate(age / 0.65) * 3.14159) * saturate(1 - dy * dy / (wid * wid));
        hgt *= 0.7 + 0.5 * Noise(float2(dy / 14 + sp.w * 31, age * 6));
        // (none where it is only a few cm high: its sides and its end are no line along the surface)
        float mound = smoothstep(max(px, 2), -max(px, 2), up - hgt) * smoothstep(-12, 0, up)
            * smoothstep(4, 14, hgt);
        // Frothy, not solid: clots of white over a thinner body, torn more toward its top.
        float froth = Fbm(float2(dy / 22 + sp.w * 13, (up - age * 90) / 22), 3);
        mound *= lerp(0.35, 1, smoothstep(0.35 + 0.25 * saturate(up / max(hgt, 1)), 0.7, froth));
        // The crown's drops.
        float drops = 0;
        for (int j = 0; j < 14; j++) {
            float2 id = float2(j * 1.37 + k * 9.1, sp.w * 57.3);
            float2 h = Hash2(id);
            float vy = (h.x - 0.5) * 2 * (110 + 260 * str);
            float vz = (240 + 420 * h.y) * (0.55 + 0.55 * str) * (1 - 0.6 * abs(h.x - 0.5));
            float t = age * (0.85 + 0.3 * Hash(id + 3.3)) - 0.05 * Hash(id + 4.4);
            float2 at = float2(vy * t, vz * t - 490 * t * t);
            if (t <= 0 || at.y < -2) {
                continue;
            }
            float r = (2.5 + 4 * Hash(id + 5.1)) * (1 - 0.35 * age);
            float rr = max(r, px * 0.9);
            float dd = length(float2(dy, up) - at);
            drops = max(drops, saturate(1.5 - dd / rr) * min(r / rr, 1));
        }
        float fade = saturate((1.6 - age) / 0.5);
        white = max(white, max(mound * 0.9, drops) * fade);
    }
    return white;
}

// A splash's foam on the top seen from above at world (x, y) (UE): a broken ring spreading from where the fighter
// crossed, and white water inside it early on. fp: a pixel's size there (cm).
float SplashFoam(float2 p, float fp) {
    float foam = 0;
    for (int k = 0; k < 4; k++) {
        float4 sp = Params[4 + k];
        float age = sp.y;
        if (!Game() || age < 0 || age > 1.6) {
            continue;
        }
        float2 o = p - float2(0, sp.x);
        float r = length(o);
        float rad = (25 + 30 * sp.z) + 230 * sqrt(age);
        if (r > rad + 60) {
            continue;
        }
        float thick = max(6 + 10 * age, fp);
        float a = atan2(o.y, o.x);
        float torn = step(0.42, Noise(float2(a * 4 + sp.w * 23, age * 2)));
        float edge = saturate(1 - abs(r - rad) / thick) * torn;
        float inner = smoothstep(rad, rad * 0.4, r) * saturate(1 - age * 1.6)
            * smoothstep(0.4, 0.7, Fbm(o / 18 + sp.w * 7, 2));
        foam = max(foam, max(edge, inner) * saturate((1.6 - age) / 0.6) * lerp(0.6, 1, sp.z));
    }
    return saturate(foam);
}
