//! Superman's signature perks: FireSlam (Buff_Superman_FireSlam_C,
//! Buff_Superman_FireSlamConditional_C, AnimNotify_SupermanFireSlam_C), IceBreaker (Buff_Superman_IceBreaker_C) and
//! SniperPunch (Buff_Superman_SniperPunch_C, whose multipliers aim_punch.rs sets).
//! Read: Buff_Superman_FireSlam, Buff_Superman_FireSlamConditional, Buff_Superman_IceBreaker,
//! Buff_Superman_SniperPunch; AnimNotify_SupermanFireSlam; SupermanAimPunch (IsSniper @9377-9579, ActivateObject's
//! block @1376-2652). The values are the perk table's (the superman perk part: the buffs' class defaults). All three
//! perks are self-only (SpawnOnAllies false).
//!
//! A Rust notify has the World, so none is kept per fighter for it.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{
    BuffExt, buff, buff_mut, buff_of, end_buff, inst_def_of, inst_view, set_buff_values, spawn_buff,
};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks, NotifyHandler};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

const FIRE_WALL: Name = name!("FireWallHazard_C");

/// `sniperBuff`: the owner's live SniperPunch buff (SupermanAimPunch's CheckHasBuff @9377 / @1390), for the marker.
pub fn sniper_buff(w: &World, f: FighterId) -> Option<BuffRef> {
    buff_of(w, f, name!("Buff_Superman_SniperPunch_C"))
}

// ---------------------------------------------------------------- Buff_Superman_FireSlam
// ReceiveBeginPlay @614 (authority): the Instigator's Comp_Spawner AfterSpawnedActor bound to AfterSpawnedActor
// @729-752 (the perk's buff is the owner's own: Instigator = owner, so the AfterSpawnedActor signal of the owner's
// spawns). AfterSpawnedActor @798: IsSoftRef(SpawnedActor, FireWallHazard_C) and the cast -> ActiveFireWall = it
// @1020.
// SpawnFireWall @510 (AnimNotify_SupermanFireSlam): the owner a Base_Fighter @530-595 -> @10: IsValid(ActiveFireWall)
// -> its K2_DestroyActor @53; then the Instigator's Comp_Spawner.CompSpawnActor(FireWallHazard_C, AttachToActor false,
// null, None, WorldTransforms true, Conv_VectorToTransform(L), (0, 0, 0), 0, 0, ...) @387, the call Firebolt's death
// spawn makes (`comp_spawn_projectile`), with L = the owner's root component's
// GetSocketLocation(Mesh.GetSocketBoneName(L_Hand)) @160-287.
// Unverified: the root is the Capsule, a primitive with no sockets: L is taken as its component location, the actor's
// (USceneComponent's GetSocketTransform for a name it has no socket of; not read in the exe). So the wall starts at
// his capsule's centre and falls to the floor.
// ext: `BuffExt::FireSlam { wall }`, ActiveFireWall as the wall's spawn count (`Projectile::seq`), -1 none.

fn spawn_fire_wall(w: &mut World, f: FighterId, b: BuffRef) {
    let wall = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::FireSlam { wall }) => *wall,
        _ => -1,
    };
    if wall >= 0 {
        let old = w.actors.projectiles.iter().find(|(_, p)| p.seq == wall).filter(|(_, p)| !p.dead).map(|(id, _)| id);
        if let Some(pr) = old {
            crate::world::spawning::destroy_projectile(w, pr, false);
        }
    }
    let (x, y) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
    let dir = [Some(0.0), Some(0.0), Some(0.0)];
    crate::world::spawning::comp_spawn_projectile(w, f, FIRE_WALL, x, y, dir, 0.0, 0.0, None);
}

fn fire_slam_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::FireSlam { wall: -1 };
    }
}

fn fire_slam_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    let Some(ActorRef::Projectile(pr)) = a.actor else { return false };
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    if p.cls != FIRE_WALL {
        return false;
    }
    let seq = p.seq;
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::FireSlam { wall } = &mut x.ext
    {
        *wall = seq;
    }
    false
}

/// AnimNotify_SupermanFireSlam (Superman_LeapAttack_Montage f176.484) Received_NotifyBegin @121-407: the owner's
/// Comp_State_Manager.CheckHasBuff(Buff_Superman_FireSlam_C) and, with the buff, its SpawnFireWall().
fn fire_slam_notify(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(b) = buff_of(w, f, name!("Buff_Superman_FireSlam_C")) {
        spawn_fire_wall(w, f, b);
    }
}

// Buff_Superman_FireSlamConditional, the OnHitBuffs entry of Superman_LeapAttack_Montage's landing hit (f171.8, 5 s).
// ReceiveBeginPlay @29 (authority): an Instigator that is no fighter @148, or whose Comp_State_Manager has no
// Buff_Superman_FireSlam_C @355 -> K2_DestroyActor @10; with it its Comp_State_Manager.SpawnBuffActor(Buff_Fire_C,
// the owner, the Instigator, 3, 0.0, true, false) @409 and K2_DestroyActor @515 -> @10 all the same. So the actor
// never outlives its BeginPlay: spawned, its status update out, and destroyed in the same call.
fn conditional_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    if let Some(from) = from
        && buff_of(w, from, name!("Buff_Superman_FireSlam_C")).is_some()
    {
        spawn_buff(w, f, name!("Buff_Fire_C"), 0.0, Some(3), Some(from), true, None);
    }
    end_buff(w, f, b);
}

