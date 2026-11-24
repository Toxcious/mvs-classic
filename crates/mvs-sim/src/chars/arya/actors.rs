//! Arya's actors of her Disguise's unmasking and of her knock-out, both direct children of
//! SpawnedActorParent with no movement of the parent's own in use (so neither is a projectile): AryaDisguiseShock_C
//! ([`DisguiseShock`]) and AryaKillPie_C ([`KillPie`]), world actors. The class data is her data file's `actors`
//! (Comp_Spawner and SpawnedActorParent for the parent's rules).
//!
//! ---- AryaDisguiseShock_C ----
//! Spawned by the Spawner notify of Arya_DisguiseAttack_Ground_Montage / _Air_montage (AttachToActor, socket None,
//! Translation (0, 0, 150), DestroyAllActorsOnNotifyEnd) and woken by the montage's ActivateSpawned notify
//! (ActivateLastObject), about 0.64 s later. Comp_Spawner HandleActorPostSpawn: the socket is None, so @10232-10321
//! takes K2_AttachToActor(owner, None) @10335 (the mesh attach @10581 is for a named socket without WorldTransforms
//! only), and WorldTransforms is false, so @10515 K2_SetActorRelativeTransform sets the notify's transform relative
//! to the owner's root: the owner's actor location + (0, 0, 150), not scaled by the mesh and not moved by the mesh's
//! hit wobble or bounce offset (the barrier and the shield actor hold the same branch). The root Capsule has
//! bAbsoluteScale, so the Sphere keeps its radius.
//! ReceiveBeginPlay @1087 (after the parent's): the particle systems, the materials and `Windup Visual`'s visibility:
//! all of it the drawing's.
//! ActivateObject @65: the parent's, MulticastActivate @75 (ParticleSystem.Activate(true) @1049), HitBoxOn? = true
//! @89, UpdateHitbox @100, Delay(Duration_0 = 3.0) @114 -> K2_DestroyActor @15. The notify's end (0.33 to 0.49 s
//! after the activation) destroys the actor first in both montages; the Delay is counted all the same.
//! The hit box is the Sphere (radius 375, tag HitBox) at the root. Sphere2D (tag "Hitbox2D") is not used: Arya's
//! Use 2D Hitboxes is false. HitBoxDamage: Damage 10, BaseKnockBack 0, KnockBackScaling 0, HitStunMult 0,
//! HitPauseTime 0, DoLOSCheck, ArmorPierce, HitEachActorOnlyOnce; On Hit Buffs: Buff_Fear_C on an enemy hit for
//! 2.0 s. DestroyedOnHit false, no MaxLifetime, Die With Instigator and DestroyOnExitedArena? (the world's and
//! the 0.1 s timer's below).
// Unverified: the Delay's first counted tick is the activation frame's (the actor ticks behind its owner's montage
// in the port; the order of the two in the build is not read).
// Deviation: the Sphere is tested against the fighters' hurt shapes as a sphere at depth 0, each frame while it is on
// (a fighter that stops being invulnerable inside it is hit: the projectile test's model, not the begin of an
// overlap). It is not tested against a projectile's HurtBox, an item or another actor, and NotifyAllyOfHit is not
// called for an ally inside it.
//! ReceiveChargeValuesFromInstigator is true in the class data: the world's copy (neither montage charges, so the
//! values are the defaults).
//! The parent's BeginPlay blast-box loop at the spawn and CheckBlastBoxExit on its 0.1 s timer, against the bounds the
//! actor stored.
//!
//! ---- AryaKillPie_C ----
//! Spawned by Pawn_Arya OnPlayerKnockedOut @11650 through Comp_Pawn_SpawnObjectRandomArea.SpawnObject
//! (`actors::spawn_area`): at the pawn's place + (0, 0, 200) in world space, direction (0, 0, 1), InitialSpeed 0,
//! MaxSpeed 0. EnableProjectileMovement is true and the class authors no ProjectileMovement template, so the
//! component has the native constructor's values (the UProjectileMovementComponent ctor 142983a00): gravity scale 1,
//! and the request's MaxSpeed 0 is no limit. It starts at rest and falls.
//! The root Capsule (half height 112, radius 112: a sphere; profile ProjectileNavPlatform, which blocks WorldStatic
//! and Platform) is the swept component.
//! ReceiveBeginPlay @1659 (after the parent's) -> @967: off a dedicated server VisualIndicator @1031 (the emitter and
//! a pop text: the drawing's); with authority @1075 a SphereTraceSingle from the actor 375 down (radius 70, trace type
//! 3 ProjectileGroundTrace) whose HitActor is cast to DestructibleObjectBP @1457: on success that object's
//! ObjectDestroyed is bound to Destroy @1536-1559 (K2_DestroyActor @2726; [`bind_ground_destroyed`]). The trace has
//! no other effect. Then Delay(Duration = 14.0) @1600 -> K2_DestroyActor @15.
//! Sphere (radius 100 at (0, 0, -40) of the root; object type HitBox) OnComponentBeginOverlap @30, with authority:
//! the other actor is a BP_Fighter_Character, is not GetOwner() @155, has a valid Comp_Actor_Destructible, and its
//! TeamIndex differs from the pie's @426-440 (the pie's is the instigator's: SpawnedActorParent @11258); not
//! IsInvulnerable? @537; a valid Comp_State_Manager. Then SpawnBuffActor(Buff_PieHeal_C, the fighter, Instigator, 0
//! stacks, 0.0 s) @713, SpawnBuffActor(Buff_SlowDown_C, the fighter, Instigator, 0 stacks, 3.0 s) @868 and
//! K2_DestroyActor @952. So an ENEMY of Arya's team eats it: healed by up to 6 and slowed for 3 s. Arya and her
//! allies pass through it and leave it where it is.
//! HitBoxOn? is false and nothing switches it on: the class's HitBoxDamage never lands.
//! NumberOfActorsAllowed is 0 (the parent's limit @5864 needs more than 0): a second pie lives beside the first.
//! Die With Instigator: Arya's own ring-out ends her pies (the world's). DestroyOnExitedArena?: the timer below.
// Unverified: bShouldBounce is taken as the engine default false (not authored; the ctor row does not list it): the
// first blocking hit ends the movement (UProjectileMovementComponent::HandleImpact -> StopSimulating, stock 4.26) and
// the pie stays there for good, also when what it rests on goes away (a ship piece). The sweep is a sphere straight
// down against the stage block and the nav blockers, the contact is exact (no pull-back gap). The spawn moves the pie
// out of a box first (FindTeleportSpot); a pie left inside one (no free spot found) stops where it is.
// Unverified: the overlap is kept per fighter, on its hurt shapes (a HitBox-type sphere meets HurtBox capsules, not
// root capsules: BaseFighter's ExperimentalHurtBoxes): the build raises the event per component, so an enemy that
// was invulnerable when its first shape came in can still eat the pie when another shape comes in; here it has to
// leave the sphere and come back.
// Unverified: the pop text's string is an FText literal the pseudo text does not render: the drawing's.
// The heal's sound (Buff_PieHeal @922) is not in the audio set.

