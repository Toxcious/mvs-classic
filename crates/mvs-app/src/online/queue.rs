//! The online queue: queue.join /
//! queue.cancel / pm.perks / pm.leave to the host core of the game's queue session, queue.state / queue.found /
//! match.start from it, in place of the build's PGI_MatchmakingHandler and its native matchmaking events
//! (OnMatchmakingStarted, OnMatchmakingCancel, OnAllPlayersRegistered); and the session the PreMatch level runs on
//! the queue's path, from the mode card's Confirm to the found match's start, with the party's two pages.
//!
//! Deviation: callbacks and promises are `QOut` items and the session's state; the timers the owner's `now`.
//! Not ported: Wait in the Lab (QueueSession labFound, labSearchEnded, labLeave: the Lab's find chain is the Lab's
//! track), the found match's preloads (loadMatchAhead: the program loads the match at its start), and Save Chosen
//! Characters (saveChosen: the port's store of the last pick).

use mvs_net::service::api::PartySay;
use mvs_protocol::loadout::NetLoadout;
use mvs_protocol::messages::{ClientMessage, QueueWhy, ServerMessage};
use mvs_protocol::online::{QueueMode, QueueParty};
use mvs_protocol::queue_rules::{QueueFound, queue_team_style};
use mvs_protocol::setup::MatchSetupWire;
use mvs_ui::menu::online::{KickNote, NetPick, PreMatchEvent, Registered};
use mvs_ui::menu::screens::pre_match::setup::PreMatchPlayer;

use super::tables::{Tables, loadout_of_cosmetics, with_mode_rules};

/// Unverified: (the port's own) how long an unasked off 'party' waits for the other member's 'cancel' say, which
/// comes another way (the party's, not the queue's) and may come second: then it is the other's cancel, not a lost
/// search.
pub const PARTY_CANCEL_WAIT_MS: f64 = 1500.0;

/// Unverified: (the port's own) how long a join waits for the server's queue.state before the search ends as at a
/// refused join (the host core answers a join at once: no answer is a session that is gone without its close).
pub const JOIN_ANSWER_MS: f64 = 15000.0;

/// The found match's start: the set-up with the bots' tables and the mode's mutators, the
/// perks by slot, this game's slot, the names, the map, the mode, the world's seed; `local` a match with no other
/// human (played by this game alone: match.leave at its end).
#[derive(Clone, Debug, PartialEq)]
pub struct QueueMatch {
    pub setup: MatchSetupWire,
    pub perks: Vec<Vec<String>>,
    pub me: u32,
    pub usernames: Vec<String>,
    pub map: String,
    pub mode: QueueMode,
    pub seed: u32,
    pub local: bool,
}

/// What the queue asks of its owner, in order.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant, reason = "a few a step, each sent at once: a box would cost more than it saves")]
pub enum QOut {
    /// A message to the queue session's host core.
    Send(ClientMessage),
    /// A say to the other party member.
    Say(PartySay),
    /// News for the pre-match screen (while it is up).
    Screen(PreMatchEvent),
}

/// The game's party when the search began full: its role and the join's list.
#[derive(Clone, Debug, PartialEq)]
pub struct PartyQueue {
    pub leader: bool,
    pub list: QueueParty,
}

/// Why a search ended.
fn why_of(w: Option<QueueWhy>) -> QueueWhy {
    w.unwrap_or(QueueWhy::Error)
}

