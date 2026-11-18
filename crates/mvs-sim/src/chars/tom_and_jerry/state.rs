//! Tom and Jerry's own fighter state (`TomAndJerryState`):
//! Pawn_TomAndJerry's own variables, those of Tom_and_Jerry_AnimBP the sim reads, and the live Jerry actor: the state a
//! Tom and Jerry (EFighter 15, BPI_GetFighterCharacter) has beside BaseFighter's. No other fighter carries it. One
//! record per class file, with its class defaults; the fields more than one file reads are `TomAndJerryState`'s own.
//! A spawned actor's own variables are on the actor (its class's struct), not here. Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ProjectileId};
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix};
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `TomAndJerryPawnState` (pawn.rs, aim.rs): BeginPlay @5024-5480, ResetJerry @5485 / JerryCoolDownMulticast @6074,
/// UpdatedVisibility @6283-6440, SetJerryAttached @6441-6484, TriggerAbilityMulticast @6553-6568, ReceiveTick @352
/// (JerryPointer @519-1203, the box trace @1927, the line traces @3380 / @4141 -> BPI_JerryWallOffset @3647);
/// Tom_and_Jerry_AnimBP's sim slot mix.
#[derive(State, Debug)]
pub struct TomAndJerryPawnState {
    /// Tom_and_Jerry_AnimBP's slot graph mix (the slotMix / slotInput hooks, pawn.rs), with its timed BlendListByBool
    /// nodes (`slot_blend`: Moving, IsFalling, WithJerry, RocketControlBool); a new pawn (mod.rs match_reset's new
    /// state) starts them at INDEX_NONE.
    pub slot_mix: SlotMix,
    /// JerryOffsetAlpha: the tick's wall and ledge alpha (@3044 / @4964 / @4996), passed to BPI_JerryWallOffset @3647.
    pub jerry_offset_alpha: Real,
    /// The JerryPointer (ReceiveTick @376-1203; the view draws it and turns the JerryPointerArrow toward
    /// `spawned_jerry` @519-665): its visibility (the component's default: shown until the first tick hides it).
    pub pointer_visible: bool,
    /// The DoOnce @35-115's IsClosed (reset @270).
    pub hide_done: bool,
    /// The Gate's state (starts closed, its init @203-209; opened @140 by the DoOnce's run; no node closes it again).
    pub gate_open: bool,
    /// SpawnedJerryAcor: the last Jerry_SpawnedActor_C (that class exactly) of Comp_Spawner.SpawnedActors the open gate
    /// found (@1055-1184; read by TargetBetweenMeAndJerry_BTD), never cleared (IsValid @480 is the reader's test: dead
    /// counts as invalid).
    pub spawned_jerry: Option<ActorRef>,
}

impl Default for TomAndJerryPawnState {
    fn default() -> Self {
        TomAndJerryPawnState {
            slot_mix: SlotMix { slot_blend: Some(SlotBlend::default()), ..SlotMix::default() },
            jerry_offset_alpha: 0.0,
            pointer_visible: true,
            hide_done: false,
            gate_open: false,
            spawned_jerry: None,
        }
    }
}

/// `TomAndJerryJerryState` (jerry.rs): Jerry_SpawnedActor, Jerry_SpawnedActor_TempInvincibility. Empty: the actor's own
/// variables are on the actor.
#[derive(State, Debug, Default)]
pub struct TomAndJerryJerryState {}

