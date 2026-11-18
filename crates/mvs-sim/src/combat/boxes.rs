//! The hit and hurt box tables, the tests of hit shapes against
//! hurt shapes over `mvs_collision::boxes`, a pawn's always-on HitBox spheres and a
//! spawned actor's own mesh shapes.

use crate::Real;
use crate::math as js;
use indexmap::IndexMap;
use indexmap::IndexSet;
use mvs_anim::clip::STEP_EPS;
use mvs_collision::boxes as cb;
use mvs_data::boxes::{ActorBoxIndex, BoxAnim, BoxCom, BoxKind, BoxShape, SocketAxes};
use mvs_data::fighter::Map;
use mvs_data::{BoxIndex, F32s};

use crate::fighter::FighterId;
use crate::fighter::common::{char_data, data_of, peers, warn_text};
use crate::fighter::traces::{body_info, response_to};
use crate::names::{Name, Names};
use crate::world::World;
use mvs_data::value::Value;

/// The axis of a frame's number (`AX_UP`, `AX_RS`; 0: a fwd or a depth number).
const AX_UP: u8 = 1;
const AX_RS: u8 = 3;

/// The collision object types (ECollisionChannel as the engine's DefaultEngine.ini names them, the object types of
/// the trace code).
pub mod obj {
    pub const WORLD_STATIC: i32 = 0;
    pub const WORLD_DYNAMIC: i32 = 1;
    pub const PAWN: i32 = 2;
    pub const PHYSICS_BODY: i32 = 3;
    pub const VEHICLE: i32 = 4;
    pub const DESTRUCTIBLE: i32 = 5;
    pub const PLATFORM_DETECTION: i32 = 6;
    pub const HIT_BOX: i32 = 7;
    pub const DAMAGE_DESTRUCTIBLE: i32 = 8;
    pub const KNOCK_BACK: i32 = 9;
    pub const PLATFORM: i32 = 10;
    pub const PROJECTILE_DESTRUCTIBLE: i32 = 11;
    pub const CLEANSE: i32 = 12;
    pub const FADE_CHECK: i32 = 13;
    pub const ITEM: i32 = 14;
    pub const PORTAL: i32 = 15;
    pub const ITEM_DESTRUCTIBLE: i32 = 16;
    pub const WORLD_STATIC_DETECTION: i32 = 17;
    pub const HURT_BOX: i32 = 18;
    pub const BLOCK_TRACE: i32 = 19;
}

/// The object types a hit shape's overlap meets (the HitBox profile's five).
pub const CH_PAWN: u8 = 1;
pub const CH_DAMAGE: u8 = 2;
pub const CH_PROJ: u8 = 4;
pub const CH_ITEM: u8 = 8;
pub const CH_HURT: u8 = 16;
pub const CH_ALL: u8 = 31;

fn ch_bit(c: &str) -> Option<u8> {
    match c {
        "Pawn" => Some(CH_PAWN),
        "DamageDestructible" => Some(CH_DAMAGE),
        "ProjectileDestructible" => Some(CH_PROJ),
        "ItemDestructible" => Some(CH_ITEM),
        "HurtBox" => Some(CH_HURT),
        _ => None,
    }
}

/// What a sampler reads of the data beside its own tables: a character's box index (or a spawned actor's
/// sub-index) and the character's frames.
#[derive(Clone, Copy)]
pub struct BoxTables<'a> {
    pub stride: u32,
    pub shapes: &'a [BoxShape],
    pub anims: &'a Map<BoxAnim>,
    pub socket_axes: Option<&'a SocketAxes>,
    pub data: &'a [f32],
}

impl<'a> BoxTables<'a> {
    pub fn of(index: &'a BoxIndex, data: &'a F32s) -> BoxTables<'a> {
        BoxTables {
            stride: index.stride,
            shapes: &index.shapes,
            anims: &index.anims,
            socket_axes: index.socket_axes.as_ref(),
            data: &data.0,
        }
    }

    pub fn of_actor(index: &'a ActorBoxIndex, data: &'a F32s) -> BoxTables<'a> {
        BoxTables {
            stride: index.stride,
            shapes: &index.shapes,
            anims: &index.anims,
            socket_axes: index.socket_axes.as_ref(),
            data: &data.0,
        }
    }
}

