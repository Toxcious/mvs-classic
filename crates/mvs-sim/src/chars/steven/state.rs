//! Steven's own fighter state (`StevenState`): Pawn_StevenUniverse's own
//! variables and those of his classes that live on the pawn, beside BaseFighter's. His Watermelon minion is the same
//! pawn class and carries a state of its own. One record per file, every field declared here whichever file writes it
//! (the record names the owner); a field several files read lives in the record of the file that writes it.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::world::World;

/// `StevenPawnState` (pawn.rs): the pawn core, the glide input, the shield ammo, BPI_Steven.
#[derive(State, Debug)]
pub struct StevenPawnState {
    /// isGliding: ReadGlideInput @1095, SetGlideValueMulticast @4168, BPI_Cleanup @8654 (false).
    pub gliding: bool,
    /// Steven_AnimBP.Gliding (BPI_SetIsGliding from ReadGlideInput @1245 and SetGlideValueMulticast @3450): the
    /// glide BlendListByBool's bActiveValue. BPI_Cleanup clears isGliding alone. Read by glide.rs.
    pub anim_gliding: bool,
    /// ReadGlideInput's DoOnce on entering the glide (@1792-2060).
    pub glide_once_closed: bool,
    /// OriginalGravityScale: BeginPlay @2132 saves CharacterMovement.GravityScale (5.25 before).
    pub original_gravity_scale: Real,
    /// UpShieldAmmo / UpShieldAmmoMax (class defaults 2 / 2; BeginPlay takes them from the data): read by
    /// StevenHasShieldAmmoCondition @79 (> 0).
    pub up_shield_ammo: Real,
    pub up_shield_ammo_max: Real,
    /// WatermelonSteven: the Watermelon minion this Steven made (MulticastWatermelonStevenSetUp @4392, watermelon.rs;
    /// BPI_ClearWatermelonStevenReference @10081, pawn.rs). A destroyed minion stays referenced (IsValid false).
    // The record writes the minion's id, which a destroyed minion keeps: the trace writer's map of a fighter gone
    // from the World is the port's to make (watermelon.rs's track).
    pub watermelon_steven: Option<FighterId>,
    /// Comp_Spawner.SpawnedActors_ClassChecked as BPI_HasWatermelonSpawned reads it: the entries of the last filter
    /// (CharHooks.spawner_filtered) whose class is exactly HealOverTimeZoneBP_C or HealGrenade_C, the actors themselves
    /// (BPI_HasWatermelonSpawned reads each one's class and whether it is gone).
    pub class_checked: Vec<ActorRef>,
}

impl Default for StevenPawnState {
    fn default() -> Self {
        StevenPawnState {
            gliding: false,
            anim_gliding: false,
            glide_once_closed: false,
            original_gravity_scale: 5.25,
            up_shield_ammo: 2.0,
            up_shield_ammo_max: 2.0,
            watermelon_steven: None,
            class_checked: Vec::new(),
        }
    }
}

/// `StevenShieldState` (shield.rs): Comp_StevenShield's variables (its class defaults).
#[derive(State, Debug)]
pub struct StevenShieldState {
    /// CurrentShieldCharges.
    pub charges: Real,
    /// Shielding.
    pub shielding: bool,
    /// Ally (ResetAlly's pick): a fighter's id, -1 for None.
    pub ally: i32,
    /// OnAllyRespawn bound to these fighters' OnFighterReset (AddUnique), a bit per Fighter.id.
    pub ally_bound: Real,
}

impl Default for StevenShieldState {
    fn default() -> Self {
        StevenShieldState { charges: 3.0, shielding: false, ally: -1, ally_bound: 0.0 }
    }
}

/// `StevenTetherState` (tether.rs): empty, the tether's variables are its projectile's, the two buffs' their
/// entries'.
#[derive(State, Debug, Default)]
pub struct StevenTetherState {}

