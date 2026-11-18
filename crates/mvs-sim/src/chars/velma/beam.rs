//! Velma's heal beam: VelmaHealBeam and VelmaChildBeam ([`Beam`], a dealer world
//! actor: DoCast's sphere trace from her hold point, its hit, the heal and the clue), Buff_VelmaHealOverTimeBlast
//! ([`BlastExt`]: the child beam and the heal pulses), Buff_VelmaHealOverTime and AnimNotify_ResetAnimBPAimDirection;
//! joined to timerTick (the beams' K2_SetTimer timers). It calls evidence.rs (`spawn_evidence_actor`, `range_f`) and
//! pawn.rs (`set_aim_value`).
//!
//! - VelmaHealBeam: Spawner notifies of Velma_HealBeam_Montage / _Air_Montage (AttachToActor at VB HoldPoint). Its
//!   BeginPlay runs at its first step (the world adds the actor behind the kind's spawn).
//!   Deviation: the build runs it inside the spawn (SpawnActor), ahead of the spawn's own follow-up.
//!   Deviation: DestroyOnExitedArena? is not tested (the beam is attached to Velma's socket: it leaves the arena with
//!   her, and Die With Instigator ends it at her ring-out).
//! - VelmaChildBeam: made by Buff_VelmaHealOverTimeBlast, never by a notify.
//! - DoCast(StartPoint) @0-5323: the end StartPoint + M x CurrentDirection x CurrentBeamLength, M = (1, 1, 1) when
//!   IsBeamFacingLeft and not UseBeamFacing, else (1, -1, 1) (@118), so the sim's x step is -(M.Y x Y x L).
//!   SphereTraceMultiForObjects(TraceRadius, [DamageDestructible, WorldStatic, ItemDestructible, Pawn, BlockTrace,
//!   ProjectileDestructible], ignore [GetOwner()], ignore self) @420; each hit in the trace's order, the owner's
//!   skipped @1003; a taken hit replaces the current end when it is strictly nearer the start.
//!   Deviation: a spawned actor's IsInvulnerableToProjectiles and IsInvulnerable? are taken false (the port keeps them
//!   on fighters alone); its Comp_Actor_Destructible is taken valid (SpawnedActorParent's component).
//! - pfgDoDamage(null HitBox, LastHitActor, packet, null, no elementals, rows, false, false) @484: HitLocation the
//!   victim's capsule. A projectile, an item, a mortar aim and a World.actors actor take `hits::do_damage`'s routes
//!   with the beam's rows; BPI_CanProjectileClank @0-69 (CanProjectileClank and ProjectileAttackType 0: the final
//!   blast alone) and BPI_GetProjectileAttackType are the kind's hooks the clank @20004 reads.
//! - The root's scale reaches no shape: DoCast's trace takes TraceRadius (a property, not a component's).

use super::evidence::{EVIDENCE_N, range_f, spawn_evidence_actor};
use super::pawn::set_aim_value;
use super::state::velma_of;
use crate::Real;
use crate::State;
use crate::actors::shield_actor::{on_projectile_blocked, shield_of};
use crate::actors::smoke_bomb::projectile_blocked;
use crate::actors::world_actor::{WorldActorState, actor_owner, actor_p, world_actor_place_of};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::buffs::{apply_healing, buff_mut, inst_view};
use crate::combat::elem::{charge_buff, charge_pct, instigator_charge};
use crate::combat::hits::{
    DamageCall, actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, do_damage as do_damage_on, hit_buffs_ally,
    hit_buffs_call, projectile_blocks,
};
use crate::combat::packet::DataVec;
use crate::combat::spawned_hits::damage_world_actor;
use crate::combat::{BuffClass, BuffExt, BuffRef, HitPacket, OnHitBuffRow};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, NotifyRef, PopKind};
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over, trigger_ability};
use crate::fighter::match_side::{SpawnRequest, post_vo, sfx};
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_BLOCK_TRACE, OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE,
    OBJ_WORLD_STATIC, TraceHit, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable, mesh_wx, mesh_wy, no_hit};
