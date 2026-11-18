//! The anim notify classes of items and spawned actors, entered by name:
//! AnimNotify_Spawner / ConditionalSpawner, AnimNotify_ActivateSpawned (Comp_Spawner) and the item pick-up, holster,
//! aim and throw notifies.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::DataVec;
use crate::combat::buffs::turn_off_hit_boxes;
use crate::combat::disguise::{copy_running, copy_spawn_point, disguise_mesh_read};
use crate::combat::elem::instigator_charge;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::{conds_ok, conds_reset_ok, notify_data};
use crate::fighter::common::warn_once;
use crate::fighter::match_side::{
    ActivateMode, ActivateRequest, Aimer, DespawnRequest, Foreign, SpawnRequest, ThrowAimInfo, activate_aimer,
    face_towards, face_towards_end, reset_aim, teleport_to_ally, throw_item,
};
use crate::fighter::moves::V2;
use crate::name;
use crate::names::Name;
use crate::registry::props::{arr, flag, flag_or, name, num, num_or, obj, text};
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::NotifyTick;
use mvs_data::value::{Props, Value};

/// `newAimer`: a ProjectileAimer of the fighter, every field from the start.
pub fn new_aimer(n: NotifyRef, attack_id: i32, sy: Real, sz: Real, throw_offset: Option<DataVec>) -> Aimer {
    Aimer { n, attack_id, sy, sz, ay: sy, az: sz, throw_offset, fl: None, pm: None, info: None }
}

/// A notify's `Conditions` (or another list of condition classes) as `condsOk` takes it: `None` when absent, an
/// entry that is no class name `None`.
pub fn cond_list(w: &World, p: &Props, key: &str) -> Option<Vec<Option<Name>>> {
    let v = p.get(key)?.as_arr()?;
    Some(v.iter().map(|c| c.as_str().and_then(|s| w.names.get(s))).collect())
}

/// `spawnerDespawn`: All (DestroyAllActorsOnNotifyEnd), else Last / First.
pub fn despawn_request(w: &World, p: &Props) -> Option<DespawnRequest> {
    let cls = text(p, "ActorClass").map_or(Name::NONE, |s| name(w, s));
    if flag(p, "DestroyAllActorsOnNotifyEnd") {
        return Some(DespawnRequest::All(cls));
    }
    let (last, first) = (flag(p, "DestroyLastActorsOnNotifyEnd"), flag(p, "DestroyFirstActorsOnNotifyEnd"));
    (last || first).then_some(DespawnRequest::Ends { cls, last, first })
}

/// A real class name of the data (not absent, not `None`).
fn class_named<'a>(p: &'a Props, key: &str) -> Option<&'a str> {
    text(p, key).filter(|s| !s.is_empty() && *s != "None")
}

/// The fighter's facing as the aimer's StartAimVector Y: (0, FacingLeft ? Y : -Y, Z).
fn aim_y(w: &World, f: FighterId, y: Real) -> Real {
    if crate::fighter::facing(w, f) < 0.0 { y } else { -y }
}