/// `StevenBarrierState` (barrier.rs): the pawn's Comp_Spawner SpawnPool of Steven_ShieldActor_C (CanBePooled).
#[derive(State, Debug, Default)]
pub struct StevenBarrierState {
    /// The released barriers waiting in the pool, each by the BlastBoxBoundsMin / Max its first BeginPlay stored
    /// (InitFromPool keeps them); CompSpawnActor takes the last (@3188, LIFO). Not in the record (the record has no
    /// pool): a World loaded from a trace starts with it empty.
    pub pool: Vec<crate::world::blast::BlastBounds>,
}

/// A pending SpawnWatermelonSteven's location (the grenade's, sim plane).
#[derive(State, Debug, Default)]
pub struct PendingSpawn {
    pub x: Real,
    pub y: Real,
}

/// `StevenWatermelonState` (watermelon.rs): the heal and the Watermelon.
#[derive(State, Debug, Default)]
pub struct StevenWatermelonState {
    /// isWatermelonSteven (the minion's own; MulticastWatermelonStevenSetUp @4621).
    pub is_watermelon_steven: bool,
    /// SpawnWatermelonSteven's LoadAsset @3143, pending until its completion (OnLoaded @6612) in the frame's leftover
    /// latent pass (watermelon.rs timer_tick). `None`: none pending.
    pub pending_spawn: Option<PendingSpawn>,
    /// BPI_Cleanup @8644 -> EarlyDestroyWatermelonSteven @8360 of the stored Watermelon, raised by the reset hook and
    /// run in the same frame's timer pass.
    pub cleanup_pending: bool,
}

/// `StevenBubbleState` (bubble.rs): empty, the actor's graph variables live on its projectile.
#[derive(State, Debug, Default)]
pub struct StevenBubbleState {}

/// `StevenGlideState` (glide.rs): Steven_AnimBP's SlowFallState and the glide list under the shapes.
#[derive(State, Debug)]
pub struct StevenGlideState {
    /// BlendListByBool on Gliding (FAlphaBlend rule): child 0's weight now, the blend's start and end weights, its
    /// time and the time run, the child last active (-1: the instance's first update, which snaps).
    pub w: Real,
    pub from_w: Real,
    pub to_w: Real,
    pub blend: Real,
    pub elapsed: Real,
    pub last: i32,
    /// SlowFallState: 0 Into, 1 Loop; the state's player time; its next update is a first update.
    pub sm: i32,
    pub sm_time: Real,
    pub sm_first: bool,
}

impl Default for StevenGlideState {
    fn default() -> Self {
        StevenGlideState {
            w: 0.0,
            from_w: 0.0,
            to_w: 0.0,
            blend: 0.0,
            elapsed: 0.0,
            last: -1,
            sm: 0,
            sm_time: 0.0,
            sm_first: true,
        }
    }
}

/// `StevenState`.
#[derive(State, Debug, Default)]
pub struct StevenState {
    pub pawn: StevenPawnState,
    pub shield: StevenShieldState,
    pub tether: StevenTetherState,
    pub barrier: StevenBarrierState,
    pub watermelon: StevenWatermelonState,
    pub bubble: StevenBubbleState,
    pub glide: StevenGlideState,
}

// ---- the call surface of the state

/// `stevenOf`: the fighter's Steven state, `None` for a fighter of another character (a failed cast to
/// Pawn_StevenUniverse / BPI_Steven).
pub fn steven_of(w: &World, f: FighterId) -> Option<&StevenState> {
    match &w.fighter(f).cs {
        CharState::Steven(s) => Some(s),
        _ => None,
    }
}

/// `stevenOf`, to write.
pub fn steven_mut(w: &mut World, f: FighterId) -> Option<&mut StevenState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Steven(s) => Some(s),
        _ => None,
    }
}

/// `steven` (his own code's read: the fighter is a Steven). A panic is a hook entered for a fighter of another
/// character.
pub fn steven(w: &World, f: FighterId) -> &StevenState {
    steven_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Steven"))
}

/// `steven`, to write.
pub fn steven_w(w: &mut World, f: FighterId) -> &mut StevenState {
    steven_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Steven"))
}
