//! One emitter's particles as vertex data (sortSprites, cameraOffset,
//! the pushes of meshes and ribbons, the values in the build's vertex structs).
//!
//! The layouts, little-endian f32 unless said:
//! - sprite (FParticleSpriteVertex, then FParticleVertexDynamicParameter), 80 bytes: Position xyz, RelativeTime,
//!   OldPosition xyz, ParticleId, Size xy, Rotation, SubImageIndex, Color rgba, DynamicValue xyzw; one per particle.
//! - mesh (FMeshParticleInstanceVertex, then its dynamic parameter), 112 bytes: Color rgba, Transform[3] (the
//!   columns of the 3x4, each with its translation), Velocity (direction, length), SubUVParams (i16 x 4), SubUVLerp,
//!   RelativeTime, DynamicValue xyzw; one per instance.
//! - ribbon (FParticleBeamTrailVertex, then the dynamic parameter), 96 bytes: Position xyz, RelativeTime,
//!   OldPosition xyz, ParticleId, Size xy, Rotation, SubImageIndex, Color rgba, Tex_U, Tex_V, Tex_U2, Tex_V2,
//!   DynamicValue xyzw; one triangle strip, the trails joined by repeated vertices.
//!
//! Every position is UE world (cm). Deviation: the build keeps a local-space sprite or mesh emitter's positions in
//! its space under LocalToWorld (the proxy's primitive buffer); here they are baked into the world
//! and `local_to_world` stays None. Deviation: the build streams the dynamic parameter apart; here it rides in the
//! same vertex.

use mvs_math::hypot::hypot3;
use mvs_math::js::{fround, max, min};
use mvs_math::libm::{cos, sin};

use crate::distribution::sample1;
use crate::draw::{DrawParams, SortMode};
use crate::emitter::{BeamPayload, EmitterInstance, Particle, R_DEAD, R_END, R_START};
use crate::math::{UNIT_X, UNIT_Z, V3, compose_rows, facing_rows, rotation_rows, safe_normal};
use crate::random::RandomSource;
use crate::system::SysState;
use crate::template::{Align, BeamNoise, EmitterTemplate};

pub const SPRITE_STRIDE: u32 = 80;
pub const MESH_STRIDE: u32 = 112;
pub const RIBBON_STRIDE: u32 = 96;

/// Where the draw looks from (UE world): the eye and the view's forward axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub origin: V3,
    pub forward: V3,
}

/// Drawing between the last two ticks, the port's own as the fighters' (the build draws the latest tick): a sprite
/// or mesh particle steps back from its latest place toward its previous one by 1 - `alpha`, and a local-space
/// emitter's particles also ride back on their component from `from` (where it was before the latest tick).
/// Ribbons show the latest tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    /// 1 draws the latest tick.
    pub alpha: f64,
    pub from: Option<V3>,
}

impl Blend {
    pub const LATEST: Blend = Blend { alpha: 1.0, from: None };
}

/// Centimetres in one tick: a longer component step is a placement, not a movement, and is not drawn between.
const BLEND_MAX_STEP: f64 = 600.0;

/// The step back from `pos` toward `old` (both world) for this frame.
fn between(t: &EmitterTemplate, s: &SysState, b: &Blend, pos: V3, old: V3) -> V3 {
    let k = 1.0 - b.alpha;
    let mut d = [(old[0] - pos[0]) * k, (old[1] - pos[1]) * k, (old[2] - pos[2]) * k];
    if let (true, Some(f)) = (t.local, b.from) {
        let m = [f[0] - s.loc[0], f[1] - s.loc[1], f[2] - s.loc[2]];
        if m[0].abs() + m[1].abs() + m[2].abs() < BLEND_MAX_STEP {
            for c in 0..3 {
                d[c] += m[c] * k;
            }
        }
    }
    d
}

fn put(v: &mut Vec<u8>, x: f64) {
    v.extend_from_slice(&(x as f32).to_le_bytes());
}

fn put3(v: &mut Vec<u8>, a: V3) {
    for x in a {
        put(v, x);
    }
}

fn put4(v: &mut Vec<u8>, a: [f64; 4]) {
    for x in a {
        put(v, x);
    }
}

fn f32s(a: V3) -> [f32; 3] {
    a.map(|x| x as f32)
}

/// The emitter's uniform buffer values.
pub fn draw_params(t: &EmitterTemplate, s: &SysState, section: Option<u32>) -> DrawParams {
    let velocity = t.align == Align::Velocity;
    let mut p = DrawParams {
        pivot: [t.pivot[0] as f32, t.pivot[1] as f32],
        sub_images: [t.sub_h as f32, t.sub_v as f32],
        velocity_aligned: velocity,
        rotate_lock: t.rotate_axis != 0,
        axis_locked: t.lock.is_some(),
        mesh_section: section,
        mirrored: s.scale[0] * s.scale[1] * s.scale[2] < 0.0,
        // 142c64010: RotationScale 1 and RotationBias 0, except RotationScale 0 for PSA_Velocity and RotationBias
        // pi / 2 for EPAL_ROTATE_Z.
        rotation_scale: if velocity { 0.0 } else { 1.0 },
        rotation_bias: if !velocity && t.rotate_z { std::f32::consts::FRAC_PI_2 } else { 0.0 },
        ..DrawParams::default()
    };
    if let Some(l) = t.lock {
        p.lock_up = f32s(s.axis(t.local, l[0]));
        p.lock_right = f32s(s.axis(t.local, l[1]));
    } else if t.rotate_axis != 0 {
        p.lock_right = f32s(s.axis(t.local, t.rotate_axis));
    }
    p
}

/// CameraOffset in the sprite and mesh fills (142c50180, 142c4f600), in simulation space: Offset along the
/// normalised direction from the particle to the camera (through the inverse LocalToWorld for a local emitter),
/// x 1048576 when the camera is within Offset; returned as a world vector.
fn camera_offset(t: &EmitterTemplate, s: &SysState, from: V3, cam: V3, off: f64) -> V3 {
    let mut c = cam;
    if t.local {
        c = s.inverse_transform_vector([cam[0] - s.loc[0], cam[1] - s.loc[1], cam[2] - s.loc[2]]);
    }
    let d = [c[0] - from[0], c[1] - from[1], c[2] - from[2]];
    let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let mut f = (if l2 > 1e-8 { 1.0 / l2.sqrt() } else { 1.0 }) * off;
    if l2 <= off * off {
        f *= 1048576.0;
    }
    let w = [d[0] * f, d[1] * f, d[2] * f];
    if t.local { s.transform_vector(w) } else { w }
}

