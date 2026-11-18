//! The shield actors: `ShieldActor`, a PlatformActor that follows its owner and
//! blocks projectiles (Wonder Woman's WW_ShieldActor and its children, Steven's, Velma's), `SocketShieldActor` (a
//! child placed at a socket: [`ShieldActor::socket`]) and the shared `shield` kind the classes' entries take
//! ([`hooks`]: spawn, step, bodies, the projectile block and its visuals).
//!
//! WW_ShieldActor_C (WW_ShieldActor) and its shared part: a SpawnedPlatformParent child (`world/spawned_platform.rs`)
//! whose BlockCollision box stops the other team's projectiles. Users: Wonder Woman's side and up specials
//! (WW_ShieldActor_C, WW_ShieldActor_U_Special_C: `chars/wonder_woman/shield.rs`), her perk's WW_EvadeProjectileBlock
//! (`chars/wonder_woman/perks.rs`: [`ShieldHooks`] of its own), Steven's Steven_ShieldActor_U_SpecialPlatform and
//! Velma's Velma_ProjectileShieldActor (children of WW_ShieldActor).
//!
//! The SpawnedPlatformParent children (`WorldActor::plat` holds the parent's variables and its two boxes) have no hit
//! of their own: a BlockCollision box (profile ProjectileDestructible) that stops the other team's projectiles.
//! WW_ShieldActor_U_Special is a child of WW_ShieldActor with one function of its own, CollisionFunction, which
//! nothing calls (no graph, and the exe has no such string): every graph below is WW_ShieldActor's and runs for both.
//!
//! The shared code reaches a shield's place (`actors::world_actor`: [`place`]).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the negated tests keep a NaN

use std::sync::Arc;

