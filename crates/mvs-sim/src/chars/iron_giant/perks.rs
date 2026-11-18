//! Iron Giant's perks: Buff_C017_AllyBarrierGrayHealth and
//! Buff_C017_ThornsStack (markers), Buff_C017_FastWeaponMode and Buff_C017_ScorchedEarth; with `mod.rs`'s
//! C017_TempHealth_DamageReflect_Passive, the passive buff ThornsStack changes.
//!
//! C017_DownThrusterAttack (actor_state.rs) reaches ScorchedEarth through [`thruster_hit_ground`].
//!
//! Read with them: Comp_C017_Hover_Rage SetRespawnRage @7700; C017_DownThrusterAttack ThrusterHitGround @3828 /
//! @5484; Comp_Spawner's pool @2774-3659.

use super::hover_rage::set_respawn_rage;
use crate::Real;
use crate::State;
use crate::actors::{ActorId, ActorRef};
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, end_buff, inst_view, remove_stack, spawn_buff};
use crate::combat::{BuffClass, BuffRef};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const FIRE_WALL: Name = name!("FireWallHazard_C");
const THRUSTER: Name = name!("C017_DownThrusterAttack_C");
const SCORCHED: Name = name!("Buff_C017_ScorchedEarth_C");
const THORNS_STACK: Name = name!("Buff_C017_ThornsStack_C");
const THORNS: Name = name!("Buff_Thorns_C");

// ---------------------------------------------------------------- Buff_C017_FastWeaponMode
// ReceiveBeginPlay @301 (the parent's, then authority @331) -> @15: the owner's Comp_C017_Hover_Rage valid @115 ->
// its SetRespawnRage(RespawnRage = 25) @200 (`hover_rage::set_respawn_rage`: AddRage(25) when the component's
// RespawnRage differs, then RespawnRage = 25); else Delay(0.2) @246 and @15 again. The component is made with the
// Giant (`init`), so the retry is reached only on an owner that is no Giant (the perk is the Giant's own,
// SpawnOnAllies false): kept as a latent count all the same.
// What it gives: at the first possession (World.reset -> addPerks) the component's RespawnRage is 0, so AddRage(25):
// he starts with 25 rage. At a respawn the pawn is the same (ResetFighterLogic -> PerkManager.ResetFighter -> Add),
// its component keeps RespawnRage 25, the new buff's SetRespawnRage changes nothing and OnOwnerReset's
// ResetRage(true) (the reset hook, after addPerks) sets the rage to 25. That order is the build's (ResetFighterLogic
// @859 PerkManager.ResetFighter spawns the buffs, their BeginPlay inside it, ahead of the @1106 OnFighterReset
// broadcast that reaches OnOwnerReset @7670).

/// `FastState`.
#[derive(State, Debug, Default)]
pub struct FastWeapon {
    /// The latent ticks left of the running Delay(0.2), 0 none.
    pub wait: Real,
}

fn fast_weapon(w: &mut World, f: FighterId, b: BuffRef) {
    let has = super::state::giant_of(w, f).is_some_and(|g| g.hover_rage.is_some());
    if has {
        let v = inst_view(w, b).num("RespawnRage").unwrap_or(0.0);
        set_respawn_rage(w, f, v);
        if let Some(BuffExt::FastWeaponMode(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.wait = 0.0;
        }
    } else if let Some(BuffExt::FastWeaponMode(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.wait = latent_frames(0.2);
    }
}

fn fast_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::FastWeaponMode(FastWeapon { wait: 0.0 });
    }
    fast_weapon(w, f, b);
}

fn fast_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let fire = match buff_mut(w, b).map(|x| &mut x.ext) {
        Some(BuffExt::FastWeaponMode(e)) if e.wait > 0.0 => {
            e.wait -= 1.0;
            e.wait == 0.0
        }
        _ => false,
    };
    if fire {
        fast_weapon(w, f, b);
    }
    false
}