pub struct QueueSession {
    /// The queues it waits in (the multiqueue).
    pub modes: Vec<QueueMode>,
    /// MatchmakingStartTime (PM@66969): the owner's clock (ms) at the search's start.
    pub start_ms: f64,
    /// bIsMatchmaking: from StartMatchmaking's join to the search's end.
    pub searching: bool,
    /// The local pick's lock.
    pub mine: Option<NetPick>,
    pub found: Option<QueueFound>,
    /// match.start, once it came.
    pub started: Option<QueueMatch>,
    /// The screen is up.
    pub live: bool,
    /// The note of a called-off found match (its leaver's name when the server gave it).
    pub left: KickNote,
    /// (the port's) the search ended while no screen was up: the main menu's note.
    pub ended: Option<KickNote>,
    pub party: Option<PartyQueue>,
    /// ?queueBots: the wait for bots the game asks for.
    pub bot_after: Option<f64>,
    /// This game's loadout for a character, sent with the join.
    pub loadout: Option<NetLoadout>,
    /// This game's compatibility id, sent with each join.
    pub compat: String,
    cancelling: bool,
    cancels_out: u32,
    joined_since_cancel: bool,
    joined: bool,
    ready: bool,
    other_ready: bool,
    stopping: bool,
    lost: Option<f64>,
    /// (the port's own) the session is gone: a join cannot go out.
    gone: bool,
    /// (the port's own) the last join's deadline for its answer (JOIN_ANSWER_MS).
    join_due: Option<f64>,
    // the queue's own: the found match (until its start), the started one, the last search's end
    // without a match, and its listening (from a join to the start or the end)
    q_found: bool,
    q_match: bool,
    q_ended: Option<QueueWhy>,
    listening: bool,
    /// The Lab's find chain or Back To Matchmaking, carried over a Lab restart: the step that runs
    /// and its ticks left (0 once the level is being left); None before either.
    pub lab_leave: Option<(LabLeave, u32)>,
    /// The find chain's DoOnce TGM@2063 / @2078, closed for the Lab's level.
    pub lab_found_once: bool,
    /// The Lab ended in BPI_EnterPreMatch: the return path opens the PreMatch level (`OnlineHub::lab_return`).
    pub back_to_pre_match: bool,
}

/// The Lab's step on its way back to the queue's pre-match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabLeave {
    /// The find chain's Delay 3.0 TGM@2871.
    Found,
    /// Back To Matchmaking's Delay 1.0 TGM@3923.
    Back,
    /// BPI_EnterPreMatch's Delay 1.0 PGI@31709 under the fade, then OpenLevel(PreMatch).
    Fade,
}

impl QueueSession {
    pub fn new(modes: Vec<QueueMode>, party: Option<PartyQueue>, compat: String) -> QueueSession {
        QueueSession {
            modes,
            start_ms: 0.0,
            searching: false,
            mine: None,
            found: None,
            started: None,
            live: false,
            left: KickNote::Left,
            ended: None,
            party,
            bot_after: None,
            loadout: None,
            compat,
            cancelling: false,
            cancels_out: 0,
            joined_since_cancel: false,
            joined: false,
            ready: false,
            other_ready: false,
            stopping: false,
            lost: None,
            gone: false,
            join_due: None,
            q_found: false,
            q_match: false,
            q_ended: None,
            listening: false,
            lab_leave: None,
            lab_found_once: false,
            back_to_pre_match: false,
        }
    }

    /// The leader's queue opens the member's (party-flow).
    pub fn opened(&self, out: &mut Vec<QOut>) {
        if self.party.as_ref().is_some_and(|p| p.leader) {
            out.push(QOut::Say(PartySay::Open { modes: self.modes.clone() }));
        }
    }

    /// The team style the screen opens with.
    pub fn team_style(&self) -> u8 {
        self.modes.first().map_or(0, |m| queue_team_style(*m))
    }

    // ---------- the online queue ----------

