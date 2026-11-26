// Flood B, the deep storm flood: dark blue-green water surges up from under the stage and settles over the main
// floor, choppy, foam on the crests; below it everything is tinted and hazed by its depth, lit by dim caustics and
// shafts from the surface, silt drifting; above it a storm darkens the sky, rain falls and lightning flashes.
// All screen passes: the water is a height field over the world, z < H(x, y), cut at the front by the plane x = Xf
// (the "glass" its waterline is seen across, edge-on from the side camera).
// Params[0]: settled level Z (cm), start Z (cm), rise seconds, wave scale.
// Params[1]: front plane X (cm), first lightning (s), second lightning (s), rain strength.

static const float PI = 3.14159265;
static const float3 DEEP = float3(0.001, 0.008, 0.016);
static const float3 MURK = float3(0.003, 0.030, 0.045);
static const float3 SHAFT = float3(0.10, 0.34, 0.32);
static const float3 FOAM = float3(0.85, 0.95, 0.95);
static const float3 STORM_SKY = float3(0.06, 0.085, 0.11);
// Absorption per cm of the water's optical length: red goes first, then green; blue-green stays.
static const float3 SIGMA = float3(1.0 / 240, 1.0 / 560, 1.0 / 700);

float Level() {
    float x = saturate(Time / max(Params[0].z, 0.01));
    float e = 1 - (1 - x) * (1 - x) * (1 - x);
    // The surge overshoots a little as it arrives, then swells slowly.
    float surge = sin(x * PI) * 38 * x * x;
    return lerp(Params[0].y, Params[0].x, e) + surge + 5 * sin(Time * 0.8) * x;
}

// Wilder while it surges, settling to the wave scale.
float Amp() {
    float settle = saturate((Time - Params[0].z) / 2.0);
    return Params[0].w * lerp(1.7, 1.0, settle);
}

// A choppy wave: sharp crests, flat troughs (mean about 0).
float Crest(float ph) {
    float s = 0.5 + 0.5 * sin(ph);
    return pow(s, 2.6) - 0.27;
}

// A long swell: rounded crests, wide troughs (mean about 0).
float Swell(float ph) {
    float s = 0.5 + 0.5 * sin(ph);
    return pow(s, 1.7) - 0.36;
}

// How rough the sea is at `p` (0.25 calm .. 1.35 building): broad stretches that drift with the wind.
float Rough(float2 p) {
    float n = Fbm(p / 2600 + float2(0.05, -0.11) * Time, 2);
    return lerp(0.42, 1.35, smoothstep(0.30, 0.72, n));
}

// A rogue set: a short train of big swells travelling through along `dir`, its middle at `at` (cm along dir).
float Rogue(float2 p, float2 dir, float at, float width, float len) {
    float x = dot(p, dir) - at;
    // The train's crests bend a little across it, so it is not a straight ridge.
    float bend = 260 * (Noise(float2(dot(p, float2(-dir.y, dir.x)) / 1400, 3.7)) - 0.5);
    float env = exp(-(x + bend) * (x + bend) / (width * width));
    return env * Swell((x + bend) * (2 * PI / len));
}

// The waves' height (cm) at world `p`, before Amp: three swells of incommensurate lengths and turns, their phases
// warped by slow noise, scaled by the sea's roughness; a little chop on top; rogue sets passing through.
float Waves(float2 p) {
    float t = Time;
    float r = Rough(p);
    float2 warp = (float2(Noise(p / 1700 + 1.3), Noise(p / 1700 + 8.1)) - 0.5) * 2.4;
    float h = 0;
    h += 15 * Swell(dot(p, float2(0.21, -0.978)) * (2 * PI / 1730) - t * 1.33 + warp.x);
    h += 10 * Swell(dot(p, float2(-0.47, -0.883)) * (2 * PI / 1170) - t * 1.71 + 2.1 + warp.y);
    h += 6 * Swell(dot(p, float2(0.73, -0.683)) * (2 * PI / 787) - t * 2.27 + 4.6 + warp.x * 0.7);
    h *= r;
    float chop = 3 * Crest(dot(p, float2(-0.86, 0.51)) * (2 * PI / 331) - t * 3.4 + warp.y * 2);
    chop += 2 * Crest(dot(p, float2(0.39, 0.92)) * (2 * PI / 219) - t * 4.3 + 1.9);
    h += chop * r * r + 7 * (Noise(p / 430 + float2(t * 0.30, -t * 0.22)) - 0.5);
    // Two sets, every 23 s and 31 s, moving across the arena (one passes the camera about 4 s in).
    h += 36 * Rogue(p, float2(0.12, -0.993), fmod(t + 13.0, 23.0) * 420 - 5200, 1500, 1500);
    h += 26 * Rogue(p, float2(-0.30, 0.954), fmod(t + 4.0, 31.0) * 360 - 6100, 1200, 1250);
    return h;
}

