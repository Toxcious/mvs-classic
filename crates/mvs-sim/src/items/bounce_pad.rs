//! Item_BouncePad_C (an ItemActorParent child of the random items pool) and the pad it lays, BouncePad_SpawnedActor_C (a
//! plain AActor). The item: Durability 2, CanUseItem false (ItemUse drops it),
//! CanEnemyDestroy false, a HitBoxDamage of zeros, ItemIdleLife 15.
//! - Thrown @29 (no parent call; the parent's is empty): CanDeploy = true.
//! - ReceiveHit, HitEnemyPlayer, HitAlly and OnHitAnything (@45 by @367-377, none calls its parent's): with CanDeploy,
//!   BeginDeferredActorSpawnFromClass(BouncePad_SpawnedActor_C, its location, rotation 0, scale 1, handling 2:
//!   AdjustIfPossibleButAlwaysSpawn) @150, the pad's Instigator = the item's @192, FinishSpawningActor @324, then
//!   ReleaseToPool @10. Without CanDeploy they do nothing.
//!
//! OnHitAnything replaces SpawnedActorParent's (no DestroyedOnHit Delay). The parent's HitNonPlayer (not overridden)
//! spends the hit after the deploy its OnHitAnything made: the port lets that spend deploy (`Item::spend`).
//! Unverified: on an enemy hit OnHitAnything (pfgDoDamage @22208) deploys and HitEnemyPlayer (@22770) comes to the
//! deploy again on the released item; one pad is made (whether ReleaseToPool leaves the item able to take the second
//! call is not read).
//!
//! The pad: InitialLifeSpan 15, BounceVelocity 3500, SpikeMultiplier 1.2. Root Box1 (BoxExtent 200 / 200 / 32, profile
//! Item, simulating, X and the rotations locked), SkeletalMesh under it (Z -30, scale 7, yaw 90), the Box DetectPawn
//! under the mesh (the BoxComponent's default extent 32, Z 14.825, scale 0.889 / 0.889 / 0.333).
//! - ReceiveBeginPlay @293 / ReceiveEndPlay @10: PS_TomJerryPoof00 at its location.
//! - ReceiveActorBeginOverlap @395 (DetectPawn is its one overlapping component): a BPI_BaseFighter BP_FighterCharacter,
//!   its Comp_Actor_Destructible not IsInvulnerable?, a FighterMovementComponent; then by CheckHitstunActive:
//!   - in hitstun: LaunchCharacter((0, 0, SpikeMultiplier x FMax(GetLastUpdateVelocity.Z, BounceVelocity)), false,
//!     true) @1131, AddHitStunFromKnockbackMulticast(0.5, zero, 1, 1) @1272, SetHitPauseTime(0.1) @1383;
//!   - else (the fighter's authority): LaunchCharacter((0, 0, BounceVelocity), false, true) @1649, BPI_CancelJump
//!     @1775, BPI_SetBlockFastFallTime(0.3) @1891, BPI_SetFastFallTriggered(false) @2012;
//!
//!   then BPI_SetIsGravityModified(false) @1503 and (authority) PlayAnimation @274: BouncePad_01_Bounce_Montage on the
//!   mesh (its notifies and ends bind nothing that acts): the view's, from `bounce_at`.
//!
//! No team test: the pad bounces its thrower too.
//!
//! Deviation: Box1 is a simulated body (gravity, the BodyInstance's LinearDamping, the stage block and the Platform
//! boxes block it from every side, X and the rotations locked); the PhysX box contact is the items' capsule rule
//! (`Item::collide`) on an axis-aligned box: the push out along the shallower axis, restitution 0.3 above 200 u/s into
//! the surface, friction 0.7 on the tangent.
//! Unverified: the engine's spawn adjustment (FindTeleportSpot on the root box) taken as one push out per blocking box.
//! Unverified: InitialLifeSpan's timer counted in the pad's own step from the frame after its spawn, against the timer
//! pass.
//! Deviation: DetectPawn's begin overlaps are tested once a frame, behind the pad's move (the build raises them inside
//! the moves of either side); GetLastUpdateVelocity is the fighter's velocity of the frame (`body.vy`), as
//! JakeHouse_Actor's.
//! Unverified: HitAlly's deploy, and the release behind an OnHitAnything whose hit spent nothing (a Light attack
//! type's HitNonPlayer), are made behind the item's hits of the frame (its ReceiveTick), not inside the hit.

