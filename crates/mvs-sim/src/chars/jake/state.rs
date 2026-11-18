//! Jake's own fighter state (`JakeState`): Pawn_Jake's own variables, those of
//! Jake_AnimBP the sim reads, and those of his two components (Comp_JakeEat, Comp_WeaponSwap): the state a Jake
//! (EFighter 9, BPI_GetFighterCharacter) has beside BaseFighter's. No other fighter carries it. Offsets are Pawn_Jake's
//! ubergraph's unless named. One record per group of files, with its class defaults; the fields every file reads are
//! `JakeState`'s own. A spawned actor's own variables are on the actor. Sim space: x = -UE Y, y = UE Z (a UE vector of
//! the play plane is kept as its two sim numbers; UE X is 0 there).
//!
//! Not here: BaseFighter's AllowWallPlay? (Jake's CDO false: the wall code's allowWallPlay / bpiAllowWallPlay),
//! DisableDash (the physics code's dashDisabled / bpiDisableDash), StickAttackX / Y (BaseFighter's, not the
//! AnimBP's: the port's right stick; InputState.rsy).
//! The Bounce Check values (Buffer, BounceVelocity, ...: dormant) and the Hulk / JakeSuit variables are read by
//! nothing live.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::combat::BuffRef;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix};
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `JakePawnState` (pawn.rs, anim.rs): BeginPlay @2464-2663, ReceiveTick @1985-2417 (ReachCheck, the charge),
/// ChargeStarted @2207 / ChargeEnd @2219, Cleanup @2718, HeadScale @2848, Use 2D Hitboxes @4430; Jake_AnimBP's sim slot
/// mix.
#[derive(State, Debug)]
pub struct JakePawnState {
    /// Jake_AnimBP's slot graph mix (written at each pose update from pawn.rs's slot_input), with its timed
    /// BlendListByBool nodes (`slot_blend`: Horse Montage2's IsFalling JumpSlot / FallingBlend); a new pawn (mod.rs
    /// match_reset's new JakeState) starts them at INDEX_NONE.
    pub slot_mix: SlotMix,
    /// Comp_Pawn_ChargeAttackManager's MontageSectionOverride and ChargeTotalTime as the last ChargingAttack begin
    /// stored them (StartAttackChargeMultipliersServer: the notify's MontageSectionOveride, None without one;
    /// TotalDuration x ChargeMultiplier 1), read by the tick's section test @2107 and GetUpdatedPercentCharged @2231.
    pub section_override: Name,
    pub charge_total_time: Real,
    /// BeginPlay's Delay 0.5 @15 (resume @70): its time left on the world clock, and whether it has fired, which binds
    /// ChargeStarted @2207 / ChargeEnd @2219 to the component's delegates @70-157 (a new pawn: match_reset).
    pub bind_delay: Real,
    pub charge_bound: bool,
    /// Jake_AnimBP's EatOverride machine (StateMachine_1, anim.rs): the state (anim.rs EAT_IDLE ...), each state's
    /// player time (Idle's: its Jake_Eat_Attack_Idle player), the Idle state's list on Moving (FAlphaBlend: Walk's
    /// weight, the weight at the switch, the time left and the whole time, the child last active, -1 at INDEX_NONE)
    /// and its Walk player's time; `eat_first`: the next update is a first update (it restarts at Idle); the last
    /// transition's crossfade (the state it left, its time and the time run: for the drawing; the shapes take the
    /// state alone).
    pub eat: i32,
    pub eat_times: [Real; 5],
    pub walk_time: Real,
    pub walk_w: Real,
    pub walk_from: Real,
    pub walk_rem: Real,
    pub walk_dur: Real,
    pub walk_last: i32,
    pub eat_first: bool,
    pub fade_from: i32,
    pub fade_dur: Real,
    pub fade_t: Real,
}

impl Default for JakePawnState {
    fn default() -> Self {
        JakePawnState {
            slot_mix: SlotMix { slot_blend: Some(SlotBlend::default()), ..SlotMix::default() },
            section_override: name!("None"),
            charge_total_time: 0.0,
            bind_delay: 0.5,
            charge_bound: false,
            eat: 0,
            eat_times: [0.0; 5],
            walk_time: 0.0,
            walk_w: 0.0,
            walk_from: 0.0,
            walk_rem: 0.0,
            walk_dur: 0.0,
            walk_last: -1,
            eat_first: true,
            fade_from: -1,
            fade_dur: 0.0,
            fade_t: 0.0,
        }
    }
}