    // A call while a found match waits for its start does nothing (that match stands: the server refuses it too).
    fn join(&mut self, now: f64, out: &mut Vec<QOut>) {
        if self.q_found && !self.q_match {
            return;
        }
        let Some(m) = self.mine.clone() else { return };
        self.listening = true;
        self.join_due = Some(now + JOIN_ANSWER_MS);
        self.q_found = false;
        self.q_match = false;
        self.q_ended = None;
        out.push(QOut::Send(ClientMessage::QueueJoin {
            modes: self.modes.clone(),
            character: m.character,
            skin: m.skin,
            compat: self.compat.clone(),
            bot_after: self.bot_after,
            loadout: self.loadout.clone(),
            // this game's share of the found match's random draws (the host core combines every human's)
            draw: Some(fresh_draw()),
            waited: None,
            party: self.party.as_ref().map(|p| p.list.clone()),
            // (the net service puts in the relay's round trip: the game does not know it)
            rtt: None,
        }));
    }

    fn q_end(&mut self, why: QueueWhy) {
        if !self.q_match {
            self.q_ended = Some(why);
        }
        self.q_found = false;
        self.listening = false;
        self.join_due = None;
    }

    /// Out of the queue, or out of the found match's pre-match: the server answers nothing.
    fn q_leave(&mut self, out: &mut Vec<QOut>) {
        out.push(QOut::Send(ClientMessage::PmLeave));
        self.q_end(QueueWhy::Left);
    }

    /// A message of the queue session.
    pub fn take(&mut self, m: &ServerMessage, tables: &Tables, me: (&str, &str), now: f64, out: &mut Vec<QOut>) {
        if !self.listening {
            return;
        }
        if matches!(m, ServerMessage::QueueState { .. } | ServerMessage::QueueFound(_)) {
            self.join_due = None;
        }
        match m {
            ServerMessage::QueueState { on, why, name } => {
                // (a refused join while a found match waits for its start: the found match stands)
                if !on && *why == Some(QueueWhy::Error) && self.q_found && !self.q_match {
                    return;
                }
                if !on {
                    self.q_end(why_of(*why));
                }
                self.on_state(*on, *why, name.as_deref(), now, out);
            }
            ServerMessage::QueueFound(f) => {
                // (the map of the server's pick and each bot's draw, from this game's tables)
                let found = tables.resolve_found(f);
                self.q_found = true;
                self.on_found(found, me, out);
            }
            ServerMessage::MatchStart(s) => {
                let Some(f) = self.found.clone().filter(|f| s.match_id.as_deref() == Some(f.match_id.as_str())) else {
                    return;
                };
                // (the bots' ring-out, banner and perks, and the mode's mutators, which the server leaves to the games)
                let (setup, perks) = tables.with_bot_tables(&s.setup, &s.perks.clone().unwrap_or_default(), &f.players);
                let m = QueueMatch {
                    setup: with_mode_rules(&setup, f.mode),
                    perks,
                    me: s.slot,
                    usernames: s.usernames.clone(),
                    map: f.map.clone(),
                    mode: f.mode,
                    seed: s.seed,
                    local: s.local == Some(true),
                };
                self.q_match = true;
                self.listening = false;
                // match.start: every human's perks are in (or their time is over): the screen's OnAllPlayersLockedPerks
                if self.live {
                    out.push(QOut::Screen(PreMatchEvent::AllLockedPerks { perks: m.perks.clone() }));
                }
                self.started = Some(m);
            }
            _ => {}
        }
    }

    /// The session is gone: a search or a found match before its start ends as at a refused join; a started match's
    /// loss is its set's.
    /// (the port's own) A search that ended before the screen came up is the screen's kick at once, and a later join
    /// with no session to go on is one too.
    pub fn connection_lost(&mut self, now: f64, out: &mut Vec<QOut>) {
        self.gone = true;
        self.join_due = None;
        if self.q_match || self.q_ended.is_some() {
            return;
        }
        self.q_end(QueueWhy::Error);
        self.on_state(false, Some(QueueWhy::Error), None, now, out);
    }

    /// (the port's own) The screen is up: a search that ended while it was not is its kick to the main menu (the
    /// screen would wait in it for an answer that cannot come).
    pub fn screen_up(&mut self, out: &mut Vec<QOut>) {
        self.live = true;
        if let Some(note) = self.ended.take() {
            out.push(QOut::Screen(PreMatchEvent::Kick(note)));
        }
    }