use crate::Real;
use crate::State;
use crate::actors::item::ItemExt;
use crate::actors::world_actor::{WorldActor, WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ItemClass, ItemId, WorldActorClass};
use crate::combat::hit::{add_hit_stun_from_knockback, set_hit_pause, timer_count, timer_ticks};
use crate::consts::{DT, GRAVITY_SCALE, GRAVITY_Z};
use crate::events::Event;
use crate::fighter::common::{actor_data, warn_text};
use crate::fighter::moves::{Impulse, cancel_jump};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, in_hitstun, invulnerable, no_collision};
use crate::math::{max, min, sign};
use crate::name;
use crate::registry::entries::{ItemHooks, WorldActorHooks};
use crate::registry::props::{num, num_in, num_or, obj};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::pawns::match_fighters;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{add_actor, new_world_actor};
use mvs_collision::Ground;
use mvs_data::value::Props;

const BOX_DEFAULT: Real = 32.0; // UBoxComponent's BoxExtent default

/// ItemExt::BouncePadItem: Item_BouncePad_C's own variables (`BouncePadItem`).
#[derive(State, Debug, Default)]
pub struct BouncePadItem {
    /// CanDeploy.
    pub can_deploy: bool,
    /// A deploy the hit asked for, made by the world hook that follows the call (the spend has no world): the flight's
    /// ReceiveHit, HitEnemyPlayer, the parent's HitNonPlayer, or HitAlly (the item's tick).
    pub deploying: bool,
    /// (the one pad, see above)
    pub deployed: bool,
}

/// WorldActorState::BouncePad: BouncePad_SpawnedActor_C's own state (`BouncePad`). Sim axes.
#[derive(State, Debug)]
pub struct BouncePad {
    /// The root Box1's centre.
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    /// Box1's half extents in the play plane (BoxExtent.Y, BoxExtent.Z).
    pub hx: Real,
    pub hy: Real,
    pub lin_damp: Real,
    /// DetectPawn in the play plane, about the root: its centre's height above the root and its half extents.
    pub detect_y: Real,
    pub detect_hx: Real,
    pub detect_hy: Real,
    /// The fighters DetectPawn overlaps (their ids), for the begin edges.
    pub inside: Vec<i32>,
    /// Frames since the spawn frame.
    pub age: i32,
    /// InitialLifeSpan's timer, in frames after the spawn frame.
    pub life_at: Real,
    /// The world tick of the last PlayAnimation (the bounce montage from its start), -1 none.
    pub bounce_at: i32,
    /// `p`: the class's parameters (the actor data's), as the record writes them.
    pub p: Option<std::sync::Arc<Props>>,
}

impl Default for BouncePad {
    fn default() -> Self {
        BouncePad {
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            hx: BOX_DEFAULT,
            hy: BOX_DEFAULT,
            lin_damp: 0.01,
            detect_y: 0.0,
            detect_hx: 0.0,
            detect_hy: 0.0,
            inside: Vec::new(),
            age: 0,
            life_at: Real::INFINITY,
            bounce_at: -1,
            p: None,
        }
    }
}

/// The pad's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &BouncePad) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn item_state(w: &mut World, it: ItemId) -> &mut BouncePadItem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::BouncePadItem(b) => b,
        _ => unreachable!("Item_BouncePad_C holds a BouncePadItem"),
    }
}

fn pad(w: &World, id: ActorId) -> &BouncePad {
    match &w.actors.actors.get(id).expect("a live actor").state {
        WorldActorState::BouncePad(s) => s,
        _ => unreachable!("BouncePad_SpawnedActor_C holds a BouncePad"),
    }
}