/// AnimNotify_Spawner and AnimNotify_ConditionalSpawner Received_NotifyBegin.
fn spawner_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let conds = cond_list(w, p, "Conditions");
    if !conds_ok(w, f, conds.as_deref(), false) {
        return;
    }
    // AnimNotify_Spawner @760-830: the owner's Comp_State_Manager TurnOffHitboxCheck true ends the begin (no spawn, no
    // aimer, no AddSpawnerNotify), after the facing and attack count reads.
    if !w.fighter(f).buffs.buffs.is_empty() && turn_off_hit_boxes(w, f) {
        return;
    }
    // the pawn's own (Shaggy: the thrown sandwich leaves the hand)
    if let Some(h) = w.fighter(f).hooks.char.spawner {
        h(w, f, n);
    }
    if flag(p, "AimOn") {
        // AnimNotify_Spawner 2373: CompSpawnActor(ProjectileAimer_C) attached to the owner instead of the actor. The
        // aimer's AimInfo event (15393): StartAimVector = (0, FacingLeft ? Y : -Y, Z) of StartAimDirection (not
        // normalised), ArrowAimVector = StartAimVector (AimType 0 to 3, @14813-15060). Its place is read on the skill
        // shot path alone (AimType 1, @5527-9367), so it is not kept. AimType 1 is not ported: refused here, no aimer.
        // Nothing in the loaded data has it; an AimType past 3 is no type of Enum_AimTypes.
        let info = obj(p, "AimInfo");
        let ty = info.and_then(|i| num(i, "AimType")).unwrap_or(0.0);
        if ty != 0.0 && ty != 2.0 && ty != 3.0 {
            warn_once(w, name!("ProjectileAimer AimType is not ported"));
            return;
        }
        let s = info.and_then(|i| obj(i, "StartAimDirection"));
        let sy = aim_y(w, f, s.and_then(|s| num(s, "Y")).unwrap_or(0.0));
        let sz = s.and_then(|s| num(s, "Z")).unwrap_or(0.0);
        let lock = info.is_some_and(|i| flag(i, "LockAimDirection"));
        let facing_left = crate::fighter::facing(w, f) < 0.0;
        let attack = w.fighter(f).decay.attack_count;
        let fi = w.fighter_mut(f);
        // A second AimOn spawner while one is open (AnimNotify_Spawner @2275-2387): StoredAimActor and StoredAimInfo
        // are overwritten and another aimer is spawned; the first one lives on.
        if fi.items.aimer.as_ref().is_some_and(|a| a.throw_offset.is_none()) {
            let under = fi.items.aimer_under.is_some();
            fi.items.aimer_under = fi.items.aimer.take();
            if under {
                warn_once(w, name!("more than two ProjectileAimers at once"));
            }
        }
        let fi = w.fighter_mut(f);
        let mut a = new_aimer(n, attack, sy, sz, None);
        if lock {
            a.fl = Some(facing_left); // ProjectileAimer SetAimInfoMulticast @15346-15369
        }
        fi.items.aimer = Some(a);
        fi.pose.aim_direction = 0.0; // ProjectileAimer ReceiveBeginPlay (ubergraph 39-545)
        return;
    }
    // A fighter with a Disguise. Comp_Spawner @651-720: BPI_CheckIsDisguised gives the mesh the socket is read on,
    // for any Spawner of the fighter: the second mesh for the length of a copied neutral, the fighter's Mesh
    // otherwise. The class is the victim's when the notify is of the second mesh's montage.
    let cls = text(p, "ActorClass").map_or(Name::NONE, |s| name(w, s));
    let attack_id = w.fighter(f).decay.attack_count;
    let charge = instigator_charge(w, f);
    let mut r = SpawnRequest {
        cls,
        n,
        attack_id,
        foreign: Foreign::None,
        copy: None,
        charge,
        aim: None,
        props: None,
        instigator: None,
        origin: None,
    };
    if w.fighter(f).shapes.disguise.is_some() {
        if copy_running(w, f) {
            r.foreign = Foreign::Disguise;
        }
        if disguise_mesh_read(w, f) {
            r.copy = Some(copy_spawn_point(w, f, p));
        }
    }
    w.fighter_mut(f).requests.spawn_requests.push(r);
}

