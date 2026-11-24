//! The general perks ElectricProjectile "Static Electricity" (Buff_ProjectileApplyBuffAfterGrounded, which gives
//! Buff_Elemental_Electric to the owner's next projectile) and FireProjectile "That's Flammable, Doc!"
//! (Buff_ProjectileToMeleeFire, a data child of Buff_ProjectileApplyBuff, which marks a hit enemy with
//! Buff_DetonateFromMelee; a melee knockback then sets him on fire, Buff_Fire).
//! Read: the graphs Buff_ProjectileApplyBuff, Buff_ProjectileApplyBuffAfterGrounded, Buff_DetonateFromMelee,
//! Buff_Elemental_Electric (offsets are each class's ubergraph's unless a function is named). Class defaults: the
//! general perk table's buffs.
//! Buff_FireGauntlets and Buff_Elemental_Fire are not on its path: the only graph that spawns Buff_FireGauntlets is
//! Buff_C020_Portal (a signature perk), and nothing of these classes spawns Buff_Elemental_Fire. The orphan
//! Buff_ElectricProjectile (Buff_Damage_Electric through the common callback) is in no PerkData's AppliedBuffs and is
//! not entered.

use super::conds::{actor_buff, class_name};
use super::hooks::{NO_ACTOR, NO_HOOKS, sound};
use super::{PerkActorExt, PerkExt, def, enter, ext, ext_mut, flag, kind, num, peer_by_id, set_ext, stacks};
use crate::Real;
use crate::actors::ActorRef;
use crate::actors::item::is_owned;
use crate::actors::world_actor::actor_elem;
use crate::combat::buffs::{Def, buff_of, spawn_buff};
use crate::combat::elem::{
    ElemBuff, ElemExt, PROJECTILE_APPLY_BUFF_ACTOR, elem_buff_mut, projectile_like, spawn_actor_buff,
};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::{UiStateOp, UiStateParams};
use crate::fighter::FighterId;
use crate::fighter::actor_y;
use crate::fighter::common::{buff_def, lookup_owner, owner_team, timer_frames, warn_once};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, BuffHooks};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const ELEM_ELECTRIC: Name = name!("Buff_Elemental_Electric_C");
const ELECTRIC: Name = name!("Buff_Damage_Electric_C");
const FIRE: Name = name!("Buff_Fire_C");
const SHIELD_ELECTRIC: Name = name!("Buff_Shield_Electric_C");
const EQUIP_SOUND: Name = name!("sfx_cha_superman_icegauntlet_start");

/// A timer's ticks as the actor buff keeps them.
fn ticks(t: Real) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "a timer's ticks are an integer")]
    let n = t as i32;
    n
}

/// A buff count as SpawnBuffActor's int argument.
fn count(s: Real) -> i32 {
    #[allow(clippy::cast_possible_truncation, reason = "a stack count is an integer")]
    let n = s as i32;
    n
}

fn actor_buff_mut(w: &mut World, a: ActorRef, i: u32) -> Option<&mut ElemBuff> {
    elem_buff_mut(w, a, i)
}

/// BPI_GetBoxCollision and BPI_GetSphereCollision valid on the actor (`ElemState.box` / `sphere`); false for both
/// without a buff list.
fn shapes(w: &World, a: ActorRef) -> (bool, bool) {
    match actor_elem(w, a) {
        Some(Some(st)) => (st.bx, st.sphere),
        _ => (false, false),
    }
}

// ---------------------------------------------------------------- Buff_ProjectileToMeleeFire
// A data child of Buff_ProjectileApplyBuff (the graph, shared with Buff_IceProjectile, is the projectile_apply_buff
// family): Buff = Buff_DetonateFromMelee, Unstacked / StackedBuffStacksToApply 1 / 2, Unstacked /
// StackedBuffDuration 3 / 3. On a fighter (the perk's own buff): the dealt condition alone (Active is read by nothing
// there).
fn always(_w: &mut World, _v: ActorRef, _o: FighterId, _dl: FighterId, _h: &HitPacket, _b: BuffRef, _hb: bool) -> bool {
    true
}