use crate::Real;
use crate::State;
use crate::actors::projectile::{class_p, hit_collision, truthy};
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileId, WorldActorClass};
use crate::combat::boxes::{BoxSampler, BoxTables, WorldSphere};
use crate::events::Event;
use crate::fighter::anim::box_pose::{put_pose, sampled_boxes};
use crate::fighter::common::{actor_def, char_data, latent_frames, owner_team, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{OBJ_PROJECTILE_DESTRUCTIBLE, TraceBody, body_info, response_to};
use crate::fighter::{FighterId, actor_y, facing, mesh_wx, mesh_wy};
use crate::math::max;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawned_platform::{SpawnedPlatform, plat_mut, platform_end, platform_step};
use mvs_data::value::Props;

/// `OBJ.HitBox`: the object type index.
const OBJ_HIT_BOX: i32 = 7;

/// A box of the actor in the play plane: its centre from an origin on the owner (forward, up) and its half extents.
#[derive(State, Copy, Debug, Default, PartialEq)]
pub struct Placed {
    pub fwd: Real,
    pub up: Real,
    pub hx: Real,
    pub hy: Real,
}

/// `SocketShieldActor`'s own part: the shield on a socket, its boxes placed again each tick from the owner's pose,
/// about the mesh (the bone shapes' origin, `mesh_wx` / `mesh_wy`). The class's templates (`comps`) are read from its
/// data when asked.
#[derive(State, Debug)]
pub struct SocketPart {
    /// Whether the boxes are placed this tick (`live`).
    pub aligned: bool,
    /// The socket's shape in the owner's box data.
    pub socket: u32,
    /// The notify's Transforms.
    pub t: Arc<Props>,
}

/// `ShieldActor`: the actor's state. (The actor's own Comp_State_Manager buffs, charge values, lifetime pause, root
/// scale and SpawnedPlatform are the World actor's.)
#[derive(State, Debug, Default)]
pub struct ShieldActor {
    pub x: Real,
    pub y: Real,
    pub age: i32,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// ActivateObject @859.
    pub activated: bool,
    /// ProjectileBlocked (SetBlockedProjectileMulticast @2902: set, never cleared).
    pub projectile_blocked: bool,
    /// The age OnProjectileBlocked's do-once reopens at (Delay(0.2) @456).
    pub gate_age: Real,
    /// The numbers of the projectiles that overlap BlockCollision.
    pub inside: Vec<i32>,
    /// StartTime (ReceiveBeginPlay @2845-2874: GetGameTimeInSeconds at the spawn), read by
    /// BPI_IgnoreWallbounceCollision.
    pub start_time: Real,
    /// BlockCollision, and the parent's Collision and WallbounceCollision; `None`: the box is not placed (see
    /// `shield_spawn`).
    pub box_: Option<Placed>,
    pub collision: Option<Placed>,
    pub wall: Option<Placed>,
    /// `SocketShieldActor`: the shield on a socket.
    pub socket: Option<SocketPart>,
    /// The spawn's Transforms of a ShieldActor placed by [`place_box`] (the notify's, static data), for SetSizeScale
    /// (`shield_scale`).
    pub spawn_t: Option<Arc<Props>>,
}

/// What a child class overrides of the shield (`ShieldActor`'s methods): chosen by the actor's state ([`hooks_of`]).
#[derive(Clone, Copy, Debug)]
pub struct ShieldHooks {
    /// `meets(pr, b)`: whether BlockCollision (the box b, at the actor) overlaps the projectile this tick.
    pub meets: fn(&World, &ShieldActor, ProjectileId, &Placed) -> bool,
    /// `meetsComp(pr, comp)`: whether BlockCollision's profile meets `comp`, a projectile's hit box that one call turns
    /// on.
    pub meets_comp: fn(&World, ProjectileId, Name) -> bool,
    /// `blockVisuals(x, y)`: Block Projectile Hit Visuals(Location).
    pub block_visuals: fn(&mut World, ActorId, Real, Real),
}

/// The shield's own (WW_ShieldActor's) [`ShieldHooks`].
pub const SHIELD: ShieldHooks = ShieldHooks { meets, meets_comp, block_visuals };

/// The [`ShieldHooks`] of the actor's class: a child's own (by its state), else the shield's.
fn hooks_of(a: &WorldActor) -> ShieldHooks {
    match a.state {
        WorldActorState::EvadeShield(_) => crate::chars::wonder_woman::perks::EVADE,
        _ => SHIELD,
    }
}

/// The shield of a world actor of a ShieldActor class (WW_EvadeProjectileBlock's included): `a instanceof
/// ShieldActor`.
pub fn shield_of(a: &WorldActor) -> Option<&ShieldActor> {
    match &a.state {
        WorldActorState::ShieldActor(s) => Some(s),
        WorldActorState::EvadeShield(e) => Some(&e.base),
        _ => None,
    }
}

/// [`shield_of`], to write.
pub fn shield_mut(a: &mut WorldActor) -> Option<&mut ShieldActor> {
    match &mut a.state {
        WorldActorState::ShieldActor(s) => Some(s),
        WorldActorState::EvadeShield(e) => Some(&mut e.base),
        _ => None,
    }
}

fn sh(w: &World, id: ActorId) -> Option<&ShieldActor> {
    w.actors.actors.get(id).and_then(shield_of)
}

fn sh_mut(w: &mut World, id: ActorId) -> Option<&mut ShieldActor> {
    w.actors.actors.get_mut(id).and_then(shield_mut)
}

/// A shield's place (its `x`, `y`), `None` for none.
pub fn place(w: &World, a: &WorldActor, s: &ShieldActor) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// `live`: whether the boxes are placed this tick (a socket shield's `aligned`; always for the others).
pub fn live(s: &ShieldActor) -> bool {
    s.socket.as_ref().is_none_or(|k| k.aligned)
}

// ------------------------------------------------------------------ the transforms

/// A notify's Transforms read as numbers: Translation (X, Y, Z), Rotation (X, Y, Z, W) and Scale3D (X, Y, Z), each
/// member at its default when absent.
#[derive(Clone, Copy, Debug)]
struct Tf {
    tr: [Real; 3],
    q: [Real; 4],
    s: [Real; 3],
}

impl Tf {
    fn of(t: &Props) -> Tf {
        let v = |k: &str, d: [Real; 3]| {
            let o = obj(t, k);
            let g = |a: &str, d: Real| o.and_then(|o| num(o, a)).unwrap_or(d);
            [g("X", d[0]), g("Y", d[1]), g("Z", d[2])]
        };
        let q = obj(t, "Rotation");
        let g = |a: &str, d: Real| q.and_then(|o| num(o, a)).unwrap_or(d);
        Tf {
            tr: v("Translation", [0.0; 3]),
            q: [g("X", 0.0), g("Y", 0.0), g("Z", 0.0), g("W", 1.0)],
            s: v("Scale3D", [1.0; 3]),
        }
    }

    /// `atScale`: the Transforms with the root's world scale SetSizeScale wrote in place of Scale3D (SetActorScale3D on
    /// the root, which has bAbsoluteScale: the scale is the root's world scale whole); itself without one.
    fn at_scale(self, k: Option<Real>) -> Tf {
        match k {
            Some(k) => Tf { s: [k; 3], ..self },
            None => self,
        }
    }
}

/// A vector member of a component template (`comp[k] ?? {}`), each axis at `d` when absent.
fn vec3(c: &Props, k: &str, d: Real) -> [Real; 3] {
    let o = obj(c, k);
    let g = |a: &str| o.and_then(|o| num(o, a)).unwrap_or(d);
    [g("X"), g("Y"), g("Z")]
}

/// `placeBox`: a box component under the root of an actor attached to its owner (no socket) at the Transforms `t`:
/// its centre (forward, up) and half extents in the play plane; `None` for a component without a BoxExtent.
pub fn place_box(comp: Option<&Props>, t: &Props) -> Option<Placed> {
    place_box_tf(comp, &Tf::of(t))
}

fn place_box_tf(comp: Option<&Props>, t: &Tf) -> Option<Placed> {
    let comp = comp.filter(|c| flag(c, "BoxExtent"))?;
    let (sx, sz) = (t.s[0], t.s[2]);
    let (l, rs, e) =
        (vec3(comp, "RelativeLocation", 0.0), vec3(comp, "RelativeScale3D", 1.0), vec3(comp, "BoxExtent", 0.0));
    Some(Placed { fwd: t.tr[0] + l[0] * sx, up: t.tr[2] + l[2] * sz, hx: e[0] * rs[0] * sx, hy: e[2] * rs[2] * sz })
}

// ------------------------------------------------------------------ the socket

// The up specials (WonderWoman_ShieldRamUp_Montage, WonderWoman_Air_ShieldRamUp_Montage) attach
// WW_ShieldActor_U_Special to HitCollision01Socket, which the mesh has: Comp_Spawner HandleActorPostSpawn
// K2_AttachToComponent(mesh, socket) @10581 (Location SnapToTarget, Rotation and Scale KeepWorld: the class defaults),
// then K2_SetActorRelativeTransform(the notify's Transforms) @10515, which writes all three parts of the root's relative
// transform. The root Capsule has bAbsoluteScale, so (exe 14298b070):
//   actor origin  = socket + socketRot(socketScale x Translation)
//   root rotation = socketRot x Rotation;  root world scale = Scale3D (the socket's scale does not reach it)
//   a box under the root: centre = origin + rootRot(Scale3D x RelativeLocation),
//                         half extents = BoxExtent x Scale3D x RelativeScale3D along the root's axes.
// The socket's place and axes are the owner's pose of the tick (the box tracks' socket and BoxSampler.axesInto, whose
// axes carry the socket's world scale as their length), at the actor's size.
// With the data's numbers: axes of length 2.018 with X forward and Z up over both Spawner windows, Translation
// (40, 0, 45), Rotation -90 degrees about Y (the root's X is the socket's Z, its Z the socket's -X), Scale3D
// (0.4, 0.3, 0.3); BlockCollision and Collision BoxExtent (175, 275, 275), WallbounceCollision 200 cubed, each at
// (0, 0, 150) with RelativeScale3D (0.3, 1, 1.25). So the origin is 80.7 forward of the socket and 90.8 over it, a
// box's centre 45 behind that, and the boxes lie flat over her head: 103.1 half wide and 21 half thick (the wall 75
// and 24).
// Facing left the build's bone is F_B x the reflected pose's partner bone (FAnimNode_MirrorBoneTable;
// WonderWoman_MirrorTable: HitCollision01 is its own partner with BoneFlipAxis Y): the socket's Y, the depth, is
// negated and the play plane is the reflection the box tracks are placed by.
// A part of the port is an upright box in the play plane (spawned_platform.rs): a tick whose root axes are not along
// forward, up and depth places nothing and warns (none in the two montages).
// What the class data makes of the parent's boxes here: DelayCollisionEnabled, so Collision (profile Platform) is a
// platform from Delay(0.2) on; IgnoreEnemies, so only her team stands on it (the parent's
// BPI_IgnorePlatformCollision); UseWallbounceCollision with WallbounceAllies / WallbounceEnemies, the wall of
// `shield_ignore_wallbounce` (after KnockbackCollisionBufferTime 1.0 of the notify's 1.2 to 1.28 s).
// Unverified: BlockCollision's template in this class names the profile ProjectileDestructible and lists other
// responses than the profile's (KnockBack and ProjectileGroundTrace Block, WorldDynamic and Pawn Overlap, nothing for
// HitBox; WW_ShieldActor's list is the profile's own). Taken as the profile's, as the parent class has them: it
// overlaps HitBox components (the projectiles shield_step tests), stops no capsule in knockback and no ground trace.
// Which of the two a body takes at load (FBodyInstance's profile against its ResponseArray) is not read in the exe; by
// the list alone no projectile component would overlap the box.

/// The actor's origin from the mesh (fwd, up) and the root's unit axes X, Y, Z as (fwd, up, depth) each.
#[derive(Clone, Copy, Debug)]
struct Frame {
    org: [Real; 2],
    rt: [Real; 9],
}

/// `socketFrame`: the actor's origin and root axes from the owner's pose; `None` when the pose or the axes are not in
/// the data, or facing left on a bone the mirror rule above is not written for.
fn socket_frame(w: &World, f: FighterId, si: u32, t: &Tf) -> Option<Frame> {
    let fm = w.fighter(f);
    let cd = char_data(w, fm.ch);
    let pose = sampled_boxes(w, f)?;
    let axes = BoxSampler::axes_into(
        &BoxTables::of(&cd.boxes, &cd.box_data),
        si,
        w.names.str(fm.anim.anim),
        fm.anim.anim_time,
    );
    let Some(mut ax) = axes else {
        put_pose(w, pose);
        return None;
    };
    let o = w.fixed.samplers.get(&fm.ch).and_then(|b| b.offsets.get(si as usize).copied());
    let Some(o) = o.map(|o| o as usize) else {
        put_pose(w, pose);
        return None;
    };
    let (sf, su) = (crate::real(pose[o]), crate::real(pose[o + 1]));
    put_pose(w, pose);
    let k = fm.body.size_scale;
    if k != 1.0 {
        for a in &mut ax {
            *a *= k;
        }
    }
    if facing(w, f) < 0.0 {
        let sk = &cd.fighter.skeleton;
        let bone = cd.boxes.shapes.get(si as usize).and_then(|s| s.bone.as_deref()).unwrap_or("");
        let flip = sk.flip.as_ref().and_then(|m| m.get(bone)).map_or("Y", String::as_str);
        let partner = sk.mirror.get(bone).map_or(bone, String::as_str);
        if partner != bone || sk.mirror_node == Some(false) || !matches!(flip, "X" | "Y" | "Z" | "XYZ") {
            return None;
        }
        for (j, a) in ["X", "Y", "Z"].iter().enumerate() {
            if flip == "XYZ" || flip == *a {
                for c in 0..3 {
                    ax[j * 3 + c] = -ax[j * 3 + c];
                }
            }
        }
    }
    let [tx, ty, tz] = t.tr;
    let org = [sf + tx * ax[0] + ty * ax[3] + tz * ax[6], su + tx * ax[1] + ty * ax[4] + tz * ax[7]];
    let len = |i: usize| (ax[i] * ax[i] + ax[i + 1] * ax[i + 1] + ax[i + 2] * ax[i + 2]).sqrt();
    let (lx, ly, lz) = (len(0), len(3), len(6));
    if !(lx != 0.0) || !(ly != 0.0) || !(lz != 0.0) {
        return None;
    }
    // The quaternion's images of X, Y and Z in the socket's frame (FQuat::RotateVector), on the socket's unit axes.
    let [x, y, z, qw] = t.q;
    let mut rt = [0.0; 9];
    for c in 0..3 {
        let (ux, uy, uz) = (ax[c] / lx, ax[3 + c] / ly, ax[6 + c] / lz);
        rt[c] = (1.0 - 2.0 * (y * y + z * z)) * ux + 2.0 * (x * y + qw * z) * uy + 2.0 * (x * z - qw * y) * uz;
        rt[3 + c] = 2.0 * (x * y - qw * z) * ux + (1.0 - 2.0 * (x * x + z * z)) * uy + 2.0 * (y * z + qw * x) * uz;
        rt[6 + c] = 2.0 * (x * z + qw * y) * ux + 2.0 * (y * z - qw * x) * uy + (1.0 - 2.0 * (x * x + y * y)) * uz;
    }
    Some(Frame { org, rt })
}

/// `socketBox`: a box component under that root; `None` for a box that is not upright in the play plane (or has a
/// rotation of its own: none of the three has).
fn socket_box(comp: Option<&Props>, t: &Tf, fr: &Frame) -> Option<Placed> {
    let comp = comp.filter(|c| flag(c, "BoxExtent") && !flag(c, "RelativeRotation"))?;
    let (l, cs, e) =
        (vec3(comp, "RelativeLocation", 0.0), vec3(comp, "RelativeScale3D", 1.0), vec3(comp, "BoxExtent", 0.0));
    let (mut fwd, mut up, mut hx, mut hy) = (fr.org[0], fr.org[1], 0.0, 0.0);
    for k in 0..3 {
        let s = t.s[k];
        let (at, h) = (l[k] * s, e[k] * s * cs[k]);
        let (rf, ru, rd) = (fr.rt[k * 3], fr.rt[k * 3 + 1], fr.rt[k * 3 + 2]);
        let n = [rf, ru, rd].iter().filter(|v| v.abs() > 1e-3).count();
        if n != 1 {
            return None;
        }
        fwd += at * rf;
        up += at * ru;
        hx += (h * rf).abs();
        hy += (h * ru).abs();
    }
    Some(Placed { fwd, up, hx, hy })
}

/// The class's component template `k` (BlockCollision, Collision, WallbounceCollision) in the owner's data.
fn comp_of<'a>(w: &'a World, owner: FighterId, cls: Name, k: &str) -> Option<&'a Props> {
    actor_def(w, ActorRef::Fighter(owner), cls).and_then(|d| obj(&d.components, k))
}