float Height(float2 p) {
    return Level() + Amp() * Waves(p);
}

float3 SurfaceNormal(float2 p) {
    float e = 12;
    float h = Waves(p);
    float hx = Waves(p + float2(e, 0)) - h;
    float hy = Waves(p + float2(0, e)) - h;
    return normalize(float3(-hx * Amp() * 1.6, -hy * Amp() * 1.6, e));
}

// Foam: patchy, streaked white where the crest rises over `on` (0..1).
float Foam(float2 q, float on) {
    float n = Fbm(q, 3);
    return saturate(on * 1.6 - 0.25) * smoothstep(0.35, 0.65, n + on * 0.35);
}

// Above water: the storm. The sky darkens toward the top, everything cools and dims, lightning flashes.
float Lightning() {
    float f = 0;
    for (int k = 0; k < 2; k++) {
        float at = k == 0 ? Params[1].y : Params[1].z;
        float dt = Time - at;
        // A double flicker, then a fade.
        f += dt >= 0 ? (exp(-dt * 7) + 0.7 * exp(-abs(dt - 0.12) * 30)) : 0;
    }
    return f;
}

float4 Storm(ScreenIn i) : SV_Target {
    float3 col = SceneColor(i.uv);
    float3 P = SceneWorld(i.pos.xy, i.uv);
    float far = saturate((length(P - CameraOrigin.xyz) - 18000) / 10000);
    // Overall: cooler, dimmer, a little desaturated.
    float lum = dot(col, float3(0.3, 0.59, 0.11));
    col = lerp(col, lum * float3(0.75, 0.9, 1.1), 0.35) * 0.72;
    // The sky: toward a heavy storm grey-blue, darkest at the top of the picture.
    float top = 1 - smoothstep(0.0, 0.55, i.uv.y);
    float sky = max(far, 0.45 * top);
    col = lerp(col, STORM_SKY * (0.6 + 0.8 * lum) * lerp(1.0, 0.55, top), saturate(sky * 0.85));
    // Rolling clouds in the sky.
    float cloud = Fbm(float2(i.uv.x * 3 + Time * 0.05, i.uv.y * 5), 4);
    col *= lerp(1.0, 0.65 + 0.6 * cloud, far);
    float flash = Lightning();
    col = col * (1 + 1.1 * flash) + float3(0.5, 0.6, 0.75) * flash * (0.15 + 0.6 * far);
    return float4(col, 1);
}

// The water, seen along the ray from the camera to the scene's point.
float Ripples(float2 p, float thick);