/// `BoxSampler`: a character's box tables (its box index and data, `World::data` by the character), sampled at a
/// montage time. Static data made once per character (`Fixed::samplers`); the scratch is the caller's
/// buffer, and the box index's caches (`UNSCALED`, `ON_CAPSULE`, `AXES`, `SHAPE_CH`,
/// `ROOT_HITS`, `SHAPE_IGNORE_STUN`) are its fields, made with the rest.
#[derive(Clone, Debug, Default)]
pub struct BoxSampler {
    /// Each shape's offset in a sampled frame.
    pub offsets: Vec<u32>,
    pub by_name: IndexMap<Name, u32>,
    /// By name in lower case (FName lookups ignore case).
    pub by_lower: IndexMap<String, u32>,
    /// The hurt shapes, in the Mesh's AttachChildren order.
    pub hurt: Vec<u32>,
    /// GetComponentsByTag: the shapes carrying a component tag (in lower case), in component order.
    pub by_tag: IndexMap<String, Vec<u32>>,
    /// The hit rotation's bone point, `None`: the data has none.
    pub com: Option<u32>,
    /// The offsets of the (fwd, up) pairs that ride the bone.
    pub com_pts: Vec<u32>,
    /// The offsets of the (fwd, up) parts of the vectors that ride the bone (a box's half extents).
    pub com_dirs: Vec<u32>,
    /// `UNSCALED`: pairs of a socket's offset in a frame and the offset of the socket shape at its bone.
    pub unscaled: Vec<u32>,
    /// `ON_CAPSULE`: the offsets of the (fwd, up) pairs of the shapes on the root capsule.
    pub on_capsule: Vec<u32>,
    /// `AXES`: the axis of each number of a frame.
    pub axes: Vec<u8>,
    /// `SHAPE_CH`: each shape's channels (CH_*); empty for an actor's index (`shapeChannels` reads a fighter's only).
    pub shape_ch: Vec<u8>,
    /// `ROOT_HITS`: a hit shape overlaps Pawn and is no HitBox object.
    pub root_hits: bool,
    /// `SHAPE_IGNORE_STUN`: each shape's IgnoreHitStun tag.
    pub shape_ignore_stun: Vec<u8>,
    /// What the hurt tests read (`mvs_collision::boxes::Sampler`).
    pub tests: cb::Sampler,
    /// `actorBoxes`' samplers: a spawned actor class's own tracks (the index's `actors`), by the class.
    pub actors: IndexMap<String, BoxSampler>,
}

fn to_u32(i: usize) -> u32 {
    u32::try_from(i).expect("a box table's offset")
}

fn at(i: u32) -> usize {
    i as usize
}

/// A frame's place from a key position that is integral and in range (the floor of a clamped time).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "integral and in range")]
fn frame_at(f: Real) -> usize {
    debug_assert!(f >= 0.0 && f.fract() == 0.0);
    f as usize
}

/// A number stored in a frame (a Float32Array's store): rounded to the nearest f32, as `fround`.
#[allow(clippy::cast_possible_truncation, reason = "the store rounds to f32")]
fn f32_store(v: Real) -> f32 {
    v
}

impl BoxSampler {
    /// `new BoxSampler(index, data)` for a character's index; the class's sub-indexes' samplers with it.
    pub fn new(index: &BoxIndex, names: &mut Names) -> Result<BoxSampler, String> {
        let mut s =
            BoxSampler::build(names, &index.shapes, index.com.as_ref().and_then(Option::as_ref), index.stride, true)?;
        if let Some(actors) = &index.actors {
            for (cls, a) in actors {
                let sub =
                    BoxSampler::build(names, &a.shapes, a.com.as_ref().and_then(Option::as_ref), a.stride, false)?;
                s.actors.insert(cls.clone(), sub);
            }
        }
        Ok(s)
    }