/// `JakeStretchState` (stretch.rs): JakeStretch; empty.
#[derive(State, Debug, Default)]
pub struct JakeStretchState {}

/// `JakeHouseState` (house.rs, perks.rs): JakeHouse_Actor, Buff_Jake_SafetyNet, Buff_AirSuperiority; empty.
#[derive(State, Debug, Default)]
pub struct JakeHouseState {}

/// `JakeHorseState` (horse.rs): JakeHorse_Actor3, JakeHorsePlatform, Buff_AttachToAllyHorse; empty.
#[derive(State, Debug, Default)]
pub struct JakeHorseState {}

/// `JakeEatState` (eat.rs, eat_buffs.rs): Comp_JakeEat's own variables but HoldingActor / HeldFighters (`JakeState`'s:
/// EatTimer, EatHoldTime 5.0, StuggleCount, MaxStruggleCount 8, StruggleMult 0.5, CanAttack, CanDropAlly,
/// TimerActive, SpitFighter, BuffsToDestroy, SlowBuff, the saved activator fields, JakeAddedArmor, EatOverridePack:
/// the CDO's in Default__Comp_JakeEat_C), Buff_JakEat, Buff_JakEatSlow.
#[derive(State, Debug, Default)]
pub struct JakeEatState {
    /// EatTimer (SetTimer(SpitTimeEnd, EatHoldTime, no loop) @6354): 0 none (an invalid handle), 1 pending (set or
    /// unpaused in the tick groups: started by the frame's timer pass without counting that frame), 2 active, 3
    /// paused, 4 fired (the handle valid, the timer gone). `rate`: the time it was started with (EatHoldTime, or what
    /// a pause left); `elapsed`: the deltas counted since it went active (the manager's InternalTime - start, a
    /// double).
    pub timer: i32,
    pub rate: crate::Clock,
    pub elapsed: crate::Clock,
    /// TimerActive.
    pub timer_active: bool,
    /// StuggleCount (the build's spelling).
    pub struggle_count: i32,
    /// The fighters whose HitstunHandler.Struggling this component's StruggleServer is bound to
    /// (BindOrUnbindStruggle(true) @6230; the unbind @2259 is given the buff actor, which has no HitstunHandler: never
    /// unbound).
    pub struggle_bound: Vec<FighterId>,
    /// SlowBuff (SpawnBuffActor's return @6175): a buff on Jake himself.
    pub slow_buff: Option<BuffRef>,
    /// CanAttack.
    pub can_attack: bool,
    /// CanDropAlly.
    pub can_drop_ally: bool,
    /// SpitFighter (the enemies SpitServer found; spit.rs reads it).
    pub spit_fighter: Vec<FighterId>,
    /// The DoOnce @6194 (IsClosed): reset by RemoveHeldFighter's last fighter @6908.
    pub closed: bool,
    /// InputPreTriggered += JakeSpitEvent (@6285) .. -= (@9445): spit.rs's JakeSpitEvent listens while it is true.
    pub spit_bound: bool,
}

/// `JakeSpitState` (spit.rs): ProjectileAimer_SpitJake, JakeSpitCharacterHitBox; empty.
#[derive(State, Debug, Default)]
pub struct JakeSpitState {}

/// `JakeAllySpitState` (ally_spit.rs): JakeEatAlly_Aim, JakeSpitAllyHitBox; empty.
#[derive(State, Debug, Default)]
pub struct JakeAllySpitState {}

/// `JakeAimState` (aim.rs): the forward-air aim punch (JakeAimer, JakeAimPunchEatListener,
/// AnimNotify_Hitframe_Jake_Aim, AnimNotify_UpdateHitframeDamage_Jake_Aim, AnimNotify_ResetAimDirection).
#[derive(State, Debug, Default)]
pub struct JakeAimState {
    /// Pawn_Jake.AimPunchVector (CDO (0, 0, 0)), its UE Y and Z: JakeAimer's SetArrowRotationMulticast @7550 writes
    /// it, AnimNotify_Hitframe_Jake_Aim / UpdateHitframeDamage_Jake_Aim read it as their knockback direction.
    pub aim_punch_y: Real,
    pub aim_punch_z: Real,
}

