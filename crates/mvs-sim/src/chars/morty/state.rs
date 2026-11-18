//! Morty's own fighter state (`MortyState`): Pawn_C019's and C019_AnimBP's own
//! variables beside BaseFighter's. Every field is declared here, whichever file writes it (the comment names the
//! owner); the World's tick writes the two lists (`world::spawning`, [`set_lists`]).

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::{SlotMix, SlotPose};
use crate::world::World;

/// `MortyState`.
#[derive(State, Debug)]
pub struct MortyState {
    /// The C019_CookGrenade actors of the fighter's Comp_Spawner, then of its allies' (spawn order), as they are ahead
    /// of the frame's projectile movement (the World's tick writes it). Made again every tick: not state (the hooks'
    /// stateSkip), but kept in the rollback state.
    pub team_grenades: Vec<ProjectileId>,
    /// A live C019Marker of this fighter's spawner, listed by the world like the grenades. Not in the record either.
    pub has_marker: bool,
    /// c019CookGrenadeOutProjectileCondition's Behind variable (pawn.rs's conditions).
    pub cook_grenade_behind: bool,
    /// C019_AnimBP's slot graph (`fighter::anim::slot_mix` blend_slots): the weight of BlendListByBool(IsMoving)'s
    /// moving side and of BlendListByBool(IsMovingBackwards)'s, -1 until that list has been updated.
    pub slot_move_w: Real,
    pub slot_back_w: Real,
    /// The poses mixed into the fighter's pose in order (CharHooks.slot_mix); `slot_blend` and `slot_players` stay
    /// `None`.
    pub slot_mix: SlotMix,
    /// `Fighter.sampleBoxes`' buffer for a `slot_mix` pose. Not state.
    #[state(skip)]
    pub mix_buf: Option<Vec<f32>>,
}

impl Default for MortyState {
    /// The field initial values.
    fn default() -> Self {
        MortyState {
            team_grenades: Vec::new(),
            has_marker: false,
            cook_grenade_behind: false,
            slot_move_w: 0.0,
            slot_back_w: -1.0,
            slot_mix: SlotMix::default(),
            mix_buf: None,
        }
    }
}

// ---- the call surface of the state

/// `mortyOf`: a fighter's Morty state, `None` when it is not a Morty.
pub fn morty_of(w: &World, f: FighterId) -> Option<&MortyState> {
    match &w.fighter(f).cs {
        CharState::Morty(s) => Some(s),
        _ => None,
    }
}

/// `mortyOf`, to write.
pub fn morty_mut(w: &mut World, f: FighterId) -> Option<&mut MortyState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Morty(s) => Some(s),
        _ => None,
    }
}

/// `morty` (his own code's read, the fighter a Morty). A panic is a hook entered for a fighter of another character.
pub fn morty(w: &World, f: FighterId) -> &MortyState {
    morty_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Morty"))
}

/// `morty`, to write.
pub fn morty_w(w: &mut World, f: FighterId) -> &mut MortyState {
    morty_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Morty"))
}

/// `World.step`'s writes of `teamGrenades` and `hasMarker`.
pub fn set_lists(w: &mut World, f: FighterId, team_grenades: &[ProjectileId], has_marker: bool) {
    let Some(m) = morty_mut(w, f) else { return };
    m.team_grenades.clear();
    m.team_grenades.extend_from_slice(team_grenades);
    m.has_marker = has_marker;
}

/// `mortyOf(f)`'s `slotMoveW` and `slotBackW` (read by the AnimBP slot code); `None` for a fighter that is no Morty.
pub fn morty_slot_weights(w: &World, f: FighterId) -> Option<(Real, Real)> {
    morty_of(w, f).map(|m| (m.slot_move_w, m.slot_back_w))
}

/// `mortyOf(f).slotMix` (written by the AnimBP slot code); the caller has checked the fighter is a Morty.
// Unverified: the TwoWayBlend's alpha is taken as clamped to [0, 1] (FInputScaleBias); the nested blends are
// flattened to one weighted sum; the hurtboxes and sockets mix the sampled box tracks linearly with the same weights;
// the inner bool blend is taken to advance only while the moving side has weight, starting at its bool's value.
pub fn morty_slot_mix(w: &mut World, f: FighterId) -> &mut Vec<SlotPose> {
    &mut morty_w(w, f).slot_mix.slot_mix
}

/// CharHooks.slotMix (`mod.rs` `slotMix: (f) => f.cs as MortyState`).
pub(super) fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&morty(w, f).slot_mix)
}

/// CharHooks.slot_mix_mut: `slotMix`, written.
pub(super) fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    morty_mut(w, f).map(|s| &mut s.slot_mix)
}