fn spawner_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let nd = notify_data(w, &data, f, n);
    let p = &nd.p;
    // Received_NotifyEnd: DestroyAllActorsOnNotifyEnd -> Comp_Spawner.DestroySpawnedActors(ActorClass). With AimOn
    // DestroySpawnedActors(ProjectileAimer_C, false, true, false) (@261): the last aimer alone (its EndPlay:
    // Comp_Spawner.ResetAim).
    if flag(p, "AimOn") && w.fighter(f).items.aimer.is_some() {
        let it = &mut w.fighter_mut(f).items;
        it.aimer = it.aimer_under.take();
        reset_aim(w, f);
    }
    // DestroySpawnedActors(ActorClass, ...) @309: with a valid class Comp_Spawner's @15 filters
    // SpawnedActors_ClassChecked (CharHooks.spawnerFiltered), whatever the three flags say.
    if let Some(c) = class_named(p, "ActorClass")
        && let Some(h) = w.fighter(f).hooks.char.spawner_filtered
    {
        h(w, f, name(w, c));
    }
    // All, Last or First
    let Some(r) = despawn_request(w, p) else { return };
    let fi = w.fighter_mut(f);
    if let Some(ends) = fi.montages.fade_ends.as_mut()
        && nd.tick == NotifyTick::BranchingPoint
    {
        ends.push(n);
    } else {
        fi.requests.despawn_requests.push(r);
    }
}

/// Received_NotifyBegin (authority): Comp_ItemManager.PickUpClosestItem.
fn pick_up_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).items.pickup_request = true;
}

/// Received_NotifyBegin: Comp_ItemManager.HolsterItem(!UnholsterBeginNotify?), which only acts with a held item
/// (IsHolstered is cleared otherwise).
fn holster_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let fi = w.fighter_mut(f);
    fi.items.item_holstered = fi.items.held_item.is_some() && !flag(p, "UnholsterBeginNotify?");
}

/// Received_NotifyEnd: UnholsterEndNotify? (class default true) -> HolsterItem(false).
fn holster_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    if flag_or(&notify_data(w, &data, f, n).p, "UnholsterEndNotify?", true) {
        w.fighter_mut(f).items.item_holstered = false;
    }
}

/// Received_NotifyBegin: Comp_Spawner.StoredAimInfo from the notify (AimType 0) and CompSpawnActor(ProjectileAimer_C):
/// StartAimVector = (0, FacingLeft ? Y : -Y, Z) of 'Start Aim Direction' (not normalised), as the Spawner notify's
/// AimOn. The class defaults stand in for what a montage leaves out (Aim Angle Max 45, Min -45, Aim Speed 1000): the
/// aimer's AimInfo is the notify's 'Aim Angle Max', 'Aim Angle Min', 'Aim Speed' and 'Input Rotation Offset'.
fn aim_throw_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let s = obj(p, "Start Aim Direction");
    let sy = aim_y(w, f, s.and_then(|s| num(s, "Y")).unwrap_or(0.0));
    let sz = s.and_then(|s| num(s, "Z")).unwrap_or(0.0);
    let off = obj(p, "ThrowStartOffset");
    let offset = DataVec {
        x: Some(off.map_or(0.0, |o| num(o, "X").unwrap_or(Real::NAN))),
        y: Some(off.map_or(0.0, |o| num(o, "Y").unwrap_or(Real::NAN))),
        z: None,
    };
    let info = ThrowAimInfo {
        max: num_or(p, "Aim Angle Max", 45.0),
        min: num_or(p, "Aim Angle Min", -45.0),
        speed: num_or(p, "Aim Speed", 1000.0),
        input_rotation: num_or(p, "Input Rotation Offset", 0.0),
    };
    let attack = w.fighter(f).decay.attack_count;
    let fi = w.fighter_mut(f);
    let mut a = new_aimer(n, attack, sy, sz, Some(offset));
    a.info = Some(info);
    fi.items.aimer = Some(a);
    fi.pose.aim_direction = 0.0;
}

