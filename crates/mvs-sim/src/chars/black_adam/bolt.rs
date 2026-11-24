//! Black Adam's bolt and Earth Smash: C021_Bolt_C ([`Bolt`], a world actor; the
//! Lightning perk hands it to an ally), C021_EarthSmashIn_C ([`SmashIn`], a world actor dead at its spawn) and the
//! pillar of its victim buff Buff_C021_EarthSmashVictim (`BuffExt::EarthSmashVictim`, [`Pillar`]). Offsets are each
//! class's ubergraph's. C021_AttachedBeam_C is not ported: nothing of the build calls its Spawn Attached Beam (no
//! other class names it; C021_Bolt's Warning Visuals, an Actor variable, is never written).
//!
//! C021_Bolt_C (SpawnedActorParent; Die With Instigator, DestroyOnExitedArena?, Use Jump Buffer For Blast Box
//! Checks, ReceiveChargeValuesFromInstigator; no HitBox component: the parent's hit boxes have nothing to tag).
//! C021_Attack_Normal_Up_Montage spawns it at 0.306 s (ConditionalSpawner, Ability0Ready, socket joint1,
//! translation (0, 0, 2000)) and activates the last one at the end of its ActivateSpawned (0.301 s + 0.41 s,
//! ActivateOnEnd?).
//! - ReceiveBeginPlay: the parent's first. Its blast-box loop (SpawnedActorParent @9347-10808): a place outside the
//!   box with the jump buffer, ForceSpawnPositionInsideBlastBox false, DestroyOnExitedArena? true:
//!   DestroySpawnedActor(true) @15066-15076 at once; else the parent's 0.1 s looping CheckBlastBoxTimer
//!   (CheckBlastBoxExit). Then @8426 (authority): with Buff_C021_Lightning on the Instigator (a perk's buff: nothing
//!   of the build's data gives it) and GetAllies > 0, SetOwner(allies[0]) and the actor at that ally + (0, 0, 2000)
//!   @8997-9175; @9266 sfx_cha_C021_lightning_strike_leadin_preliminary_alert on the Instigator; Delay(0.7) @9359 ->
//!   @15: sfx_cha_C021_lightning_strike_leadin_electric on the Instigator (the activation, 0.405 s after the spawn,
//!   destroys the actor first unless the end comes late).
//! - ActivateObject @586 (authority), push_flow 3428 first:
//!   Get Invincible Fighters @9972: every BaseFighter that IsInvulnerable? joins IgnoreActors (AddUnique, kept).
//!   SphereTraceSingle(the owner's location -> the actor, 50, channel 5, no ignore list, self) @733: a blocking hit
//!   moves the actor to the hit's Location @1214.
//!   BoxTraceSingleForObjects(the actor -> the owner's location, (100, 100, 0), [HurtBox, Platform, WorldStatic],
//!   IgnoreActors, self) @1369. Target = the hit's Location when it blocks, else the owner's location.
//!   Bolt Visuals(Target) @1826 -> @9414: PS_C021_ThunderSky at the origin, beam source the actor, target Target.
//!   A hit actor that is a BaseFighter @2012: @3668 the Instigator's team = the target's and the target not
//!   invulnerable: SpawnBuffActor(Buff_AllyElectricGauntlets_C, target, Instigator, 1, 4.0); @4144 the actor's own
//!   pfgDoDamage(null, target, the hit below, OnHitBuffs Buff_Damage_Electric x 3 on a hit enemy); @3617 Warning
//!   Visuals (never written: null).K2_DestroyActor, K2_DestroyActor; @5937 PS_C021_NAD_Impact at Target; @3428
//!   sfx_cha_C021_lightning_strike_cast_bigbolt on the Instigator. Another hit actor (or none): @3617, @3428.
//!   The trace usually ends in Black Adam's own hurt capsules (the actor is 2000 over his socket): his own
//!   gauntlets, and pfgDoDamage on its own Instigator (the seismic reactor's self branch: OnHitBuffs' no-hit rows
//!   alone).
//!
//! C021_EarthSmashIn_C (SpawnedActorParent, the same parent flags): C021_Attack_Normal_Down_Montage spawns it at
//! 0.899 s. ReceiveBeginPlay (the parent's first: the charge copy, the blast test) @15 (authority, the owner a
//! BaseFighter): every BaseFighter of another TeamIndex @151-612 gets SpawnBuffActor(Buff_C021_EarthSmashVictim_C,
//! it, the owner, 1, 0.0, refresh) @681, and the returned buff's Percent Charged = the actor's
//! Comp_Pawn_ChargeAttack.PercentCharged @840 (after the buff's BeginPlay has run); then K2_DestroyActor @912. The
//! location is read by nothing: no spawn place is taken.
//!
//! Buff_C021_EarthSmashVictim_C (BuffDuration 0 from the spawn: SpawnBuffActor sets it ahead of BeginPlay):
//! - ReceiveBeginPlay @3870 (authority, the Instigator a BaseFighter): Pillar Source Fighter, Percent Charged = the
//!   Instigator's Comp_Pawn_ChargeAttackManager.PercentCharged @4024, Spawn Pillar @4187 -> @7057 -> @7018:
//!   CheckForSurface(GroundCheckDistanceFire 600), Stop Movement = true @7041 (ReceiveTick's CheckForSurface @4305
//!   returns at once from then on), @639.
//! - CheckForSurface @4329: AnyGroundDetected = false; i = 3..6 with i % 4 != 0, the owner a BaseFighter: the
//!   direction (0, sin(i pi / 2), cos(i pi / 2)), the base its Mesh socket root for i % 6 == 0 else its actor
//!   location, start = base + direction x (i % 6 == 0 ? -R : R), end = base + direction x the distance;
//!   SphereTraceSingle(R = GroundCheckRadius 75, channel 3, no ignore list, self) @5424; a hit: ground, Surface
//!   Normal = ImpactNormal, GroundPoint = ImpactPoint, the buff at GroundPoint, Surface = the hit actor. The last
//!   hit stands (i = 3: sim +x, 5: sim -x, 6: down from 75 over the root).
//! - @639: the owner not BPI_IsDead and a ground: Final Point = GroundPoint, the actor's rotation
//!   RotatorFromAxisAndAngle(X, Get Signed Angle Between(Surface Normal, up, X)) @892-1059,
//!   PS_C021_Impact_Warning_Decal @1127; BuffDuration (0) < Burst Stall (0.3):
//!   Delay(Lerp(Burst Stall, Burst Stall / 2, Percent Charged)) @1334 -> @15. No ground or a dead owner:
//!   K2_DestroyActor @1393.
//! - @15: PS_C021_Impact_Decal @145, Do Pillar Damage @199, Delay(1.0) @213 -> @268: the Emitters destroyed,
//!   K2_DestroyActor @550.
//! - Do Pillar Damage @8974 (the Instigator a BaseFighter): ignore its GetAllies + itself, H = Lerp(Min, Max Blast
//!   Height, pct), R = Lerp(Min, Max Blast Radius, pct); BoxOverlapActors(the actor + Surface Normal x H / 2,
//!   RotateVector((R, R, H), the actor's rotation), [Pawn, ProjectileDestructible]) @9653; per actor the buff's own
//!   pfgDoDamage(null, it) @3434: Damage Base Damage x Lerp(1, Damage Charge Multiplier, pct), BaseKnockBack
//!   (NearlyEqual(normal Z, 0, 1e-6) ? Horizontal : Vertical) x Lerp(1, Knockback Charge Multiplier, pct),
//!   KnockBackScaling 10, direction (normal Y, normal Z), HitReaction 5, HitPauseTime 0.2, HitStunMult 1.2,
//!   IgnoreFacing, no OnHitBuffs; HitSuccessful: sfx_cha_C021_attack_normal_down_hit_large (pct == 1) or _hit at
//!   GroundPoint.
//!
//! The box's half extent is RotateVector((R, R, H), the actor's rotation), so a wall's pillar has a negative
//! component: a wall on the right (i = 3, normal UE (0, 1, 0), angle -90) gives (R, H, -R), its Z below 0; one on
//! the left (i = 5, normal (0, -1, 0), angle 90) gives (R, -H, R), its Y below 0; the floor (angle 0) none. The shape
//! adapter clamps each component to 1e-4 and the box is world-axis aligned (`fighter::traces::box_overlap_actors`):
//! the right wall's pillar is a flat box H wide at GroundPoint's height, the left wall's a line 2R high.
// Deviation: the buff's latent Delays run in the frame's timer pass (the buff's `timer`) instead of its own tick; a
// Delay made in BeginPlay counts its frame, one made in a Delay's link counts from the next.
// Unverified: GetSocketLocation(root) is the mesh component's place plus the skeleton's root height (the world's
// 'root' socket).
// Unverified: GetDecayedBuffDuration and the SweepBuffState refresh of a second earth smash on a pillar still
// standing are `combat::buffs::spawn_buff`'s (null back: the second actor sets no percent, as @830).
// Unverified: HitSuccessful of the pillar's hit is taken as the call's Hit (false on Light Projectile Armor).

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, spawn_buff};
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hits::{
    actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, hit_buffs_ally, hit_buffs_call, hit_by, projectile_blocks,
};
use crate::combat::packet::{DataEnum, DataVec};
use crate::combat::phases::damage_spawned_actor;
use crate::combat::{BuffClass, BuffRef, HitPacket, OnHitBuffRow};
use crate::events::Event;
use crate::fighter::common::{actor_def, first_ally, latent_frames, lookup_owner, owner_team, warn_text};
use crate::fighter::control::{DodgeState, attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_HURT_BOX, OBJ_PAWN, OBJ_PLATFORM, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_STATIC, TRACE_PROJECTILE_GROUND,
    TRACE_PROJECTILE_GROUND_NO_PLATFORM, TraceBody, box_overlap_actors, box_trace_single_for_objects,
    sphere_trace_single,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable, mesh_wx, mesh_wy, no_hit};
