//! Marvin's pawn core: Pawn_C018's tick (the wall-cling trace), the
//! disintegration self-damage timers, BPI_C018 (the functions the other files call on a Marvin), the conditions
//! Condition_C018CannonFireReady / CannonSpawned / CannonNotSpawned / DisintegrationHasHit / HasAimPerk / HasNoAimPerk,
//! C018MissilesPerkCondition / C018NoMissilesPerkCondition, and the notifies AnimNotify_C018SetAnimAimRotation,
//! _HasPerkSetAnimAimRotation, _StartCannonAiming. Its variables are `MarvinState::pawn`, `disint`, `aim_deg`,
//! `wall_cling_blend` and `ufo` (state.rs).
//!
//! Offsets are those of Pawn_C018's ubergraph unless a function is named. BeginPlay (@3175-3385): the parent's,
//! SkinApplied -> OnSkinApplied (the heart skin, a skin's: the view), and the AnimBP cache (@3249-3366): nothing for
//! the sim. ReceiveTick (@5018): the parent's, the wall-cling trace below, then the heart skin's opacity (@5973-6007:
//! a skin's, HeartSkinOpacityMax 0 on the others: nothing).
//!
//! Not here: the AI (BPI_ReadyForAI), the reversal (reverse.rs), the piloting (BPI_SetCannonAiming @1636 and the rest:
//! dormant), the warp helpers (@30-1256, @1796-2829: dormant), MulticastReverseProjectile (@3391-4226: no caller),
//! HeadScale / FindMirrorSocket (the view).

use super::state::{MarvinDisint, marvin, marvin_of, marvin_w};
use crate::Real;
use crate::actors::{ActorId, ActorRef, world_actor};
use crate::combat::buffs::buff_of;
use crate::combat::hit::apply_direct_damage;
use crate::consts::DELTA_SECONDS;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::actor_y;
use crate::fighter::anim::SlotMix;
use crate::fighter::common::{actor_is_a, lookup_owner, warn_text};
use crate::fighter::control::pawn_prop;
use crate::fighter::match_side::notify_props;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND_NO_PLATFORM, box_trace_single};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::props::{flag_or, num_or};
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Props;

// ---------------------------------------------------------------- the wall cling

/// ReceiveTick @5042-5927, while IsWallSliding (`wall.wall_sliding`): BoxTraceSingle(self, loc + (0, 0,
/// FlagInWallHeight), loc, half size (r + 10, r + 10, 0), no rotation, trace type 5 (ProjectileGroundNoPlatform), not
/// complex, ignoring self) @5316, r the root capsule's own CapsuleRadius (unscaled: `cap_radius_u`); then
/// WallClingFlagBlend = FClamp(ImpactPoint.Z - loc.Z, 0, FlagInWallHeight) / FlagInWallHeight @5757-5900 and
/// AnimBP.SetWallBlendSpaceValue(it) @5927 -> C018_AnimBP @3511-3553: Base_AnimBP_C018's WallBlendSpaceValue = 1 - it.
/// No hit leaves the hit result zeroed: ImpactPoint.Z 0 (the graph reads it without the bBlockingHit test).
/// The box: UE X (depth) and Y (sim x) r + 10, Z 0 (the shape adapter's 1e-4 floor).
fn wall_cling(w: &mut World, f: FighterId) {
    if !w.fighter(f).wall.wall_sliding {
        return; // @5042
    }
    let h = pawn_prop(w, f, "FlagInWallHeight").expect("Pawn_C018 FlagInWallHeight");
    let x = w.fighter(f).body.x;
    let y = actor_y(w, f);
    let r = w.fighter(f).capsule.cap_radius_u + 10.0; // @5155
    let hit = box_trace_single(
        w,
        x,
        y + h,
        x,
        y,
        r,
        0.0,
        r,
        TRACE_PROJECTILE_GROUND_NO_PLATFORM,
        &[],
        Some(ActorRef::Fighter(f)),
    );
    let iz = hit.map_or(0.0, |t| t.iy);
    let blend = min(max(iz - y, 0.0), h) / h;
    let s = marvin_w(w, f);
    s.wall_cling_blend = blend; // @5900
    s.pawn.wall_blend_space_value = 1.0 - blend; // @3511-3553
}

/// CharHooks.tick: the pawn's ReceiveTick after the parent's.
pub fn tick(w: &mut World, f: FighterId) {
    wall_cling(w, f);
}