use crate::input::{Dir, get8_way};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, rotate_angle_axis};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorHooks, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{arr, flag, num};
use crate::registry::{ActorEntry, BuffEntry, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use crate::world::spawning::{add_actor, after_spawned_actor, new_world_actor};

pub const HEAL_BEAM: &str = "VelmaHealBeam_C";
pub const CHILD_BEAM: &str = "VelmaChildBeam_C";
pub const HEAL_OVER_TIME_BLAST: &str = "Buff_VelmaHealOverTimeBlast_C";
pub const HEAL_OVER_TIME: &str = "Buff_VelmaHealOverTime_C";

const CHILD_BEAM_N: Name = name!("VelmaChildBeam_C");
/// A timer that is off (`NO_TIMER`).
const NO_TIMER: i32 = -1;
/// The VFXTimer's rate: SetVFXPositioning, 0.02 s, looping (@1039).
const VFX_RATE: Real = 0.02;
const DT: Real = DELTA_SECONDS;
/// The cast's object types: [DamageDestructible, WorldStatic, ItemDestructible, Pawn, BlockTrace,
/// ProjectileDestructible].
fn types() -> [Real; 6] {
    [
        OBJ_DAMAGE_DESTRUCTIBLE,
        OBJ_WORLD_STATIC,
        OBJ_ITEM_DESTRUCTIBLE,
        OBJ_PAWN,
        OBJ_BLOCK_TRACE,
        OBJ_PROJECTILE_DESTRUCTIBLE,
    ]
    .map(crate::real)
}

/// `Beam` (VelmaHealBeam_C, VelmaChildBeam_C): one beam. The actor's owner is the Instigator (Velma: the hits'
/// attribution, Die With Instigator, the beam's own Comp_Actor_Destructible team), `holder` GetOwner (the mesh the
/// beam is attached to: the socket, the trace's ignored actor, IsBeamFacingLeft of VelmaHealBeam, the team DoCast
/// compares with): Velma for VelmaHealBeam, the ally for VelmaChildBeam.
// Unverified: the record holds no `holder` (a Fighter is kept there and the record's row reads numbers alone),
// so a child beam loaded from the trace has none and takes its owner for it; its owner held it a heal beam's.
#[derive(State, Debug)]
pub struct Beam {
    /// GetOwner; `None`: the actor's owner (a heal beam's, or a child loaded from the record).
    pub holder: Option<crate::fighter::FighterId>,
    /// ChargingHit (the class's).
    pub hit: HitPacket,
    /// FinalBlastHit, its knockback X set by the facing at BeginPlay (@6729-6941).
    pub final_hit: HitPacket,
    pub x: Real,
    pub y: Real,
    /// ProjectileAttackType: the class's 2 (Transcendent), 0 from Spawn Final Blast @8582.
    pub attack_type: i32,
    /// VelmaChildBeam's BeamFacingLeft: the Instigator's BPI_IsFacingLeft at its BeginPlay @267-313.
    pub stored_left: bool,
    /// CurrentDirection (Y, Z): X stays 0.
    pub dir_y: Real,
    pub dir_z: Real,
    /// CurrentRotator's pitch (degrees).
    pub pitch: Real,
    /// CurrentBeamLength.
    pub length: Real,
    /// BeamDuration.
    pub duration: Real,
    /// LocalStartTime (ToggleVisuals @3542), in world ticks.
    pub start_tick: i32,
    pub end_x: Real,
    pub end_y: Real,
    pub reach_x: Real,
    pub reach_y: Real,
    /// BeamParticle valid (ToggleVisuals true .. false).
    pub on: bool,
    /// LastHitActor: DoCast's HitActor, written by SpawnProjectile @422 and SetVFXPositioning @399.
    pub last_hit: Option<ActorRef>,
    pub pivot: Real,
    /// BeginPlay run (VelmaHealBeam: at its first step).
    pub begun: bool,
    /// VFXTimer: the world tick each timer was set at (`NO_TIMER`: off) and the pulses a looping one has fired.
    pub vfx_set: i32,
    pub vfx_n: i32,
    /// ProjectileTimer: SpawnChargingDamageHit, DamageFrequency, looping (@4093).
    pub proj_set: i32,
    pub proj_n: i32,
    /// BeamTimer: Spawn Final Blast, BeamDuration, once (@4487).
    pub beam_set: i32,
    /// DestroyAfterDelay, DestroyDelay, once (@102).
    pub die_set: i32,
}

impl Default for Beam {
    /// The field initial values.
    fn default() -> Self {
        Beam {
            holder: None,
            hit: HitPacket::default(),
            final_hit: HitPacket::default(),
            x: 0.0,
            y: 0.0,
            attack_type: 0,
            stored_left: false,
            dir_y: 1.0,
            dir_z: 0.0,
            pitch: 0.0,
            length: 0.0,
            duration: 0.0,
            start_tick: 0,
            end_x: 0.0,
            end_y: 0.0,
            reach_x: 0.0,
            reach_y: 0.0,
            on: false,
            last_hit: None,
            pivot: 0.0,
            begun: false,
            vfx_set: NO_TIMER,
            vfx_n: 0,
            proj_set: NO_TIMER,
            proj_n: 0,
            beam_set: NO_TIMER,
            die_set: NO_TIMER,
        }
    }
}

/// Buff_VelmaHealOverTimeBlast's extension (`BlastExt`).
#[derive(State, Debug, Default)]
pub struct BlastExt {
    /// The Instigator, until the child beam is spawned.
    pub from: Option<FighterId>,
    /// The heal timer's start (the fighter's tick).
    pub set: i32,
    /// Pulses fired.
    pub n: i32,
    /// The LifeTimer's rate (s; 0: endless).
    pub life: Real,
}

fn beam(w: &World, id: ActorId) -> Option<&Beam> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Beam(s) => Some(s),
        _ => None,
    }
}

/// `Beam.lastHit`: the actor the beam last hit (`VelmaBeam_BTT`'s LightBeam.LastHitActor read); `None` for an actor
/// no longer in its arena or one that is no beam.
pub fn last_hit(w: &World, id: ActorId) -> Option<ActorRef> {
    beam(w, id).and_then(|b| b.last_hit)
}

fn beam_mut(w: &mut World, id: ActorId) -> Option<&mut Beam> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Beam(s) => Some(s),
        _ => None,
    }
}

fn dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

fn owner_of(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a live beam").owner
}

fn is_child(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_some_and(|a| a.cls == CHILD_BEAM_N)
}

/// GetOwner (`holder`).
fn holder_of(w: &World, id: ActorId) -> FighterId {
    beam(w, id).and_then(|b| b.holder).unwrap_or_else(|| owner_of(w, id))
}

/// IsBeamFacingLeft: VelmaHealBeam's reads GetOwner's BPI_IsFacingLeft now (@0-145), the child's the field it stored.
fn left(w: &World, id: ActorId) -> bool {
    if is_child(w, id) { beam(w, id).is_some_and(|b| b.stored_left) } else { facing(w, holder_of(w, id)) < 0.0 }
}

fn pnum(w: &World, id: ActorId, k: &str) -> Real {
    num(actor_p(w, ActorRef::Actor(id)), k).unwrap_or(0.0)
}

fn pflag(w: &World, id: ActorId, k: &str) -> bool {
    flag(actor_p(w, ActorRef::Actor(id)), k)
}

/// `chargingRows` / `finalRows`: the class's 'On Hit Charging Buffs Array' / 'On Hit Buffs Array'.
fn rows(w: &World, id: ActorId, k: &str) -> Vec<OnHitBuffRow> {
    OnHitBuffRow::rows(&w.names, actor_p(w, ActorRef::Actor(id)).get(k))
}

/// The beam's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &Beam) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The beam's DealerActor parts (`x`, `y`, `facing`, `attackId`: BeginPlay @6340-6521 writes BPI_GetAttackID of
/// GetOwner into the packets, a fighter for both classes: 0).
pub fn dealer_parts(w: &World, a: &WorldActor, s: &Beam) -> (Real, Real, Real, i32) {
    let left = if a.cls == CHILD_BEAM_N { s.stored_left } else { facing(w, s.holder.unwrap_or(a.owner)) < 0.0 };
    (s.x, s.y, if left { -1.0 } else { 1.0 }, 0)
}

/// The beam's HitBoxDamage packet (`hit`).
pub fn hit(s: &Beam) -> &HitPacket {
    &s.hit
}

/// The beam's own `attackType` (`beam.rs`:130`, 0 again at `beam.rs`:461`).
pub fn attack_type(s: &Beam) -> i32 {
    s.attack_type
}

/// BPI_GetProjectileAttackType of the beam (`WorldActorHooks::attack_type`).
fn beam_attack_type(w: &World, id: ActorId) -> i32 {
    beam(w, id).map_or(0, attack_type)
}

