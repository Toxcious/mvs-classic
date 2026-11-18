//! Buff_Damage_Electric, the electric debuff: a chain of 1-damage hits every 0.8 s from its
//! owner to the fighters around it. Given by Creature's SeismicReactor (2 stacks, 0.0 s, to an enemy), by
//! Buff_AllyElectricGauntlets' first hit (3 stacks, 0.0 s: `combat::gauntlet`), C021's Bolt and chain, and Garnet's
//! zone (Garnet not ported yet).
//!
//! Buff_Low_Damage_Electric_C, its one child (Wonder Woman's LassoProjectileFastV3 OnHitBuffs[0] and the
//! LassoGrapple perk's enemy overlap, `chars::wonder_woman::perks_lasso`), has no variable and no function of its
//! own: it runs this graph unchanged. Its class default overrides the Damage struct's Damage alone (0.5); the rest
//! of the struct is the parent's (the import merges a child default's struct over its parent's member by member:
//! `mvs_uasset::typed::merge_defaults`). Same entry, its own class.
//! The child (a data buff) has an entry of its own here: its pulses and its loop sound.
//!
//! Read: Buff_Damage_Electric (offsets are its ubergraph's), BuffParentActor (the loop sound).
//! Class defaults: IsDebuff, CanStack, MaxStacks 10, StackDecayRate 1 (no StackDecayBuffer: one stack a second, the
//! buff ends with its last one when spawned with 0.0 s; the parent's, `combat::buffs`), BuffDuration 5 (the spawners
//! pass their own), DamageIntervalTimer 0.8, ChainLightningRange 500, SourceTeamIndex -1, Damage: Damage 1, no
//! knockback, HitStunMult 0, HitPauseTime 0, ArmorPierce, AttackID 0, AttackDecayType 0, no OnHitAudio. The damage
//! per Trigger does not grow with CurrentStackAmount; a stack added to a live one (AddStack) starts no Trigger and no
//! timer (the BeginPlay is the spawn's).
//!
//! ReceiveBeginPlay @15: the parent's (StartLoopSoundMulticast: SFX_Loop_Start
//! sfx_shared_combat_electric_chain_buff_loop on the buff actor), ElectricVFX @1865 (PS_Garnet_Debuff_Electricity on
//! the owner: the `electricFx` loop event), then on authority SourceTeamIndex = the Instigator's
//! Comp_Actor_Destructible TeamIndex when that is valid @89-254 (else -1, which no fighter has: everyone in reach is
//! hit), Trigger() @303 and K2_SetTimerDelegate(Trigger, DamageIntervalTimer, looping) @340; the handle is not kept.
//!
//! Trigger @398: ChainedFighters cleared; SphereOverlapActors(the owner's location, ChainLightningRange, object type
//! Pawn, BP_FighterCharacter_C, nothing ignored) @533; for each actor found with a Comp_Actor_Destructible,
//! implementing BPI_BaseFighter, of a TeamIndex other than SourceTeamIndex @856-1192: AddUnique to ChainedFighters
//! @1261, then the buff actor's own Comp_Pawn_HitBoxHandler.pfgDoDamage(null, it, Damage, ElementalBuffs =
//! [Buff_Elemental_NoArmorDamage] for the owner itself and none for the others, no OnHitBuffs) @1521. The owner is in
//! the sphere: it is hit whenever it is not on the Instigator's team. After the loop TriggerVFX(ChainedFighters)
//! @1670 (a multicast; not on a dedicated server, @4287): per fighter, the owner -> sfx_cha_reindog_tether_hit on the
//! owner @2813 and PS_Debuff_ElectricityBurst on its mesh @3047; another -> PS_Garnet_ElecBeam from it to the owner
//! @3504-3982 and sfx_shared_combat_electric_chain_buff_tendril_cast on the owner @4060.
//!
//! ReceiveEndPlay @1773: the parent's (StopLoopSound: no SFX_Loop_Stop; SFX_Loop_End
//! sfx_shared_combat_electric_chain_buff_end at the owner's location @5132), then the Particle destroyed @1831. The
//! timer goes with the actor (AActor::EndPlay clears the object's timers; the class's timer hook runs only while the
//! buff lives). BPI On Fighter Owner Knocked Back is empty (@4286).
//!
//! The hit: pfgDoDamage on a handler whose owner is the buff actor, no hit box (`apply_projectile_hit` with the buff
//! actor as the dealer, as Buff_DelayKnockUp in `chars::batman::bomb`): no dealt mods or charge, no attack decay
//! (RegisterHitFromFighter finds no fighter or spawned actor), the light path (no knockback, no hitstun, no pause),
//! the Instigator credited (RecordDamage). An invulnerable fighter takes nothing (@4608; no dodge: BaseKnockBack 0).
//! A DamageDisabled one fails DamageAndLaunch's CanBeHitBy (@659): @718 -> @8047, its DamageManager's HitByAlly is
//! broadcast (whatever the team) and nothing else (`refused`). A pulled fighter has no
//! collision (`no_collision`): the overlap does not find it. The owner's NoArmorDamage skips projectile armor and
//! CalculateArmor (DamageAndLaunch @286 -> @479); the others' ArmorPierce breaks their armor packs (Comp_Armor
//! CalculateArmor @845). pfgDoDamage spawns the ElementalBuffs' actors on the victim (SpawnOnHitElementalBuffs @296)
//! and destroys them after the hit (@1386): Buff_Elemental_NoArmorDamage's graph is empty (every event returns), so
//! only its tag in DamageAndLaunch acts.
//!
//! Without an Instigator (a spawn that passes none) SourceTeamIndex stays -1 and every fighter in reach is hit, by
//! the same handler, the buff actor its PreviousAttacker: `buff_actor_light_hit` with no source (ArmorPierce, the
//! owner's NoArmorDamage).
//! Unverified: that path is the Thorns / fire pulse model of a buff actor's light hit (combo decay by combo
//! resistance alone, no attack decay registered), not `apply_projectile_hit`'s; no ported spawner passes no
//! Instigator.
//!
//! A Trigger and the owner's last stack in the same timer tick: the Trigger first, then the stack timer's RemoveStack
//! destroys the buff (no Trigger after it). FTimerManager pops the lower ExpireTime: the Trigger's is T(spawn) + k x
//! float(0.8) (k = 1, 2, ...: ExpireTime += Rate), the stack timer's T(set) + 1.0 with T(set) the frame's clock 60
//! frames earlier (DELTA_SECONDS x 60 = 1.000008 s), which is k x 6.4e-6 - 8e-6 s behind the Trigger's: later for
//! every k >= 2, and k = 1 cannot tie (the stack timer is set at the spawn or after). The port has that order by its
//! layout: the stack timer of a timer tick is counted in the next frame's buff tick, the Trigger in this frame's class
//! timers. With 4 stacks the 6th Trigger (4.0 s) runs.
//! Deviation: the Trigger of the BeginPlay is run in the frame's timer pass (the class timers) after the call that
//! spawned the buff, not inside it: the same frame, after the tick groups.
//! Unverified: the overlap is the sphere against the fighters' root capsules in the play plane, in the sim's fighter
//! order (PhysX's order is not read).

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{
    BuffExt, BuffRef, buff, buff_actor_light_hit, buff_mut, buff_signal, hit_by_ally_bound, inst_def_of, inst_view,
};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{apply_projectile_hit, dealer};
use crate::combat::{BuffClass, HitPacket};
use crate::events::{ElectricKind, Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::control::data_name;
use crate::fighter::moves::V2;
use crate::fighter::{actor_y, invulnerable};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::{num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::{World, capsules_in_sphere};

pub const ELECTRIC: Name = name!("Buff_Damage_Electric_C");
pub const LOW_ELECTRIC: Name = name!("Buff_Low_Damage_Electric_C");
const NO_ARMOR: [Name; 1] = [name!("Buff_Elemental_NoArmorDamage_C")];

/// The class's own variables (`ElectricState`): SourceTeamIndex (`team`), the Instigator (`by`), the BeginPlay's
/// Trigger still owed (`now`), the owner's tick of the next timer Trigger (`at`).
#[derive(crate::State, Debug, Default)]
pub struct ElectricState {
    pub team: i32,
    pub by: Option<FighterId>,
    pub now: bool,
    pub at: Real,
}

/// The key of the loop on the buff actor: per class (a fighter can hold one of each, each with its own loop).
fn loop_key(w: &World, f: FighterId, b: BuffRef) -> SoundKey {
    let low = buff(w, b).is_some_and(|x| x.class == BuffClass::BuffLowDamageElectric);
    let tag = if low { name!("Buff_Low_Damage_Electric_C:") } else { name!("Buff_Damage_Electric_C:") };
    SoundKey { tag, n: w.fighter(f).id }
}

/// A sound name of the class data (`d.k`, a non-empty string).
fn data_str(w: &World, v: Option<&mvs_data::value::Value>) -> Option<Name> {
    v.and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| data_name(w, s))
}

/// The chain's hit on a DamageDisabled fighter, which DamageAndLaunch's CanBeHitBy refuses (@659): @718 -> @8047, the
/// DamageManager's HitByAlly (DamageHitByAlly, as `combat::buffs` light_hit_refused: no `by`, its DamageDealer the
/// buff actor, which no ported binder leaps off), whatever the team; then
/// pfgDoDamage's NotifyAllyOfHit @21344 (Comp_Pawn_HitBoxHandler @0-937), whose HitByAlly @584 needs a valid
/// Instigator, not the victim, of the victim's TeamIndex: never for a fighter the loop hits (another TeamIndex than
/// SourceTeamIndex, the Instigator's), kept for the order. Each sent while a buff of the fighter is bound to it.
/// Not ported: CalculateArmor @7560 (ahead of the team test for the others' hit, ArmorPierce), as light_hit_refused.
fn refused(w: &mut World, o: FighterId, h: &HitPacket, by: Option<FighterId>) {
    if hit_by_ally_bound(w, o) {
        let arg = BuffSignalArg { hit: Some(Box::new(h.clone())), ..BuffSignalArg::default() };
        buff_signal(w, o, BuffSignal::DamageHitByAlly, arg);
    }
    let Some(s) = by else { return };
    if s != o && w.fighter(s).team.team == w.fighter(o).team.team && hit_by_ally_bound(w, o) {
        let arg = BuffSignalArg { hit: Some(Box::new(h.clone())), ..BuffSignalArg::default() };
        buff_signal(w, o, BuffSignal::HitByAlly, arg);
    }
}

/// Trigger @398 and TriggerVFX @2500.
fn trigger(w: &mut World, f: FighterId, b: BuffRef, team: i32, by: Option<FighterId>) {
    let (h, range) = {
        let d = inst_def_of(w, b);
        let d = d.as_deref();
        (
            d.and_then(|d| obj(d, "Damage")).map(|p| HitPacket::from_props(&w.names, p, None)).unwrap_or_default(),
            d.and_then(|d| num(d, "ChainLightningRange")).unwrap_or(0.0),
        )
    };
    let all = w.pawns.as_deref().unwrap_or(&w.players).to_vec();
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let mut chained: Vec<FighterId> = Vec::new();
    for o in capsules_in_sphere(w, &all, fx, fy, range) {
        if w.fighter(o).team.team == team {
            continue;
        }
        if !chained.contains(&o) {
            chained.push(o);
        }
        if invulnerable(w, o) {
            continue;
        }
        if w.fighter(o).invuln.damage_disabled {
            refused(w, o, &h, by);
            continue;
        }
        let (ox, oy) = (w.fighter(o).body.x, actor_y(w, o));
        if by.is_none() {
            buff_actor_light_hit(w, f, b, o, None, h.damage.unwrap_or(0.0), h.armor_pierce.unwrap_or(false), o == f);
            continue;
        }
        // The buff actor's handler, at the victim (the dealer's x, y), its AttackID the Damage packet's.
        let mut d = dealer(w, ActorRef::Buff(b));
        d.x = ox;
        d.y = oy;
        d.attack_id = h.attack_id.unwrap_or(0);
        // HitLocation of a null HitCollision: the victim's capsule.
        let el: &[Name] = if o == f { &NO_ARMOR } else { &[] };
        apply_projectile_hit(w, &d, o, &h, &[], V2 { x: ox, y: oy }, None, el, 0.0);
    }
    let fid = w.fighter(f).id;
    for o in chained {
        if o == f {
            w.events.push(Event::Sound {
                fighter: fid,
                ev: name!("sfx_cha_reindog_tether_hit"),
                x: None,
                y: None,
                hit: None,
                pooled: false,
                key: None,
            });
            w.events.push(Event::ElectricFx {
                fighter: fid,
                kind: ElectricKind::Burst,
                on: None,
                target: None,
                x: None,
                y: None,
            });
        } else {
            let target = Some(w.fighter(o).id);
            w.events.push(Event::ElectricFx {
                fighter: fid,
                kind: ElectricKind::Beam,
                on: None,
                target,
                x: Some(fx),
                y: Some(fy),
            });
            w.events.push(Event::Sound {
                fighter: fid,
                ev: name!("sfx_shared_combat_electric_chain_buff_tendril_cast"),
                x: None,
                y: None,
                hit: None,
                pooled: false,
                key: None,
            });
        }
    }
}

fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _: Option<Real>) {
    let (start, iv, src_team) = {
        let d = inst_view(w, b);
        (data_str(w, d.get("SFX_Loop_Start")), d.num("DamageIntervalTimer"), d.num("SourceTeamIndex"))
    };
    let fid = w.fighter(f).id;
    if let Some(ev) = start {
        let key = loop_key(w, f, b);
        w.fighter_mut(f).events.push(Event::SoundStart { fighter: fid, ev, key });
    }
    w.fighter_mut(f).events.push(Event::ElectricFx {
        fighter: fid,
        kind: ElectricKind::Loop,
        on: Some(true),
        target: None,
        x: None,
        y: None,
    });
    // Set inside the spawn (the tick groups): the timer is pending and counts from this frame's timer pass
    // (timer_ticks). A looping timer re-arms by ExpireTime += Rate: timer_ticks(0.8) = 48 frames each, exact for the
    // first 2600 Triggers.
    let team = match from {
        Some(o) => w.fighter(o).team.team,
        None => src_team.map_or(-1, crate::math::to_int32),
    };
    let at = crate::real(w.fighter(f).clock.tick) + timer_ticks(iv);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Electric(ElectricState { team, by: from, now: true, at });
    }
}