// ---------------------------------------------------------------- the disintegration self-damage

// The two timers in the frame's timer pass (CharHooks.timer_tick; FTimerManager::Tick as `resetAimTimer` has it): set
// in the tick groups (the ray notify's begin / end, ray.rs) a timer is pending until this pass, where it goes active
// with ExpireTime = InternalTime + Rate; it fires on InternalTime > ExpireTime; the looping one is called CallCount
// times in a row and re-armed by ExpireTime += CallCount x Rate. Both are set by the same call, so their
// InternalTimes are equal: two due in one pass are taken by expire time (FTimerManager::Tick).
fn inc_rate(w: &World, f: FighterId) -> Real {
    pawn_prop(w, f, "DisintegrationDamageIncrementTime").expect("Pawn_C018 DisintegrationDamageIncrementTime")
}

/// TakeDisintegrationDamage @4807-4975: IsTakingDisintegrationDamage and not DisintegrationHasHit ->
/// ApplyDamage(self, DisintegrationDamageIncrement, GetController(), self) @4904 (`apply_direct_damage`:
/// Comp_Actor_Damage OnTakeAnyDamage; the causer is the pawn itself, so RecordDamage counts it in his own
/// DamageDealt: no self test there).
fn take_disintegration_damage(w: &mut World, f: FighterId) {
    let d = &marvin(w, f).disint;
    if !(d.taking && !d.has_hit) {
        return;
    }
    let amount = pawn_prop(w, f, "DisintegrationDamageIncrement").expect("Pawn_C018 DisintegrationDamageIncrement");
    apply_direct_damage(w, f, amount, Some(f));
}

fn fire_inc(w: &mut World, f: FighterId) {
    let rate = inc_rate(w, f);
    let d = &marvin(w, f).disint;
    let calls = ((d.inc_t - d.inc_at) / f64::from(rate)).trunc() + 1.0;
    let mut i = 0.0;
    while i < calls && marvin(w, f).disint.inc_timer == 2 {
        take_disintegration_damage(w, f);
        i += 1.0;
    }
    let d = &mut marvin_w(w, f).disint;
    if d.inc_timer == 2 {
        d.inc_at += f64::from(crate::math::narrow(calls * f64::from(rate)));
    }
}

/// EndDisintegrationDamageOnTimer @4976: IsTakingDisintegrationDamage -> MulticastEndDisintegrationDamage @4986 ->
/// @5001 BPI_SetDamageFromDisintegration(false, false) (both timers cleared).
fn fire_end(w: &mut World, f: FighterId) {
    let d = &mut marvin_w(w, f).disint;
    d.end_timer = 0; // a one-shot timer is done before its call
    if d.taking {
        set_damage_from_disintegration(w, f, false, false);
    }
}

/// pawn.rs's `timerTick` (`disintTimers`, the joined timerTick's only part).
pub fn timer_tick(w: &mut World, f: FighterId) {
    let rate = inc_rate(w, f);
    let d = &mut marvin_w(w, f).disint;
    let inc_on = d.inc_timer == 2;
    let end_on = d.end_timer == 2;
    if d.inc_timer == 1 {
        d.inc_timer = 2;
        d.inc_t = 0.0;
        d.inc_at = f64::from(rate);
    }
    if d.end_timer == 1 {
        d.end_timer = 2;
        d.end_t = 0.0;
        d.end_at = f64::from(d.end_rate);
    }
    if inc_on {
        d.inc_t += f64::from(DELTA_SECONDS);
    }
    if end_on {
        d.end_t += f64::from(DELTA_SECONDS);
    }
    let inc_due = inc_on && d.inc_t > d.inc_at;
    let end_due = end_on && d.end_t > d.end_at;
    let inc_first = inc_due && d.inc_at < d.end_at;
    if end_due && !inc_first {
        fire_end(w, f);
        if inc_due && marvin(w, f).disint.inc_timer == 2 {
            fire_inc(w, f);
        }
    } else {
        if inc_due {
            fire_inc(w, f);
        }
        if end_due && marvin(w, f).disint.end_timer == 2 {
            fire_end(w, f);
        }
    }
}

