//! The host core's ways out: a player's link (its messages) and the referee of its matches. The time
//! is the caller's `now`; the core's beats keep the steady rule of `STEADY_CATCH_UP` inside its own timer queue.

use mvs_protocol::messages::HostWhy;

/// A link the core was given, by `Core::connect`: a second link of one player id is another key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnKey(pub u64);

/// A player's link to the host: messages on a reliable, ordered channel (a match's packets go between the games and
/// never pass the core). `id` is the player's as the link vouches for it (never a message's word); `name` what the
/// link knows the player by ('' none) until their hello names them.
pub trait Connection: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    /// A message: a JSON text `{ t, ... }` (`mvs_protocol::messages::ServerMessage`). Err: the link is broken (its
    /// text is logged); the core sends it nothing more and closes it, 'link error', at the end of the call it broke
    /// into, so that the call runs to its end for everyone else.
    fn send(&mut self, text: &str) -> Result<(), String>;
    /// The core ends the link (a second call does nothing). The core takes the link as closed itself, as the host's
    /// close reports: a `Core::closed` for it afterwards does nothing. `why` goes to the player as the
    /// link's last word (a guest's FRAME_BYE). A link that closes by itself during one of the core's calls is
    /// reported by its owner with `Core::closed` once that call returns.
    fn close(&mut self, why: HostWhy);
}

/// A human of a match as the referee takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefereeHuman {
    pub slot: u32,
    pub id: String,
}

/// A match's go: `match_id` the core's number for it, its slot count (its bots' among them), its humans by slot, its
/// tick 0 on the core's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct RefereeStart {
    pub match_id: u32,
    pub slots: u32,
    pub humans: Vec<RefereeHuman>,
    pub tick0: f64,
}

/// Who referees the core's matches (`mvs_protocol::referee`): the net process, in its own process or by asking a
/// relay. The core says when a match goes, who leaves it and when it ends; the packets never pass the core. A human
/// whose game has sent nothing for SILENT_MS comes back as `Core::away(match, slot, now)` (and `Core::back`), one gone
/// for RESUME_MS more as `Core::gone(match, slot, now)`.
pub trait MatchReferee: Send {
    /// Returns the id every human's game sends the match's packets to (match.go's referee): the host's own, or a
    /// relay's referee.
    fn start(&mut self, m: &RefereeStart, now: f64) -> String;
    /// A human left the match: its stream ends at the ruled line.
    fn left(&mut self, match_id: u32, slot: u32);
    /// The match is over for the core.
    fn end(&mut self, match_id: u32);
    /// Whether one more match may start now, beside the `pending` ones this core has found that have not started
    /// yet (in their pre-match or loading; false: the relay referees as many as it may). Asked when a queue group is
    /// found and at a lobby.startMatch with the core's pending count, and at a match's go with 0 (a place for it
    /// now): while it is false the queue holds its tickets, a lobby.startMatch is refused, and a match at its go is
    /// ended for its peers (match.left 'error').
    fn room(&self, pending: usize) -> bool {
        let _ = pending;
        true
    }
}

/// The beats one wake-up of a steady repeating timer runs at most; past them it starts counting again from now. A
/// steady timer aims each run at the next beat (start + n * ms): a late wake-up shortens the next wait instead of
/// adding to it.
pub const STEADY_CATCH_UP: u32 = 4;
