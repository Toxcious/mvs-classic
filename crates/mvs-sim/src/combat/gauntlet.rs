//! Buff_SupermanAllyIceGauntlet and its data children: the gauntlet a team mate is given,
//! which hands a buff to the first enemy its owner hits.
//!
//! Read: Buff_SupermanAllyIceGauntlet (the graph; offsets are its ubergraph's), Buff_AllyElectricGauntlets (the
//! child), BuffParentActor (StartLoopSound @5219, StopLoopSound @4913). Superman's pellets give the parent
//! (`chars::superman::ice`); the classes are entered here because Creature's SeismicReactor and C021's Bolt and chain
//! give the electric child, and Rick's portal perk the fire child.
//!
//! Buff_SupermanAllyIceGauntlet (BuffParentActor child; the graph is the class's own): given by a pellet to a team
//! mate with a duration of 0.0: no life timer (BuffDuration is the caller's, SetLifeTimer(0) sets none), so the
//! class's BuffDuration 6 never runs for it and the buff stays until the team mate's own hit lands or he dies.
//! MaxStacks 1, no CanStack; CanRefresh finds no active life timer, so a second pellet adds nothing.
//! BPI On Fighter Owner Hit Enemy Player @1789 (a do-once): SpawnBuffActor(BuffToApplyEnemy Buff_Ice, the hit player,
//! the buff's Instigator, StacksToApplyEnemy 3, 0.0) @374, HitVisuals @462 (HitVFX on the hit player, 'SFX Hit' at
//! his place), and with ExpireOnHit the buff is destroyed @495.
//! BPI After Status Update Multicast @1860: applied -> ApplyGauntlet_FX (GauntletVFX on R_Hand and L_Hand, 'SFX
//! Start' with PlayEquipSound), removed -> RemoveGauntlet_FX(true) ('SFX End', TimedOutVFX on both hands).
//! Not here: the emitters (the renderer draws the gauntlets from the buff on the fighter; HitVisuals' HitVFX is the
//! `gauntletHit` event), BPI_IncrementCoopActions (a player-state counter), OnMeshChanged.
//! BuffParentActor's own loop sound is posted per class (StartLoopSoundMulticast @1593 -> SFX_Loop_Start at
//! BeginPlay, StopLoopSound @4301 -> SFX_Loop_Stop / SFX_Loop_End at EndPlay): this class's ReceiveEndPlay @916 calls
//! the parent's first, so its SFX_Loop_End (sfx_cha_superman_icegauntlet_end; no SFX_Loop_Start or SFX_Loop_Stop in
//! its defaults) goes out at the owner's place (PF_Post_Event_At_Location @5132, the owner's K2_GetActorLocation
//! @5082) beside the status update's 'SFX End' (the same event). The electric child below posts the whole loop.
//! Unverified: the port's order puts 'SFX End' (the removed status update) ahead of SFX_Loop_End; the build's
//! StopLoopSound @4301 runs ahead of StatusUpdateMulticast @4349.
//!
//! Buff_AllyElectricGauntlets (a data child: ReceiveBeginPlay @10 forwards to the parent's, nothing else). Its data:
//! BuffToApplyEnemy Buff_Damage_Electric (`combat::electric`), StacksToApplyEnemy 3, ExpireOnHit false (the gauntlet
//! stays to its life timer after the do-once), PlayEquipSound false, 'SFX Start' / 'SFX End' / 'SFX Hit' None,
//! GauntletVFX PS_CreatureCrystal_Buff, HitVFX PS_Electricity_HitImpact, BuffDuration 6 (the spawners pass their own:
//! SeismicReactor 10 s, C021's Bolt 4 s). BuffParentActor's loop sound: SFX_Loop_Start
//! (sfx_cha_garnet_attack_special_u_gauntlet_glow_loop_superbuff) on the buff actor at BeginPlay
//! (StartLoopSoundMulticast @1593 -> @5258), SFX_Loop_Stop (sfx_cha_garnet_attack_special_u_charge_loop_stop) on the
//! buff actor at EndPlay (@4301 -> @4957); no SFX_Loop_End. The renderer keeps the actor's sound object by the key.
//!
//! Buff_FireGauntlets (Abilities/Shield; a data child with no graph: its CDO sets BuffToApplyEnemy Buff_Fire_C,
//! VFXScaling, GauntletVFX PS_Buff_FireFist_VFX and HitVFX None; the rest is the parent's or BuffParentActor's). Given
//! by Buff_C020_Portal (Rick's perk) to an ally fighter that goes through the perk holder's portal, with 0 stacks and
//! a duration of AllyMeleeIgniteDuration (0: no life timer, so it stays to the first hit, ExpireOnHit, or the
//! fighter's death). Its data: StacksToApplyEnemy 3, ExpireOnHit, PlayEquipSound, CanRefresh, 'SFX Start' / 'SFX End'
//! / 'SFX Hit' the ice gauntlet's sounds, TimedOutVFX PS_WonderWomanBracers_Cancel. No HitVFX: the `gauntletHit`
//! event draws nothing. Its SFX_Loop_End (sfx_cha_superman_icegauntlet_end) is the parent's, posted at its end as the
//! parent's is.
//!
//! Garnet's zone gauntlets (Characters/Garnet/Blueprints; GarnetElectricZone gives them): data children with no
//! bytecode of their own: Buff_GarnetElecOutsideBubble's parent is Buff_AllyElectricGauntlets,
//! Buff_GarnetElecInsideBubble's is OutsideBubble, Buff_GarnetFullPowerGauntlets' is Buff_AllyElectricGauntlets
//! (SuperStruct of each class). So each is the electric gauntlet's graph on its own defaults: BuffToApplyEnemy
//! Buff_Damage_Electric, StacksToApplyEnemy 3, ExpireOnHit false, no 'SFX Start' / 'SFX End' / 'SFX Hit', the do-once
//! hit; the loop sounds SFX_Loop_Start (..._gauntlet_glow_loop_insidebubble / _outsidebubble / _superbuff) and
//! SFX_Loop_Stop (sfx_cha_garnet_attack_special_u_charge_loop_stop). Their own data the shared buff code reads:
//! InsideBubble and OutsideBubble CanRefresh, BuffDuration 6 (the zone passes 0 and 2.0); FullPowerGauntlets
//! CanRefresh false, BuffDuration 5 (the zone passes 6.0), DealtDamageMods DamageMod 1.25 (multiplied), StateInfo
//! BreakBuffs Buff_AllyElectricGauntlets and Buff_GarnetElecInsideBubble (not OutsideBubble).

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, inst_view, spawn_buff};
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::actor_y;
use crate::fighter::common::buff_def;
use crate::fighter::control::data_name;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::truthy;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The class's own variables: the Instigator (`from`, set at the begin when there is one) and the do-once's
/// Temp_bool_IsClosed_Variable (`closed`). The `ext` object exists from the begin with an Instigator or from the
/// first hit (`made`); an entry with `BuffExt::None` reads as no extension.
#[derive(crate::State, Debug, Default)]
pub struct GauntletState {
    pub from: Option<FighterId>,
    pub closed: bool,
}