/// BPI_CanProjectileClank @0-69: CanProjectileClank AND ProjectileAttackType == 0 (Light from the CDO, Normal from
/// Spawn Final Blast @8582: the final blast alone clanks).
fn beam_can_clank(w: &World, id: ActorId) -> bool {
    pflag(w, id, "CanProjectileClank") && beam_attack_type(w, id) == 0
}

// ---------------------------------------------------------------- the socket

/// GetSocketLocation(BeamAttachSocket) on GetOwner's mesh, X set to 0 (SpawnProjectile @221-330, SetVFXPositioning's
/// BeamParticle, attached there @3805). The boxes data samples VB HoldPoint (the heal beam's) for Velma and VB Spine
/// (VelmaChildBeam's, a virtual bone of every skeleton) for every fighter.
fn socket_at(w: &World, f: FighterId, name: &str) -> V2 {
    let ch = w.fighter(f).ch;
    let index = crate::chars::iron_giant::actor_state::socket_index(w, f, name);
    let pose = index.and_then(|_| crate::fighter::anim::sampled_boxes(w, f));
    let (Some(i), Some(pose)) = (index, pose) else {
        let msg = format!("VelmaHealBeam: socket {name} not sampled on {} (the actor location taken)", w.names.str(ch));
        warn_text(w, &msg);
        return V2 { x: w.fighter(f).body.x, y: actor_y(w, f) };
    };
    let o = w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(i).expect("a box index")).copied());
    let o = usize::try_from(o.expect("a socket offset")).expect("a box offset");
    let at = V2 { x: mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), y: mesh_wy(w, f) + crate::real(pose[o + 1]) };
    crate::fighter::anim::put_pose(w, pose);
    at
}

/// `place(b)`: the beam at its holder's attach socket.
fn update_place(w: &mut World, id: ActorId) {
    let socket =
        crate::registry::props::text(actor_p(w, ActorRef::Actor(id)), "BeamAttachSocket").unwrap_or("None").to_owned();
    let at = socket_at(w, holder_of(w, id), &socket);
    if let Some(b) = beam_mut(w, id) {
        b.x = at.x;
        b.y = at.y;
    }
}

// ---------------------------------------------------------------- DoCast

/// The cast's outputs (EndPoint, HitActor): `valid` is IsValid(HitActor), true for the level's block too, whose
/// actor the traces leave `None`.
struct Cast {
    x: Real,
    y: Real,
    actor: Option<ActorRef>,
    valid: bool,
}

/// Vector_Distance in the play plane (float).
fn dist(dx: Real, dy: Real) -> Real {
    ((dx * dx) + (dy * dy)).sqrt()
}

/// `teamOf`: the actor's own team (a fighter's), else its owner's, -1 for none.
fn team_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Fighter(f) => w.fighter(f).team.team,
        _ => actor_owner(w, a).map_or(-1, |o| w.fighter(o).team.team),
    }
}

/// `spawnedType`: BPI_GetProjectileAttackType of a hit actor with BPI_SpawnedActor (a projectile, an item, an actor of
/// World.actors and the other spawned actors the traces enter), `None` for one without the interface.
fn spawned_type(w: &World, a: ActorRef) -> Option<i32> {
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| p.attack_type),
        ActorRef::Item(id) => Some(crate::actors::item::item_attack_type(w, id)),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|m| m.attack_type),
        ActorRef::Actor(id) => {
            let own = crate::actors::world_actor::instance_attack_type(w, id);
            Some(
                own.unwrap_or_else(|| crate::math::to_int32(num(actor_p(w, a), "ProjectileAttackType").unwrap_or(0.0))),
            )
        }
        _ => {
            actor_owner(w, a).map(|_| crate::math::to_int32(num(actor_p(w, a), "ProjectileAttackType").unwrap_or(0.0)))
        }
    }
}

#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(d < best)` is true for a NaN")]
fn do_cast(w: &mut World, id: ActorId, sx: Real, sy: Real) -> Cast {
    let me = ActorRef::Actor(id);
    let holder = holder_of(w, id);
    let inst = owner_of(w, id);
    let k = if left(w, id) && !pflag(w, id, "UseBeamFacing") { -1.0 } else { 1.0 };
    let (dir_y, dir_z, length) = beam(w, id).map_or((1.0, 0.0, 0.0), |b| (b.dir_y, b.dir_z, b.length));
    let tx = sx + k * (dir_y * length);
    let ty = sy + (dir_z * length);
    let (mut ex, mut ey, mut best) = (tx, ty, dist(tx - sx, ty - sy));
    let (mut actor, mut valid) = (None, false);
    let mut hits: Vec<TraceHit> = Vec::new();
    let radius = pnum(w, id, "TraceRadius");
    sphere_trace_multi_for_objects(
        w,
        sx,
        sy,
        tx,
        ty,
        radius,
        &types(),
        &[Some(ActorRef::Fighter(holder))],
        Some(me),
        &mut hits,
    );
    let ignore: Vec<Real> = arr(actor_p(w, me), "IgnoreProjectileTypes")
        .iter()
        .filter_map(mvs_data::value::Value::as_f64)
        .map(crate::math::narrow)
        .collect();
    let holder_team = w.fighter(holder).team.team;
    for h in &hits {
        let a = h.actor;
        if a == Some(ActorRef::Fighter(holder)) {
            continue;
        }
        let take;
        if h.object_type == OBJ_BLOCK_TRACE {
            take = a.is_some_and(|a| team_of(w, a) == w.fighter(inst).team.team) || !h.initial;
        } else if let Some(ActorRef::Fighter(v)) = a {
            // BPI_SpawnedActor's cast fails: type 0, never in the list ([2, 4] on both classes).
            if ignore.contains(&0.0) {
                continue;
            }
            let inv = invulnerable(w, v) || projectile_blocked(w, v);
            let team = w.fighter(v).team.team;
            take = (!inv && team != holder_team) || team == holder_team;
        } else {
            match a.and_then(|a| spawned_type(w, a).map(|t| (a, t))) {
                Some((a, t)) => {
                    if ignore.contains(&crate::real(t)) {
                        continue;
                    }
                    take = team_of(w, a) != holder_team;
                }
                None => take = h.object_type == OBJ_WORLD_STATIC,
            }
        }
        if !take {
            continue;
        }
        let (cx, cy) = if h.blocking { (h.x, h.y) } else { (tx, ty) };
        let d = dist(cx - sx, cy - sy);
        if !(d < best) {
            continue;
        }
        best = d;
        ex = cx;
        ey = cy;
        actor = a;
        valid = true;
    }
    // WW_ShieldActor OnProjectileBlocked @523
    if let Some(ActorRef::Actor(s)) = actor
        && w.actors.actors.get(s).is_some_and(|x| shield_of(x).is_some())
    {
        on_projectile_blocked(w, s, ex, ey);
    }
    Cast { x: ex, y: ey, actor, valid }
}

