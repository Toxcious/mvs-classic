//! Superman's heat vision: SupermanGroundLazer_C and its child
//! SupermanGroundLazerAir_C (the sweep, [`LaserSweep`]) and SupermanGroundLazerDamage_C (the beam's hit,
//! [`LaserDamage`]), the laser trace.
//!
//! Sources: SupermanGroundLazer and SupermanGroundLazerDamage (the air class has no bytecode of its own) and a native
//! read; the class defaults from the data.
//!
//! The ability (slot 0: 3 charges, one back every 10 s, the montage rows refused without one) is the shared ability
//! manager's: both sweep classes have TriggerAbilityOnSpawnIndex, so the spawn spends the charge.
//!
//! ---- SupermanGroundLazer (Superman_GroundLazer_Montage) and SupermanGroundLazerAir (Superman_AirLazer_Montage) --
//! A SpawnedActorParent child attached to the pawn for the Spawner notify's length (DestroyAllActorsOnNotifyEnd).
//! It has no component tagged HitBox: its own HitBoxDamage (all zero) and On Hit Buffs never land.
//! ReceiveBeginPlay @4210: LastVector, and on the authority the instigator's Comp_Spawner "spawned actor" delegate
//! bound to OnInstigatorSpawnedActor @5278-5413.
//! ReceiveTick @5414 (authority): AngleAlpha += DeltaSeconds / AnimNotifyDuration @5473-5565 (the notify's
//! TotalDuration, handed over by Comp_Spawner through BPI_SetAnimNotifyDuration; no clamp). The ray: from the mesh
//! socket EyeLocation (X dropped) along VLerp(StartVector, EndVector, AngleAlpha) with its Y turned to the
//! instigator's facing, not normalised, times LaserLength @5727-6322, through LineTraceThroughPlatforms @6373.
//! SetBeamVisuals @7777 and the PreImpact emitter's visibility @7838 are the drawing's (the fields below).
//! With a hit @7880: HitPlatform / HitLocations get the hit actor and the ImpactPoint in that actor's space
//! @8742-8923, SpawnGroundEffectMulticast @9288 (PS_SupermanLaserPreImpact at the ImpactPoint, rotator (Roll 0,
//! Pitch -DegAtan2(Normal.Y, Normal.Z), Yaw 90), scale (s, s, 0.5) with s = FClamp(Lerp(0.5, 2,
//! NormalizeToRange(|ImpactPoint - owner location|, 200, 3500)), 0.5, 2) @11000-11392); every fighter of the trace
//! that is not in IgnitedActors yet joins it and gets SpawnBuffActor(Buff_Fire_C, fighter, instigator, 1 stack,
//! duration 0) from the instigator's state manager @2940-3355; and with no valid GroundLaserDamageActor the damage
//! actor is spawned through the instigator's Comp_Spawner at the first stored point, with this actor's AttackID @4036.
//! Then @9458: AngleAlpha >= 1 -> K2_DestroyActor.
//! Without a hit of the trace nobody is ignited, whatever fighters the ray crossed: the loop sits behind the trace's
//! return value @7880.
//! OnInstigatorSpawnedActor @11703: a SupermanGroundLazerDamage unbinds the delegate @11849-11872, a valid cached one
//! is destroyed @11956 and the new one is cached @11992. GroundLaserDamageActor has that one writer, behind the
//! unbind, so no sweep that hears the delegate holds one: the destroy never runs (two live sweeps both unbind at the
//! first damage actor of the pawn and both cache it) and is not ported. The damage actor of the sweep before lives on
//! until its own end.
//! ReceiveEndPlay @9604, however the actor ended: GroundLaserDamageActor.ActivateGroundLaserExplosion(HitPlatform,
//! HitLocations, AnimNotifyDuration). The pawn's reset destroys his actors newest first (Comp_Spawner @2781-2921):
//! the damage actor is gone before the sweep's EndPlay, which then wakes nothing.
//! The actor's delta is its own: in the owner's hit pause the montage stands and the sweep runs on to AngleAlpha >= 1.
//! The scan @15-2293 over the Sphere's overlapping actors (a wedge test against LastVector) writes nothing that is
//! read: not ported.
//!
//! LineTraceThroughPlatforms @0-3869: SphereTraceMultiForObjects, radius 5, object types WorldStatic, BlockTrace
//! (19), Platform (10) and Pawn, ignoring the owner and self. The hits are walked in order (native: sorted by Time,
//! every matching shape of the whole sweep): a fighter never ends the walk and is listed when CanBeHitBy(this
//! actor's team) and not invulnerable @974-1667; a component of object type 31 ends it when it is another team's and
//! no initial overlap @2393-2929 (31 is ECC_GameTraceChannel18, BlockTrace: C021_ForceField's TraceCollision; of its
//! own team or an initial overlap the walk goes on); a BasePlatformBP only when the ImpactPoint is more than
//! PlatformPassThroughDistance from the start @3000-3675; anything else at once @3804 (Morty's ship pieces and the
//! spawned platforms' parts: object type Platform and no BasePlatformBP @3745; WW_BracerShield's Capsule,
//! WorldStatic, of any team). No hit left: false.
//! The port's trace is `fighter::traces::sphere_trace_multi_for_objects` with those four types: every body it gathers
//! of them, its order among hits of equal Time and its shapes (its own Unverified and Deviation notes). A fighter is a
//! hit actor of the World's fighters (BPI_BaseFighter); the stage's pass-through platforms are its bodies with a
//! platform index. The normal is the ImpactNormal: the face's (the ground effect's pitch reads it @9154-9246).
//! Deviation: the stored points are world points. The build stores them in the hit actor's space
//! (InverseTransformLocation of the BasePlatformBP's Parent component, else of the HitActor @8268-8923) and takes them
//! back at the replay (SupermanGroundLazerDamage @1657 / @2758 / @3302), so the track follows what it hit when that
//! moves: no stage platform of the port moves, but an actor hit (C021_ForceField_Perk_C moving with Black Adam) keeps
//! its track where it was.
//!
//! ---- SupermanGroundLazerDamage ----
//! Spawned by the sweep's first hit with its hit box off (HitBoxOn? false), woken by the sweep's EndPlay.
//! ActivateGroundLaserExplosion @2894: the arrays and AnimTime stored, Activated, the actor at the first point @3392,
//! HitBoxOn? = true @3415 and UpdateHitbox @3426 (the overlaps are tested where the box is switched on: the first
//! tick moves it on to point 1), then the multicast @3455: Explotion (PS_SupermanLaserImpact) on with its Team
//! parameter and sfx_cha_superman_laser_hit at the actor.
//! ReceiveTick @613 (authority, Activated): CurrentTime += DeltaSeconds; q = Round(CurrentTime / (AnimTime / count));
//! the actor goes to point q, past the end to the last one, and LastUsedIndex = q (0 past the end); with q >= count
//! @510: Delay(0.15), Explotion.Deactivate, Delay(0.066), K2_DestroyActor. The hit box stays on through that tail.
//! Its hit shape is the Box (tags HitBox, HitBox2D; BoxExtent Y / Z times HitBoxScale in the play plane, 32 half
//! sizes: the constructor): HitBoxDamage 5 / 1995 / 13 straight up, HitReaction 4, HitPauseTime 0.25,
//! HitEachActorOnlyOnce, and the row Buff_Fire_C of On Hit Buffs.
//! The Box on the actors that are no fighter (its template's types: Pawn, WorldDynamic and the destructible ones) is
//! the hit box dealer's pass over the actors (`combat::hits::hit_box_actor_hits`): DestroyProjectiles of the packet
//! takes the projectiles and items it begins to overlap through do_damage.
//! A team mate in the Box gets pfgDoDamage as anyone (the graph has no ally logic of its own) behind the same gates:
//! the entry rows, the ally rows @5931-6835 (none: Buff_Fire's ApplyBuffAlly? is false) and NotifyAllyOfHit @21344
//! (`actor_ally_hit`); DamageAndLaunch between them refuses him (CanBeHitBy).
//! Deviation: tested at the actor's hit pass only ([`damage_overlaps`]), after the fighters.
//! Unverified: the order of the two actors' ticks is taken as the sweep's first: the damage actor's first replay tick
//! is the frame of the sweep's end.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::HitPacket;
use crate::combat::buffs::spawn_buff;
use crate::combat::elem::{charge_buff, charge_pct, instigator_charge};
use crate::combat::hits::{
    HitShape, actor_ally_hit, actor_hit_enemy, apply_projectile_hit, dealer, hit_box_actor_hits, hit_buffs_call,
    projectile_blocks,
};
use crate::combat::phases::{alive, give_vbuf, no_hit, victim_pose};
use crate::combat::spawned_hits::{class_rows, hit_box_types};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::{put_pose, sampled_boxes};
use crate::fighter::body::PlatformRef;
use crate::fighter::common::{actor_data, latent_frames, owner_team, warn_once, warn_text};
use crate::fighter::control::{actor_spawn_ability, attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_BLOCK_TRACE, OBJ_PAWN, OBJ_PLATFORM, OBJ_WORLD_STATIC, TraceHit, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable, mesh_wx, mesh_wy};
use crate::math::ue::{deg_atan2, round_to_int, size2d};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, num, num_in, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use indexmap::IndexSet;

