//! Taz's (Pawn_C015, C015) HUD calls in the sim: AnimNotify_C015EatUI and the pawn's
//! BPI_SetEatUI. The labels themselves are the HUD's, his meter's feed is read off `TazState.eat_level`.

use super::taz_of;
use crate::events::{Event, NotifyRef};
use crate::fighter::FighterId;
use crate::fighter::match_side::{notify_props, post_vo};
use crate::name;
use crate::registry::props::flag;
use crate::registry::{NotifyHandler, Registry, RegistryError};
use crate::world::World;

/// Pawn_C015 BPI_SetEatUI(Active, OnGround) @1128-3870. Nondedicated and locally controlled only (@1128-1240): every
/// pawn offline (an AI pawn is locally controlled in standalone); the event goes out for each and the UI keeps its
/// own locality test for the labels.
/// Active: the Chew label on the ground, then the Spit label (@1264-3373), then PF_Post_VO_Event(self,
/// ability_sandwich, no context, 0.2, AlwaysPlayVO false, PlayOnlyLocally false) @3409, reached only past a valid Spit
/// widget (@3228-3257: pop_flow when PopInputLabel made none, which it does for a pawn that is bot controlled,
/// UI_Functions PopInputLabel @213-372). With no context fighter Lib_Audio asks the VO actor (the pawn) for
/// BPI_Character_Viewer and plays with no context tags (as C020_Buff_Silenced's call). Inactive: the labels' Exit
/// (@3474-3759), nothing else.
// Deviation: online, the call runs on the owning client alone (locally controlled), so its RandomFloat and the line are
// that machine's; the port's sim runs it on every peer for a pawn that is not a bot's (the shared stream draws, the
// page posts the line where its sound rules post it).
fn set_eat_ui(w: &mut World, f: FighterId, active: bool, on_ground: bool) {
    let id = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::EatUi { fighter: id, on: active, on_ground });
    if !active || w.fighter(f).controls.bot_controlled {
        return;
    }
    // Deviation (`post_vo`'s rule): the pawn's own stream draws the RandomFloat
    post_vo(w, f, name!("ability_sandwich"), None, 0.2);
}

/// AnimNotify_C015EatUI, a notify state: Received_NotifyBegin @215 the mesh owner's BPI_SetEatUI(true, On Ground),
/// Received_NotifyEnd @215 BPI_SetEatUI(false, On Ground), behind DoesImplementInterface(BPI_C015) and the cast (a
/// Taz; on another fighter's mesh, a Disguise's copy of his montage, nothing). On Ground: the notify's property (class
/// default false): true in C015_Eat_Ground_Montage2 and C015_Down_Special_Ground_Montage, unset in the two air
/// montages.
fn eat_ui_begin(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if taz_of(w, f).is_some() {
        let g = flag(notify_props(w, f, n), "On Ground");
        set_eat_ui(w, f, true, g);
    }
}

fn eat_ui_end(w: &mut World, f: FighterId, n: NotifyRef, _idx: i32) {
    if taz_of(w, f).is_some() {
        let g = flag(notify_props(w, f, n), "On Ground");
        set_eat_ui(w, f, false, g);
    }
}

/// This file's registry entries (AnimNotify_C015EatUI_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_C015EatUI_C",
        NotifyHandler { begin: Some(eat_ui_begin), end: Some(eat_ui_end), ..NotifyHandler::default() },
    )
}
