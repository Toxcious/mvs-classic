//! ProjectileBarrier (Blueprints/Projectiles/ProjectileBarrier, a
//! SpawnedActorParent child): a sphere around its owner that destroys the other team's projectiles. A child class
//! enters its own kind with [`entry`] and gives its overrides as [`BarrierHooks`], found by its class
//! ([`child_hooks`]): C016ProjectileBarrier_C (`chars/lebron/bubble.rs`), later C023A_ProjectileBarrier_C.
//!
//! Who uses it in the build: one child class (LeBron's, under Blueprints/Projectiles: it overrides
//! ShouldActorBeBlockedAndDestroyed alone) spawned by two of his montages; nothing spawns the parent itself and no
//! other class is a child (C023A's barrier is a SpawnedActorParent child with a graph of its own: it takes this model
//! of the sphere with its own begin overlap, [`BarrierHooks::begin`]). A child is not covered by the parent's entry:
//! it enters its own kind with [`entry`].
//!
//! Components (templates): the root Capsule of SpawnedActorParent, here NoCollision and without overlap events;
//! HitBox, a sphere of 325 (object type HitBox, NoCollision in the template; overlaps Pawn, WallJump,
//! DamageDestructible, ItemDestructible); StopProjectiles, a sphere of 325 with profile ProjectileDestructible
//! (object type ProjectileDestructible = ECC_GameTraceChannel8, QueryAndPhysics in the template, every listed
//! channel Ignore but HitBox: Overlap). Comp_Actor_Destructible: DamageDisabled, IsProjectile. Defaults:
//! 'HitBoxOn?' false, DestroyedOnHit false, DestroyOnBlocked false, BlockedBufforMontage true, HitBuffer 0.25 (no
//! reader), HitBoxDamage 4 / 750 / 10 / (0.5, 0.75) (no hit box is ever on: unused); from SpawnedActorParent
//! 'Die With Instigator' and 'DestroyOnExitedArena?' true, no MaxLifetime.
//!
//! ReceiveBeginPlay @2175: the parent's BeginPlay (its SetTeamColor @13150 runs this class's override @1635: the
//! BarrierVFX emitter), then with authority the owner's Buff_ProjectileBarrierDefense, if it has one, gets
//! SetActive(true) @1700-2087, then Delay(0.0) @2195, which resumes at 15 (the LatentActionInfo's
//! EX_SkipOffsetConst): HitBox.SetCollisionEnabled(QueryOnly) @15 and StopProjectiles.SetCollisionEnabled(QueryOnly)
//! @53. So StopProjectiles overlaps from the spawn on (its template is on; begin overlaps fire right after
//! BeginPlay inside FinishSpawningActor) and the Delay only changes its type; HitBox comes on at the Delay, with no
//! handler: 'HitBoxOn?' is false, so the parent's UpdateHitbox takes HitBoxesOff, and this class binds
//! StopProjectiles' begin overlap alone (ComponentDelegateBinding_0).
//! ReceiveEndPlay @2250: the parent's, BarrierVFX destroyed @2312, the owner's buff SetActive(false) @654-1041.
//! The parent's BeginPlay blast-box loop and its CheckBlastBoxTimer (`world/blast.rs`). No tick, no timer, no
//! ActivateObject of its own: it ends with the spawning notify (DestroyAllActorsOnNotifyEnd), with its instigator
//! ('Die With Instigator') or with the match.
//!
//! StopProjectiles' begin overlap @1129 (BndEvt @108), for the other actor:
//!   ShouldActorBeBlockedAndDestroyed(other) @1129, a function a child overrides ([`BarrierHooks::blocks`]); false
//!     ends it;
//!   the other's BPI_Blocked(GetOwner()) @1270 when it has BPI_SpawnedActor (`combat::hits::blocked`);
//!   BlockedVFX(the other's actor location) @1366, a multicast @1630 -> @1450: sfx_cha_wonderwoman_bracerblock
//!     posted with PF_Post_Event_At_Location at the literal (0, 0, 0), not at the location (raw statement @1470:
//!     EX_VectorConst 0, 0, 0), and SpawnEmitterAtLocation(PS_HitImpact_Directional, the location, scale 1.5) @1560;
//!   the other's K2_DestroyActor @1389 (its EndPlay: SpawnedActorParent's death effects), at once and whatever
//!     BPI_Blocked did;
//!   with BlockedBufforMontage @1425: BlockedBuffandMontage @1435, a multicast @2483 -> @2350: the owner, cast to
//!     BPI_C016, gets BPI_GetBallBack @2445 ([`BarrierHooks::get_ball_back`]: only LeBron's pawn has the interface).
//! The parent's ShouldActorBeBlockedAndDestroyed (function @0-551): the other has a Comp_Actor_Destructible @0-80,
//! its BPI_GetDestroyOnBlocked @173 (false for an actor without BPI_SpawnedActor @535) is true, it has no
//! BPI_EnvironmentalHazard @219 and its Comp_Actor_Destructible.TeamIndex is not the barrier's @337. So: the
//! SpawnedActorParent children of another team with DestroyOnBlocked and no BPI_EnvironmentalHazard (a fire wall,
//! Rick's fart bomb: [`barrier_blocks`]). A fighter (no BPI_SpawnedActor) and a projectile without DestroyOnBlocked
//! (Morty's grenades) pass through and get no BPI_Blocked. A thrown item is blocked in the build: ItemActorParent_C
//! is a SpawnedActorParent child (DestroyOnBlocked true at the parent's CDO, written by no item class), its
//! ThrowHitBox is of type HitBox and its TeamIndex the thrower's (ItemActorParent @1364).
//!
//! What overlaps StopProjectiles: components of object type HitBox whose response to ProjectileDestructible is
//! Overlap, with collision on: the HitBox profile (a spawned actor's HitBox / HitBox2D once 'HitBoxOn?' turned them
//! on, a fighter's attack boxes). Nothing else: a projectile with its hit box off goes through untouched.
//!
//! Model: the projectiles of the World, each one's hit circle (`hit_collision`, `hit_radius`, at the actor), the
//! thrown items' ThrowHitBox (`box_on`, out of a hand, `hit_radius` at the item), the world actors by their
//! HitBox-type components that overlap ProjectileDestructible (`WorldActorHooks::hit_shapes`: a dealer's hit box, a
//! shockwave's spheres, HarleyBomb's PlayerAttachCollision), the Morty portals (their Box) and a projectile's hit box
//! that one call turns on and destroys (`WorldActorHooks::hit_box_on`, met inside that call: C016_SassBubble's Box)
//! against the sphere in the play plane (a sphere as its circle, a box at its centre's depth); an overlap begins when
//! the shape was not inside at the test before. World's typed lists hold no other class the predicate takes:
//! C019Marker (AllyHitbox / GarnetHitbox) has no DestroyOnBlocked, and C019_MortarAim's, C019_Ship's and
//! C019AimPunch's components are of other object types. Not met either, by their data (no component of the HitBox
//! type that overlaps ProjectileDestructible): C017_CannonExplotion's UpwardBurst (no saved body: OverlapAllDynamic,
//! WorldDynamic), MouseTrap_SpawnedActor's and VelmaCarryRun_Actor's HitBox (Custom with no saved ObjectType:
//! WorldDynamic), AryaKillPie's Sphere (HitBox, but ProjectileDestructible at the channel's default Ignore).
//! The test runs where the actor spawns (the projectiles stand where the frame before left them: the spawn is inside
//! the owner's notify, ahead of their movement ticks), in the actor's step, after the fighters' and the
//! projectiles' moves of the frame and ahead of the projectiles' hit tests: a projectile the barrier destroys in a
//! frame deals no hit in that frame; behind each world actor's tick and each thrown item's move
//! (`WorldActorHooks::spawned_moved`), the item's ahead of its hit tests; and inside a shockwave's tick, between the
//! move of each stage and its hits (`chars/shaggy/shockwave.rs` `shock_hits`, `chars/iron_giant` stomp hits),
//! BeginPlay's first stage included.
//! Unverified: an actor that deals its hits within its tick from shapes already on (HarleyJackInTheBox's detection,
//! C017_DownThrusterAttack's flame boxes) is met behind them; the build's order of one UpdateOverlaps (the barrier's
//! begin overlap against the dealer's own hit test) is not read.
//! Unverified: the order of one move's begin overlaps of a projectile's hit box (the barrier's sphere ahead of a
//! fighter's hurtbox) is the port's, not read; it matters for a projectile whose hit circle reaches a hurtbox in the
//! frame it enters the sphere of 325.
//! Deviation: a projectile SPAWNED inside the sphere is the other way round here: World's spawn test deals its hit at
//! the spawn place first and the barrier destroys it later in the same frame (the build: one query at the spawn place
//! when the hit box turns on, order of sphere and hurtbox not read).
//! Deviation: for the projectiles, one test a frame, after both moves. The build tests the sphere when the owner moves
//! (it is attached: the projectiles still at the frame before's place) and again when each projectile moves.
//! Unverified: a world actor's components are met at their place after its whole tick, the hit box it turned on in
//! that tick included (the build: at each SetWorldLocation and SetCollisionEnabled of the tick).
//! Unverified: the overlap at the spawn is taken once, at the attached place (the build updates overlaps after
//! BeginPlay at the spawn transform and again at the attach). The switch to QueryOnly at the Delay is taken to raise
//! no event for a projectile already inside (stock 4.26: UpdateOverlaps keeps a pair it already holds; not read in
//! the exe for a type change between two query-enabled types).
//! Not ported: the items, world actors and portals for a class with its own begin overlap ([`BarrierHooks::begin`]).
//! A projectile's shape is its hit circle (hitRadius at the actor; 0 for a class with only a 3D HitBox), not the
//! capsule with its offset.
//! A fighter's hit box over StopProjectiles (pfgDoDamage on this IsProjectile actor): the kind's `hurt_box`
//! ([`barrier_hurt_box`], the sphere on ProjectileDestructible) takes it into `fighter_hits_actor`. What that hit
//! does, read: Comp_Pawn_DamageManager DamageAndLaunch's CanBeHitBy @718 is false for the DamageDisabled component, so
//! @8047 broadcasts HitByAlly, whose SpawnedActorParent handler @24031 needs AlliesCanDeflectMoveDirection and
//! EnableProjectileMovement (both false here: nothing), and the hit is no Hit (@7956-8023); then @8093 goes on only
//! with the hit's ForceKnockback (AlwaysReceiveKnockback is false in the component) (`actor_damage_and_launch`).
//! DestroyOnExitedArena is ported ([`barrier_step`]: the actor is attached to its owner).
//! Not ported: HitBox (QueryOnly from the Delay on; no handler in this class; what another actor's handler does on
//! meeting a HitBox-type sphere of a DamageDisabled actor is not read); a projectile's hit box meeting StopProjectiles
//! with its own pfgDoDamage (the order against the barrier's begin overlap is native, not read); the SetActive calls
//! (no class of the dumps but the two barriers and the buff itself names Buff_ProjectileBarrierDefense: no pawn has
//! it; warned if one ever does).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the negated tests keep a NaN