const SWEEP: &str = "SupermanGroundLazer_C";
const SWEEP_AIR: &str = "SupermanGroundLazerAir_C";
const DAMAGE: &str = "SupermanGroundLazerDamage_C";
const TRACE_RADIUS: Real = 5.0;

// ------------------------------------------------------------------ the states

/// A SupermanGroundLazer or SupermanGroundLazerAir. For the drawing: the beam of the last tick runs from (`sx`, `sy`),
/// the EyeLocation socket, to (`ex`, `ey`): the ImpactPoint with `hit`, else the ray's far end; (`nx`, `ny`) is the
/// normal at the ImpactPoint; `track` holds the points stored so far (x, y pairs), `alpha` AngleAlpha.
#[derive(State, Debug, Default)]
pub struct LaserSweep {
    pub attack_id: i32,
    /// StartVector / EndVector, Y towards the facing, and Z.
    pub start_y: Real,
    pub start_z: Real,
    pub end_y: Real,
    pub end_z: Real,
    /// LaserLength.
    pub length: Real,
    /// PlatformPassThroughDistance.
    pub pass_through: Real,
    /// AnimNotifyDuration, as float32.
    pub duration: Real,
    /// AngleAlpha.
    pub alpha: Real,
    /// The Comp_Spawner delegate is bound to OnInstigatorSpawnedActor.
    pub bound: bool,
    /// GroundLaserDamageActor.
    pub damage: Option<ActorRef>,
    /// ReceiveEndPlay has run.
    pub ended: bool,
    /// IgnitedActors, by fighter id.
    pub ignited: Vec<i32>,
    /// HitLocations: x, y per stored point.
    pub track: Vec<Real>,
    pub sx: Real,
    pub sy: Real,
    pub ex: Real,
    pub ey: Real,
    pub hit: bool,
    pub nx: Real,
    pub ny: Real,
    /// The sweep's ticks.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
}