use crate::math::ue::{KINDA_SMALL_NUMBER, cosf, deg_acos, normal, quat_from_axis_angle, quat_rotate_vector, sinf};
use crate::math::{self, to_int32};
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{flag, obj};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, check_blast_box_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::{is_match_fighter, match_fighters};
use crate::world::spawn_spot::{spawn_place, spawn_spot};
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, request_props};

const BOLT: &str = "C021_Bolt_C";
const SMASH_IN: &str = "C021_EarthSmashIn_C";
const VICTIM: &str = "Buff_C021_EarthSmashVictim_C";

/// `lerp` of the file: float32 steps.
fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + ((b - a) * t)
}

// ------------------------------------------------------------------ C021_Bolt

/// `Bolt`. For the drawing: (`x`, `y`) the actor (2000 over the socket); it lives from the spawn to its activation (or
/// the blast box). Bolt Visuals' places (@9414: the actor, Target) are the activation frame's PS_C021_ThunderSky event
/// (`to`: Target): the activation destroys the actor in the same frame. The strike on a fighter is the
/// PS_C021_NAD_Impact event.
#[derive(State, Debug)]
pub struct Bolt {
    pub attack_id: i32,
    pub facing: Real,
    pub x: Real,
    pub y: Real,
    /// The actor's ticks, the spawn frame's counted.
    pub age: i32,
    /// CheckBlastBoxTimer's next expiry, in seconds of counted deltas.
    pub blast_at: Real,
    /// Its BlastBoxBoundsMin / Max (SpawnedActorParent @9720-10573).
    pub blast_bounds: BlastBounds,
    /// GetOwner: the Instigator, or the ally the Lightning perk hands it to (SetOwner).
    pub trace_from: Option<FighterId>,
    /// IgnoreActors (Get Invincible Fighters' AddUnique).
    pub ignore: Vec<FighterId>,
}