// ------------------------------------------------------------------ the place

/// `follow`: the boxes placed from their origin on the owner: his actor location (attached to the actor itself), or
/// for a socket shield his socket on the mesh, placed again from the pose of the tick.
pub fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = shield_of(a) else { return };
    let (f, cls) = (a.owner, a.cls);
    let Some(sock) = &s.socket else {
        let (ox, oy) = (w.fighter(f).body.x, actor_y(w, f));
        put(w, id, ox, oy);
        return;
    };
    let (si, t) = (sock.socket, Tf::of(&sock.t));
    let ts = t.at_scale(a.size_scale);
    // (each box is written as it is placed, up to the first that is not: an && chain over the actor's boxes)
    let mut placed: [Option<Placed>; 3] = [None; 3];
    let ok = socket_frame(w, f, si, &t).is_some_and(|fr| {
        for (i, k) in ["BlockCollision", "Collision", "WallbounceCollision"].iter().enumerate() {
            placed[i] = socket_box(comp_of(w, f, cls, k), &ts, &fr);
            if placed[i].is_none() {
                return false;
            }
        }
        true
    });
    let Some(s) = sh_mut(w, id) else { return };
    for (slot, p) in [&mut s.box_, &mut s.collision, &mut s.wall].into_iter().zip(placed) {
        if let Some(p) = p {
            *slot = Some(p);
        }
    }
    if let Some(k) = &mut s.socket {
        k.aligned = ok;
    }
    let (col, wall) = (s.collision, s.wall);
    if let Some(pl) = plat_mut(w, id) {
        pl.listed = ok;
    }
    if !ok {
        crate::fighter::common::warn_parts(
            w,
            &[w.names.str(cls), ": socket boxes not upright in the play plane: not placed"],
        );
        return;
    }
    if let Some(pl) = plat_mut(w, id) {
        if let (Some(c), Some(p)) = (col, pl.collision_part()) {
            p.hx = c.hx;
            p.hy = c.hy;
        }
        if let (Some(c), Some(p)) = (wall, pl.wallbounce_part()) {
            p.hx = c.hx;
            p.hy = c.hy;
        }
    }
    let (ox, oy) = (mesh_wx(w, f), mesh_wy(w, f));
    put(w, id, ox, oy);
}