use indexmap::IndexSet;
use mvs_collision::Ground;

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::spawn_buff;
use crate::combat::elem::charge_pct;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::consts::{DELTA_SECONDS, GRAVITY_Z};
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, sphere_trace_single};
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::{self, to_int32};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::find_teleport_spot;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::request_props;
use crate::world::wall_bp::{WallActor, bind_ground_destroyed, wall_delegate_fns};

const SHOCK: &str = "AryaDisguiseShock_C";
const PIE: &str = "AryaKillPie_C";

/// A truthy number as JavaScript reads it: not 0, not NaN.
fn truthy(v: Option<Real>) -> bool {
    v.is_some_and(|v| v != 0.0 && !v.is_nan())
}

// ------------------------------------------------------------------ AryaDisguiseShock

/// `DisguiseShock`. For the drawing: (`x`, `y`) the actor (the sphere's centre), `radius` the Sphere's;
/// PS_Arya_FearWarning plays from the spawn, PS_Arya_Fear from `activated` on; `Windup Visual` is hidden by BeginPlay
/// and never shown. The Team parameter is the owner's.
#[derive(State, Debug)]
pub struct DisguiseShock {
    pub attack_id: i32,
    /// HitBoxDamage.
    pub hit: HitPacket,
    pub facing: Real,
    /// Sphere.SphereRadius (unscaled: the root's scale is taken at the read).
    pub radius: Real,
    /// The notify's translation Z, relative to the owner's actor location (unscaled).
    pub up: Real,
    pub x: Real,
    pub y: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// ActivateObject has run (ParticleSystem is on).
    pub activated: bool,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// Ticks left of Delay(Duration_0), 0: not running.
    pub wait: i32,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
}

