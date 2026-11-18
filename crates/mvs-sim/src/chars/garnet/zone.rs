//! The up special: the ProjectileAimer use, GarnetLightningBolt_C ([`GarnetBolt`],
//! a projectile), GarnetElectricZone_C ([`ElectricZone`], a DealerActor with a Comp_State_Manager), the electric charge
//! and the zone's FullPower timer (its part of the joined timerTick).
//!
//! Garnet_Attack_Special_U_Montage3's Spawner notify makes a ProjectileAimer (AimInfo AimType 0, the stick rule) whose
//! ActorClass is the Blueprints-folder GarnetLightningBolt; the montage's ActivateSpawned fires it. The bolt's end makes
//! the zone. Classes:
//! - GarnetLightningBolt (Characters/Garnet/Blueprints, a ParentProjectilePhysics child). Its flight, hit (HitBoxDamage
//!   Damage 1, BaseKnockBack 500, On Hit Buffs Buff_Damage_Electric 1 stack to an enemy hit), gravity scale 4, the
//!   environment deaths and the ability slot at the spawn are the parent's. Its own graph (ubergraph offsets): BeginPlay
//!   @1096 the parent's, then sfx_cha_garnet_attack_special_u_missile_loop on itself @1106; Blocked @1176:
//!   ProjectileBlocked = true, then the parent's Blocked @1187; EndPlay @1072 -> @10 (authority):
//!   SphereTraceSingleForObjects(its location, its location, radius 60, [WorldStatic, Platform], ignoring itself) @87; a
//!   hit and not ProjectileBlocked @235-302, with the Instigator's Comp_Spawner valid @336-416 ->
//!   CompSpawnActor(GarnetElectricZone, at its location, the Instigator, AttackID 0) @566. A bolt that ends in the air
//!   (its lifetime, the blast box) or that was blocked makes no zone. Characters/Garnet/Projectiles/GarnetLightningBolt
//!   (the legacy class) is reached by nothing live: not entered.
//! - GarnetElectricZone (a SpawnedActorParent child): at the class below.
//! - Buff_GarnetElecInsideBubble, Buff_GarnetElecOutsideBubble, Buff_GarnetFullPowerGauntlets: `combat::gauntlet`.
//!
//! GarnetZoneState (ChargingElectricity / ElectricityFullyCharged) is dormant in the build (StartElectricChargeServer is
//! reached only from the dormant montages' AnimNotify_GarnetStartElectricCharge): no writer, the CDO false stays.
//!
//! GarnetElectricZone's data: TimeTillFullPower 5, DestroyOnBlocked false, Die With Instigator true, CanBePooled false,
//! Struct Ref (the explosion: Damage 6, BaseKnockBack 1800, KnockBackScaling 21, direction (0.1, 1), HitReaction 4,
//! HitPauseTime 0.2, IgnoreFacing, ForceKnockback, OnHitAudio sfx_cha_garnet_hit_lightningbolt, OnHitEffectOverride
//! PS_HitImpact_Big), Explosion On Hit Buffs (Buff_Damage_Electric, 4 stacks, to an enemy hit), no HitBoxTags component,
//! the passive Buff_Elemental_Electric (BuffSelf, 1 stack, Duration 0). Collision (a capsule 300 / 750 turned (0, 90, 90):
//! its axis along the depth, a circle of 300 in the play plane) and Sphere (300): the same circle here.
//! BPI_GetSphereCollision returns Sphere (Buff_Elemental_Electric's looping pulse: `ElementalParts::sphere`), the
//! graph's fighter loops read Collision.
//! - ReceiveBeginPlay @2265: the parent's, then SphereTraceMultiForObjects(location, location - (0, 0, 500), 30,
//!   [WorldStatic, Platform]) @2379; the first hit whose ImpactNormal is (0, 0, 1) within 0.0001 @1665 ->
//!   K2_SetActorLocation(its Location) @1944, break; none: it stays. Then (authority) K2_SetTimerDelegate(FullPower,
//!   TimeTillFullPower, one-shot) @2049, and sfx_cha_garnet_attack_special_u_charge_loop on itself @2537.
//! - Collision's begin overlap @3995 (authority): a BP_FighterCharacter of the zone's TeamIndex gets
//!   SpawnBuffActor(Buff_GarnetElecInsideBubble, it, it, 0 stacks, 0.0: no life timer) @4384. Its end overlap @4469
//!   (authority, not AtFullPower): the same gates; its InsideBubble destroyed @5050, then
//!   SpawnBuffActor(Buff_GarnetElecOutsideBubble, it, it, 0, 2.0) @5137. Garnet herself is of the team.
//! - FullPower @2916 (the timer): AtFullPower = true; each BP_FighterCharacter Collision overlaps, of the zone's team:
//!   SpawnBuffActor(Buff_GarnetFullPowerGauntlets, it, it, 0, 6.0) @3821; then ReleaseToPool @3980, which without
//!   CanBePooled is K2_DestroyActor.
//! - ReceiveEndPlay @5257: the parent's (its buff children end; then @15507 / @15557 -> @17018-18136 DeathVFX
//!   PS_GarnetUpSpecialFullChargedHit and 'Ak Death Location' sfx_cha_garnet_attack_special_u_charge_loop_stop at the
//!   actor unless DestroyVFXBlocked), then @2220 (authority, not DestroyedEarly): each BaseFighter Collision overlaps
//!   @84-1054 with a Comp_Actor_Destructible: Struct Ref.ForceKnockback = its TeamIndex != the zone's @824-906, then
//!   pfgDoDamage(no hit box, it, Struct Ref, no hurt box, no elements, Explosion On Hit Buffs) @1054, whatever the team.
//!   Then the components' end overlaps (DestroyComponent on a primitive fires end-overlap events: after EndPlay): at full
//!   power they do nothing; a zone ended before it swaps an ally's InsideBubble for OutsideBubble there.
//! - BPI Clean (BPI_EnvironmentalHazard; C019ScrubShield): MulticastCleaned @5281 (DestroyedEarly = true,
//!   BPI_SetDestroyEffectsBlocked(true) @5292: no death effects), DestroySpawnedActor(false), Success true: no explosion.
//!
//! Model: the overlaps are tested once a tick in the zone's step against the bodies the traces gather (as Black Adam's
//! field); FullPower in the frame's timer pass (the owner's `timer_tick`); the explosion inside FullPower's destroy,
//! behind the parent's EndPlay part, and in the list pass for the other ends.
// Deviation: a fighter through the circle inside one tick is missed; the begin and end overlaps come in the zone's
// step, not inside each fighter's move; FullPower and the explosion read the overlaps as they are at the call.
// Deviation: the circle is the Collision capsule's section and the Sphere's at depth 0 (the capsule's 750 half height
// along the depth is not read); the order of the explosion's and FullPower's victims is the traces' gather order
// (fighters in sim order), not PhysX's.
// Unverified: a zone whose owner is no Garnet (a reflected bolt's reflector, a disguise's copy of the up special) has no
// Garnet timer_tick: its FullPower runs in the kind's `late`, ahead of the timer pass.
// Not here: the zone's own VFX (PS_Garnet_DropBubble: the view's).

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p};
use crate::actors::world_actor::{actor_elem, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId, WorldActor};
use crate::actors::{WorldActorClass, WorldActorState};
use crate::chars::garnet::state::garnet_of;
use crate::combat::HitPacket;
use crate::combat::buffs::{buff_of, end_buff, spawn_buff};
use crate::combat::elem::{charge_pct, elem_tick, end_actor_children, instigator_charge};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::packet::OnHitBuffRow;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_data, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_HURT_BOX, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PLATFORM, OBJ_PROJECTILE_DESTRUCTIBLE,
    OBJ_WORLD_STATIC, TraceHit, sphere_overlap_actors, sphere_trace_multi_for_objects,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable, no_hit};
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, ElementalParts, HazardHooks, ProjectileHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::reset_pass;
use crate::world::spawning::{comp_spawn_actor, death_vfx, new_world_actor};
use std::sync::Arc;

