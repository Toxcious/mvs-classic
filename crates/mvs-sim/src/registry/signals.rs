//! The signals the registry's hooks take by name: a buff's (`BuffSignal`), an
//! actor buff's (`ActorBuffSignal`), a fighter's bound ones (`FighterSignal`), an owner's to its actors
//! (`OwnerSignal`), a condition class's (`CondSignal`).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::BuffRef;
use crate::fighter::FighterId;
use crate::names::Name;

/// `BuffSignal`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum BuffSignal {
    BeforeLeapInitiated,
    BfLanded,
    BuffStunEnded,
    HitStunEnded,
    KnockedBack,
    HitByEnemy,
    HitByAlly,
    DamageHitByAlly,
    HitEnemy,
    TempHealthAdded,
    TempHealthPackRemoved,
    TempHealthFullyRemoved,
    ArmorBreak,
    ExitedArena,
    GiantCrouchChanged,
    EvadeTriggered,
    DashTriggered,
    InputPreTriggered,
    IsJumping,
    CrouchChange,
    BfDefeated,
    PlayerKnockedOut,
    AfterSpawnedActor,
    ItemThrown,
    EvadedAttack,
    AfterEvadedAttack,
    IsBouncing,
    FastFallLanded,
    HitBoxesDeactivated,
    MontageEnded,
    AfterActorTeleported,
    MoveCommandCompleted,
    FighterInputTriggered,
    NewBuffSpawned,
    BuffStacksAdded,
    ConditionalBuffUpdate,
    HitPauseTimeChanged,
    IsAttackingStart,
    IsAttackingEnd,
    HitAllyProjectile,
    HitAnything,
}

/// `BuffSignalArg`: what a signal carries, each part present for the signals that send it.
#[derive(Clone, Debug, Default)]
pub struct BuffSignalArg {
    pub crouched: Option<bool>,
    pub dir: Option<Real>,
    pub duration: Option<Real>,
    pub input: Option<i32>,
    pub value: Option<bool>,
    pub victim: Option<FighterId>,
    pub ally_actor: Option<ActorRef>,
    pub hit_actor: Option<ActorRef>,
    pub damage: Option<Real>,
    pub actor: Option<ActorRef>,
    pub pack: Option<Name>,
    pub bounce: Option<Real>,
    pub dealer: Option<FighterId>,
    pub pierce: Option<bool>,
    pub base_kb: Option<Real>,
    pub hit_dir: Option<[Real; 2]>,
    pub height: Option<Real>,
    pub montage: Option<Name>,
    pub interrupted: Option<bool>,
    pub teleported: Option<ActorRef>,
    pub found: Option<bool>,
    pub presser: Option<FighterId>,
    pub team: Option<i32>,
    pub buff_cls: Option<Name>,
    pub buff_owner: Option<FighterId>,
    pub stacks: Option<Real>,
    pub source: Option<BuffRef>,
    // Field (T9): `hit`, the hit packet.
    pub hit: Option<Box<crate::combat::HitPacket>>,
    /// `by`: EvadedAttack / AfterEvadedAttack's dodged actor; a hit's KnockedBack, HitByEnemy
    /// and the like: `actorBy` of the hit's actor.
    pub by: Option<crate::fighter::control::DodgeBy>,
}

/// `ActorBuffSignal`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum ActorBuffSignal {
    HitAnything,
    Reflected,
    HitByEnemy,
    HitEnemyPlayer,
    HitEnemyPlayerFirst,
    BasketBallPickedUp,
    HitboxOverlap,
    FollowerHitReactStart,
    FollowerHitReactEnd,
    FollowerIsAttackingStart,
    FollowerIsAttackingEnd,
}

/// `FighterSignal`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum FighterSignal {
    Facing,
    ExitedArena,
    TakeAnyDamage,
    AdjustedVisibility,
    UpdatedSubMesh,
    AbilityCooldownUpdated,
    InputTriggered,
    NewBuffSpawned,
}

/// `OwnerSignal`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum OwnerSignal {
    Facing,
    Jumping,
    WallSliding,
}

/// `CondSignal`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum CondSignal {
    HitEnemyLight,
    HitNonPlayer,
}

/// `ActorBuffArg`: an actor buff signal's argument.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ActorBuffArg {
    pub by: crate::fighter::FighterId,
    pub kx: crate::Real,
    pub ky: crate::Real,
}