impl Default for DisguiseShock {
    /// The field initial values.
    fn default() -> Self {
        DisguiseShock {
            attack_id: 0,
            hit: HitPacket::default(),
            facing: 0.0,
            radius: 0.0,
            up: 0.0,
            x: 0.0,
            y: 0.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            activated: false,
            hit_on: false,
            wait: 0,
            hit_set: IndexSet::new(),
        }
    }
}

fn shock_of(w: &World, id: ActorId) -> Option<&DisguiseShock> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::DisguiseShock(s) => Some(s),
        _ => None,
    }
}

fn shock_mut(w: &mut World, id: ActorId) -> Option<&mut DisguiseShock> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::DisguiseShock(s) => Some(s),
        _ => None,
    }
}

/// The kind's `spawn`: the state around the class data, attached to the owner (the follow), then the parent's BeginPlay
/// blast-box loop.
fn shock_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let mut s = DisguiseShock {
        attack_id: r.attack_id,
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None),
        facing: crate::fighter::facing(w, f),
        radius: obj(&a.components, "Sphere").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0),
        hit_on: flag(&a.p, "HitBoxOn?"),
        ..DisguiseShock::default()
    };
    if s.radius == 0.0 {
        warn_text(w, &format!("{SHOCK}: no Sphere radius in the data"));
    }
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    let (tx, ty) = (t.and_then(|t| num(t, "X")), t.and_then(|t| num(t, "Y")));
    // A named socket or WorldTransforms is another branch of Comp_Spawner; neither montage has an X or Y
    // translation: refused, not guessed.
    let socket = text(&np, "Socket").filter(|s| !s.is_empty() && *s != "None");
    if !flag(&np, "AttachToActor") || flag(&np, "WorldTransforms") || socket.is_some() || truthy(tx) || truthy(ty) {
        warn_text(
            w,
            &format!(
                "{SHOCK}: a spawn that is not attached to the owner (no socket) with a Z translation alone is not ported"
            ),
        );
    }
    s.up = t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
    // the attachment: the owner's actor location and the relative translation
    s.x = w.fighter(f).body.x;
    s.y = actor_y(w, f) + s.up;
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    (WorldActorState::DisguiseShock(s), dead)
}

/// ActivateObject. A second call starts no second Delay (a Delay that is pending ignores the call) and has nothing else
/// to change.
fn shock_activate(w: &mut World, id: ActorId) {
    let delay = num(actor_p(w, ActorRef::Actor(id)), "Duration_0").unwrap_or(0.0);
    let Some(s) = shock_mut(w, id) else { return };
    s.activated = true;
    s.hit_on = true;
    if s.wait == 0 {
        s.wait = to_int32(latent_frames(delay));
    }
}

fn shock_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let f = a.owner;
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let (ox, oy) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(s) = shock_mut(w, id) else { return };
    s.age += 1;
    s.x = ox;
    s.y = oy + s.up;
    // Delay(Duration_0) -> K2_DestroyActor.
    let mut dead = false;
    if s.wait > 0 {
        s.wait -= 1;
        dead = s.wait == 0;
    }
    // CheckBlastBoxExit on its 0.1 s timer: the spawn frame is not counted.
    if !dead {
        dead = blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y);
    }
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The Sphere (object type HitBox) where it is now: none while HitBoxOn? is false.
/// The root's absolute scale is the Sphere's parent (SetActorScale3D).
fn shock_sphere(s: &DisguiseShock, dead: bool, scale: Option<Real>) -> Option<WorldSphere> {
    if !s.hit_on || dead {
        return None;
    }
    Some(WorldSphere { x: s.x, y: s.y, z: Some(0.0), r: s.radius * scale.unwrap_or(1.0), ..WorldSphere::default() })
}