use crate::Real;
use crate::State;
use crate::actors::projectile::{hit_collision, hit_near, truthy};
use crate::actors::world_actor::{
    WorldActorState, actor_dead, actor_entry, actor_p, actor_place, world_actor_place_of,
};
use crate::actors::{AcceleratorId, ActorClass, ActorId, ActorRef, ItemId, ProjectileId, WorldActorClass};
use crate::combat::boxes::{WorldSphere, seg_box_dist2};
use crate::combat::buffs::buff_of;
use crate::events::Event;
use crate::fighter::common::{actor_data, actor_def, first_check, owner_team, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::{BodyInfo, OBJ_PROJECTILE_DESTRUCTIBLE, TraceBody, body_info, response_to};
use crate::fighter::{FighterId, actor_y, facing};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, num_in, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};

/// `OBJ.HitBox`: the object type index.
const OBJ_HIT_BOX: i32 = 7;

/// `ProjectileBarrier`: the actor's state. (The actor's own Comp_State_Manager buffs, charge values, lifetime pause
/// and root scale are the World actor's.)
#[derive(State, Debug, Default)]
pub struct ProjectileBarrier {
    /// StopProjectiles' centre: the actor's location.
    pub x: Real,
    pub y: Real,
    /// The actor's ticks.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The overlaps that passed the predicate (the port's own count: tests, the view).
    pub blocked: i32,
    /// The numbers of the projectiles and world actors that overlap StopProjectiles, in the order the overlaps began
    /// (one count: the World's actor number).
    pub inside: Vec<i32>,
    /// The same for the thrown items (their numbers are the items' own count).
    pub inside_items: Vec<i32>,
    /// The notify's relative translation in the owner's frame (X forward, Z up).
    pub fwd: Real,
    pub up: Real,
    /// StopProjectiles' sphere at the spawn's scale, 0 for an actor the port could not place.
    pub radius: Real,
}