impl Default for Bolt {
    /// The field initial values.
    fn default() -> Self {
        Bolt {
            attack_id: 0,
            facing: 0.0,
            x: 0.0,
            y: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
            trace_from: None,
            ignore: Vec::new(),
        }
    }
}

/// `SmashIn`: C021_EarthSmashIn_C's state: none (the actor is dead at its spawn).
#[derive(State, Debug, Default)]
pub struct SmashIn {}

/// `BOLT_HIT`: the hit ActivateObject builds @4144-5703 (KnockBackDirectionVector per target is added by the call).
pub fn bolt_hit() -> HitPacket {
    HitPacket {
        damage: Some(4.0),
        base_knock_back: Some(1700.0),
        knock_back_scaling: Some(10.0),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.175),
        hit_stun_mult: Some(1.0),
        on_hit_audio: Some(name!("sfx_cha_C021_lightning_strike_bigbolt_hit")),
        ignore_facing: Some(true),
        attack_id: Some(0),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(false),
        destroy_projectiles: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// `BOLT_BUFFS`: its OnHitBuffs @4215-4427.
fn bolt_buffs() -> [OnHitBuffRow; 1] {
    [OnHitBuffRow {
        applied_buffs: name!("Buff_Damage_Electric_C"),
        apply_buff_self: false,
        apply_buff_hit: true,
        apply_buff_ally: false,
        apply_buff_enemy: true,
        require_hit_to_apply_buff_to_self: false,
        stack_count: Some(3),
        buff_duration: Some(0.0),
    }]
}

fn bolt_of(w: &World, id: ActorId) -> Option<&Bolt> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Bolt(s) => Some(s),
        _ => None,
    }
}

fn bolt_mut(w: &mut World, id: ActorId) -> Option<&mut Bolt> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::Bolt(s) => Some(s),
        _ => None,
    }
}