    // (the port's own) the search over with the note: the screen's kick, else the main menu's note.
    fn kick(&mut self, note: KickNote, out: &mut Vec<QOut>) {
        self.searching = false;
        if self.live {
            out.push(QOut::Screen(PreMatchEvent::Kick(note)));
        } else {
            self.ended = Some(note);
        }
    }

    // ---------- the queue run's state and found handlers ----------

    fn on_state(&mut self, on: bool, why: Option<QueueWhy>, name: Option<&str>, now: f64, out: &mut Vec<QOut>) {
        // an older cancel's answer, after a newer join: let go (the newer join's search goes on)
        if !on && why == Some(QueueWhy::Cancel) && self.cancels_out > 0 {
            self.cancels_out -= 1;
            let stale = self.joined_since_cancel;
            if self.cancels_out == 0 {
                self.joined_since_cancel = false;
            }
            if stale {
                return;
            }
        }
        if !on {
            self.cancelling = false;
        } else {
            self.joined = true;
        }
        // (the port's own) an off 'party' before the join was taken is the server's refusal of the party's join
        let refused = !on && why == Some(QueueWhy::Party) && !self.joined;
        // the other member's 'cancel' stopped this search: whatever off answers it is the cancel the build's owner
        // and member get unasked (OnMatchmakingCancel PM@81099: back to state 4, the lock kept)
        if !on && self.stopping {
            self.stopping = false;
            self.searching = false;
            self.party_stopped(out);
            // (the member's 'ready' that came while this search stopped)
            self.party_go(now, out);
            return;
        }
        // (the port's own) A party's ticket ended under this game, which did not ask: the other member's join went
        // out (a 'cancel' said on its way: up to PARTY_CANCEL_WAIT_MS for it) or did not come in time.
        if !on
            && self.party.is_some()
            && self.found.is_none()
            && !refused
            && matches!(why, Some(QueueWhy::Party | QueueWhy::PartyLate))
        {
            if !self.searching {
                return;
            }
            self.searching = false;
            self.lost = Some(now + if why == Some(QueueWhy::Party) { PARTY_CANCEL_WAIT_MS } else { 0.0 });
            return;
        }
        // OnMatchAbandonedByPlayer MH@31771: "{username} has disconnected" (MH@34468, the leaver's username)
        if why == Some(QueueWhy::Left)
            && let Some(n) = name.filter(|n| !n.is_empty())
        {
            self.left = KickNote::LeftNamed(n.to_owned());
        }
        if !on {
            self.searching = false;
        }
        let ended = match why {
            Some(QueueWhy::Left) => self.left.clone(),
            Some(QueueWhy::Compat) => KickNote::CompatMissing,
            _ if refused => KickNote::PartyRefused,
            _ => KickNote::Error,
        };
        // (the port's) an end while no screen is up is kept for the return (a cancel cannot come from there)
        if !self.live {
            if !on && why != Some(QueueWhy::Cancel) {
                self.ended = Some(ended);
            }
            return;
        }
        out.push(QOut::Screen(if on {
            PreMatchEvent::MatchmakingStarted
        } else if why == Some(QueueWhy::Cancel) {
            PreMatchEvent::MatchmakingCancel
        } else {
            PreMatchEvent::Kick(ended)
        }));
    }

    fn on_found(&mut self, f: QueueFound, me: (&str, &str), out: &mut Vec<QOut>) {
        if self.found.is_some() {
            return;
        }
        self.found = Some(f);
        if !self.live {
            return;
        }
        if let Some(r) = self.registered(me) {
            out.push(QOut::Screen(PreMatchEvent::AllRegistered(Box::new(r))));
        }
    }