/// Received_NotifyEnd (also when the montage is interrupted): with a ProjectileAimer, ThrowItemMulticast(the arrow
/// mirrored for the facing, true, ThrowStartOffset x the size scale (1)), then the aimer is destroyed. ThrowItem
/// mirrors the direction back: the throw goes along the arrow.
fn aim_throw_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    let it = &w.fighter(f).items;
    let throw = it.aimer.as_ref().and_then(|a| Some((a.ay, a.az, a.throw_offset?)));
    if it.aimer.is_some() || it.aimer_under.is_some() {
        reset_aim(w, f); // the aimer's EndPlay
    }
    let it = &mut w.fighter_mut(f).items;
    it.aimer = None;
    it.aimer_under = None;
    if let Some((ay, az, o)) = throw {
        let off = V2 { x: o.x.unwrap_or(Real::NAN), y: o.y.unwrap_or(Real::NAN) };
        throw_item(w, f, -ay, az, Some(off));
    }
    // DestroySpawnedActors(ProjectileAimer_C) @1829 refilters SpawnedActors_ClassChecked (Comp_Spawner @5301 -> @15)
    if let Some(h) = w.fighter(f).hooks.char.spawner_filtered {
        h(w, f, name!("ProjectileAimer_C"));
    }
}

/// Received_NotifyBegin: Comp_ItemManager.ThrowItem(ThrowDirection, false, (0, 0)). Morty's drop montage leaves the
/// class default direction (0, 0, 0): a drop where the item is.
fn item_throw_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    let d = obj(p, "ThrowDirection");
    if d.is_some_and(|d| flag(d, "Y") || flag(d, "Z")) {
        warn_once(w, name!("AnimNotify_ItemThrow with a direction is not ported"));
    }
    throw_item(w, f, 0.0, 0.0, None);
}

fn nearby_buff_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    w.fighter_mut(f).requests.nearby_buff_requests.push(n);
}

/// AnimNotify_FaceTowards: the begin turns the owner to its target, the end clears the mesh's relative rotation.
fn face_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    face_towards(w, f, n);
}

fn face_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    face_towards_end(w, f);
}

/// AnimNotify_ActivateTeleportToAlly Received_NotifyBegin: the owner's CompTeleportToAlly component's TeleportToAlly
/// (its NotifyEnd does nothing).
fn teleport_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    teleport_to_ally(w, f);
}

/// AnimNotify_ConditionalSpawner Received_NotifyEnd (@219-288): BPI_CheckConditions(Conditions) again, with the
/// state of this moment, and only then the Spawner's NotifyEnd. Each Check runs (up to the first false one).
fn cond_spawner_end(w: &mut World, f: FighterId, n: NotifyRef, idx: i32) {
    let data = w.data.clone();
    let conds = cond_list(w, &notify_data(w, &data, f, n).p, "Conditions");
    if !conds_ok(w, f, conds.as_deref(), false) {
        return;
    }
    spawner_end(w, f, n, idx);
}

/// `p.Conditions?.length && !condsResetOk(f, p.Conditions)`.
fn reset_conds_fail(w: &mut World, f: FighterId, p: &Props) -> bool {
    let conds = cond_list(w, p, "Conditions");
    conds.as_ref().is_some_and(|c| !c.is_empty()) && !conds_reset_ok(w, f, conds.as_deref())
}

/// AnimNotify_ActivateSpawned, a notify state. Received_NotifyBegin @5-791: with Conditions, each one's Reset and
/// Check; a false one ends the notify. Then ActivateOnEnd? @891 returns with no activation; else the owner's
/// Comp_Spawner.ActivateActors @1321. Received_NotifyEnd: an instance without ActivateOnEnd? does nothing at its end,
/// one with it activates at the end alone, after the Conditions' Reset and Check.
fn activate_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if reset_conds_fail(w, f, p) || flag(p, "ActivateOnEnd?") {
        return;
    }
    activate_spawned(w, f, p);
}

fn activate_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    let data = w.data.clone();
    let p = &notify_data(w, &data, f, n).p;
    if !flag(p, "ActivateOnEnd?") || reset_conds_fail(w, f, p) {
        return;
    }
    activate_spawned(w, f, p);
}