/// A SupermanGroundLazerDamage. For the drawing: (`x`, `y`) the actor, `exploding` Explotion's state (on from the
/// activation to the first delay's end), `track` the points it replays.
#[derive(State, Debug, Default)]
pub struct LaserDamage {
    pub attack_id: i32,
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    /// HitBoxDamage.
    pub hit: HitPacket,
    /// The Box's half extents in the play plane, and its half depth (BoxExtent X).
    pub hx: Real,
    pub hy: Real,
    pub hd: Real,
    /// Activated.
    pub activated: bool,
    /// HitBoxOn?.
    pub hit_on: bool,
    pub exploding: bool,
    /// HitLocations: x, y per point.
    pub track: Vec<Real>,
    /// AnimTime.
    pub anim_time: Real,
    /// CurrentTime.
    pub time: Real,
    /// LastUsedIndex.
    pub last_index: Real,
    /// Ticks since the replay passed its end, that tick counted.
    pub tail: i32,
    /// HitEachActorOnlyOnce.
    pub hit_set: IndexSet<i32>,
    /// The actors the Box overlaps now.
    pub over: IndexSet<ActorRef>,
    /// Their HitEachActorOnlyOnce track.
    pub once: IndexSet<ActorRef>,
    /// The gathered object types the Box answers.
    pub types: Vec<Real>,
}

fn sweep_of(w: &World, id: ActorId) -> Option<&LaserSweep> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::LaserSweep(s) => Some(s),
        _ => None,
    }
}

fn sweep_mut(w: &mut World, id: ActorId) -> Option<&mut LaserSweep> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::LaserSweep(s) => Some(s),
        _ => None,
    }
}

fn damage_of(w: &World, id: ActorId) -> Option<&LaserDamage> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::LaserDamage(s) => Some(s),
        _ => None,
    }
}

fn damage_mut(w: &mut World, id: ActorId) -> Option<&mut LaserDamage> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::LaserDamage(s) => Some(s),
        _ => None,
    }
}

// ------------------------------------------------------------------ the trace

/// The accepted hit of [`laser_trace`]: the ImpactPoint, the ImpactNormal and the stage platform's index (-1 for the
/// stage block, -2 for any other body).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LaserHit {
    pub x: Real,
    pub y: Real,
    pub nx: Real,
    pub ny: Real,
    pub platform: i32,
}