fn sound(fighter: i32, ev: crate::names::Name) -> Event {
    Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None }
}

/// The kind's `spawn`: the place (the notify's socket, translation (0, 0, 2000)), PostActorConstruction's
/// FindTeleportSpot, the parent's BeginPlay blast-box loop (DestroySpawnedActor(true)), the Lightning perk's hand-over
/// @8426 and the alert sound.
fn bolt_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let sp = spawn_place(w, f, &np);
    let at = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: np.clone() },
        &SpawnSpotA { components: a.components.clone() },
        sp.x,
        sp.y,
    );
    let mut b =
        Bolt { attack_id: r.attack_id, facing: facing(w, f), x: at.x, y: at.y, trace_from: Some(f), ..Bolt::default() };
    let sb = spawn_box(w);
    // The parent's blast-box loop (DestroySpawnedActor(true)); the child's BeginPlay goes on.
    let dead = blast_box_loop(
        &sb,
        &mut b.x,
        &mut b.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut b.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    // @8426: the Lightning perk hands the bolt to the first ally (@8942 / @9020 Array_Get(GetAllies, 0), in
    // `common::allies`' order).
    if buff_of(w, f, name!("Buff_C021_Lightning_C")).is_some()
        && let Some(ally) = first_ally(w, f)
    {
        b.trace_from = Some(ally);
        b.x = w.fighter(ally).body.x;
        b.y = actor_y(w, ally) + 2000.0;
    }
    let fid = w.fighter(f).id;
    w.events.push(sound(fid, name!("sfx_cha_C021_lightning_strike_leadin_preliminary_alert")));
    (WorldActorState::Bolt(b), dead)
}

/// Delay(0.7) @9359 in ticks.
fn leadin() -> Real {
    latent_frames(0.7)
}

fn bolt_step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(owner_id) = w.actors.actors.get(id).map(|a| a.owner_id) else { return };
    let Some(b) = bolt_mut(w, id) else { return };
    b.age += 1;
    // CheckBlastBoxExit on the parent's 0.1 s looping timer (the spawn frame not counted); bolt.rs stores the next
    // expiry as a float32.
    let due = doe && (crate::consts::clock(crate::real(b.age - 1)) > f64::from(b.blast_at));
    if due {
        b.blast_at += 0.1;
    }
    if due && check_blast_box_exit(b.x, b.y, &b.blast_bounds) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    if crate::real(b.age) == leadin() {
        w.events.push(sound(owner_id, name!("sfx_cha_C021_lightning_strike_leadin_electric")));
    }
}