// ---------------------------------------------------------------- pfgDoDamage

/// Comp_Pawn_HitBoxHandler.pfgDoDamage(null HitBox, LastHitActor, packet, null, no elementals, rows, false, false)
/// @484 (`doDamage`): a fighter of another team: the dodge, the hit (IgnoreFacing: the packet's X is UE's Y, sim -x);
/// an ally: the rows of a hit on an ally and HitByAlly (no HitSuccessful); the Instigator itself: the rows that need
/// no hit. Any other: `hits::do_damage`'s route with the beam's rows (OnHitBuffsParam); a World.actors actor at its
/// own place (LocalHitLocation of a call with no HitBox).
fn do_damage(w: &mut World, id: ActorId, a: ActorRef, packet: &HitPacket, rows: &[OnHitBuffRow]) -> bool {
    let me = ActorRef::Actor(id);
    let inst = owner_of(w, id);
    match a {
        ActorRef::Fighter(vic) => {
            if w.fighter(vic).team.team != w.fighter(inst).team.team {
                if no_hit(w, vic) || projectile_blocks(w, &dealer(w, me), vic) {
                    return false;
                }
                if invulnerable(w, vic) {
                    let tick = w.flow.tick;
                    attack_dodged(w, me, vic, 0, packet.base_knock_back.unwrap_or(0.0), tick, None, false);
                    return false;
                }
                dodge_over(w, me, vic);
                let kb = packet.knock_back_direction_vector.unwrap_or_default();
                let dir = if packet.ignore_facing.unwrap_or(false) {
                    Some(V2 { x: -kb.x.unwrap_or(0.0), y: kb.y.unwrap_or(0.0) })
                } else {
                    None
                };
                let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                let d = dealer(w, me);
                if !apply_projectile_hit(w, &d, vic, packet, rows, at, dir, &[], charge_pct(w, me)) {
                    return false;
                }
                actor_hit_enemy(w, &d, vic);
                return true;
            }
            hit_buffs_call(w, inst, rows);
            if vic != inst && !no_hit(w, vic) && !invulnerable(w, vic) {
                hit_buffs_ally(w, inst, vic, rows, charge_buff(w, me));
                ally_hit(w, &dealer(w, me), vic, packet);
            }
            false
        }
        // OnHitBuffsParam: the beam's own rows (charging or final); the clank @20004 of the final blast (type Normal)
        // on a clanking projectile or a thrown item of another team (`spawned_hits::clank`).
        // Deviation: pfgDoDamage's routes with the call's rows, not the class rows.
        ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Mortar(_) => {
            do_damage_on(w, me, a, packet, &DamageCall { hit_box: false, comp: None, clank: false, rows: Some(rows) })
        }
        ActorRef::Actor(o) => {
            let (bx, by) = beam(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
            let (x, y) = world_actor_place_of(w, o).map_or((bx, by), |(x, y, _)| (x, y));
            let call = DamageCall { hit_box: false, comp: None, clank: false, rows: Some(rows) };
            damage_world_actor(w, me, o, packet, Some(V2 { x, y }), &call)
        }
        _ => {
            let cls = crate::actors::world_actor::actor_cls(w, a)
                .map_or_else(|| "an actor".to_owned(), |c| w.names.str(c).to_owned());
            let msg = format!(
                "{}: pfgDoDamage on {cls} not ported",
                w.names.str(w.actors.actors.get(id).expect("a beam").cls)
            );
            warn_text(w, &msg);
            false
        }
    }
}

// ---------------------------------------------------------------- SpawnProjectile

/// SpawnProjectile(SpawnEvidenceOnHit, HitBoxDamageParam, OnHitBuffs) @0-795 (`spawnProjectile`): DoCast from the
/// socket; a valid actor gets pfgDoDamage, then HitEffect(the end, HitSuccessful, the actor) @588; with
/// SpawnEvidenceOnHit and a hit on a BP_FighterCharacter, SpawnEvidence @760.
fn spawn_projectile(w: &mut World, id: ActorId, evidence: bool, packet: &HitPacket, rows: &[OnHitBuffRow]) {
    update_place(w, id);
    let (bx, by) = beam(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
    let c = do_cast(w, id, bx, by);
    if let Some(b) = beam_mut(w, id) {
        b.last_hit = c.actor; // @422
    }
    if !c.valid {
        return;
    }
    let ok = c.actor.is_some_and(|a| do_damage(w, id, a, packet, rows));
    hit_effect(w, id, c.x, c.y, ok, c.actor);
    if evidence
        && ok
        && let Some(ActorRef::Fighter(v)) = c.actor
        && !w.fighter(v).life.not_player_character
    {
        spawn_evidence(w, id, v);
    }
}

/// HitEffect multicast @7794: PS_HitImpact_Directional at (-100, the end) with rot(0, 180, 0) when IsBeamFacingLeft,
/// else rot(0, 0, 0); without HitSuccessful, with PlayAllyHitSound, on a BPI_BaseFighter of the beam's team (its
/// Comp_Actor_Destructible valid): sfx_cha_velma_healbeam_ally_hit at the end.
fn hit_effect(w: &mut World, id: ActorId, x: Real, y: Real, ok: bool, a: Option<ActorRef>) {
    let cls = w.actors.actors.get(id).expect("a beam").cls;
    let inst = owner_of(w, id);
    let fighter = w.fighter(inst).id;
    let rot_y = if left(w, id) { 180.0 } else { 0.0 };
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_HitImpact_Directional")),
        x,
        y,
        ox: -100.0,
        rot: [0.0, rot_y, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if ok || !pflag(w, id, "PlayAllyHitSound") {
        return;
    }
    let Some(ActorRef::Fighter(v)) = a else { return };
    if w.fighter(v).team.team != w.fighter(inst).team.team {
        return;
    }
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_velma_healbeam_ally_hit"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// SpawnEvidence @9197: RandomBoolWithWeight(EvidenceSpawnChance), then RandomFloatInRange(-3000, 3000) (the speed),
/// RandomFloatInRange(-1, 1) (Y), RandomFloatInRange(-1, 1) (Z) in that order (@9236, @9274, @9377), the hit
/// fighter's location.
// Unverified: FRand from the world's stream (`random_int`), as bugs/safe.
fn spawn_evidence(w: &mut World, id: ActorId, vic: FighterId) {
    let chance = pnum(w, id, "EvidenceSpawnChance");
    if !(chance > 0.0 && w.random_int(16_777_216.0) / 16_777_216.0 <= chance) {
        return;
    }
    let speed = range_f(w, -3000.0, 3000.0);
    let dy = range_f(w, -1.0, 1.0);
    let dz = range_f(w, -1.0, 1.0);
    let inst = owner_of(w, id);
    let (x, y) = (w.fighter(vic).body.x, actor_y(w, vic));
    spawn_evidence_actor(w, inst, EVIDENCE_N, x, y, 0.0, dy, speed, dz); // @9696, direction (0, Y, Z)
}

// ---------------------------------------------------------------- the multicast and the aim

/// SetDirectionAndRotatorMulticast(CurrentDirection, CurrentRotator with pitch + 90) @2751 / @6308 -> @4622: the
/// direction, CurrentRotator = its pitch - 90; with SetInstigatorAnim and a BPI_Velma Instigator,
/// BPI_SetAimValue(that pitch) @5038. The rest (@5084-6092) places the scrolling words widget (the HUD).
fn multicast(w: &mut World, id: ActorId) {
    let Some(b) = beam_mut(w, id) else { return };
    b.pitch = (b.pitch + 90.0) - 90.0;
    let pitch = b.pitch;
    let inst = owner_of(w, id);
    if pflag(w, id, "SetInstigatorAnim") && velma_of(w, inst).is_some() {
        set_aim_value(w, inst, pitch);
    }
}

/// The Normal node (FVector::GetSafeNormal(1e-4)) on (0, y, z).
fn safe_normal(y: Real, z: Real) -> (Real, Real) {
    let [_, ny, nz] = normal([0.0, y, z], KINDA_SMALL_NUMBER);
    (ny, nz)
}

/// GetDirection(VectorAroundAxis (ax, 0, 0), SelectVector (0, 0, fz)) @0-463: RotateAngleAxis(CurrentDirection, DT x
/// AimRotateDegreesPerSecond, the axis) (about X: Y' = C.Y - S.ax.Z, Z' = S.ax.Y + C.Z), Normal(1e-4); the fallback
/// when its Y is past 0 (UseBeamFacing and IsBeamFacingLeft: Y > 0, else Y < 0). Then (Tick @2222-2581) Z clamped to
/// [AimLowestZValue, AimHighestZValue], Normal(1e-4) is CurrentDirection, the pitch Lerp(-45, 45, (Z + 1) / 2).
fn aim(w: &mut World, id: ActorId, ax: Real, fz: Real) {
    let deg = DT * pnum(w, id, "AimRotateDegreesPerSecond");
    let p = actor_p(w, ActorRef::Actor(id));
    let low = num(p, "AimLowestZValue").unwrap_or(-1.0);
    let high = num(p, "AimHighestZValue").unwrap_or(1.0);
    let flip = pflag(w, id, "UseBeamFacing") && left(w, id);
    let Some(b) = beam_mut(w, id) else { return };
    let [_, ry, rz] = rotate_angle_axis([0.0, b.dir_y, b.dir_z], deg, [ax, 0.0, 0.0]);
    let (mut ny, mut nz) = safe_normal(ry, rz);
    let pick = if flip { ny > 0.0 } else { ny < 0.0 };
    if pick {
        ny = 0.0;
        nz = fz;
    }
    let z = min(max(nz, low), high);
    (b.dir_y, b.dir_z) = safe_normal(ny, z);
    b.pitch = -45.0 + (((b.dir_z + 1.0) / 2.0) * 90.0);
}

// ---------------------------------------------------------------- BeginPlay, Tick

/// VelmaHealBeam ReceiveBeginPlay @9049 -> @8567 -> @6719 (VelmaChildBeam's @10-332 first stores BeamFacingLeft and
/// calls it): FinalBlastHit's knockback X = IsBeamFacingLeft ? X : -X @6729-6941; Start Light Beam @8893 (authority):
/// CurrentDirection (0, -1, 0) with UseBeamFacing and IsBeamFacingLeft, else (0, 1, 0); CurrentRotator rot(0, 0, 0)
/// and the multicast @6134-6308; SpawnProjectile(false, ChargingHit, On Hit Charging Buffs Array) @6557; then
/// ProjectileTimer @4006, BeamTimer (BeamDuration = FullDuration - 0.1) @4331, ToggleVisuals(true, BeamDuration)
/// @6666.
fn begin_play(w: &mut World, id: ActorId) {
    update_place(w, id);
    let left = left(w, id);
    let use_facing = pflag(w, id, "UseBeamFacing");
    let full = pnum(w, id, "FullDuration");
    let tick = w.flow.tick;
    let Some(b) = beam_mut(w, id) else { return };
    b.begun = true;
    let kb = b.final_hit.knock_back_direction_vector.unwrap_or_default();
    b.final_hit.knock_back_direction_vector = Some(DataVec {
        x: Some(if left { kb.x.unwrap_or(0.0) } else { -kb.x.unwrap_or(0.0) }),
        y: Some(kb.y.unwrap_or(0.0)),
        z: None,
    });
    b.dir_y = if use_facing && left { -1.0 } else { 1.0 };
    b.dir_z = 0.0;
    b.pitch = 0.0;
    let packet = b.hit.clone();
    multicast(w, id);
    let charging = rows(w, id, "On Hit Charging Buffs Array");
    spawn_projectile(w, id, false, &packet, &charging);
    if dead(w, id) {
        return;
    }
    if let Some(b) = beam_mut(w, id) {
        b.proj_set = tick;
        b.proj_n = 0;
        b.duration = full - 0.1;
        b.beam_set = tick;
    }
    toggle_on(w, id);
}

/// ToggleVisuals(true, Duration) @3412 (not on a dedicated server: the port runs it): BeamDuration, LocalStartTime,
/// the emitter at the socket (the view), CurrentBeamLength = 0 @952, VFXTimer @1039, the skin colour (the view), then
/// SetVFXPositioning() @937.
fn toggle_on(w: &mut World, id: ActorId) {
    let tick = w.flow.tick;
    if let Some(b) = beam_mut(w, id) {
        b.start_tick = tick;
        b.on = true;
        b.length = 0.0;
        b.vfx_set = tick;
        b.vfx_n = 0;
    }
    set_vfx_positioning(w, id);
}

/// ToggleVisuals(false) @3937: VFXTimer cleared, the emitter destroyed @160-199.
fn toggle_off(w: &mut World, id: ActorId) {
    if let Some(b) = beam_mut(w, id) {
        b.on = false;
        b.vfx_set = NO_TIMER;
    }
}

/// SetVFXPositioning @0-1058 (the emitter valid): DoCast from the emitter's location (X 0) -> LastHitActor,
/// EndPoint; LocalCurrentEnd = EndPoint + Normal(EndPoint - start) x BeamVFXBuffer (end points 0 and 1); end point 2
/// = start + (that - start) x clamp((now - LocalStartTime) / BeamDuration, 0, 1).
fn set_vfx_positioning(w: &mut World, id: ActorId) {
    if !beam(w, id).is_some_and(|b| b.on) {
        return;
    }
    update_place(w, id);
    let (sx, sy) = beam(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
    let c = do_cast(w, id, sx, sy);
    let buf = pnum(w, id, "BeamVFXBuffer");
    let tick = w.flow.tick;
    let Some(b) = beam_mut(w, id) else { return };
    b.last_hit = c.actor; // @399
    let (ny, nz) = safe_normal(c.x - sx, c.y - sy);
    b.end_x = c.x + ny * buf;
    b.end_y = c.y + nz * buf;
    let t =
        if b.duration > 0.0 { min(max((crate::real(tick - b.start_tick) * DT) / b.duration, 0.0), 1.0) } else { 1.0 };
    b.reach_x = sx + (b.end_x - sx) * t;
    b.reach_y = sy + (b.end_y - sy) * t;
}

/// ReceiveTick @9054 -> @1135 (`beamStep`): (authority) the Instigator's BPI_Get8WayDirection(true): Up, Up Forward,
/// Up Back turn the beam up (@1985: axis (-1, 0, 0) and fallback (0, 0, -1) with UseBeamFacing and IsBeamFacingLeft,
/// else (1, 0, 0) and (0, 0, 1)), Down, Down Forward, Down Back down (@2784: the axes and fallbacks swapped), then
/// the multicast; Forward, Back, Neutral, Default the multicast alone. Always @1794: CurrentBeamLength =
/// min(DT x BeamVFXGrowthValue + it, BeamVFXMaxLength).
// Unverified: the stick is the Instigator's input of this frame, as chars/garnet/gauntlets reads it; DT the world's
// (the beam's own time dilation, the hit pause of ApplyHitPauseToDamageDealer, is not ported).
fn beam_step(w: &mut World, id: ActorId) {
    if !beam(w, id).is_some_and(|b| b.begun) {
        begin_play(w, id);
        if dead(w, id) {
            return;
        }
    }
    update_place(w, id);
    let inst = owner_of(w, id);
    let (ix, iy) = (w.fighter(inst).controls.inp.x, w.fighter(inst).controls.inp.y);
    let d = get8_way(ix, iy, facing(w, inst));
    let flip = pflag(w, id, "UseBeamFacing") && left(w, id);
    match d {
        Dir::Up | Dir::FwdUp | Dir::BackUp => {
            let v = if flip { -1.0 } else { 1.0 };
            aim(w, id, v, v);
        }
        Dir::Down | Dir::FwdDown | Dir::BackDown => {
            let v = if flip { 1.0 } else { -1.0 };
            aim(w, id, v, v);
        }
        _ => {}
    }
    multicast(w, id);
    let (growth, max_len) = (pnum(w, id, "BeamVFXGrowthValue"), pnum(w, id, "BeamVFXMaxLength"));
    if let Some(b) = beam_mut(w, id) {
        b.length = min((DT * growth) + b.length, max_len);
    }
}

// ---------------------------------------------------------------- the timers

/// Spawn Final Blast @8582 (BeamTimer): ProjectileAttackType = 0; SpawnProjectile(true, FinalBlastHit, On Hit Buffs
/// Array); with TriggerBeamCooldown, GetOwner's AbilityManager.TriggerAbilityServer(TriggerAbilityIndex) @8834; then
/// with ProjectileTimer active @4572 -> @4178: ProjectileTimer, FinalBlastEndDelayTimer and BeamTimer cleared,
/// ToggleVisuals(false) @15, and with DestroyDelay > 0 the DestroyAfterDelay timer @102 (K2_DestroyActor @9872).
/// VelmaHealBeam (DestroyDelay 0) lives on until its Spawner notify's end (DestroyAllActorsOnNotifyEnd).
fn final_blast(w: &mut World, id: ActorId) {
    let Some(b) = beam_mut(w, id) else { return };
    b.attack_type = 0;
    let packet = b.final_hit.clone();
    let final_rows = rows(w, id, "On Hit Buffs Array");
    spawn_projectile(w, id, true, &packet, &final_rows);
    if dead(w, id) {
        return;
    }
    if pflag(w, id, "TriggerBeamCooldown") {
        let slot = crate::math::to_int32(pnum(w, id, "TriggerAbilityIndex"));
        trigger_ability(w, holder_of(w, id), slot);
    }
    let delay = pnum(w, id, "DestroyDelay");
    let tick = w.flow.tick;
    let Some(b) = beam_mut(w, id) else { return };
    if b.proj_set == NO_TIMER {
        return;
    }
    b.proj_set = NO_TIMER;
    b.beam_set = NO_TIMER;
    toggle_off(w, id);
    if delay > 0.0
        && let Some(b) = beam_mut(w, id)
    {
        b.die_set = tick;
    }
}

/// A timer set at tick `set` with Rate r fires its n-th pulse at the first tick with (tick - set) x DT > n x r (the
/// timer manager's strict test); the timers of one tick go by expire time.
// Unverified: a tie is taken in the order vfx, projectile, beam, destroy.
fn expire(set: i32, n: i32, rate: Real) -> Real {
    crate::real(set) * DT + crate::real(n) * rate
}

fn is_due(tick: i32, set: i32, n: i32, rate: Real) -> bool {
    set != NO_TIMER && rate > 0.0 && crate::real(tick - set) * DT > crate::real(n) * rate
}

fn beam_timers(w: &mut World, id: ActorId) {
    loop {
        if dead(w, id) {
            return;
        }
        let tick = w.flow.tick;
        let (proj, die) = (pnum(w, id, "DamageFrequency"), pnum(w, id, "DestroyDelay"));
        let Some(b) = beam(w, id) else { return };
        let (mut which, mut at) = (0, Real::INFINITY);
        if is_due(tick, b.vfx_set, b.vfx_n + 1, VFX_RATE) {
            which = 1;
            at = expire(b.vfx_set, b.vfx_n + 1, VFX_RATE);
        }
        if is_due(tick, b.proj_set, b.proj_n + 1, proj) && expire(b.proj_set, b.proj_n + 1, proj) < at {
            which = 2;
            at = expire(b.proj_set, b.proj_n + 1, proj);
        }
        if is_due(tick, b.beam_set, 1, b.duration) && expire(b.beam_set, 1, b.duration) < at {
            which = 3;
            at = expire(b.beam_set, 1, b.duration);
        }
        if is_due(tick, b.die_set, 1, die) && expire(b.die_set, 1, die) < at {
            which = 4;
        }
        match which {
            0 => return,
            1 => {
                if let Some(b) = beam_mut(w, id) {
                    b.vfx_n += 1;
                }
                set_vfx_positioning(w, id);
            }
            2 => {
                let packet = beam_mut(w, id).map(|b| {
                    b.proj_n += 1;
                    b.hit.clone()
                });
                let charging = rows(w, id, "On Hit Charging Buffs Array");
                if let Some(p) = packet {
                    spawn_projectile(w, id, false, &p, &charging);
                }
            }
            3 => {
                if let Some(b) = beam_mut(w, id) {
                    b.beam_set = NO_TIMER;
                }
                final_blast(w, id);
            }
            _ => {
                if let Some(b) = beam_mut(w, id) {
                    b.die_set = NO_TIMER;
                }
                if let Some(a) = w.actors.actors.get_mut(id) {
                    a.dead = true;
                }
            }
        }
    }
}

/// beam.rs's part of the joined timerTick (after evidence.rs's and police.rs's): the beams whose Instigator is `f` (a
/// VelmaChildBeam on an ally included), in list order (an actor added during the pass waits for the next frame: its
/// timers were set in this one).
pub fn timer_tick(w: &mut World, f: FighterId) {
    let mut ids = Vec::new();
    let mut i = 0;
    while let Some(id) = w.actors.actors.nth(i) {
        i += 1;
        ids.push(id);
    }
    for id in ids {
        let Some(a) = w.actors.actors.get(id) else { continue };
        if a.owner != f || a.dead || !matches!(a.state, WorldActorState::Beam(_)) {
            continue;
        }
        if beam(w, id).is_some_and(|b| b.begun) {
            beam_timers(w, id);
        }
    }
}

// ---------------------------------------------------------------- the kinds

/// The kind's `spawn`: the beam of the class's data (the place and BeginPlay come at its first step).
fn heal_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    (WorldActorState::Beam(new_beam(w, f, f, &a.p)), false)
}

/// `new Beam(cls, owner, holder, a)`.
fn new_beam(w: &World, owner: FighterId, holder: FighterId, p: &mvs_data::value::Props) -> Beam {
    let packet = |k: &str| {
        HitPacket::from_props(
            &w.names,
            crate::registry::props::obj(p, k).unwrap_or(&mvs_data::value::Props::new()),
            None,
        )
    };
    Beam {
        holder: (holder != owner).then_some(holder),
        hit: packet("ChargingHit"),
        final_hit: packet("FinalBlastHit"),
        attack_type: crate::math::to_int32(num(p, "ProjectileAttackType").unwrap_or(0.0)),
        ..Beam::default()
    }
}

// ---------------------------------------------------------------- Buff_VelmaHealOverTimeBlast

/// The child beam of the ally `ally`, spawned by `inst` (`spawnChild`).
/// VelmaChildBeam BeginPlay @10-332: IncrementCoopActions (stats, not ported), BeamFacingLeft = the Instigator's
/// facing, then the parent's.
// Deviation: the actor is not put in the ally's Comp_Spawner list (an ally Velma's own HealBeam notify end would
// take it).
fn spawn_child(w: &mut World, ally: FighterId, inst: FighterId) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(inst), CHILD_BEAM_N) else {
        warn_text(w, "Buff_VelmaHealOverTimeBlast: no VelmaChildBeam_C in the Instigator's data");
        return;
    };
    let mut s = new_beam(w, inst, ally, &a.p);
    s.stored_left = facing(w, inst) < 0.0;
    // (the holder is the ally even when it is the Instigator: two fields)
    s.holder = Some(ally);
    let actor =
        new_world_actor(w, CHILD_BEAM_N, WorldActorClass::VelmaChildBeam, inst, false, WorldActorState::Beam(s));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, inst);
    add_actor(w, id, charge, false);
    begin_play(w, id);
    // The ally's Comp_Spawner.CompSpawnActor (Buff_VelmaHealOverTimeBlast @281): HandleActorPostSpawn's
    // AfterSpawnedActor @12116 behind the BeginPlay.
    after_spawned_actor(w, ally, ActorRef::Actor(id));
}

fn blast_ext_mut(w: &mut World, b: BuffRef) -> Option<&mut BlastExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::HealOverTimeBlast(e) => Some(e),
        _ => None,
    }
}

