//! Harley Quinn's own fighter state (`HarleyState`): HarleyQuinn_AnimBP's linked
//! idle layer. What Pawn_Harley and the AnimBP hold beside it is not sim state (state.rs's header); her bomb in the
//! hand is the fighter's equipped list, her spawned actors keep their state on themselves (bomb.rs, actors.rs).

use crate::State;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::world::World;

/// `HarleyState`.
#[derive(State, Debug, Default)]
pub struct HarleyState {
    /// HarleyQuinn_AnimBP SetIdleAnim (@0-115), bound to the fighter's ItemEquipped and ItemUnequipped (@467-554): the
    /// linked layer's Anim_Idle is HarleyQuinn_Nav_Idle_HoldItem (true) or HarleyQuinn_Nav_Idle (false; the layer's own
    /// default), by BPI_IsHoldingSatchel at that broadcast. Written by her `equip` hook only (pawn.rs); a new AnimBP
    /// instance has its default (`world::reset`).
    pub hold_idle: bool,
}

// ---- the call surface of the state

/// `harleyOf`: a fighter's Harley state, `None` when it is not a Harley Quinn.
pub fn harley_of(w: &World, f: FighterId) -> Option<&HarleyState> {
    match &w.fighter(f).cs {
        CharState::Harley(s) => Some(s),
        _ => None,
    }
}

/// `harleyOf`, to write.
pub fn harley_mut(w: &mut World, f: FighterId) -> Option<&mut HarleyState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Harley(s) => Some(s),
        _ => None,
    }
}

/// `harley` (her own code's read, the fighter a Harley Quinn). A panic is a hook entered for a fighter of another
/// character.
pub fn harley(w: &World, f: FighterId) -> &HarleyState {
    harley_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Harley Quinn"))
}

/// `harley`, to write.
pub fn harley_w(w: &mut World, f: FighterId) -> &mut HarleyState {
    harley_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Harley Quinn"))
}