/// `ActivationConditions`: ConditionNumber, ConditionValue.
fn activation_conds(p: &Props) -> Vec<(i32, bool)> {
    arr(p, "ActivationConditions")
        .iter()
        .filter_map(Value::as_obj)
        .map(|c| (crate::math::to_int32(num(c, "ConditionNumber").unwrap_or(0.0)), flag(c, "ConditionValue")))
        .collect()
}

/// Comp_Spawner.ActivateActors (ubergraph 5237, 4005-4287): SetChargeVariables, then BPI_ActivateObject on the actors
/// "Get Spawned Actor" returns for SpawnedActorClass (@159-791): with ActivateAllObjects every one of the class,
/// whatever the other two say; else with Last and First the first and the last; else with Last the last; else (First
/// alone) the first.
fn activate_spawned(w: &mut World, f: FighterId, p: &Props) {
    let cls = text(p, "SpawnedActorClass");
    if cls == Some("ProjectileAimer_C") {
        activate_aimer(w, f);
        return;
    }
    let (all, last, first) =
        (flag(p, "ActivateAllObjects"), flag(p, "ActivateLastObject"), flag(p, "ActivateFirstObject"));
    // Not ported, in no montage of the data: none of the three set (@235 returns with the output untouched).
    if !all && !last && !first {
        warn_once(w, name!("ActivateSpawned with none of All / Last / First is not ported"));
    } else if let Some(c) = class_named(p, "SpawnedActorClass") {
        let c = name(w, c);
        // Get Spawned Actor @43-132 (SpawnedActors_ClassChecked)
        if let Some(h) = w.fighter(f).hooks.char.spawner_filtered {
            h(w, f, c);
        }
        let mode = if all {
            ActivateMode::All
        } else if last && first {
            ActivateMode::FirstLast
        } else if last {
            ActivateMode::Last
        } else {
            ActivateMode::First
        };
        // UpdateCharge: SetChargeVariables(the actor, the owner) @3727 ahead of each BPI_ActivateObject, with the
        // owner's values of this call. ActivationConditions: ActivateActors @3285.
        let charge = if flag(p, "UpdateCharge") { instigator_charge(w, f) } else { None };
        let conds = activation_conds(p);
        w.fighter_mut(f).requests.activate_requests.push(ActivateRequest { cls: c, mode, conds, charge, only: None });
    } else {
        activate_unfiltered(w, f, p);
    }
}

/// One entry of the unfiltered SpawnedActors list: an actor of the fighter's list, or one of its open aimers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Listed {
    Actor(ActorRef),
    AimerUnder,
    Aimer,
}

/// ActivateActors with no SpawnedActorClass (C018_Air_NormalNeutral_Montage 0.5451, Garnet_Attack_Special_
/// MoveToMarker_Montage2 0.5817: ActivateLastObject alone). Get Spawned Actor: IsValidClass(None) fails @29 -> @955
/// SpawnedActors_ClassChecked = SpawnedActors (no filter), then the same pick @159-791, of every class the owner's
/// Comp_Spawner spawned. Each pick goes to the world as a request for that one actor. The port's SpawnedActors:
/// the fighter's `spawned_actors` (an actor marked dead or reflected away has left it), then the open
/// ProjectileAimers (the one under, then the top one).
// Deviation: the aimers are taken as spawned after every actor of the list; an actor the owner's Comp_Spawner spawns
// while an aimer is open comes after the aimer in the build's list, here before it. SpawnedActors_ClassChecked is not
// written: no class of the call reads it.
fn activate_unfiltered(w: &mut World, f: FighterId, p: &Props) {
    let fi = w.fighter(f);
    let mut l: Vec<Listed> =
        fi.items.spawned_actors.iter().copied().filter(|&a| listed(w, a)).map(Listed::Actor).collect();
    if fi.items.aimer_under.is_some() {
        l.push(Listed::AimerUnder);
    }
    if fi.items.aimer.is_some() {
        l.push(Listed::Aimer);
    }
    let (Some(&head), Some(&tail)) = (l.first(), l.last()) else { return }; // Array_Get out of range: None
    let (all, last, first) =
        (flag(p, "ActivateAllObjects"), flag(p, "ActivateLastObject"), flag(p, "ActivateFirstObject"));
    let picks = if all {
        l
    } else if last && first {
        vec![head, tail]
    } else {
        vec![if last { tail } else { head }]
    };
    // the owner's values of this call, one copy per request
    let update = flag(p, "UpdateCharge");
    for a in picks {
        match a {
            Listed::Aimer => activate_aimer(w, f),
            Listed::AimerUnder => {
                warn_once(
                    w,
                    name!("ActivateSpawned without a class on the ProjectileAimer under the top one is not ported"),
                );
            }
            Listed::Actor(ActorRef::Item(_)) => {
                warn_once(w, name!("ActivateSpawned without a class on an item is not ported"));
            }
            Listed::Actor(a) => {
                let cls = actor_cls(w, a);
                let conds = activation_conds(p);
                let charge = if update { instigator_charge(w, f) } else { None };
                let r = ActivateRequest { cls, mode: ActivateMode::All, conds, charge, only: Some(a) };
                w.fighter_mut(f).requests.activate_requests.push(r);
            }
        }
    }
}