/// What [`laser_trace`] found: the accepted hit, if any, and the fighters listed on the way to it, in the walk's order
/// (listed either way).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LaserTrace {
    pub hit: Option<LaserHit>,
    pub fighters: Vec<FighterId>,
}

/// Comp_Actor_Destructible.TeamIndex of a hit actor (a spawned actor's owner's); an actor with none reads TeamIndex
/// through a None component, 0 (@2660-2711).
fn team_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Fighter(f) => w.fighter(f).team.team,
        a => {
            let t = owner_team(w, a);
            if t == -1 { 0 } else { t }
        }
    }
}

/// The hit's ImpactNormal: on a box, FindBoxOpposingNormal's face (among the axes of the normal with |n| > 1e-4, the
/// one with the least dot with the trace delta: the top face on a corner unless the ray runs flatter than 45
/// degrees), else the sweep's normal.
// Unverified: the boxes are the bodies the traces give a floor object (the stage block, its platforms and solid
// boxes, the ships' pieces, the actors' solids); a box an actor class's `bodies` entry adds keeps the sweep's normal.
fn impact_normal(h: &TraceHit, dx: Real, dy: Real) -> (Real, Real) {
    let boxed = match h.floor {
        Some(PlatformRef::Stage(_) | PlatformRef::Spawned(..) | PlatformRef::Solid(_) | PlatformRef::Runtime(_)) => {
            true
        }
        Some(PlatformRef::Poly(_)) => false,
        None => h.actor.is_none() && h.component == name!("Floor"),
    };
    let (mut nx, mut ny) = (h.nx, h.ny);
    let ax = nx.abs() > 1e-4;
    let ay = ny.abs() > 1e-4;
    if boxed && (ax || ay) {
        let sx = if nx > 0.0 { 1.0 } else { -1.0 };
        let sy = if ny > 0.0 { 1.0 } else { -1.0 };
        if !ay || (ax && sx * dx < sy * dy) {
            nx = sx;
            ny = 0.0;
        } else {
            nx = 0.0;
            ny = sy;
        }
    }
    (nx, ny)
}

/// LineTraceThroughPlatforms from (x0, y0) to (x0 + dx, y0 + dy) for the owner's actor `own` (ignored with the
/// owner): the accepted hit, and the fighters listed either way.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn laser_trace(
    w: &mut World,
    owner: FighterId,
    x0: Real,
    y0: Real,
    dx: Real,
    dy: Real,
    pass_through: Real,
    own: Option<ActorRef>,
) -> LaserTrace {
    let types =
        [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_BLOCK_TRACE), crate::real(OBJ_PLATFORM), crate::real(OBJ_PAWN)];
    let mut hits = std::mem::take(&mut w.fighter_mut(owner).bufs.hits);
    hits.clear();
    sphere_trace_multi_for_objects(
        w,
        x0,
        y0,
        x0 + dx,
        y0 + dy,
        TRACE_RADIUS,
        &types,
        &[Some(ActorRef::Fighter(owner))],
        own,
        &mut hits,
    );
    let team = w.fighter(owner).team.team;
    let mut out = LaserTrace::default();
    for h in &hits {
        if let Some(ActorRef::Fighter(o)) = h.actor {
            // DoesImplementInterface(BPI_BaseFighter) @687: listed when CanBeHitBy and not IsInvulnerable? @974-1667.
            if w.fighter(o).team.team != team
                && !w.fighter(o).invuln.damage_disabled
                && !invulnerable(w, o)
                && !out.fighters.contains(&o)
            {
                out.fighters.push(o);
            }
            continue;
        }
        if h.object_type == OBJ_BLOCK_TRACE {
            if !(h.actor.is_some_and(|a| team_of(w, a) != team) && !h.initial) {
                continue;
            }
        } else if h.platform >= 0 && !(size2d([h.ix - x0, h.iy - y0]) > pass_through) {
            continue;
        }
        let (nx, ny) = impact_normal(h, dx, dy);
        let platform = if h.platform >= 0 {
            h.platform
        } else if h.actor.is_none() && h.component == name!("Floor") {
            -1
        } else {
            -2
        };
        out.hit = Some(LaserHit { x: h.ix, y: h.iy, nx, ny, platform });
        break;
    }
    hits.clear();
    w.fighter_mut(owner).bufs.hits = hits;
    out
}

// ------------------------------------------------------------------ the sweep