    /// `channels`: the index is a fighter's, whose channels the hit tests read (a hit test reads them and
    /// fails on a bad one; an actor's index is never read, so its shapes are not checked: C020's Meeseeks GolfHitBox).
    fn build(
        names: &mut Names,
        shapes: &[BoxShape],
        com: Option<&BoxCom>,
        stride: u32,
        channels: bool,
    ) -> Result<BoxSampler, String> {
        let mut s = BoxSampler::default();
        let mut o = 2u32;
        for (i, sh) in shapes.iter().enumerate() {
            let i = to_u32(i);
            s.offsets.push(o);
            s.by_name.insert(names.intern(&sh.name), i);
            s.by_lower.insert(sh.name.to_lowercase(), i);
            let is_box = sh.is_box == Some(true);
            let on_com = sh.com == Some(true);
            if sh.kind == BoxKind::Hurt {
                s.hurt.push(i);
            }
            if on_com {
                s.com_pts.push(o);
            }
            if on_com && sh.kind != BoxKind::Socket && sh.capsule {
                s.com_pts.push(o + 3);
            }
            if on_com && is_box {
                s.com_dirs.extend([o + 3, o + 6, o + 9]);
            }
            o += if sh.kind == BoxKind::Socket {
                2
            } else if is_box {
                12
            } else if sh.capsule {
                7
            } else {
                4
            };
        }
        // The Mesh's AttachChildren order (the SCS construction order): the order an overlap update visits them in.
        // (Array.sort is stable, as sort_by_key.)
        let order = |i: &u32| shapes[at(*i)].order.map_or(crate::real(*i), crate::real);
        let by_order = |a: &u32, b: &u32| order(a).total_cmp(&order(b));
        s.hurt.sort_by(by_order);
        for (i, sh) in shapes.iter().enumerate() {
            if sh.kind == BoxKind::Socket {
                continue;
            }
            for t in &sh.tags {
                s.by_tag.entry(t.to_lowercase()).or_default().push(to_u32(i));
            }
        }
        for l in s.by_tag.values_mut() {
            l.sort_by(by_order);
        }
        for (i, sh) in shapes.iter().enumerate() {
            let Some(un) = &sh.unscaled else { continue };
            let Some(&b) = names.get(un).and_then(|n| s.by_name.get(&n)) else {
                return Err(format!("box data: {} names {}, which is no shape", sh.name, un));
            };
            s.unscaled.extend([s.offsets[i], s.offsets[at(b)]]);
        }
        for (i, sh) in shapes.iter().enumerate() {
            if sh.on_capsule != Some(true) {
                continue;
            }
            if sh.kind == BoxKind::Socket || sh.com == Some(true) {
                return Err(format!(
                    "box data: {} on the root capsule is a socket or on the hit rotation bone (not handled)",
                    sh.name
                ));
            }
            // A box (its centre moves, its three half extent vectors are directions and stay).
            s.on_capsule.push(s.offsets[i]);
            if sh.capsule {
                s.on_capsule.push(s.offsets[i] + 3);
            }
        }
        s.com = com.map(|c| c.offset);
        s.axes = axes_of(shapes, &s.offsets, s.com, stride);
        if channels {
            s.shape_ch = shape_channels(shapes)?;
            s.root_hits = shapes
                .iter()
                .enumerate()
                .any(|(i, sh)| sh.kind == BoxKind::Hit && s.shape_ch[i] != CH_ALL && (s.shape_ch[i] & CH_PAWN) != 0);
        }
        s.shape_ignore_stun =
            shapes.iter().map(|sh| u8::from(sh.tags.iter().any(|t| t.to_lowercase() == "ignorehitstun"))).collect();
        s.tests = cb::Sampler {
            capsule: shapes.iter().map(|sh| sh.capsule).collect(),
            radius: shapes.iter().map(|sh| sh.radius).collect(),
            offsets: s.offsets.iter().map(|&o| at(o)).collect(),
            hurt: s.hurt.iter().map(|&i| at(i)).collect(),
        };
        Ok(s)
    }

    /// `scale`: a sampled pose at the actor scale k (Comp_SizeManager's SizeScale); a socket on a virtual bone
    /// whose world scale the AnimBP replaces by 1 keeps its offset from the bone.
    pub fn scale(&self, s: &mut [f32], k: Real) {
        for v in s.iter_mut() {
            *v = f32_store(crate::real(*v) * k);
        }
        for p in self.unscaled.as_chunks::<2>().0 {
            let (o, b) = (at(p[0]), at(p[1]));
            s[o] = f32_store(crate::real(s[b]) + (crate::real(s[o]) - crate::real(s[b])) / k);
            s[o + 1] = f32_store(crate::real(s[b + 1]) + (crate::real(s[o + 1]) - crate::real(s[b + 1])) / k);
        }
    }

    /// `scale3`: a sampled pose at a non-uniform actor scale (X and Y by xy, Z by z): the up numbers times z, a
    /// radius scale times min(xy, z), the rest times xy.
    /// Deviation: UCapsuleComponent takes the radius times min(X, Y) and the half height times Z of its own world
    /// scale, which a bone's rotation mixes; the port's capsule is its two end points, which take the axes as they
    /// are, and one radius.
    pub fn scale3(&self, s: &mut [f32], xy: Real, z: Real) {
        let r = js::min(xy, z);
        for (v, &a) in s.iter_mut().zip(&self.axes) {
            let k = if a == AX_UP {
                z
            } else if a == AX_RS {
                r
            } else {
                xy
            };
            *v = f32_store(crate::real(*v) * k);
        }
        for p in self.unscaled.as_chunks::<2>().0 {
            let (o, b) = (at(p[0]), at(p[1]));
            s[o] = f32_store(crate::real(s[b]) + (crate::real(s[o]) - crate::real(s[b])) / xy);
            s[o + 1] = f32_store(crate::real(s[b + 1]) + (crate::real(s[o + 1]) - crate::real(s[b + 1])) / z);
        }
    }

    /// `onCapsule`: a pose's shapes on the root capsule put back at the capsule (the mesh's offset taken off).
    pub fn on_capsule(&self, s: &mut [f32], mx: Real, mz: Real) {
        if self.on_capsule.is_empty() || (mx == 0.0 && mz == 0.0) {
            return;
        }
        for &o in &self.on_capsule {
            let o = at(o);
            s[o] = f32_store(crate::real(s[o]) - mx);
            s[o + 1] = f32_store(crate::real(s[o + 1]) - mz);
        }
    }

    /// `tagged`: the shapes of the components carrying the tag (AActor.GetComponentsByTag), in attach order.
    pub fn tagged(&self, tag: &str) -> &[u32] {
        self.tagged_suffixed(tag, "")
    }

