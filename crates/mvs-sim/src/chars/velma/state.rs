//! Velma's own fighter state (`VelmaState`): Pawn_Velma's own variables (EFighter
//! 19), those of Velma_AnimBP the sim reads, and the live actors she owns, beside BaseFighter's. The police minion's
//! (Pawn_VelmaAIBotPolice, the `velmapolice` data) is [`VelmaPoliceState`], made by its own entry (police.rs).
//! Offsets are Pawn_Velma's ubergraph's. One record per group of files, with its class defaults; the fields several
//! files read are `VelmaState`'s own. The evidence count itself is Buff_EvidenceCount's (evidence.rs), and
//! Buff_Arrested's / Buff_Flatten's fields live on the victim's buff: none of them is a field here.
//! Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ProjectileId};
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix};
use crate::world::World;

/// `VelmaPawnState` (pawn.rs): BeginPlay's forks (UI_FinnCharge set-up @428, its Delay 0.1 retry @916, EndOfMatch,
/// SkinApplied), BPI_SetAimValue @7001, BPI_IsCarrying @6642, HeadScale, Cleanup, HitStunStart / End; Velma_AnimBP's
/// sim slot mix (root `not HitStun and IsCarryingAlly`, then IsCrouching, on the timed bool of `fighter::slot_blend`).
#[derive(State, Debug)]
pub struct VelmaPawnState {
    /// Velma_AnimBP's slot graph mix (`fighter::anim` graph_slots writes it from pawn.rs's slot_input), with its timed
    /// BlendListByBool nodes' state (`slot_blend`); a new pawn (mod.rs match_reset's new state) starts it at
    /// INDEX_NONE. `mixBuf` is no sim state.
    pub slot_mix: SlotMix,
}

impl Default for VelmaPawnState {
    fn default() -> Self {
        VelmaPawnState { slot_mix: SlotMix { slot_blend: Some(SlotBlend::default()), ..SlotMix::default() } }
    }
}

/// `VelmaEvidenceState` (evidence.rs, perks.rs): the timer @1336-1374, SpawnEvidence @1983-2316, Buff_EvidenceCount,
/// Buff_PoliceEvidence, VelmaEvidence (+ NoGrav), Buff_SpawnEvidence, Buff_Studied.
#[derive(State, Debug, Default)]
pub struct VelmaEvidenceState {
    /// The pawn's SpawnEvidence timer (K2_SetTimerDelegate @1374, looping): its Rate in seconds (float32; 0: not drawn
    /// yet).
    pub period: Real,
    /// The fighter tick it was set at.
    pub start: i32,
    /// The expiries so far.
    pub fired: i32,
    /// The fighter tick of its next expiry.
    pub timer_at: i32,
}

/// `VelmaPoliceCallState` (police.rs): AnimNotify_Velma_CallPolice, SpawnPoliceServer @5689-6037 (Delay 1 @5634,
/// Delay 8); the minion's own state is [`VelmaPoliceState`].
#[derive(State, Debug, Default)]
pub struct VelmaPoliceCallState {
    /// Pawn_Velma's two latent Delays of the call, in frames left (0: none pending): Delay 1 @5634 (-> @971
    /// AllowPoliceCall = true, then Delay 8 @982 -> @98 K2_DestroyActor of the latest spawn). police.rs velma_timers
    /// counts them.
    pub delay1: i32,
    pub delay8: i32,
    /// SpawnPoliceServer's CallFunc_SpawnAIFromClass_ReturnValue: the latest police spawned, the one the Delay 8
    /// destroys (a later call that the pending Delay 8 outlives does not move it: the node's own pin keeps the newest
    /// value).
    pub last_spawn: Option<FighterId>,
}

/// `VelmaBeamState` (beam.rs): VelmaHealBeam, VelmaChildBeam, Buff_VelmaHealOverTimeBlast / Buff_VelmaHealOverTime,
/// AnimNotify_ResetAnimBPAimDirection. Empty: the beam's state is its actor's.
#[derive(State, Debug, Default)]
pub struct VelmaBeamState {}

/// `VelmaMarkState` (mark.rs): VelmaMarkProjectile, Buff_VelmaSpawnHomingTarget, VelmaAimProjectile,
/// VelmamSassFollowUpProjectile, the Mark / NoMark conditions. The live mark is `VelmaState::mark`.
#[derive(State, Debug, Default)]
pub struct VelmaMarkState {
    /// The buff's CompSpawnActor is running (mark.rs `quietSpawn`): its AfterMarkCreated binding (HitBoxOn? false)
    /// runs inside the spawn, ahead of the test at the spawn place. Set and cleared inside one call: never held across
    /// a tick.
    pub quiet_spawn: bool,
}