pub const LIGHTNING_BOLT: &str = "GarnetLightningBolt_C";
pub const ELECTRIC_ZONE: &str = "GarnetElectricZone_C";
const ZONE: Name = name!("GarnetElectricZone_C");
const INSIDE_BUBBLE: Name = name!("Buff_GarnetElecInsideBubble_C");
const OUTSIDE_BUBBLE: Name = name!("Buff_GarnetElecOutsideBubble_C");
const FULL_POWER: Name = name!("Buff_GarnetFullPowerGauntlets_C");
const BOLT_LOOP: Name = name!("sfx_cha_garnet_attack_special_u_missile_loop");
const ZONE_LOOP: Name = name!("sfx_cha_garnet_attack_special_u_charge_loop");

/// HitBox, the object type the zone's Collision and Sphere have (the `responder` of the overlap queries).
const OBJ_HIT_BOX: Real = 7.0;

/// The MakeArray [0, 10] of both traces (bolt @44, zone @2275).
fn ground() -> [Real; 2] {
    [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_PLATFORM)]
}

/// What the zone's Collision and Sphere overlap (both: object type HitBox, Overlap to Pawn, DamageDestructible,
/// ProjectileDestructible, ItemDestructible and HurtBox): a fighter's capsules, the spawned actors' bodies.
fn fighter_types() -> [Real; 2] {
    [crate::real(OBJ_PAWN), crate::real(OBJ_HURT_BOX)]
}