/// ReceiveBeginPlay @15 (the parent's first): (authority, a BPI_BaseFighter owner) the owner's Comp_Spawner
/// CompSpawnActor(VelmaChildBeam_C, attached, parent the owner, socket None, identity, ..., attack ID 0, the buff's
/// Instigator) @281, then the looping HealingTickTime timer @438 -> OnHealingTick @2604: the owner's DamageManager
/// valid -> ApplyHealing(HealingOnInterval) @2425. BPI After Status Update Multicast @606 (applied or refreshed, a
/// BPI_BaseFighter owner): PopText "HEALING" @1442, PS_Heal at the mesh Root (the view),
/// sfx_cha_velma_speaker_heal_start and sfx_cha_arya_pie_heal on the Instigator; removed:
/// sfx_cha_velma_speaker_end at the owner's Root @2219.
// Deviation: the child beam is spawned in the buff's timer pass of the same frame (`class_timers`, after the pawns'
// timerTick where the final blast ran); the build spawns it inside the hit, ahead of HitEffect and the rest of the
// pass. The applied visuals are posted from the begin (the status call of the spawn comes ahead of it, without the
// Instigator); the class's CanOverride makes every reapply a new actor, so no refresh reaches it.
fn blast_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, d: Option<Real>) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::Healing,
        stacks: 0.0,
        gain: None,
        params: None,
    });
    match from {
        Some(from) => {
            sfx(w, from, Some(name!("sfx_cha_velma_speaker_heal_start")));
            sfx(w, from, Some(name!("sfx_cha_arya_pie_heal")));
        }
        None => warn_text(w, "Buff_VelmaHealOverTimeBlast without an Instigator: no child beam"),
    }
    let e = BlastExt { from, set: w.fighter(f).clock.tick, n: 0, life: d.unwrap_or(0.0) };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::HealOverTimeBlast(e);
    }
}

