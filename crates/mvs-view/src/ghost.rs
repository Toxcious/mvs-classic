//! (the port's own) The Chaos rule Ghosts as one screen sees it: a fighter the rule hides (`own_rules::ghost_hidden`)
//! is hidden whole from the players of other machines and from the bots (its model, its name tag and damage over it,
//! its pop texts, the effects on it or spawned at it), and drawn as its outline alone for a player of this machine,
//! who still has to play it. The views, the HUD and the effects all read [`ghost`].

use mvs_sim::World;
use mvs_sim::fighter::FighterId;

/// How this screen shows a fighter under Ghosts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ghost {
    /// Drawn as any fighter (the rule off, or the fighter attacks, is hit or flashes).
    #[default]
    Shown,
    /// Nothing gives its place away.
    Hidden,
    /// A fighter of this machine's players: its outline alone; its tag and effects stay.
    Outline,
}

/// Whether fighter `f` is played by a human on this machine: a human's slot offline (every local player of a shared
/// screen), the local player's slot `me` alone online. Under Body Swap the slot is the driving player's; a minion or
/// an unbound mode pawn is nobody's.
pub fn local_human(w: &World, f: FighterId, me: usize, online: bool) -> bool {
    let Ok(slot) = usize::try_from(mvs_sim::world::pawns::slot_of(w, f)) else { return false };
    let slot = if slot < w.players.len() { mvs_sim::world::wheel_events::input_slot(w, slot) } else { slot };
    let human = w.setup.players.get(slot).is_some_and(|p| !p.bot);
    human && (!online || slot == me)
}

/// How this screen (local player `me`, `online` a relay match) shows fighter `f` this tick.
pub fn ghost(w: &World, f: FighterId, me: usize, online: bool) -> Ghost {
    if !mvs_sim::world::own_rules::ghost_hidden(w, f) {
        Ghost::Shown
    } else if local_human(w, f, me, online) {
        Ghost::Outline
    } else {
        Ghost::Hidden
    }
}
