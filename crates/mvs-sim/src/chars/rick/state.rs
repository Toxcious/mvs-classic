//! Rick's own fighter state (`RickState`): Pawn_C020's own variables beside
//! BaseFighter's, the C020_AnimBPV2 slot graph's inputs, and Comp_Pawn_Portal's variables. Offsets are Pawn_C020's
//! ubergraph's unless named. The other files keep their per-actor state on their actors and reach the pawn through
//! pawn.rs's BPI functions and the joined hooks (mod.rs).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::world::World;

/// `PortalComp`: Comp_Pawn_Portal's variables (portal_comp.rs).
#[derive(State, Debug, Default)]
pub struct PortalComp {
    /// AlliedPortals: the portal actors (portal.rs's `PortalActor` world actors).
    pub allied: Vec<ActorRef>,
    /// TeleportVisuals' Delay(0.5): frames left (0: not running).
    pub exit_left: Real,
    /// K2Node_CustomEvent_TeleportVFXLocation: the last call's ExitLocation (sim x, y).
    pub vfx_x: Real,
    pub vfx_y: Real,
}

/// `RickState`.
#[derive(State, Debug)]
pub struct RickState {
    /// ListeningForAbilityHit: BPI_ListenForAbilityHit(On) @2177 (AnimNotify_C020ListenForAbilityHit's begin true,
    /// end false); OnHit reads it @2197. pawn.rs.
    pub listening: bool,
    /// C020_AnimBPV2 ChargeAmount: the pawn's GetChargeMulticast @1059 (ChargeEnd's GetUpdatedPercentCharged), 0 at
    /// every montage start (OnMontageStarted_Event_0 @3788). LongWhip's TwoWayBlend alpha. pawn.rs.
    pub charge_amount: Real,
    /// C020_AnimBPV2 Aim Direction: 0.5 at the anim instance's initialization @1576, BPI_SetAimDirection's
    /// FClamp(v, 0, 1) @3600-3647. TwoWayBlend_1's alpha, and TwoWayBlend_2's through clamp(|aim - 0.5| x 2). pawn.rs.
    pub aim_direction: Real,
    /// C020_AnimBPV2's slot graph mix (`fighter::anim` graph_slots writes it at each pose update from pawn.rs's
    /// slot_input).
    pub slot_mix: SlotMix,
    /// Comp_Pawn_Portal's variables (portal_comp.rs).
    pub portal_comp: PortalComp,
}

impl Default for RickState {
    fn default() -> Self {
        RickState {
            listening: false,
            charge_amount: 0.0,
            aim_direction: 0.5,
            slot_mix: SlotMix::default(),
            portal_comp: PortalComp::default(),
        }
    }
}

// ---- the call surface of the state

/// `rickOf`: the fighter's Rick state, `None` for a fighter of another character (a failed cast to BPI_C020 /
/// C020_AnimBPV2).
pub fn rick_of(w: &World, f: FighterId) -> Option<&RickState> {
    match &w.fighter(f).cs {
        CharState::Rick(s) => Some(s),
        _ => None,
    }
}

/// `rickOf`, to write.
pub fn rick_mut(w: &mut World, f: FighterId) -> Option<&mut RickState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Rick(s) => Some(s),
        _ => None,
    }
}

/// `rick` (his own code's read: the fighter is a Rick). A panic is a hook entered for a fighter of another character.
pub fn rick(w: &World, f: FighterId) -> &RickState {
    rick_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Rick"))
}

/// `rick`, to write.
pub fn rick_w(w: &mut World, f: FighterId) -> &mut RickState {
    rick_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Rick"))
}
