//! Bugs Bunny's own fighter state (`BugsState`): Pawn_BugsBunny's own variables
//! (BugsBunnyV2, EFighter 12) beside BaseFighter's, and BugsBunny_AnimBP's glide list. Offsets are Pawn_BugsBunny's
//! ubergraph's. One record (one class); the owner of each field is named on it.
//!
//! What the pawn holds beside this is not sim state, or nothing reads it (the list below): HulkAnimBP,
//! OriginalWalkSpeed, OriginalAirControl, SpeedOn, BombEquipped?, Use 2D Hitboxes (CharDef.use2D), the AnimBP's
//! HeadScale (Fighter.headScale).

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::names::Name;
use crate::world::World;

/// `BugsState`.
#[derive(State, Debug)]
pub struct BugsState {
    /// isGliding: ReadGlideInput @2084, SetGlideValueMulticast @3210, BPI_Cleanup @4046 (false). pawn.rs.
    pub gliding: bool,
    /// BugsBunny_AnimBP.IsGliding (BPI_SetIsGliding from ReadGlideInput @2234 and SetGlideValueMulticast @3346): not
    /// the pawn's, BPI_Cleanup clears isGliding alone. pawn.rs writes, glide.rs reads.
    pub anim_gliding: bool,
    /// The glide BlendListByBool (glide.rs): child 0's weight (child 1's is 1 - it), the running FAlphaBlend's start
    /// and end weights, its time and the time run.
    pub glide_w: Real,
    pub glide_from_w: Real,
    pub glide_to_w: Real,
    pub glide_blend: Real,
    pub glide_elapsed: Real,
    /// The BugsBunny_Nav_PropellerEars player's time, and its advance not yet dispatched to the sequence notifies
    /// (`fighter::anim` seqNotifyTick). glide.rs.
    pub glide_time: Real,
    pub glide_seq_from: Real,
    pub glide_seq_delta: Real,
    /// The glide list's active child, -1 before the first update (it snaps). glide.rs.
    pub glide_last: i32,
    /// ReadGlideInput's DoOnce on entering the glide (@2781-3003; StartClosed false @975). pawn.rs.
    pub glide_once_closed: bool,
    /// OriginalGravityScale: BeginPlay @642 saves CharacterMovement.GravityScale (the class default 5.25 before).
    /// pawn.rs.
    pub original_gravity_scale: Real,
    /// SkipImpulseValidation: BPI_TunnelPunch @420 true. Read by BaseFighter ValidateMoveCommand alone, which the port
    /// does not have. pawn.rs.
    pub skip_impulse_validation: bool,
    /// "As Lingering Love Projectile": BPI_SetBugsLingeringKiss @400 (pawn.rs `set_lingering_kiss`). Never cleared: a
    /// destroyed kiss stays referenced (IsValid is false then).
    pub lingering_kiss: Option<ProjectileId>,
    /// ItemEquip: RandomItem @3515, which nothing calls, so it stays None. Read by AnimNotify_BugsBunnyEquipRandom (in
    /// no montage of his data).
    pub item_equip: Name,
}

impl Default for BugsState {
    fn default() -> Self {
        BugsState {
            gliding: false,
            anim_gliding: false,
            glide_w: 0.0,
            glide_from_w: 0.0,
            glide_to_w: 0.0,
            glide_blend: 0.0,
            glide_elapsed: 0.0,
            glide_time: 0.0,
            glide_seq_from: 0.0,
            glide_seq_delta: 0.0,
            glide_last: -1,
            glide_once_closed: false,
            original_gravity_scale: crate::consts::GRAVITY_SCALE,
            skip_impulse_validation: false,
            lingering_kiss: None,
            item_equip: crate::name!("None"),
        }
    }
}

// ---- the call surface of the state

/// `bugsOf`: the fighter's Bugs Bunny state, `None` for a fighter of another character (a failed cast to
/// Pawn_BugsBunny / BPI_BugsBunny).
pub fn bugs_of(w: &World, f: FighterId) -> Option<&BugsState> {
    match &w.fighter(f).cs {
        CharState::Bugs(s) => Some(s),
        _ => None,
    }
}

/// `bugsOf`, to write.
pub fn bugs_mut(w: &mut World, f: FighterId) -> Option<&mut BugsState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Bugs(s) => Some(s),
        _ => None,
    }
}

/// `bugs` (his own code's read: the fighter is a Bugs Bunny). A panic is a hook entered for a fighter of another
/// character.
pub fn bugs(w: &World, f: FighterId) -> &BugsState {
    bugs_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Bugs Bunny"))
}

/// `bugs`, to write.
pub fn bugs_w(w: &mut World, f: FighterId) -> &mut BugsState {
    bugs_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Bugs Bunny"))
}