/// `BarrierHooks`: what a child class overrides.
#[derive(Clone, Copy, Debug, Default)]
pub struct BarrierHooks {
    /// The child's ShouldActorBeBlockedAndDestroyed(Actor), in place of the parent's ([`barrier_blocks`], which an
    /// override that calls the parent calls itself). Asked for every actor that begins to overlap.
    pub blocks: Option<fn(&World, ActorId, ActorRef) -> bool>,
    /// BlockedBuffandMontage @2350-2445: the owner's BPI_GetBallBack (the owner: whoever spawned the actor).
    pub get_ball_back: Option<fn(&mut World, ActorId, FighterId)>,
    /// A class whose StopProjectiles begin overlap is a graph of its own, in place of the whole of barrierBegin.
    /// Called once per projectile whose hit circle begins to overlap the sphere, and for a hit box that one call turns
    /// on and destroys (C016_SassBubble's Box).
    pub begin: Option<fn(&mut World, ActorId, ProjectileId)>,
}

/// The hooks of a class's barrier kind: its child's, none for the parent.
pub fn child_hooks(class: WorldActorClass) -> BarrierHooks {
    match class {
        WorldActorClass::C016ProjectileBarrier => crate::chars::lebron::bubble::BARRIER,
        WorldActorClass::C023AProjectileBarrier => crate::chars::gizmo::popcorn::HIDE_BOX_HOOKS,
        _ => BarrierHooks::default(),
    }
}

// ------------------------------------------------------------------ the actor

pub(crate) fn bar(w: &World, id: ActorId) -> Option<&ProjectileBarrier> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::ProjectileBarrier(s) => Some(s),
        _ => None,
    }
}

pub(crate) fn bar_mut(w: &mut World, id: ActorId) -> Option<&mut ProjectileBarrier> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::ProjectileBarrier(s) => Some(s),
        _ => None,
    }
}

/// The hooks of the barrier `id`'s own class.
fn hooks_of(w: &World, id: ActorId) -> BarrierHooks {
    w.actors.actors.get(id).map_or_else(BarrierHooks::default, |a| child_hooks(a.class))
}