// ---------------------------------------------------------------- Buff_C017_ScorchedEarth
// ReceiveBeginPlay @1995 (authority) -> @1000: the Instigator's Comp_Spawner AfterSpawnedActor bound to
// AfterSpawnedThruster @1094 (self-only perk: the owner's own `AfterSpawnedActor` signal).
// AfterSpawnedThruster @10: IsSoftRef(C017_DownThrusterAttack) and the cast -> ThrusterActor = it @254 and its
// ThrusterHitGround bound to SpawnFirewall @273-296 (an AddUnique: once per object). Else IsSoftRef(FireWallHazard)
// @342: its location's Y equal to LeftTransform.Y @706 -> LeftFirewall @758; else equal to RightTransform.Y @924 ->
// RightFirewall @976. World Y is sim -x: the test is on x.
// SpawnFirewall(HitLocation, IsLeftThruster) @1140: ThrusterActor already in that side's set -> nothing @1292; else
// Set_Add @1325; CurrentSourceThruster not valid or not ThrusterActor @1448-1500 -> the valid LeftFirewall and
// RightFirewall destroyed @1557 / @1636; CurrentSourceThruster = ThrusterActor @1672; Left- or RightTransform =
// HitLocation @1705 / @1963; the Instigator's Comp_Spawner.CompSpawnActor(FireWallHazard_C, AttachToActor false,
// WorldTransforms true, Conv_VectorToTransform(HitLocation), zero direction, 0, 0) @1840, as Superman's FireSlam
// (`comp_spawn_projectile`). Its AfterSpawnedActor comes inside that call, after the transform was stored. The sets
// are never cleared and the class has no EndPlay.
// The pool: C017_DownThrusterAttack is CanBePooled; Comp_Spawner takes the last actor of the class's pool array
// (@2944-3573, InitFromPool @3399) and spawns a new one only with the array empty; SpawnedActorParent releases it to
// the array at its end (ReleaseToPool @22789, AddActorToPool AddUnique @246). A reused thruster is the same object, so
// it is already in both sets: only the first Down Air of a buff's life (one life) whose flames reach the ground makes
// walls. The port makes a new DownThruster per spawn; its identity here is a pool slot per owner, the lowest slot no
// live thruster of this buff's life holds (0 unless one is still alive: the pool array is then empty and the build
// makes a new object).
// Deviation: which pooled object comes back when more than one is in the array (the last released, LIFO; only after
// two thrusters were out at once) is not modelled: the lowest free slot.
// Unverified: the wall's location is read after its spawn push-out (World.pushOutRoot); a wall the push-out moves
// sideways fails the Y test and is held as neither Left nor Right (not destroyed by a later thruster).

/// `ScorchState`.
#[derive(State, Debug)]
pub struct Scorch {
    /// ThrusterActor: its pool slot, -1 none.
    pub thruster: i32,
    /// The ids (`seq`) of the thrusters spawned in this buff's life (bound) ...
    pub bound: Vec<i32>,
    /// ... and their pool slots.
    pub slots: Vec<i32>,
    /// ThrustersUsedForLeftFirewall (slots).
    pub used_l: Vec<i32>,
    /// ThrustersUsedForRightFirewall (slots).
    pub used_r: Vec<i32>,
    /// CurrentSourceThruster: a slot, -1 none.
    pub source: i32,
    /// LeftTransform's place (sim x, y).
    pub lx: Real,
    pub ly: Real,
    /// RightTransform's.
    pub rx: Real,
    pub ry: Real,
    /// LeftFirewall: the projectile's id (`seq`), -1 none.
    pub left: i32,
    /// RightFirewall.
    pub right: i32,
}

impl Default for Scorch {
    fn default() -> Scorch {
        Scorch {
            thruster: -1,
            bound: Vec::new(),
            slots: Vec::new(),
            used_l: Vec::new(),
            used_r: Vec::new(),
            source: -1,
            lx: 0.0,
            ly: 0.0,
            rx: 0.0,
            ry: 0.0,
            left: -1,
            right: -1,
        }
    }
}

fn scorch_mut(w: &mut World, b: BuffRef) -> Option<&mut Scorch> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::ScorchedEarth(e) => Some(e),
        _ => None,
    }
}

fn scorch_ref(w: &World, b: BuffRef) -> Option<&Scorch> {
    match &buff(w, b)?.ext {
        BuffExt::ScorchedEarth(e) => Some(e),
        _ => None,
    }
}

fn live_wall(w: &World, seq: i32) -> Option<crate::actors::ProjectileId> {
    if seq < 0 {
        return None;
    }
    w.actors.projectiles.iter().find(|(_, p)| p.seq == seq && !p.dead).map(|(id, _)| id)
}

fn spawn_firewall(w: &mut World, f: FighterId, b: BuffRef, x: Real, y: Real, is_left: bool) {
    let Some(e) = scorch_mut(w, b) else { return };
    let th = e.thruster;
    let set = if is_left { &mut e.used_l } else { &mut e.used_r };
    if set.contains(&th) {
        return;
    }
    set.push(th);
    let clear = e.source < 0 || e.source != th;
    let (l, r) = (e.left, e.right);
    e.source = th;
    if is_left {
        (e.lx, e.ly) = (x, y);
    } else {
        (e.rx, e.ry) = (x, y);
    }
    if clear {
        // Ahead of the store; only the order of two writes differs, and they touch nothing the destroy reads.
        if let Some(l) = live_wall(w, l) {
            crate::world::spawning::destroy_projectile(w, l, false);
        }
        if let Some(r) = live_wall(w, r) {
            crate::world::spawning::destroy_projectile(w, r, false);
        }
    }
    crate::world::spawning::comp_spawn_projectile(w, f, FIRE_WALL, x, y, [Some(0.0); 3], 0.0, 0.0, None);
}