fn pad_mut(w: &mut World, id: ActorId) -> &mut BouncePad {
    match &mut w.actors.actors.get_mut(id).expect("a live actor").state {
        WorldActorState::BouncePad(s) => s,
        _ => unreachable!("BouncePad_SpawnedActor_C holds a BouncePad"),
    }
}

// ---------------------------------------------------------------- the item

/// `deploy`: the pad laid at the item's place for its owner, once.
fn deploy(w: &mut World, it: ItemId) {
    let b = item_state(w, it);
    b.deploying = false;
    if b.deployed {
        return;
    }
    b.deployed = true;
    if reset_pass(w) {
        return; // the Lab reset's end pass
    }
    let (owner, x, y) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.owner, i.x, i.y)
    };
    // (CanDeploy comes from a throw, which names the thrower: an item never thrown lays no pad)
    let Some(owner) = owner else { return };
    let data = std::sync::Arc::clone(&w.data);
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(owner), name!("BouncePad_SpawnedActor_C")) else {
        warn_text(w, "Item_BouncePad_C: no BouncePad_SpawnedActor_C in the data (no pad)");
        return;
    };
    let state = new_pad(w, owner, a, x, y);
    let actor = new_world_actor(
        w,
        name!("BouncePad_SpawnedActor_C"),
        WorldActorClass::BouncePadSpawnedActor,
        owner,
        false,
        WorldActorState::BouncePad(state),
    );
    let id = w.actors.actors.insert(actor);
    add_actor(w, id, None, false);
}

/// OnHitAnything (`ActorKind.onHitAnything`, `hits::hit_anything`): with CanDeploy, the deploy; the release follows in
/// the hit's own spend (HitEnemyPlayer / HitNonPlayer).
fn hit_anything(w: &mut World, it: ItemId, _victim: ActorRef) {
    if item_state(w, it).can_deploy {
        deploy(w, it);
    }
}

fn thrown(w: &mut World, it: ItemId) {
    item_state(w, it).can_deploy = true;
}

fn receive_hit(w: &mut World, it: ItemId, _destroyed: bool) -> bool {
    if item_state(w, it).deploying {
        deploy(w, it);
    }
    false
}

fn hit_enemy(w: &mut World, it: ItemId, _vic: FighterId) -> bool {
    if !item_state(w, it).deploying {
        return false;
    }
    deploy(w, it);
    true
}

/// ReceiveHit @45 also fires for the dropped body's contacts (bNotifyRigidBodyCollision on ItemActorParent's Capsule),
/// and CanDeploy stays set once thrown: a thrown pad caught and dropped deploys where it lands (in the tick).
fn component_hit(w: &mut World, it: ItemId, _c: crate::actors::item::ItemContact) -> bool {
    let b = item_state(w, it);
    if b.can_deploy && !b.deployed {
        b.deploying = true;
    }
    false
}

fn hit_non_player(w: &mut World, it: ItemId, _destroyed: bool) -> bool {
    if item_state(w, it).deploying {
        deploy(w, it);
    }
    false
}

fn tick(w: &mut World, it: ItemId) -> bool {
    if item_state(w, it).deploying {
        deploy(w, it);
    }
    item_state(w, it).deployed
}

fn sub() -> ItemExt {
    ItemExt::BouncePadItem(BouncePadItem::default())
}

// ---------------------------------------------------------------- the pad