/// StopProjectiles' sphere now: SetSizeScale's SetActorScale3D sets the root's world scale whole, in place of the
/// spawn's Scale3D (absolute).
fn scaled_radius(w: &World, id: ActorId) -> Real {
    let (Some(b), Some(a)) = (bar(w, id), w.actors.actors.get(id)) else { return 0.0 };
    let Some(k) = a.size_scale else { return b.radius };
    if !(b.radius > 0.0) {
        return b.radius;
    }
    let r = actor_def(w, ActorRef::Fighter(a.owner), a.cls)
        .and_then(|d| num_in(&d.components, "StopProjectiles", "SphereRadius"))
        .unwrap_or(0.0);
    r * k
}

/// `follow`: attached to the owner (K2_AttachToActor with no socket): its location is the owner's actor location plus
/// the relative translation, turned with the owner.
fn follow(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let (fx, fy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    if let Some(b) = bar_mut(w, id) {
        b.x = fx + b.fwd * fc;
        b.y = fy + b.up;
    }
}

/// StopProjectiles' sphere: its centre and radius now.
struct Ball {
    x: Real,
    y: Real,
    r: Real,
}

fn ball(w: &World, id: ActorId) -> Option<Ball> {
    let b = bar(w, id)?;
    Some(Ball { x: b.x, y: b.y, r: scaled_radius(w, id) })
}

// ------------------------------------------------------------------ the predicate

/// `barrierBlocks`: the parent's ShouldActorBeBlockedAndDestroyed for a SpawnedActorParent child: DestroyOnBlocked @173
/// AND another TeamIndex @337 AND NOT DoesImplementInterface(BPI_EnvironmentalHazard) @219-457. The interface is read as
/// the class's entry `hazard`: the build's projectile classes with the interface that the port has, FireWallHazard_C,
/// FartBomb_SpawnedActor_C and C018Flag_C / C018AirFlag_C, enter it (the build's other classes with it: MouseTrap,
/// BoomBox, GarnetElectricZone, SlowtoStunGrenade are not ported; SmokeGrenadeOcclusionBP and BugsBunnyTunnelParent are
/// world actors; the barriers, WindWall and C019ScrubShield have it with no BPI Clean and are no projectiles).
/// An item's DestroyOnBlocked is ItemActorParent's CDO's (true; no item class writes it).
pub fn barrier_blocks(w: &World, a: ActorId, o: ActorRef) -> bool {
    let Some(own) = w.actors.actors.get(a).map(|x| x.owner) else { return false };
    truthy(actor_p(w, o).get("DestroyOnBlocked"))
        && owner_team(w, o) != w.fighter(own).team.team
        && !actor_entry(w, o).is_some_and(|e| e.hazard.is_some())
}

// ------------------------------------------------------------------ the shapes

fn circle_in(b: &Ball, x: Real, y: Real, r: Real) -> bool {
    let (dx, dy, rr) = (x - b.x, y - b.y, b.r + r);
    dx * dx + dy * dy <= rr * rr
}

/// One of an actor's HitBox-type shapes against the sphere in the play plane: a sphere as its circle (its depth not
/// read), a box at the depth of its centre.
fn shapes_in(b: &Ball, shapes: &[WorldSphere]) -> bool {
    for s in shapes {
        if s.bx.is_none() {
            if circle_in(b, s.x, s.y, s.r) {
                return true;
            }
            continue;
        }
        let z = s.z.unwrap_or(0.0);
        if seg_box_dist2(s, b.x, b.y, z, b.x, b.y, z) <= b.r * b.r {
            return true;
        }
    }
    false
}

fn touches(w: &World, a: ActorId, pr: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    if p.dead || !hit_collision(p) {
        return false;
    }
    let Some(b) = ball(w, a) else { return false };
    let near = hit_near(w, p, b.x, b.y);
    circle_in(&b, near.x, near.y, p.hit_radius)
}

/// A thrown item's ThrowHitBox (object type HitBox) while it is on and out of a hand, at the item (the circle its hits
/// test).
fn item_touches(w: &World, a: ActorId, it: ItemId) -> bool {
    let Some(i) = w.actors.items.get(it) else { return false };
    if i.dead || i.owner.is_none() || !i.box_on() || i.holder >= 0 {
        return false;
    }
    let Some(b) = ball(w, a) else { return false };
    circle_in(&b, i.x, i.y, crate::actors::item::item_hit_radius(w, it))
}

/// The portal's Box, of the HitBox object type and overlapping
/// ProjectileDestructible, on from the spawn. Its depth extent is the box's Y.
fn portal_shape(w: &World, ac: AcceleratorId) -> Option<WorldSphere> {
    let c = w.actors.accelerators.get(ac)?;
    let d = actor_def(w, ActorRef::Fighter(c.owner), c.cls);
    let b = d.and_then(|d| obj(&d.components, "Box"));
    let e = b.and_then(|b| obj(b, "BoxExtent"));
    let l = b.and_then(|b| obj(b, "RelativeLocation"));
    let g = |p: Option<&mvs_data::value::Props>, k: &str| p.and_then(|p| num(p, k)).unwrap_or(0.0);
    Some(WorldSphere {
        x: c.x + g(l, "X") * c.facing,
        y: c.y + g(l, "Z"),
        z: Some(0.0),
        r: 0.0,
        bx: Some([g(e, "X"), 0.0, 0.0, 0.0, g(e, "Z"), 0.0, 0.0, 0.0, g(e, "Y")]),
        ..WorldSphere::default()
    })
}

/// A Morty portal of the World by its Box.
fn portal_touches(w: &World, a: ActorId, ac: AcceleratorId) -> bool {
    let Some(p) = w.actors.accelerators.get(ac) else { return false };
    if p.dead {
        return false;
    }
    let (Some(b), Some(s)) = (ball(w, a), portal_shape(w, ac)) else { return false };
    shapes_in(&b, &[s])
}

/// `then` of a world actor's `hitShapes`: `None` for a class without the hook, `Some(None)` for a null.
fn with_shapes<R>(w: &World, o: ActorId, then: impl FnOnce(Option<Option<&[WorldSphere]>>) -> R) -> R {
    let Some(f) = actor_entry(w, ActorRef::Actor(o)).and_then(|e| match e.hooks {
        ActorHooks::World(h) => h.hit_shapes,
        _ => None,
    }) else {
        return then(None);
    };
    // The World's buffer, taken out for the call (a nested call finds it empty and uses its own).
    let mut v = std::mem::take(&mut *w.scratch.actor_shapes.borrow_mut());
    v.clear();
    let r = then(Some(if f(w, o, &mut v) { Some(&v) } else { None }));
    *w.scratch.actor_shapes.borrow_mut() = v;
    r
}

/// An actor of the World's actors by its HitBox-type components that overlap ProjectileDestructible.
fn actor_touches(w: &World, a: ActorId, o: ActorId) -> bool {
    let Some(x) = w.actors.actors.get(o) else { return false };
    if x.dead || o == a {
        return false;
    }
    with_shapes(w, o, |s| match s {
        None => {
            unmet_check(w, o);
            false
        }
        Some(None) => false,
        Some(Some(v)) => ball(w, a).is_some_and(|b| shapes_in(&b, v)),
    })
}

/// A world actor class with no `hit_shapes` whose data has a component that would meet StopProjectiles (of the HitBox
/// object type, overlapping ProjectileDestructible) and DestroyOnBlocked: warned once, the barrier passes it. A class
/// without DestroyOnBlocked is refused by the predicate whatever it overlaps. Run once per class (`UNMET_CHECKED`).
fn unmet_check(w: &World, o: ActorId) {
    let r = ActorRef::Actor(o);
    let Some(x) = w.actors.actors.get(o) else { return };
    if !first_check(w, "ProjectileBarrier unmet", x.cls) {
        return;
    }
    if !truthy(actor_p(w, r).get("DestroyOnBlocked")) || actor_entry(w, r).is_some_and(|e| e.hazard.is_some()) {
        return; // (BPI_EnvironmentalHazard: the predicate refuses it)
    }
    let Some(d) = actor_def(w, ActorRef::Fighter(x.owner), x.cls) else { return };
    for (n, c) in &d.components {
        let comp = c.as_obj();
        let hit = body_info(comp, None).is_some_and(|i: BodyInfo| i.kind == OBJ_HIT_BOX)
            && response_to(comp, crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)) != name!("ECR_Ignore");
        if hit {
            let cls = w.names.str(x.cls);
            warn_text(
                w,
                &format!(
                    "ProjectileBarrier: {cls}'s {n} would meet StopProjectiles; its kind gives no hitShapes, so it passes"
                ),
            );
            return;
        }
    }
}