// ---------------------------------------------------------------- Buff_DetonateFromMelee
// CDO: IsDebuff, BuffDuration 3, CanOverride (a new mark replaces the old one, its latch with it), CanStack,
// PreventStacksWhenReapplied, MaxStacks 5. TakenDamageModCondition: while !DetonateOnHit @0, DetonateOnHit =
// (Attacker == HitCollision.GetOwner()) @85-123 and the answer is that; once set it stays and the function returns
// its default false (its mods are DoNothing either way).
// BPI On Fighter Owner Knocked Back (BuffParentActor @251 forwards the owner's KnockedBack) -> @976: the Attacker
// valid and a BP_FighterCharacter, and DetonateOnHit @1084 -> @29: DetonateOnHit = false, SpawnBuffActor(Buff_Fire,
// the owner, the Attacker as Instigator, CurrentStackAmount, 0.0) @176, UIIgnite @264, K2_DestroyActor @10. A
// projectile's hit (the Attacker is the actor) fails the cast; a hit with no knockback path (light, armor) sends no
// KnockedBack.
// UIIgnite @283: the owner's broker AddStateForPlayer(FireProjectilePerk, StateType 1, DisplayType 2, -1, -1, Timer
// -1, MI_State_Ignite, Active, PopText "Ignited", AutoRemoveTime 2). BPI After Status Update Multicast @1141:
// removed -> RemoveStateForPlayer(FireProjectilePerkCount) @1414; applied or refreshed -> AddStateForPlayer(
// FireProjectilePerkCount, StateType 1, DisplayType 2, -1, -1, Timer BuffDuration, MI_State_Ignite, not Active,
// PopText "", AutoRemoveTime BuffDuration) @1957. (CanRefresh is not set: no refresh reaches it.)
// Unverified: Timer / AutoRemoveTime are the class's BuffDuration (3), which every spawner here passes too.
fn ignite(id: Name, timer: Real, active: bool, pop_text: Name, auto_remove_time: Real) -> UiStateOp {
    UiStateOp::Add(UiStateParams {
        id,
        state_type: 1,
        display_type: 2,
        current_value: -1.0,
        max_value: -1.0,
        timer,
        icon_material: name!("MI_State_Ignite"),
        active,
        pop_text,
        auto_remove_time,
        subicon: None,
    })
}

fn detonate_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    set_ext(w, b, PerkExt::Detonate { latch: false });
}

// Attacker == HitCollision.GetOwner(): with a valid HitCollision its owner is the attacker in every ported call
// (BuffHooks.taken_cond), without one GetOwner is None against a valid Attacker: so `boxed`.
fn detonate_taken(
    w: &mut World,
    _f: FighterId,
    b: BuffRef,
    _att: Option<ActorRef>,
    boxed: bool,
    _h: &HitPacket,
    _by: Option<FighterId>,
) -> bool {
    let Some(PerkExt::Detonate { latch }) = ext_mut(w, b) else { return false };
    if *latch {
        return false;
    }
    *latch = boxed;
    boxed
}

fn detonate_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::KnockedBack || !matches!(ext(w, b), Some(PerkExt::Detonate { latch: true })) {
        return false;
    }
    // the Attacker cast to BP_FighterCharacter @1084 (none for an actor)
    let Some(by) = a.by.and_then(|d| d.fighter) else { return false };
    set_ext(w, b, PerkExt::Detonate { latch: false });
    let n = count(stacks(w, b));
    spawn_buff(w, f, FIRE, 0.0, Some(n), Some(by), true, None);
    let op = ignite(name!("FireProjectilePerk"), -1.0, true, name!("Ignited"), 2.0);
    w.fighter_mut(f).controls.ui_state_ops.push(op);
    true
}

fn detonate_status(w: &mut World, f: FighterId, c: BuffClass, _stacks: Real, applied: bool, removed: bool) {
    let op = if removed {
        UiStateOp::Remove(name!("FireProjectilePerkCount"))
    } else if applied {
        let cls = class_name(w, c);
        let t = num(Def::of(buff_def(w, ActorRef::Fighter(f), cls)), "BuffDuration");
        ignite(name!("FireProjectilePerkCount"), t, false, Name::NONE, t)
    } else {
        return;
    };
    w.fighter_mut(f).controls.ui_state_ops.push(op);
}