fn actor_types() -> [Real; 3] {
    [crate::real(OBJ_DAMAGE_DESTRUCTIBLE), crate::real(OBJ_PROJECTILE_DESTRUCTIBLE), crate::real(OBJ_ITEM_DESTRUCTIBLE)]
}

// ---------------------------------------------------------------- GarnetLightningBolt

/// `GarnetBolt extends Projectile` (GarnetLightningBolt_C).
#[derive(State, Debug, Default)]
pub struct GarnetBolt {
    /// ProjectileBlocked (CDO false): Blocked @1176.
    pub projectile_blocked: bool,
    /// Its ReceiveEndPlay ran (the world's destroy and its list pass both reach it).
    pub bolt_ended: bool,
}

fn bpr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn bpm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn bolt_mut(w: &mut World, id: ProjectileId) -> &mut GarnetBolt {
    match &mut bpm(w, id).ext {
        ProjectileExt::GarnetBolt(b) => b,
        e => panic!("GarnetLightningBolt_C without its state: {e:?}"),
    }
}

fn bolt_loop(w: &World, id: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: bpr(w, id).seq }
}

/// ReceiveEndPlay @10-692.
fn bolt_end(w: &mut World, id: ProjectileId) {
    let b = bolt_mut(w, id);
    if b.bolt_ended {
        return;
    }
    b.bolt_ended = true;
    let blocked = b.projectile_blocked;
    let (fighter, key) = (bpr(w, id).owner_id, bolt_loop(w, id));
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    let (x, y) = (bpr(w, id).x, bpr(w, id).y);
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, x, y, x, y, 60.0, &ground(), &[], Some(ActorRef::Projectile(id)), &mut hits);
    let hit = !hits.is_empty(); // the single trace's ReturnValue: a body it starts in
    if !hit || blocked {
        return;
    }
    // GetInstigator: the projectile's owner (a reflect sets it again), whose Comp_Spawner spawns.
    if let Some(f) = bpr(w, id).owner {
        spawn_zone(w, f, x, y);
    }
}

fn bolt_construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::GarnetBolt(GarnetBolt::default());
}

/// @1106: PF_Post_Event(sfx_cha_garnet_attack_special_u_missile_loop, self): keyed per projectile, stopped at its end
/// with the port's default fade.
fn bolt_begin(w: &mut World, id: ProjectileId) {
    let (fighter, key) = (bpr(w, id).owner_id, bolt_loop(w, id));
    w.events.push(Event::SoundStart { fighter, ev: BOLT_LOOP, key });
}

/// @1176-1187: ProjectileBlocked = true, then SpawnedActorParent's Blocked (DestroyOnBlocked true: the world's rule for a
/// class without the hook).
fn bolt_blocked(w: &mut World, id: ProjectileId, _by: ActorRef) {
    bolt_mut(w, id).projectile_blocked = true;
    if flag(class_p(w, bpr(w, id)), "DestroyOnBlocked") {
        bpm(w, id).blocked_destroy = true;
    }
}