/// Mesh.GetSocketLocation(EyeLocation): the sweep's start.
fn eye(w: &World, f: FighterId) -> (Real, Real) {
    let ch = w.fighter(f).ch;
    let at = w.fixed.samplers.get(&ch).and_then(|b| {
        let si = *b.by_name.get(&name!("socket:EyeLocation"))?;
        b.offsets.get(usize::try_from(si).ok()?).copied()
    });
    if let Some(o) = at
        && let Some(pose) = sampled_boxes(w, f)
    {
        let o = usize::try_from(o).expect("a box offset");
        let r = (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]));
        put_pose(w, pose);
        return r;
    }
    warn_once(w, name!("laser: socket EyeLocation missing"));
    (w.fighter(f).body.x, actor_y(w, f))
}

/// OnInstigatorSpawnedActor of every sweep of the pawn that is still bound, for a new damage actor: unbound, the
/// actor cached (a bound sweep holds none: the header).
fn damage_spawned(w: &mut World, d: ActorId, owner: FighterId) {
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let dead = w.actors.actors.get(x).is_none_or(|a| a.dead);
        let mine = w.actors.actors.get(x).is_some_and(|a| a.owner == owner);
        if dead || !mine {
            continue;
        }
        if let Some(l) = sweep_mut(w, x)
            && l.bound
        {
            l.bound = false;
            l.damage = Some(ActorRef::Actor(d));
        }
    }
}

/// CompSpawnActor(SupermanGroundLazerDamage) at (x, y) for the sweep `sweep`.
pub fn spawn_damage(w: &mut World, sweep: ActorId, x: Real, y: Real) -> Option<ActorId> {
    let a = w.actors.actors.get(sweep)?;
    let f = a.owner;
    let attack_id = sweep_of(w, sweep)?.attack_id;
    let data = w.data.clone();
    let cls = name!("SupermanGroundLazerDamage_C");
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), cls) else {
        warn_text(w, &format!("spawned actor not in the data ({DAMAGE})"));
        return None;
    };
    let state = new_damage(w, f, attack_id, def, x, y);
    let actor = crate::world::spawning::new_world_actor(
        w,
        cls,
        WorldActorClass::SupermanGroundLazerDamage,
        f,
        false,
        WorldActorState::LaserDamage(state),
    );
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    crate::world::spawning::comp_spawn_actor(w, id, charge, f);
    actor_spawn_ability(w, f, &def.p);
    damage_spawned(w, id, f);
    Some(id)
}

/// ReceiveEndPlay, once. A damage actor that died first (the pawn's reset: the header) is not woken.
fn sweep_end(w: &mut World, id: ActorId) {
    let Some(l) = sweep_mut(w, id) else { return };
    if l.ended {
        return;
    }
    l.ended = true;
    let (d, track, duration) = (l.damage, l.track.clone(), l.duration);
    if let Some(ActorRef::Actor(did)) = d
        && w.actors.actors.get(did).is_some_and(|x| !x.dead)
    {
        damage_activate(w, did, track, duration);
    }
}

/// The EndPlay of the sweeps that died since the last call (a notify's end, the pawn's reset), ahead of the damage
/// actors' ticks of the frame.
fn end_sweeps(w: &mut World) {
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(x).is_some_and(|a| a.dead && matches!(a.state, WorldActorState::LaserSweep(_))) {
            sweep_end(w, x);
        }
    }
}

fn set_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn sweep_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let doe = actor_data(w, &data, ActorRef::Fighter(f), cls).is_some_and(|d| flag(&d.p, "DestroyOnExitedArena?"));
    let (ox, oy) = (w.fighter(f).body.x, actor_y(w, f));
    let Some(l) = sweep_mut(w, id) else { return };
    let frames = crate::real(l.age);
    l.age += 1;
    if blast_timer_exit(doe, &mut l.blast_at, &l.blast_bounds, frames, ox, oy) {
        set_dead(w, id);
    } else {
        sweep_move(w, id, f, cls);
    }
}