/// ActivateObject @586.
fn bolt_activate(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let inst = a.owner;
    let Some(b) = bolt_of(w, id) else { return };
    let from = b.trace_from.unwrap_or(inst);
    // Get Invincible Fighters @9972-581.
    for o in match_fighters(w) {
        if invulnerable(w, o)
            && let Some(b) = bolt_mut(w, id)
            && !b.ignore.contains(&o)
        {
            b.ignore.push(o);
        }
    }
    let (fx, fy) = (w.fighter(from).body.x, actor_y(w, from));
    // @733: from the owner's location to the actor.
    let (bx, by) = bolt_of(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
    let st = sphere_trace_single(w, fx, fy, bx, by, 50.0, TRACE_PROJECTILE_GROUND_NO_PLATFORM, &[], Some(me));
    if let Some(st) = st
        && st.blocking
        && let Some(b) = bolt_mut(w, id)
    {
        b.x = st.x;
        b.y = st.y;
    }
    // @1369: from the actor down to the owner's location.
    let (bx, by) = bolt_of(w, id).map_or((0.0, 0.0), |b| (b.x, b.y));
    let ignore: Vec<Option<ActorRef>> =
        bolt_of(w, id).map_or_else(Vec::new, |b| b.ignore.iter().map(|&o| Some(ActorRef::Fighter(o))).collect());
    let types = [crate::real(OBJ_HURT_BOX), crate::real(OBJ_PLATFORM), crate::real(OBJ_WORLD_STATIC)];
    let h = box_trace_single_for_objects(w, bx, by, fx, fy, 100.0, 0.0, 100.0, &types, &ignore, Some(me));
    let blocking = h.as_ref().is_some_and(|h| h.blocking);
    let (tx, ty) = match &h {
        Some(h) if blocking => (h.x, h.y),
        _ => (fx, fy),
    };
    let iid = w.fighter(inst).id;
    // Bolt Visuals(Target) @1826.
    w.events.push(Event::ProjectileDeath {
        cls: name!("C021_Bolt_C"),
        fighter: iid,
        vfx: Some(name!("PS_C021_ThunderSky")),
        x: bx,
        y: by,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: Some([tx, ty]),
    });
    let vic = match h.as_ref().and_then(|h| h.actor) {
        Some(ActorRef::Fighter(v)) if is_match_fighter(w, v) => Some(v),
        _ => None,
    };
    let dead_now = |w: &mut World| {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
    };
    if let Some(vic) = vic {
        let same_team = w.fighter(vic).team.team == w.fighter(inst).team.team;
        // @3668: an ally of the Instigator (himself among them) that is not invulnerable.
        if same_team && !invulnerable(w, vic) {
            spawn_buff(w, vic, name!("Buff_AllyElectricGauntlets_C"), 4.0, Some(1), Some(inst), true, None);
        }
        // @4144: (the owner's location - the actor's) x (0, 10, 1), (Y, Z) as the vector, not normalised.
        let ky = -(fx - bx) * 10.0;
        let kz = fy - by;
        let mut p = bolt_hit();
        p.knock_back_direction_vector = Some(DataVec { x: Some(ky), y: Some(kz), z: None });
        // IgnoreFacing: the vector's X is UE's Y, sim -x.
        let dir = V2 { x: -ky, y: kz };
        let rows = bolt_buffs();
        let (vx, vy) = (w.fighter(vic).body.x, actor_y(w, vic));
        if !same_team {
            if !crate::combat::phases::no_hit_by(w, inst, vic) && !projectile_blocks(w, &dealer(w, me), vic) {
                if invulnerable(w, vic) {
                    let tick = w.flow.tick;
                    attack_dodged(w, me, vic, b_attack_id(w, id), 1700.0, tick, None, false);
                } else {
                    dodge_over(w, me, vic);
                    let d = dealer(w, me);
                    let cp = charge_pct(w, me);
                    if apply_projectile_hit(w, &d, vic, &p, &rows, V2 { x: vx, y: vy }, Some(dir), &[], cp) {
                        actor_hit_enemy(w, &d, vic);
                    }
                }
            }
        } else if vic != inst {
            hit_buffs_call(w, inst, &rows);
            if !no_hit(w, vic) && !invulnerable(w, vic) {
                let mult = charge_buff(w, me);
                hit_buffs_ally(w, inst, vic, &rows, mult);
                let d = dealer(w, me);
                ally_hit(w, &d, vic, &p);
            }
        } else {
            hit_buffs_call(w, inst, &rows);
        }
        dead_now(w); // @3617
        w.events.push(Event::ProjectileDeath {
            cls: name!("C021_Bolt_C"),
            fighter: iid,
            vfx: Some(name!("PS_C021_NAD_Impact")),
            x: tx,
            y: ty,
            ox: 0.0,
            rot: [0.0; 3],
            scale: 1.0,
            tint: None,
            to: None,
        });
    } else {
        dead_now(w); // @3617
    }
    w.events.push(sound(iid, name!("sfx_cha_C021_lightning_strike_cast_bigbolt"))); // @3428
}

fn b_attack_id(w: &World, id: ActorId) -> i32 {
    bolt_of(w, id).map_or(0, |b| b.attack_id)
}

/// The Capsule (no profile in the data: the shape default OverlapAllDynamic, WorldDynamic, not gathered) is the
/// actor's one body: no query of the port asks for its type.
fn no_bodies(_: &World, _: ActorId, _: &mut Vec<TraceBody>) {}

// ------------------------------------------------------------------ C021_EarthSmashIn

/// The kind's `spawn` (the parent's BeginPlay copy of the charge, then @15): every enemy gets the victim buff, its
/// pillar made at once and its Percent Charged set @840. The actor is dead at its spawn @912.
fn smash_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = crate::fighter::common::actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    // receiveCharge(s, r.charge): the parent's BeginPlay copy, ahead of @840 (the world's addActor makes it again)
    let pct = if flag(&a.p, "ReceiveChargeValuesFromInstigator") { r.charge.map_or(0.0, |c| c.pct) } else { 0.0 };
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        if w.fighter(o).team.team == team {
            continue;
        }
        let Some(nb) = spawn_buff(w, o, name!("Buff_C021_EarthSmashVictim_C"), 0.0, Some(1), Some(f), true, None)
        else {
            continue;
        };
        if pillar(w, nb).is_some_and(|e| e.pending) {
            spawn_pillar(w, o, nb);
        }
        if let Some(e) = pillar_mut(w, nb) {
            e.pct = pct; // @840
        }
    }
    (WorldActorState::SmashIn(SmashIn {}), true)
}