// ---------------------------------------------------------------- Buff_Superman_IceBreaker
// DealtDamageModCondition @0-517 alone. The Victim cast to BP_FighterCharacter and its Comp_State_Manager valid
// (@0-159; else false @506, nothing written); CheckHasBuff(Buff_Ice_C) @224 -> DealtDamageMods.DamageMod =
// DamageBuffBase + BuffStacks x DamageBuffPerIceStack @354-446 (1.05 + 0.01 x stacks, written whether or not the
// victim has ice: 1.05) and ShouldApplyBuffMods = HasBuff @482. DamageModType is Multiply in the data. The pass reads
// the mods after the condition (`dealt_mods_over`), so the hit takes the value just written.
fn ice_breaker_dealt(
    w: &mut World,
    victim: ActorRef,
    _owner: FighterId,
    _dealer: FighterId,
    _hit: &HitPacket,
    b: BuffRef,
    _first: bool,
) -> bool {
    let ActorRef::Fighter(v) = victim else { return false }; // @0-65 -> @506: nothing written
    let ice = buff_of(w, v, name!("Buff_Ice_C"));
    let d = inst_view(w, b);
    let base = d.num("DamageBuffBase").unwrap_or(0.0);
    let per = d.num("DamageBuffPerIceStack").unwrap_or(0.0);
    let mut mods = d.obj("DealtDamageMods").cloned().unwrap_or_default();
    let n = ice.and_then(|i| buff(w, i)).map_or(0.0, |x| x.stacks);
    let m = base + (n * per);
    mods.insert("DamageMod".into(), Value::Num(f64::from(m)));
    let mut v = Props::new();
    v.insert("DealtDamageMods".into(), Value::Obj(mods));
    set_buff_values(w, b, &v);
    ice.is_some()
}

// ---------------------------------------------------------------- Buff_Superman_SniperPunch
// No BeginPlay of its own. SetMultipliersServer(DistancePercent) @752 (SupermanAimPunch ActivateObject @2652: the
// owner's buff found by CheckHasBuff): FClamp(DistancePercent, 0, 1) @752; SetMultiplierMulticast(Lerp(Low, High
// DistanceDamageMultiplier), Lerp(Low, High DistanceKnockbackMultiplier)) @909 -> @670: DealtDamageMods.DamageMod and
// KnockBackMod written (both Multiply in the data, read by the dealt pass); then @10: the owner (a Base_Fighter)
// Comp_Pawn_HitBoxHandler HitBoxesDeactivated bound to ResetMultipliersFromHitbox @109-132 and the mesh's
// AnimInstance OnMontageEnded to ResetMultipliersFromMontageEnd @259-282 (the HitBoxesDeactivated / MontageEnded
// signals). Either one -> @328: SetMultiplierMulticast(1, 1), then both unbound @451-624. Its
// DealtDamageModCondition is BuffParentActor's (true): every hit of the owner in between takes the two multipliers.
// C016AimPunchSlam (LeBron's) calls SetMultipliersServer too, on a LeBron that holds this buff: only Superman's perk
// gives it (self-only), so that call is never reached.

/// Lerp as the graph's: float32 `a + t * (b - a)`.
fn lerp(a: Real, b: Real, t: Real) -> Real {
    a + (t * (b - a))
}

fn set_multiplier_multicast(w: &mut World, b: BuffRef, dmg: Real, kb: Real) {
    let mut mods = inst_view(w, b).obj("DealtDamageMods").cloned().unwrap_or_default();
    mods.insert("DamageMod".into(), Value::Num(f64::from(dmg)));
    mods.insert("KnockBackMod".into(), Value::Num(f64::from(kb)));
    let mut v = Props::new();
    v.insert("DealtDamageMods".into(), Value::Obj(mods));
    set_buff_values(w, b, &v);
}

/// `setMultipliersServer(f, b, distancePercent)` (SupermanAimPunch ActivateObject @2652).
pub fn set_multipliers_server(w: &mut World, _f: FighterId, b: BuffRef, distance_percent: Real) {
    let (kb, dmg) = {
        let d = inst_def_of(w, b);
        let g = |k: &str| d.as_ref().and_then(|d| d.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
        let t = min(max(distance_percent, 0.0), 1.0);
        (
            lerp(
                crate::math::narrow(g("LowDistanceKnockbackMultiplier")),
                crate::math::narrow(g("HighDistanceKnockbackMultiplier")),
                t,
            ),
            lerp(
                crate::math::narrow(g("LowDistanceDamageMultiplier")),
                crate::math::narrow(g("HighDistanceDamageMultiplier")),
                t,
            ),
        )
    };
    set_multiplier_multicast(w, b, dmg, kb);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::SniperPunch { bound: true };
    }
}

fn sniper_on(w: &mut World, _f: FighterId, b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitBoxesDeactivated && s != BuffSignal::MontageEnded {
        return false;
    }
    if !matches!(buff(w, b).map(|x| &x.ext), Some(BuffExt::SniperPunch { bound: true })) {
        return false;
    }
    set_multiplier_multicast(w, b, 1.0, 1.0);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::SniperPunch { bound: false };
    }
    false
}

/// This file's registry entries.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_SupermanFireSlam_C",
        NotifyHandler { begin: Some(fire_slam_notify), ..NotifyHandler::default() },
    )?;
    let list = [
        BuffEntry::new(
            BuffClass::BuffSupermanFireSlam,
            BuffHooks { begin: Some(fire_slam_begin), on: Some(fire_slam_on), ..BuffHooks::default() },
        ),
        BuffEntry::new(
            BuffClass::BuffSupermanFireSlamConditional,
            BuffHooks { begin: Some(conditional_begin), ..BuffHooks::default() },
        ),
        BuffEntry::new(
            BuffClass::BuffSupermanIceBreaker,
            BuffHooks { dealt_cond: Some(ice_breaker_dealt), ..BuffHooks::default() },
        ),
        BuffEntry::new(BuffClass::BuffSupermanSniperPunch, BuffHooks { on: Some(sniper_on), ..BuffHooks::default() }),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