/// `TomAndJerryFollowState` (follower.rs, cheese.rs): Jerry_SpawnedActor's follow variables (class defaults 0 / false),
/// kept on the pawn that owns him: a pawn has one live Jerry at a time (`TomAndJerryState::jerry_actor`), and the
/// record is that actor's (`jerry_id`) from the attach (follower.rs attach_to_ally, which starts it over: a new actor's
/// variables are at the class defaults). Buff_Cheese keeps its own state on its buff entry (cheese.rs).
#[derive(State, Debug)]
pub struct TomAndJerryFollowState {
    /// The Jerry actor (its id, `WorldActor::seq`) the fields below are of; -1 none.
    pub jerry_id: i32,
    /// Follow_Y_Offset (SetFollowingVariables @356: the ally's root capsule scaled radius).
    pub follow_y: Real,
    /// Follow_Z_Offset (@228: -(the scaled half height - 80)).
    pub follow_z: Real,
    /// CrouchHeight (@783: -(scale x CrouchedHalfHeight - 40)).
    pub crouch_height: Real,
    /// IsCrouching (ReceiveTick @19358: the ally's CMC IsCrouching).
    pub is_crouching: bool,
    /// The actor's relative location in the ally's root frame as the last K2_SetActorRelativeLocation left it (@2623,
    /// UpdateFacing @18101, CrouchChange @18451): Y (FacingLeft ? -Follow_Y_Offset : Follow_Y_Offset; the depth, which
    /// the facing's yaw turns with it: the world offset is the same side whatever the facing, the view draws it) and Z
    /// (up).
    pub rel_y: Real,
    pub rel_z: Real,
    /// The ally's four delegates bound (@3187-3453, behind UpdateFollower @3142): FacingChanged -> UpdateFacing,
    /// AdjustedVisibility -> AdjustVisibility, OnExitedArena -> DieWithFollower, OnTakeAnyDamage -> FollowerDamage.
    pub bound: bool,
    /// Delay(0.1) @3768 (-> @722): ticks left, 0 none.
    pub delay: i32,
}

impl Default for TomAndJerryFollowState {
    fn default() -> Self {
        TomAndJerryFollowState {
            jerry_id: -1,
            follow_y: 0.0,
            follow_z: 0.0,
            crouch_height: 0.0,
            is_crouching: false,
            rel_y: 0.0,
            rel_z: 0.0,
            bound: false,
            delay: 0,
        }
    }
}

/// `TomAndJerryChampagneState` (champagne.rs): Champagne Attack F / B, LaunchCork, CorkAmmo, JerryCork.
/// Jerry_SpawnedActor's champagne variables (its CDO's), kept for the live Jerry (Tom has one at most): `actor_id` is
/// the actor they belong to, and a new one (its BeginPlay @13402: CorkAmmo = CorkStartingAmmo) starts them over
/// (champagne.rs cork_begin_play). `TomAndJerryState::cork_ammo` is its CorkAmmo.
#[derive(State, Debug)]
pub struct TomAndJerryChampagneState {
    /// The Jerry actor (its id, `WorldActor::seq`) these belong to (-1: none yet).
    pub actor_id: i32,
    /// ChampagneGate (CDO true).
    pub gate: bool,
    /// ChampagneTimer (0.5 s, @2214): frames left until ChampagneTimerFunction, 0: not running.
    pub gate_left: i32,
    /// ChampagneStartTime (GetGameTimeInSeconds at the Champagne window's begin @26696).
    pub start_time: Real,
    /// CorkQuedFacing (Champagne Attack @411).
    pub queued_facing: bool,
    /// Widget (UI_JerryCorkCount) visibility: hidden in the template, SetAmmoUI_Visuals / @28231.
    pub widget_visible: bool,
    /// The Champagne mesh's animation (the view): `idle` (Champagne_Idle @26625) or `shoot` (Champagne_Shoot @26227),
    /// and the world tick it started at.
    pub bottle_anim: Name,
    pub bottle_tick: i32,
    /// PS_TomJerry_ChampagnePop at the bottle's Neck (@1792): the world tick of the last launch (the view); -1 none.
    pub pop_tick: i32,
}

impl Default for TomAndJerryChampagneState {
    fn default() -> Self {
        TomAndJerryChampagneState {
            actor_id: -1,
            gate: true,
            gate_left: 0,
            start_time: 0.0,
            queued_facing: false,
            widget_visible: false,
            bottle_anim: name!("idle"),
            bottle_tick: 0,
            pop_tick: -1,
        }
    }
}