/// The buff's timers: the child beam, then the heal pulses.
// Unverified: the fourth pulse (4 x 0.25) and the 1 s LifeTimer expire together; the LifeTimer is taken first, so the
// buff heals 3 times. Both are set in one BeginPlay, the LifeTimer first (the parent's SetLifeTimer @1289 ahead of
// the child's HealingTickTimer @438), so both enter the timer heap in that order; the 4.26 heap's sift-up moves an
// element only past a strictly later parent, so with only the two in the heap the LifeTimer stays on top when the
// pulse is pushed back with the same ExpireTime, and its end (EndPlay clears the actor's timers) drops the pulse. A
// heap holding other timers can lay out two equal keys either way (4.26 source, not read in the exe).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(x > 0)` is true for a NaN")]
fn blast_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = blast_ext_mut(w, b) else { return false };
    if let Some(inst) = e.from.take() {
        spawn_child(w, f, inst);
    }
    let d = inst_view(w, b);
    let rate = d.num("HealingTickTime").unwrap_or(0.0);
    let on = d.num("HealingOnInterval").unwrap_or(0.0);
    if !(rate > 0.0) {
        return false;
    }
    loop {
        let tick = w.fighter(f).clock.tick;
        let Some(e) = blast_ext_mut(w, b) else { return false };
        if !(crate::real(tick - e.set) * DT > crate::real(e.n + 1) * rate) {
            break;
        }
        if e.life > 0.0 && crate::real(e.n + 1) * rate >= e.life {
            break; // the LifeTimer first (the Unverified above)
        }
        e.n += 1;
        apply_healing(w, f, on);
    }
    false
}