/// The loop's key (`b.cls + ':' + f.id`).
fn loop_key(w: &World, f: FighterId, class: BuffClass) -> SoundKey {
    let tag = match class {
        BuffClass::BuffAllyElectricGauntlets => name!("Buff_AllyElectricGauntlets_C:"),
        BuffClass::BuffGarnetElecInsideBubble => name!("Buff_GarnetElecInsideBubble_C:"),
        BuffClass::BuffGarnetElecOutsideBubble => name!("Buff_GarnetElecOutsideBubble_C:"),
        BuffClass::BuffGarnetFullPowerGauntlets => name!("Buff_GarnetFullPowerGauntlets_C:"),
        BuffClass::BuffSupermanAllyIceGauntlet => name!("Buff_SupermanAllyIceGauntlet_C:"),
        _ => name!("Buff_FireGauntlets_C:"),
    };
    SoundKey { tag, n: w.fighter(f).id }
}

/// A string of the class data (`d.k`, posted when non-empty).
fn data_str(w: &World, d: Option<&Props>, k: &str) -> Option<Name> {
    d.and_then(|d| d.get(k)).and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| data_name(w, s))
}

/// The class's defaults (`buffDef(f, b.cls)`).
fn class_def(w: &World, f: FighterId, class: BuffClass) -> Option<&Props> {
    let n = w.names.get(class.registered_name()?)?;
    buff_def(w, ActorRef::Fighter(f), n)
}

fn sound(w: &mut World, f: FighterId, ev: Name, x: Real, y: Real) {
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter,
        ev,
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

fn begin(w: &mut World, _f: FighterId, b: BuffRef, from: Option<FighterId>, _: Option<Real>) {
    if from.is_some()
        && let Some(x) = buff_mut(w, b)
    {
        x.ext = BuffExt::Gauntlet(GauntletState { from, closed: false });
    }
}

fn on(w: &mut World, f: FighterId, b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(vic) = a.victim.filter(|_| sig == BuffSignal::HitEnemy) else { return false };
    // The do-once @1789-1825 (Temp_bool_IsClosed_Variable, never reset): the first hit alone, whatever ExpireOnHit
    // says.
    let Some(x) = buff_mut(w, b) else { return false };
    if !matches!(x.ext, BuffExt::Gauntlet(_)) {
        x.ext = BuffExt::Gauntlet(GauntletState::default());
    }
    let BuffExt::Gauntlet(g) = &mut x.ext else { unreachable!("set above") };
    if g.closed {
        return false;
    }
    g.closed = true;
    let from = g.from;
    let cls = x.cls;
    // (ExpireOnHit as a graph wrote it: Mutator_Boost_IceGauntlets @246)
    let (apply, stacks, hit_sfx, expire) = {
        let d = inst_view(w, b);
        (
            d.get("BuffToApplyEnemy").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| data_name(w, s)),
            d.num("StacksToApplyEnemy").map_or(1, crate::math::to_int32),
            d.get("SFX Hit").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| data_name(w, s)),
            d.flag("ExpireOnHit"),
        )
    };
    // The holder's Comp_State_Manager makes the call (Buff_SupermanAllyIceGauntlet @303-374): its NewBuffSpawned /
    // BuffStacksAdded (Buff_Garnet_ElectricGroove on a Garnet holding the gauntlets).
    if let Some(c) = apply {
        spawn_buff(w, vic, c, 0.0, Some(stacks), from, true, Some(f));
    }
    let (fid, vid) = (w.fighter(f).id, w.fighter(vic).id);
    w.fighter_mut(f).events.push(Event::GauntletHit { fighter: fid, cls, victim: vid });
    if let Some(ev) = hit_sfx {
        let (vx, vy) = (w.fighter(vic).body.x, actor_y(w, vic));
        sound(w, f, ev, vx, vy);
    }
    expire
}