/// `!a.dead && !a.reflect?.unlisted`.
fn listed(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => {
            w.actors.projectiles.get(id).is_some_and(|p| !p.dead && !p.reflect.as_ref().is_some_and(|r| r.unlisted))
        }
        ActorRef::Actor(id) => w.actors.actors.get(id).is_some_and(|x| !x.dead),
        ActorRef::Item(id) => {
            w.actors.items.get(id).is_some_and(|x| !x.dead && !x.reflect.as_ref().is_some_and(|r| r.unlisted))
        }
        _ => true,
    }
}

fn actor_cls(w: &World, a: ActorRef) -> Name {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(Name::NONE, |p| p.cls),
        ActorRef::Actor(id) => w.actors.actors.get(id).map_or(Name::NONE, |x| x.cls),
        ActorRef::Item(id) => w.actors.items.get(id).map_or(Name::NONE, |x| x.cls),
        _ => Name::NONE,
    }
}

fn begin(b: fn(&mut World, FighterId, NotifyRef, i32)) -> NotifyHandler {
    NotifyHandler { begin: Some(b), strict: true, ..NotifyHandler::default() }
}

fn begin_end(
    b: fn(&mut World, FighterId, NotifyRef, i32),
    e: fn(&mut World, FighterId, NotifyRef, i32),
) -> NotifyHandler {
    NotifyHandler { begin: Some(b), end: Some(e), strict: true, ..NotifyHandler::default() }
}

pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let n = &mut r.notifies;
    n.enter("AnimNotify_ItemPickUp_C", begin(pick_up_begin))?;
    n.enter("AnimNotify_ItemHolster_C", begin_end(holster_begin, holster_end))?;
    n.enter("AnimNotify_AimItemThrow_C", begin_end(aim_throw_begin, aim_throw_end))?;
    n.enter("AnimNotify_ItemThrow_C", begin(item_throw_begin))?;
    n.enter("AnimNotify_ApplyBuffNearbyPlayer_C", begin(nearby_buff_begin))?;
    n.enter("AnimNotify_FaceTowards_C", begin_end(face_begin, face_end))?;
    n.enter("AnimNotify_ActivateTeleportToAlly_C", begin(teleport_begin))?;
    n.enter("AnimNotify_Spawner_C", begin_end(spawner_begin, spawner_end))?;
    n.enter("AnimNotify_ConditionalSpawner_C", begin_end(spawner_begin, cond_spawner_end))?;
    n.enter("AnimNotify_ActivateSpawned_C", begin_end(activate_begin, activate_end))?;
    Ok(())
}
