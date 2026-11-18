//! Finn's own fighter state (`FinnState`): Pawn_Finn's own variables (EFighter 17)
//! and those of his own classes that live on the pawn, beside BaseFighter's. Offsets are Pawn_Finn's ubergraph's. One
//! record per class file, with its class defaults; the fields several files read are `FinnState`'s own.
//! Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotBlend, SlotMix};
use crate::world::World;

/// `FinnPawnState` (pawn.rs): BeginPlay @1941-2195, Cleanup @7786-7833, HandleBarSwitch, the run meter, Use2D;
/// Finn_AnimBP's sim slot mix.
#[derive(State, Debug)]
pub struct FinnPawnState {
    /// Finn_AnimBP's slot graph mix (`fighter::anim` graph_slots writes it from pawn.rs's slot_input: IsFalling @1673,
    /// Moving @1840), with its timed BlendListByBool nodes' state (`slot_blend`: IsFalling [1.0, 0.1] over Moving
    /// [0.1, 0.1]); a new pawn starts it at INDEX_NONE.
    pub slot_mix: SlotMix,
}

impl Default for FinnPawnState {
    fn default() -> Self {
        FinnPawnState { slot_mix: SlotMix { slot_blend: Some(SlotBlend::default()), ..SlotMix::default() } }
    }
}

/// `FinnCoinsState` (coins.rs): SpawnTreasure @2381-7455, BPI_Finn_DropCoins @8786-8824.
#[derive(State, Debug, Default)]
pub struct FinnCoinsState {
    /// TreasureSpawnLocation (@2824: the hit fighter's location, UE X dropped), 'Treasure Knock Back' (@3285) and
    /// 'Treasure Knock Back Direction Vector' (@4211, in the spawner's facing frame, x forward). Kept between hits.
    pub spawn_x: Real,
    pub spawn_y: Real,
    pub knock_back: Real,
    pub dir_x: Real,
    pub dir_y: Real,
    /// FinnTreasureBuff's LastStackCount (CDO 0; @1028): a new buff starts at 0 (coins.rs's buff entry sets it back
    /// at the buff's end).
    pub last_stack_count: Real,
}

/// `FinnGemState` (gem.rs): empty, the gem's state is its own item's.
#[derive(State, Debug, Default)]
pub struct FinnGemState {}

/// `FinnDashState` (aimer.rs): the side special.
#[derive(State, Debug)]
pub struct FinnDashState {
    /// Pawn_Finn.DashTarget: the aimer's SetAimActorMulticast @4451 and AimOverlap begin @3897, cleared by its
    /// OnTargetDestroyed @9939. Read by BPI_Finn_Dash_Aim and FinnAimDashCondition.
    pub target: Option<ActorRef>,
    /// TeleportVisuals @10011 (the view's): PS_Finn_DashTrail made at the World tick `trail_at` (-1: none yet), its
    /// relative pitch in degrees.
    pub trail_at: i32,
    pub trail_pitch: Real,
}

impl Default for FinnDashState {
    fn default() -> Self {
        FinnDashState { target: None, trail_at: -1, trail_pitch: 0.0 }
    }
}

/// `FinnStoreState` (shop.rs): empty, the shop's state is the Finn_Shop actor's, the shop buffs' their extensions.
#[derive(State, Debug, Default)]
pub struct FinnStoreState {}

/// `FinnChopState` (bmo.rs): empty.
#[derive(State, Debug, Default)]
pub struct FinnChopState {}

/// `FinnPerksState` (perks.rs): empty, the perks' state is the buffs' and the actors'.
#[derive(State, Debug, Default)]
pub struct FinnPerksState {}

/// `FinnMomentum`: Comp_FinnMomentumCharge's variables (momentum.rs writes them: StartMomentum @193, the tick
/// @1156-2221, EndMomentum @225, OnFighterReset). Class defaults from Default__Comp_FinnMomentumCharge_C.
#[derive(State, Debug)]
pub struct FinnMomentum {
    /// StoredMomentum (s).
    pub stored: Real,
    /// MaxStoredMomentum: StartMomentum's Max Momentum @193.
    pub max: Real,
    /// MomentumAmount: EndMomentum's clamp(NormalizeToRange(stored, 0, max), 0, 1) @372.
    pub amount: Real,
    /// DamageMult (CDO 1.0) / KnockBackMult (CDO 1.0).
    pub dmg_mult: Real,
    pub kb_mult: Real,
    /// CalculateMomentum: StartMomentum @162, EndMomentum clears it.
    pub calculating: bool,
    /// HighFiveCharge, HighFivePercent, HighFiveMaxCharge (CDO 4.5).
    pub high_five: Real,
    pub high_five_pct: Real,
    pub high_five_max: Real,
    /// The run meter's fill (the last UpdateChargeUI value above 0): UI_FinnCharge's bar, kept for the view.
    pub meter: Real,
}

impl Default for FinnMomentum {
    fn default() -> Self {
        FinnMomentum {
            stored: 0.0,
            max: 0.0,
            amount: 0.0,
            dmg_mult: 1.0,
            kb_mult: 1.0,
            calculating: false,
            high_five: 0.0,
            high_five_pct: 0.0,
            high_five_max: 4.5,
            meter: 0.0,
        }
    }
}

/// `FinnState`.
#[derive(State, Debug, Default)]
pub struct FinnState {
    pub pawn: FinnPawnState,
    pub coins: FinnCoinsState,
    pub gem: FinnGemState,
    pub dash: FinnDashState,
    pub store: FinnStoreState,
    pub chop: FinnChopState,
    pub perks: FinnPerksState,
    /// Comp_FinnMomentumCharge (momentum.rs).
    pub momentum: FinnMomentum,
    /// LastHitActor / LastHitTime: SpawnTreasure @2602 / @2650 (coins.rs), the game time in seconds (float32). Read by
    /// Finn_Dash_AimerExperiment @2551 (aimer.rs).
    pub last_hit_actor: Option<ActorRef>,
    pub last_hit_time: Real,
    /// DropCoinOverride / DropCoinAmount: BPI_Finn_DropCoins @8786-8824 sets them, SpawnTreasure's FinalHit path clears
    /// the flag @4595 (coins.rs).
    pub drop_coin_override: bool,
    pub drop_coin_amount: Real,
    /// The live Finn_Dash_AimerExperiment (aimer.rs), Finn_Shop (shop.rs) and BMO_FallowActor (bmo.rs): written by
    /// their files at the spawn, cleared at the end.
    pub aimer: Option<ActorRef>,
    pub shop: Option<ActorRef>,
    pub bmo: Option<ActorRef>,
    /// The run meter on (TurnOnRunUi @8829) / off (TurnOffRunUI @8845). momentum.rs.
    pub run_ui: bool,
}

// ---- the call surface of the state

/// `finnOf`: the fighter's Finn state, `None` for a fighter of another character (a failed cast to Pawn_Finn /
/// BPI_Finn).
pub fn finn_of(w: &World, f: FighterId) -> Option<&FinnState> {
    match &w.fighter(f).cs {
        CharState::Finn(s) => Some(s),
        _ => None,
    }
}

/// `finnOf`, to write.
pub fn finn_mut(w: &mut World, f: FighterId) -> Option<&mut FinnState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Finn(s) => Some(s),
        _ => None,
    }
}

/// `finn` (his own code's read: the fighter is a Finn). A panic is a hook entered for a fighter of another character.
pub fn finn(w: &World, f: FighterId) -> &FinnState {
    finn_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Finn"))
}

/// `finn`, to write.
pub fn finn_w(w: &mut World, f: FighterId) -> &mut FinnState {
    finn_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Finn"))
}