fn bolt_kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(bolt_construct),
        begin: Some(bolt_begin),
        blocked: Some(bolt_blocked),
        // ReceiveEndPlay runs inside K2_DestroyActor (`destroy_projectile`); `end` for the ends that do not go through it.
        destroyed: Some(bolt_end),
        end: Some(bolt_end),
        ..ProjectileHooks::default()
    }
}

// ---------------------------------------------------------------- GarnetElectricZone

/// `ElectricZone` (GarnetElectricZone_C).
#[derive(State, Debug, Default)]
pub struct ElectricZone {
    /// The AttackID (the Instigator's attack count at the spawn: Comp_Spawner @2444-2747).
    pub attack_id: i32,
    /// HitBoxDamage.
    pub hit: HitPacket,
    /// Its place in the play plane, after BeginPlay's trace down (sim x, y).
    pub x: Real,
    pub y: Real,
    /// FacingFlipOnSpawn: the Instigator's facing at the spawn.
    pub facing: Real,
    /// Collision's CapsuleRadius (Sphere's SphereRadius is the same 300), unscaled.
    pub radius: Real,
    /// AtFullPower (CDO false): FullPower sets it.
    pub at_full_power: bool,
    /// The World.tick the one-shot FullPower timer fires at (BeginPlay schedules it TimeTillFullPower, 5 s, after the
    /// trace; -1 before): the view's charge = how far the zone is from it.
    pub full_power_at: Real,
    /// DestroyedEarly (MulticastCleaned).
    pub destroyed_early: bool,
    /// SpawnedActorParent DestroyVFXBlocked (BPI_SetDestroyEffectsBlocked, MulticastCleaned @5292).
    pub destroy_fx_blocked: bool,
    /// Its ReceiveEndPlay ran.
    pub ended: bool,
    /// The fighters Collision overlaps (ids, in the order they began).
    pub inside: Vec<i32>,
}

fn zs(w: &World, id: ActorId) -> &ElectricZone {
    match &w.actors.actors.get(id).expect("a listed actor").state {
        WorldActorState::ElectricZone(z) => z,
        s => panic!("GarnetElectricZone_C without its state: {s:?}"),
    }
}

fn zs_mut(w: &mut World, id: ActorId) -> &mut ElectricZone {
    match &mut w.actors.actors.get_mut(id).expect("a listed actor").state {
        WorldActorState::ElectricZone(z) => z,
        s => panic!("GarnetElectricZone_C without its state: {s:?}"),
    }
}

fn zone_owner(w: &World, id: ActorId) -> FighterId {
    w.actors.actors.get(id).expect("a listed actor").owner
}

fn zone_dead(w: &World, id: ActorId) -> bool {
    w.actors.actors.get(id).is_none_or(|a| a.dead)
}

/// Collision and Sphere are the root Capsule's children with no scale of their own: GetScaledCapsuleRadius and the
/// overlap shapes take the root's scale.
fn scaled_radius(w: &World, id: ActorId) -> Real {
    zs(w, id).radius * w.actors.actors.get(id).expect("a listed actor").size_scale.unwrap_or(1.0)
}

fn zone_loop(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: name!("garnetzone"), n: w.actors.actors.get(id).expect("a listed actor").seq }
}

/// The ElectricZone's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &ElectricZone) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The ElectricZone as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &ElectricZone) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The ElectricZone's HitBoxDamage packet (`hit`).
pub fn hit(s: &ElectricZone) -> &HitPacket {
    &s.hit
}

/// CompSpawnActor(GarnetElectricZone) from the bolt's EndPlay: AttackID 0 is the Instigator's attack count
/// (Comp_Spawner @2444-2747), FacingFlipOnSpawn the Instigator's facing. The trace down, the timer, the loop and the
/// first overlaps are the BeginPlay (`zone_begin`), behind `add_actor`; then AfterSpawnedActor (Buff_Enlarge's binding,
/// the buffs').
fn spawn_zone(w: &mut World, f: FighterId, x: Real, y: Real) {
    let data = Arc::clone(&w.data);
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(f), ZONE) else {
        warn_text(w, &format!("spawned actor not exported ({ELECTRIC_ZONE})"));
        return;
    };
    let none = mvs_data::value::Props::new();
    let radius = d
        .components
        .get("Collision")
        .and_then(|c| c.as_obj())
        .and_then(|c| num(c, "CapsuleRadius"))
        .or_else(|| obj(&d.components, "Sphere").and_then(|c| num(c, "SphereRadius")))
        .unwrap_or(0.0);
    let state = ElectricZone {
        attack_id: w.fighter(f).decay.attack_count,
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        x,
        y,
        facing: facing(w, f),
        radius,
        full_power_at: -1.0,
        ..ElectricZone::default()
    };
    let actor =
        new_world_actor(w, ZONE, WorldActorClass::GarnetElectricZone, f, false, WorldActorState::ElectricZone(state));
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    comp_spawn_actor(w, id, charge, f);
}

