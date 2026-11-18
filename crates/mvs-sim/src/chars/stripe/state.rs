//! Stripe's own fighter state (`StripeState`): Pawn_C023B's own variables, those
//! of its components (Comp_C023B_TargetTracker, Comp_C023B_Bomb, the live part of Comp_C023B_EnemyAttach) and of
//! C023B_AnimBP the sim reads, and the live actors he owns. Offsets are Pawn_C023B's ubergraph's unless named. One
//! record per class file, with its class defaults; the fields every file reads are `StripeState`'s own. A spawned
//! actor's own variables are on the actor. Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ProjectileId};
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix, SlotPlayers};
use crate::names::Name;
use crate::world::World;

/// `StripePawnState` (pawn.rs): BeginPlay's binds @971-1945, the gun handlers @2020-2458, LastDecay @2538-2649, Set
/// COM Angle, BPI_C023B, Com_Fall_Override; C023B_AnimBP's sim slot mix.
#[derive(State, Debug)]
pub struct StripePawnState {
    /// C023B_AnimBP's slot graph mix, with its timed BlendListByBool nodes (`slot_blend`: _2 on IsFalling, _1 on
    /// IsAttached) and its SequencePlayer and untimed list (`slot_players`: C023B_Special_Bite_Loop, OverrideKB).
    pub slot_mix: SlotMix,
    /// OverrideKB as the anim instance's last update read it: the AltKB LinkedAnimGraph (pawn.rs anim_update, the
    /// anim_set hook).
    pub ev_kb_override: bool,
    /// COM_Override_Angle, COM_Override_Alpha, COM_BlendStartTime (s of world time), COM_BlendTime
    /// (BPI_C023B_Set_COM_Angle @11789, AddPauseTime @12156).
    pub com_angle: Real,
    pub com_alpha: Real,
    pub com_blend_start: Real,
    pub com_blend_time: Real,
    /// COM_Fall_Override_On, COM_FallOverride (BPI_Override_Fall_COM, the update @9236).
    pub fall_on: bool,
    pub com_fall: Real,
    /// ChainSawOverride, ChainSawRot (SetOverrideChainsawMulticast @12231, the update @9534).
    pub chain_saw_on: bool,
    pub chain_saw_rot: Real,
    /// What the last update left for the graph's evaluation: ModifyBone_9's alpha and pitch, ModifyBone_7's on /
    /// pitch, ModifyBone_5's on / pitch.
    pub ev_com_alpha: Real,
    pub ev_com_pitch: Real,
    pub ev_fall_on: bool,
    pub ev_fall: Real,
    pub ev_saw_on: bool,
    pub ev_saw_rot: Real,
}

impl Default for StripePawnState {
    fn default() -> Self {
        StripePawnState {
            slot_mix: SlotMix {
                slot_blend: Some(SlotBlend::default()),
                slot_players: Some(SlotPlayers::default()),
                ..SlotMix::default()
            },
            ev_kb_override: false,
            com_angle: 0.0,
            com_alpha: 0.0,
            com_blend_start: 0.0,
            com_blend_time: 0.0,
            fall_on: false,
            com_fall: 0.0,
            chain_saw_on: false,
            chain_saw_rot: 0.0,
            ev_com_alpha: 0.0,
            ev_com_pitch: 0.0,
            ev_fall_on: false,
            ev_fall: 0.0,
            ev_saw_on: false,
            ev_saw_rot: 0.0,
        }
    }
}

/// `StripeSawState` (saw.rs): empty.
#[derive(State, Debug, Default)]
pub struct StripeSawState {}

/// `StripeGunState` (gun.rs): Comp_C023B_TargetTracker's variables.
#[derive(State, Debug, Default)]
pub struct StripeGunState {
    /// TargetTimer (K2_SetTimer(TargetTimerFunction, TimeTargeted 1.5) @618): 0 invalid, 1 pending, 2 active,
    /// 3 paused, 4 fired; the time it was started with; the deltas counted since it went active.
    pub timer: i32,
    pub rate: crate::Clock,
    pub elapsed: crate::Clock,
    /// The fighters whose OnExitedArena the tracker bound to OnTargetExitArena (@557).
    pub exit_bound: Vec<FighterId>,
    /// The spawned actors whose OnEndPlay it bound to OnTargetDeath (@796).
    pub end_bound: Vec<ProjectileId>,
}