/// `put(ox, oy)`: the actor and its parts placed from the origin (ox, oy).
fn put(w: &mut World, id: ActorId, ox: Real, oy: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let fc = facing(w, a.owner);
    let Some(a) = w.actors.actors.get_mut(id) else { return };
    let Some(s) = shield_mut(a) else { return };
    s.x = ox + s.box_.map_or(0.0, |b| b.fwd * fc);
    s.y = oy + s.box_.map_or(0.0, |b| b.up);
    let (c, wl) = (s.collision, s.wall);
    let Some(pl) = a.plat.as_mut() else { return };
    if let Some(c) = c
        && let Some(p) = pl.collision_part()
    {
        p.place(ox + c.fwd * fc, oy + c.up);
    }
    if let Some(c) = wl
        && let Some(p) = pl.wallbounce_part()
    {
        p.place(ox + c.fwd * fc, oy + c.up);
    }
}

// ------------------------------------------------------------------ the spawn

// CompSpawnActor with AttachToActor. WW_ShieldRamForward names socket 'Shield', which the mesh has not (neither a bone
// nor a socket of that name, nor of its mirrored name): Comp_Spawner GetSocket @0-960 answers None, so
// HandleActorPostSpawn attaches the actor to the owner itself, K2_AttachToActor(owner, None) @10335, and sets the
// notify's Transforms as the actor's relative transform @10515: Translation (120, 0, -150) in the owner's frame (X
// forward), Scale3D (0.6, 1, 1) as the world scale of the root Capsule (bAbsoluteScale). A box of the three
// (BlockCollision, Collision, WallbounceCollision: BoxExtent 200, RelativeLocation (0, 0, 150), RelativeScale3D
// (0.3, 1, 1.25) each) then stands 120 in front of the owner's actor location at its height, 36 thick and 250 up and
// down.
// Unverified: USkinnedMeshComponent::DoesSocketExist is native and not read; "the mesh has no Shield" is the skeleton's
// bone and socket lists.
// What the parent's boxes do for WW_ShieldActor (class data): Collision is NoCollision and has no Delay to turn it on,
// so the actor is no platform; WallbounceCollision (object type Platform, QueryAndPhysics, UseWallbounceCollision /
// WallbounceAllies / WallbounceEnemies true) is a wall for the fighters in hitstun (`shield_ignore_wallbounce`). It
// leaves ProjectileGroundTrace at the channel's default and ignores Item and PhysicsBody: no other query meets it.
// Not ported: ReceiveAnyDamage @1736 (the block visuals at the actor; the actor's Comp_Actor_Destructible has
// DamageDisabled).

/// A new shield of `owner` with its boxes (`None`: not placed): the constructor's fields (its SpawnedPlatform is
/// made by [`spawned`] once the World holds the actor).
pub fn new_shield(
    w: &World,
    owner: FighterId,
    b: Option<Placed>,
    c: Option<Placed>,
    wl: Option<Placed>,
) -> ShieldActor {
    ShieldActor {
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        start_time: w.fighter(owner).clock.time,
        box_: b,
        collision: c,
        wall: wl,
        ..ShieldActor::default()
    }
}