/// SortSpriteParticles (142c58dc0): ViewProjDepth by the depth along the view, DistanceToView by the squared
/// distance, Age_OldestFirst by the Flags counter, Age_NewestFirst by its complement; descending, ties in index
/// order. False: no sort, the particles' own order; true: `sort.idx` holds the places in `em.particles` in draw
/// order.
fn sort_sprites(t: &EmitterTemplate, s: &SysState, em: &EmitterInstance, view: &View, sort: &mut SpriteSort) -> bool {
    let ps = &em.particles;
    let mode = t.sort;
    let by_view = matches!(mode, SortMode::ViewProjDepth | SortMode::DistanceToView);
    if !by_view && !matches!(mode, SortMode::AgeOldestFirst | SortMode::AgeNewestFirst) {
        return false;
    }
    let (cam, fwd) = (view.origin, view.forward);
    let SpriteSort { keys, idx } = sort;
    keys.clear();
    keys.extend(ps.iter().map(|&sl| {
        let p = &em.slots[sl];
        if by_view {
            let w = s.to_world(t.local, p.loc);
            let d = [w[0] - cam[0], w[1] - cam[1], w[2] - cam[2]];
            if mode == SortMode::ViewProjDepth {
                d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2]
            } else {
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
            }
        } else if mode == SortMode::AgeOldestFirst {
            f64::from(p.order)
        } else {
            f64::from(!p.order & 0x1ff_ffff)
        }
    }));
    idx.clear();
    idx.extend(0..ps.len());
    // Descending and stable, as JavaScript's sort is; -0 is taken as 0. Deviation: a NaN key (JavaScript's comparator
    // answers 0 for it, leaving TimSort's order to its neighbours) sorts to an end here (total_cmp), so the order
    // is total.
    idx.sort_by(|&a, &b| {
        let (ka, kb) = (keys[a] + 0.0, keys[b] + 0.0);
        kb.total_cmp(&ka)
    });
    true
}

/// The sprite sort's buffers, kept by the caller from draw to draw: the keys and the sorted places.
#[derive(Debug, Default)]
pub struct SpriteSort {
    keys: Vec<f64>,
    idx: Vec<usize>,
}

fn draw_count(t: &EmitterTemplate, n: usize) -> usize {
    if t.max_draw >= 0.0 { min(n as f64, t.max_draw) as usize } else { n }
}

/// The sprite fill (142c50180): the first MaxDrawCount particles in index or sorted order. Returns the count.
pub fn fill_sprites(
    t: &EmitterTemplate,
    s: &SysState,
    em: &EmitterInstance,
    view: &View,
    blend: &Blend,
    sort: &mut SpriteSort,
    out: &mut Vec<u8>,
) -> u32 {
    let sorted = sort_sprites(t, s, em, view, sort);
    let n = draw_count(t, em.particles.len());
    for k in 0..n {
        let sl = em.particles[if sorted { sort.idx[k] } else { k }];
        let p = &em.slots[sl];
        // ApplyOrbitToPosition (142c36a70): Offset onto the position and PreviousOffset onto the old one.
        let o = if t.local { p.orbit } else { s.transform_vector(p.orbit) };
        let po = if t.local { p.orb_prev } else { s.transform_vector(p.orb_prev) };
        let sim = [p.loc[0] + o[0], p.loc[1] + o[1], p.loc[2] + o[2]];
        let mut pos = s.to_world(t.local, sim);
        let mut old = s.to_world(t.local, [p.old[0] + po[0], p.old[1] + po[1], p.old[2] + po[2]]);
        if p.cam_offset != 0.0 {
            let d = camera_offset(t, s, sim, view.origin, p.cam_offset);
            for c in 0..3 {
                pos[c] += d[c];
                old[c] += d[c];
            }
        }
        if blend.alpha < 1.0 {
            let d = between(t, s, blend, pos, old);
            for c in 0..3 {
                pos[c] += d[c];
                old[c] += d[c];
            }
        }
        // Size = |Size x Scale3D| per axis, Y = X for PSA_Square, each negated where BaseSize is (the UV flip).
        let mut sx = (p.size[0] * s.scale[0]).abs();
        let mut sy = if t.align == Align::Square { sx } else { (p.size[1] * s.scale[1]).abs() };
        if p.base_size[0] < 0.0 {
            sx = -sx;
        }
        if p.base_size[1] < 0.0 {
            sy = -sy;
        }
        put3(out, pos);
        put(out, p.rel_time);
        put3(out, old);
        // Unverified: ParticleId = (Flags & STATE_CounterMask) / 10000 as the 4.26 sprite fill writes it (not read
        // in the exe; none is drawn).
        put(out, f64::from(p.order) / 10000.0);
        put(out, sx);
        put(out, sy);
        put(out, p.rot);
        put(out, p.sub_image);
        put4(out, p.color);
        put4(out, p.dyn_param);
    }
    n as u32
}

/// The mesh fill (142c4d450): instances from index min(count, MaxDrawCount) - 1 down to 0, no sort. Returns the
/// count.
pub fn fill_meshes(
    t: &EmitterTemplate,
    s: &SysState,
    em: &EmitterInstance,
    view: &View,
    blend: &Blend,
    out: &mut Vec<u8>,
) -> u32 {
    let n = draw_count(t, em.particles.len());
    for i in (0..n).rev() {
        push_mesh(t, s, &em.slots[em.particles[i]], view, blend, out);
    }
    n as u32
}