// ---------------------------------------------------------------- Buff_ProjectileApplyBuffAfterGrounded
// CDO: RequiredSecondsBase 6, RequiredSecondsPerStack -2, CanStack, MaxStacks 5, GauntletVFX PS_CreatureCrystal_Buff,
// VFXScaling 1.125, PlayEquipSound. GetRequiredSeconds: RequiredSecondsBase + CurrentStackAmount x
// RequiredSecondsPerStack (4 s at one stack, 2 s at two), float32.
// ReceiveBeginPlay @1016, authority, the owner a BP_FighterCharacter: SecondsRemaining = GetRequiredSeconds @1182;
// its Comp_Spawner AfterSpawnedActor -> OnSpawnActor @1260 / @1283, its Comp_ItemManager OnItemThrown ->
// OnItemThrown @1375 / @1398 (the AfterSpawnedActor / ItemThrown signals). No EndPlay of its own.
// ReceiveTick @1444 -> @10: authority, !IsCharged @44, the owner a BP_FighterCharacter: IsMovingOnGround @186 ->
// VSize(Velocity) != 0 @349 -> SecondsRemaining -= DeltaSeconds @397, IsCharged = SecondsRemaining <= 0 @504, and
// when charged CreateBuffVisuals @537; not on the ground: SecondsRemaining = GetRequiredSeconds @556 (each tick).
// Standing still on the ground keeps what is left; once charged the gate skips all of it (a jump keeps the charge).
// CreateBuffVisuals @611 (not a dedicated server, the owner a BaseFighter): SpawnVFX (GauntletVFX on L_Hand and
// R_Hand, VFXScaling x the mesh's VFX scale, Team = GetColorIndex), the MeshManager's ActiveMeshChanged bind @797,
// and with PlayEquipSound sfx_cha_superman_icegauntlet_start at the buff's location @925.
// HandleProjectileAttack(actor) (OnSpawnActor @1492, any actor the Comp_Spawner spawns; OnItemThrown @1520):
// projectileLike and IsCharged @243 -> SpawnBuffActor(Buff_Elemental_Electric, the actor, the fighter as
// Instigator, 0 stacks, 0.0) on the actor's own State Manager @366, SecondsRemaining = GetRequiredSeconds @473,
// IsCharged = false @500, RemoveBuffVisuals @511. An actor that fails the test leaves the charge.
// Unverified: the tick is the buff list's pass (the frame's start: the grounded flag and the velocity the last
// frame's movement left), DeltaSeconds the fixed step (a buff actor has no hitpause dilation).
// Unverified: Velocity is the fighter's (vx, vy); "on the ground" is the fighter's on_floor.
// Not ported: the hand emitters (no render entry), RemoveVFX, and the ActiveMeshChanged rebind.
fn required_seconds(w: &World, b: BuffRef) -> Real {
    let d = def(w, b);
    num(d, "RequiredSecondsBase") + (stacks(w, b) * num(d, "RequiredSecondsPerStack"))
}

fn grounded_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let left = required_seconds(w, b);
    set_ext(w, b, PerkExt::Grounded { left, charged: false });
}

fn grounded_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(PerkExt::Grounded { left, charged: false }) = ext(w, b).copied() else { return false };
    let x = w.fighter(f);
    if !x.body.on_floor {
        let left = required_seconds(w, b);
        set_ext(w, b, PerkExt::Grounded { left, charged: false });
        return false;
    }
    if x.body.vx == 0.0 && x.body.vy == 0.0 {
        return false;
    }
    let left = left - DELTA_SECONDS;
    let charged = left <= 0.0;
    set_ext(w, b, PerkExt::Grounded { left, charged });
    if charged && flag(def(w, b), "PlayEquipSound") {
        let at = [w.fighter(f).body.x, actor_y(w, f)];
        sound(w, f, EQUIP_SOUND, Some(at));
    }
    false
}

fn grounded_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(actor) = a.actor else { return false };
    if s != BuffSignal::AfterSpawnedActor && s != BuffSignal::ItemThrown {
        return false;
    }
    if !matches!(ext(w, b), Some(PerkExt::Grounded { charged: true, .. })) {
        return false;
    }
    if let ActorRef::Item(it) = actor
        && !is_owned(w, it)
    {
        return false;
    }
    if !projectile_like(w, actor) {
        return false;
    }
    spawn_actor_buff(w, actor, ELEM_ELECTRIC, 0, 0.0, true, Some(f));
    let left = required_seconds(w, b);
    set_ext(w, b, PerkExt::Grounded { left, charged: false });
    false
}