/// `TomAndJerryTennisState` (tennis.rs): Tom_and_Jerry_TennisBall (+ _Reflected), Jerry's tennis side,
/// TomAndJerry_NoJerry_SlingShot_Aim. Empty.
#[derive(State, Debug, Default)]
pub struct TomAndJerryTennisState {}

/// `TomAndJerrySlingshotState` (slingshot.rs): TomAndJerry_SlingShot_Aim_02. Empty: the aimer's variables are on the
/// actor (slingshot.rs `SlingAim`), the live one is `TomAndJerryState::sling_aimer`.
#[derive(State, Debug, Default)]
pub struct TomAndJerrySlingshotState {}

/// `TomAndJerryRocketState` (rocket.rs): JerryRocket, Jerry's LaunchRocket. Empty.
#[derive(State, Debug, Default)]
pub struct TomAndJerryRocketState {}

/// `TomAndJerryKickedState` (kicked.rs): JerryKickedRocket, TomAimRocket1. Empty.
#[derive(State, Debug, Default)]
pub struct TomAndJerryKickedState {}

/// `TomAndJerryFishingState` (fishing.rs): FishingLure, Buff_TomAndJerry_Bait.
#[derive(State, Debug, Default)]
pub struct TomAndJerryFishingState {
    /// The main mesh AnimInstance's OnMontageEnded broadcasts so far (the lures sample it: fishing.rs).
    pub montage_ends: i32,
}

/// `TomAndJerryTrapState` (trap.rs, dynamite.rs): MouseTrap_SpawnedActor, Buff_MouseTrap, Dynamite_SpawnedActor
/// (+ CantSplitDynamite), Buff_TomAndJerry_DynamiteSplit. Empty: the trap's state is the actor's.
#[derive(State, Debug, Default)]
pub struct TomAndJerryTrapState {}

/// `TomAndJerryAnim`: Tom_and_Jerry_AnimBP's variables that the sim reads (the slot mix's inputs; the JerryHit
/// sphere's bones) and that the actors and notifies write through the BPI_Anim_BP / Tom_and_Jerry_AnimBP events
/// (pawn.rs's setters). The AnimBP is the pawn's mesh's own: one per pawn, its variables at the class defaults
/// (0 / false) when the pawn is made.
#[derive(State, Debug)]
pub struct TomAndJerryAnim {
    /// AimDirection: BPI_SetAimDirection @16057 (-> @992: AimDirectionInverted = AimDirection x -1, pawn.rs derives
    /// it in the slot mix). Callers: TomAndJerry_SlingShot_Aim_02 (slingshot.rs), TomAndJerry_NoJerry_SlingShot_Aim
    /// (tennis.rs).
    pub aim_direction: Real,
    /// WallAlpha: BPI_JerryWallOffset(Alpha) @16029, from the pawn's tick @3647 (pawn.rs). JerryWallOffset =
    /// FMax(TurnAlpha, WallAlpha) @1295-1341 is the AnimBP's own update (pawn.rs derives it), into ModifyBone_6 / _7
    /// unless IgnoreJerryWallOffset (@7341 / @7809).
    pub wall_alpha: Real,
    /// TurnAlpha: the AnimBP's own FInterpTo @1268 (pawn.rs).
    pub turn_alpha: Real,
    /// IgnoreJerryWallOffset: AnimNotify_JerryIgnoreWall's begin / end (pawn.rs).
    pub ignore_wall_offset: bool,
    /// RocketControlBool: BPI_SetControlBool @16129. Caller: JerryRocket (rocket.rs). Picks ControlL / ControlR.
    pub rocket_control: bool,
    /// BlendListByBool_1's FAlphaBlend over Slot ControlL / ControlR (its state at index 0; arms.rs arms_update steps
    /// it at each anim update, the view reads its weight). A new anim instance starts it at INDEX_NONE.
    pub arms: SlotBlend,
    /// OverrideMirror? / MirrorOverride: BPI_SetFacingOverride @16089 / @16109, from AnimNotify_OverrideMirror's begin
    /// and end (pawn.rs's notify; the NoJerry slingshot montages). TwoWayBlend's alpha (OverrideMirror? ?
    /// MirrorOverride : false) @15320-15431 between SlingShotAim and SlingShotAimBack.
    pub override_mirror: bool,
    pub mirror_override: bool,
    /// JerryWallOffset = FMax(TurnAlpha, WallAlpha) @1295-1341, at the last BlueprintUpdateAnimation (aim.rs): the
    /// alpha of ModifyBone_6 / _7 (@7341 / @7809: SelectFloat(0, JerryWallOffset, IgnoreJerryWallOffset)) on
    /// jerry_un_COM.
    pub jerry_wall_offset: Real,
    /// WithJerry (class default true) = BPI_IsJerryVisible @1614-1660 at each BlueprintUpdateAnimation (aim.rs): the
    /// BlendListByBool_2 between the LinkedAnimLayer (the anim set) and the LinkedAnimGraph (the noJerry set).
    pub with_jerry: bool,
    /// OldFacingLeft (class default true): OnFacingChanged's last facing (@94), TurnAlpha = 1 at a change (@71).
    pub old_facing_left: bool,
}