float4 Flood(ScreenIn i) : SV_Target {
    float3 C = CameraOrigin.xyz;
    float3 P = SceneWorld(i.pos.xy, i.uv);
    float3 d = P - C;
    float dist = min(length(d), 200000);
    float3 dn = d / max(length(d), 1e-3);
    P = C + dn * dist;
    float3 scene = SceneColor(i.uv);

    float level = Level();
    float amp = Amp();
    float hmax = level + 75 * amp;
    float hmin = level - 30 * amp;
    float xf = Params[1].x;

    // Where the ray enters the water: across the front plane (below its wavy top), or down through the surface.
    float enter = -1;
    bool front = false;
    float sFront = (C.x > xf && dn.x < 0) ? (xf - C.x) / dn.x : 0;
    float rim = 1e9;
    if (sFront > 0 && sFront < dist) {
        float3 Q = C + dn * sFront;
        float hq = Height(Q.xy);
        if (Q.z < hq) {
            enter = sFront;
            front = true;
            rim = hq - Q.z;
        }
    }
    float3 n = float3(0, 0, 1);
    if (!front && dn.z < 0) {
        float s0 = max(sFront, (C.z - hmax) / -dn.z);
        float s1 = min(dist, (C.z - hmin) / -dn.z);
        if (s0 < s1) {
            float prev = s0;
            float stride = (s1 - s0) / 40;
            [loop] for (int k = 0; k <= 40; k++) {
                float s = s0 + stride * k;
                float3 R = C + dn * s;
                if (R.z < Height(R.xy)) {
                    float lo = prev, hi = s;
                    for (int b = 0; b < 6; b++) {
                        float m = (lo + hi) * 0.5;
                        float3 M = C + dn * m;
                        if (M.z < Height(M.xy)) {
                            hi = m;
                        } else {
                            lo = m;
                        }
                    }
                    enter = hi;
                    break;
                }
                prev = s;
            }
        }
        // A point under the surface the march stepped over (the ray grazing a trough).
        if (enter < 0 && P.z < Height(P.xy) && dist < 100000) {
            enter = dist;
        }
    }
    if (enter < 0) {
        // Spray over the waterline where a big crest breaks: specks flung up and falling back, on the front plane.
        if (sFront > 0 && sFront < dist) {
            float3 Q = C + dn * sFront;
            float hw = Waves(Q.xy);
            float up = Q.z - (level + amp * hw);
            float brk = smoothstep(17, 27, hw) * (0.5 + 0.5 * Noise(float2(Q.y / 500, Time * 0.7)));
            float tall = 40 + 110 * brk;
            if (up > 0 && up < tall && brk > 0.01) {
                float3 spray = 0;
                for (int k = 0; k < 2; k++) {
                    float size = 22 + 12 * k;
                    float2 q = float2(Q.y - Time * 140, up + Time * (60 - 140 * frac(Time * 0.6 + k * 0.5))) / size + 5.1 * k;
                    float2 cell = floor(q);
                    float pick = Hash(cell + 1.7 * k);
                    float2 at = 0.2 + 0.6 * Hash2(cell);
                    float rr = length((frac(q) - at) * size);
                    spray += saturate(1 - rr / (3 + 4 * pick)) * step(0.45, pick);
                }
                float fade = brk * saturate(1 - up / tall);
                scene = lerp(scene, FOAM * (0.6 + 0.5 * Lightning()), saturate(spray.x * fade * 1.2));
                // A soft mist over the break.
                scene = lerp(scene, FOAM * 0.45, 0.30 * brk * saturate(1 - up / (tall * 0.7)));
            }
        }
        return float4(scene, 1);
    }

    float3 E = C + dn * enter;
    float hp = Height(P.xy);
    float below = max(hp - P.z, 0);
    float path = max(dist - enter, 0);
    bool surface = !front && path > 0.5;

    // Refraction: the scene below wobbles by the waves (the surface's slope, or a current behind the front plane).
    float2 wob;
    if (surface) {
        n = SurfaceNormal(E.xy);
        float calm = saturate((enter - 3500) / 9000);
        n = normalize(lerp(n, float3(0, 0, 1), calm * 0.7));
        wob = n.xy * float2(0.020, 0.030);
    } else {
        float2 q = float2(P.y / 140 + Time * 0.6, P.z / 110 - Time * 0.9);
        wob = (float2(Noise(q), Noise(q + 5.3)) - 0.5) * 0.010;
    }
    wob *= saturate(path / 300 + below / 120);
    float2 ruv = i.uv + wob * float2(Screen.y * Screen.z, 1);
    float3 under = SceneColor(ruv);
    // Graded under the water: less colour, dimmer with depth (the light is taken from above).
    float ul = dot(under, float3(0.3, 0.59, 0.11));
    under = lerp(under, ul * float3(0.55, 1.0, 1.05), 0.35) * lerp(0.52, 0.36, saturate(below / 900));

    // Caustics on what faces up under the water: a dim moving net.
    float3 N = SceneNormal(i.pos.xy);
    if (N.z > 0.25 && below > 0) {
        float2 cq = P.xy / 150 + (float2(Noise(P.xy / 300 + Time * 0.2), Noise(P.xy / 300 - Time * 0.2)) - 0.5) * 1.6;
        float2 c = Cells(cq, Time * 1.7);
        float caus = smoothstep(0.22, 0.02, c.y - c.x);
        under *= 1 + caus * 1.6 * N.z * exp(-below / 700) * saturate(below / 25);
    }

    // The optical length: mostly the depth below the surface (the side view reads water by its depth), some of the path.
    float opt = below * 1.1 + path * 0.22;
    // (never quite opaque: a fighter knocked deep stays a dim shape, not only its outline)
    float3 trans = max(exp(-opt * SIGMA), float3(0.10, 0.24, 0.28) * exp(-below / 2500));
    float3 murk = lerp(MURK, DEEP, saturate(below / 1500));
    float3 col = under * trans + murk * (1 - trans);

    // Shafts of light down from the surface: slanted bands, strongest near the top, swaying.
    float3 shafts = 0;
    float span = min(path, 2500);
    [loop] for (int k = 0; k < 6; k++) {
        float3 R = E + dn * (span * (k + 0.5) / 6);
        float dz = max(level - R.z, 0);
        float band = Noise(float2((R.y + R.z * 0.45) / 230 + 0.4 * sin(Time * 0.35), Time * 0.15));
        band = pow(saturate(band * 1.25 - 0.15), 4);
        shafts += band * exp(-dz / 420) * saturate(dz / 25);
    }
    col += SHAFT * shafts / 6 * saturate(span / 400 + below / 200) * 0.9;

    // Silt drifting in the water: specks on three sheets across the view.
    float3 silt = 0;
    [loop] for (int k = 0; k < 3; k++) {
        float x = xf - 120 - k * 520;
        float s = (x - C.x) / dn.x;
        if (s > enter && s < dist) {
            float3 Q = C + dn * s;
            float dq = Height(Q.xy) - Q.z;
            float size = 80 + 35 * k;
            float2 q = float2(Q.y + Time * (14 + 6 * k), Q.z - Time * 7 + 9 * sin(Time * 0.7 + k)) / size + 7.3 * k;
            float2 cell = floor(q);
            float pick = Hash(cell + 3.1);
            float2 at = 0.2 + 0.6 * Hash2(cell);
            float r = length((frac(q) - at) * size);
            float dot1 = saturate(1 - r / (3.5 + 3 * pick)) * step(0.82, pick);
            silt += dot1 * saturate(dq / 30) * exp(-(s - enter) / 1800);
        }
    }
    col += float3(0.35, 0.55, 0.52) * silt * 0.3;

    // The top: the surface seen from above (gloss, sky, foam), and the rim of the front plane's waterline.
    float wv = Waves(E.xy);
    float crest = saturate((wv - 15) / 14) * (1 - saturate((enter - 3000) / 7000) * 0.5);
    if (surface) {
        float3 r = reflect(dn, n);
        // The water's body: dark teal, its faces toward the storm light a lighter green, the rest deep (toon-banded
        // a little, as the game's light is).
        float3 toLight = normalize(float3(-1, 0.25, 0.32));
        float lit = saturate(dot(n, normalize(float3(-0.6, 0.3, 0.75))) * 1.4 - 0.55);
        lit = floor(lit * 3 + 0.5) / 3;
        float3 body = lerp(float3(0.004, 0.030, 0.040), float3(0.020, 0.115, 0.115), lit);
        col = lerp(col, body, 0.75);
        float3 sky = STORM_SKY * 0.9;
        float2 suv = ScreenUV(E - C + r * 1800);
        if (all(suv > 0.0) && all(suv < 1.0)) {
            sky = lerp(sky, SceneColor(suv) * 0.4, 0.25);
        }
        float fres = lerp(0.08, 0.35, pow(1 - saturate(dot(n, -dn) * 4), 2));
        col = lerp(col, sky, fres);
        float spec = pow(saturate(dot(r, toLight)), 120);
        float near = 1 - saturate((enter - 2500) / 6000);
        col += float3(0.7, 0.85, 0.9) * spec * (0.5 * near + 1.0 * Lightning());
        // Foam as wind streaks: long thin lines along the wind (sim -y), thickest on the big crests, thin in the
        // troughs behind them, absent where the sea is calm.
        float2 wq = float2(E.y / 900 + Time * 0.12, E.x / 38);
        float streak = smoothstep(0.58, 0.78, Fbm(wq + float2(0, Noise(E.xy / 1200) * 3), 3));
        float rough = smoothstep(0.7, 1.2, Rough(E.xy));
        float f = max(crest * smoothstep(0.35, 0.6, Fbm(E.xy / float2(160, 45) + Time * 0.2, 3)), streak * rough * 0.45);
        col = lerp(col, FOAM * (0.55 + 0.5 * Lightning()), f * 0.9);
        // Rain rings on the surface (thicker far off, where they are packed into a fine shimmer).
        float rip = Ripples(E.xy, 2.0 + enter * 0.0006) * Params[1].w;
        col = lerp(col, FOAM * 0.6, saturate(rip * 0.5));
    }
    // The waterline: where the water meets a thing (the scene's point just under the surface), a foamy lip.
    float lip = surface ? saturate(1 - below / 30) * saturate(1 - path / 500) : 0;
    // The front plane's wavy top: a bright edge, frothy on its crests.
    float edge = front ? saturate(1 - rim / 12) : 0;
    // Whitecaps hang down from the crests of the line, broken into clots.
    float capOn = saturate((Waves(E.xy) - 17) / 12);
    float froth = front ? Foam(float2(E.y / 40 + Time * 0.5, rim / 25 + Time * 0.3), capOn) * saturate(1 - rim / (25 + 60 * capOn)) : 0;
    float white = max(max(lip * 0.8, edge * 0.85), froth);
    col = lerp(col, FOAM * (0.6 + 0.5 * Lightning()), white);
    // A cool sheen just under the line, where light comes through the surface.
    float glow = front ? exp(-rim / 40) : exp(-below / 40) * saturate(1 - path / 900);
    col += float3(0.03, 0.12, 0.12) * glow;
    // The lightning lights the water too.
    col += murk * 4 * Lightning() * exp(-below / 600);
    return float4(col, 1);
}