/// The shield's place at its spawn without its parts (`follow`, which [`spawned`] runs again with them): the origin
/// on the owner and the box's offset.
fn spawn_place(w: &World, f: FighterId, s: &mut ShieldActor) {
    let fc = facing(w, f);
    let (ox, oy) = match &s.socket {
        None => (w.fighter(f).body.x, actor_y(w, f)),
        Some(_) => (mesh_wx(w, f), mesh_wy(w, f)),
    };
    s.x = ox + s.box_.map_or(0.0, |b| b.fwd * fc);
    s.y = oy + s.box_.map_or(0.0, |b| b.up);
}

/// `shieldSpawn`: the class's spawn (the kind's `spawn`).
fn shield_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms").cloned().unwrap_or_default();
    let tf = Tf::of(&t);
    let q = tf.q;
    let socket = text(&np, "Socket");
    let si = match socket {
        None | Some("" | "None") => None,
        Some(s) => {
            let ch = w.fighter(f).ch;
            w.names.get(&format!("socket:{s}")).and_then(|n| w.fixed.samplers.get(&ch)?.by_name.get(&n).copied())
        }
    };
    let turned = q[0].abs() > 1e-4 || q[1].abs() > 1e-4 || q[2].abs() > 1e-4;
    let def = actor_def(w, ActorRef::Fighter(f), r.cls);
    let c = |k: &str| def.and_then(|d| obj(&d.components, k));
    let mut s: Option<ShieldActor> = None;
    if flag(&np, "AttachToActor") && c("BlockCollision").is_some_and(|b| flag(b, "BoxExtent")) {
        match si {
            None => {
                if !turned {
                    let (b, col, wl) = (
                        place_box_tf(c("BlockCollision"), &tf),
                        place_box_tf(c("Collision"), &tf),
                        place_box_tf(c("WallbounceCollision"), &tf),
                    );
                    let mut n = new_shield(w, f, b, col, wl);
                    n.spawn_t = Some(Arc::new(t.clone()));
                    s = Some(n);
                }
            }
            Some(si) => {
                if c("Capsule").is_some_and(|k| flag(k, "bAbsoluteScale"))
                    && let Some(fr) = socket_frame(w, f, si, &tf)
                    && let Some(b) = socket_box(c("BlockCollision"), &tf, &fr)
                    && let Some(col) = socket_box(c("Collision"), &tf, &fr)
                    && let Some(wl) = socket_box(c("WallbounceCollision"), &tf, &fr)
                {
                    let mut n = new_shield(w, f, Some(b), Some(col), Some(wl));
                    n.socket = Some(SocketPart { aligned: true, socket: si, t: Arc::new(t.clone()) });
                    s = Some(n);
                }
            }
        }
    }
    let mut s = s.unwrap_or_else(|| {
        let sock = socket.unwrap_or("undefined");
        let msg =
            format!("{}: boxes not placed (socket {sock}): it stops no projectile and no fighter", w.names.str(r.cls));
        warn_text(w, &msg);
        new_shield(w, f, None, None, None)
    });
    spawn_place(w, f, &mut s);
    let dead = shield_blast_loop(w, f, r.cls, &mut s);
    (WorldActorState::ShieldActor(s), dead)
}

/// `shieldBlastLoop`: SpawnedActorParent's BeginPlay blast-box loop on a placed shield, for every spawn of the class
/// and its children (`shield_step` runs the CheckBlastBoxTimer); true: the loop destroyed it.
pub fn shield_blast_loop(w: &World, f: FighterId, cls: Name, s: &mut ShieldActor) -> bool {
    let p = actor_def(w, ActorRef::Fighter(f), cls).map(|d| &d.p);
    let force = p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox"));
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    blast_box_loop(&spawn_box(w), &mut s.x, &mut s.y, None, force, &mut s.blast_bounds, doe)
}

/// The rest of the spawn once the World holds the actor (the kind's `spawned`): the constructor's SpawnedPlatform
/// with the half extents of the placed Collision and WallbounceCollision, then `follow` with its parts.
pub fn spawned(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = shield_of(a) else { return };
    let (f, cls) = (a.owner, a.cls);
    let (c, wl) = (s.collision.map(|p| (p.hx, p.hy)), s.wall.map(|p| (p.hx, p.hy)));
    let pl = SpawnedPlatform::new(w, f, cls, c, wl);
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.plat = Some(pl);
    }
    follow(w, id);
}

// ------------------------------------------------------------------ the kind

/// `shieldIgnoreWallbounce`: BPI_IgnoreWallbounceCollision(Object, Ignore) @1881-2156, WW_ShieldActor's own
/// (WW_ShieldActor_U_Special has none of its own: this one): the parent's call @2128 with Ignore || Object ==
/// GetOwner() @1901-1939 || GetGameTimeInSeconds - StartTime <= KnockbackCollisionBufferTime (1.0) @1977-2090. So the
/// wall is never there for her who holds the shield, nor for anyone in the actor's first second.
/// The clock is the world's (the owner's clock, float32), read where the port makes the call
/// (`spawned_platform::wallbounce_tick`).
fn shield_ignore_wallbounce(w: &mut World, id: ActorId, f: FighterId, ignore: bool) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = shield_of(a) else { return };
    let (own, cls) = (a.owner, a.cls);
    let buffer = actor_def(w, ActorRef::Fighter(own), cls).and_then(|d| num(&d.p, "KnockbackCollisionBufferTime"));
    let early = (w.fighter(own).clock.time - s.start_time) <= buffer.unwrap_or(0.0);
    crate::world::spawned_platform::ignore_wallbounce_collision(w, id, f, ignore || f == own || early);
}

/// `meets` (ShieldActor's): BlockCollision (the box b, at the actor) against the projectile's hit circle with the hit
/// box on (the note at `shield_step`).
pub fn meets(w: &World, s: &ShieldActor, pr: ProjectileId, b: &Placed) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    if !hit_collision(p) {
        return false;
    }
    let dx = max((p.x - s.x).abs() - b.hx, 0.0);
    let dy = max((p.y - s.y).abs() - b.hy, 0.0);
    dx * dx + dy * dy <= p.hit_radius * p.hit_radius
}