impl Default for TomAndJerryAnim {
    fn default() -> Self {
        TomAndJerryAnim {
            aim_direction: 0.0,
            wall_alpha: 0.0,
            turn_alpha: 0.0,
            ignore_wall_offset: false,
            rocket_control: false,
            arms: SlotBlend::default(),
            override_mirror: false,
            mirror_override: false,
            jerry_wall_offset: 0.0,
            with_jerry: true,
            old_facing_left: true,
        }
    }
}

/// `TomAndJerryState`.
#[derive(State, Debug)]
pub struct TomAndJerryState {
    /// pawn.rs.
    pub pawn: TomAndJerryPawnState,
    /// jerry.rs.
    pub jerry: TomAndJerryJerryState,
    /// follower.rs.
    pub follow: TomAndJerryFollowState,
    /// champagne.rs.
    pub champagne: TomAndJerryChampagneState,
    /// tennis.rs.
    pub tennis: TomAndJerryTennisState,
    /// slingshot.rs.
    pub slingshot: TomAndJerrySlingshotState,
    /// rocket.rs.
    pub rocket: TomAndJerryRocketState,
    /// kicked.rs.
    pub kicked: TomAndJerryKickedState,
    /// fishing.rs.
    pub fishing: TomAndJerryFishingState,
    /// trap.rs.
    pub trap: TomAndJerryTrapState,