fn timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(BuffExt::Electric(e)) = buff(w, b).map(|x| &x.ext) else { return false };
    let (team, by, now) = (e.team, e.by, e.now);
    if now {
        if let Some(BuffExt::Electric(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.now = false;
        }
        trigger(w, f, b, team, by);
    }
    let Some(x) = buff(w, b) else { return false };
    let BuffExt::Electric(e) = &x.ext else { return false };
    if x.frames > 0.0 && crate::real(w.fighter(f).clock.tick) >= e.at {
        let iv = inst_view(w, b).num("DamageIntervalTimer");
        if let Some(BuffExt::Electric(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
            e.at += timer_ticks(iv);
        }
        trigger(w, f, b, team, by);
    }
    false
}

fn end(w: &mut World, f: FighterId, b: BuffRef) {
    let (start, end) = {
        let d = inst_view(w, b);
        (data_str(w, d.get("SFX_Loop_Start")), data_str(w, d.get("SFX_Loop_End")))
    };
    let (fid, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    if let Some(ev) = end {
        w.fighter_mut(f).events.push(Event::Sound {
            fighter: fid,
            ev,
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
    w.fighter_mut(f).events.push(Event::ElectricFx {
        fighter: fid,
        kind: ElectricKind::Loop,
        on: Some(false),
        target: None,
        x: None,
        y: None,
    });
    // The loop goes with the buff actor's AkComponent (StopWhenOwnerDestroyed, OnUnregister's StopAll:
    // UAkComponent::OnUnregister).
    // Unverified: StopAll taken as a stop without a fade.
    if start.is_some() {
        let key = loop_key(w, f, b);
        w.fighter_mut(f).events.push(Event::SoundStop {
            fighter: fid,
            key,
            fade: Some(0.0),
            ev: None,
            curve: None,
            cut: false,
        });
    }
}

/// The class's entry (`buffs({ [ELECTRIC]: ... })`) and its child's, with the same hooks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for class in [BuffClass::BuffDamageElectric, BuffClass::BuffLowDamageElectric] {
        let e = BuffEntry::new(
            class,
            BuffHooks { begin: Some(begin), timer: Some(timer), end: Some(end), ..BuffHooks::default() },
        );
        r.buffs.enter(e.class.registered_name().expect("a registered class"), e)?;
    }
    Ok(())
}