/// FDynamicMeshEmitterData::CalculateParticleTransform (142c38b60): PreRotation x Scale(Size x Source.Scale) x
/// Rotation x Translation (x ComponentToWorld when local), or the camera facing rows; the orbit Offset is the last
/// translation; the instance velocity is this tick's displacement; the SubUV cells A = floor(index), B = A + 1.
fn push_mesh(t: &EmitterTemplate, s: &SysState, p: &Particle, view: &View, blend: &Blend, out: &mut Vec<u8>) {
    let o = if t.local { p.orbit } else { s.transform_vector(p.orbit) };
    let mut pos = s.to_world(t.local, [p.loc[0] + o[0], p.loc[1] + o[1], p.loc[2] + o[2]]);
    if p.cam_offset != 0.0 {
        let d = camera_offset(t, s, p.loc, view.origin, p.cam_offset);
        for c in 0..3 {
            pos[c] += d[c];
        }
    }
    if blend.alpha < 1.0 {
        let po = if t.local { p.orb_prev } else { s.transform_vector(p.orb_prev) };
        let a0 = s.to_world(t.local, [p.loc[0] + o[0], p.loc[1] + o[1], p.loc[2] + o[2]]);
        let a1 = s.to_world(t.local, [p.old[0] + po[0], p.old[1] + po[1], p.old[2] + po[2]]);
        let d = between(t, s, blend, a0, a1);
        for c in 0..3 {
            pos[c] += d[c];
        }
    }
    let d2r = std::f64::consts::PI / 180.0;
    let al = p.mesh_align;
    let r = if t.cam_facing {
        facing_rows(pos, view.origin, p.rot, t.cam_facing_no_up)
    } else {
        rotation_rows(
            p.rot + (p.mesh_rot[1] + al[1]) * d2r,
            p.rot + (p.mesh_rot[2] + al[2]) * d2r,
            p.rot + (p.mesh_rot[0] + al[0]) * d2r,
        )
    };
    let k = if t.local { [1.0; 3] } else { s.scale };
    let mut m = [0.0; 9];
    for i in 0..3 {
        for c in 0..3 {
            m[3 * i + c] = r[3 * i + c] * p.size[i] * k[i];
        }
    }
    let pre = p.mesh_init;
    if pre[0] != 0.0 || pre[1] != 0.0 || pre[2] != 0.0 {
        m = compose_rows(&rotation_rows(pre[1] * d2r, pre[2] * d2r, pre[0] * d2r), &m);
    }
    if t.local && !t.cam_facing {
        let mut msrot = [0.0; 9];
        for j in 0..9 {
            msrot[j] = s.rot[j] * s.scale[j / 3];
        }
        m = compose_rows(&m, &msrot);
    }
    let (q, qp) = (p.orbit, p.orb_prev);
    let mut d0 = [0.0; 3];
    for c in 0..3 {
        d0[c] = p.loc[c] + q[c] - (p.old[c] + qp[c]);
    }
    let mut vel = [0.0, 0.0, 0.0, 1.0];
    if d0[0] != 0.0 || d0[1] != 0.0 || d0[2] != 0.0 {
        let v = if t.local { s.transform_vector(d0) } else { d0 };
        let len = hypot3(v[0], v[1], v[2]);
        if len > 1e-8 {
            vel[0] = v[0] / len;
            vel[1] = v[1] / len;
            vel[2] = v[2] / len;
        }
        vel[3] = len;
    }
    let a = p.sub_image.floor();
    let b = a + 1.0;
    let sub = [a % t.sub_h, (a / t.sub_h).floor(), b % t.sub_h, (b / t.sub_h).floor()];
    put4(out, p.color);
    for c in 0..3 {
        put4(out, [m[c], m[3 + c], m[6 + c], pos[c]]);
    }
    put4(out, vel);
    for x in sub {
        out.extend_from_slice(&(x as i16).to_le_bytes());
    }
    put(out, p.sub_image - p.sub_image.floor());
    put(out, p.rel_time);
    put4(out, p.dyn_param);
}

/// cvttss2si: truncation, 0x80000000 when out of range.
fn ftoi(x: f64) -> f64 {
    if (-2147483648.0..2147483648.0).contains(&x) { x.trunc() } else { -2147483648.0 }
}

/// A ribbon point's up: with RenderAxis CameraUp normalize(Tangent) x normalize(Location - ViewOrigin), the view's
/// forward axis (InvViewMatrix row 2) when that is nearly zero, normalised; otherwise the payload Up.
fn ribbon_up(camera_up: bool, p: &Particle, view: &View) -> V3 {
    let r = p.rb.as_ref().unwrap();
    if !camera_up {
        return r.up;
    }
    let tn = safe_normal(r.tangent);
    let d = safe_normal([p.loc[0] - view.origin[0], p.loc[1] - view.origin[1], p.loc[2] - view.origin[2]]);
    let mut up = [tn[1] * d[2] - tn[2] * d[1], tn[2] * d[0] - tn[0] * d[2], tn[0] * d[1] - tn[1] * d[0]];
    if up.iter().all(|x| x.abs() <= 1e-4) {
        up = view.forward;
    }
    safe_normal(up)
}

/// The ribbon strip's writer: a pair's two vertices, and the repeated vertices between trails.
struct Strip<'a> {
    out: &'a mut Vec<u8>,
    count: u32,
    /// The place in `out` of the last vertex written, repeated to close a trail.
    last: Option<usize>,
    /// A trail ended: the next pair's first vertex is written twice.
    join: bool,
}

/// One pair's values: the centre and up, the half width, Tex_U, Tex_U2, Tex_V2 and the colour and dynamic
/// parameter.
struct Pair {
    pos: V3,
    up: V3,
    w: f64,
    u: f64,
    u2: f64,
    v2: f64,
    color: [f64; 4],
    dyn_param: [f64; 4],
}

impl Strip<'_> {
    /// pos + up x w (Tex_V 0, Tex_V2 0) and pos - up x w (Tex_V 1, Tex_V2 v2): the order and winding of the
    /// two triangles a pair.
    fn pair(&mut self, p: &Particle, q: &Pair) {
        for side in 0..2 {
            let w = if side == 0 { q.w } else { -q.w };
            let stride = RIBBON_STRIDE as usize;
            if side == 0 && self.join {
                if let Some(l) = self.last {
                    self.out.extend_from_within(l..l + stride);
                }
                self.vertex(p, q, side, w);
                self.count += 2;
                self.join = false;
            }
            self.last = Some(self.out.len());
            self.vertex(p, q, side, w);
            self.count += 1;
        }
    }

    /// One vertex of a pair: `side` 0 the + side, 1 the - side, at half width `w` (signed).
    fn vertex(&mut self, p: &Particle, q: &Pair, side: u32, w: f64) {
        let v = &mut *self.out;
        // FillVertexData (142c46e00): Position = OldPosition = the point +- Up x w.
        let at = [q.pos[0] + q.up[0] * w, q.pos[1] + q.up[1] * w, q.pos[2] + q.up[2] * w];
        put3(v, at);
        put(v, p.rel_time);
        put3(v, at);
        // Unverified: ParticleId 0, Size (w, w), the particle's Rotation and SubImageIndex (the trail
        // FillVertexData's other fields, not read; none is drawn).
        put(v, 0.0);
        put(v, q.w);
        put(v, q.w);
        put(v, p.rot);
        put(v, p.sub_image);
        put4(v, q.color);
        put(v, q.u);
        put(v, f64::from(side));
        put(v, q.u2);
        put(v, if side == 0 { 0.0 } else { q.v2 });
        put4(v, q.dyn_param);
    }
}

