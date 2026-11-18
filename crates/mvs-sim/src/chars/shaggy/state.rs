//! Shaggy's own fighter state (`ShaggyState`).

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::{equip_add, equip_remove, has_equipped};
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `ShaggyState`. Its `rock` is no field: the accessor reads and writes the fighter's equipped `RockMesh`.
#[derive(State, Debug)]
pub struct ShaggyState {
    pub hold_pose: bool,
    pub rage_shirt_time: Real,
    /// -1: none.
    pub rage_shirt: Real,
    /// The AnimBP's float (its 1/3 steps clamp to 0 where a double left 2^-53).
    pub kick_w: crate::Real,
    pub kick_mix: Option<KickMix>,
    pub last_snack_spawn: Option<Real>,
}

impl Default for ShaggyState {
    /// The field initial values.
    fn default() -> Self {
        ShaggyState {
            hold_pose: false,
            rage_shirt_time: 0.0,
            rage_shirt: -1.0,
            kick_w: 1.0,
            kick_mix: None,
            last_snack_spawn: None,
        }
    }
}

/// `kickMix`: the kick's blend of an anim.
#[derive(State, Debug, Default)]
pub struct KickMix {
    pub anim: Name,
    pub time: Real,
    pub w: Real,
}

// ---- the call surface of the state

/// `shaggyOf`: a fighter's Shaggy state, `None` when it is not a Shaggy.
pub fn shaggy_of(w: &World, f: FighterId) -> Option<&ShaggyState> {
    match &w.fighter(f).cs {
        CharState::Shaggy(s) => Some(s),
        _ => None,
    }
}

/// `shaggy` (his own code's read, the fighter a Shaggy), to write.
pub fn shaggy_mut(w: &mut World, f: FighterId) -> Option<&mut ShaggyState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Shaggy(s) => Some(s),
        _ => None,
    }
}

/// `ShaggyState.rock`: 'RockMesh' is in his Comp_Spawner.IsEquippedComponents (the fighter's shared list,
/// written by AnimNotify_Equip).
pub fn rock(w: &World, f: FighterId) -> bool {
    has_equipped(w, f, name!("rockmesh"))
}

/// `ShaggyState.rock = on`: true adds the name when it is not there, false takes every entry out.
// Deviation: the build's writers there use Array_Add, which can leave the name twice; one entry is kept here.
pub fn set_rock(w: &mut World, f: FighterId, on: bool) {
    let tag = name!("RockMesh");
    if !on {
        equip_remove(w, f, tag);
    } else if !rock(w, f) {
        equip_add(w, f, tag);
    }
}