/// `meetsComp` (ShieldActor's): BlockCollision's profile ProjectileDestructible (object type ProjectileDestructible,
/// HitBox: Overlap) against a component of the HitBox object type that overlaps ProjectileDestructible.
pub fn meets_comp(w: &World, pr: ProjectileId, comp: Name) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    let c = p
        .owner
        .and_then(|o| actor_def(w, ActorRef::Fighter(o), p.cls))
        .and_then(|d| obj(&d.components, w.names.str(comp)));
    body_info(c, None).is_some_and(|i| i.kind == OBJ_HIT_BOX)
        && response_to(c, crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)) != name!("ECR_Ignore")
}

/// `blockVisuals` (ShieldActor's): Block Projectile Hit Visuals(Location) (@1746, the parent's: OnProjectileBlocked
/// @116 calls it inside the do-once, ahead of SetBlockedProjectileMulticast): sfx_cha_wonderwoman_bracerblock at the
/// location @1766 and PS_HitImpact_Directional (scale 1.5) there @1650.
pub fn block_visuals(w: &mut World, id: ActorId, x: Real, y: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (cls, fid) = (a.cls, w.fighter(a.owner).id);
    let ev = name!("sfx_cha_wonderwoman_bracerblock");
    w.events.push(Event::Sound { fighter: fid, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_HitImpact_Directional")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.5,
        tint: None,
        to: None,
    });
}

/// `shieldStep`: the actor's tick: the parent's, the boxes placed, CheckBlastBoxTimer, then BlockCollision's overlaps.
///
/// BlockCollision's begin overlap (WW_ShieldActor @2157): the other actor a SpawnedActorParent; nothing for one of the
/// shield's team (Comp_Actor_Destructible.TeamIndex @2232-2350), for an environmental hazard without
/// BlockEnvironmentalHazards @2351-2410 (none ported), or for a Heavy one (BPI_GetProjectileAttackType 1) without
/// BlockHeavyProjectiles @2420-2510 (set on both classes). Then BlockHitLocation = the projectile's location @2570 and
/// the projectile's Blocked(the shield's owner) @2617 (`combat::hits::blocked`: SpawnedActorParent's event, or the
/// class's own); without the projectile's DestroyOnBlocked the rest is skipped @2662; else the owner's
/// BPI_BlockedProjectile @2793 (the meter) and OnProjectileBlocked @1857.
/// OnProjectileBlocked @523, a do-once that Delay(0.2) reopens: sfx_cha_wonderwoman_bracerblock and
/// PS_HitImpact_Directional (scale 1.5) at the location @116, ProjectileBlocked = true @139 and, with
/// MontageSectionJumpOnHit (false on both classes), a section jump.
// Unverified: the overlap is taken as the projectile's hit circle (its hitbox on) against the box in the play plane,
// begun when it was not inside the tick before; the profile's responses to a projectile's other components (HurtBox,
// root) are not read (`meets`; WW_EvadeProjectileBlock's OverlapAllDynamic box meets the root instead:
// chars/wonder_woman/perks.rs). The projectiles of the World's projectile list only (no item, no other actor).
pub fn shield_step(w: &mut World, id: ActorId) {
    let Some(s) = sh_mut(w, id) else { return };
    s.age += 1;
    platform_step(w, id); // the parent's tick
    follow(w, id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let hooks = hooks_of(a);
    let doe = actor_def(w, ActorRef::Fighter(a.owner), a.cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let Some(s) = sh_mut(w, id) else { return };
    let frames = crate::real(s.age - 1);
    if blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, frames, s.x, s.y) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    let Some(b) = s.box_.filter(|_| live(s)) else { return };
    let mut inside = std::mem::take(&mut s.inside);
    let projectiles = &w.actors.projectiles;
    for i in (0..inside.len()).rev() {
        if !projectiles.iter().any(|(_, p)| p.seq == inside[i] && !p.dead) {
            inside.remove(i);
        }
    }
    if let Some(s) = sh_mut(w, id) {
        s.inside = inside;
    }
    // (a begin overlap can end other projectiles and list new ones: the list is read as it stands)
    let mut k = 0;
    while let Some(pr) = w.actors.projectiles.nth(k) {
        k += 1;
        let Some(p) = w.actors.projectiles.get(pr) else { continue };
        if p.dead {
            continue;
        }
        let seq = p.seq;
        let Some(s) = sh(w, id) else { return };
        let at = s.inside.iter().position(|&x| x == seq);
        if !(hooks.meets)(w, s, pr, &b) {
            if let (Some(at), Some(s)) = (at, sh_mut(w, id)) {
                s.inside.remove(at);
            }
            continue;
        }
        if at.is_some() {
            continue;
        }
        if let Some(s) = sh_mut(w, id) {
            s.inside.push(seq);
        }
        shield_begin(w, id, pr);
    }
}

/// BlockCollision's begin overlap @2157 for a SpawnedActorParent: the team test @2232 (the hazard test @2351 is not
/// taken here), the heavy test @2420, Blocked(GetOwner()) @2617, then with DestroyOnBlocked the owner's
/// BPI_BlockedProjectile @2793 and OnProjectileBlocked(the other's actor location @2520) @1857.
fn shield_begin(w: &mut World, id: ActorId, pr: ProjectileId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    if owner_team(w, ActorRef::Projectile(pr)) == w.fighter(f).team.team {
        return;
    }
    let heavy_ok = actor_def(w, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "BlockHeavyProjectiles"));
    if p.attack_type == 1 && !heavy_ok {
        return;
    }
    let destroy = truthy(class_p(w, p).get("DestroyOnBlocked"));
    let (x0, y0) = (p.x, p.y);
    crate::combat::hits::blocked(w, ActorRef::Projectile(pr), ActorRef::Fighter(f));
    if !destroy {
        return;
    }
    if let Some(h) = w.fighter(f).hooks.char.blocked_projectile {
        h(w, f);
    }
    // (the projectile's place after Blocked, or its place before it when Blocked removed the projectile)
    let (x, y) = w.actors.projectiles.get(pr).map_or((x0, y0), |p| (p.x, p.y));
    on_projectile_blocked(w, id, x, y);
}