/// `JakeWeaponsState` (weapons.rs): Comp_WeaponSwap's own variables but CurrentWeapon / WeaponEquiped (`JakeState`'s:
/// AddedWeapons as the weapon meshes' tags or names, HoldSlotName, MeshTag), AnimNotify_WeaponSwap /
/// WeaponHideCurrent, Buff_Jake_WeaponMaster / WeaponMasterAlly, AnimNotify_PopText; empty.
#[derive(State, Debug, Default)]
pub struct JakeWeaponsState {}

/// `JakeAnim`: Jake_AnimBP's variables that the sim reads (the slot mix's inputs) and that Jake's actors and his
/// components write through the BPI_Anim_BP / Jake_AnimBP calls (pawn.rs's setters). The AnimBP is the pawn's mesh's
/// own: one per pawn, its variables at the class defaults (0 / false) when the pawn is made.
#[derive(State, Debug, Default)]
pub struct JakeAnim {
    /// ChargeAmount: BPI_SetChargeValue @8846 from the pawn's ReceiveTick -> GetChargeServer @2192 ->
    /// GetChargeMulticast -> @2417 (the charge value: Comp_Pawn_ChargeAttackManager GetUpdatedPercentCharged @2231);
    /// the AnimBP's ChargeStarted @8713 zeroes it. Blends ShortAttack / ReachAttack of Weapon_Charge_Attack_U
    /// (pawn.rs).
    pub charge_amount: Real,
    /// StretchOn: BPI_SetStretchOn @8576 (JakeStretch, stretch.rs): the COM ModifyBone @2898-3018.
    pub stretch_on: bool,
    /// CalculateStretchAngle: BPI_CalculateStretchAngle @8596 (JakeStretch, stretch.rs); @2828.
    pub calculate_stretch_angle: bool,
    /// StretchDirection: the AnimBP's own update @2828 while CalculateStretchAngle (pawn.rs).
    pub stretch_direction: Real,
    /// AimDirection: BPI_SetAimDirection @8544 (JakeAimer aim.rs, ProjectileAimer_SpitJake spit.rs,
    /// AnimNotify_ResetAimDirection aim.rs); AimDirectionInverted = -AimDirection @3055 (pawn.rs derives it in the
    /// slot mix).
    pub aim_direction: Real,
    /// AimDistanceAlpha: BPI_SetAimDIstanceAlpha @8616 (JakeAimer, aim.rs).
    pub aim_distance_alpha: Real,
    /// EatOverrideBool: BPI_HoldingFighter @8644 (Comp_JakeEat's visuals @8183 / @8801, eat.rs).
    pub eat_override: bool,
}