/// An actor of the World's actors with no place of its own: its first hit shape's centre.
fn actor_at(w: &World, o: ActorId) -> (Real, Real) {
    // (`typeof o.x === 'number'`: a class with a place of its own)
    if let Some((px, py, _)) = world_actor_place_of(w, o) {
        return (px, py);
    }
    with_shapes(w, o, |s| match s {
        Some(Some(v)) if !v.is_empty() => (v[0].x, v[0].y),
        _ => (0.0, 0.0),
    })
}

// ------------------------------------------------------------------ the begin overlap

/// The begin overlap @1129 for one actor. Its location (K2_GetActorLocation @1316) is read after BPI_Blocked.
fn barrier_begin(w: &mut World, a: ActorId, o: ActorRef, hooks: &BarrierHooks) {
    let blocks = match hooks.blocks {
        Some(h) => h(w, a, o),
        None => barrier_blocks(w, a, o),
    };
    if !blocks {
        return;
    }
    let Some(act) = w.actors.actors.get(a) else { return };
    let (f, cls) = (act.owner, act.cls);
    if let Some(b) = bar_mut(w, a) {
        b.blocked += 1;
    }
    crate::combat::hits::blocked(w, o, ActorRef::Fighter(f));
    let (x, y) = match o {
        ActorRef::Actor(oid) => actor_at(w, oid),
        o => actor_place(w, o).map_or((0.0, 0.0), |(x, y, _)| (x, y)),
    };
    let fid = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_wonderwoman_bracerblock"),
        x: Some(0.0),
        y: Some(0.0),
        hit: None,
        pooled: false,
        key: None,
    });
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
    // K2_DestroyActor @1389, at once (an item's or a world actor's EndPlay: destroySpawnedActor with no Delay)
    if actor_dead(w, o) {
        // (BPI_Blocked ended it)
    } else if let ActorRef::Projectile(pr) = o {
        crate::world::spawning::destroy_projectile(w, pr, false);
    } else {
        crate::world::spawning::destroy_spawned_actor(w, o, 0.0);
    }
    let buffor = actor_def(w, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "BlockedBufforMontage"));
    if buffor && let Some(g) = hooks.get_ball_back {
        g(w, a, f);
    }
}