/// The pad made at the item's place for `owner` (`new BouncePad`): its boxes from the class data, pushed out of the
/// boxes it starts in, and the poof of BeginPlay.
fn new_pad(w: &mut World, owner: FighterId, a: &mvs_data::fighter::ActorDef, x: Real, y: Real) -> BouncePad {
    let comps = &a.components;
    let box1 = obj(comps, "Box1");
    let mesh = obj(comps, "SkeletalMesh");
    let det = obj(comps, "Box");
    let vec = |p: Option<&Props>, k: &str, axis: &str, d: Real| p.and_then(|p| num_in(p, k, axis)).unwrap_or(d);
    let mut s = BouncePad {
        hx: vec(box1, "BoxExtent", "Y", BOX_DEFAULT),
        hy: vec(box1, "BoxExtent", "Z", BOX_DEFAULT),
        lin_damp: box1.and_then(|b| obj(b, "BodyInstance")).and_then(|b| num(b, "LinearDamping")).unwrap_or(0.01),
        ..BouncePad::default()
    };
    // DetectPawn: its extent x its scale x the mesh's scale; the mesh's yaw 90 turns the box's X onto the world's Y (the
    // sim's x). Its centre: the mesh's Z + the mesh's Z scale x the box's Z. (Their Y and X offsets are below 0.001.)
    let ms = |axis: &str| mesh.and_then(|m| num_in(m, "RelativeScale3D", axis)).unwrap_or(1.0);
    let bs = |axis: &str| det.and_then(|m| num_in(m, "RelativeScale3D", axis)).unwrap_or(1.0);
    let be = |axis: &str| det.and_then(|m| num_in(m, "BoxExtent", axis)).unwrap_or(BOX_DEFAULT);
    let yaw = mesh.and_then(|m| num_in(m, "RelativeRotation", "Yaw")).unwrap_or(0.0);
    let yaw90 = (yaw.abs() - 90.0).abs() < 1.0;
    s.detect_hx = if yaw90 { be("X") * bs("X") * ms("X") } else { be("Y") * bs("Y") * ms("Y") };
    s.detect_hy = be("Z") * bs("Z") * ms("Z");
    s.detect_y = mesh.and_then(|m| num_in(m, "RelativeLocation", "Z")).unwrap_or(0.0)
        + ms("Z") * det.and_then(|m| num_in(m, "RelativeLocation", "Z")).unwrap_or(0.0);
    s.x = x;
    s.y = y;
    s.p = Some(std::sync::Arc::new(a.p.clone()));
    let life = num(&a.p, "InitialLifeSpan").unwrap_or(0.0);
    s.life_at = if life > 0.0 { timer_ticks(Some(life)) } else { Real::INFINITY };
    // Spawn handling 2 (AdjustIfPossibleButAlwaysSpawn): pushed out of what it starts in.
    let mut boxes = std::mem::take(&mut w.scratch.blockers);
    w.blockers_into(BlockKind::Item, &mut boxes);
    collide(&mut s, &w.stage.ground, false);
    for g in &boxes {
        collide(&mut s, g, false);
    }
    w.scratch.blockers = boxes;
    let mut polys = Vec::new();
    crate::fighter::traces::solids::blocker_polys(w, BlockKind::Item, &mut polys);
    collide_polys(&polys, &mut s, false);
    if !reset_pass(w) {
        poof(w, owner, s.x, s.y);
    }
    s
}