/// `JakeState`.
#[derive(State, Debug)]
pub struct JakeState {
    /// pawn.rs, anim.rs.
    pub pawn: JakePawnState,
    /// stretch.rs.
    pub stretch: JakeStretchState,
    /// house.rs, perks.rs.
    pub house: JakeHouseState,
    /// horse.rs.
    pub horse: JakeHorseState,
    /// eat.rs, eat_buffs.rs.
    pub eat: JakeEatState,
    /// spit.rs.
    pub spit: JakeSpitState,
    /// ally_spit.rs.
    pub ally_spit: JakeAllySpitState,
    /// aim.rs.
    pub aim: JakeAimState,
    /// weapons.rs.
    pub weapons: JakeWeaponsState,
    // The fields every file reads. Writers in brackets.
    /// Jake_AnimBP's inputs (pawn.rs's setters; the writers are named per field).
    pub anim: JakeAnim,
    /// Pawn_Jake ReachCheck (CDO false): ChargeStarted @2207 sets it, ChargeEnd @2219 / Cleanup @2718 / the tick's
    /// section test @2004-2181 clear it; while it holds, the tick calls GetChargeServer @2192 (pawn.rs).
    pub reach_check: bool,
    /// HeadScale (the pawn's BPI_HeadScale @2848 writes Jake_AnimBP.HeadScale: UE (X, Y, Z), 1 1 1 at rest), read by
    /// the view's head bones (gated by X > 1 and not StretchOn / EatOverride). pawn.rs writes it; UE order.
    pub head_scale: [Real; 3],
    /// Comp_JakeEat HoldingActor (CDO false): Jake holds someone (AddHeldFighers @5007 .. RemoveHeldFighter @7412),
    /// read by JakeEat_Hold / JakeEat_NotHold (pawn.rs's conditions) and the spit files (eat.rs writes it).
    pub holding_actor: bool,
    /// Comp_JakeEat HeldFighters: the swallowed fighters (enemies and allies), in the order AddHeldFighers added them
    /// (eat.rs writes it; spit.rs / ally_spit.rs read it; the view draws the eaten state from it).
    pub held_fighters: Vec<FighterId>,
    /// Comp_WeaponSwap CurrentWeapon (CDO 0) and WeaponEquiped (CDO false; ReceiveBeginPlay @1075 clears it): the
    /// weapon index CycleWeapon steps and whether a weapon is out, read by JakeWeaponFist / JakeWeaponHammer (pawn.rs's
    /// conditions), WeaponMaster (weapons.rs) and the view's weapon meshes (weapons.rs writes them). The index's
    /// meaning (which mesh) is weapons.rs's read of AddedWeapons (the meshes tagged MeshTag, in the component order of
    /// the pawn).
    pub weapon: i32,
    pub weapon_equipped: bool,
    /// Comp_WeaponSwap TempHidAllWeapons / UnhidEquippedWeapon (AnimNotify_WeaponHideCurrent): the weapons hidden for
    /// a while (weapons.rs writes it, the view reads it with `weapon` / `weapon_equipped`).
    pub weapons_hidden: bool,
    // The live spawned actors other files and the view read: each a world actor, written by its file at the spawn and
    // cleared at its end (the build's are the spawn manager's lists).
    /// JakeStretch (stretch.rs).
    pub stretch_actor: Option<ActorRef>,
    /// JakeHouse_Actor (house.rs).
    pub house_actor: Option<ActorRef>,
    /// JakeHorse_Actor3 (horse.rs).
    pub horse_actor: Option<ActorRef>,
    /// JakeAimer (aim.rs).
    pub aimer: Option<ActorRef>,
    /// ProjectileAimer_SpitJake (spit.rs).
    pub spit_aimer: Option<ActorRef>,
    /// JakeEatAlly_Aim (ally_spit.rs; spawned by Comp_JakeEat @5678, eat.rs calls ally_spit.rs's spawn).
    pub ally_aim: Option<ActorRef>,
}

impl Default for JakeState {
    fn default() -> Self {
        JakeState {
            pawn: JakePawnState::default(),
            stretch: JakeStretchState::default(),
            house: JakeHouseState::default(),
            horse: JakeHorseState::default(),
            eat: JakeEatState::default(),
            spit: JakeSpitState::default(),
            ally_spit: JakeAllySpitState::default(),
            aim: JakeAimState::default(),
            weapons: JakeWeaponsState::default(),
            anim: JakeAnim::default(),
            reach_check: false,
            head_scale: [1.0; 3],
            holding_actor: false,
            held_fighters: Vec::new(),
            weapon: 0,
            weapon_equipped: false,
            weapons_hidden: false,
            stretch_actor: None,
            house_actor: None,
            horse_actor: None,
            aimer: None,
            spit_aimer: None,
            ally_aim: None,
        }
    }
}

// ---- the call surface of the state (`jake`, `jakeOf`)

/// `jakeOf`: the fighter's Jake state, `None` for a fighter of another character (a failed cast to Pawn_Jake / its
/// components).
pub fn jake_of(w: &World, f: FighterId) -> Option<&JakeState> {
    match &w.fighter(f).cs {
        CharState::Jake(s) => Some(s),
        _ => None,
    }
}

/// `jakeOf`, to write.
pub fn jake_mut(w: &mut World, f: FighterId) -> Option<&mut JakeState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Jake(s) => Some(s),
        _ => None,
    }
}

/// `jake` (his own code's read: the fighter is a Jake). A panic is a hook entered for a fighter of another character.
pub fn jake(w: &World, f: FighterId) -> &JakeState {
    jake_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Jake"))
}

/// `jake`, to write.
pub fn jake_w(w: &mut World, f: FighterId) -> &mut JakeState {
    jake_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Jake"))
}