    /// [`BoxTables::tagged`] of `tag` followed by `suffix` (the 2D components' `<tag>2D`), looked up without
    /// building the string for an ASCII tag of up to 64 bytes.
    pub fn tagged_suffixed(&self, tag: &str, suffix: &str) -> &[u32] {
        let mut buf = [0u8; 64];
        let n = tag.len() + suffix.len();
        let key = if n <= buf.len() && tag.is_ascii() && suffix.is_ascii() {
            buf[..tag.len()].copy_from_slice(tag.as_bytes());
            buf[tag.len()..n].copy_from_slice(suffix.as_bytes());
            buf[..n].make_ascii_lowercase();
            std::borrow::Cow::Borrowed(std::str::from_utf8(&buf[..n]).expect("ascii bytes"))
        } else {
            std::borrow::Cow::Owned(format!("{tag}{suffix}").to_lowercase())
        };
        self.by_tag.get(&*key).map_or(&[], Vec::as_slice)
    }

    /// `has`.
    pub fn has(t: &BoxTables, anim: &str) -> bool {
        t.anims.contains_key(anim)
    }

    /// `sample`: linear interpolation between the two nearest frames; the earlier frame as it is when the
    /// sequence's Interpolation is Step (anim/clip's key lookup, with its STEP_EPS). False without the sequence
    /// (`out` untouched); `out` holds a frame's `stride` numbers.
    pub fn sample(t: &BoxTables, anim: &str, time: Real, out: &mut [f32]) -> bool {
        let Some(a) = t.anims.get(anim) else { return false };
        let stride = at(t.stride);
        let d = t.data;
        let (f, n) = key_pos(a, time);
        let base = at(a.offset);
        if a.step == Some(true) {
            let b = base + frame_at(js::min((f + crate::math::narrow(STEP_EPS)).floor(), n - 1.0)) * stride;
            out[..stride].copy_from_slice(&d[b..b + stride]);
            return true;
        }
        let i0 = f.floor();
        let i1 = js::min(i0 + 1.0, n - 1.0);
        let w = f - i0;
        let b0 = base + frame_at(i0) * stride;
        let b1 = base + frame_at(i1) * stride;
        for k in 0..stride {
            out[k] = f32_store(crate::real(d[b0 + k]) * (1.0 - w) + crate::real(d[b1 + k]) * w);
        }
        true
    }

    /// `rootInto`: the root track (fwd, up) of a sequence at a time, by `sample`'s key lookup. None without the
    /// sequence.
    pub fn root_into(t: &BoxTables, anim: &str, time: Real) -> Option<[Real; 2]> {
        let a = t.anims.get(anim)?;
        let stride = at(t.stride);
        let d = t.data;
        let (f, n) = key_pos(a, time);
        let base = at(a.offset);
        if a.step == Some(true) {
            let b = base + frame_at(js::min((f + crate::math::narrow(STEP_EPS)).floor(), n - 1.0)) * stride;
            return Some([crate::real(d[b]), crate::real(d[b + 1])]);
        }
        let i0 = f.floor();
        let i1 = js::min(i0 + 1.0, n - 1.0);
        let w = f - i0;
        let b0 = base + frame_at(i0) * stride;
        let b1 = base + frame_at(i1) * stride;
        Some([
            crate::real(d[b0]) * (1.0 - w) + crate::real(d[b1]) * w,
            crate::real(d[b0 + 1]) * (1.0 - w) + crate::real(d[b1 + 1]) * w,
        ])
    }

    /// `axesInto`: the axes of a socket shape in a sequence's pose at a time (the socket's local X, Y and Z as
    /// (fwd, up, depth) each), from the `socketAxes` block or the shape's `axesConst`. None for a shape without
    /// axes in the data or a sequence the data has not.
    pub fn axes_into(t: &BoxTables, shape: u32, anim: &str, time: Real) -> Option<[Real; 9]> {
        let s = &t.shapes[at(shape)];
        let mut out = [0.0; 9];
        if let Some(c) = &s.axes_const {
            out.copy_from_slice(&c[..9]);
            return Some(out.map(crate::math::narrow));
        }
        let (sa, ax, a) = (t.socket_axes?, s.axes?, t.anims.get(anim)?);
        let d = t.data;
        let (f, n) = key_pos(a, time);
        let row = at(a.offset / t.stride);
        let stride = at(sa.stride);
        let base = at(sa.offset) + at(ax) * 9;
        if a.step == Some(true) {
            let b = base + (row + frame_at(js::min((f + crate::math::narrow(STEP_EPS)).floor(), n - 1.0))) * stride;
            for (k, v) in out.iter_mut().enumerate() {
                *v = f64::from(crate::real(d[b + k]));
            }
            return Some(out.map(crate::math::narrow));
        }
        let i0 = f.floor();
        let i1 = js::min(i0 + 1.0, n - 1.0);
        let w = f - i0;
        let b0 = base + (row + frame_at(i0)) * stride;
        let b1 = base + (row + frame_at(i1)) * stride;
        for (k, v) in out.iter_mut().enumerate() {
            *v = f64::from(crate::real(d[b0 + k]) * (1.0 - w) + crate::real(d[b1 + k]) * w);
        }
        Some(out.map(crate::math::narrow))
    }
}