// ---------------------------------------------------------------- Buff_Elemental_Electric
// A Buff_ElementalParent child (MaxStacks 10, CanStack, BuffDuration 0, OverlapTimer 0.5). ReceiveBeginPlay @560:
// the parent's (its binds of the owner's HitBoxHandler HitEnemyPlayer and the rest), Team Index = the Instigator's
// Comp_Actor_Destructible TeamIndex @755, then @10: authority, with the owner's BPI_GetBoxCollision or
// BPI_GetSphereCollision valid, the looping ApplyOverlapElectric timer every OverlapTimer @232.
// ElementalHitEnemy(HitPlayer) @875: the parent's (empty), then authority, the HitPlayer's State Manager valid, no
// Buff_Shield_Electric on him @1082-1235 -> SpawnBuffActor(Buff_Damage_Electric, the HitPlayer, the Instigator,
// CurrentStackAmount, 0.0) @1325. The elemental stays on the actor (no destroy after it), so each enemy hit of the
// actor gives it again. Bound in its BeginPlay, behind the actor class's own handler: the HitEnemyPlayer signal.
// Its other handlers (HitAlly, HitAnything, the overlaps) call the parent's empty ones.
// ApplyOverlapElectric @0-2808, the timer's handler (ActorBuffHooks.overlap_timer: the host's tick counts it and
// passes what its box or sphere overlaps). LocalOverlapingActors, a local of the function (empty at each call), is
// appended: with a valid box GetOverlappingActors(any class) @204 and then (SpawnedActorParent) @307; then with a
// valid sphere (BP_FighterCharacter) @606 and (SpawnedActorParent) @717. Each entry in that order @817-1997: a
// BP_FighterCharacter @1061 with a Comp_Actor_Destructible @1250 whose TeamIndex is not Team Index @1399, a State
// Manager @1579 and no Buff_Shield_Electric @1728 gets SpawnBuffActor(Buff_Damage_Electric, it, the Instigator, 1,
// 0.0) @2723; any other entry that is a SpawnedActorParent @2061 with a Comp_Actor_Destructible of TeamIndex == Team
// Index @2277 and a State Manager @2398 gets SpawnBuffActor(Buff_Elemental_Electric, it, the Instigator, 1, 0.0)
// @2508 (a stack on a live one, CanStack to MaxStacks 10). A box host's spawned actors are met twice (both box
// calls list them).
// The ElementalOnCapsuleEndOverlap override (@1881) calls the parent's (Buff_ElementalParent @3571: empty), as do
// the other overlap handlers: nothing to port.
// Unverified: the host itself is left out of its own overlaps (whether the build's component overlaps list their
// own actor is not read); a box host's any-class list is taken as its fighters, then its spawned actors (the order
// of the mixed list is PhysX's); the spawned actors' team is their owner's (Comp_Actor_Destructible TeamIndex, as
// everywhere in the port). FireWall (the one box host) passes no spawned actors: warned at the begin.
// On a fighter (no caller): warned.
fn electric_def(w: &World, a: ActorRef, i: u32) -> Def<'_> {
    let Some(b) = actor_buff(w, a, i) else { return Def::default() };
    let cls = class_name(w, b.class);
    Def::of(buff_def(w, ActorRef::Fighter(lookup_owner(w, a)), cls))
}

fn electric_state(w: &World, a: ActorRef, i: u32) -> (i32, i32) {
    match actor_buff(w, a, i).map(|e| e.ext) {
        Some(ElemExt::Perk(PerkActorExt::Electric { by, team })) => (by, team),
        _ => (-1, -1),
    }
}