    // The fields more than one file reads. Writers in brackets.
    /// Tom_and_Jerry_AnimBP's inputs (pawn.rs's setters; the writers are named per field).
    pub anim: TomAndJerryAnim,
    /// Pawn_TomAndJerry JerryAttached (CDO true): Jerry on Tom (the JerryMesh sub-mesh shown).
    /// SetJerryAttachedMulticast @6465-6484 writes it and SetSubMeshVisibility(JerryMesh, JerryAttached, true) @6484
    /// (pawn.rs; callers through pawn.rs set_is_jerry_attached: Jerry's EndPlay @7704 and the pickup in jerry.rs, the
    /// cooldown's end @5553). Read by BPI_CheckJerryAttached, JerryOutCondition / JerryNotOutCondition (pawn.rs's
    /// conditions).
    pub jerry_attached: bool,
    /// Pawn_TomAndJerry VisibleCoolDown (CDO true): JerryCoolDownMulticast @6074 sets it true at the cooldown's end
    /// @5553, false at its start @5889 (pawn.rs). Read by the pointer (pawn.rs) and the view.
    pub visible_cooldown: bool,
    /// Pawn_TomAndJerry JerryOffsetOverrideTrace (a bool, class default false: the SelectFloat's bPickA):
    /// BPI_SetJerryOffsetTrace(Value) @332, read by the tick's traces @1725 / @2512 (pawn.rs; caller
    /// AnimNotify_JerryOffsetOverride, pawn.rs).
    pub jerry_offset_override_trace: bool,
    /// The live Jerry_SpawnedActor (or its TempInvincibility child) Tom owns, `None` while Jerry is on Tom or gone
    /// (jerry.rs writes it at the spawn and clears it at its end). Every other file reaches Jerry through it.
    pub jerry_actor: Option<ActorRef>,
    /// The live aimers (world actors): TomAndJerry_SlingShot_Aim_02 (slingshot.rs), TomAndJerry_NoJerry_SlingShot_Aim
    /// (tennis.rs).
    pub sling_aimer: Option<ActorRef>,
    pub no_jerry_aimer: Option<ActorRef>,
    /// The tennis balls in play (Tom_and_Jerry_TennisBall and _Reflected projectiles, in spawn order; the NoJerry aim
    /// keeps up to 8, DestroySpawnedActors @5077). tennis.rs writes it; the view and the bots (TennisBallInPlay) read
    /// it.
    pub tennis_balls: Vec<ProjectileId>,
    /// Jerry's cork count for the UI_JerryCorkCount widget (CorkAmmo; Jerry's CDO: CorkAmmo 5, CorkStartingAmmo 3,
    /// CorkAllyReloadAmmo 5). champagne.rs writes it (CorkAmmoTracker @2310, CorkAmmoSetrMulticast); the view reads
    /// it. The build keeps it on the actor: each Jerry's BeginPlay @13402 sets CorkStartingAmmo, so it is the live
    /// Jerry's count (champagne.rs cork_begin_play starts it over for a new actor;
    /// `TomAndJerryChampagneState::widget_visible`: the widget).
    pub cork_ammo: i32,
}

impl Default for TomAndJerryState {
    fn default() -> Self {
        TomAndJerryState {
            pawn: TomAndJerryPawnState::default(),
            jerry: TomAndJerryJerryState::default(),
            follow: TomAndJerryFollowState::default(),
            champagne: TomAndJerryChampagneState::default(),
            tennis: TomAndJerryTennisState::default(),
            slingshot: TomAndJerrySlingshotState::default(),
            rocket: TomAndJerryRocketState::default(),
            kicked: TomAndJerryKickedState::default(),
            fishing: TomAndJerryFishingState::default(),
            trap: TomAndJerryTrapState::default(),
            anim: TomAndJerryAnim::default(),
            jerry_attached: true,
            visible_cooldown: true,
            jerry_offset_override_trace: false,
            jerry_actor: None,
            sling_aimer: None,
            no_jerry_aimer: None,
            tennis_balls: Vec::new(),
            cork_ammo: 3,
        }
    }
}

/// `tjOf`: any fighter's; `None` when it is not a Tom and Jerry (a failed cast to Pawn_TomAndJerry / BPI_TomAndJerry).
pub fn tj_of(w: &World, f: FighterId) -> Option<&TomAndJerryState> {
    match &w.fighter(f).cs {
        CharState::TomAndJerry(s) => Some(s),
        _ => None,
    }
}

/// `tjOf`, to write.
pub fn tj_mut(w: &mut World, f: FighterId) -> Option<&mut TomAndJerryState> {
    match &mut w.fighter_mut(f).cs {
        CharState::TomAndJerry(s) => Some(s),
        _ => None,
    }
}

/// `tj`: his own code's read (the fighter is a Tom and Jerry). A panic is a hook entered for a fighter of another
/// character.
pub fn tj(w: &World, f: FighterId) -> &TomAndJerryState {
    tj_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Tom and Jerry"))
}

/// [`tj`], to write.
pub fn tj_w(w: &mut World, f: FighterId) -> &mut TomAndJerryState {
    tj_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Tom and Jerry"))
}
