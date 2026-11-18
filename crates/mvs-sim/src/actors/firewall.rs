//! FireWallHazard (a SpawnedActorParent child with a ProjectileMovement) and the pawns' Comp_Pawn_FireWall, which
//! keeps how many of one fighter's walls stand.
//!
//! The class defaults and components are in the spawner's data file (FireWallHazard_C,
//! Comp_Pawn_FireWall_GEN_VARIABLE: the fighter data merges the component's class default MaxFirewalls 1 into the
//! pawn's entry). Spawned by LeBron's Dunk1 (an AnimNotify_ConditionalSpawner behind C016IgniteDunkEnabledCondition,
//! which only a perk gives), Superman's FireSlam (a perk's) and Creature's moves.
//!
//! The wall deals no hit (HitBoxOn? false, an all-zero HitBoxDamage that no graph reads). What it does is its passive
//! rows (Comp_Pawn_PassiveBuffManager, all BuffSelf: Buff_Elemental_Fireproof 0 stacks, Buff_Elemental_Fire 5,
//! Buff_Elemental_Iceproof 0) and its BPI_GetBoxCollision (the Box component): with a box Buff_Elemental_Fire's
//! looping 0.5 s timer runs ApplyFireOverTime on the fighters the box overlaps (one stack of Buff_Fire on each of
//! another team, 20 stacks of Buff_Ice off each of the wall's own), and the Box's begin overlap with another spawned
//! actor sets an allied one on fire ([`box_actors`]). It ends at its MaxLifetime (8 s, the parent's timer), when its
//! elemental ends ([`buff_ended`]), when its owner has more walls than MaxFirewalls (the component below), with its
//! instigator (Die With Instigator) and outside the arena.

use crate::Real;
use crate::State;
use crate::actors::projectile::{Projectile, ProjectileExt};
use crate::actors::{ActorClass, ActorRef, ProjectileClass, ProjectileId};
use crate::combat::BuffClass;
use crate::combat::elem::{elem_of, elem_overlap, elem_tick};
use crate::combat::shapes::FireWallComp;
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::traces::{TraceBody, body_info};
use crate::math::{max, to_int32};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, ElementalParts, HazardHooks, ProjectileHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The class.
pub const FIRE_WALL: &str = "FireWallHazard_C";
/// The component's class.
const COMPONENT: &str = "Comp_Pawn_FireWall_C";
const ELEM_FIRE: BuffClass = BuffClass::BuffElementalFire;
const ELEM_ICE: BuffClass = BuffClass::BuffElementalIce;

/// `FireWall extends Projectile`: what the class adds.
#[derive(State, Debug, Default)]
pub struct FireWall {
    /// The Box component (BPI_GetBoxCollision): its centre above the root and its half extents in the play plane
    /// (RelativeLocation Z 100, BoxExtent (200, 200, 200), under the Capsule root, no rotation of its own).
    pub box_z: Real,
    pub box_hx: Real,
    pub box_hy: Real,
    /// The HurtBox (a CapsuleComponent, object type ProjectileDestructible, under the Capsule root at RelativeLocation
    /// Z 270, upright: no RelativeRotation): its centre above the root and the half length of its axis,
    /// CapsuleHalfHeight 500 less CapsuleRadius 240, x the root scale. The base reads the HurtBox as a sphere, so the
    /// class gives its own shape (`projectile::hurt_y` / `hurt_dist2`).
    pub hurt_z: Real,
    pub hurt_half: Real,
    /// Its Buff_Elemental_Fire / Buff_Elemental_Ice were seen alive at the last look ([`buff_ended`]).
    pub had_fire: bool,
    pub had_ice: bool,
    /// The spawn counts (`Projectile::seq`) of the projectiles and items whose hurt shape the Box overlapped at the
    /// last test (the begin overlap of each is met once: [`box_actors`]).
    pub in_proj: Vec<i32>,
    pub in_items: Vec<i32>,
    /// A wall the world spawned ([`make_world_wall`]): the yaw of its spawn transform.
    pub spawn_yaw: Option<Real>,
    /// The Team a graph wrote on its ParticleSystem and its Decal's dynamic material after FinishSpawning (the drawing
    /// side's; `None`: none written).
    pub fx_team: Option<Real>,
}

// ---- the constructor