fn electric_pulse(w: &mut World, a: ActorRef, i: u32, fighters: &[FighterId], actors: &[ActorRef]) {
    // looping: set again inside the timer tick
    let t = ticks(timer_ticks(Some(num(electric_def(w, a, i), "OverlapTimer"))));
    if let Some(e) = actor_buff_mut(w, a, i) {
        e.timer = Some(t);
    }
    let (by, team) = electric_state(w, a, i);
    let from = if by >= 0 { peer_by_id(w, by) } else { None };
    let (bx, sphere) = shapes(w, a);
    let give_actors = |w: &mut World| {
        for &o in actors {
            if o != a && owner_team(w, o) == team {
                spawn_actor_buff(w, o, ELEM_ELECTRIC, 1, 0.0, true, from);
            }
        }
    };
    // @2672-2723: on the victim's own state manager
    let give_fighters = |w: &mut World| {
        for &o in fighters {
            // Deviation: the port's spare switch spares an opponent the pulse, its source fighter gone or not.
            let spared = crate::combat::phases::spares_team(w, team, o);
            if w.fighter(o).team.team != team && buff_of(w, o, SHIELD_ELECTRIC).is_none() && !spared {
                spawn_buff(w, o, ELECTRIC, 0.0, Some(1), from, true, Some(o));
            }
        }
    };
    if bx {
        give_fighters(w); // @204: any class
        give_actors(w);
        give_actors(w); // @307: SpawnedActorParent
    }
    if sphere {
        give_fighters(w); // @606
        give_actors(w); // @717
    }
}

fn electric_begin(w: &mut World, _f: FighterId, _b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    warn_once(w, name!("Buff_Elemental_Electric_C on a fighter: not ported (no caller among the ported classes)"));
}

// @10-555: authority; BPI_GetBoxCollision valid @189, else BPI_GetSphereCollision valid @496 -> K2_SetTimer(
// ApplyOverlapElectric, OverlapTimer, looping) @232.
fn electric_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    let (by, team) = from.map_or((-1, -1), |o| (w.fighter(o).id, w.fighter(o).team.team));
    let (bx, sphere) = shapes(w, a);
    let t = ticks(timer_frames(Some(num(electric_def(w, a, i), "OverlapTimer"))));
    if let Some(e) = actor_buff_mut(w, a, i) {
        e.ext = ElemExt::Perk(PerkActorExt::Electric { by, team });
        if bx || sphere {
            e.timer = Some(t);
        }
    }
    if bx {
        const BOX: Name = name!(
            "Buff_Elemental_Electric_C on a box collision: ApplyOverlapElectric meets no spawned actor (the host passes none)"
        );
        warn_once(w, BOX);
    }
    false
}

fn electric_actor_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, a_arg: Option<&ActorBuffArg>) -> bool {
    let arg = a_arg.map(|x| ActorRef::Fighter(x.by));
    let Some(ActorRef::Fighter(v)) = arg else { return false };
    if s != ActorBuffSignal::HitEnemyPlayer || buff_of(w, v, SHIELD_ELECTRIC).is_some() {
        return false;
    }
    let (by, _) = electric_state(w, a, i);
    let n = actor_buff(w, a, i).map_or(0, |e| e.stacks);
    let from = if by >= 0 { w.by_id(by) } else { None };
    // @1254-1325: SpawnBuffActor on the hit player's own Comp_State_Manager (its NewBuffSpawned / BuffStacksAdded).
    spawn_buff(w, v, ELECTRIC, 0.0, Some(n), from, true, Some(v));
    false
}

pub(super) fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    enter(
        r,
        &[
            kind(
                C::BuffProjectileToMeleeFire,
                BuffHooks { dealt_cond: Some(always), ..NO_HOOKS },
                Some(PROJECTILE_APPLY_BUFF_ACTOR),
            ),
            kind(
                C::BuffDetonateFromMelee,
                BuffHooks {
                    begin: Some(detonate_begin),
                    taken_cond: Some(detonate_taken),
                    on: Some(detonate_on),
                    status: Some(detonate_status),
                    ..NO_HOOKS
                },
                None,
            ),
            kind(
                C::BuffProjectileApplyBuffAfterGrounded,
                BuffHooks { begin: Some(grounded_begin), tick: Some(grounded_tick), on: Some(grounded_on), ..NO_HOOKS },
                None,
            ),
            kind(
                C::BuffElementalElectric,
                BuffHooks { begin: Some(electric_begin), ..NO_HOOKS },
                Some(ActorBuffHooks {
                    begin: Some(electric_actor_begin),
                    overlap_timer: Some(electric_pulse),
                    on: Some(electric_actor_on),
                    ..NO_ACTOR
                }),
            ),
        ],
    )
}