/// `ActorKind.hitBoxOn`: a projectile's hit box that lives inside one call (C016_SassBubble's Box:
/// `chars/lebron/bubble.rs`) against BlockCollision where the owner stands at that call (attached to him: re-placed by
/// `follow`, the owner's actor location or, for a socket shield, his socket on the mesh). Both in the play plane. The
/// pair is listed in `inside`, so the call's second overlap update begins nothing again; the step's next pass ends it
/// (`meets`: that projectile's hit circle is off), as the box's destroy does in the build.
fn shield_box_on(w: &mut World, id: ActorId, pr: ProjectileId, comp: Name, s: &WorldSphere) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let hooks = hooks_of(a);
    let Some(sa) = shield_of(a) else { return };
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let Some(v) = s.bx else { return };
    if sa.box_.is_none() || a.dead || p.dead || sa.inside.contains(&p.seq) || !(hooks.meets_comp)(w, pr, comp) {
        return;
    }
    let seq = p.seq;
    follow(w, id);
    let Some(sa) = sh(w, id) else { return };
    let Some(b) = sa.box_.filter(|_| live(sa)) else { return };
    let hx = v[0].abs() + v[3].abs() + v[6].abs();
    let hy = v[1].abs() + v[4].abs() + v[7].abs();
    if (s.x - sa.x).abs() > b.hx + hx || (s.y - sa.y).abs() > b.hy + hy {
        return;
    }
    if let Some(sa) = sh_mut(w, id) {
        sa.inside.push(seq);
    }
    shield_begin(w, id, pr);
}

/// `onProjectileBlocked(Location)` @523: a do-once that Delay(0.2) reopens; the block visuals at the location,
/// ProjectileBlocked = true. Callers: `shield_begin`, Velma's beam (a beam's cast ending on the shield).
pub fn on_projectile_blocked(w: &mut World, id: ActorId, x: Real, y: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let hooks = hooks_of(a);
    let (f, cls) = (a.owner, a.cls);
    let Some(s) = sh_mut(w, id) else { return };
    if crate::real(s.age) < s.gate_age {
        return;
    }
    s.gate_age = crate::real(s.age) + latent_frames(0.2);
    (hooks.block_visuals)(w, id, x, y);
    if let Some(s) = sh_mut(w, id) {
        s.projectile_blocked = true;
    }
    if actor_def(w, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "MontageSectionJumpOnHit")) {
        warn_text(w, &format!("{}: MontageSectionJumpOnHit not ported", w.names.str(cls)));
    }
}