/// A projectile's begin overlap: the child's own graph, or the shared one.
fn begin_projectile(w: &mut World, a: ActorId, pr: ProjectileId, hooks: &BarrierHooks) {
    match hooks.begin {
        Some(h) => h(w, a, pr),
        None => barrier_begin(w, a, ActorRef::Projectile(pr), hooks),
    }
}

/// StopProjectiles' overlaps at the actor's place now: the pairs that ended leave the lists, the new ones begin.
// Unverified: the projectiles, then the thrown items, then the world actors; the order of one move's begin overlaps is
// not read.
fn barrier_overlaps(w: &mut World, a: ActorId, hooks: &BarrierHooks) {
    if !bar(w, a).is_some_and(|b| b.radius > 0.0) {
        return;
    }
    for i in (0..bar(w, a).map_or(0, |b| b.inside.len())).rev() {
        let Some(&seq) = bar(w, a).and_then(|b| b.inside.get(i)) else { continue };
        if !still_inside(w, a, seq)
            && let Some(b) = bar_mut(w, a)
        {
            b.inside.remove(i);
        }
    }
    for i in (0..bar(w, a).map_or(0, |b| b.inside_items.len())).rev() {
        let Some(&seq) = bar(w, a).and_then(|b| b.inside_items.get(i)) else { continue };
        let still = w.actors.items.iter().any(|(id, it)| it.seq == seq && item_touches(w, a, id));
        if !still && let Some(b) = bar_mut(w, a) {
            b.inside_items.remove(i);
        }
    }
    // (a begin overlap can end other actors and list new ones: the lists are read as they stand)
    let mut k = 0;
    while let Some(pr) = w.actors.projectiles.nth(k) {
        k += 1;
        if !touches(w, a, pr) {
            continue;
        }
        let seq = w.actors.projectiles.get(pr).map_or(-1, |p| p.seq);
        let Some(b) = bar_mut(w, a) else { return };
        if b.inside.contains(&seq) {
            continue;
        }
        b.inside.push(seq);
        begin_projectile(w, a, pr, hooks);
    }
    if hooks.begin.is_some() {
        return; // Not ported: a class with its own begin overlap meeting an item, a world actor or a portal
    }
    let mut k = 0;
    while let Some(it) = w.actors.items.nth(k) {
        k += 1;
        if !item_touches(w, a, it) {
            continue;
        }
        let seq = w.actors.items.get(it).map_or(-1, |i| i.seq);
        let Some(b) = bar_mut(w, a) else { return };
        if b.inside_items.contains(&seq) {
            continue;
        }
        b.inside_items.push(seq);
        barrier_begin(w, a, ActorRef::Item(it), hooks);
    }
    let mut k = 0;
    while let Some(o) = w.actors.actors.nth(k) {
        k += 1;
        if !actor_touches(w, a, o) {
            continue;
        }
        let seq = w.actors.actors.get(o).map_or(-1, |x| x.seq);
        let Some(b) = bar_mut(w, a) else { return };
        if b.inside.contains(&seq) {
            continue;
        }
        b.inside.push(seq);
        barrier_begin(w, a, ActorRef::Actor(o), hooks);
    }
    let mut k = 0;
    while let Some(ac) = w.actors.accelerators.nth(k) {
        k += 1;
        if !portal_touches(w, a, ac) {
            continue;
        }
        let seq = w.actors.accelerators.get(ac).map_or(-1, |x| x.seq);
        let Some(b) = bar_mut(w, a) else { return };
        if b.inside.contains(&seq) {
            continue;
        }
        b.inside.push(seq);
        barrier_begin(w, a, ActorRef::Accelerator(ac), hooks);
    }
}

/// Is the projectile, world actor or portal numbered `seq` still inside the sphere.
fn still_inside(w: &World, a: ActorId, seq: i32) -> bool {
    w.actors.projectiles.iter().any(|(id, p)| p.seq == seq && touches(w, a, id))
        || w.actors.actors.iter().any(|(id, x)| x.seq == seq && actor_touches(w, a, id))
        || w.actors.accelerators.iter().any(|(id, x)| x.seq == seq && portal_touches(w, a, id))
}