/// pawn.rs's `death` (the joined death's only part): the slot mix cleared (the port does not update a dead pawn's
/// AnimBP).
pub fn death(w: &mut World, f: FighterId) {
    marvin_w(w, f).pawn.slot_mix.slot_mix.clear();
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&marvin(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    Some(&mut marvin_w(w, f).pawn.slot_mix)
}

fn wall_blend_space_value(w: &World, f: FighterId) -> Real {
    marvin(w, f).pawn.wall_blend_space_value
}

/// The value hooks of `pawnHooks` in Marvin's CharHooks. C018_AnimBP's slot graph from the data (`fighter::anim`
/// graph_slots): the inputs (aim.rs `slot_input`, AimValue from its update) and the mix it writes
/// (`MarvinPawnState::slot_mix`, which the box sampler reads for the shapes; the wall blend space's rows go into it
/// too); Base_AnimBP_C018's WallBlendSpaceValue (the wall-cling trace's 1 - WallClingFlagBlend).
pub fn hooks(h: &mut CharHooks) {
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.slot_input = Some(super::aim::slot_input);
    h.wall_blend_space_value = Some(wall_blend_space_value);
}

// ---------------------------------------------------------------- BPI_C018

// The pawn's interface (BPI_C018; Pawn_C018's implementations at the offsets named) that the other files call on a
// Marvin (`marvin_of(w, f).is_some()`).

/// The classes whose BeginPlay writes EnableProjectileMovement = false (xref writes: C019Marker @5615, GarnetMarker2
/// @6170, GarnetGauntletsIdle @15); every other actor keeps its class default.
fn enable_projectile_movement(w: &World, owner: FighterId, cls: Name, p: &Props) -> bool {
    if !flag_or(p, "EnableProjectileMovement", false) {
        return false;
    }
    // MOVEMENT_OFF: C019Marker_C, GarnetMarker2_C, GarnetGauntletsIdle_C
    let own = ActorRef::Fighter(owner);
    ![name!("C019Marker_C"), name!("GarnetMarker2_C"), name!("GarnetGauntletsIdle_C")]
        .into_iter()
        .any(|c| actor_is_a(w, own, cls, c) == Some(true))
}

/// Enum_ProjectileType 0 Normal, 1 Heavy, 2 Light @169-365.
fn type_ok(t: Real) -> bool {
    t == 0.0 || t == 1.0 || t == 2.0
}

/// The `hit_on` a World.actors kind keeps as its own variable (as Item and the kinds below do); `None` for a kind
/// without one.
fn own_hit_on(w: &World, id: ActorId) -> Option<bool> {
    use world_actor::WorldActorState as S;
    Some(match &w.actors.actors.get(id)?.state {
        S::DisguiseShock(s) => s.hit_on,
        S::HighFiveShockwave(s) => s.hit_on,
        S::GemBoostBlast(s) => s.hit_on,
        S::ShockwaveHitBox(s) => s.hit_on,
        S::AirFlap(s) => s.hit_on,
        S::LaserDamage(s) => s.hit_on,
        S::Dogpile(s) => s.hit_on,
        S::JerryActor(s) => s.hit_on,
        _ => return None,
    })
}

/// `isViableForProjectileTweaks` (BPI_IsViableForProjectileTweaks(Actor), its own function @0-634): the cast to
/// SpawnedActorParent (@0-65; false otherwise @623), then IsViable = HitBoxOn? and (BPI_GetEnableProjectileMovement
/// or (an ItemActorParent and IsThrown)) and its BPI_GetProjectileAttackType is 0, 1 or 2 (@79-599). Callers: the
/// bubble (bubble.rs), the flag's homing (homing.rs), the reversal (ReverseProjectile @0, reverse.rs).
/// - A Projectile (every one a SpawnedActorParent): HitBoxOn? the variable (`hit_var`), its attack type,
///   EnableProjectileMovement its class default unless its class writes it false at BeginPlay (the three classes of `enable_projectile_movement`).
/// - An Item (ItemActorParent, a SpawnedActorParent child): its `hit_on`, IsThrown (`thrown`), the class default of
///   EnableProjectileMovement, its attack type.
/// - A World.actors entry: the cast by the class chain of the owner's data (none: a warning, false); HitBoxOn? the
///   actor's own `hit_on` where its kind keeps the variable (as Item and the kinds of `own_hit_on` do), else its class
///   default; its class default ProjectileAttackType.
// Unverified: a World.actors kind that writes HitBoxOn? keeps it as `hit_on` or this reads the class default; an
// ItemActorParent there (none in the port) reads IsThrown false.
pub fn is_viable_for_projectile_tweaks(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get(id) else { return false };
            let owner = lookup_owner(w, a);
            p.hit_var
                && enable_projectile_movement(w, owner, p.cls, crate::actors::projectile::class_p(w, p))
                && type_ok(crate::real(p.attack_type))
        }
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get(id) else { return false };
            let p = crate::actors::item::ItemDef::at(&w.data, it.src).p;
            it.hit_on
                && (flag_or(p, "EnableProjectileMovement", false) || it.thrown)
                && type_ok(crate::real(crate::actors::item::item_attack_type(w, id)))
        }
        _ => {
            let (Some(cls), Some(owner)) = (world_actor::actor_cls(w, a), world_actor::actor_owner(w, a)) else {
                return false;
            };
            let Some(sap) = actor_is_a(w, a, cls, name!("SpawnedActorParent_C")) else {
                warn_text(
                    w,
                    &format!(
                        "marvin BPI_IsViableForProjectileTweaks: no class chain in the data for {}: not viable",
                        w.names.str(cls)
                    ),
                );
                return false;
            };
            if !sap {
                return false;
            }
            let p = world_actor::actor_p(w, a);
            let own = match a {
                ActorRef::Actor(id) => own_hit_on(w, id),
                _ => None,
            };
            let hit_on = own.unwrap_or_else(|| flag_or(p, "HitBoxOn?", false));
            if actor_is_a(w, a, cls, name!("ItemActorParent_C")) == Some(true) {
                warn_text(
                    w,
                    &format!(
                        "marvin BPI_IsViableForProjectileTweaks: {} is an ItemActorParent in World.actors: IsThrown read as false",
                        w.names.str(cls)
                    ),
                );
            }
            let t = num_or(p, "ProjectileAttackType", 0.0);
            hit_on && enable_projectile_movement(w, owner, cls, p) && type_ok(t)
        }
    }
}