/// ReceiveBeginPlay @2265 (behind the parent's, `add_actor`): the trace down and the first upward face @2275-2215, the
/// timer @2049, the loop @2537, the spawn's begin overlaps.
fn zone_begin(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let (x, y) = (zs(w, id).x, zs(w, id).y);
    let mut hits: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, x, y, x, y - 500.0, 30.0, &ground(), &[], Some(me), &mut hits);
    for h in &hits {
        if h.nx.abs() <= 0.0001 && (h.ny - 1.0).abs() <= 0.0001 {
            let z = zs_mut(w, id);
            z.x = h.x;
            z.y = h.y;
            break;
        }
    }
    // @2049: it fires at the timer pass timerTicks later (Timer set path).
    let till = num(actor_p(w, me), "TimeTillFullPower").unwrap_or(0.0);
    zs_mut(w, id).full_power_at = crate::real(w.flow.tick) + timer_ticks(Some(till));
    let (fighter, key) = (w.actors.actors.get(id).expect("a listed actor").owner_id, zone_loop(w, id));
    w.events.push(Event::SoundStart { fighter, ev: ZONE_LOOP, key });
    overlap_pass(w, id); // the spawn's begin overlaps, behind BeginPlay (Begin overlaps at a run-time spawn)
}

/// The fighters Collision overlaps now (GetOverlappingActors(BaseFighter_C / BP_FighterCharacter_C)).
fn overlap_fighters(w: &mut World, id: ActorId) -> Vec<FighterId> {
    let (x, y, r) = (zs(w, id).x, zs(w, id).y, scaled_radius(w, id));
    sphere_overlap_actors(w, x, y, r, &fighter_types(), &[], OBJ_HIT_BOX)
        .into_iter()
        .filter_map(|a| if let ActorRef::Fighter(f) = a { Some(f) } else { None })
        .collect()
}

fn zone_team(w: &World, id: ActorId) -> i32 {
    w.fighter(zone_owner(w, id)).team.team
}

/// @3995-4384
fn begin_overlap(w: &mut World, id: ActorId, o: FighterId) {
    if w.fighter(o).team.team != zone_team(w, id) {
        return;
    }
    spawn_buff(w, o, INSIDE_BUBBLE, 0.0, Some(0), Some(o), true, None);
}

/// @4469-5137
fn end_overlap(w: &mut World, id: ActorId, o: FighterId) {
    if zs(w, id).at_full_power || w.fighter(o).team.team != zone_team(w, id) {
        return;
    }
    if let Some(b) = buff_of(w, o, INSIDE_BUBBLE) {
        end_buff(w, o, b);
    }
    spawn_buff(w, o, OUTSIDE_BUBBLE, 2.0, Some(0), Some(o), true, None);
}

fn overlap_pass(w: &mut World, id: ActorId) {
    let now = overlap_fighters(w, id);
    let now_ids: Vec<i32> = now.iter().map(|&o| w.fighter(o).id).collect();
    let mut k = zs(w, id).inside.len();
    while k > 0 {
        k -= 1;
        let Some(&fid) = zs(w, id).inside.get(k) else { continue };
        if now_ids.contains(&fid) {
            continue;
        }
        zs_mut(w, id).inside.remove(k);
        if let Some(o) = w.by_id(fid) {
            end_overlap(w, id, o);
        }
    }
    for (o, fid) in now.into_iter().zip(now_ids) {
        if zs(w, id).inside.contains(&fid) {
            continue;
        }
        zs_mut(w, id).inside.push(fid);
        begin_overlap(w, id, o);
    }
}