fn sweep_move(w: &mut World, id: ActorId, f: FighterId, cls: crate::names::Name) {
    let Some(l) = sweep_mut(w, id) else { return };
    l.alpha += DELTA_SECONDS / l.duration;
    let (alpha, pass_through) = (l.alpha, l.pass_through);
    let ly = l.start_y + alpha * (l.end_y - l.start_y);
    let lz = l.start_z + alpha * (l.end_z - l.start_z);
    let length = l.length;
    let (sx, sy) = eye(w, f);
    let fc = facing(w, f);
    let (dx, dy) = (ly * fc * length, lz * length);
    let tr = laser_trace(w, f, sx, sy, dx, dy, pass_through, Some(ActorRef::Actor(id)));
    let Some(l) = sweep_mut(w, id) else { return };
    l.sx = sx;
    l.sy = sy;
    l.hit = tr.hit.is_some();
    if let Some(h) = tr.hit {
        let (x, y) = (h.x, h.y);
        l.ex = x;
        l.ey = y;
        l.nx = h.nx;
        l.ny = h.ny;
        l.track.push(x);
        l.track.push(y);
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        let s = min(max(0.5 + ((size2d([x - fx, y - fy]) - 200.0) / 3300.0) * 1.5, 0.5), 2.0);
        let fid = w.fighter(f).id;
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter: fid,
            vfx: Some(name!("PS_SupermanLaserPreImpact")),
            x,
            y,
            ox: 0.0,
            rot: [-deg_atan2(-h.nx, h.ny), 90.0, 0.0],
            scale: s,
            tint: None,
            to: None,
        });
        for o in tr.fighters {
            let oid = w.fighter(o).id;
            let Some(l) = sweep_mut(w, id) else { return };
            if l.ignited.contains(&oid) {
                continue;
            }
            l.ignited.push(oid);
            spawn_buff(w, o, name!("Buff_Fire_C"), 0.0, Some(1), Some(f), true, None);
        }
        let Some(l) = sweep_of(w, id) else { return };
        let none_alive = match l.damage {
            Some(ActorRef::Actor(d)) => w.actors.actors.get(d).is_none_or(|a| a.dead),
            _ => true,
        };
        if none_alive {
            let (x0, y0) = (l.track[0], l.track[1]);
            spawn_damage(w, id, x0, y0);
        }
    } else {
        l.ex = l.sx + dx;
        l.ey = l.sy + dy;
    }
    if sweep_of(w, id).is_some_and(|l| l.alpha >= 1.0) {
        set_dead(w, id);
        sweep_end(w, id);
    }
}

/// The kind's `spawn`: the state around the class data; the parent's BeginPlay blast-box loop at its place (attached
/// with no transform, the owner's actor location); its CheckBlastBoxTimer there in each step.
fn sweep_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::LaserSweep(LaserSweep::default()), true);
    };
    let p = &a.p;
    // AnimNotifyDuration: the TotalDuration of the Spawner notify that asked for the actor.
    let duration = crate::math::narrow(crate::fighter::anim::notify_data(w, &data, f, r.n).dur);
    let (bx, by) = (w.fighter(f).body.x, w.fighter(f).body.y);
    let mut l = LaserSweep {
        attack_id: r.attack_id,
        start_y: num_in(p, "StartVector", "Y").unwrap_or(0.0),
        start_z: num_in(p, "StartVector", "Z").unwrap_or(0.0),
        end_y: num_in(p, "EndVector", "Y").unwrap_or(0.0),
        end_z: num_in(p, "EndVector", "Z").unwrap_or(0.0),
        length: num(p, "LaserLength").unwrap_or(0.0),
        pass_through: num(p, "PlatformPassThroughDistance").unwrap_or(0.0),
        duration,
        bound: true,
        sx: bx,
        sy: by,
        ex: bx,
        ey: by,
        ny: 1.0,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        ..LaserSweep::default()
    };
    let (mut x, mut y) = (bx, actor_y(w, f));
    let force = flag(p, "ForceSpawnPositionInsideBlastBox");
    let doe = flag(p, "DestroyOnExitedArena?");
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut x, &mut y, None, force, &mut l.blast_bounds, doe);
    (WorldActorState::LaserSweep(l), dead)
}

// ------------------------------------------------------------------ the damage actor

/// Delay(0.15) from the tick that passed the end: Explotion.Deactivate.
fn tail_off() -> Real {
    latent_frames(0.15)
}

/// Then Delay(0.066): K2_DestroyActor.
fn tail_end() -> Real {
    tail_off() + latent_frames(0.066)
}