// ------------------------------------------------------------------ Buff_C021_EarthSmashVictim

/// `Pillar`: Buff_C021_EarthSmashVictim's own variables (the buff entry's `ext`). For the drawing
/// x, y, angle, warned, burst, pct, quake. The emitters are
/// the buff's own (Spawn Emitter Team Color keeps them in Emitters, destroyed at its end @268-550), so they are flags
/// on the buff, not events.
#[derive(State, Debug)]
pub struct Pillar {
    /// Pillar Source Fighter, the Instigator (`None` while the buff has no extension).
    pub by: Option<FighterId>,
    /// The buff actor's Comp_Pawn_HitBoxHandler dodge variables.
    pub dodge: Option<DodgeState>,
    /// Percent Charged.
    pub pct: Real,
    /// Spawn Pillar not run yet.
    pub pending: bool,
    /// GroundPoint (sim): the buff actor's place (K2_SetActorTransform(Final Point) @1059).
    pub x: Real,
    pub y: Real,
    /// PS_C021_Impact_Warning_Decal spawned @1127.
    pub warned: bool,
    /// PS_C021_Impact_Decal spawned @145.
    pub burst: bool,
    /// Spawn Earthquake Visual Multicast ran: never (no caller).
    pub quake: bool,
    /// Surface Normal, UE (Y, Z).
    pub ny: Real,
    pub nz: Real,
    /// The actor's roll: Get Signed Angle Between's, degrees.
    pub angle: Real,
    /// The world tick the first Delay ends at (-1: none).
    pub fire_at: i32,
    /// The tick the Delay(1.0) ends at (-1: none).
    pub end_at: i32,
}

impl Default for Pillar {
    /// The begin's values as one packet.
    fn default() -> Self {
        Pillar {
            by: None,
            dodge: None,
            pct: 0.0,
            pending: true,
            x: 0.0,
            y: 0.0,
            warned: false,
            burst: false,
            quake: false,
            ny: 0.0,
            nz: 0.0,
            angle: 0.0,
            fire_at: -1,
            end_at: -1,
        }
    }
}

fn pillar(w: &World, b: BuffRef) -> Option<&Pillar> {
    match &buff(w, b)?.ext {
        BuffExt::EarthSmashVictim(e) if e.by.is_some() => Some(e),
        _ => None,
    }
}

fn pillar_mut(w: &mut World, b: BuffRef) -> Option<&mut Pillar> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::EarthSmashVictim(e) if e.by.is_some() => Some(e),
        _ => None,
    }
}

/// Get Signed Angle Between(n, up, X) for a normal in UE (Y, Z): DegAcos(Normal(n, 1e-4) . up) x
/// SignOfFloat((up x Normal(n, 1e-4)) . X).
fn signed_angle(ny: Real, nz: Real) -> Real {
    let [_, y, z] = normal([0.0, ny, nz], KINDA_SMALL_NUMBER);
    deg_acos(z) * math::sign(-y)
}

/// The Mesh's socket root: the mesh component's place plus the skeleton's root height (the world's vacuum socket).
fn root_y(w: &World, f: FighterId) -> Real {
    let fi = w.fighter(f);
    mesh_wy(w, f) + crate::math::narrow(crate::combat::hits::char_data(w, fi).boxes.mesh_z)
}

/// CheckForSurface(distance) @4329: the ground found, written into the pillar's normal and ground point.
fn check_for_surface(w: &mut World, f: FighterId, b: BuffRef, dist: Real) -> bool {
    let mut ground = false;
    let r = 75.0; // GroundCheckRadius
    for i in 3..=6 {
        if i % 4 == 0 {
            continue;
        }
        let a = (crate::real(i) * std::f32::consts::PI) / 2.0;
        let (dy, dz) = (sinf(a), cosf(a)); // the Sin / Cos nodes @5152 / @5115: UE (0, Y, Z)
        let root = i % 6 == 0;
        let bx = if root { mesh_wx(w, f) } else { w.fighter(f).body.x };
        let by = if root { root_y(w, f) } else { actor_y(w, f) };
        let sel = if root { -r } else { r };
        let (x0, y0, x1, y1) = (bx - dy * sel, by + dz * sel, bx - dy * dist, by + dz * dist);
        let h = sphere_trace_single(w, x0, y0, x1, y1, r, TRACE_PROJECTILE_GROUND, &[], None);
        let Some(h) = h.filter(|h| h.blocking) else { continue };
        ground = true;
        if let Some(e) = pillar_mut(w, b) {
            e.ny = -h.nx; // ImpactNormal in UE (Y, Z)
            e.nz = h.ny;
            e.x = h.ix;
            e.y = h.iy;
        }
    }
    ground
}