/// `StripeBombState` (bomb.rs): Comp_C023B_Bomb's variables; its first three fields every file reads.
#[derive(State, Debug)]
pub struct StripeBombState {
    /// Pawn_C023B HasBomb (EquipBombVisuals @720 -> BPI_C023B_SetBombValue @819).
    pub equipped: bool,
    /// BombTimer running: kept equal to `timer > 0`.
    pub active: bool,
    /// BombTimer: the pawn tick of the timer pass it fires in, 0 none; the handle valid; the tick it was set in and
    /// its Rate.
    pub timer: i32,
    pub timer_valid: bool,
    pub timer_set: i32,
    pub timer_rate: Real,
    /// TimeRemainingOnDrop (DropBomb @82).
    pub time_remaining_on_drop: Real,
    /// The TimerEmitter's TimerOffset and the pawn tick it was spawned in (-1: none). The view's.
    pub fuse_offset: Real,
    pub fuse_at: i32,
    /// The delegates the component binds (AddUnique): EventKnockedBack (SetBomb @353), HitSound (@2225),
    /// TurnOffHitBoxes (@2477), UpdateHitOnBounce (@2612).
    pub knocked_bound: bool,
    pub hit_sound_bound: bool,
    pub stun_end_bound: bool,
    pub bounce_bound: bool,
    /// C023B_AnimBP OverrideKB (BPI_C023B_SetKnockBackOverride: ExplosionLaunch @2345, TurnOffHitBoxes @330).
    pub kb_override: bool,
    /// 'Hit Box Damage'.KnockBackDirectionVector as ExplosionLaunch @1547 / UpdateHitOnBounce @188 last wrote it (the
    /// build's axes); the class default (1, 1).
    pub body_dir_x: Real,
    pub body_dir_y: Real,
}

impl Default for StripeBombState {
    fn default() -> Self {
        StripeBombState {
            equipped: false,
            active: false,
            timer: 0,
            timer_valid: false,
            timer_set: 0,
            timer_rate: 0.0,
            time_remaining_on_drop: 0.0,
            fuse_offset: 0.0,
            fuse_at: -1,
            knocked_bound: false,
            hit_sound_bound: false,
            stun_end_bound: false,
            bounce_bound: false,
            kb_override: false,
            body_dir_x: 1.0,
            body_dir_y: 1.0,
        }
    }
}

/// `StripePerkState` (perks.rs).
#[derive(State, Debug, Default)]
pub struct StripePerkState {
    /// Buff_C023B_Leap LaunchNextLeap: false at each new DashListener, true at a BeforeLeapInitiated of his team.
    pub launch_next_leap: bool,
}

/// `StripeState`.
#[derive(State, Debug, Default)]
pub struct StripeState {
    pub pawn: StripePawnState,
    pub saw: StripeSawState,
    pub gun: StripeGunState,
    pub bomb: StripeBombState,
    pub perks: StripePerkState,
    /// LastDecay (Name): UpdateLastDecayMulticast @2538-2649 (pawn.rs); read by C023B_ChainsawAttackHitCondition
    /// (dash.rs). `Name::NONE` is None.
    pub last_decay: Name,
    /// Comp_C023B_TargetTracker's current target (gun.rs): a fighter or a spawned actor (the saw).
    pub target: Option<ActorRef>,
    /// C023B_AnimBP AimBack and the aim direction the TargetTracker sets (gun.rs writes).
    pub aim_back: Real,
    pub aim_direction: Real,
    /// ChainsawVisibilityMI (pawn.rs writes).
    pub chainsaw_visible: bool,
    /// The live C023B_DashListener (dash.rs).
    pub dash_listener: Option<ActorRef>,
    /// The live saws (saw.rs).
    pub saws: Vec<ProjectileId>,
}

// ---- the call surface of the state (`sp`, `spOf`)

/// `spOf`: the fighter's Stripe state, `None` for a fighter of another character (a failed cast to Pawn_C023B /
/// BPI_C023B).
pub fn stripe_of(w: &World, f: FighterId) -> Option<&StripeState> {
    match &w.fighter(f).cs {
        CharState::Stripe(s) => Some(s),
        _ => None,
    }
}

/// `spOf`, to write.
pub fn stripe_mut(w: &mut World, f: FighterId) -> Option<&mut StripeState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Stripe(s) => Some(s),
        _ => None,
    }
}

/// `sp` (his own code's read: the fighter is a Stripe). A panic is a hook entered for a fighter of another character.
pub fn stripe(w: &World, f: FighterId) -> &StripeState {
    stripe_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Stripe"))
}

/// `sp`, to write.
pub fn stripe_w(w: &mut World, f: FighterId) -> &mut StripeState {
    stripe_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Stripe"))
}