/// The ribbon fill: DetermineVertexAndTriangleCount (RenderingInterpCount per segment from the distance and tangent
/// steps, TriangleCount on the head, written into the particles as the build does) and FillVertexData (each trail
/// from its head, the Hermite steps between particles). Every ribbon draws its world positions: the ribbon
/// FillReplayData 142c70d60 clears bUseLocalSpace. Returns the vertex count; 0 when the build makes no dynamic data
/// (no triangles, or past the 16-bit index limit).
pub fn fill_ribbon(t: &EmitterTemplate, s: &SysState, em: &mut EmitterInstance, view: &View, out: &mut Vec<u8>) -> u32 {
    let rb = t.ribbon.as_ref().unwrap();
    let do_dist = rb.step.abs() > 1e-8;
    let do_tan = rb.tan_scale.abs() > 1e-8 || rb.tan_diff;
    let (mut tan_f, mut dist_f, mut trails, mut sum) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..em.particles.len() {
        let ps = em.particles[i];
        let r = em.slots[ps].rb.as_ref().unwrap();
        if (r.ty == R_START || r.ty == R_DEAD) && r.next.is_none() {
            let r = em.slots[ps].rb.as_mut().unwrap();
            r.interp_count = 0.0;
            r.triangles = 0.0;
            continue;
        }
        if r.ty != R_END {
            continue;
        }
        let Some(mut prev) = r.prev else { continue };
        let mut cur = ps;
        let mut tris = 0.0;
        loop {
            if do_tan {
                let a = safe_normal(em.slots[cur].rb.as_ref().unwrap().tangent);
                let b = safe_normal(em.slots[prev].rb.as_ref().unwrap().tangent);
                tan_f = (b[0] * a[0] + b[1] * a[1] + b[2] * a[2] - 1.0) * -0.5;
            }
            if do_dist {
                let (cl, pl) = (em.slots[cur].loc, em.slots[prev].loc);
                dist_f = hypot3(cl[0] - pl[0], cl[1] - pl[1], cl[2] - pl[2]) / rb.step;
                if rb.tan_diff && tan_f < 0.5 {
                    dist_f *= min(max(tan_f, 0.0), 0.5) * 2.0;
                }
            }
            let n = min(ftoi(dist_f), 65535.0) + min(ftoi(tan_f * rb.tan_scale), 65535.0);
            let c = em.slots[cur].rb.as_mut().unwrap();
            c.interp_count = if n > 0.0 { n } else { 1.0 };
            tris += c.interp_count * 2.0;
            let Some(pp) = em.slots[prev].rb.as_ref().unwrap().prev else { break };
            cur = prev;
            prev = pp;
        }
        let h = em.slots[prev].rb.as_mut().unwrap();
        h.triangles = tris;
        h.interp_count = 1.0;
        trails += 1.0;
        sum += tris + 2.0;
    }
    let total = if trails > 0.0 { sum + (trails - 2.0) * 2.0 } else { 0.0 };
    if total <= 0.0 || total + 2.0 >= 65536.0 {
        return 0;
    }
    let sx = s.scale[0];
    let tiled = rb.tiling > 0.0;
    let em = &*em;
    let mut strip = Strip { out, count: 0, last: None, join: false };
    for &hs in &em.particles {
        let hr = em.slots[hs].rb.as_ref().unwrap();
        if (hr.ty != R_START && hr.ty != R_DEAD) || hr.next.is_none() {
            continue;
        }
        if strip.count > 0 {
            strip.join = true;
        }
        let inc = 2.0 / hr.triangles;
        let mut u = 0.0;
        let mut prev: Option<usize> = None;
        let mut prev_up = UNIT_Z;
        let mut at = Some(hs);
        while let Some(cs) = at {
            let cur = &em.slots[cs];
            let cr = cur.rb.as_ref().unwrap();
            let up = ribbon_up(rb.camera_up, cur, view);
            let w = sx * cur.size[0];
            match prev {
                Some(ps) if cr.interp_count >= 2.0 => {
                    let pv = &em.slots[ps];
                    let pr = pv.rb.as_ref().unwrap();
                    let n = cr.interp_count;
                    let step = 1.0 / n;
                    let pw = sx * pv.size[0];
                    let mut i = n - 1.0;
                    while i >= 0.0 {
                        let a = i * step;
                        let a2 = a * a;
                        let a3 = a2 * a;
                        let (h00, h10, h11, h01) =
                            (2.0 * a3 - 3.0 * a2 + 1.0, a3 - 2.0 * a2 + a, a3 - a2, 3.0 * a2 - 2.0 * a3);
                        let mut q = Pair {
                            pos: [0.0; 3],
                            up: [0.0; 3],
                            w: (pw - w) * a + w,
                            u,
                            u2: if tiled { (pr.tiled_u - cr.tiled_u) * a + cr.tiled_u } else { u },
                            v2: 1.0,
                            color: [0.0; 4],
                            dyn_param: [0.0; 4],
                        };
                        for k in 0..3 {
                            q.pos[k] = h00 * cur.loc[k] + h10 * cr.tangent[k] + h11 * pr.tangent[k] + h01 * pv.loc[k];
                            // (lerp(x, y) = (y - x) x a + x)
                            q.up[k] = (prev_up[k] - up[k]) * a + up[k];
                        }
                        for k in 0..4 {
                            q.color[k] = (pv.color[k] - cur.color[k]) * a + cur.color[k];
                            q.dyn_param[k] = (pv.dyn_param[k] - cur.dyn_param[k]) * a + cur.dyn_param[k];
                        }
                        strip.pair(cur, &q);
                        u += inc;
                        i -= 1.0;
                    }
                }
                _ => {
                    let q = Pair {
                        pos: cur.loc,
                        up,
                        w,
                        u,
                        u2: if tiled { cr.tiled_u } else { u },
                        v2: 1.0,
                        color: cur.color,
                        dyn_param: cur.dyn_param,
                    };
                    strip.pair(cur, &q);
                    u += inc;
                }
            }
            prev_up = up;
            prev = Some(cs);
            at = cr.next;
        }
    }
    strip.count
}

