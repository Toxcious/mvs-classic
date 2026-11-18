//! Arya's own fighter state (`AryaState`): Pawn_Arya's own variables beside
//! BaseFighter's, and the state's small call surface the other files use. Offsets are Pawn_Arya's ubergraph's.
//! Every field is declared here, whichever file writes it (the comment names the owner).
//!
//! What the graph holds beside this is not sim state:
//! - StolenFaceAbilityOverride / StolenAbilityAttacks (CreateStolenAbilityAttacks @2841): built once from the attack
//!   tables, the same for every Arya of a data file (pawn.rs `stolen_pack`);
//! - the two maps of BeginPlay's capture @7137-8365 (each HitBox / HurtBox sphere's radius, each capsule's size): the
//!   class's own values, which the box set has; `hitboxes_off` alone says which of the two sizes is on;
//! - CheckingForAllyPivot, CheckingAllyAssassinate, "In Ally Bounce" (BPI_Cleanup @13519 writes them false): their
//!   true-writers and readers are classes her live data never reaches (dormant);
//! - the sword's material and submesh writes (@4135, @4915, ReceiveTick @8370): drawing.

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::latent_frames;
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `BACKSTAB`: the buff her BeginPlay gives her.
pub const BACKSTAB: Name = name!("Buff_AryaBackStab_C");
/// `EVADE_NEUTRAL`.
pub const EVADE_NEUTRAL: Name = name!("Arya_Evade_Neutral_Montage");

/// `AryaState`.
#[derive(State, Debug)]
pub struct AryaState {
    /// BranchOverrideActive (class default false): "BPI Activate Branch Override" @6011 writes it at a change alone;
    /// BPI_Cleanup @13563 clears it without the rest of the off branch (pawn.rs).
    pub branch_override_active: bool,
    /// TrackedDisguiseNotifies: the BranchWindows of the Disguise's copy that OnBranchGroupBlocked @12403 filed with
    /// the pawn's own anim instance, in the order they were added (@12475 AddUnique). The notifies are of the
    /// victim's data file (pawn.rs).
    pub tracked: Vec<NotifyRef>,
    /// ActiveDisguiseBranchesWithAttacks: + 1 @12821 / - 1 @13422 per tracked window with Branches or BranchGroups;
    /// 0 at the off branch @6544. BPI_Cleanup does not write it (pawn.rs).
    pub active_branches: i32,
    /// IsAssassinating: "BPI Set Assassinating" @5212 (AnimNotify_AryaSetAssassinating), false at BPI_Cleanup
    /// (pawn.rs).
    pub is_assassinating: bool,
    /// "BPI Disable Normal Hitboxes"(Disable) @5232, the last value written (AnimNotify_AryaDeactivateHitboxes): her
    /// Mesh's HitBox / HurtBox shapes are at radius 0 (true) or at the sizes BeginPlay stored (false) (pawn.rs).
    pub hitboxes_off: bool,
    /// ReceiveBeginPlay's Delay(1.0) @8663, in pawn ticks left; 0: it has fired (its resume @35 ran) (pawn.rs).
    pub begin_delay: Real,
    /// OnPlayerKnockedOut is bound to the game mode's PlayerKnockedOut (@266-289, in that resume, with more than one
    /// player) (pawn.rs).
    pub ko_bound: bool,
    /// OnPlayerKnockedOut's DoOnce @8984 is closed (opened again by its Delay(2.0)'s resume @331 alone) (pawn.rs).
    pub ko_closed: bool,
    /// That Delay(2.0) @10971, in pawn ticks left; 0: none (pawn.rs).
    pub ko_delay: Real,
}

impl Default for AryaState {
    /// The field initial values.
    fn default() -> Self {
        AryaState {
            branch_override_active: false,
            tracked: Vec::new(),
            active_branches: 0,
            is_assassinating: false,
            hitboxes_off: false,
            begin_delay: latent_frames(1.0),
            ko_bound: false,
            ko_closed: false,
            ko_delay: 0.0,
        }
    }
}

// ---- the call surface of the state

/// `aryaOf`: a fighter's Arya state, `None` when it is not an Arya (a failed cast to BPI_Arya).
pub fn arya_of(w: &World, f: FighterId) -> Option<&AryaState> {
    match &w.fighter(f).cs {
        CharState::Arya(s) => Some(s),
        _ => None,
    }
}

/// `aryaOf`, to write.
pub fn arya_mut(w: &mut World, f: FighterId) -> Option<&mut AryaState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Arya(s) => Some(s),
        _ => None,
    }
}

/// `arya` (her own code's read, the fighter an Arya). A panic is a hook entered for a fighter of another character.
pub fn arya(w: &World, f: FighterId) -> &AryaState {
    arya_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not an Arya"))
}

/// `arya`, to write.
pub fn arya_w(w: &mut World, f: FighterId) -> &mut AryaState {
    arya_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not an Arya"))
}