    /// The found players as the screen takes them (Handle All Players Registered); `me` this player's (id, name).
    pub fn registered(&self, me: (&str, &str)) -> Option<Registered> {
        let f = self.found.as_ref()?;
        let my = f.me as usize;
        let players = f
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| PreMatchPlayer {
                account_id: if i == my { me.0.to_owned() } else { p.id.clone() },
                username: if i == my && !me.1.is_empty() { me.1.to_owned() } else { p.name.clone() },
                team: u8::try_from(p.team).unwrap_or(0),
                is_bot: p.bot,
                character: (!p.character.is_empty()).then(|| p.character.clone()),
                skin: p.skin.clone(),
                // (the server's queue.found: a player's banner, badges and icon)
                cosmetics: p.cosmetics.as_ref().map(|c| loadout_of_cosmetics(c, p.ring_out.as_deref().unwrap_or(""))),
                net_cosmetics: p.cosmetics.clone(),
            })
            .collect();
        Some(Registered {
            players,
            me: my,
            map: f.map.clone(),
            team_style: queue_team_style(f.mode),
            perks: self.started.as_ref().map(|m| m.perks.clone()),
        })
    }

    // ---------- the screen's hooks ----------

    /// Sends the local pick's join.
    fn rejoin(&mut self, now: f64, out: &mut Vec<QOut>) {
        if self.mine.is_none() {
            return;
        }
        // (the port's own) no session for the join: the search ends as at a lost connection
        if self.gone {
            return self.kick(KickNote::Error, out);
        }
        self.searching = true;
        self.joined = false;
        if self.cancels_out > 0 {
            self.joined_since_cancel = true;
        }
        self.join(now, out);
    }

    fn send_cancel(&mut self, out: &mut Vec<QOut>) {
        self.cancelling = true;
        self.cancels_out += 1;
        self.joined_since_cancel = false;
        out.push(QOut::Send(ClientMessage::QueueCancel));
    }

    // this game out of the search: a party's other page told ('cancel', once per lock), the join cancelled
    fn withdraw(&mut self, out: &mut Vec<QOut>) {
        if self.party.is_some() && self.ready {
            self.ready = false;
            out.push(QOut::Say(PartySay::Cancel));
        }
        if self.searching && !self.cancelling && self.found.is_none() {
            self.send_cancel(out);
        }
    }

    /// HandleCharacterConfirmed and the timer's lock (localLocked).
    pub fn local_locked(&mut self, p: NetPick) {
        match &mut self.mine {
            Some(m) if m.character == p.character => m.skin = p.skin,
            _ => self.mine = Some(p),
        }
    }

    /// The pick taken back (localUnlocked): a party's other game is told and this game's join goes out.
    pub fn local_unlocked(&mut self, out: &mut Vec<QOut>) {
        self.mine = None;
        if self.party.is_some() {
            self.withdraw(out);
        }
    }

    /// OnPlayersReadyToMatchmake's StartMatchmaking (PM@68200); a party's page runs it at its own lock and stays in
    /// state 4 until the party's search starts (party_go).
    pub fn start_matchmaking(&mut self, now: f64, out: &mut Vec<QOut>) {
        if self.mine.is_none() {
            return;
        }
        // MatchmakingStartTime (PM@66969) and Toggle Matchmaking's bIsMatchmaking
        self.start_ms = now;
        let Some(party) = &self.party else { return self.rejoin(now, out) };
        self.ready = true;
        // (the port's own) the member's lock told to the leader, whose search waits for it
        if !party.leader {
            out.push(QOut::Say(PartySay::Ready));
            return;
        }
        self.party_go(now, out);
    }

    /// The owner's Toggle Matchmaking(false); a member's AskLeaderToCancelMatchmaking (PM@80191).
    pub fn cancel_matchmaking(&mut self, out: &mut Vec<QOut>) {
        self.withdraw(out);
    }

    /// The four slots with their empties.
    pub fn perks_locked(perks: &[String], out: &mut Vec<QOut>) {
        out.push(QOut::Send(ClientMessage::PmPerks { perks: perks.iter().take(4).cloned().collect() }));
    }

    // ---------- the party ----------

    /// The leader's search, once both picks are locked (its own and the member's 'ready'): state 11, its join, and
    /// the 'queue' the member's join waits for (the two within the server's PARTY_WAIT_MS).
    fn party_go(&mut self, now: f64, out: &mut Vec<QOut>) {
        if !self.party.as_ref().is_some_and(|p| p.leader)
            || !self.ready
            || !self.other_ready
            || self.searching
            || self.cancelling
            || self.found.is_some()
            || self.mine.is_none()
        {
            return;
        }
        if self.live {
            out.push(QOut::Screen(PreMatchEvent::PartyStarted));
        }
        self.rejoin(now, out);
        out.push(QOut::Say(PartySay::Queue { modes: self.modes.clone() }));
    }

    /// The search stopped by the other member: the screen back to state 4 with its lock.
    fn party_stopped(&mut self, out: &mut Vec<QOut>) {
        if self.live {
            out.push(QOut::Screen(PreMatchEvent::PartyStopped));
        }
    }

    /// The party's search ends under this game (the other page left it, the party ended): the main menu with the
    /// note, as the queue's errors go (BPI_KickBackToMainMenu).
    pub fn party_ended(&mut self, note: KickNote, out: &mut Vec<QOut>) {
        if self.party.is_none() || self.found.is_some() {
            return;
        }
        if self.live {
            out.push(QOut::Screen(PreMatchEvent::Kick(note)));
            return;
        }
        self.ended = Some(note);
        self.searching = false;
    }

    /// (the port's own) The other member's say to the party's search (partyQueueSay): true when the search took it.
    /// `other` is the other member's name.
    pub fn party_say(&mut self, say: &PartySay, other: &str, now: f64, out: &mut Vec<QOut>) -> bool {
        let Some(leader) = self.party.as_ref().map(|p| p.leader) else { return false };
        if self.found.is_some() {
            return false;
        }
        match say {
            PartySay::Ready if leader => {
                self.other_ready = true;
                self.party_go(now, out);
                true
            }
            PartySay::Open { modes } if !leader => {
                if *modes != self.modes {
                    return false;
                }
                if self.ready {
                    out.push(QOut::Say(PartySay::Ready));
                }
                true
            }
            PartySay::Queue { modes } if !leader => {
                if *modes != self.modes {
                    return false;
                }
                if self.mine.is_some() && self.ready && !self.searching && !self.cancelling {
                    self.rejoin(now, out);
                }
                true
            }
            PartySay::Cancel => {
                if leader {
                    self.other_ready = false;
                }
                // (an unasked off 'party' already come: this was its cause)
                if self.lost.take().is_some() {
                    self.party_stopped(out);
                } else if self.searching {
                    self.stopping = true;
                    if !self.cancelling {
                        self.send_cancel(out);
                    }
                } else {
                    self.party_stopped(out);
                }
                true
            }
            PartySay::Home => {
                self.party_ended(KickNote::PartyLeft(other.to_owned()), out);
                true
            }
            _ => false,
        }
    }

    /// The timers: an unasked off 'party' whose 'cancel' did not come ends the search; (the port's own) a join with
    /// no answer in JOIN_ANSWER_MS ends it as at a refused join.
    pub fn pump(&mut self, now: f64, out: &mut Vec<QOut>) {
        if self.lost.is_some_and(|t| now >= t) {
            self.lost = None;
            self.party_ended(KickNote::PartySearchEnded, out);
        }
        if self.join_due.is_some_and(|t| now >= t) {
            eprintln!("mvs: online: the queue's join had no answer");
            self.join_due = None;
            self.q_end(QueueWhy::Error);
            self.on_state(false, Some(QueueWhy::Error), None, now, out);
        }
    }

    pub fn next_due(&self) -> Option<f64> {
        [self.lost, self.join_due].into_iter().flatten().reduce(f64::min)
    }

    /// Out of the queue (endQueue): the party told this game is back in the menus, pm.leave.
    pub fn end(&mut self, out: &mut Vec<QOut>) {
        if self.party.is_some() && self.started.is_none() {
            out.push(QOut::Say(PartySay::Home));
        }
        self.q_leave(out);
    }
}