/// ViewMatrix.TransformPosition(Bounds.Origin).Z as the float the translucent sort key holds.
pub fn view_z(o: V3, view: &View) -> f64 {
    let (c, f) = (view.origin, view.forward);
    fround((o[0] - c[0]) * f[0] + (o[1] - c[1]) * f[1] + (o[2] - c[2]) * f[2])
}

/// A beam's up at `loc` toward `end`: normalize(loc - end) x (loc - ViewOrigin), normalised; the camera's up axis when
/// that cannot be normalised.
fn beam_up(loc: V3, end: V3, view: &View, cam_up: V3) -> V3 {
    let r = safe_normal([loc[0] - end[0], loc[1] - end[1], loc[2] - end[2]]);
    let d = [loc[0] - view.origin[0], loc[1] - view.origin[1], loc[2] - view.origin[2]];
    let u = [r[1] * d[2] - r[2] * d[1], r[2] * d[0] - r[0] * d[2], r[0] * d[1] - r[1] * d[0]];
    let l = hypot3(u[0], u[1], u[2]);
    if l * l > 1e-8 { [u[0] / l, u[1] / l, u[2] / l] } else { cam_up }
}

/// FQuat(Right, angle).RotateVector(u), Right = normalize(loc - end): u itself at angle 0.
fn turn_beam(u: V3, loc: V3, end: V3, angle: f64) -> V3 {
    if angle == 0.0 {
        return u;
    }
    let r = safe_normal([loc[0] - end[0], loc[1] - end[1], loc[2] - end[2]]);
    let (c, sn) = (cos(angle), sin(angle));
    let k = (r[0] * u[0] + r[1] * u[1] + r[2] * u[2]) * (1.0 - c);
    [
        u[0] * c + (r[1] * u[2] - r[2] * u[1]) * sn + r[0] * k,
        u[1] * c + (r[2] * u[0] - r[0] * u[2]) * sn + r[1] * k,
        u[2] * c + (r[0] * u[1] - r[1] * u[0]) * sn + r[2] * k,
    ]
}

/// An interpolated beam point's up: Right x (at - ViewOrigin) normalised with UpVectorStepSize 0, else (and when that
/// cannot be normalised) the camera's up axis.
fn beam_up_at(at: V3, right: V3, up_step: f64, view: &View, cam_up: V3) -> V3 {
    if up_step != 0.0 {
        return cam_up;
    }
    let d = [at[0] - view.origin[0], at[1] - view.origin[1], at[2] - view.origin[2]];
    let u = [right[1] * d[2] - right[2] * d[1], right[2] * d[0] - right[0] * d[2], right[0] * d[1] - right[1] * d[0]];
    let l2 = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
    if !(l2 > 1e-8) {
        return cam_up;
    }
    let l = l2.sqrt();
    [u[0] / l, u[1] / l, u[2] / l]
}

/// `v` normalised (FVector::Normalize: unchanged unless |v|^2 > 1e-8) x `l`.
fn beam_scaled(v: V3, l: f64) -> V3 {
    let n = safe_normal(v);
    [n[0] * l, n[1] * l, n[2] * l]
}

/// A noisy beam's point k: the source + Direction x StepSize x (k + 1) + NoiseRangeScale x the noise point x the
/// noise distance scale through LocalToWorld.TransformVector.
fn beam_noise_at(s: &SysState, bm: &BeamPayload, nrs: f64, k: usize) -> V3 {
    let o = noise_offset(s, bm, nrs, k);
    let d = bm.step * (k as f64 + 1.0);
    [bm.src[0] + bm.dir[0] * d + o[0], bm.src[1] + bm.dir[1] * d + o[1], bm.src[2] + bm.dir[2] * d + o[2]]
}

/// NoiseRangeScale x LocalToWorld.TransformVector(TargetNoisePoints[k] x the noise distance scale); a point past the
/// payload's (a module frequency above the beam's own) as 0.
fn noise_offset(s: &SysState, bm: &BeamPayload, nrs: f64, k: usize) -> V3 {
    let n = bm.noise.get(k).copied().unwrap_or([0.0; 3]);
    let o = s.transform_vector([n[0] * bm.noise_scale, n[1] * bm.noise_scale, n[2] * bm.noise_scale]);
    [nrs * o[0], nrs * o[1], nrs * o[2]]
}

/// `a` x `fa` + `b` x `fb`.
fn mix(a: V3, fa: f64, b: V3, fb: f64) -> V3 {
    [a[0] * fa + b[0] * fb, a[1] * fa + b[1] * fb, a[2] * fa + b[2] * fb]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// FMath::CubicInterp(p0, t0, p1, t1, a) with the build's basis order (142c3eb10: h00 p0 + h10 t0 + h11 t1 + h01 p1).
fn hermite(p0: V3, t0: V3, p1: V3, t1: V3, a: f64) -> V3 {
    let a2 = a * a;
    let a3 = a2 * a;
    let (h00, h10, h11, h01) = (((a3 + a3) - a2 * 3.0) + 1.0, (a3 - (a2 + a2)) + a, a3 - a2, a2 * 3.0 - (a3 + a3));
    std::array::from_fn(|c| h00 * p0[c] + h10 * t0[c] + h11 * t1[c] + h01 * p1[c])
}

/// What the interpolated-noise fill reads of its beam besides the payload.
struct InterpNoise<'a> {
    s: &'a SysState,
    view: &'a View,
    cam_up: V3,
    /// UpVectorStepSize.
    up_step: f64,
    /// The beam's index in the emitter.
    index: usize,
    /// Size.X x Scale.X.
    size: f64,
    /// NoiseRangeScale and NoiseTangentStrength at EmitterTime (FillReplayData).
    nrs: f64,
    nts: f64,
    sheets: f64,
    /// InterpolationPoints.
    interp: usize,
    /// A TaperMethod: the widths x the payload's TaperValues.
    tapered: bool,
    /// The source's tangent: the Source module's, else WorldToLocal's X axis (LocalToWorld.InverseTransformVector(1,
    /// 0, 0)).
    src_tan: V3,
    /// A Target module (the replay's bUseTarget): the last segment takes the payload's target tangent.
    use_target: bool,
}