/// `VelmaCarryState` (carry.rs): VelmaCarryRun_Actor, Buff_Velma_Carry_Speed.
#[derive(State, Debug, Default)]
pub struct VelmaCarryState {
    /// BaseFighter IsHoldingAlly on a Velma (BPI_GetIsHoldingAlly; class default false): VelmaCarryRun_Actor's
    /// ButtonPressed @7399 sets it, its EndPlay @10025 clears it. carry.rs is_holding_ally reads it
    /// (VelmaNotHoldingFighterCondition, capture.rs).
    pub holding_ally: bool,
}

/// `VelmaCaptureState` (capture.rs): Buff_VelmaAttachToAlly, Buff_Flatten (shared code),
/// VelmaNotHoldingFighterCondition. Empty: the capture's state is the victim's buff's.
#[derive(State, Debug, Default)]
pub struct VelmaCaptureState {}

/// `VelmaGadgetState` (gadgets.rs): VelmaVialAttack, SlowtoStunGrenade / Buff_SlowIntoStun, VelmaBookProjectile,
/// Velma_AirSideSp_BookBlast, Velma_SPAIR_CameraFlash, Velma_SassBubblev2. Empty: their state is their actors'.
#[derive(State, Debug, Default)]
pub struct VelmaGadgetState {}

/// `VelmaState`.
#[derive(State, Debug)]
pub struct VelmaState {
    /// pawn.rs.
    pub pawn: VelmaPawnState,
    /// evidence.rs.
    pub evidence: VelmaEvidenceState,
    /// police.rs.
    pub call: VelmaPoliceCallState,
    /// beam.rs.
    pub beam: VelmaBeamState,
    /// mark.rs.
    pub marks: VelmaMarkState,
    /// carry.rs.
    pub carry: VelmaCarryState,
    /// capture.rs.
    pub capture: VelmaCaptureState,
    /// gadgets.rs.
    pub gadgets: VelmaGadgetState,

    // ---- the fields every file reads (writers in brackets)
    /// Velma_AnimBP AimDirection: BPI_SetAimDirection @9247 (AimDirectionInverted = AimDirection x -1 @9274, pawn.rs
    /// derives it), from the pawn's BPI_SetAimValue @7001 -> @6598 -> @6419 -> @6551 (MapRangeClamped(Value, -30, 30,
    /// -1, 1)). Callers through pawn.rs set_aim_value: VelmaHealBeam @5038 (beam.rs);
    /// AnimNotify_ResetAnimBPAimDirection (beam.rs). The AnimBP is the pawn's mesh's own: its variables at the class
    /// defaults (0 / false) when the pawn is made.
    pub aim_direction: Real,
    /// Velma_AnimBP IsCarryingAlly: BPI_IsCarrying @9344 (false again @9387), from the pawn's BPI_IsCarrying @6642 ->
    /// @6717. Caller through pawn.rs set_carrying: VelmaCarryRun_Actor's nine BPI_IsCarrying calls (carry.rs). The
    /// slot mix's root reads `not HitStun and IsCarryingAlly` (@5550).
    pub carrying_ally: bool,
    /// The AnimBP's HitStun, from the pawn's HitStunStart / HitStunEnd (pawn.rs writes it; the slot mix's root reads
    /// it).
    pub anim_hit_stun: bool,
    /// Pawn_Velma AllowPoliceCall (CDO true): SpawnPoliceServer reads it @5689 and clears it @6026 (police.rs).
    pub allow_police_call: bool,
    /// The live police minion (Pawn_VelmaAIBotPolice, a minion fighter of the `velmapolice` data: `spawn_minion`),
    /// none when none (police.rs writes it at the spawn @5963 / MulticastBotPoliceSetup @6037 and clears it at its
    /// end). The view and the bots read it.
    pub police: Option<FighterId>,
    /// The evidence count the pawn caches for UI_FinnCharge (EvidenceCount, BPI_SetEvidenceCount @1037;
    /// Buff_EvidenceCount calls it). evidence.rs writes it, the HUD reads it (UpdateHighFiveProgressBar).
    pub evidence_ui: Real,
    /// The live VelmaCarryRun_Actor (a world actor), none when none. carry.rs writes it; capture.rs and the bots read
    /// it.
    pub carry_run: Option<ActorRef>,
    /// The live VelmaMarkProjectile, none when none. mark.rs writes it; MarkProjectileVelmaCondition (mark.rs) and
    /// gadgets.rs's book read it.
    pub mark: Option<ProjectileId>,
    /// GlassesOffCount (@2891-3277: BPI_SetGlasses @6089, AnimNotify_VisionGlasses): pawn.rs's glasses_off /
    /// glasses_on write it, the HUD reads it (the GlassesOff widget, up while it is above 0).
    pub glasses_off_count: i32,
}