/// `ActorKind.spawnedMoved`: the one world actor or thrown item that moved, against the sphere where the owner's move
/// of the frame left it (attached: it moves with the owner, whose tick came first).
fn barrier_moved(w: &mut World, a: ActorId, o: ActorRef) {
    let hooks = hooks_of(w, a);
    let Some(act) = w.actors.actors.get(a) else { return };
    if act.dead || hooks.begin.is_some() || !bar(w, a).is_some_and(|b| b.radius > 0.0) {
        return;
    }
    follow(w, a);
    match o {
        ActorRef::Item(it) => {
            let Some(seq) = w.actors.items.get(it).map(|i| i.seq) else { return };
            let at = bar(w, a).and_then(|b| b.inside_items.iter().position(|&s| s == seq));
            if !item_touches(w, a, it) {
                if let (Some(i), Some(b)) = (at, bar_mut(w, a)) {
                    b.inside_items.remove(i);
                }
                return;
            }
            if at.is_some() {
                return;
            }
            if let Some(b) = bar_mut(w, a) {
                b.inside_items.push(seq);
            }
            barrier_begin(w, a, o, &hooks);
        }
        ActorRef::Actor(oid) => {
            let Some(seq) = w.actors.actors.get(oid).map(|x| x.seq) else { return };
            let at = bar(w, a).and_then(|b| b.inside.iter().position(|&s| s == seq));
            if !actor_touches(w, a, oid) {
                if let (Some(i), Some(b)) = (at, bar_mut(w, a)) {
                    b.inside.remove(i);
                }
                return;
            }
            if at.is_some() {
                return;
            }
            if let Some(b) = bar_mut(w, a) {
                b.inside.push(seq);
            }
            barrier_begin(w, a, o, &hooks);
        }
        _ => {}
    }
}

/// `ActorKind.hitBoxOn`: a projectile's hit box that lives inside one call (C016_SassBubble's Box), against the sphere
/// where the owner stands at that call (attached: it moves with the owner). The component meets StopProjectiles as the
/// model above says: of the HitBox object type, overlapping ProjectileDestructible. The pair is listed in `inside`, so
/// the call's second overlap update begins nothing again, and leaves it at the barrier's next pass (`touches`: that
/// projectile's hit circle is off), as the box's destroy ends it in the build.
fn barrier_box_on(w: &mut World, a: ActorId, pr: ProjectileId, comp: crate::names::Name, bx: &WorldSphere) {
    let hooks = hooks_of(w, a);
    let Some(act) = w.actors.actors.get(a) else { return };
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (Some(own), seq) = (p.owner, p.seq) else { return };
    let Some(b) = bar(w, a) else { return };
    if !(b.radius > 0.0) || act.dead || p.dead || b.inside.contains(&seq) {
        return;
    }
    let c = actor_def(w, ActorRef::Fighter(own), p.cls).and_then(|d| obj(&d.components, w.names.str(comp)));
    if body_info(c, None).is_none_or(|i| i.kind != OBJ_HIT_BOX)
        || response_to(c, crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)) == name!("ECR_Ignore")
    {
        return;
    }
    follow(w, a);
    if !ball(w, a).is_some_and(|b| shapes_in(&b, std::slice::from_ref(bx))) {
        return;
    }
    if let Some(b) = bar_mut(w, a) {
        b.inside.push(seq);
    }
    begin_projectile(w, a, pr, &hooks);
}

// ------------------------------------------------------------------ the spawn

/// CompSpawnActor with AttachToActor and socket None (both of the build's spawners): Comp_Spawner
/// HandleActorPostSpawn attaches the actor to the owner @10335 and sets the notify's Transforms as its relative
/// transform @10515: Translation in the owner's frame, Scale3D as the world scale of the root Capsule
/// (bAbsoluteScale), which the two spheres under it take.
/// Any other spawn form is not placed (warned): the class has no user of one.
fn barrier_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::ProjectileBarrier(ProjectileBarrier::default()), true);
    };
    let cls = w.names.str(r.cls);
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms");
    let tr = t.and_then(|t| obj(t, "Translation"));
    let sc = t.and_then(|t| obj(t, "Scale3D"));
    let g = |p: Option<&mvs_data::value::Props>, k: &str| p.and_then(|p| num(p, k));
    let (tx, ty, tz) = (g(tr, "X").unwrap_or(0.0), g(tr, "Y").unwrap_or(0.0), g(tr, "Z").unwrap_or(0.0));
    let sx = g(sc, "X").unwrap_or(1.0);
    let c = obj(&def.components, "StopProjectiles");
    let sphere = c.and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0);
    let mut radius = sphere * sx;
    let socket = text(&np, "Socket").is_some_and(|s| !s.is_empty() && s != "None");
    if !(sphere != 0.0 && !sphere.is_nan()) {
        warn_text(w, &format!("{cls}: no StopProjectiles sphere in its data: it stops nothing"));
    } else if !flag(&np, "AttachToActor")
        || socket
        || ty != 0.0
        || g(sc, "Y").unwrap_or(1.0) != sx
        || g(sc, "Z").unwrap_or(1.0) != sx
    {
        warn_text(
            w,
            &format!(
                "{cls}: spawn form not ported (not attached, a socket, a depth offset or an uneven scale): it stops nothing"
            ),
        );
        radius = 0.0;
    } else if c
        .and_then(|c| obj(c, "BodyInstance"))
        .and_then(|b| text(b, "CollisionEnabled"))
        .is_some_and(|e| e.ends_with("NoCollision"))
    {
        // (the build's template is QueryAndPhysics: the sphere is on from the spawn)
        warn_text(
            w,
            &format!(
                "{cls}: StopProjectiles is off in its template: the Delay's switch is not ported, it stops nothing"
            ),
        );
        radius = 0.0;
    }
    if flag(&def.p, "MaxLifetime") {
        warn_text(w, &format!("{cls}: MaxLifetime not ported for a barrier"));
    }
    if flag(&def.p, "DeathVFX") || flag(&def.p, "Ak Death Location") {
        warn_text(w, &format!("{cls}: the parent's death effects are not ported for a barrier"));
    }
    if buff_of(w, f, name!("Buff_ProjectileBarrierDefense_C")).is_some() {
        warn_text(w, &format!("{cls}: Buff_ProjectileBarrierDefense_C.SetActive not ported"));
    }
    let (fc, fx, fy) = (facing(w, f), w.fighter(f).body.x, actor_y(w, f));
    let mut b = ProjectileBarrier {
        x: fx + tx * fc,
        y: fy + tz,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        fwd: tx,
        up: tz,
        radius,
        ..ProjectileBarrier::default()
    };
    let force = flag(&def.p, "ForceSpawnPositionInsideBlastBox");
    let doe = flag(&def.p, "DestroyOnExitedArena?");
    // the parent's BeginPlay loop
    let dead = blast_box_loop(&spawn_box(w), &mut b.x, &mut b.y, None, force, &mut b.blast_bounds, doe);
    (WorldActorState::ProjectileBarrier(b), dead)
}