/// Spawn Pillar @7018 and @639.
fn spawn_pillar(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some(e) = pillar_mut(w, b) {
        e.pending = false;
    }
    let ground = check_for_surface(w, f, b, 600.0);
    let tick = w.flow.tick;
    let dead = w.fighter(f).life.dead > 0.0;
    let Some(e) = pillar_mut(w, b) else { return };
    if dead || !ground {
        // K2_DestroyActor @1393: the timer pass of this frame
        e.fire_at = tick;
        e.end_at = tick;
        return;
    }
    e.angle = signed_angle(e.ny, e.nz);
    e.warned = true; // PS_C021_Impact_Warning_Decal, scale 1, offset (300, 0, 0) @1127
    // BuffDuration 0 < Burst Stall: Delay(Lerp(0.3, 0.15, pct)) made in BeginPlay.
    e.fire_at = tick + to_int32(latent_frames(lerp(0.3, 0.15, e.pct))) - 1;
}

/// Do Pillar Damage @8974.
fn pillar_damage(w: &mut World, b: BuffRef) {
    let Some(e) = pillar(w, b) else { return };
    let Some(inst) = e.by else { return };
    let (pct, ex, ey, ny, nz, angle) = (e.pct, e.x, e.y, e.ny, e.nz, e.angle);
    let team = w.fighter(inst).team.team;
    let mut ignore: Vec<Option<ActorRef>> = match_fighters(w)
        .into_iter()
        .filter(|&o| o != inst && w.fighter(o).team.team == team && w.fighter(o).life.dead <= 0.0)
        .map(|o| Some(ActorRef::Fighter(o)))
        .collect();
    ignore.push(Some(ActorRef::Fighter(inst)));
    let hh = lerp(175.0, 300.0, pct);
    let rr = lerp(80.0, 100.0, pct);
    // RotateVector((R, R, H), FQuat(X, angle)): X stays, (Y, Z) turn by the angle.
    // Unverified: the build turns the extent by the actor's rotation (GreaterGreater_VectorRotator @9504, FRotator::
    // RotateVector's rotation matrix) after RotatorFromAxisAndAngle -> MakeTransform -> K2_GetActorRotation (@950,
    // @1000, @9269); the port turns it by the axis-angle quaternion alone.
    let [_, hy, hz] = quat_rotate_vector(quat_from_axis_angle([1.0, 0.0, 0.0], angle), [rr, rr, hh]);
    // As passed (UE Y along sim x, Z up, X the depth); box_overlap_actors clamps a negative one.
    let (hx, hy, depth) = (hy, hz, rr);
    let cx = ex - ny * (hh / 2.0);
    let cy = ey + nz * (hh / 2.0);
    let hit = HitPacket {
        damage: Some(3.0 * lerp(1.0, 2.0, pct)),
        base_knock_back: Some((if nz.abs() <= 1e-6 { 800.0 } else { 1400.0 }) * lerp(1.0, 1.25, pct)),
        knock_back_scaling: Some(10.0),
        knock_back_direction_vector: Some(DataVec { x: Some(ny), y: Some(nz), z: None }),
        attack_tag: Some(name!("None")),
        hit_reaction: Some(DataEnum::Value(5)),
        hit_pause_time: Some(0.2),
        hit_stun_mult: Some(1.2),
        ignore_facing: Some(true),
        attack_id: Some(0),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        additive_kb_mode: Some(DataEnum::Value(0)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(false),
        destroy_projectiles: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    };
    let bkb = hit.base_knock_back.unwrap_or(0.0);
    let dir = V2 { x: -ny, y: nz };
    let me = ActorRef::Buff(b);
    let types = [crate::real(OBJ_PAWN), crate::real(OBJ_PROJECTILE_DESTRUCTIBLE)];
    let list = box_overlap_actors(w, cx, cy, hx, hy, depth, &types, &ignore, -1.0);
    for a in list {
        let mut ok = false;
        match a {
            ActorRef::Projectile(pid) => {
                let Some(p) = w.actors.projectiles.get(pid) else { continue };
                let cls = p.cls;
                let owner = ActorRef::Fighter(lookup_owner(w, a));
                let is_proj = actor_def(w, owner, cls)
                    .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
                    .is_some_and(|c| flag(c, "IsProjectile"));
                if !is_proj {
                    // A spawned actor whose Comp_Actor_Destructible is not IsProjectile (SeismicReactor_C, met at its
                    // DamageBox): DamageAndLaunch at once; HitSuccessful is its Hit.
                    ok = damage_spawned_actor(w, me, pid, &hit);
                } else if !p.dead {
                    // A projectile victim: its OnHitByAlly / OnHitByEnemy alone, HitSuccessful false. The dealer is no
                    // projectile: attacker type -1 (as a melee hit's).
                    let ally = owner_team(w, a) == team;
                    hit_by(w, pid, ally, dir.x, dir.y, bkb, -1, None);
                }
            }
            ActorRef::Fighter(vic) if is_match_fighter(w, vic) => {
                if crate::combat::phases::no_hit_by(w, inst, vic) {
                    continue;
                }
                if invulnerable(w, vic) {
                    // AttackingActor: the buff actor (its handler's owner).
                    let tick = w.flow.tick;
                    attack_dodged(w, me, vic, 0, bkb, tick, None, false);
                } else {
                    dodge_over(w, me, vic);
                    let mut d = dealer(w, me);
                    d.x = ex;
                    d.y = ey;
                    d.attack_id = 0;
                    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                    ok = apply_projectile_hit(w, &d, vic, &hit, &[], at, Some(dir), &[], 0.0);
                }
            }
            _ => {}
        }
        if !ok {
            continue;
        }
        let ev = if pct == 1.0 {
            name!("sfx_cha_C021_attack_normal_down_hit_large")
        } else {
            name!("sfx_cha_C021_attack_normal_down_hit")
        };
        let fid = w.fighter(inst).id;
        w.events.push(Event::Sound { fighter: fid, ev, x: Some(ex), y: Some(ey), hit: None, pooled: false, key: None });
    }
}

/// ReceiveBeginPlay @3870: Spawn Pillar needs the world, so EarthSmashIn's spawn runs it on the buff it got back,
/// before its own @840 (a spawn from elsewhere runs it in the next timer pass, warned).
fn victim_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let Some(from) = from else {
        warn_text(w, &format!("{VICTIM} without an Instigator: no pillar"));
        return;
    };
    let e = Pillar { by: Some(from), pct: w.fighter(from).charge.charge_pct, ..Pillar::default() };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::EarthSmashVictim(e);
    }
}