/// The key position of a time in a sequence and its frame count.
fn key_pos(a: &BoxAnim, time: Real) -> (Real, Real) {
    let n = crate::real(a.num_frames);
    let f = if n > 1.0 && a.length > 0.0 {
        js::min(js::max(time / crate::math::narrow(a.length), 0.0), 1.0) * (n - 1.0)
    } else {
        0.0
    };
    (f, n)
}

/// `axesOf`: the axis of each number of a frame.
fn axes_of(shapes: &[BoxShape], offsets: &[u32], com: Option<u32>, stride: u32) -> Vec<u8> {
    let mut a = vec![0u8; at(stride)];
    let fu = |a: &mut Vec<u8>, o: u32| a[at(o) + 1] = AX_UP;
    fu(&mut a, 0);
    for (i, s) in shapes.iter().enumerate() {
        let o = offsets[i];
        if s.kind == BoxKind::Socket {
            fu(&mut a, o);
        } else if s.is_box == Some(true) {
            for k in 0..4 {
                fu(&mut a, o + 3 * k);
            }
        } else if s.capsule {
            fu(&mut a, o);
            fu(&mut a, o + 3);
            a[at(o) + 6] = AX_RS;
        } else {
            fu(&mut a, o);
            a[at(o) + 3] = AX_RS;
        }
    }
    if let Some(c) = com {
        fu(&mut a, c);
    }
    a
}

/// `shapeChannels`.
fn shape_channels(shapes: &[BoxShape]) -> Result<Vec<u8>, String> {
    let mut v = Vec::with_capacity(shapes.len());
    for sh in shapes {
        let mut m = CH_ALL;
        if let Some(ov) = &sh.overlaps {
            m = 0;
            for c in ov {
                let Some(b) = ch_bit(c) else {
                    return Err(format!("shape {}: overlap channel {} is not one of the HitBox profile's", sh.name, c));
                };
                m |= b;
            }
        }
        if sh.kind == BoxKind::Hit && sh.capsule && m != CH_PAWN {
            return Err(format!("shape {}: a capsule hit shape that overlaps more than Pawn (not handled)", sh.name));
        }
        v.push(m);
    }
    Ok(v)
}

/// `WorldSphere`: a hit or hurt shape placed in the world.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct WorldSphere {
    pub x: Real,
    pub y: Real,
    /// Depth (actor Y); `None` for a shape extruded along the depth.
    pub z: Option<Real>,
    pub r: Real,
    /// The hitbox component's place among the Mesh's attach children.
    pub order: Option<i32>,
    /// A detector's channels (CH_*); `None`: all five.
    pub ch: Option<i32>,
    /// The component has the IgnoreHitStun tag.
    pub ignore_stun: bool,
    /// A BoxComponent's three half-extent vectors (x, y, depth each); x, y, z its centre and r 0.
    pub bx: Option<[Real; 9]>,
    /// A capsule hit shape's second end (x, y, depth); x, y, z its first.
    pub seg: Option<[Real; 3]>,
}

impl WorldSphere {
    fn tests(&self) -> cb::Sphere {
        cb::Sphere {
            x: f64::from(self.x),
            y: f64::from(self.y),
            z: self.z.map(f64::from),
            r: f64::from(self.r),
            box_: self.bx.map(|a| a.map(f64::from)),
            seg: self.seg.map(|a| a.map(f64::from)),
        }
    }
}

/// `ExtraHurt`: an upright capsule hurt shape beside the box table's.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ExtraHurt {
    pub x: Real,
    pub y0: Real,
    pub y1: Real,
    pub r: Real,
}

impl ExtraHurt {
    fn tests(&self) -> cb::Extra {
        cb::Extra { x: f64::from(self.x), y0: f64::from(self.y0), y1: f64::from(self.y1), r: f64::from(self.r) }
    }
}

/// The hurt shapes switched off, as the tests take them (membership alone is read).
fn off_list(off: Option<&IndexSet<i32>>) -> Option<Vec<usize>> {
    off.filter(|o| !o.is_empty()).map(|o| o.iter().filter_map(|&i| usize::try_from(i).ok()).collect())
}

/// The call of a hurt test over the collision crate's: the victim's place, its extra capsule and its off list.
fn with_victim<T>(
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
    f: impl FnOnce(&cb::Victim) -> T,
) -> T {
    let e = extra.map(ExtraHurt::tests);
    let o = off_list(off);
    let v = cb::Victim {
        vic,
        vx: f64::from(vx),
        vy: f64::from(vy),
        vf: f64::from(v_facing),
        extra: e.as_ref(),
        off: o.as_deref(),
    };
    f(&v)
}

// ---- the call surface of the box tests

/// The squared distance between the segment from a to b and the shape `s`.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn seg_box_dist2(s: &WorldSphere, ax: Real, ay: Real, az: Real, bx: Real, by: Real, bz: Real) -> Real {
    crate::math::narrow(cb::seg_box_dist2(&s.tests(), ax, ay, az, bx, by, bz))
}

