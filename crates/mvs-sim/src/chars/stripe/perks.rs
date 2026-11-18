//! Stripe's perks: Buff_C023B_Dynamite (PerkGroup_C023B_Shockwave),
//! Buff_C023B_Leap (BeforeLeapInitiated, C023B_DashProjectileBlock) and Buff_C023B_PerkAttach ("Feeding After
//! Midnight"; Buff_TransferDebuffs is shared: `combat::transfer_debuffs`). bomb.rs and dash.rs call its listeners
//! directly (`selfBounce.listener` and `onAttachedToEnemy`). All three perks are self-only (SpawnOnAllies
//! false), spawned with duration 0 (no life timer) at each possession. Perk_C023B_Leap has no TargetCharacter.
//!
//! Buff_C023B_Dynamite: ReceiveBeginPlay @320, authority @330: the owner's Comp_C023B_Bomb valid @141 (else Delay(0)
//! @15 and again) -> OnSelfBounce += OnExplosionBounce @184-278. OnExplosionBounce @365: the component valid @465 ->
//! CreateBombExplosion(Comp_Pawn_HitBoxHandler) @546 (`bomb::create_bomb_explosion`: the explosion at his place, the
//! enemies in 250 hit by his handler). OnSelfBounce is broadcast by the component's UpdateHitOnBounce @419 on a bounce
//! he instigated himself. Every Stripe has the component from his spawn, so the binding is made at the BeginPlay.
//! Unverified: the binding is taken as the live buff: the delegate keeps no list of its own here (one Dynamite per
//! Stripe).
//!
//! Buff_C023B_Leap: ReceiveBeginPlay @189, authority @199: the owner's Comp_Spawner valid -> AfterSpawnedActor +=
//! OnSpawnedActor @418-441; its Comp_State_Manager valid -> NewBuffSpawned += AfterBuffSpawned @596-690.
//! LaunchNextLeap is false on a new actor (set false here). OnSpawnedActor @912: a C023B_DashListener -> @55
//! LaunchNextLeap = false @65, BeforeLeapInitiated += BeforeActorLeap @105-128. AfterBuffSpawned @736: a
//! C023B_DashProjectileBlock -> BeforeLeapInitiated += BeforeBuffLeap @815-838. BeforeActorLeap @996 / BeforeBuffLeap
//! @884 -> BeforeLeap(ProjectileTeamIndex) @1024: the owner's Comp_Actor_Destructible valid, its TeamIndex ==
//! ProjectileTeamIndex @1209 -> @174 -> @10 LaunchNextLeap = true. GetUseLeap is LaunchNextLeap (dash.rs's LongLeap
//! conditions).
//! Unverified: the binding is taken as given for every listener and block of the owner while the buff lives: one
//! spawned ahead of the buff's BeginPlay (none in a match: the perk buff is spawned at the possession) would not reach
//! it in the build.
//!
//! Buff_C023B_PerkAttach: ReceiveBeginPlay @284, authority @294: the owner's Comp_C023B_EnemyAttach valid @399 ->
//! AttachedToEnemy += OnEnemyAttached @442-536. OnEnemyAttached(AttachedEnemy) @582 -> @10: the owner's
//! Comp_State_Manager valid -> SpawnBuffActor(Buff_TransferDebuffs_C, AttachedEnemy, the owner, 0, 0.0, true, false)
//! @195: on the enemy, Stripe its Instigator, the call made by Stripe's manager (`combat::transfer_debuffs` moves
//! Stripe's debuffs to the enemy). AttachedToEnemy is broadcast by SendAttachedDispatch (dash.rs: the DashListener's
//! HitEnemy @972).

use super::bomb::create_bomb_explosion;
use super::dash::DASH_LISTENER;
use super::state::{stripe_mut, stripe_of};
use crate::actors::{ActorRef, WorldActorState};
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffRef, buff_of, spawn_buff};
use crate::fighter::FighterId;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const DYNAMITE: Name = name!("Buff_C023B_Dynamite_C");
const LEAP: Name = name!("Buff_C023B_Leap_C");
const PERK_ATTACH: Name = name!("Buff_C023B_PerkAttach_C");
const TRANSFER_DEBUFFS: Name = name!("Buff_TransferDebuffs_C");

/// The bomb's self bounce's listener (`selfBounce.listener`): Dynamite's OnExplosionBounce. Called by bomb.rs.
pub fn self_bounce(w: &mut World, f: FighterId) {
    if buff_of(w, f, DYNAMITE).is_some() {
        create_bomb_explosion(w, f);
    }
}

/// The attach's listeners (`onAttachedToEnemy` entries, by SendAttachedDispatch): PerkAttach's
/// OnEnemyAttached for each of the owner's live PerkAttach buffs. Called by dash.rs.
pub fn attached_to_enemy(w: &mut World, f: FighterId, target: FighterId) {
    let n = w.fighter(f).buffs.buffs.iter().filter(|b| b.cls == PERK_ATTACH && b.frames > 0.0).count();
    for _ in 0..n {
        spawn_buff(w, target, TRANSFER_DEBUFFS, 0.0, Some(0), Some(f), true, Some(f));
    }
}

/// Leap's ReceiveBeginPlay: LaunchNextLeap false.
fn leap_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _dur: Option<crate::Real>) {
    if let Some(s) = stripe_mut(w, f) {
        s.perks.launch_next_leap = false;
    }
}

/// Leap's AfterSpawnedActor (a DashListener: false) and BeforeLeapInitiated (the owner's team: true).
fn leap_on(w: &mut World, f: FighterId, _b: BuffRef, sig: BuffSignal, a: &BuffSignalArg) -> bool {
    if stripe_of(w, f).is_none() {
        return false;
    }
    match sig {
        BuffSignal::AfterSpawnedActor => {
            if let Some(ActorRef::Actor(id)) = a.actor
                && let Some(x) = w.actors.actors.get(id)
                && matches!(x.state, WorldActorState::StripeListener(_))
                && w.names.str(x.cls) == DASH_LISTENER
                && let Some(s) = stripe_mut(w, f)
            {
                s.perks.launch_next_leap = false;
            }
        }
        BuffSignal::BeforeLeapInitiated => {
            if a.team == Some(w.fighter(f).team.team)
                && let Some(s) = stripe_mut(w, f)
            {
                s.perks.launch_next_leap = true;
            }
        }
        // NewBuffSpawned with C023B_DashProjectileBlock_C: the binding alone (above).
        _ => {}
    }
    false
}

/// This file's registry entries (Buff_C023B_Dynamite_C, Buff_C023B_Leap_C, Buff_C023B_PerkAttach_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter("Buff_C023B_Dynamite_C", BuffEntry::new(BuffClass::BuffC023BDynamite, BuffHooks::default()))?;
    r.buffs.enter(
        "Buff_C023B_Leap_C",
        BuffEntry::new(
            BuffClass::BuffC023BLeap,
            BuffHooks { begin: Some(leap_begin), on: Some(leap_on), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter("Buff_C023B_PerkAttach_C", BuffEntry::new(BuffClass::BuffC023BPerkAttach, BuffHooks::default()))
}

/// `LEAP`, for dash.rs's conditions.
pub const LEAP_CLASS: Name = LEAP;