/// `setDisintegrationHit` (BPI_SetDisintegrationHit(Value) @2931 -> @2834-2864: authority,
/// MulticastDisintegrationHasHit -> @4787 DisintegrationHasHit = Value). Called by ray.rs.
pub fn set_disintegration_hit(w: &mut World, f: FighterId, v: bool) {
    marvin_w(w, f).disint.has_hit = v;
}

/// `setDamageFromDisintegration` (BPI_SetDamageFromDisintegration(TakeDamage, InAir) @2936-3003): with TakeDamage !=
/// IsTakingDisintegrationDamage (@2974), IsTakingDisintegrationDamage = TakeDamage and @1261 (authority): taking ->
/// the looping TakeDisintegrationDamage timer of DisintegrationDamageIncrementTime @1328 and the one-shot
/// EndDisintegrationDamageOnTimer timer of SelectFloat(DisintegrationDamageMaxDurationAir, ...Ground, InAir) @1490;
/// not taking -> both handles cleared @1575-1595. Called by ray.rs (its notify).
pub fn set_damage_from_disintegration(w: &mut World, f: FighterId, take: bool, in_air: bool) {
    if take == marvin(w, f).disint.taking {
        return;
    }
    let rate = if take {
        let k = if in_air { "DisintegrationDamageMaxDurationAir" } else { "DisintegrationDamageMaxDurationGround" };
        Some(pawn_prop(w, f, k).unwrap_or_else(|| panic!("Pawn_C018 {k}")))
    } else {
        None
    };
    let d: &mut MarvinDisint = &mut marvin_w(w, f).disint;
    d.taking = take;
    if let Some(r) = rate {
        d.inc_timer = 1;
        d.end_timer = 1;
        d.end_rate = r;
    } else {
        d.inc_timer = 0;
        d.end_timer = 0;
    }
}

/// `disintegrationHasHit` (BPI_DisintegrationHasHit: DisintegrationHasHit or not IsTakingDisintegrationDamage @0-67).
pub fn disintegration_has_hit(w: &World, f: FighterId) -> bool {
    let d = &marvin(w, f).disint;
    d.has_hit || !d.taking
}

/// `setCannon` (BPI_SetCannon(Cannon) @1616, the Cannon variable): the live UFO, `None` at its end. Called by ufo.rs
/// (the UFO's BeginPlay @11515; and perks.rs's platform, through ufo.rs).
pub fn set_cannon(w: &mut World, f: FighterId, ufo: Option<ActorRef>) {
    marvin_w(w, f).ufo = ufo;
}