/// `makeWall`: `new FireWall(...)` then `wallShapes`. Every Comp_Spawner spawn has yaw +-90: the Box's local X runs
/// along the facing, so its play-plane side is BoxExtent.X.
fn construct(w: &World, p: &mut Projectile) {
    let fw = wall_shapes(w, p, true);
    p.ext = ProjectileExt::FireWall(fw);
}

/// `makeWorldWall`: a spawn with no owner (BeginDeferredActorSpawnFromClass by a graph, owner None) at the origin,
/// with the yaw of the spawn transform's rotation (0 when it states none): at yaw 0 the Box's local Y runs along the
/// play plane, so its play-plane side is BoxExtent.Y.
// Ported, not on the trace: no spawn path of the port makes an ownerless projectile yet (`make_world_at_rest`).
pub fn make_world_wall(w: &World, p: &mut Projectile, yaw: Real) {
    let mut fw = wall_shapes(w, p, (yaw.abs() - 90.0).abs() < 1.0);
    fw.spawn_yaw = Some(yaw);
    p.ext = ProjectileExt::FireWall(fw);
}

fn comp<'a>(c: &'a Props, name: &str) -> Option<&'a Props> {
    c.get(name).and_then(Value::as_obj)
}

fn num(o: Option<&Props>, k: &str) -> Option<Real> {
    o.and_then(|o| o.get(k)).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `wallShapes`: the Box and the HurtBox from the class data at the root's scale; the HurtBox's radius replaces the
/// base sphere's.
fn wall_shapes(w: &World, p: &mut Projectile, side_x: bool) -> FireWall {
    let def = crate::actors::projectile::class_def(w, p);
    let cs = def.map(|d| &d.components);
    let bx = cs.and_then(|c| comp(c, "Box"));
    let hb = cs.and_then(|c| comp(c, "HurtBox"));
    let ext = bx.and_then(|b| comp(b, "BoxExtent"));
    let rad = num(hb, "CapsuleRadius").unwrap_or(0.0);
    let hh = num(hb, "CapsuleHalfHeight").unwrap_or(0.0);
    let s = p.scale;
    let fw = FireWall {
        box_z: num(bx.and_then(|b| comp(b, "RelativeLocation")), "Z").unwrap_or(0.0) * s,
        box_hx: num(ext, if side_x { "X" } else { "Y" }).unwrap_or(0.0) * s,
        box_hy: num(ext, "Z").unwrap_or(0.0) * s,
        hurt_z: num(hb.and_then(|b| comp(b, "RelativeLocation")), "Z").unwrap_or(0.0) * s,
        hurt_half: max(hh - rad, 0.0) * s,
        ..FireWall::default()
    };
    p.hurt_radius = rad * s;
    fw
}

fn wall(w: &World, pr: ProjectileId) -> &FireWall {
    match &w.actors.projectiles.get(pr).expect("a live projectile").ext {
        ProjectileExt::FireWall(f) => f,
        e => panic!("FireWallHazard_C without its state: {e:?}"),
    }
}

fn wall_mut(w: &mut World, pr: ProjectileId) -> &mut FireWall {
    match &mut w.actors.projectiles.get_mut(pr).expect("a live projectile").ext {
        ProjectileExt::FireWall(f) => f,
        e => panic!("FireWallHazard_C without its state: {e:?}"),
    }
}

// ---- the overlaps

/// The fighters GetOverlappingActors gives for the Box (object type HitBox, GameTraceChannel3; it overlaps Pawn,
/// PhysicsBody, DamageDestructible, ProjectileDestructible, ItemDestructible and HurtBox): a fighter whose root
/// capsule or hurt shapes it touches (`box_hurt_hit`, as Iron Giant's thrusters), the wall's own team included.
// Unverified: a fighter out of the match (dead) is left out; whether the build's overlap list still holds one is not
// read.
fn box_fighters(w: &mut World, pr: ProjectileId) -> Vec<FighterId> {
    let x = w.actors.projectiles.get(pr).expect("a live projectile").x;
    let y = w.actors.projectiles.get(pr).expect("a live projectile").y;
    let f = wall(w, pr);
    let (cy, hx, hy) = (y + f.box_z, f.box_hx, f.box_hy);
    let mut out = Vec::new();
    for o in crate::registry::props::all_fighters(w) {
        if w.fighter(o).life.dead != 0.0 {
            continue;
        }
        let mut vp = Vec::new();
        if crate::fighter::anim::sample_boxes(w, o, &mut vp)
            && crate::combat::hit::box_hurt_hit(w, o, x, cy, hx, hy, &vp)
        {
            out.push(o);
        }
    }
    out
}

/// The Box's begin overlap with another spawned actor. Buff_ElementalParent binds the Box's OnComponentBeginOverlap
/// (@1674-1761) to ElementalOnBoxBeginOverlap; Buff_Elemental_Fire's @12718 calls ElementalFireOverlap(other) @12942
/// when the owner's BPI_ShouldActorOverlap is false (the wall's: the parent's false). ElementalFireOverlap @0-2037
/// (both actors have a Comp_Actor_Destructible: every SpawnedActorParent has one) gives an actor of the same team
/// that is a SpawnedActorParent with a state manager and no Buff_Elemental_Fireproof SpawnBuffActor(
/// Buff_Elemental_Fire, other, other, CurrentStackAmount, 0.0) @1949 (`elem_overlap`). The Box overlaps
/// ProjectileDestructible (the projectiles' HurtBox) and ItemDestructible (an item's Sphere: `hurt_radius`, 0 while it
/// is off).
// Unverified: only those shapes are met. The Box overlaps PhysicsBody, DamageDestructible and HurtBox too (many
// projectiles' root Capsule is a PhysicsBody of profile ProjectileNavPlatform): whether a second component of the
// same actor raises a second begin (the build's event is per component pair; a second SpawnBuffActor adds stacks to
// the live buff) is not decoded. Tested in the play plane after the wall's own move (`moved`); an overlap that
// another actor's later move of the frame begins is met at the wall's next move. An actor already inside the Box
// when the wall spawns begins at the wall's first move: whether the spawn's initial overlaps come before the
// elemental's bind (the passive manager's buffs are spawned inside the wall's BeginPlay) is not read.
fn box_actors(w: &mut World, pr: ProjectileId) {
    let a = ActorRef::Projectile(pr);
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let f = wall(w, pr);
    let (cx, cy, hx, hy) = (p.x, p.y + f.box_z, f.box_hx, f.box_hy);
    let ids: Vec<ProjectileId> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for id in ids {
        if id == pr {
            continue;
        }
        let Some(o) = w.actors.projectiles.get(id) else { continue };
        let seq = o.seq;
        let over = !o.dead && o.hurt_radius > 0.0 && hurt_to_box2(o, cx, cy, hx, hy) <= o.hurt_radius * o.hurt_radius;
        let f = wall_mut(w, pr);
        let k = f.in_proj.iter().position(|&s| s == seq);
        if !over {
            if let Some(k) = k {
                f.in_proj.remove(k);
            }
            continue;
        }
        if k.is_some() {
            continue;
        }
        f.in_proj.push(seq);
        elem_overlap(w, a, ActorRef::Projectile(id));
    }
    let ids: Vec<_> = w.actors.items.iter().map(|(id, _)| id).collect();
    for id in ids {
        let Some(it) = w.actors.items.get(id) else { continue };
        let seq = it.seq;
        // (a stage item no one has picked up has its Sphere off)
        let r = if it.dead || it.owner.is_none() { 0.0 } else { it.hurt_radius() };
        let over = r > 0.0 && item_to_box2(it.x, it.y, cx, cy, hx, hy) <= r * r;
        let f = wall_mut(w, pr);
        let k = f.in_items.iter().position(|&s| s == seq);
        if !over {
            if let Some(k) = k {
                f.in_items.remove(k);
            }
            continue;
        }
        if k.is_some() {
            continue;
        }
        f.in_items.push(seq);
        if crate::actors::item::is_owned(w, id) {
            elem_overlap(w, a, ActorRef::Item(id));
        }
    }
    // An actor the world has dropped ends its overlap.
    let live_p: Vec<i32> = w.actors.projectiles.iter().map(|(_, p)| p.seq).collect();
    let live_i: Vec<i32> = w.actors.items.iter().map(|(_, i)| i.seq).collect();
    let f = wall_mut(w, pr);
    f.in_proj.retain(|s| live_p.contains(s));
    f.in_items.retain(|s| live_i.contains(s));
}

/// Squared distance from a projectile's hurt core (its centre, or another wall's capsule axis) to the box.
fn hurt_to_box2(o: &Projectile, cx: Real, cy: Real, hx: Real, hy: Real) -> Real {
    let half = match &o.ext {
        ProjectileExt::FireWall(f) => f.hurt_half,
        _ => 0.0,
    };
    let dx = max((crate::actors::projectile::hurt_x(o) - cx).abs() - hx, 0.0);
    let dy = max((crate::actors::projectile::hurt_y(o) - cy).abs() - hy - half, 0.0);
    dx * dx + dy * dy
}

fn item_to_box2(x: Real, y: Real, cx: Real, cy: Real, hx: Real, hy: Real) -> Real {
    let dx = max((x - cx).abs() - hx, 0.0);
    let dy = max((y - cy).abs() - hy, 0.0);
    dx * dx + dy * dy
}

// ---- the kind

/// ReceiveBeginPlay @453: the parent's; then, with a BaseFighter instigator (@493; a wall the world spawned has none:
/// the cast's failure pops the flow @558, so of what follows only the BuffEnding bind runs):
/// PF_Post_Event(sfx_cha_reindog_firewall_impact) on itself @568, PF_Post_Event_At_Location(
/// sfx_cha_reindog_firewall_loop) at its location @661 (its playing ID kept: Ak Ground Fire Loop @747), the Team of
/// its ParticleSystem and of its Decal's dynamic material (@774-1225: the drawing side); then the instigator's
/// Comp_Pawn_FireWall.FireWallOn(self) when it has one @1280-1461; then its state manager's BuffEnding bound to
/// ABuffIsEnding @1499-1522.
/// The passive rows (Comp_Pawn_PassiveBuffManager) came ahead (the parent's BeginPlay): the Box makes
/// Buff_Elemental_Fire's 0.5 s timer run (BPI_GetBoxCollision is valid); BPI_ShouldActorOverlap is the parent's
/// false; not pooled.
fn begin(w: &mut World, pr: ProjectileId) {
    let a = ActorRef::Projectile(pr);
    let (fire, ice) = (elem_of(w, a, ELEM_FIRE).is_some(), elem_of(w, a, ELEM_ICE).is_some());
    let f = wall_mut(w, pr);
    f.had_fire = fire;
    f.had_ice = ice;
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let (owner, x, y, seq) = (p.owner, p.x, p.y, p.seq);
    let Some(o) = owner else { return };
    let id = w.fighter(o).id;
    w.events.push(Event::Sound {
        fighter: id,
        ev: name!("sfx_cha_reindog_firewall_impact"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    // Deviation: the build posts the loop at the wall's location (PF_Post_Event_At_Location @661); here it is a
    // SoundStart on the owner fighter, with no place of its own.
    w.events.push(Event::SoundStart { fighter: id, ev: name!("sfx_cha_reindog_firewall_loop"), key: loop_key(seq) });
    fire_wall_on(w, o, seq);
}

fn loop_key(seq: i32) -> SoundKey {
    SoundKey { tag: name!("firewall"), n: seq }
}

/// The Box's overlaps with the other spawned actors, where its move ends.
fn moved(w: &mut World, pr: ProjectileId) {
    if !w.actors.projectiles.get(pr).is_some_and(|p| p.dead) {
        box_actors(w, pr);
    }
}

/// ReceiveTick @1878: first the parent's ReceiveTick (SpawnedActorParent @8771: SpawnedActorParentVelocity =
/// GetVelocity, then CheckBlastBoxExit; the port's projectile step does both for every projectile, and nothing reads
/// the wall's SpawnedActorParentVelocity: it has no hit). Then every actor of the game state's
/// BPI_GetWallbounceCollisions gets BPI_IgnoreWallbounceCollision(self, true) @20-263. Then the elemental's timer
/// (`elem_tick`: the timer pass, here inside the actor's tick, as for every elemental owner) and ABuffIsEnding.
// Not ported: the BPI_IgnoreWallbounceCollision call takes the wall's root (a ProjectileNavPlatform capsule, object
// type PhysicsBody) off a spawned platform's WallbounceCollision; the port's projectile move meets the nav parts of
// the blockers for every projectile alike, so a WallbounceCollision that blocks PhysicsBody would stop the wall.
fn step(w: &mut World, pr: ProjectileId) -> bool {
    let a = ActorRef::Projectile(pr);
    // elem_tick's ApplyFireOverTime reads the overlaps only on the tick its timer fires.
    let due = crate::actors::world_actor::actor_elem(w, a)
        .and_then(|e| e.as_ref())
        .is_some_and(|s| s.buffs.iter().any(|b| b.timer == Some(1)));
    let fighters = if due { box_fighters(w, pr) } else { Vec::new() };
    elem_tick(w, a, &fighters, &[]);
    buff_ended(w, pr)
}

/// ABuffIsEnding(BuffClass) @1739, bound to the state manager's BuffEnding: K2_DestroyActor @1863 when the class is
/// exactly Buff_Elemental_Ice or Buff_Elemental_Fire. The Fire row's 5 stacks go by RemoveBuffStack (Superman's ice
/// pellet takes one a touch; Buff_Elemental_Ice's overlap two) and the buff ends at 0.
// Deviation: the build destroys the wall inside the call that ended the buff; here the wall's next tick sees the
// elemental gone, one tick later at most.
fn buff_ended(w: &mut World, pr: ProjectileId) -> bool {
    let a = ActorRef::Projectile(pr);
    let (fire, ice) = (elem_of(w, a, ELEM_FIRE).is_some(), elem_of(w, a, ELEM_ICE).is_some());
    let f = wall_mut(w, pr);
    if (f.had_fire && !fire) || (f.had_ice && !ice) {
        return true;
    }
    f.had_fire = fire;
    f.had_ice = ice;
    false
}

/// ReceiveDestroyed @1564: the parent's; PF_Post_Event_At_Location(sfx_shared_combat_firewall_end) at its location
/// @1602; ExecuteActionOnPlayingID(Stop, Ak Ground Fire Loop, 800 ms, curve 4 = linear) @1688 (a wall with no
/// instigator posted no loop: the stop has no playing ID).
fn destroyed(w: &mut World, pr: ProjectileId) {
    let a = ActorRef::Projectile(pr);
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let (x, y, seq, owner) = (p.x, p.y, p.seq, p.owner);
    let s = crate::fighter::common::actor_sound_from(w, a);
    w.events.push(Event::Sound {
        fighter: s.fighter,
        ev: name!("sfx_shared_combat_firewall_end"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: s.pooled,
        key: None,
    });
    if let Some(o) = owner {
        let fighter = w.fighter(o).id;
        w.events.push(Event::SoundStop {
            fighter,
            key: loop_key(seq),
            fade: Some(0.8),
            ev: None,
            curve: None,
            cut: false,
        });
    }
}

/// The HurtBox as a body of the traces: the upright capsule of `hurt_dist2` (axis of half `hurt_half` about
/// `hurt_y`, radius `hurt_radius`) in the play plane, object type ProjectileDestructible, while `hurt_radius` > 0 (as
/// the World's hit tests take the HurtBox). What meets it: the object queries for ProjectileDestructible (the Earth
/// Smash pillar's overlap, C021_ForceField's). The root Capsule (PhysicsBody) is the traces' own gather of a
/// projectile's root; the Box is of object type HitBox, which no query gathers.
fn bodies(w: &World, a: ActorRef, out: &mut Vec<TraceBody>) {
    let ActorRef::Projectile(id) = a else { return };
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    if p.hurt_radius <= 0.0 || p.hurt_radius.is_nan() {
        return;
    }
    let owner = crate::fighter::common::lookup_owner(w, a);
    let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), p.cls);
    let Some(info) = body_info(comps.and_then(|c| comp(c, "HurtBox")), None).filter(|i| i.on) else { return };
    let hy = crate::actors::projectile::hurt_y(p);
    let half = wall(w, id).hurt_half;
    out.push(TraceBody {
        ax: p.x,
        ay: hy - half,
        bx: p.x,
        by: hy + half,
        hx: 0.0,
        hy: 0.0,
        r: p.hurt_radius,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component: name!("HurtBox"),
        actor: Some(a),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// BPI_EnvironmentalHazard (C019ScrubShield). What its Sphere overlaps: the Box (object type HitBox, Overlap to
/// ProjectileDestructible, no graph turns it off); the HurtBox is of the Sphere's own type (Ignore) and the root
/// Capsule a PhysicsBody (Ignore); the parent's hit box is off (HitBoxOn? false).
fn clean_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Projectile(id) = a else { return false };
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    let f = wall(w, id);
    if p.dead || !(f.box_hx > 0.0 && f.box_hy > 0.0) {
        return false;
    }
    let dx = max((x - p.x).abs() - f.box_hx, 0.0);
    let dy = max((y - (p.y + f.box_z)).abs() - f.box_hy, 0.0);
    dx * dx + dy * dy <= r * r
}

/// BPI Clean @0-15: Blocked(null), the parent's (no override: DestroyOnBlocked -> Delay(0.0) ->
/// DestroySpawnedActor), Success = true.
fn clean(w: &mut World, a: ActorRef) -> bool {
    let ActorRef::Projectile(id) = a else { return false };
    let p = w.actors.projectiles.get(id).expect("a live projectile");
    if crate::actors::projectile::truthy(crate::actors::projectile::class_p(w, p).get("DestroyOnBlocked")) {
        w.actors.projectiles.get_mut(id).expect("a live projectile").blocked_destroy = true;
    }
    true
}

/// The class's `projectile` kind.
pub fn kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        bodies: Some(bodies),
        begin: Some(begin),
        moved: Some(moved),
        step: Some(step),
        destroyed: Some(destroyed),
        ..ProjectileHooks::default()
    }
}

/// The class's entry (`FIRE_WALL_ENTRY`); FireStormHazard_C (a child whose ReceiveBeginPlay calls this one's first and
/// adds only a drawn growth) enters the same parts. The wall's own entry is marked ownerless: every read of the owner
/// above was checked for none (a graph's spawn with owner None: [`make_world_wall`]).
pub fn entry() -> ActorEntry {
    ActorEntry {
        elemental: Some(ElementalParts { box_: true, ..ElementalParts::default() }),
        hazard: Some(HazardHooks { overlaps: clean_overlaps, clean }),
        ..ActorEntry::new(ActorClass::Projectile(ProjectileClass::FireWallHazard), ActorHooks::Projectile(kind()))
    }
}

/// `actors({ [FIRE_WALL]: { ...FIRE_WALL_ENTRY, ownerless: true } })`.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(FIRE_WALL, ActorEntry { ownerless: true, ..entry() })
}

// ---- Comp_Pawn_FireWall (a component of Pawn_Shaggy, Pawn_c016 and Superman's pawn in the data; Creature's)
// What the component holds that the port keeps: `Fighter::shapes.fire_wall`, made by the first FireWallOn of the pawn
// (a fighter that never had a wall holds none) and dropped by the world's reset (a new pawn has a new component: an
// empty FireWallArray and a Delay chain from its own BeginPlay).

/// The fighter's Comp_Pawn_FireWall's class defaults (`component`).
fn component(w: &World, f: FighterId) -> Option<&Value> {
    let ch = w.names.str(w.fighter(f).ch);
    w.data.chars.get(ch)?.fighter.components.pawn.values().find(|c| c.ty == COMPONENT).map(|c| &c.p)
}

/// The frames since the pawn was made (the world's reset: `creation_time`), as the world clock counts them: the
/// clock's own float32 steps from creation to now (GetTimeSeconds is a float32 sum; its step at a later time is not
/// DELTA_SECONDS, so a division would miscount in a long session).
fn frames_since_creation(w: &World, f: FighterId) -> i32 {
    let c = &w.fighter(f).clock;
    let (mut t, mut n) = (c.creation_time, 0);
    while t < c.time {
        t += crate::consts::DELTA_SECONDS;
        n += 1;
    }
    n
}

/// The component's state, made at its first use (a wall's FireWallOn, a perk's SetMaxFireWalls). `in_frame`: made
/// inside a frame whose `fire_wall_tick` is still to come (a wall's BeginPlay, a respawn's perk buffs); else between
/// frames (the perk's buff of a new pawn, the reset's perks), where the next `fire_wall_tick` is the next frame's.
fn made(w: &mut World, f: FighterId, in_frame: bool) -> &mut FireWallComp {
    if w.fighter(f).shapes.fire_wall.is_none() {
        // The Delay chain has run since the pawn's BeginPlay: this frame is `since` frames into it, and the check
        // runs on the frames whose count is a multiple of the period (below), this one's included (its
        // `fire_wall_tick` comes later in the frame).
        let period = to_int32(crate::fighter::common::latent_frames(0.2));
        let since = frames_since_creation(w, f);
        let wait = if in_frame { ((period - since % period) % period) + 1 } else { period - since % period };
        w.fighter_mut(f).shapes.fire_wall = Some(FireWallComp { walls: Vec::new(), wait, max: None });
    }
    w.fighter_mut(f).shapes.fire_wall.as_mut().expect("made above")
}

/// FireWallOn(FireWall) @490: Array_Add(FireWallArray, it). The wall's BeginPlay calls it on the instigator's
/// component when GetComponentByClass finds one (@1300-1380). `wall`: the wall's spawn count.
fn fire_wall_on(w: &mut World, f: FighterId, wall: i32) {
    if component(w, f).is_none() {
        return;
    }
    made(w, f, true).walls.push(wall);
}

/// SetMaxFireWalls(n) @559: MaxFirewalls = n, on the fighter's Comp_Pawn_FireWall (false: he has none). Called by
/// Buff_Creature_FireStorm's BeginPlay alone. The world's reset gives the new pawn the class data's again
/// (`fire_wall_reset` drops the state).
// Ported, not on the trace: Creature is not ported.
pub fn set_max_fire_walls(w: &mut World, f: FighterId, n: i32) -> bool {
    if component(w, f).is_none() {
        return false;
    }
    // (the reset sets creation_time to the time of the new pawn)
    let in_frame = w.fighter(f).clock.time != w.fighter(f).clock.creation_time;
    made(w, f, in_frame).max = Some(n);
    true
}

// ---- the call surface of the fire wall

/// Clears the fighter's walls.
pub fn fire_wall_reset(w: &mut World, f: FighterId) {
    w.fighter_mut(f).shapes.fire_wall = None;
}

/// The component's ReceiveBeginPlay @485 -> @15: while FireWallArray holds
/// more than MaxFirewalls its first entry is destroyed when valid @181-279 and removed (Array_RemoveItem: every entry
/// equal to it) @315-370, whether or not it was valid; then Delay(0.2) @430, which resumes at @15. So the check runs at
/// the pawn's BeginPlay and every 0.2 s after it, and takes out one entry a pass at most: a dead wall still counts
/// until it is the first entry over the limit. The Delay is a component's: the world's leftover latent pass counts
/// it, ahead of the timer pass (this call).
// Unverified: the phase. The pass is taken to count the Delay made at the pawn's BeginPlay from the first frame after
// it (the check on the frames 12, 24, ... after the world's reset).
pub fn fire_wall_tick(w: &mut World, f: FighterId) {
    let max = {
        let Some(c) = w.fighter(f).shapes.fire_wall.as_ref() else { return };
        if c.wait - 1 > 0 {
            None
        } else {
            Some(c.max.map(crate::real).or_else(|| {
                component(w, f)
                    .and_then(|p| p.as_obj())
                    .and_then(|p| p.get("MaxFirewalls"))
                    .and_then(|v| v.as_f64())
                    .map(crate::math::narrow)
            }))
        }
    };
    let wait = crate::math::to_int32(crate::fighter::common::latent_frames(0.2));
    let c = w.fighter_mut(f).shapes.fire_wall.as_mut().expect("checked above");
    c.wait -= 1;
    let Some(max) = max else { return };
    c.wait = wait;
    let Some(max) = max else {
        crate::actors::world_actor::warn("Comp_Pawn_FireWall_C: no MaxFirewalls in the fighter's data");
        return;
    };
    let n = c.walls.len();
    if crate::real(u32::try_from(n).unwrap_or(u32::MAX)) <= max {
        return;
    }
    let first = c.walls[0];
    let pr = w.actors.projectiles.iter().find(|(_, p)| p.seq == first).filter(|(_, p)| !p.dead).map(|(id, _)| id);
    if let Some(pr) = pr {
        crate::world::spawning::destroy_projectile(w, pr, true);
    }
    if let Some(c) = w.fighter_mut(f).shapes.fire_wall.as_mut() {
        c.walls.retain(|x| *x != first);
    }
}