/// FullPower @2916-3980.
fn full_power(w: &mut World, id: ActorId) {
    zs_mut(w, id).at_full_power = true;
    for o in overlap_fighters(w, id) {
        if w.fighter(o).team.team == zone_team(w, id) {
            spawn_buff(w, o, FULL_POWER, 6.0, Some(0), Some(o), true, None);
        }
    }
    // ReleaseToPool -> K2_DestroyActor: the parent's EndPlay part (its buff children), then the class's.
    w.actors.actors.get_mut(id).expect("a listed actor").dead = true;
    end_actor_children(w, ActorRef::Actor(id));
    zone_end(w, id);
}

/// ReceiveEndPlay @2220-1158, then the end overlaps of its destroyed components.
fn zone_end(w: &mut World, id: ActorId) {
    if zs(w, id).ended {
        return;
    }
    zs_mut(w, id).ended = true;
    let (fighter, key) = (w.actors.actors.get(id).expect("a listed actor").owner_id, zone_loop(w, id));
    w.events.push(Event::SoundStop { fighter, key, fade: None, ev: None, curve: None, cut: false });
    if reset_pass(w) {
        return; // the Lab reset's end pass acts on the old pawns
    }
    // SpawnedActorParent ReceiveEndPlay @15557 -> @17018-18136, ahead of the class's part: the event shape of
    // `death_vfx` (Death VFX Offset and rotation as there, the actor's yaw by its facing).
    if !zs(w, id).destroy_fx_blocked {
        let me = ActorRef::Actor(id);
        let (x, y, fc) = (zs(w, id).x, zs(w, id).y, zs(w, id).facing);
        let ev = text(actor_p(w, me), "Ak Death Location").filter(|s| !s.is_empty()).map(|s| w.lit(s));
        if let Some(ev) = ev {
            w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
        }
        let ev = death_vfx(w, ZONE, fighter, actor_p(w, me), x, y, fc);
        w.events.push(ev);
    }
    if !zs(w, id).destroyed_early {
        explode(w, id);
    }
    let inside = std::mem::take(&mut zs_mut(w, id).inside);
    for fid in inside {
        if let Some(o) = w.by_id(fid) {
            end_overlap(w, id, o);
        }
    }
}

/// @84-1054: pfgDoDamage on each fighter Collision overlaps, as Black Adam's field's hit. HitLocation of a null hit
/// box: the victim's capsule. IgnoreFacing: the vector's X is UE +Y, sim -x.
fn explode(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let none = mvs_data::value::Props::new();
    let p = actor_p(w, me);
    let base = obj(p, "Struct Ref").unwrap_or(&none);
    let rows = OnHitBuffRow::rows(&w.names, p.get("Explosion On Hit Buffs"));
    let d = obj(base, "KnockBackDirectionVector");
    let dir = V2 { x: -d.and_then(|d| num(d, "X")).unwrap_or(0.0), y: d.and_then(|d| num(d, "Y")).unwrap_or(0.0) };
    let packet = HitPacket::from_props(&w.names, base, None);
    let attack_id = zs(w, id).attack_id;
    let team = zone_team(w, id);
    for vic in overlap_fighters(w, id) {
        if w.fighter(vic).life.dead > 0.0 {
            continue;
        }
        let enemy = w.fighter(vic).team.team != team;
        let pkt = HitPacket { force_knockback: Some(enemy), ..packet.clone() }; // @824-906
        if enemy {
            if no_hit(w, vic) || projectile_blocks(w, &dealer(w, me), vic) {
                continue;
            }
            if invulnerable(w, vic) {
                let tick = w.flow.tick;
                attack_dodged(w, me, vic, attack_id, pkt.base_knock_back.unwrap_or(0.0), tick, None, false);
            } else {
                dodge_over(w, me, vic);
                let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                let dl = dealer(w, me);
                if apply_projectile_hit(w, &dl, vic, &pkt, &rows, at, Some(dir), &[], charge_pct(w, me)) {
                    let dl = dealer(w, me);
                    actor_hit_enemy(w, &dl, vic);
                }
            }
        } else if !no_hit(w, vic) && !invulnerable(w, vic) {
            let dl = dealer(w, me);
            ally_hit(w, &dl, vic, &pkt);
        }
    }
}