/// The constructor: the Box's half extents (BoxExtent times HitBoxScale; the Box's own RelativeScale3D (2, 2, 2) never
/// reaches an overlap: the spawn's UpdateHitbox (HitBoxOn? false) runs HitBoxesOff -> Comp_Pawn_HitBoxHandler
/// DeactivateHitbox, SetRelativeScale3D((1, 1, 1)) @17288, while the Box is NoCollision (GetHitBoxes finds it by
/// HitBox or HitBox2D alike); the activation's ActivateHitbox then turns collision on @16817 at that scale and sets
/// HitBoxScale @16855).
fn new_damage(
    w: &World,
    f: FighterId,
    attack_id: i32,
    a: &mvs_data::fighter::ActorDef,
    x: Real,
    y: Real,
) -> LaserDamage {
    let b = obj(&a.components, "Box");
    let s = obj(&a.p, "HitBoxScale");
    let scale = |k: &str| match s {
        Some(s) => num(s, k).unwrap_or(Real::NAN),
        None => 1.0,
    };
    let extent = |k: &str| b.and_then(|b| num_in(b, "BoxExtent", k)).unwrap_or(0.0);
    LaserDamage {
        attack_id,
        x,
        y,
        facing: facing(w, f),
        hit: HitPacket::from_props(&w.names, obj(&a.p, "HitBoxDamage").unwrap_or(&mvs_data::value::Props::new()), None),
        hx: extent("Y") * scale("Y"),
        hy: extent("Z") * scale("Z"),
        hd: extent("X") * scale("X"),
        hit_on: flag(&a.p, "HitBoxOn?"),
        types: hit_box_types(b),
        ..LaserDamage::default()
    }
}

/// ActivateGroundLaserExplosion.
fn damage_activate(w: &mut World, id: ActorId, track: Vec<Real>, anim_time: Real) {
    let Some(d) = damage_mut(w, id) else { return };
    d.activated = true;
    d.anim_time = anim_time;
    if track.len() >= 2 {
        d.x = track[0];
        d.y = track[1];
    }
    d.track = track;
    d.hit_on = true;
    d.hit_set.clear();
    d.once.clear();
    damage_overlaps(w, id); // UpdateHitbox, at the first point
    let Some(a) = w.actors.actors.get(id) else { return };
    let owner = a.owner;
    let Some(d) = damage_mut(w, id) else { return };
    d.exploding = true;
    let (x, y) = (d.x, d.y);
    let fighter = w.fighter(owner).id;
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_superman_laser_hit"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// Round(CurrentTime / (AnimTime / count)): the point the damage actor stands on (count and past: the end).
pub fn laser_track_index(time: Real, anim_time: Real, count: Real) -> Real {
    crate::real(round_to_int(time / (anim_time / count)))
}

/// ReceiveTick.
fn damage_step(w: &mut World, id: ActorId) {
    end_sweeps(w);
    let Some(d) = damage_mut(w, id) else { return };
    if !d.activated {
        return;
    }
    d.time += DELTA_SECONDS;
    let n = d.track.len() / 2;
    #[allow(clippy::cast_precision_loss, reason = "a short list")]
    let q = laser_track_index(d.time, d.anim_time, n as Real);
    #[allow(clippy::cast_precision_loss, reason = "a short list")]
    let past = q >= n as Real;
    let i = if past { n.checked_sub(1) } else { usize::try_from(crate::math::to_int32(q)).ok() };
    if let Some(i) = i
        && let (Some(&px), Some(&py)) = (d.track.get(2 * i), d.track.get(2 * i + 1))
    {
        d.x = px;
        d.y = py;
    }
    d.last_index = if past { 0.0 } else { q };
    if !past {
        return;
    }
    d.tail += 1;
    if crate::real(d.tail) == tail_off() {
        d.exploding = false;
    }
}

/// The Box against the hurt shapes where it stands, as a projectile's hit box: the enemies' hit, a team mate's ally
/// call (`actor_ally_hit`, as `combat::phases` makes it for a projectile), the owner's nothing.
fn damage_overlaps(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, k) = (a.owner, a.size_scale.unwrap_or(1.0)); // the root's scale on the Box
    let Some(d) = damage_of(w, id) else { return };
    if !d.hit_on {
        return;
    }
    let (x, y, hx, hy, hd) = (d.x, d.y, d.hx * k, d.hy * k, d.hd * k);
    let (attack_id, each_once) = (d.attack_id, d.hit.hit_each_actor_only_once.unwrap_or(false));
    let base_kb = d.hit.base_knock_back.unwrap_or(0.0);
    let team = w.fighter(owner).team.team;
    for vic in match_fighters(w) {
        let Some(d) = damage_of(w, id) else { return };
        let vid = w.fighter(vic).id;
        if vic == owner || !alive(w, vic) || no_hit(w, vic) || d.hit_set.contains(&vid) {
            continue;
        }
        let ally = w.fighter(vic).team.team == team;
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = crate::combat::hit::hurt::box_hurt_hit(w, vic, x, y, hx, hy, &vp);
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            if !ally {
                let tick = w.flow.tick;
                attack_dodged(w, me, vic, attack_id, base_kb, tick, None, false);
            }
            continue;
        }
        dodge_over(w, me, vic);
        if each_once && let Some(d) = damage_mut(w, id) {
            d.hit_set.insert(vid);
        }
        let hp = damage_of(w, id).map(|d| d.hit.clone()).unwrap_or_default();
        let rows = class_rows(w, me);
        if ally {
            hit_buffs_call(w, owner, &rows); // the entry rows @842-2821
            let (dl, mult) = (dealer(w, me), charge_buff(w, me));
            actor_ally_hit(w, &dl, vic, &hp, &rows, mult);
            continue;
        }
        let pct = charge_pct(w, me);
        let dl = dealer(w, me);
        if apply_projectile_hit(w, &dl, vic, &hp, &rows, V2 { x, y }, None, &[], pct) {
            actor_hit_enemy(w, &dealer(w, me), vic);
        }
    }
    if w.actors.actors.get(id).is_some_and(|a| !a.dead) {
        damage_actor_overlaps(w, id, Some(HitShape::Box { x, y, hx, hy, depth: hd }));
    }
}