/// Whether the shape `s` overlaps the box `b`.
pub fn shape_over_box(s: &WorldSphere, b: &WorldSphere) -> bool {
    cb::shape_over_box(&s.tests(), &b.tests())
}

/// Whether the shape `s` overlaps the upright capsule `e`.
pub fn shape_over_capsule(s: &WorldSphere, e: &ExtraHurt) -> bool {
    cb::shape_over_capsule(&s.tests(), &e.tests())
}

/// The end of the segment of hurt shape `i`.
pub fn hurt_end(boxes: &BoxSampler, i: i32) -> Real {
    let i = usize::try_from(i).expect("a shape");
    if boxes.tests.capsule[i] { 3.0 } else { 0.0 }
}

/// The radius of hurt shape `i`.
pub fn hurt_r(boxes: &BoxSampler, i: i32, vic: &[f32], o: Real) -> Real {
    let i = usize::try_from(i).expect("a shape");
    let o = frame_at(o);
    crate::math::narrow(
        boxes.tests.radius[i] * f64::from(crate::real(vic[o + if boxes.tests.capsule[i] { 6 } else { 3 }])),
    )
}

/// Whether a hit box touches a hurt shape of the victim.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn box_hits_hurt(
    boxes: &BoxSampler,
    cx: Real,
    cy: Real,
    hx: Real,
    hy: Real,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> bool {
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::box_hits_hurt(&boxes.tests, cx, cy, hx, hy, v))
}

/// [`box_hits_hurt`] over the victim's pose given as points.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn box_hits_hurt_points(
    boxes: &BoxSampler,
    cx: Real,
    cy: Real,
    hx: Real,
    hy: Real,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> bool {
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::box_hits_hurt_points(&boxes.tests, cx, cy, hx, hy, v))
}

/// The first hurt shape a hit shape touches, -1 none.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn first_hurt(
    boxes: &BoxSampler,
    s: &WorldSphere,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> Real {
    let s = s.tests();
    crate::real(with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::first_hurt(&boxes.tests, &s, v)))
}

/// [`first_hurt`] over the victim's pose given as points.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn first_hurt_points(
    boxes: &BoxSampler,
    s: &WorldSphere,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> Real {
    let s = s.tests();
    crate::real(with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::first_hurt_points(&boxes.tests, &s, v)))
}

/// The hurt shapes a hit shape touches.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn hurts_touched(
    boxes: &BoxSampler,
    s: &WorldSphere,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
    out: &mut Vec<Real>,
) -> Real {
    let s = s.tests();
    let mut got = Vec::new();
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::hurts_touched(&boxes.tests, &s, v, &mut got));
    out.clear();
    out.extend(got.iter().map(|&k| crate::real(k)));
    crate::real(to_u32(out.len()))
}

/// [`hurts_touched`] over the victim's pose given as points.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn hurts_touched_points(
    boxes: &BoxSampler,
    s: &WorldSphere,
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
    out: &mut Vec<Real>,
) -> Real {
    let s = s.tests();
    let mut got = Vec::new();
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::hurts_touched_points(&boxes.tests, &s, v, &mut got));
    out.clear();
    out.extend(got.iter().map(|&k| crate::real(k)));
    crate::real(to_u32(out.len()))
}

/// Whether any of a set of spheres hits a hurt shape of the victim.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn spheres_hit_hurt(
    boxes: &BoxSampler,
    spheres: &[WorldSphere],
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> Option<WorldSphere> {
    let ss: Vec<cb::Sphere> = spheres.iter().map(WorldSphere::tests).collect();
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::spheres_hit_hurt(&boxes.tests, &ss, v))
        .map(|n| spheres[n].clone())
}

/// [`spheres_hit_hurt`] over the victim's pose given as points.
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn spheres_hit_hurt_points(
    boxes: &BoxSampler,
    spheres: &[WorldSphere],
    vic: &[f32],
    vx: Real,
    vy: Real,
    v_facing: Real,
    extra: Option<&ExtraHurt>,
    off: Option<&IndexSet<i32>>,
) -> Option<WorldSphere> {
    let ss: Vec<cb::Sphere> = spheres.iter().map(WorldSphere::tests).collect();
    with_victim(vic, vx, vy, v_facing, extra, off, |v| cb::spheres_hit_hurt_points(&boxes.tests, &ss, v))
        .map(|n| spheres[n].clone())
}

// ---- a pawn's always-on HitBox spheres

/// `PawnHitSphere`: a pawn's always-on HitBox component (not a Hitframe's hit shape).
#[derive(Clone, PartialEq, Debug)]
pub struct PawnHitSphere {
    /// The component's template name (the data's key).
    pub name: String,
    /// SphereRadius.
    pub radius: Real,
    /// RelativeLocation.Z under the mesh.
    pub z: Real,
    /// The object types (traces' OBJ) it does not Ignore.
    pub types: Vec<i32>,
}