fn status(w: &mut World, f: FighterId, class: BuffClass, _stacks: Real, applied: bool, removed: bool) {
    let d = class_def(w, f, class);
    let equip = d.and_then(|d| d.get("PlayEquipSound")).is_some_and(truthy);
    let (start, end) = (data_str(w, d, "SFX Start"), data_str(w, d, "SFX End"));
    let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
    if applied
        && equip
        && let Some(ev) = start
    {
        sound(w, f, ev, x, y);
    }
    if removed && let Some(ev) = end {
        sound(w, f, ev, x, y);
    }
}

fn end(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(class) = buff(w, b).map(|x| x.class) else { return };
    let ev = data_str(w, class_def(w, f, class), "SFX_Loop_End");
    if let Some(ev) = ev {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        sound(w, f, ev, x, y);
    }
}

/// The electric gauntlet's begin: the parent's ReceiveBeginPlay (BuffParentActor's, then the class's @550) ahead of
/// the child's own: the loop sound is the BuffParentActor's BeginPlay, the Instigator the class's.
fn electric_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, dur: Option<Real>) {
    let Some(class) = buff(w, b).map(|x| x.class) else { return };
    if let Some(ev) = data_str(w, class_def(w, f, class), "SFX_Loop_Start") {
        let (fighter, key) = (w.fighter(f).id, loop_key(w, f, class));
        w.fighter_mut(f).events.push(Event::SoundStart { fighter, ev, key });
    }
    begin(w, f, b, from, dur);
}

fn electric_end(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(class) = buff(w, b).map(|x| x.class) else { return };
    let d = class_def(w, f, class);
    let (start, stop, end) =
        (data_str(w, d, "SFX_Loop_Start"), data_str(w, d, "SFX_Loop_Stop"), data_str(w, d, "SFX_Loop_End"));
    let (fighter, key) = (w.fighter(f).id, loop_key(w, f, class));
    // cut: SFX_Loop_Stop is posted on the buff actor itself (StopLoopSound @4957), so its Play part takes the loop's
    // key and the StopAll below cuts it before it is heard. SFX_Loop_End goes on a pooled component
    // (PF_Post_Event_At_Location @5132) and plays on.
    if let Some(ev) = stop {
        w.fighter_mut(f).events.push(Event::SoundStop {
            fighter,
            key,
            fade: None,
            ev: Some(ev),
            curve: None,
            cut: true,
        });
    }
    // The buff actor's destroy: PF_Post_Event's StopWhenOwnerDestroyed StopAll, no fade. Garnet's glow loops'
    // SFX_Loop_Stop stops only the zone's charge loop (bank Stop action 1067081683), so this is what ends them.
    if start.is_some() {
        w.fighter_mut(f).events.push(Event::SoundStop {
            fighter,
            key,
            fade: Some(0.0),
            ev: None,
            curve: None,
            cut: false,
        });
    }
    if let Some(ev) = end {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        sound(w, f, ev, x, y);
    }
}

/// The six classes (`buffs({ ... })`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    let plain =
        BuffHooks { begin: Some(begin), on: Some(on), status: Some(status), end: Some(end), ..BuffHooks::default() };
    let electric = BuffHooks { begin: Some(electric_begin), end: Some(electric_end), ..plain };
    for (c, h) in [
        (C::BuffSupermanAllyIceGauntlet, plain),
        (C::BuffAllyElectricGauntlets, electric),
        (C::BuffFireGauntlets, plain),
        (C::BuffGarnetElecInsideBubble, electric),
        (C::BuffGarnetElecOutsideBubble, electric),
        (C::BuffGarnetFullPowerGauntlets, electric),
    ] {
        r.buffs.enter(c.registered_name().expect("a registered class"), BuffEntry::new(c, h))?;
    }
    Ok(())
}