/// The buff's timer pass: true ends the buff (@550, or @1393).
fn victim_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = pillar(w, b) else { return true };
    if e.pending {
        warn_text(w, &format!("{VICTIM} spawned outside {SMASH_IN}: the pillar a frame late"));
        spawn_pillar(w, f, b);
    }
    let tick = w.flow.tick;
    let Some(e) = pillar(w, b) else { return true };
    if e.end_at >= 0 && tick >= e.end_at {
        return true; // @550 (or @1393)
    }
    if e.fire_at >= 0 && tick == e.fire_at {
        // @15
        if let Some(e) = pillar_mut(w, b) {
            e.burst = true; // PS_C021_Impact_Decal, scale Lerp(0.6, 1, pct), offset (300, 0, 0) @145
        }
        pillar_damage(w, b);
        if let Some(e) = pillar_mut(w, b) {
            e.end_at = tick + to_int32(latent_frames(1.0)); // made in the Delay's link: from the next frame
        }
    }
    false
}

/// This file's registry entries (the bolt, the smash, the victim buff).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let bolt = WorldActorHooks {
        spawn: Some(bolt_spawn),
        // nothing reads it: its traces have their own radii and extents, its effects are emitters at locations
        size: Some(SizeHooks::default()),
        bodies: Some(no_bodies),
        activate: Some(bolt_activate),
        step: Some(bolt_step),
        ..WorldActorHooks::default()
    };
    // Owner reads: its place, facing, team, buffs and charge, its socket joint1 (spawn_place warns and takes the
    // actor's place for a mesh without it); nothing of Pawn_C021 alone.
    let mut e = ActorEntry::new(ActorClass::World(WorldActorClass::C021Bolt), ActorHooks::World(bolt));
    e.foreign_owner = true;
    r.actors.enter(BOLT, e)?;
    // Owner reads: its team and charge.
    let smash = WorldActorHooks { spawn: Some(smash_spawn), bodies: Some(no_bodies), ..WorldActorHooks::default() };
    let mut e = ActorEntry::new(ActorClass::World(WorldActorClass::C021EarthSmashIn), ActorHooks::World(smash));
    e.foreign_owner = true;
    r.actors.enter(SMASH_IN, e)?;
    let victim = BuffHooks { begin: Some(victim_begin), timer: Some(victim_timer), ..BuffHooks::default() };
    r.buffs.enter(VICTIM, BuffEntry::new(BuffClass::BuffC021EarthSmashVictim, victim))
}