/// `setAnimAimRotation` (BPI_SetAnimAimRotation(Degrees) @2903, Anim Aim Rotation Degrees). Callers: the aim rotation
/// notifies (below).
pub fn set_anim_aim_rotation(w: &mut World, f: FighterId, deg: Real) {
    marvin_w(w, f).aim_deg = deg;
}

/// `getAnimAimRotation` (BPI_GetAnimAimRotation: Anim Aim Rotation Degrees).
pub fn get_anim_aim_rotation(w: &World, f: FighterId) -> Real {
    marvin(w, f).aim_deg
}

/// IsValid(Cannon): the actor lives (a destroyed one is pending kill).
fn live_cannon(w: &World, f: FighterId) -> Option<ActorRef> {
    marvin_of(w, f)?.ufo.filter(|&u| !world_actor::actor_dead(w, u))
}

/// BPI_IsCannonReadyToFire @0-273: IsValid(Cannon), the cast to C018_Platform_C (C018_PerkPlatform_C, its child,
/// passes), CurrentCannonAmmo > 0 and ReadyToFire (@122-238); else false.
fn cannon_ready_to_fire(w: &World, f: FighterId) -> bool {
    let Some(u) = live_cannon(w, f) else { return false };
    let Some(cls) = world_actor::actor_cls(w, u) else { return false };
    if actor_is_a(w, ActorRef::Fighter(f), cls, name!("C018_Platform_C")) != Some(true) {
        return false;
    }
    super::ufo::ready_to_fire(w, u)
}

// ---------------------------------------------------------------- the conditions

// Condition_C018*, C018MissilesPerkCondition, C018NoMissilesPerkCondition (Check):
// - CannonFireReady / CannonSpawned: DoesImplementInterface(BPI_C018), then BPI_IsCannonReadyToFire /
//   BPI_IsCannonSpawned (IsValid(Cannon) @0-48); false for another actor. CannonNotSpawned: not its parent's
//   (Condition_C018CannonSpawned) Check, so true for another actor.
// - DisintegrationHasHit: the cast to BPI_C018, BPI_DisintegrationHasHit; false otherwise.
// - HasAimPerk: the cast to BPI_BaseFighter (every fighter), BPI_CheckIfHasBuff(Buff_C018_Aim_C).hasBuff; HasNoAimPerk:
//   not its parent's Check. MissilesPerkCondition / NoMissilesPerkCondition: the same with Buff_C018_Missiles_C, the
//   second negated in its own Check (@230).
// BPI_CheckIfHasBuff: a live buff of the class or a child (`buff_of`: the class; neither has a child).

fn has_buff(w: &World, f: FighterId, cls: Name) -> bool {
    buff_of(w, f, cls).is_some()
}

/// Condition_C018CannonFireReady_C (BPI_IsCannonReadyToFire: ufo.rs [`super::ufo::ready_to_fire`]).
fn cannon_fire_ready(w: &mut World, f: FighterId) -> bool {
    cannon_ready_to_fire(w, f)
}

/// Condition_C018CannonSpawned_C.
fn cannon_spawned(w: &mut World, f: FighterId) -> bool {
    live_cannon(w, f).is_some()
}

/// Condition_C018CannonNotSpawned_C.
fn cannon_not_spawned(w: &mut World, f: FighterId) -> bool {
    live_cannon(w, f).is_none()
}

/// Condition_C018DisintegrationHasHit_C.
fn disintegration_hit_cond(w: &mut World, f: FighterId) -> bool {
    marvin_of(w, f).is_some() && disintegration_has_hit(w, f)
}

/// Condition_C018HasAimPerk_C.
fn has_aim_perk(w: &mut World, f: FighterId) -> bool {
    has_buff(w, f, name!("Buff_C018_Aim_C"))
}

/// Condition_C018HasNoAimPerk_C.
fn has_no_aim_perk(w: &mut World, f: FighterId) -> bool {
    !has_buff(w, f, name!("Buff_C018_Aim_C"))
}

/// C018MissilesPerkCondition_C.
fn missiles_perk(w: &mut World, f: FighterId) -> bool {
    has_buff(w, f, name!("Buff_C018_Missiles_C"))
}

/// C018NoMissilesPerkCondition_C.
fn no_missiles_perk(w: &mut World, f: FighterId) -> bool {
    !has_buff(w, f, name!("Buff_C018_Missiles_C"))
}

// ---------------------------------------------------------------- the notifies