/// FDynamicBeam2EmitterData::FillData_InterpolatedNoise (142c3eb10), for a beam with low-frequency noise and
/// InterpolationPoints > 0; per beam with TriangleCount and Steps both non-zero, per sheet, one strip:
/// - InterpStep = InterpolationSteps / Steps (int over int), its fraction f and its integer part n.
/// - The source pair: Right = normalize(source - (f x Points[n] + (1 - f) x Points[n + 1] + the noise offset of
///   point 0)); Up = Right x (source - ViewOrigin) normalised (the camera's Y axis when it cannot be) with
///   UpVectorStepSize 0, or 1 on the first beam; else Up is the last one worked out (it carries over from beam to
///   beam). Sheet k > 0 turns Up about Right by k x pi / Sheets. Width Size x TaperValues[0]; Tex_U 0.
/// - Step k = 0 .. Steps - 1: the point Cur = (1 - f) x Points[k n] + f x (Points[k n + 1], the target on the last
///   step) + the noise offset of point k; the next point (1 - f) x Points[(k + 1) n + 1] + f x Points[(k + 1) n] (the
///   last step: (1 - f) x the target + f x Points[(k + 1) n]) + the noise offset of point k + 1, or, on the last step
///   of a locked beam, the target (with bTargetNoise, plus the noise offset of point Frequency); the tangent
///   normalize((1 - NoiseTension) / 2 x (next - LastDraw)) x NoiseTangentStrength. NoiseTessellation points
///   CubicInterp(LastDraw, LastTangent, Cur, that tangent, j / NoiseTessellation), each a pair with Right =
///   normalize(the point before - the point), Up worked out from it with UpVectorStepSize 0 (not kept), else the
///   kept Up; width Size x TaperValues[k x NoiseTessellation + j - 1]; Tex_U from 1 / (Frequency x
///   NoiseTessellation), stepping by that much. Then LastDraw = Cur, LastTangent = the tangent.
/// - A locked beam's last segment to the target (plus the noise offset of point Frequency with bTargetNoise):
///   tangent normalize((1 - NoiseTension) / 2 x (target - LastDraw)), or the payload's target tangent with a
///   Target module (the replay's bUseTarget), x NoiseTangentStrength; NoiseTessellation more points as above,
///   TaperValues[Steps x NoiseTessellation + j].
///
/// LastTangent starts as the source tangent x NoiseTangentStrength (not normalised) and is not reset per sheet.
/// Frequency here is the module's (at least 1). Tex_U2 and Tex_V2 are not written (0 here). Every beam is locked in
/// the port (Speed 0: a travelling beam is a gap of the template).
/// The smooth noise (bSmooth, NoiseLockTime >= 0: the point steps toward NextNoisePoints by NoiseSpeed x
/// NoiseRate, snapping within NoiseLockRadius) is not ported: no beam has it (the template's gap).
/// Unverified: a point index past the payload's InterpolationPoints reads the payload's next floats in the build (the
/// noise rate and delta); 0 here. The grapple meets it with weight 1 - f = 1 (step 248 reads Points[250] of 250),
/// harmless only while its NoiseTangentStrength is 0 (the read feeds the tangent alone).
// Unverified: the fill's Up starts at (0, 0, 0) in the build (UpVectorStepSize above 1, never worked out); here the
// shared one, the camera's axis. No beam has such a step size.
fn fill_interp_noise(
    strip: &mut Strip<'_>,
    p: &Particle,
    bm: &BeamPayload,
    nz: &BeamNoise,
    c: &InterpNoise<'_>,
    up: &mut V3,
) {
    if bm.tris == 0.0 || bm.steps == 0.0 {
        return;
    }
    let tess_n = max(nz.tess.trunc(), 1.0);
    let tess = tess_n as usize;
    let freq_mod = if nz.freq.trunc() > 0.0 { nz.freq.trunc() } else { 1.0 };
    let inv_tess = 1.0 / tess_n;
    let tex_inc = 1.0 / (freq_mod * tess_n);
    let steps = bm.steps as usize;
    let step_ratio = c.interp as f64 / bm.steps.trunc();
    let n = step_ratio.trunc() as usize;
    let f = step_ratio - step_ratio.trunc();
    let g = 1.0 - f;
    let at = |j: usize| bm.pts.get(j).copied().unwrap_or([0.0; 3]);
    let off = |k: usize| noise_offset(c.s, bm, c.nrs, k);
    let half = (1.0 - nz.tension) * 0.5;
    let taper_at = |k: usize| bm.taper.get(k).copied().unwrap_or(f64::NAN);
    let per_up =
        |interim: V3, pos: V3, kept: V3| if c.up_step == 0.0 { beam_up(interim, pos, c.view, c.cam_up) } else { kept };
    let tangent = |next: V3, last: V3| {
        let t = safe_normal([(next[0] - last[0]) * half, (next[1] - last[1]) * half, (next[2] - last[2]) * half]);
        [t[0] * c.nts, t[1] * c.nts, t[2] * c.nts]
    };
    let pair = |pos: V3, up: V3, w: f64, u: f64| Pair {
        pos,
        up,
        w,
        u,
        u2: 0.0,
        v2: 0.0,
        color: p.color,
        dyn_param: p.dyn_param,
    };
    let mut last_tan = [c.src_tan[0] * c.nts, c.src_tan[1] * c.nts, c.src_tan[2] * c.nts];
    let mut taper = 1.0;
    let mut sheet = 0.0;
    while sheet < c.sheets {
        let angle = (std::f64::consts::PI / c.sheets) * sheet;
        if strip.count > 0 {
            strip.join = true;
        }
        let mut last = bm.src;
        let first = add(mix(at(n), f, at(n + 1), g), off(0));
        if c.up_step == 0.0 || (c.up_step == 1.0 && c.index == 0) {
            *up = beam_up(bm.src, first, c.view, c.cam_up);
        }
        if c.tapered {
            taper = taper_at(0);
        }
        strip.pair(p, &pair(bm.src, turn_beam(*up, bm.src, first, angle), c.size * taper, 0.0));
        let mut u = tex_inc;
        // (the tessellated points of one segment, LastDraw to `to` with the tangent `tan`, taper index `k0` + j)
        let mut segment = |strip: &mut Strip, last: V3, last_tan: V3, to: V3, tan: V3, k0: usize, taper: &mut f64| {
            let mut interim = last;
            for j in 0..tess {
                let pos = hermite(last, last_tan, to, tan, (j + 1) as f64 * inv_tess);
                let w_up = per_up(interim, pos, *up);
                if c.tapered {
                    *taper = taper_at(k0 + j);
                }
                strip.pair(p, &pair(pos, turn_beam(w_up, interim, pos, angle), c.size * *taper, u));
                u += tex_inc;
                interim = pos;
            }
        };
        for k in 0..steps {
            let i0 = k * n;
            let cur = if k + 1 == steps { mix(bm.tgt, f, at(i0), g) } else { mix(at(i0 + 1), f, at(i0), g) };
            let cur = add(cur, off(k));
            let next = if k + 1 == steps {
                // (locked: the target, not the interpolated point after it)
                if nz.target_noise { add(bm.tgt, off(nz.freq as usize)) } else { bm.tgt }
            } else {
                add(mix(at(i0 + n), f, at(i0 + n + 1), g), off(k + 1))
            };
            let tan = tangent(next, last);
            segment(strip, last, last_tan, cur, tan, k * tess, &mut taper);
            last = cur;
            last_tan = tan;
        }
        let to = if nz.target_noise { add(bm.tgt, off(nz.freq as usize)) } else { bm.tgt };
        let tan = if c.use_target {
            [bm.tgt_tan[0] * c.nts, bm.tgt_tan[1] * c.nts, bm.tgt_tan[2] * c.nts]
        } else {
            tangent(bm.tgt, last)
        };
        segment(strip, last, last_tan, to, tan, steps * tess, &mut taper);
        sheet += 1.0;
    }
}