/// `shieldBodies`: BlockCollision as a body of the traces (`ActorKind.bodies`): the box `shield_step` tests, at the
/// actor in the play plane (`put`), extruded along the depth, while it is placed. Object type and responses are the
/// profile's (ProjectileDestructible: object type 11, no trace channel blocked), as the note above takes them for
/// WW_ShieldActor_U_Special_C too (its own list would block ProjectileGroundTrace). The parent's Collision and
/// WallbounceCollision are the solids the traces gather already; Capsule has no collision. What meets it: the object
/// queries for ProjectileDestructible (the Earth Smash pillar's overlap, C021_ForceField's), and a dealer's hit box
/// that overlaps ProjectileDestructible: its handler's pfgDoDamage on the shield (a projectile's or an item's hit body
/// through `ActorEntry::spawned_meet_bodies`, an actor's hit box through `hit_box_actor_hits`, the sass bubbles' Box).
/// DamageDisabled (WW_ShieldActor's Comp_Actor_Destructible) makes DamageAndLaunch refuse every one; a team mate's
/// dealer gives its ally rows @5931-6835 (WonderWoman_MeterBracerClash's Buff_ArmorPack, an actor buff with no hooks).
fn shield_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = shield_of(a) else { return };
    let Some(b) = s.box_.filter(|_| live(s)) else { return };
    let comp = comp_of(w, a.owner, a.cls, "BlockCollision");
    let Some(own) = body_info(comp, None).filter(|i| i.on) else { return };
    let Some(info) = body_info(None, Some(own.profile)) else { return };
    out.push(TraceBody {
        ax: s.x,
        ay: s.y,
        bx: s.x,
        by: s.y,
        hx: b.hx,
        hy: b.hy,
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("BlockCollision"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// `shieldScale`: Comp_SizeManager.SetSizeScale (`ActorKind.size`): the root's world scale is the size (absolute, in
/// place of the spawn's Scale3D), so each box's place on the root and its extents are `place_box`'s at that scale. A
/// socket shield reads it each tick (`follow`).
fn shield_scale(w: &mut World, id: ActorId, _new: Real, _old: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let Some(s) = shield_of(a) else { return };
    if s.socket.is_some() {
        follow(w, id);
        return;
    }
    if s.box_.is_none() && s.collision.is_none() && s.wall.is_none() {
        return; // not placed (the spawn's warning): no box to scale
    }
    let (f, cls) = (a.owner, a.cls);
    let Some(t) = s.spawn_t.clone() else {
        let msg =
            format!("{}: SetSizeScale on a shield with no spawn transform kept: its boxes stay", w.names.str(cls));
        warn_text(w, &msg);
        return;
    };
    let ts = Tf::of(&t).at_scale(a.size_scale);
    let n = ["BlockCollision", "Collision", "WallbounceCollision"].map(|k| place_box_tf(comp_of(w, f, cls, k), &ts));
    let Some(s) = sh_mut(w, id) else { return };
    let mut parts = [None, None];
    for (i, slot) in [&mut s.box_, &mut s.collision, &mut s.wall].into_iter().enumerate() {
        if let (Some(p), Some(n)) = (slot.as_mut(), n[i]) {
            *p = n;
            if i > 0 {
                parts[i - 1] = Some(n);
            }
        }
    }
    if let Some(pl) = plat_mut(w, id) {
        if let (Some(n), Some(p)) = (parts[0], pl.collision_part()) {
            p.hx = n.hx;
            p.hy = n.hy;
        }
        if let (Some(n), Some(p)) = (parts[1], pl.wallbounce_part()) {
            p.hx = n.hx;
            p.hy = n.hy;
        }
    }
    follow(w, id);
}

/// ActivateObject @859: Activated = true (and the section jump of MontageSectionJumpOnHit, unset).
fn shield_activate(w: &mut World, id: ActorId) {
    if let Some(s) = sh_mut(w, id) {
        s.activated = true;
    }
}

/// The `shield` kind the classes' entries take: the parent's ReceiveEndPlay and BPI_IgnorePlatformCollision are not
/// overridden by either class (entered as the parent's). No view: the classes draw nothing of their own.
pub fn hooks() -> WorldActorHooks {
    WorldActorHooks {
        spawn: Some(shield_spawn),
        spawned: Some(spawned),
        size: Some(SizeHooks { scale: Some(shield_scale), ..SizeHooks::default() }),
        bodies: Some(shield_bodies),
        step: Some(shield_step),
        end: Some(platform_end),
        ignore_platform_collision: Some(crate::world::spawned_platform::ignore_platform_collision),
        ignore_wallbounce_collision: Some(shield_ignore_wallbounce),
        hit_box_on: Some(shield_box_on),
        activate: Some(shield_activate),
        ..WorldActorHooks::default()
    }
}

/// An entry of the `shield` kind for `class` under `parents` (`{ parents, ...shield }`): a projectile's or an
/// item's hit body meets its BlockCollision (`spawned_meet_bodies`, `shield_bodies`).
// Deviation: the begin of that overlap is pfgDoDamage on the shield, as the dealer's handler makes it on any
// HitActor; the build's overlap of a projectile or item with the shield's body is not read.
pub fn entry(class: WorldActorClass, parents: &'static [&'static str]) -> ActorEntry {
    let mut e = ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks()));
    e.parents = parents;
    e.spawned_meet_bodies = true;
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::value::Value;

    fn props(v: &str) -> Props {
        match serde_json::from_str::<Value>(v).expect("a JSON object") {
            Value::Obj(o) => o,
            _ => unreachable!(),
        }
    }

    // placeBox with WW_ShieldRamForward's numbers (the head comment): 120 in front, 36 thick, 250 up and down.
    #[test]
    fn place_box_side_special() {
        let t = props(r#"{"Translation":{"X":120,"Y":0,"Z":-150},"Scale3D":{"X":0.6,"Y":1,"Z":1}}"#);
        let c = props(
            r#"{"BoxExtent":{"X":200,"Y":200,"Z":200},"RelativeLocation":{"X":0,"Y":0,"Z":150},"RelativeScale3D":{"X":0.3,"Y":1,"Z":1.25}}"#,
        );
        let p = place_box(Some(&c), &t).expect("placed");
        assert_eq!(p, Placed { fwd: 120.0, up: 0.0, hx: 200.0 * 0.3 * 0.6, hy: 250.0 });
        assert!(place_box(Some(&props("{}")), &t).is_none());
        // atScale: the size in place of Scale3D
        let s = place_box_tf(Some(&c), &Tf::of(&t).at_scale(Some(2.0))).expect("placed");
        // (float32: 200 x 0.3 x 2 is 120.00001)
        assert_eq!(s, Placed { fwd: 120.0, up: 300.0 - 150.0, hx: 120.00001, hy: 500.0 });
    }

    // socketBox with the up special's numbers (the socket note): axes X forward, Z up of length 2.018, Rotation -90
    // degrees about Y: the boxes lie flat, 103.1 half wide and 21 half thick.
    #[test]
    fn socket_box_up_special() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let t = Tf { tr: [40.0, 0.0, 45.0], q: [0.0, -h, 0.0, h], s: [0.4, 0.3, 0.3] };
        let k = 2.018;
        let ax = [k, 0.0, 0.0, 0.0, 0.0, k, 0.0, k, 0.0];
        // the frame socket_frame makes of those axes at a socket at (0, 0)
        let org = [t.tr[0] * ax[0] + t.tr[2] * ax[6], t.tr[0] * ax[1] + t.tr[2] * ax[7]];
        let [x, y, z, qw] = t.q;
        let mut rt = [0.0; 9];
        for c in 0..3 {
            let (ux, uy, uz) = (ax[c] / k, ax[3 + c] / k, ax[6 + c] / k);
            rt[c] = (1.0 - 2.0 * (y * y + z * z)) * ux + 2.0 * (x * y + qw * z) * uy + 2.0 * (x * z - qw * y) * uz;
            rt[3 + c] = 2.0 * (x * y - qw * z) * ux + (1.0 - 2.0 * (x * x + z * z)) * uy + 2.0 * (y * z + qw * x) * uz;
            rt[6 + c] = 2.0 * (x * z + qw * y) * ux + 2.0 * (y * z - qw * x) * uy + (1.0 - 2.0 * (x * x + y * y)) * uz;
        }
        let fr = Frame { org, rt };
        let c = props(
            r#"{"BoxExtent":{"X":175,"Y":275,"Z":275},"RelativeLocation":{"X":0,"Y":0,"Z":150},"RelativeScale3D":{"X":0.3,"Y":1,"Z":1.25}}"#,
        );
        let p = socket_box(Some(&c), &t, &fr).expect("upright");
        // the box's half extents and forward offset after the float32 socket frame (103.125, 21 and org - 45 ideal)
        assert_eq!((p.hx, p.hy, p.fwd), (103.12499, 21.000004, 35.719997));
        // a box with a rotation of its own is not placed
        let r = props(r#"{"BoxExtent":{"X":1,"Y":1,"Z":1},"RelativeRotation":{"Pitch":10}}"#);
        assert!(socket_box(Some(&r), &t, &fr).is_none());
        // a root turned off the play plane's axes is not placed
        let off = Frame { org, rt: [0.7, 0.7, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0] };
        assert!(socket_box(Some(&c), &t, &off).is_none());
    }
}