// AnimNotify_C018SetAnimAimRotation: Received_NotifyBegin, the mesh owner cast to BPI_C018 (another owner, a Disguise's
// thief playing his copied montage: nothing) -> BPI_SetAnimAimRotation(Rotation Degrees) @121; Received_NotifyEnd ->
// BPI_SetAnimAimRotation(0) @121. AnimNotify_C018HasPerkSetAnimAimRotation (its child): begin, the owner's
// BPI_CheckIfHasBuff(Buff_C018_Aim_C) @121 -> the parent's begin @276, else the BPI_C018 cast and
// BPI_SetAnimAimRotation(0) @518; end: the parent's end with the buff @276, else BPI_SetAnimAimRotation(0) @509: 0
// either way. AnimNotify_C018StartCannonAiming (C018FireCannon_Air / FireCannon): begin / end,
// DoesImplementInterface(BPI_C018) -> BPI_SetCannonAiming(true / false) -> Pawn_C018 @1636: IsValid(Cannon), the cast
// to C018Cannon_C, SetAiming. The Cannon is the UFO (C018_Platform, BPI_SetCannon @11515) or nothing: C018Cannon is
// spawned only by the dormant SpawnCannon montage, whose spawn the port does not make, so the cast fails and nothing
// happens. An entry that does nothing, by that reason (not a default).

fn rotation_degrees(w: &World, f: FighterId, n: NotifyRef) -> Real {
    num_or(notify_props(w, f, n), "Rotation Degrees", 0.0)
}

/// AnimNotify_C018SetAnimAimRotation_C's begin.
fn set_aim_begin(w: &mut World, f: FighterId, n: NotifyRef, _i: i32) {
    if marvin_of(w, f).is_some() {
        let d = rotation_degrees(w, f, n);
        set_anim_aim_rotation(w, f, d);
    }
}

/// AnimNotify_C018SetAnimAimRotation_C's end.
fn set_aim_end(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if marvin_of(w, f).is_some() {
        set_anim_aim_rotation(w, f, 0.0);
    }
}

/// AnimNotify_C018HasPerkSetAnimAimRotation_C's begin.
fn perk_aim_begin(w: &mut World, f: FighterId, n: NotifyRef, _i: i32) {
    if marvin_of(w, f).is_some() {
        let d = if has_buff(w, f, name!("Buff_C018_Aim_C")) { rotation_degrees(w, f, n) } else { 0.0 };
        set_anim_aim_rotation(w, f, d);
    }
}

/// AnimNotify_C018HasPerkSetAnimAimRotation_C's end.
fn perk_aim_end(w: &mut World, f: FighterId, _n: NotifyRef, _i: i32) {
    if marvin_of(w, f).is_some() {
        set_anim_aim_rotation(w, f, 0.0);
    }
}

/// AnimNotify_C018StartCannonAiming_C's begin and end: nothing (empty handlers).
fn nothing(_w: &mut World, _f: FighterId, _n: NotifyRef, _i: i32) {}

/// This file's registry entries: the eight conditions and the three notifies.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("Condition_C018CannonFireReady_C", Condition::Fn(cannon_fire_ready))?;
    r.conditions.enter("Condition_C018CannonSpawned_C", Condition::Fn(cannon_spawned))?;
    r.conditions.enter("Condition_C018CannonNotSpawned_C", Condition::Fn(cannon_not_spawned))?;
    r.conditions.enter("Condition_C018DisintegrationHasHit_C", Condition::Fn(disintegration_hit_cond))?;
    r.conditions.enter("Condition_C018HasAimPerk_C", Condition::Fn(has_aim_perk))?;
    r.conditions.enter("Condition_C018HasNoAimPerk_C", Condition::Fn(has_no_aim_perk))?;
    r.conditions.enter("C018MissilesPerkCondition_C", Condition::Fn(missiles_perk))?;
    r.conditions.enter("C018NoMissilesPerkCondition_C", Condition::Fn(no_missiles_perk))?;
    r.notifies.enter(
        "AnimNotify_C018SetAnimAimRotation_C",
        NotifyHandler { begin: Some(set_aim_begin), end: Some(set_aim_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C018HasPerkSetAnimAimRotation_C",
        NotifyHandler { begin: Some(perk_aim_begin), end: Some(perk_aim_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C018StartCannonAiming_C",
        NotifyHandler { begin: Some(nothing), end: Some(nothing), ..NotifyHandler::default() },
    )
}
