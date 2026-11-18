//! Taz's own fighter state (`TazState`): Pawn_C015's own variables beside
//! BaseFighter's. Offsets are of Pawn_C015's ubergraph.

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::combat::BuffRef;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;

/// An entry of EatenFighters (`{ f, v, slot }`): the fighter, its value, the pair's index in the map's sparse element
/// array.
#[derive(State, Debug)]
pub struct EatenFighter {
    pub f: FighterId,
    pub v: Real,
    pub slot: i32,
}

/// OffsetSpitFighter's pending Delay(0.0) (@17863 -> @605) with the event's parameters as the last call left them.
#[derive(State, Debug)]
pub struct SpitOffset {
    pub vic: FighterId,
    pub dx: Real,
    pub dy: Real,
}

/// `TazState`.
#[derive(State, Debug)]
pub struct TazState {
    // ---- the eat (eat.rs, pawn.rs)
    /// EatLevel: the meter, 0..1.
    pub eat_level: Real,
    /// EatenFighters (TMap<BP_FighterCharacter, int>) in Map_Keys order (by `slot`); eat_free: the freed indices (the
    /// last one is taken next); eat_slots: the element array's length.
    pub eaten_fighters: Vec<EatenFighter>,
    pub eat_free: Vec<i32>,
    pub eat_slots: i32,
    /// A BPI_SpitFighters(Hit) call of this frame's AnimNotify_C015SpitFighter, run by the World after the mesh ticks
    /// (eat.rs spit_fighters): the notify, whose Hit (the montage's data) the record writes. `None`: none.
    pub spit_hit: Option<NotifyRef>,
    /// The pending Delay(0.0) of OffsetSpitFighter. `None`: none pending.
    pub spit_offset: Option<SpitOffset>,

    // ---- the projectile eat and the burp (projectile_eat.rs, burp.rs)
    /// ProjectilesBeingEaten: BPI_SetIsEatingProjectile(true) +1, (false) -1 clamped >= 0.
    pub projectiles_being_eaten: Real,
    /// StoredProjectile (a class name, empty = None: the getter substitutes C015Trash), StoredStartingSpeed /
    /// StoredMaxSpeed: SetIsEatingProjectile's storage @6521-7307, ResetStoredProjectile @442-546.
    pub stored_projectile: crate::names::Name,
    pub stored_starting_speed: Real,
    pub stored_max_speed: Real,
    /// BurpingProjectile, HittingProjectiles.
    pub burping_projectile: bool,
    pub hitting_projectiles: bool,

    // ---- the tornado (tornado.rs)
    /// TornadoActive, TazTornadoMovementOn, IsNonCooldownTornado, TornadoingAlly.
    pub tornado_active: bool,
    pub tornado_movement_on: bool,
    pub is_non_cooldown_tornado: bool,
    pub tornadoing_ally: bool,
    /// HitAlly, HitAllies (CheckForAllyInFront's AddUnique), ListeningForAllyHits.
    pub hit_ally: bool,
    pub hit_allies: Vec<FighterId>,
    pub listening_for_ally_hits: bool,
    /// TazTornadoBuff: the Buff_C015Tornado SetTornadoMovementMode spawned (@5794). Read through IsValid.
    pub tornado_buff: Option<BuffRef>,
    /// TornadoTimer (EndTornadoTimer @5997 / @15014): the handle is valid; the fighter tick whose timer pass fires it
    /// (-1: none); the tick it was set in and its Rate.
    pub timer_valid: bool,
    pub timer_at: i32,
    pub timer_set: i32,
    pub timer_rate: Real,
    /// OnAllyHit's DoOnce (@14729-14789, ResetTornadoHitAlly @17213 opens it).
    pub ally_hit_closed: bool,

    // ---- the dogpile (dogpile.rs)
    /// DogPileActive (BPI_SetControllingDogpile).
    pub dog_pile_active: bool,
    /// DogPileTimer: the fighter tick its DogPileExpire fires at, -1 while not active.
    pub dog_pile_at: i32,
    /// The fighter tick whose timer pass makes AnimNotify_C015Dogpile's held-back NotifyEnd; -1: none held.
    pub dog_pile_end_at: i32,
    /// HurtBoxFightCloud's collision (BPI_ActivateFightCloudHurtBox).
    pub fight_cloud_hurt: bool,

    // ---- Tasty and the chicken (tasty.rs, chicken.rs)
    /// SpawningMeleeBuffActor (BPI_SetListenForTastyBuffGiver).
    pub spawning_melee_buff_actor: bool,

    // ---- the sim pose (pose.rs): C015_AnimBP's slot graph inputs and the mix
    pub slot_mix: SlotMix,
    /// C015_AnimBP's BlueprintUpdateAnimation variables (@3830).
    pub run_value: Real,
    pub turning_blend: Real,
    pub was_facing_left: bool,
    pub landing_blend: Real,
    pub moving_in_air_blend: Real,
    pub was_moving_on_ground: bool,
    pub percent_charged: Real,
    pub aim_direction: Real,
}

impl Default for TazState {
    fn default() -> Self {
        TazState {
            eat_level: 0.0,
            eaten_fighters: Vec::new(),
            eat_free: Vec::new(),
            eat_slots: 0,
            spit_hit: None,
            spit_offset: None,
            projectiles_being_eaten: 0.0,
            stored_projectile: crate::names::Name::default(),
            stored_starting_speed: 0.0,
            stored_max_speed: 0.0,
            burping_projectile: false,
            hitting_projectiles: false,
            tornado_active: false,
            tornado_movement_on: false,
            is_non_cooldown_tornado: false,
            tornadoing_ally: false,
            hit_ally: false,
            hit_allies: Vec::new(),
            listening_for_ally_hits: false,
            tornado_buff: None,
            timer_valid: false,
            timer_at: -1,
            timer_set: 0,
            timer_rate: 0.0,
            ally_hit_closed: false,
            dog_pile_active: false,
            dog_pile_at: -1,
            dog_pile_end_at: -1,
            fight_cloud_hurt: false,
            spawning_melee_buff_actor: false,
            slot_mix: SlotMix::default(),
            run_value: 0.0,
            turning_blend: 0.0,
            was_facing_left: false,
            landing_blend: 0.0,
            moving_in_air_blend: 0.0,
            was_moving_on_ground: false,
            percent_charged: 0.0,
            aim_direction: 0.0,
        }
    }
}

// ---- the call surface of the state

/// `tazOf`: the fighter's Taz state, `None` for a fighter of another character.
pub fn taz_of(w: &World, f: FighterId) -> Option<&TazState> {
    match &w.fighter(f).cs {
        CharState::Taz(s) => Some(s),
        _ => None,
    }
}

/// `tazOf`, to write.
pub fn taz_mut(w: &mut World, f: FighterId) -> Option<&mut TazState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Taz(s) => Some(s),
        _ => None,
    }
}

/// `taz` (his own code's read). A panic is a hook entered for a fighter of another character.
pub fn taz(w: &World, f: FighterId) -> &TazState {
    taz_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Taz"))
}

/// `taz`, to write.
pub fn taz_w(w: &mut World, f: FighterId) -> &mut TazState {
    taz_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Taz"))
}