fn shock_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let (Some(a), Some(s)) = (w.actors.actors.get(id), shock_of(w, id)) else { return false };
    match shock_sphere(s, a.dead, a.size_scale) {
        Some(sp) => {
            out.push(sp);
            true
        }
        None => false,
    }
}

/// The Sphere against the enemies' hurt shapes: the handler's pfgDoDamage gates in the projectile test's order.
fn shock_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (Some(a), Some(s)) = (w.actors.actors.get(id), shock_of(w, id)) else { return };
    let Some(sphere) = shock_sphere(s, a.dead, a.size_scale) else { return };
    let (f, attack_id, hit) = (a.owner, s.attack_id, s.hit.clone());
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = shock_of(w, id) else { return };
        if w.fighter(vic).team.team == team
            || !alive(w, vic)
            || crate::combat::phases::no_hit_by_team(w, team, vic)
            || s.hit_set.contains(&vid)
        {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = hurt_hit(w, vic, std::slice::from_ref(&sphere), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, sphere.x, sphere.y, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = shock_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        // the On Hit Buffs row (Buff_Fear_C, 2.0 s) with it
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: sphere.x, y: sphere.y }, None, &[], charge_pct(w, me))
        {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

// ------------------------------------------------------------------ AryaKillPie

/// `KillPie`. For the drawing: (`x`, `y`) the actor (the root sphere's centre); the mesh ScoobySnackStaticMesh is
/// BugsBunny_Prop_Pie at relative (0, 0, -125), yaw 90, scale (1.5, 0.35, 3.0); PS_Arya_Pie is attached to the root
/// from the spawn with the owner's Team parameter; the pop text shows for the first 1.25 s (`age` in ticks) 150 over
/// the actor. `stopped`: it has landed. It ends without an effect of its own (the eater's buffs have theirs).
#[derive(State, Debug)]
pub struct KillPie {
    /// The root Capsule's radius (a sphere: its half height is the radius).
    pub radius: Real,
    /// Sphere.SphereRadius.
    pub eat_radius: Real,
    /// Sphere.RelativeLocation.Z.
    pub eat_up: Real,
    /// Ticks of Delay(Duration), the spawn frame counted.
    pub life: i32,
    pub x: Real,
    pub y: Real,
    /// ProjectileMovement's velocity Z.
    pub vy: Real,
    /// StopSimulating has run.
    pub stopped: bool,
    pub age: i32,
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// The fighters inside the Sphere, by id.
    pub over: IndexSet<i32>,
}

impl Default for KillPie {
    /// The field initial values.
    fn default() -> Self {
        KillPie {
            radius: 0.0,
            eat_radius: 0.0,
            eat_up: 0.0,
            life: 0,
            x: 0.0,
            y: 0.0,
            vy: 0.0,
            stopped: false,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
            over: IndexSet::new(),
        }
    }
}

fn pie_of(w: &World, id: ActorId) -> Option<&KillPie> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::KillPie(s) => Some(s),
        _ => None,
    }
}

fn pie_mut(w: &mut World, id: ActorId) -> Option<&mut KillPie> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::KillPie(s) => Some(s),
        _ => None,
    }
}

/// The pie's Destroy, bound to the ground's ObjectDestroyed: K2_DestroyActor @2726.
const PIE_DESTROY: &str = "AryaKillPie_C.Destroy";

fn pie_destroy(w: &mut World, a: ActorRef, _wall: WallActor) {
    if let ActorRef::Actor(id) = a
        && let Some(x) = w.actors.actors.get_mut(id)
    {
        x.dead = true;
    }
}

