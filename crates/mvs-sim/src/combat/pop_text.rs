//! The pop-up texts: a Blueprint graph's UI_PopTextParams (a MakeStruct ahead of
//! UI_Functions PopText or BPI_PopText) as the 'popText' event of kind `NotifyText` carries it, by the field names of
//! AnimNotify_PopText's struct. The words are never the sim's: `text` is the constant's Game.locres key or a class
//! default's text as the fighter data holds it.

use crate::Real;
use crate::events::{Event, PopKind, PopParams};
use crate::fighter::FighterId;
use crate::names::Name;
use crate::world::World;

/// `GraphPopText`: a graph's UI_PopText call.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct GraphPopText {
    /// Text: its Game.locres key, and a class default's SourceString / LocalizedString.
    pub text_key: Name,
    pub source_string: Option<Name>,
    pub localized_string: Option<Name>,
    pub lifetime: Real,
    /// Color: R, G, B, A.
    pub color: [Real; 4],
    /// Location: the offset from the attach actor (space 3), or the world point (space 0, no attach actor).
    pub location: [Real; 3],
    /// UI_ScreenSpace by value: 3 attached to the event's fighter, 0 the world point of `location`.
    pub space: u8,
    pub scale: Real,
    /// 0, 1 or 2.
    pub justification: u8,
    pub delay: Real,
    /// Posted by UI_PopText Show.
    pub audio: Option<Name>,
    /// BPI_PopText on the owner (BaseFighter AddPopTextToQueue: the pawn's queue), else UI_Functions PopText at once.
    pub queued: bool,
    /// Shown only where the owner's Instigator IsLocallyControlled (the graph's test ahead of the call).
    pub local_only: bool,
    /// FText Format's named arguments (FormatArgumentData of ArgumentValueType Int).
    pub args: Vec<(Name, Real)>,
}

// ---- the call surface of the pop texts

/// `popTextOn`: the 'popText' event of `p` on `f` (the attach actor, or whose queue a
/// queued text joins).
pub fn pop_text_on(w: &mut World, f: FighterId, p: &GraphPopText) {
    let fighter = w.fighter(f).id;
    let params = Some(PopParams::Graph(Box::new(p.clone())));
    // (the fighter's queue, flushed into the frame's with its other events)
    w.fighter_mut(f).events.push(Event::PopText {
        fighter,
        kind: PopKind::NotifyText,
        stacks: 0.0,
        gain: None,
        params,
    });
}