/// A random u32 from the process's hasher keys and a counter (mvs-app has no random crate; the draw needs no more).
fn fresh_draw() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, Ordering::Relaxed));
    // (the low half of the hash)
    #[allow(clippy::cast_possible_truncation)]
    let d = h.finish() as u32;
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_ui::menu::online::KickNote;
    use serde_json::json;

    fn msg(v: serde_json::Value) -> ServerMessage {
        serde_json::from_value(v).unwrap()
    }

    fn sends(out: &[QOut]) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                QOut::Send(m) => Some(m.text()),
                _ => None,
            })
            .collect()
    }

    fn pick() -> NetPick {
        NetPick { character: "shaggy".into(), skin: String::new() }
    }

    #[test]
    fn a_solo_search_finds_and_starts() {
        let t = Tables::default();
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.live = true;
        let mut out = Vec::new();
        q.start_matchmaking(0.0, &mut out);
        assert!(out.is_empty(), "no join before a lock");
        q.local_locked(pick());
        q.start_matchmaking(5.0, &mut out);
        let s = sends(&out);
        assert!(s[0].contains(r#""t":"queue.join""#) && s[0].contains(r#""compat":"c1""#) && q.searching);
        out.clear();
        q.take(&msg(json!({ "t": "queue.state", "on": true })), &t, ("me", "Me"), 6.0, &mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::MatchmakingStarted)]);
        out.clear();
        let found = json!({ "t": "queue.found", "match": "m1", "mode": "1v1", "mapPick": 0, "me": 0, "players": [
            { "id": "me", "name": "x", "team": 0, "character": "", "skin": "", "bot": false },
            { "id": "p2", "name": "Two", "team": 1, "character": "", "skin": "", "bot": false } ] });
        q.take(&msg(found), &t, ("me", "Me"), 7.0, &mut out);
        let Some(QOut::Screen(PreMatchEvent::AllRegistered(r))) = out.first() else { panic!("{out:?}") };
        assert!(r.players[0].username == "Me" && r.players[1].account_id == "p2");
        out.clear();
        // (another match's start is not this one's)
        let start = |id: &str| {
            msg(json!({ "t": "match.start", "match": id, "seed": 9, "setup": { "teamStyle": 0, "players": [] },
                "slot": 0, "usernames": ["Me", "Two"], "map": "", "perks": [["a"], ["b"]] }))
        };
        q.take(&start("m0"), &t, ("me", "Me"), 8.0, &mut out);
        assert!(q.started.is_none());
        q.take(&start("m1"), &t, ("me", "Me"), 8.0, &mut out);
        assert!(q.started.as_ref().is_some_and(|m| m.seed == 9 && m.mode == QueueMode::OneVOne));
        assert!(matches!(&out[0], QOut::Screen(PreMatchEvent::AllLockedPerks { .. })));
    }

    #[test]
    fn stale_cancels_and_lost_searches() {
        let t = Tables::default();
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.live = true;
        let mut out = Vec::new();
        q.local_locked(pick());
        q.start_matchmaking(0.0, &mut out);
        q.cancel_matchmaking(&mut out);
        assert!(sends(&out).last().is_some_and(|m| m.contains("queue.cancel")));
        // a join again before the cancel's answer: that answer is let go
        q.start_matchmaking(1.0, &mut out);
        out.clear();
        q.take(&msg(json!({ "t": "queue.state", "on": false, "why": "cancel" })), &t, ("me", ""), 2.0, &mut out);
        assert!(out.is_empty() && q.searching);
        // the session lost under a new search: it ends as at a refused join
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.live = true;
        q.local_locked(pick());
        q.start_matchmaking(0.0, &mut out);
        out.clear();
        q.connection_lost(3.0, &mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::Kick(KickNote::Error))]);
    }

    #[test]
    fn a_search_with_no_session_or_no_answer_kicks_the_screen() {
        let t = Tables::default();
        let mut out = Vec::new();
        // the session lost before the screen came up: the screen is kicked as it comes up
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.connection_lost(1.0, &mut out);
        assert!(out.is_empty() && q.ended == Some(KickNote::Error));
        q.screen_up(&mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::Kick(KickNote::Error))]);
        assert!(q.ended.is_none());
        // a lock after the loss: no join goes out, the screen is kicked
        out.clear();
        q.local_locked(pick());
        q.start_matchmaking(2.0, &mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::Kick(KickNote::Error))]);
        assert!(!q.searching);
        // a join the server never answers ends after JOIN_ANSWER_MS
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.screen_up(&mut out);
        q.local_locked(pick());
        out.clear();
        q.start_matchmaking(10.0, &mut out);
        assert_eq!(q.next_due(), Some(10.0 + JOIN_ANSWER_MS));
        out.clear();
        q.pump(10.0 + JOIN_ANSWER_MS - 1.0, &mut out);
        assert!(out.is_empty());
        q.pump(10.0 + JOIN_ANSWER_MS, &mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::Kick(KickNote::Error))]);
        assert!(!q.searching && q.next_due().is_none());
        // an answered join keeps searching past the wait
        let mut q = QueueSession::new(vec![QueueMode::OneVOne], None, "c1".into());
        q.screen_up(&mut out);
        q.local_locked(pick());
        q.start_matchmaking(0.0, &mut out);
        q.take(&msg(json!({ "t": "queue.state", "on": true })), &t, ("me", ""), 1.0, &mut out);
        out.clear();
        q.pump(JOIN_ANSWER_MS * 2.0, &mut out);
        assert!(out.is_empty() && q.searching && q.next_due().is_none());
    }

    #[test]
    fn a_party_leader_waits_for_the_members_ready() {
        let t = Tables::default();
        let party = PartyQueue { leader: true, list: QueueParty { members: vec!["me".into(), "p2".into()] } };
        let mut q = QueueSession::new(vec![QueueMode::TwoVTwo], Some(party), "c1".into());
        q.live = true;
        let mut out = Vec::new();
        q.opened(&mut out);
        assert_eq!(out, [QOut::Say(PartySay::Open { modes: vec![QueueMode::TwoVTwo] })]);
        out.clear();
        q.local_locked(pick());
        q.start_matchmaking(0.0, &mut out);
        assert!(out.is_empty() && !q.searching);
        assert!(q.party_say(&PartySay::Ready, "Two", 0.0, &mut out));
        assert!(matches!(out[0], QOut::Screen(PreMatchEvent::PartyStarted)));
        assert!(sends(&out)[0].contains(r#""party":{"members":["me","p2"]}"#));
        assert_eq!(out.last(), Some(&QOut::Say(PartySay::Queue { modes: vec![QueueMode::TwoVTwo] })));
        out.clear();
        // an unasked off 'party': the other's cancel is waited for, else the search ends
        q.take(&msg(json!({ "t": "queue.state", "on": true })), &t, ("me", ""), 1.0, &mut out);
        out.clear();
        q.take(&msg(json!({ "t": "queue.state", "on": false, "why": "party" })), &t, ("me", ""), 2.0, &mut out);
        assert!(out.is_empty() && q.next_due() == Some(2.0 + PARTY_CANCEL_WAIT_MS));
        q.pump(2.0 + PARTY_CANCEL_WAIT_MS, &mut out);
        assert_eq!(out, [QOut::Screen(PreMatchEvent::Kick(KickNote::PartySearchEnded))]);
    }
}