/// The rest of the pie's spawn once it is listed: the trace @1075-1559 and `bindGroundDestroyed`
/// the hit actor cast to DestructibleObjectBP has ObjectDestroyed bound to the pie's Destroy; a
/// failed cast binds nothing.
// Deviation: run once the pie is listed (the World's `spawned` hook, the first place its id exists), so not for a
// pie its parent's BeginPlay already destroyed: that one is bound too, and a broadcast skips and drops it, so
// only a wall's listener list in the record differs until that wall's next broadcast.
fn pie_spawned(w: &mut World, id: ActorId) {
    let Some(p) = pie_of(w, id) else { return };
    let (x, y) = (p.x, p.y);
    let me = ActorRef::Actor(id);
    let hit = sphere_trace_single(w, x, y, x, y - 375.0, 70.0, TRACE_PROJECTILE_GROUND, &[], Some(me));
    bind_ground_destroyed(w, hit.as_ref(), me, PIE_DESTROY);
}

/// The height of the sphere's centre where it rests on the box `g` at the column `x`, or `None` when it passes beside.
fn rest_on(g: &Ground, x: Real, r: Real) -> Option<Real> {
    let dx = math::max(math::max(crate::math::narrow(g.left - f64::from(x)), 0.0), x - crate::math::narrow(g.right));
    if dx >= r {
        return None;
    }
    Some(crate::math::narrow(g.top + f64::from((r * r - dx * dx).sqrt())))
}

/// ProjectileMovement's tick: gravity on a velocity that starts at 0 (v' = v - g dt, the move v dt + (v' - v) dt / 2),
/// swept straight down; the first blocking hit stops it for good.
fn pie_fall(w: &mut World, id: ActorId) {
    let shapes = w.blocker_shapes(BlockKind::Nav);
    let boxes = &shapes.boxes;
    let ground = w.stage.ground;
    let Some(a) = pie_mut(w, id) else { return };
    let dt = DELTA_SECONDS;
    let v0 = a.vy;
    let v1 = v0 - GRAVITY_Z * dt;
    let to = a.y + v0 * dt + ((v1 - v0) * dt) / 2.0;
    let mut land = Real::NEG_INFINITY;
    for k in 0..=boxes.len() {
        let g = if k == 0 { &ground } else { &boxes[k - 1] };
        let Some(top) = rest_on(g, a.x, a.radius) else { continue };
        if a.y < top {
            // Not above it: inside the box (stops where it is: the header's Unverified note) or below it (falls on).
            if a.y > crate::math::narrow(g.bottom - f64::from(top - crate::math::narrow(g.top))) {
                land = math::max(land, a.y);
            }
            continue;
        }
        if to <= top {
            land = math::max(land, top);
        }
    }
    // a polygon solid: the sphere swept down against it (the start inside or touching: stops where it is)
    for q in &shapes.polys {
        if let Some(h) = q.sweep(a.x, a.y, 0.0, to - a.y, 0.0, 0.0, a.radius) {
            land = math::max(land, if h.initial { a.y } else { crate::math::narrow(h.y) });
        }
    }
    if land == Real::NEG_INFINITY {
        a.y = to;
        a.vy = v1;
        return;
    }
    a.y = land;
    a.vy = 0.0;
    a.stopped = true;
}

fn pie_step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = pie_mut(w, id) else { return };
    s.age += 1;
    let stopped = s.stopped;
    if !stopped {
        pie_fall(w, id);
    }
    let Some(s) = pie_mut(w, id) else { return };
    // Delay(Duration) -> K2_DestroyActor; the blast box timer behind it, as the projectile's expiry counts it.
    let dead =
        s.age >= s.life || blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y);
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// Sphere's OnComponentBeginOverlap, where the pie and the fighters stand at the frame's end.
fn pie_hits(w: &mut World, id: ActorId) {
    let (Some(a), Some(s)) = (w.actors.actors.get(id), pie_of(w, id)) else { return };
    let f = a.owner;
    // a sphere at the play plane's depth (z 0), not one extruded along it
    let sphere = WorldSphere { x: s.x, y: s.y + s.eat_up, z: Some(0.0), r: s.eat_radius, ..WorldSphere::default() };
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let inside = vic != f && alive(w, vic) && {
            match victim_pose(w, vic) {
                Some(vp) => {
                    let met = hurt_hit(w, vic, std::slice::from_ref(&sphere), &vp).is_some();
                    give_vbuf(w, vp);
                    met
                }
                None => false,
            }
        };
        let Some(s) = pie_mut(w, id) else { return };
        if !inside {
            s.over.shift_remove(&vid);
            continue;
        }
        if !s.over.insert(vid) {
            continue;
        }
        if w.fighter(vic).team.team == team || invulnerable(w, vic) {
            continue;
        }
        spawn_buff(w, vic, name!("Buff_PieHeal_C"), 0.0, Some(0), Some(f), true, None);
        spawn_buff(w, vic, name!("Buff_SlowDown_C"), 3.0, Some(0), Some(f), true, None);
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
}