impl Default for VelmaState {
    fn default() -> Self {
        VelmaState {
            pawn: VelmaPawnState::default(),
            evidence: VelmaEvidenceState::default(),
            call: VelmaPoliceCallState::default(),
            beam: VelmaBeamState::default(),
            marks: VelmaMarkState::default(),
            carry: VelmaCarryState::default(),
            capture: VelmaCaptureState::default(),
            gadgets: VelmaGadgetState::default(),
            aim_direction: 0.0,
            carrying_ally: false,
            anim_hit_stun: false,
            allow_police_call: true,
            police: None,
            evidence_ui: 0.0,
            carry_run: None,
            mark: None,
            glasses_off_count: 0,
        }
    }
}

/// `vmOf`.
pub fn velma_of(w: &World, f: FighterId) -> Option<&VelmaState> {
    match &w.fighter(f).cs {
        CharState::Velma(s) => Some(s),
        _ => None,
    }
}

/// `vmOf`, to write.
pub fn velma_mut(w: &mut World, f: FighterId) -> Option<&mut VelmaState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Velma(s) => Some(s),
        _ => None,
    }
}

/// `vm`: her own code's read (the fighter is a Velma). A panic is a hook entered for a fighter of another character.
pub fn velma(w: &World, f: FighterId) -> &VelmaState {
    velma_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Velma"))
}

/// [`velma`], to write.
pub fn velma_w(w: &mut World, f: FighterId) -> &mut VelmaState {
    velma_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Velma"))
}

/// `VelmaPoliceState`: Pawn_VelmaAIBotPolice's own variables, made by police.rs's `velmapolice` entry's state hook.
/// Three are what the view and the bots read (class defaults false / None: not in the cooked data); the rest are
/// police.rs's (InitialLocation, JenkinsPin, ...). The car's damage morph (@3340-3467: Comp_Actor_Damage.CurrentDamage /
/// DamageToKill 18 through EaseOutCurve into Damage_Adj) is read off the minion fighter's own damage by the view: no
/// field.
#[derive(State, Debug, Default)]
pub struct VelmaPoliceState {
    /// ArrestEnabled: true after its Delay 3 (police.rs).
    pub arrest_enabled: bool,
    /// PerformedArrest (police.rs; VelmaPolice_HasPerformedArrest_BTD reads it).
    pub performed_arrest: bool,
    /// TargetedEnemy (police.rs; the pawn's Delay(0) loop @492 writes it from the BTS's EnemyActor).
    pub targeted_enemy: Option<FighterId>,
    /// InitialLocation @3008 (sim x, actor y).
    pub initial_x: Real,
    pub initial_y: Real,
    /// Delay(TimeBeforeArrest) @2906 -> @451, frames left (0: none).
    pub delay3: i32,
    /// The Delay(0) @1152 -> @492 pending (BeginPlay's ActivateTargetVisuals, again until a target is read).
    pub target_loop: bool,
    /// ArrestedVisuals' DoOnce (Temp_bool_IsClosed_Variable @1881).
    pub visuals_closed: bool,
    /// The fighters FullBody overlaps (its begin-overlap events: police.rs overlap_pass).
    pub inside: Vec<FighterId>,
    /// JenkinsPin @972 (VelmaJenkinsPin_C, police.rs [`JenkinsPin`](super::police::JenkinsPin)).
    pub pin: Option<ActorRef>,
}

/// `policeOf`.
pub fn police_of(w: &World, f: FighterId) -> Option<&VelmaPoliceState> {
    match &w.fighter(f).cs {
        CharState::VelmaPolice(s) => Some(s),
        _ => None,
    }
}

/// `policeOf`, to write.
pub fn police_mut(w: &mut World, f: FighterId) -> Option<&mut VelmaPoliceState> {
    match &mut w.fighter_mut(f).cs {
        CharState::VelmaPolice(s) => Some(s),
        _ => None,
    }
}

/// The minion's own code's read (the fighter is a Velma police minion). A panic is a hook entered for a fighter of
/// another character.
pub fn police(w: &World, f: FighterId) -> &VelmaPoliceState {
    police_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Velma police minion"))
}

/// [`police`], to write.
pub fn police_w(w: &mut World, f: FighterId) -> &mut VelmaPoliceState {
    police_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Velma police minion"))
}
