//! Black Adam's own fighter state (`BlackAdamState`, and
//! `LightningManager`): Pawn_C021's own variables beside BaseFighter's and C021_AnimBP's sim-side inputs. Offsets are
//! Pawn_C021's ubergraph's. Every field is declared here, whichever file writes it (the comment names the owner).
//!
//! What the pawn holds beside this is not sim state or has no reader (state.rs's header): the AnimBP reference,
//! SupermanCapsuleHight / Radius, the ground-check variables of BPI_SetGroundPositions (never called), the heart
//! skin's material and emitters, Meter Added On Hit.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;
use indexmap::IndexMap;

/// Comp_C021_LightningManager (`LightningManager`): TargetedActors (the latest AttackID that chained each actor) and
/// ThrowDirections (ThrowUp per AttackID), each in its insertion order (as a map keeps them: a write to a key already
/// there
/// keeps its place; a removal is `shift_remove`). ResetLightningManager clears both (chain.rs, the `reset` hook).
#[derive(State, Debug, Default)]
pub struct LightningManager {
    pub targets: IndexMap<ActorRef, i32>,
    pub dirs: IndexMap<i32, bool>,
}

/// `BlackAdamState`.
#[derive(State, Debug)]
pub struct BlackAdamState {
    /// C021_AnimBP.Hover Direction (x forward of his facing, y up): ReceiveTick @1986-2197 writes it every tick from
    /// Comp_Pawn_Fly.Last Direction (pawn.rs).
    pub hover_x: Real,
    pub hover_y: Real,
    /// C021_AnimBP's four BlendListByInt (MoveBack, MoveForward, MoveDown, MoveUp): the child-1 weights and the last
    /// active index, -1 until the list's first update (anim.rs).
    pub list_w: [Real; 4],
    pub list_index: [i32; 4],
    /// The tracks mixed into the fighter's pose in order (CharHooks.slot_mix, anim.rs); `slot_blend` and
    /// `slot_players` stay `None`.
    pub slot_mix: SlotMix,
    /// `Fighter.sampleBoxes`' buffer for a `slot_mix` pose. Not state.
    #[state(skip)]
    pub mix_buf: Option<Vec<f32>>,
    /// His Comp_C021_LightningManager (chain.rs).
    pub lightning: LightningManager,
}

impl Default for BlackAdamState {
    /// The field initial values.
    fn default() -> Self {
        BlackAdamState {
            hover_x: 0.0,
            hover_y: 0.0,
            list_w: [0.0; 4],
            list_index: [-1; 4],
            slot_mix: SlotMix::default(),
            mix_buf: None,
            lightning: LightningManager::default(),
        }
    }
}

// ---- the call surface of the state

/// `blackAdamOf`: a fighter's Black Adam state, `None` when it is not a Black Adam (a failed cast to Pawn_C021 /
/// BPI_C021).
pub fn black_adam_of(w: &World, f: FighterId) -> Option<&BlackAdamState> {
    match &w.fighter(f).cs {
        CharState::BlackAdam(s) => Some(s),
        _ => None,
    }
}

/// `blackAdamOf`, to write.
pub fn black_adam_mut(w: &mut World, f: FighterId) -> Option<&mut BlackAdamState> {
    match &mut w.fighter_mut(f).cs {
        CharState::BlackAdam(s) => Some(s),
        _ => None,
    }
}

/// `blackAdam` (his own code's read, the fighter a Black Adam). A panic is a hook entered for a fighter of another
/// character.
pub fn black_adam(w: &World, f: FighterId) -> &BlackAdamState {
    black_adam_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Black Adam"))
}

/// `blackAdam`, to write.
pub fn black_adam_w(w: &mut World, f: FighterId) -> &mut BlackAdamState {
    black_adam_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Black Adam"))
}