/// The kind's `spawn`, with collision handling 2 (AdjustIfPossibleButAlwaysSpawn, Comp_Spawner @9054 / @9531):
/// FindTeleportSpot moves the root sphere out of the boxes it is spawned in before BeginPlay, against what blocks it
/// (ProjectileNavPlatform: WorldStatic and Platform, the boxes its fall is swept against); where it finds no free spot
/// it stays in place.
fn pie_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let (cap, sp) = (obj(&a.components, "Capsule"), obj(&a.components, "Sphere"));
    let mut pie = KillPie {
        radius: cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(0.0),
        eat_radius: sp.and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0),
        eat_up: sp.and_then(|c| obj(c, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0),
        life: to_int32(latent_frames(num(&a.p, "Duration").unwrap_or(0.0))),
        stopped: !flag(&a.p, "EnableProjectileMovement"),
        ..KillPie::default()
    };
    if pie.radius == 0.0 || cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(0.0) != pie.radius {
        warn_text(w, &format!("{PIE}: the root is not the sphere this port sweeps"));
    }
    let t = obj(&np, "Transforms").and_then(|t| obj(t, "Translation"));
    // The one caller (`actors::spawn_area`) hands a world place and no speed.
    if !flag(&np, "WorldTransforms") || truthy(num(&np, "InitialSpeed")) {
        warn_text(w, &format!("{PIE}: a spawn that is not at a world place at rest is not ported"));
    }
    pie.x = -t.and_then(|t| num(t, "Y")).unwrap_or(0.0);
    pie.y = t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
    if pie.radius != 0.0 {
        let mut boxes = w.blocker_shapes(BlockKind::Nav);
        boxes.boxes.insert(0, w.stage.ground);
        let at = find_teleport_spot(boxes.view(), pie.x, pie.y, pie.radius);
        pie.x = at.x;
        pie.y = at.y;
    }
    let sb = spawn_box(w);
    // the parent's BeginPlay loop
    let dead = blast_box_loop(
        &sb,
        &mut pie.x,
        &mut pie.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut pie.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    // @1075-1559: pie_spawned
    (WorldActorState::KillPie(pie), dead)
}

/// This file's registry entries: the two classes and the pie's Destroy, bound to a destructible's ObjectDestroyed.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let shock = WorldActorHooks {
        spawn: Some(shock_spawn),
        // the Sphere reads the scale (shock_sphere), the view too
        size: Some(SizeHooks::default()),
        hit_shapes: Some(shock_hit_shapes),
        activate: Some(shock_activate),
        step: Some(shock_step),
        hits: Some(shock_hits),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        SHOCK,
        ActorEntry::new(ActorClass::World(WorldActorClass::AryaDisguiseShock), ActorHooks::World(shock)),
    )?;
    let pie = WorldActorHooks {
        spawn: Some(pie_spawn),
        spawned: Some(pie_spawned),
        step: Some(pie_step),
        hits: Some(pie_hits),
        ..WorldActorHooks::default()
    };
    r.actors.enter(PIE, ActorEntry::new(ActorClass::World(WorldActorClass::AryaKillPie), ActorHooks::World(pie)))?;
    wall_delegate_fns(r, &[(PIE_DESTROY, pie_destroy)])
}