fn blast_status(w: &mut World, f: FighterId, _c: BuffClass, _stacks: Real, _applied: bool, removed: bool) {
    if removed {
        sfx(w, f, Some(name!("sfx_cha_velma_speaker_end")));
    }
}

/// Buff_VelmaHealOverTime ReceiveBeginPlay @2401 -> @2250 (authority): FireVFXOn (PopText "HEALING" for a
/// BPI_BaseFighter owner, PS_Heal_Loop at VFX_Root (the view), sfx_cha_velma_speaker_heal_start @1750),
/// InitialVisuals @2406 (the Instigator and the owner BP_FighterCharacter: PS_Velma_MegaphoneAlly at VB Spine (the
/// view), PF_Post_VO_Event(the Instigator, ability_heal, probability 0.05) @3231); then @2090 -> @1816: a
/// BP_FighterCharacter owner (else nothing more: no heal, the buff lives its duration) -> Delay(MaxLifetime) ->
/// K2_DestroyActor, and at once the DamageManager valid -> ApplyHealing(HealingOnInterval),
/// PF_Post_Event(SFX_Damage: None) and the Delay(HealingTickTime) loop @30-297: Buff_HealOverTime's loop
/// (`combat::buffs::heal_begin`).
fn hot_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter: id,
        kind: PopKind::Healing,
        stacks: 0.0,
        gain: None,
        params: None,
    });
    sfx(w, f, Some(name!("sfx_cha_velma_speaker_heal_start")));
    if let Some(from) = from
        && !w.fighter(from).life.not_player_character
        && !w.fighter(f).life.not_player_character
    {
        post_vo(w, from, name!("ability_heal"), None, 0.05);
    }
    if w.fighter(f).life.not_player_character {
        return;
    }
    let tick = crate::real(w.fighter(f).clock.tick);
    let d = inst_view(w, b);
    let (on, every, life) =
        (d.num("HealingOnInterval").unwrap_or(0.0), d.num("HealingTickTime").unwrap_or(0.0), d.num("MaxLifetime"));
    if let Some(x) = buff_mut(w, b) {
        x.born = Some(tick);
    }
    apply_healing(w, f, on);
    let Some(x) = buff_mut(w, b) else { return };
    x.heal = Some(max(latent_frames(every) - 1.0, 1.0));
    if let Some(l) = life
        && l > 0.0
        && x.frames > latent_frames(l)
    {
        x.frames = latent_frames(l);
    }
}