/// `pawnHitSpheres`: the pawn's always-on HitBox components, in the data's order: a component whose object type is
/// HitBox by the data alone (ObjectType ECC_GameTraceChannel3, or a Custom body with no ObjectType on the HitBox
/// profile) with its collision on (`bodyInfo`), a sphere on the mesh's axis. In the roster's data one component is
/// such: Pawn_Garnet's 'Sing Sphere' (radius 550 at (0, 0, 169.52382) under CharacterMesh0, QueryAndPhysics).
/// Made on each call (one list per data object would do): static data, read by a test with a buff's
/// sphere alone.
/// Not ported: a component of this kind under another parent, off the mesh's axis (RelativeLocation X or Y), or of
/// another shape than a sphere is warned and left out (none in the roster's data).
pub fn pawn_hit_spheres(w: &World, f: FighterId) -> Vec<PawnHitSphere> {
    let d = data_of(w, f);
    let mut out = Vec::new();
    for (name, c) in &d.fighter.components.pawn {
        let Value::Obj(p) = &c.p else { continue };
        let Some(Value::Obj(body)) = p.get("BodyInstance") else { continue };
        let ot = body.get("ObjectType");
        let hit_box = match ot {
            Some(Value::Str(t)) => t == "ECC_GameTraceChannel3",
            None => matches!(body.get("CollisionProfileName"), Some(Value::Str(n)) if n == "HitBox"),
            _ => false,
        };
        if !hit_box {
            continue;
        }
        let Some(info) = body_info(Some(p), None) else { continue };
        if info.kind != obj::HIT_BOX || !info.on {
            continue;
        }
        let loc = p.get("RelativeLocation").and_then(Value::as_obj);
        let part = |k: &str| loc.and_then(|l| l.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
        let radius = p.get("SphereRadius").and_then(Value::as_f64).unwrap_or(f64::from(Real::NAN));
        if radius.is_nan() || radius <= 0.0 || part("X") != 0.0 || part("Y") != 0.0 {
            let msg = format!(
                "pawnhitbox: {}'s {name} (a HitBox body with collision on) is no sphere on the mesh's axis: left out",
                d.key
            );
            warn_text(w, &msg);
            continue;
        }
        let types = (0..=obj::BLOCK_TRACE)
            .filter(|&t| response_to(Some(p), crate::real(t)) != crate::name!("ECR_Ignore"))
            .collect();
        out.push(PawnHitSphere {
            name: name.clone(),
            radius: crate::math::narrow(radius),
            z: crate::math::narrow(part("Z")),
            types,
        });
    }
    out
}

/// `pawnSpherePlace`: the component's world sphere in the play plane, from the fighter's mesh place (meshWX,
/// meshWY), its actor scale (xy, z) and its box index's mesh scale and mesh Z. Returns (x, y, r).
pub fn pawn_sphere_place(
    mesh_wx: Real,
    mesh_wy: Real,
    xy: Real,
    sz: Real,
    mesh_scale: Real,
    mesh_z: Real,
    d: &PawnHitSphere,
) -> [Real; 3] {
    let r = d.radius * mesh_scale * js::min(xy.abs(), sz.abs());
    let y = mesh_wy + ((mesh_z + d.z * mesh_scale) * sz);
    [mesh_wx, y, r]
}

// ---- a spawned actor's own mesh shapes

/// `actorBoxes`: the sampler of a spawned actor class's own tracks and the tables it reads: the owner's character
/// data's, else a peer's (`actorDef`'s rule). `None` (warned once) when no data has them.
pub fn actor_boxes(w: &World, owner: FighterId, cls: Name) -> Option<(&BoxSampler, BoxTables<'_>)> {
    let key = w.names.str(cls);
    let find = |f: FighterId| {
        let ch = w.fighter(f).ch;
        let d = char_data(w, ch);
        let idx = d.boxes.actors.as_ref()?.get(key)?;
        let s = w.fixed.samplers.get(&ch)?.actors.get(key)?;
        Some((s, BoxTables::of_actor(idx, &d.box_data)))
    };
    let found = find(owner).or_else(|| peers(w).iter().find_map(|&o| find(o)));
    if found.is_none() {
        warn_text(w, &format!("{key}: no actor box tracks in the data: no hit shapes"));
    }
    found
}

/// `pushActorShapes`: the spheres of the shapes `shapes` (of `bs`) in the pose `pose`, pushed onto `out`. The
/// actor at (x, y) facing `facing`; mirror: facing left is a reflection of the right-facing places (its depth
/// stays), else the actor turns. hit_scale: the HitBoxesOn call's HitBoxScale (the radius and a capsule's half
/// height take it, the place on the bone does not); actor_scale: the root's scale.
/// Deviation: a capsule is three spheres (its two ends and its middle, each of its radius: between two of them
/// along the axis it is a little narrower than the capsule).
/// Unverified: the sign of the depth axis against the port's z is taken as the mesh's (it only moves a sphere off
/// the play plane).
#[allow(clippy::too_many_arguments, reason = "the shape test's inputs")]
pub fn push_actor_shapes(
    out: &mut Vec<WorldSphere>,
    t: &BoxTables,
    bs: &BoxSampler,
    pose: &[f32],
    shapes: &[u32],
    x: Real,
    y: Real,
    facing: Real,
    mirror: bool,
    hit_scale: Real,
    actor_scale: Real,
) {
    let ds = (if mirror { 1.0 } else { facing }) * actor_scale;
    let fs = facing * actor_scale;
    let k = actor_scale;
    let p = |i: usize| crate::real(pose[i]);
    let mut put = |x: Real, y: Real, z: Real, r: Real| {
        out.push(WorldSphere { x, y, z: Some(z), r, ..WorldSphere::default() });
    };
    for &i in shapes {
        let s = &t.shapes[at(i)];
        let o = at(bs.offsets[at(i)]);
        if s.kind == BoxKind::Socket || s.is_box == Some(true) {
            // (a socket or a box is not handled)
            continue;
        }
        if !s.capsule {
            put(
                x + p(o) * fs,
                y + p(o + 1) * k,
                p(o + 2) * ds,
                crate::math::narrow(s.radius * f64::from(hit_scale) * f64::from(p(o + 3)) * f64::from(k)),
            );
            continue;
        }
        let f = (p(o) + p(o + 3)) / 2.0;
        let u = (p(o + 1) + p(o + 4)) / 2.0;
        let d = (p(o + 2) + p(o + 5)) / 2.0;
        let hf = ((p(o + 3) - p(o)) / 2.0) * hit_scale;
        let hu = ((p(o + 4) - p(o + 1)) / 2.0) * hit_scale;
        let hd = ((p(o + 5) - p(o + 2)) / 2.0) * hit_scale;
        let r = s.radius * f64::from(hit_scale) * f64::from(p(o + 6)) * f64::from(k);
        put(x + f * fs, y + u * k, d * ds, crate::math::narrow(r));
        put(x + (f - hf) * fs, y + (u - hu) * k, (d - hd) * ds, crate::math::narrow(r));
        put(x + (f + hf) * fs, y + (u + hu) * k, (d + hd) * ds, crate::math::narrow(r));
    }
}

/// `BoxSampler.sample` of character `ch`'s tables: the box tracks of `anim` at `time`
/// into `out` (a frame's `stride` numbers); false without the sequence (`out` untouched).
pub fn sample_of(w: &World, ch: Name, anim: &str, time: Real, out: &mut [f32]) -> bool {
    let d = char_data(w, ch);
    BoxSampler::sample(&BoxTables::of(&d.boxes, &d.box_data), anim, time, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(name: &str, kind: BoxKind, capsule: bool, tags: &[&str]) -> BoxShape {
        BoxShape {
            name: name.to_owned(),
            kind,
            bone: None,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            order: None,
            is_2d: false,
            capsule,
            abs_scale: None,
            radius: 10.0,
            half_height: 0.0,
            vbone: None,
            on_capsule: None,
            is_box: None,
            extent: None,
            abs_rot: None,
            missing: None,
            overlaps: None,
            unscaled: None,
            rot: None,
            scale: None,
            com: None,
            axes: None,
            axes_const: None,
        }
    }

    #[test]
    fn offsets_and_axes_follow_the_shapes() {
        let shapes = vec![
            shape("A", BoxKind::Hit, false, &["BodyHitBox"]),
            shape("B", BoxKind::Hurt, true, &[]),
            shape("S", BoxKind::Socket, false, &[]),
        ];
        let s =
            BoxSampler::build(&mut crate::names::Names::default(), &shapes, None, 2 + 4 + 7 + 2, true).expect("tables");
        assert_eq!(s.offsets, vec![2, 6, 13]);
        assert_eq!(s.hurt, vec![1]);
        assert_eq!(s.tagged("bodyhitbox"), &[0]);
        assert_eq!(s.axes, vec![0, 1, 0, 1, 0, 3, 0, 1, 0, 0, 1, 0, 3, 0, 1]);
        assert_eq!(s.shape_ch, vec![CH_ALL, CH_ALL, CH_ALL]);
    }

    #[test]
    fn sample_lerps_and_steps() {
        let shapes = vec![shape("A", BoxKind::Hit, false, &[])];
        let anims: Map<BoxAnim> = [(
            "M".to_owned(),
            BoxAnim { num_frames: 3, length: 1.0, rate_scale: 1.0, root_motion: false, offset: 0, step: None },
        )]
        .into_iter()
        .collect();
        let data: Vec<f32> = (0..18).map(|i| i as f32).collect();
        let t = BoxTables { stride: 6, shapes: &shapes, anims: &anims, socket_axes: None, data: &data };
        let mut out = [0f32; 6];
        assert!(BoxSampler::sample(&t, "M", 0.25, &mut out));
        assert_eq!(out, [3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(BoxSampler::root_into(&t, "M", 0.75), Some([9.0, 10.0]));
        assert!(!BoxSampler::sample(&t, "N", 0.0, &mut out));
    }
}