/// The beam fill. FDynamicBeam2EmitterData's DoBufferFill (142c3e220) runs
/// FillVertexData_NoNoise (142c47f70) or, with low-frequency noise, FillData_Noise (142c41620), per beam with
/// triangles and per sheet, as a triangle strip of FParticleBeamTrailVertex (FillIndexData 142c315a0); the strips
/// joined here by repeated vertices as the ribbon's trails. RenderGeometry is not read on this path: a beam with it
/// false is drawn.
/// Size = Size.X x Scale.X. Up = Right x (Location - ViewOrigin) with Right = normalize(Location - EndPoint) of the
/// segment, the camera's up axis (`cam_up`, CameraToWorld's Y, View +0x390) when that is nearly zero;
/// UpVectorStepSize 0 works it out per segment, 1 once for the first beam. Sheet k > 0 turns Up about Right by k x pi
/// / Sheets. Each edge is the point +- Up x Size.
/// Without noise: the source (Tex_U 0) and the particle's Location (Tex_U = TextureTile, at least 1, or the source to
/// target distance / TextureTileDistance when that is above 1e-4), Tex_V 0 / 1, Tex_U2 0 at the source and 1 at the
/// end, Tex_V2 as Tex_V.
/// With noise (InterpolationPoints 0): from the source, Steps points at Direction x StepSize apart, each displaced by
/// NoiseRangeScale(EmitterTime) x LocalToWorld.TransformVector(noise point), then the target; between two of them
/// NoiseTessellation points of FMath::CubicInterp with the tangents (first: the source tangent normalised x the source
/// strength; a noise point's: (1 - NoiseTension) / 2 x (next - last), the last one's the target tangent, normalised x
/// NoiseTangentStrength, the target strength for the last). Tex_U steps by 1 / (frequency x NoiseTessellation) from
/// 0, so it ends at (frequency + 1) / frequency; Tex_U2 and Tex_V2 are not written by this fill (0 here).
/// With noise and InterpolationPoints > 0: FillData_InterpolatedNoise (142c3eb10), `fill_interp_noise`.
/// Returns the vertex count.
pub fn fill_beam(
    t: &EmitterTemplate,
    s: &SysState,
    em: &EmitterInstance,
    view: &View,
    cam_up: V3,
    rng: &mut dyn RandomSource,
    out: &mut Vec<u8>,
) -> u32 {
    let b = t.beam.as_ref().unwrap();
    let sx = s.scale[0];
    // (up: carried from one beam and sheet to the next)
    let mut up = cam_up;
    let mut strip = Strip { out, count: 0, last: None, join: false };
    for (i, p) in em.particles().enumerate() {
        let bm = p.bm.as_deref().unwrap();
        if bm.tris == 0.0 {
            continue;
        }
        let size = p.size[0] * sx;
        if let Some(nz) = b.noise.as_ref().filter(|_| !bm.noise.is_empty() && b.interp > 0) {
            let c = InterpNoise {
                s,
                view,
                cam_up,
                up_step: b.up_step,
                index: i,
                size,
                nrs: sample1(&nz.range_scale, em.time, &s.params, rng),
                nts: sample1(&nz.tangent_strength, em.time, &s.params, rng),
                sheets: b.sheets,
                interp: b.interp,
                tapered: b.taper.is_some(),
                src_tan: if b.source.is_some() { bm.src_tan } else { s.inverse_transform_vector(UNIT_X) },
                use_target: b.target.is_some(),
            };
            fill_interp_noise(&mut strip, p, bm, nz, &c, &mut up);
            continue;
        }
        let per_beam = b.up_step == 0.0 || (b.up_step == 1.0 && i == 0);
        let tv = &bm.taper;
        // (empty: no taper; an index past the values reads undefined as JavaScript does, NaN: the pair
        // has no width and draws nothing)
        let taper = |k: usize| if tv.is_empty() { 1.0 } else { tv.get(k).copied().unwrap_or(f64::NAN) };
        let pair = |pos: V3, up: V3, w: f64, u: f64, u2: f64, v2: f64| Pair {
            pos,
            up,
            w,
            u,
            u2,
            v2,
            color: p.color,
            dyn_param: p.dyn_param,
        };
        let mut sheet = 0.0;
        while sheet < b.sheets {
            let angle = (std::f64::consts::PI / b.sheets) * sheet;
            sheet += 1.0;
            if strip.count > 0 {
                strip.join = true;
            }
            if (b.noise.is_none() || bm.noise.is_empty()) && !bm.pts.is_empty() && b.interp >= 2 {
                // FillVertexData_NoNoise, InterpolationPoints >= 2 (142c47f70): Right stays normalize(source - the
                // first point) for the whole beam; the source pair, then one pair per interpolated point, each with
                // Up = Right x (the point before it - ViewOrigin) normalised (UpVectorStepSize 0; else, and when it
                // cannot be normalised, the camera's axis), turned for the sheet about that Right. Tex_U steps by 1 /
                // InterpolationPoints, or by |target - source| / TextureTileDistance / InterpolationPoints when that
                // distance is above 1e-4; Tex_U2 by 1 / Steps; Tex_V and Tex_V2 0 / 1. Taper: the source x
                // TaperValues[0], point j x [j + 1].
                let n = b.interp as f64;
                let p0 = bm.pts[0];
                let right = safe_normal([bm.src[0] - p0[0], bm.src[1] - p0[1], bm.src[2] - p0[2]]);
                let du = if b.tile_dist > 1e-4 {
                    hypot3(bm.tgt[0] - bm.src[0], bm.tgt[1] - bm.src[1], bm.tgt[2] - bm.src[2]) * (1.0 / b.tile_dist)
                        / n
                } else {
                    1.0 / n
                };
                let du2 = 1.0 / bm.steps;
                let u0 = turn_beam(beam_up_at(bm.src, right, b.up_step, view, cam_up), bm.src, p0, angle);
                strip.pair(p, &pair(bm.src, u0, size * taper(0), 0.0, 0.0, 1.0));
                let (mut tu, mut tu2) = (0.0, 0.0);
                let mut prev = bm.src;
                let mut j = 0;
                while (j as f64) < bm.steps {
                    let pos = bm.pts[j];
                    tu += du;
                    tu2 += du2;
                    let uj = turn_beam(beam_up_at(prev, right, b.up_step, view, cam_up), bm.src, p0, angle);
                    strip.pair(p, &pair(pos, uj, size * taper(j + 1), tu, tu2, 1.0));
                    prev = pos;
                    j += 1;
                }
                continue;
            }
            let Some(nz) = b.noise.as_ref().filter(|_| !bm.noise.is_empty()) else {
                if per_beam {
                    up = beam_up(bm.src, p.loc, view, cam_up);
                }
                let w = turn_beam(up, bm.src, p.loc, angle);
                let tiles = if b.tile_dist > 1e-4 {
                    hypot3(bm.tgt[0] - bm.src[0], bm.tgt[1] - bm.src[1], bm.tgt[2] - bm.src[2]) / b.tile_dist
                } else {
                    max(b.tile, 1.0)
                };
                // (taper: the source pair x TaperValues[0], the end pair x [1])
                strip.pair(p, &pair(bm.src, w, size * taper(0), 0.0, 0.0, 1.0));
                strip.pair(p, &pair(p.loc, w, size * taper(1), tiles, 1.0, 1.0));
                continue;
            };
            let nrs = sample1(&nz.range_scale, em.time, &s.params, rng);
            let inc = 1.0 / (bm.freq * nz.tess);
            let mut last_draw = bm.src;
            let src_str =
                if b.source.is_some() { bm.src_str } else { sample1(&nz.tangent_strength, em.time, &s.params, rng) };
            let mut last_tan = beam_scaled(bm.src_tan, src_str);
            let noise_str = sample1(&nz.tangent_strength, em.time, &s.params, rng);
            let mut cur = beam_noise_at(s, bm, nrs, 0);
            let mut u = 0.0;
            if per_beam {
                up = beam_up(last_draw, cur, view, cam_up);
            }
            strip.pair(p, &pair(last_draw, turn_beam(up, last_draw, cur, angle), size, u, 0.0, 0.0));
            u += inc;
            let half = (1.0 - nz.tension) / 2.0;
            // the Steps noise points, then the target
            let steps = bm.steps as usize;
            for k in 0..=steps {
                let tan = if k == steps {
                    cur = bm.tgt;
                    // (the target's own tangent with a Target module; else from the point one step past the last)
                    let l = if k == 0 { noise_str } else { bm.tgt_str };
                    if b.target.is_some() {
                        beam_scaled(bm.tgt_tan, l)
                    } else {
                        let mut tmp = [0.0; 3];
                        for c in 0..3 {
                            tmp[c] = (bm.src[c] + bm.dir[c] * bm.step * (k as f64 + 1.0) - last_draw[c]) * half;
                        }
                        beam_scaled(tmp, l)
                    }
                } else {
                    if k > 0 {
                        cur = beam_noise_at(s, bm, nrs, k);
                    }
                    if k + 1 == steps {
                        beam_scaled(bm.tgt_tan, bm.tgt_str)
                    } else {
                        let next = beam_noise_at(s, bm, nrs, k + 1);
                        let mut tmp = [0.0; 3];
                        for c in 0..3 {
                            tmp[c] = (next[c] - last_draw[c]) * half;
                        }
                        beam_scaled(tmp, noise_str)
                    }
                };
                // (interim: the tessellated point before)
                let mut interim = last_draw;
                let mut j = 1.0;
                while j <= nz.tess {
                    let a = j / nz.tess;
                    let a2 = a * a;
                    let a3 = a2 * a;
                    let (h00, h10, h11, h01) =
                        (2.0 * a3 - 3.0 * a2 + 1.0, a3 - 2.0 * a2 + a, a3 - a2, 3.0 * a2 - 2.0 * a3);
                    let mut pos = cur;
                    if j != nz.tess {
                        for c in 0..3 {
                            pos[c] = h00 * last_draw[c] + h10 * last_tan[c] + h11 * tan[c] + h01 * cur[c];
                        }
                    }
                    if b.up_step == 0.0 {
                        up = beam_up(interim, pos, view, cam_up);
                    }
                    strip.pair(p, &pair(pos, turn_beam(up, interim, pos, angle), size, u, 0.0, 0.0));
                    u += inc;
                    interim = pos;
                    j += 1.0;
                }
                last_draw = cur;
                last_tan = tan;
            }
        }
    }
    strip.count
}