/// The overlaps at the spawn, the end of `barrierSpawn` (for an actor the BeginPlay loop left alive): run once the
/// World has put the actor in its arena, ahead of `add_actor` (its number, its passive buffs) as in the traces.
///
/// Deviation: the fence is already in `World.actors` during this pass (the arena insert gives the id the hooks take).
/// A world actor a blocked projectile spawns at its death is listed after the fence here and before it in the
/// per-tick trace's actor order, which the order of the passes keeps; a walk of the actor list here sees the fence.
/// The build's order of these is not read.
fn barrier_spawned(w: &mut World, a: ActorId) {
    let hooks = hooks_of(w, a);
    barrier_overlaps(w, a, &hooks);
}

// ------------------------------------------------------------------ the traces' body

/// StopProjectiles as a body of the traces: the sphere `barrier_overlaps` tests (radius at the root's scale) at the
/// actor, object type ProjectileDestructible; QueryAndPhysics from the template and QueryOnly from @53, both answer a
/// query; none while the actor is not placed (radius 0). What meets it: the object queries for ProjectileDestructible
/// (the Earth Smash pillar's overlap, C021_ForceField's). Capsule has no collision and HitBox (QueryOnly from the
/// Delay on) is of object type HitBox, which no query gathers.
fn barrier_bodies(w: &World, id: ActorId, out: &mut Vec<TraceBody>) {
    let Some(b) = bar(w, id) else { return };
    if !(b.radius > 0.0) {
        return;
    }
    let Some(a) = w.actors.actors.get(id) else { return };
    let comp = actor_def(w, ActorRef::Fighter(a.owner), a.cls).and_then(|d| obj(&d.components, "StopProjectiles"));
    let Some(info) = body_info(comp, None).filter(|i| i.on) else { return };
    out.push(TraceBody {
        ax: b.x,
        ay: b.y,
        bx: b.x,
        by: b.y,
        hx: 0.0,
        hy: 0.0,
        r: scaled_radius(w, id),
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("StopProjectiles"),
        actor: Some(ActorRef::Actor(id)),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

// ------------------------------------------------------------------ the kind

/// `WorldActorHooks::hurt_box`: StopProjectiles' sphere (the actor's place, the scaled radius) for a fighter's hit box
/// (`melee_actors::actor_hurt_channel` reads its object type, ProjectileDestructible). None while the actor is not
/// placed (radius 0).
// Deviation: the barrier gets no hurt box here, so no fighter's hit box meets it. Unverified: the build's side.
fn barrier_hurt_box(w: &World, id: ActorId) -> Option<WorldSphere> {
    let b = bar(w, id)?;
    if !(b.radius > 0.0) {
        return None;
    }
    Some(WorldSphere { x: b.x, y: b.y, r: scaled_radius(w, id), ..WorldSphere::default() })
}

/// The actor's step: its place under the owner after the fighters' moves, then the overlaps.
fn barrier_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let doe = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    follow(w, id);
    let Some(b) = bar_mut(w, id) else { return };
    b.age += 1;
    let frames = crate::real(b.age - 1);
    if blast_timer_exit(doe, &mut b.blast_at, &b.blast_bounds, frames, b.x, b.y) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    let hooks = hooks_of(w, id);
    barrier_overlaps(w, id, &hooks);
}

/// `projectileBarrier(hooks)`: the kind of ProjectileBarrier or of a child class (its hooks by [`child_hooks`], read
/// when it runs). Its sphere reads the scaled radius; the view's emitter is set to a world scale of 1.
pub fn entry(class: WorldActorClass) -> ActorEntry {
    let hooks = WorldActorHooks {
        spawn: Some(barrier_spawn),
        spawned: Some(barrier_spawned),
        size: Some(SizeHooks::default()),
        bodies: Some(barrier_bodies),
        hurt_box: Some(barrier_hurt_box),
        spawned_moved: Some(barrier_moved),
        hit_box_on: Some(barrier_box_on),
        step: Some(barrier_step),
        ..WorldActorHooks::default()
    };
    ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
}

/// ProjectileBarrier_C's entry.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter("ProjectileBarrier_C", entry(WorldActorClass::ProjectileBarrier))
}