/// The pulse's lists (Buff_Elemental_Electric's ApplyOverlapElectric: Sphere's GetOverlappingActors of
/// BP_FighterCharacter, then of SpawnedActorParent), asked only on the tick its timer fires (as the fire wall).
fn pulse_due(w: &World, id: ActorId) -> bool {
    actor_elem(w, ActorRef::Actor(id))
        .and_then(|e| e.as_ref())
        .is_some_and(|s| s.buffs.iter().any(|b| b.timer == Some(1)))
}

fn zone_step(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    overlap_pass(w, id);
    if !pulse_due(w, id) {
        elem_tick(w, me, &[], &[]);
        return;
    }
    let (x, y, r) = (zs(w, id).x, zs(w, id).y, scaled_radius(w, id));
    // Unverified: a body of a class with an owner and a class (a spawned actor) is kept, not a fighter or the zone.
    let acts: Vec<ActorRef> = sphere_overlap_actors(w, x, y, r, &actor_types(), &[], OBJ_HIT_BOX)
        .into_iter()
        .filter(|a| *a != me && !matches!(a, ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_)))
        .collect();
    let fighters = overlap_fighters(w, id);
    elem_tick(w, me, &fighters, &acts);
}

/// The kind's `late`: a zone whose owner is no Garnet runs its FullPower here (no joined timer_tick).
fn zone_late(w: &mut World, id: ActorId) {
    let owner = zone_owner(w, id);
    if !zone_dead(w, id) && garnet_of(w, owner).is_none() && crate::real(w.flow.tick) == zs(w, id).full_power_at {
        full_power(w, id);
    }
}

fn zone_hazard_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    let Some(z) = w.actors.actors.get(id) else { return false };
    !z.dead && size2d([x - zs(w, id).x, y - zs(w, id).y]) <= scaled_radius(w, id) + r
}

fn zone_clean(w: &mut World, a: ActorRef) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    let z = zs_mut(w, id);
    z.destroyed_early = true; // MulticastCleaned @5281
    z.destroy_fx_blocked = true; // its BPI_SetDestroyEffectsBlocked(true) @5292
    w.actors.actors.get_mut(id).expect("a listed actor").dead = true; // DestroySpawnedActor(false): its EndPlay in the list pass
    true
}

/// zone.rs's part of the joined timerTick (before sing.rs's: the load order): the zones' FullPower timers, for the
/// zones this Garnet spawned (Comp_Spawner.SpawnedActors).
pub fn timer_tick(w: &mut World, f: FighterId) {
    let mut i = 0;
    while let Some(&a) = w.fighter(f).items.spawned_actors.get(i) {
        i += 1;
        let ActorRef::Actor(id) = a else { continue };
        let Some(z) = w.actors.actors.get(id) else { continue };
        if z.cls == ZONE && !z.dead && crate::real(w.flow.tick) == zs(w, id).full_power_at {
            full_power(w, id);
        }
    }
}

/// This file's registry entries (GarnetLightningBolt_C, GarnetElectricZone_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // foreign_owner: the bolt reads its owner as the Instigator alone (its spawner, the zone's team and attack count,
    // the class data through actor_def's peers), whoever that is.
    r.actors.enter(
        LIGHTNING_BOLT,
        ActorEntry {
            foreign_owner: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::GarnetLightningBolt),
                ActorHooks::Projectile(bolt_kind()),
            )
        },
    )?;
    let zone = WorldActorHooks {
        begin_play: Some(zone_begin),
        // Its shapes read `scaled_radius`; the view the root's scale.
        size: Some(SizeHooks::default()),
        // Collision and Sphere are of the HitBox object type (the traces gather none), the root Capsule NoCollision.
        bodies: Some(|_, _, _| {}),
        step: Some(zone_step),
        late: Some(zone_late),
        end: Some(zone_end),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        ELECTRIC_ZONE,
        ActorEntry {
            // its BPI_GetSphereCollision @0
            elemental: Some(ElementalParts { sphere: true, ..ElementalParts::default() }),
            hazard: Some(HazardHooks { overlaps: zone_hazard_overlaps, clean: zone_clean }),
            ..ActorEntry::new(ActorClass::World(WorldActorClass::GarnetElectricZone), ActorHooks::World(zone))
        },
    )
}