/// Comp_Pawn_HitBoxHandler's overlap pass over the actors that are no fighter (`hit_box_actor_hits`): DestroyProjectiles
/// of the packet takes the projectiles and items the Box begins to overlap.
fn damage_actor_overlaps(w: &mut World, id: ActorId, shape: Option<HitShape>) {
    let Some(d) = damage_mut(w, id) else { return };
    let (types, hit) = (d.types.clone(), d.hit.clone());
    let (mut over, mut once) = (std::mem::take(&mut d.over), std::mem::take(&mut d.once));
    hit_box_actor_hits(w, ActorRef::Actor(id), &hit, &types, shape, &mut over, &mut once);
    if let Some(d) = damage_mut(w, id) {
        d.over = over;
        d.once = once;
    }
}

/// The hits pass: the overlaps, then (the second delay's end, after the frame's ticks and overlaps) the end.
fn damage_hits(w: &mut World, id: ActorId) {
    damage_overlaps(w, id);
    if damage_of(w, id).is_some_and(|d| crate::real(d.tail) >= tail_end()) {
        set_dead(w, id);
    }
}

// ------------------------------------------------------------------ the entries

/// This file's registry entries: the two sweep classes (the air one a child) and the damage actor.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // The trace runs in the world from the eye socket, the eye beams take world transforms: nothing reads a size.
    let sweep = WorldActorHooks {
        spawn: Some(sweep_spawn),
        size: Some(SizeHooks::default()),
        step: Some(sweep_step),
        end: Some(sweep_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        SWEEP,
        ActorEntry::new(ActorClass::World(WorldActorClass::SupermanGroundLazer), ActorHooks::World(sweep)),
    )?;
    r.actors.enter(
        SWEEP_AIR,
        ActorEntry {
            parents: &[SWEEP],
            ..ActorEntry::new(ActorClass::World(WorldActorClass::SupermanGroundLazerAir), ActorHooks::World(sweep))
        },
    )?;
    // The Box (a child of the root) at the root's scale (damage_overlaps); the view's Explotion too.
    let damage = WorldActorHooks {
        size: Some(SizeHooks::default()),
        step: Some(damage_step),
        hits: Some(damage_hits),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        DAMAGE,
        ActorEntry::new(ActorClass::World(WorldActorClass::SupermanGroundLazerDamage), ActorHooks::World(damage)),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_index_rounds_half_up() {
        // 3 points over 0.3 s: one every 0.1 s.
        assert_eq!(laser_track_index(0.0, 0.3, 3.0), 0.0);
        assert_eq!(laser_track_index(0.1, 0.3, 3.0), 1.0);
        assert_eq!(laser_track_index(0.16, 0.3, 3.0), 2.0);
        assert_eq!(laser_track_index(0.3, 0.3, 3.0), 3.0);
    }

    #[test]
    fn impact_normal_picks_the_top_face_on_a_corner() {
        let h = TraceHit {
            time: 0.5,
            distance: 1.0,
            blocking: true,
            initial: false,
            x: 0.0,
            y: 0.0,
            ix: 0.0,
            iy: 0.0,
            nx: 0.7,
            ny: 0.7,
            depth: 0.0,
            object_type: 0,
            profile: crate::names::Name::NONE,
            component: name!("Floor"),
            actor: None,
            platform: -1,
            floor: None,
        };
        // a ray going right and steeply down meets the corner: the top face (the lesser dot)
        assert_eq!(impact_normal(&h, 1.0, -2.0), (0.0, 1.0));
        // a ray going left and flatter: the side face (the lesser dot)
        assert_eq!(impact_normal(&h, -2.0, -1.0), (1.0, 0.0));
    }
}