/// `thrusterHitGround`: C017_DownThrusterAttack's ThrusterHitGround(HitLocation, IsLeft) broadcast (@3828 left foot,
/// @5484 right; each tick a foot's TraceFlame hits), sent by `actors` (`thruster_step`). Its one binder is this buff's
/// SpawnFirewall, bound on a thruster its AfterSpawnedThruster saw.
pub fn thruster_hit_ground(w: &mut World, th: ActorId, x: Real, y: Real, is_left: bool) {
    let Some((owner, seq)) = w.actors.actors.get(th).map(|a| (a.owner, a.seq)) else { return };
    let Some(b) = buff_of(w, owner, SCORCHED) else { return };
    if !scorch_ref(w, b).is_some_and(|e| e.bound.contains(&seq)) {
        return;
    }
    spawn_firewall(w, owner, b, x, y, is_left);
}

fn scorch_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ScorchedEarth(Scorch::default());
    }
}

fn scorch_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::AfterSpawnedActor {
        return false;
    }
    let Some(act) = a.actor else { return false };
    match act {
        ActorRef::Actor(id) if w.actors.actors.get(id).is_some_and(|x| x.cls == THRUSTER) => {
            // the bindings of thrusters gone since are dropped (their ids are not reused)
            let seq = w.actors.actors.get(id).map_or(-1, |a| a.seq);
            let live: Vec<i32> = w
                .fighter(f)
                .items
                .spawned_actors
                .iter()
                .filter_map(|o| match *o {
                    ActorRef::Actor(oid) if oid != id => {
                        w.actors.actors.get(oid).filter(|x| x.cls == THRUSTER && !x.dead).map(|x| x.seq)
                    }
                    _ => None,
                })
                .collect();
            let Some(e) = scorch_mut(w, b) else { return false };
            let keep: Vec<(i32, i32)> =
                e.bound.iter().copied().zip(e.slots.iter().copied()).filter(|(i, _)| live.contains(i)).collect();
            let mut slot = 0;
            while keep.iter().any(|&(_, s2)| s2 == slot) {
                slot += 1;
            }
            e.bound = keep.iter().map(|&(i, _)| i).chain([seq]).collect();
            e.slots = keep.iter().map(|&(_, s2)| s2).chain([slot]).collect();
            e.thruster = slot;
        }
        ActorRef::Projectile(pr) => {
            let Some(p) = w.actors.projectiles.get(pr).filter(|p| p.cls == FIRE_WALL) else { return false };
            let (x, seq) = (p.x, p.seq);
            let Some(e) = scorch_mut(w, b) else { return false };
            if x == e.lx {
                e.left = seq;
            } else if x == e.rx {
                e.right = seq;
            }
        }
        _ => {}
    }
    false
}

// ---------------------------------------------------------------- C017_TempHealth_DamageReflect_Passive
// The buff Pawn_C017's Comp_Pawn_PassiveBuffManager gives him and his allies (World.passiveBuffs), the Giant its
// Instigator. ReceiveBeginPlay @1880 -> @1474 (authority): the owner's Comp_Armor.TempHealthAdded (@1568),
// TempHealthFullyRemoved (@1703) and TempHealthPackRemoved (@1838); never unbound.
// TempHealthAdded @132 (the pack's type): the owner with Buff_C017_ThornsStack (CheckHasBuff @208; the perk
// GrayHealthThornsStack, SpawnOnAllies) -> the type not in TempHealthTypes @467 -> the spawn below; without it (@926)
// the spawn only while ReflectDamageBuff is not valid. The spawn: SpawnBuffActor(Buff_Thorns_C, the owner, the
// Instigator, 1 stack, Duration 0) @548, ReflectDamageBuff = its NewBuff @632 and Set_Add(type) @651 (on both paths).
// Then @702, on an owner other than the Instigator: PF_Post_Event of the two teammate sounds on the buff actor (@795,
// @860), whichever path ran.
// TempHealthPackRemoved @970 (the type): the marker, ReflectDamageBuff valid and the type in the set @1362 ->
// Set_Remove @1372 and ReflectDamageBuff.RemoveStack(1) @1432.
// TempHealthRemoved @15 (fully removed): TempHealthTypes cleared, ReflectDamageBuff destroyed when valid @95.
// Buff_Thorns is CanStack (MaxStacks 5), not CanOverride: from the second type on (the marker's path) the spawn adds a
// stack to the live one and its NewBuff is none (a BP frame's out param left zeroed; `spawn_buff` returns none there
// too), so ReflectDamageBuff goes none: the later pack removals and the full removal then take nothing off, and the
// Thorns keep their stacks. Kept as the build has it.
// Unverified: what SpawnBuffActor returns on a stack add is taken as none (above).
// The set is kept only with the marker (made at its first add): it is read on the marker's paths alone, and the
// marker, a perk's, is on the owner for each whole life, so the no-marker path's Set_Add is never read.