fn poof(w: &mut World, owner: FighterId, x: Real, y: Real) {
    let fighter = w.fighter(owner).id;
    w.events.push(Event::ProjectileDeath {
        cls: name!("BouncePad_SpawnedActor_C"),
        fighter,
        vfx: Some(name!("PS_TomJerryPoof00")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

/// `collide`: the box against a stage box: the push out along the shallower axis; `moving`: the bounce and the
/// friction on the velocity.
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated tests keep the NaN result")]
fn collide(a: &mut BouncePad, b: &Ground, moving: bool) {
    let cx = (b.left + b.right) / 2.0;
    let cy = (b.top + b.bottom) / 2.0;
    let ox = a.hx + crate::math::narrow((b.right - b.left) / 2.0) - (a.x - crate::math::narrow(cx)).abs();
    let oy = a.hy + crate::math::narrow((b.top - b.bottom) / 2.0) - (a.y - crate::math::narrow(cy)).abs();
    if !(ox > 0.0) || !(oy > 0.0) {
        return;
    }
    let (mut nx, mut ny) = (0.0, 0.0);
    let pen;
    if oy <= ox {
        ny = if a.y >= crate::math::narrow(cy) { 1.0 } else { -1.0 };
        pen = oy;
    } else {
        nx = if a.x >= crate::math::narrow(cx) { 1.0 } else { -1.0 };
        pen = ox;
    }
    respond(a, nx, ny, pen, moving);
}

/// The polygon stage solids that block an Item against the box, after the boxes: the push out along the polygon's
/// least way out (the shape traces' overlap), then `collide`'s bounce and friction.
// Unverified: PhysX's convex contact is not read.
fn collide_polys(polys: &[crate::world::stage_query::PolyBlock], a: &mut BouncePad, moving: bool) {
    for q in polys {
        if let Some((nx, ny, pen)) = q.box_mtd(a.x, a.y, a.hx, a.hy)
            && pen > 0.0
        {
            respond(a, nx, ny, pen, moving);
        }
    }
}

/// The push out of `pen` along (nx, ny); `moving`: the bounce and the friction on the velocity.
fn respond(a: &mut BouncePad, nx: Real, ny: Real, pen: Real, moving: bool) {
    a.x += nx * pen;
    a.y += ny * pen;
    if !moving {
        return;
    }
    // Deviation: the `kinematicOf` term (a moving stage piece as a kinematic body) is left out; the six traced
    // stages mark none, as `item_boxes`.
    let vn = a.vx * nx + a.vy * ny;
    if vn >= 0.0 {
        return;
    }
    let e = if -vn > 200.0 { 0.3 } else { 0.0 };
    let jn = -(1.0 + e) * vn;
    a.vx += nx * jn;
    a.vy += ny * jn;
    let (tx, ty) = (-ny, nx);
    let vt = a.vx * tx + a.vy * ty;
    let jt = min(vt.abs(), 0.7 * jn) * sign(vt);
    a.vx -= tx * jt;
    a.vy -= ty * jt;
}

fn physics(w: &mut World, id: ActorId) {
    let mut boxes = std::mem::take(&mut w.scratch.blockers);
    w.blockers_into(BlockKind::Item, &mut boxes);
    let mut polys = Vec::new();
    crate::fighter::traces::solids::blocker_polys(w, BlockKind::Item, &mut polys);
    let ground = w.stage.ground;
    let a = pad_mut(w, id);
    let dt = DT;
    a.vy -= GRAVITY_Z * dt;
    let damp = max(0.0, 1.0 - a.lin_damp * dt);
    a.vx *= damp;
    a.vy *= damp;
    a.x += a.vx * dt;
    a.y += a.vy * dt;
    collide(a, &ground, true);
    for g in &boxes {
        collide(a, g, true);
    }
    collide_polys(&polys, a, true);
    w.scratch.blockers = boxes;
}

/// DetectPawn (an axis-aligned box in the play plane) against the fighter's root capsule (a vertical segment of radius
/// `cap_radius`): the distance from the box to the segment.
fn over_fighter(w: &World, id: ActorId, f: FighterId) -> bool {
    let a = pad(w, id);
    let cy = a.y + a.detect_y;
    let r = cap_radius(w, f);
    let seg = capsule_half(w, f) - r;
    let ay = actor_y(w, f);
    let dx = max(0.0, (w.fighter(f).body.x - a.x).abs() - a.detect_hx);
    let dy = max(max(0.0, cy - a.detect_hy - (ay + seg)), ay - seg - (cy + a.detect_hy));
    dx * dx + dy * dy <= r * r
}

/// The begin overlaps of this frame, in the fighters' order.
fn overlaps(w: &mut World, id: ActorId) {
    for f in match_fighters(w) {
        let fid = w.fighter(f).id;
        let i = pad(w, id).inside.iter().position(|&k| k == fid);
        if !(w.fighter(f).life.dead <= 0.0 && !no_collision(w, f) && over_fighter(w, id, f)) {
            if let Some(i) = i {
                pad_mut(w, id).inside.remove(i);
            }
            continue;
        }
        if i.is_some() {
            continue;
        }
        pad_mut(w, id).inside.push(fid);
        bounce(w, id, f);
    }
}

/// LaunchCharacter(v, XYOverride false, ZOverride true): PendingLaunchVelocity, the XY part added to the velocity at the
/// call (ACharacter::LaunchCharacter), as JakeHouse_Actor's launch.
fn launch_z(w: &mut World, f: FighterId, vz: Real) {
    let vx = w.fighter(f).body.vx;
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx, vy: vz, xy: true, z: true, kind: 0, d: None });
    m.hit_launch = true; // a direct LaunchCharacter: the CMC takes it under the stun too
}

fn bounce(w: &mut World, id: ActorId, f: FighterId) {
    if invulnerable(w, f) {
        return; // IsInvulnerable? @577
    }
    let p = actor_p(w, ActorRef::Actor(id));
    let bv = num_or(p, "BounceVelocity", 0.0);
    let spike = num_or(p, "SpikeMultiplier", 1.0);
    if in_hitstun(w, f) {
        let vy = w.fighter(f).body.vy;
        launch_z(w, f, spike * max(vy, bv));
        add_hit_stun_from_knockback(w, f, 0.5, 0.0, 0.0, 1.0, 1.0);
        set_hit_pause(w, f, crate::real(timer_count(0.1)), 0.1);
    } else {
        launch_z(w, f, bv);
        cancel_jump(w, f);
        let ff = &mut w.fighter_mut(f).fast_fall;
        if 0.3 > ff.block_ff_time.unwrap_or(0.0) {
            ff.block_ff_time = Some(0.3); // BPI_SetBlockFastFallTime keeps the larger
        }
        ff.ff_triggered = false;
    }
    // BPI_SetIsGravityModified(false): GravityModifierCounter - 1 (not below 0), NetGravity(DefaultGravityScale) when it
    // is then 0 (BaseFighter ubergraph 106007-106120, as AnimNotify_ModifyGravity's end).
    let m = &mut w.fighter_mut(f).moves;
    if m.gravity_mod_count > 0 {
        m.gravity_mod_count -= 1;
    }
    if m.gravity_mod_count == 0 {
        m.net_gravity = Some(GRAVITY_SCALE);
    }
    let tick = w.flow.tick;
    pad_mut(w, id).bounce_at = tick; // PlayAnimation
}

/// The kind's `step`: InitialLifeSpan (AActor's life span timer -> Destroy), the pad's move and its overlaps.
fn step(w: &mut World, id: ActorId) {
    let a = pad_mut(w, id);
    a.age += 1;
    if crate::real(a.age) >= a.life_at {
        w.actors.actors.get_mut(id).expect("a live actor").dead = true;
        return;
    }
    physics(w, id);
    overlaps(w, id);
}

fn end(w: &mut World, id: ActorId) {
    if reset_pass(w) {
        return;
    }
    let (x, y) = (pad(w, id).x, pad(w, id).y);
    let owner = w.actors.actors.get(id).expect("a live actor").owner;
    poof(w, owner, x, y);
}

/// This file's registry entries (Item_BouncePad_C, BouncePad_SpawnedActor_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = ItemHooks {
        sub: Some(sub),
        thrown: Some(thrown),
        receive_hit: Some(receive_hit),
        hit_enemy: Some(hit_enemy),
        component_hit: Some(component_hit),
        hit_non_player: Some(hit_non_player),
        tick: Some(tick),
        hit_anything: Some(hit_anything),
        ..ItemHooks::default()
    };
    // (the world's random items spawn the item with no owner: its graph reads the owner only at the deploy, which a
    // throw precedes)
    r.actors.enter(
        ItemClass::ItemBouncePad.name(),
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemBouncePad), ActorHooks::Item(hooks))
        },
    )?;
    let hooks = WorldActorHooks { step: Some(step), end: Some(end), ..WorldActorHooks::default() };
    r.actors.enter(
        "BouncePad_SpawnedActor_C",
        ActorEntry {
            // no SpawnedActorParent
            keep_at_lab_reset: true,
            ..ActorEntry::new(ActorClass::World(WorldActorClass::BouncePadSpawnedActor), ActorHooks::World(hooks))
        },
    )
}