/// AnimNotify_ResetAnimBPAimDirection Received_Notify: the owner's AnimInstance cast to BPI_AnimBP ->
/// BPI_SetAimDirection(0) (Velma_AnimBP @9247: AimDirection). Another pawn's AnimBP (a Disguise's copy): Velma's
/// variable alone is ported, nothing.
fn reset_aim(w: &mut World, f: FighterId, _n: NotifyRef) {
    if let Some(v) = super::state::velma_mut(w, f) {
        v.aim_direction = 0.0;
    }
}

/// This file's registry entries: VelmaHealBeam_C, VelmaChildBeam_C (no spawn: Buff_VelmaHealOverTimeBlast makes it),
/// Buff_VelmaHealOverTimeBlast_C, Buff_VelmaHealOverTime_C, AnimNotify_ResetAnimBPAimDirection_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        HEAL_BEAM,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::VelmaHealBeam),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks::default()),
                spawn: Some(heal_spawn),
                step: Some(beam_step),
                attack_type: Some(beam_attack_type),
                can_clank: Some(beam_can_clank),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        CHILD_BEAM,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::VelmaChildBeam),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks::default()),
                step: Some(beam_step),
                attack_type: Some(beam_attack_type),
                can_clank: Some(beam_can_clank),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.buffs.enter(
        HEAL_OVER_TIME_BLAST,
        BuffEntry::new(
            BuffClass::BuffVelmaHealOverTimeBlast,
            BuffHooks {
                begin: Some(blast_begin),
                timer: Some(blast_timer),
                status: Some(blast_status),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.buffs.enter(
        HEAL_OVER_TIME,
        BuffEntry::new(BuffClass::BuffVelmaHealOverTime, BuffHooks { begin: Some(hot_begin), ..BuffHooks::default() }),
    )?;
    r.notifies.enter(
        "AnimNotify_ResetAnimBPAimDirection_C",
        NotifyHandler { instant: Some(reset_aim), ..NotifyHandler::default() },
    )
}