/// `{ thorns, types? }`.
#[derive(State, Debug, Default)]
pub struct DamageReflect {
    /// ReflectDamageBuff.
    pub thorns: Option<BuffRef>,
    /// TempHealthTypes, `None` until the marker's first add.
    pub types: Option<Vec<Name>>,
}

fn reflect_begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.src = from;
        x.ext = BuffExt::DamageReflect(DamageReflect::default());
    }
}

fn reflect_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(BuffExt::DamageReflect(e)) = buff(w, b).map(|x| &x.ext) else { return false };
    let thorns = e.thorns;
    let live = thorns.is_some_and(|t| buff(w, t).is_some_and(|x| x.frames > 0.0));
    let marked = buff_of(w, f, THORNS_STACK).is_some();
    let src = buff(w, b).and_then(|x| x.src);
    let pack = a.pack.unwrap_or(Name::NONE);
    match s {
        BuffSignal::TempHealthAdded => {
            let has_type = match buff(w, b).map(|x| &x.ext) {
                Some(BuffExt::DamageReflect(e)) => e.types.as_ref().is_some_and(|t| t.contains(&pack)),
                _ => false,
            };
            if if marked { !has_type } else { !live } {
                let nb = spawn_buff(w, f, THORNS, 0.0, Some(1), src, true, None);
                if let Some(BuffExt::DamageReflect(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
                    e.thorns = nb;
                    if marked {
                        e.types.get_or_insert_with(Vec::new).push(pack);
                    }
                }
            }
            if src != Some(f) {
                let fighter = w.fighter(f).id;
                for ev in [
                    name!("sfx_cha_C017_special_neutral_screws_teammate_leadin"),
                    name!("sfx_cha_C017_special_neutral_screws_teammate_impact"),
                ] {
                    w.fighter_mut(f).events.push(Event::Sound {
                        fighter,
                        ev,
                        x: None,
                        y: None,
                        hit: None,
                        pooled: false,
                        key: None,
                    });
                }
            }
        }
        BuffSignal::TempHealthPackRemoved => {
            let i = match buff(w, b).map(|x| &x.ext) {
                Some(BuffExt::DamageReflect(e)) => e.types.as_ref().and_then(|t| t.iter().position(|&n| n == pack)),
                _ => None,
            };
            let (true, true, Some(i)) = (live, marked, i) else { return false };
            if let Some(BuffExt::DamageReflect(e)) = buff_mut(w, b).map(|x| &mut x.ext)
                && let Some(t) = e.types.as_mut()
            {
                t.remove(i);
            }
            if let Some(t) = thorns {
                remove_stack(w, f, t, 1);
            }
        }
        BuffSignal::TempHealthFullyRemoved => {
            if let Some(BuffExt::DamageReflect(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
                if let Some(t) = e.types.as_mut() {
                    t.clear();
                }
                e.thorns = None;
            }
            if live && let Some(t) = thorns {
                end_buff(w, f, t);
            }
        }
        _ => {}
    }
    false
}

/// This file's registry entries (the four perk buffs and C017_TempHealth_DamageReflect_Passive_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // No graph: C017_HasPerk_GrayHealth / C017_Not_HasPerk_GrayHealth are its only readers and no asset of the build
    // uses either condition (no reference in the data or the bytecode). It does nothing in this build.
    r.buffs.enter(
        BuffClass::BuffC017AllyBarrierGrayHealth.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffC017AllyBarrierGrayHealth, BuffHooks::default()),
    )?;
    // No graph: the marker C017_TempHealth_DamageReflect_Passive checks.
    r.buffs.enter(
        BuffClass::BuffC017ThornsStack.registered_name().expect("a registered class"),
        BuffEntry::new(BuffClass::BuffC017ThornsStack, BuffHooks::default()),
    )?;
    r.buffs.enter(
        BuffClass::BuffC017FastWeaponMode.registered_name().expect("a registered class"),
        BuffEntry::new(
            BuffClass::BuffC017FastWeaponMode,
            BuffHooks { begin: Some(fast_begin), tick: Some(fast_tick), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        BuffClass::BuffC017ScorchedEarth.registered_name().expect("a registered class"),
        BuffEntry::new(
            BuffClass::BuffC017ScorchedEarth,
            BuffHooks { begin: Some(scorch_begin), on: Some(scorch_on), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        BuffClass::C017TempHealthDamageReflectPassive.registered_name().expect("a registered class"),
        BuffEntry::new(
            BuffClass::C017TempHealthDamageReflectPassive,
            BuffHooks { begin: Some(reflect_begin), on: Some(reflect_on), ..BuffHooks::default() },
        ),
    )
}