// Rain in the world: drops fall on vertical sheets at many depths across the view, each sheet drawn only where it
// is in front of the scene and above the water, so a streak ends where it meets the surface (or a thing) and none
// fall under the water. Where a drop lands on the water a small crown of droplets jumps up. Every drop's and splash's
// place and timing is hashed afresh per fall, so nothing repeats on a grid or a cycle.
float4 Rain(ScreenIn i) : SV_Target {
    float3 col = SceneColor(i.uv);
    float3 C = CameraOrigin.xyz;
    float3 P = SceneWorld(i.pos.xy, i.uv);
    float3 d = P - C;
    float dist = length(d);
    float3 dn = d / max(dist, 1e-3);
    float level = Level();
    float amp = Amp();
    float rain = 0;
    float splash = 0;
    [loop] for (int k = 0; k < 12; k++) {
        // Sheets from near the camera to behind the stage, their spacing jittered.
        // (none in front of the front plane: there the camera looks down past a sheet onto the water body)
        float x = Params[1].x - (k == 0 ? 0 : k * 330 + 140 * Hash(float2(k, 1.7)));
        float s = (x - C.x) / dn.x;
        if (dn.x >= 0 || s <= 0 || s >= dist) {
            continue;
        }
        float3 Q = C + dn * s;
        float hq = level + amp * Waves(Q.xy);
        float above = Q.z - hq;
        if (above < -4) {
            continue;
        }
        // A streak is about a pixel wide whatever its distance.
        float px = s * 0.00055;
        // The drops: cells in (slanted y, z rising with the fall), one drop or none per cell, its place random.
        float speed = 1500 + 200 * Hash(float2(k, 4.2));
        float cw = 34 + 3 * k;
        float ch = 420;
        float u = Q.y + Q.z * 0.24 + 37 * k;
        float v = Q.z + Time * speed + 911 * k;
        float2 cell = float2(floor(u / cw), floor(v / ch));
        float pick = Hash(cell + 0.37 * k);
        float u0 = (cell.x + 0.15 + 0.7 * Hash(cell + 5.1)) * cw;
        float v0 = (cell.y + 0.8 * Hash(cell + 9.3)) * ch;
        float len = 70 + 70 * Hash(cell + 2.2);
        float across = saturate(1 - abs(u - u0) / max(px * 1.2, 0.8));
        float along = v - v0;
        float streak = across * smoothstep(0, len * 0.25, along) * smoothstep(len, len * 0.4, along);
        // (the streak's bottom fades in the last few cm above the water: it ends on the surface)
        rain += streak * step(pick, 0.27) * saturate(above / 6) * lerp(1.0, 0.55, k / 11.0);

        // Splashes where drops land: cells along the sheet's waterline; each cell fires at random moments, at a
        // random place, the near sheets sparse, the far ones dense.
        if (above < 45) {
            float sw = lerp(260, 60, k / 11.0);
            float c = floor((Q.y + 53 * k) / sw);
            float rate = 1.4 + 1.6 * Hash(float2(c, k + 0.5));
            float tt = Time * rate + Hash(float2(c, k + 7.7));
            float key = floor(tt);
            float ph = frac(tt);
            float2 id = float2(c * 1.31 + key * 7.13, k + key * 0.173);
            if (Hash(id) < 0.7) {
                float y0 = (c + 0.1 + 0.8 * Hash(id + 3.3)) * sw - 53 * k;
                float dy = Q.y - y0;
                float size = lerp(1.0, 0.65, k / 11.0);
                float dotr = max(px * 1.1, 1.6) * size;
                float crown = 0;
                for (int j = 0; j < 4; j++) {
                    float jr = Hash(id + j * 1.7);
                    float vx = (j - 1.5) * (14 + 14 * jr) * size;
                    float vz = (34 + 30 * jr) * size;
                    float2 at = float2(vx * ph, vz * ph - 85 * size * ph * ph);
                    float r = length(float2(dy, above) - at);
                    crown += saturate(1 - r / dotr);
                }
                // A white flick on the surface where it hit, spreading and fading.
                float flick = saturate(1 - abs(dy) / (4 + 24 * ph * size)) * saturate(1 - above / max(px * 1.5, 2));
                splash += (crown * (1 - ph * 0.7) + flick * (1 - ph) * 0.6) * step(-1, above);
            }
        }
    }
    float3 tint = float3(0.55, 0.62, 0.70) * (1 + 2 * Lightning());
    col += tint * rain * 0.45 * Params[1].w;
    col = lerp(col, FOAM * 0.7 * (1 + Lightning()), saturate(splash * 0.75 * Params[1].w));
    return float4(col, 1);
}

// Rings where drops land on the surface seen from above (the Flood pass's), random in place and time.
float Ripples(float2 p, float thick) {
    float sum = 0;
    for (int l = 0; l < 2; l++) {
        float size = l == 0 ? 150 : 97;
        float2 q = p / size + l * 0.5;
        float2 c = floor(q);
        float rate = 0.9 + 1.2 * Hash(c + l * 3.1);
        float tt = Time * rate + Hash(c + 7.7 + l);
        float key = floor(tt);
        float ph = frac(tt);
        float2 id = c + float2(key * 7.13, key * 3.71 + l);
        if (Hash(id) < 0.55) {
            float2 at = (c + 0.25 + 0.5 * Hash2(id + 1.3)) * size - l * 0.5 * size;
            float rad = 4 + 30 * ph;
            float dd = length(p - at);
            sum += saturate(1 - abs(dd - rad) / thick) * (1 - ph) * (1 - ph);
        }
    }
    return sum;
}
