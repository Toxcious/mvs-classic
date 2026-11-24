//! An online match of the menus: this
//! peer's rollback session around the World, its packets to the match's referee from the host core's go on, and what
//! ends the match from the connection.

use std::cell::RefCell;
use std::rc::Rc;

use mvs_net::service::api::ClosedWhy;
use mvs_net::session::{Session, SessionConfig, SlotKind};
use mvs_protocol::match_wire::{RESUME_MS, RESUME_TRY_MS, WireInput};
use mvs_protocol::messages::{ClientMessage, LeftWhy, ServerMessage};
use mvs_protocol::profile::MatchReportPlayer;
use mvs_protocol::setup::MatchSetupWire;
use mvs_sim::ai::bt::BtBot;
use mvs_sim::{InputState, World};
use mvs_sim_trace::Book;
use mvs_trace::Trace;
use mvs_ui::menu::online::OnlineNotice;

use super::desync_report::{self, DesyncReport};
use super::dev::OnlineTick;
use super::hub::{NetMatchStart, OnlineHub};
use super::pre_match::PreMatchStarted;
use crate::{Error, Result};

/// In a match the ping to the host is measured every second (the next start gate's wait), between matches every 2 s.
const PING_MATCH_MS: f64 = 1000.0;
const PING_MENUS_MS: f64 = 2000.0;

/// A trace's inputs fed as the local player's (`--feed`).
struct Feed {
    trace: Trace,
    book: Book,
    next: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Gate {
    /// match.loaded not sent yet.
    Load,
    /// Waiting for match.go.
    Go,
    /// The go came: tick 0 at this time (ms).
    At(f64),
    Play,
}

/// An online match's tick source over the menus' session.
pub struct NetMatch {
    hub: Rc<RefCell<OnlineHub>>,
    me: usize,
    usernames: Vec<String>,
    slots: Vec<SlotKind>,
    map: String,
    /// A queue match: its result goes through its set's end session.
    queue: bool,
    gate: Gate,
    linked: bool,
    session: Option<Session>,
    feed_name: Option<String>,
    feed: Option<Feed>,
    /// The peers that left and are not decided yet: (name, '' the match's time limit; slot, -1 the host's end).
    left: Vec<(String, i32)>,
    carry: Vec<mvs_sim::Event>,
    note: Option<String>,
    stopped: bool,
    ended: bool,
    /// The match's result, read at its end: the winning team, each slot's numbers.
    report: Option<(i32, Vec<MatchReportPlayer>)>,
    /// Each slot's player id from match.go ('' a bot), for the match chat's senders.
    peers: Vec<String>,
    /// The match's chats not taken yet (sender id, to whom, text).
    chats: Vec<(String, mvs_protocol::messages::ChatTo, String)>,
    /// (the port's own) The slots the host core says are away (match.away), in the order told.
    away: Vec<usize>,
    /// (the port's own) The match's path as the host core chose it before the go (OnlineHub::match_path): the referee's
    /// slot (-1 the relay) and each slot's round trip to it, for the ping line of a match this game referees.
    path: Option<(i32, Vec<Option<u32>>)>,
    /// (the port's own) The session's host core, and so the match's referee, runs in this game (OnlineHub::own_core):
    /// the host of a lobby over Epic or a local network.
    own_core: bool,
    /// What the desync report says of the match.
    base: ReportBase,
    /// By slot, by tick from 0: the inputs the confirmed line ran (Session::on_ruled), for the desync report.
    ruled: Recorded,
}

impl std::fmt::Debug for NetMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetMatch").field("me", &self.me).field("gate", &self.gate).finish_non_exhaustive()
    }
}

/// KismetMathLibrary Round (FloorToInt(A + 0.5)), clamped to the report's range.
fn round(v: f64) -> u32 {
    // (a match's numbers: whole, small, never negative)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let r = (v + 0.5).floor().max(0.0) as u32;
    r
}

/// (the port's own) The match's ping line's number (NetMatch::ping): `path` the pm.path kept for the match, `me` this
/// game's slot, `own_core` the session's core is this game's, `live` the session's round trip to the referee.
fn match_ping(
    path: Option<&(i32, Vec<Option<u32>>)>,
    me: usize,
    own_core: bool,
    live: impl FnOnce() -> Option<f64>,
) -> Option<f64> {
    let slot = i32::try_from(me).ok();
    if let Some((referee, ms)) = path.filter(|p| Some(p.0) == slot) {
        let others = ms.iter().enumerate().filter(|(i, _)| i32::try_from(*i).ok() != Some(*referee));
        return others.filter_map(|(_, m)| *m).max().map(f64::from);
    }
    if own_core {
        return None;
    }
    let own = || path.and_then(|p| p.1.get(me).copied().flatten()).map(f64::from);
    live().or_else(own)
}

/// By slot, by tick from 0: the confirmed line's inputs as the wire encodes them.
pub type Recorded = Rc<RefCell<Vec<Vec<WireInput>>>>;

/// The session's confirmed line recorded for the desync report: by slot (of `slots`), each tick's input as the wire
/// encodes it, from tick 0 (Session::on_ruled).
pub fn record_ruled(session: &mut Session, slots: usize) -> Recorded {
    let ruled = Rc::new(RefCell::new(vec![Vec::new(); slots]));
    let r = ruled.clone();
    session.on_ruled(Box::new(move |_, inputs| {
        for (l, i) in r.borrow_mut().iter_mut().zip(inputs) {
            l.push(mvs_net::codec::encode(i));
        }
    }));
    ruled
}

/// What a desync report says of its match besides the desync and the line.
#[derive(Clone, Debug, Default)]
pub struct ReportBase {
    /// The set-up as the World ran it, its seed, the map match.start named and the stage the World loaded.
    pub setup: MatchSetupWire,
    pub seed: u32,
    pub map: String,
    pub stage: String,
    /// This player's slot.
    pub me: usize,
}

impl ReportBase {
    /// The match of match.start: its set-up with each slot's sent perks (glue's start, as
    /// withSentPerks), its seed, map and this player's slot; the stage is the World's (`ran`).
    pub fn of(s: &PreMatchStarted) -> ReportBase {
        let mut setup = s.setup.clone();
        for (i, p) in setup.players.iter_mut().enumerate() {
            p.perks = Some(mvs_sim::setup::perk_list(s.perks.get(i).map_or(&[][..], Vec::as_slice)));
        }
        ReportBase { setup, seed: s.seed, map: s.map.clone(), stage: String::new(), me: s.slot as usize }
    }

    /// The World the match runs: its stage, and each fighter the port does not draw (glue's ported_setup, the
    /// default character in its place) without its character, as the World has it.
    pub fn ran(&mut self, w: &World) {
        self.stage.clone_from(&w.data.stage_key);
        for (p, q) in self.setup.players.iter_mut().zip(&w.setup.players) {
            if q.character.is_none() {
                p.character = None;
            }
        }
    }
}

/// The desync report of a match from what it was and its recorded line.
pub fn desync_report_of(
    compat: String,
    base: &ReportBase,
    d: &mvs_net::session::Desync,
    ruled: &[Vec<WireInput>],
) -> DesyncReport {
    DesyncReport {
        version: mvs_release::VERSION.to_owned(),
        compat,
        setup: base.setup.clone(),
        seed: base.seed,
        map: base.map.clone(),
        stage: base.stage.clone(),
        me: base.me,
        tick: d.tick,
        slot: d.slot,
        mine: d.mine,
        theirs: d.theirs,
        inputs: ruled.to_vec(),
    }
}

/// Each slot's numbers as HandleWinLoss reads them for the match record.
pub fn match_report(w: &World) -> (i32, Vec<MatchReportPlayer>) {
    report_of(&mvs_sim::world::flow::podium_data(w))
}

/// match_report over the match's podium data (the end of game's).
pub fn report_of(podium: &[mvs_sim::world::flow::PodiumEntry]) -> (i32, Vec<MatchReportPlayer>) {
    let n = |v: i32| u32::try_from(v).unwrap_or(0);
    let players = podium
        .iter()
        .map(|p| MatchReportPlayer {
            slot: n(p.player_index),
            damage: round(f64::from(p.damage_dealt)),
            ringouts: n(p.ring_outs),
            deaths: n(p.falls),
            score: round(f64::from(p.score)),
            assists: n(p.assists),
            assisted: n(p.assisted),
            doubles: n(p.doubles),
            dodges: n(p.dodges),
        })
        .collect();
    let team = podium.iter().find(|p| p.is_winner).map_or(-1, |p| p.team_index);
    (team, players)
}

impl NetMatch {
    /// The match of `start` on the hub's session; `bots` each slot's bot flag.
    pub fn new(hub: Rc<RefCell<OnlineHub>>, start: &NetMatchStart, bots: &[bool], feed: Option<String>) -> NetMatch {
        let me = start.start.slot as usize;
        let slots = bots
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if *b {
                    SlotKind::Bot
                } else if i == me {
                    SlotKind::Local
                } else {
                    SlotKind::Remote
                }
            })
            .collect();
        let (path, own_core) = {
            let h = hub.borrow();
            (h.match_path().cloned(), h.own_core())
        };
        {
            let mut h = hub.borrow_mut();
            h.match_inbox(true);
            let now = h.clock();
            let OnlineHub { client, .. } = &mut *h;
            if let Some(c) = client {
                c.ping_every(PING_MATCH_MS, now);
            }
        }
        NetMatch {
            hub,
            me,
            usernames: start.start.usernames.clone(),
            slots,
            map: start.start.map.clone(),
            queue: start.queue.is_some(),
            gate: Gate::Load,
            linked: false,
            session: None,
            feed_name: feed,
            feed: None,
            left: Vec::new(),
            carry: Vec::new(),
            note: None,
            stopped: false,
            ended: false,
            report: None,
            peers: Vec::new(),
            chats: Vec::new(),
            away: Vec::new(),
            path,
            own_core,
            base: ReportBase::of(&start.start),
            ruled: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// The stage the match loaded (unused: the desync report takes the World's at the go, ReportBase::ran).
    pub fn set_stage_key(&mut self, _key: &str) {}

    /// The match reached its own end (its results).
    pub fn ended(&mut self) {
        self.ended = true;
    }

    /// Why the match stopped early, once.
    pub fn note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// The match stops early: `why` to the log, and (the port's own) a notice the menus show when they are back.
    fn stop(&mut self, why: String) {
        if !self.stopped && !self.ended {
            let notice = format!("The match ended: {why}.");
            self.hub.borrow_mut().notice(OnlineNotice::Host(notice));
            self.stop_quiet(why);
        }
    }

    /// The match stops early with no notice of its own (a desync: its report's notice says it).
    fn stop_quiet(&mut self, why: String) {
        if !self.stopped && !self.ended {
            self.stopped = true;
            self.note = Some(why);
        }
    }

    fn name(&self, slot: i32) -> String {
        usize::try_from(slot).ok().and_then(|s| self.usernames.get(s)).cloned().unwrap_or_else(|| "A player".to_owned())
    }

    fn send(&self, m: &ClientMessage) {
        let mut h = self.hub.borrow_mut();
        let OnlineHub { client, port, .. } = &mut *h;
        if let Some(c) = client
            && c.open
        {
            c.send(port, m);
        }
    }

    // The session's news for the match: its packets, match.go, match.left, match.desync, its close.
    fn inbox(&mut self, now: f64) -> Option<(f64, Vec<String>, String)> {
        let inbox = {
            let mut h = self.hub.borrow_mut();
            h.poll(now);
            h.take_inbox()
        };
        if let Some(s) = &mut self.session {
            for p in &inbox.packets {
                s.receive(p, now);
            }
        }
        self.chats.extend(inbox.chats);
        let mut go = None;
        for m in inbox.messages {
            match m {
                ServerMessage::MatchGo { in_ms, peers, referee } => go = Some((f64::from(in_ms), peers, referee)),
                // A peer leaves: the host core tells who and why, and the referee ends that slot's stream at its
                // ruled line. The match runs on until the confirmed line has reached the end of the leaver's stream.
                // Deviation: PandaGameMode ProcessPlayerConnectionEnded puts an easy bot in a queue match leaver's
                // place; the port ends every online match for everyone at a leaver.
                ServerMessage::MatchLeft { slot, why, .. } => {
                    if why == LeftWhy::Rematch {
                        continue;
                    }
                    self.away.retain(|&s| i32::try_from(s).ok() != Some(slot));
                    let me = i32::try_from(self.me).unwrap_or(-1);
                    if slot == me {
                        self.stop("the connection to the other players was lost".to_owned());
                    } else if slot >= 0 && !self.linked {
                        let n = self.name(slot);
                        self.stop(format!("{n} left the match"));
                    } else {
                        let who = if why == LeftWhy::TooLong { String::new() } else { self.name(slot) };
                        self.left.push((who, slot));
                    }
                }
                ServerMessage::MatchDesync { .. } => self.stop("the match went out of sync".to_owned()),
                // Deviation: the port's own rule (the build has no way back into a match): a player whose link dropped
                // is away, their fighter standing, until they are back or gone.
                ServerMessage::MatchAway { slot, away } => {
                    let s = usize::try_from(slot).unwrap_or(usize::MAX);
                    self.away.retain(|&x| x != s);
                    if away && s != self.me {
                        self.away.push(s);
                    }
                }
                _ => {}
            }
        }
        if let Some(why) = inbox.closed {
            self.stop(if why == ClosedWhy::HostLeft {
                "the host left the match".to_owned()
            } else {
                "the connection to the other players was lost".to_owned()
            });
        }
        self.after_tick();
        go
    }

    // a leaver decided once the confirmed line is where its stream ends.
    fn after_tick(&mut self) {
        let Some(s) = &self.session else { return };
        let mut stop = None;
        self.left.retain(|(who, slot)| {
            let end = match usize::try_from(*slot) {
                Err(_) => Some(0),
                Ok(i) => s.end_of(i),
            };
            let Some(end) = end else { return true };
            if s.confirmed() < end {
                return true;
            }
            if s.game_end().is_some_and(|g| g < end) {
                return false;
            }
            stop.get_or_insert_with(|| {
                if who.is_empty() { "the match ran too long".to_owned() } else { format!("{who} left the match") }
            });
            false
        });
        if let Some(why) = stop {
            self.stop(why);
        }
    }

    /// The online start gate: match.loaded once, then the go: its link, and tick 0 `in` ms after the
    /// host sent it, less this peer's one-way time (half its smallest ping; none measured, or one over twice `in`: at
    /// once). True once the match runs: its session made on `world`.
    pub fn gate(&mut self, world: &World) -> Result<bool> {
        let now = self.hub.borrow().clock();
        if self.gate == Gate::Load {
            self.send(&ClientMessage::MatchLoaded);
            self.gate = Gate::Go;
        }
        let go = self.inbox(now);
        if self.stopped {
            return Ok(false);
        }
        if let (Gate::Go, Some((in_ms, peers, referee))) = (self.gate, go) {
            let ping = {
                let mut h = self.hub.borrow_mut();
                let OnlineHub { client, port, .. } = &mut *h;
                let Some(c) = client else { return Ok(false) };
                self.peers.clone_from(&peers);
                c.start_match(port, peers, u32::try_from(self.me).unwrap_or(0), referee);
                c.ping_min()
            };
            self.linked = true;
            let ahead = if in_ms > 0.0 && ping >= 0.0 && ping <= 2.0 * in_ms { in_ms - ping / 2.0 } else { 0.0 };
            self.gate = Gate::At(now + ahead);
        }
        match self.gate {
            Gate::At(at) if now >= at => {
                self.play(world, now)?;
                Ok(true)
            }
            Gate::Play => Ok(true),
            _ => Ok(false),
        }
    }

    fn play(&mut self, world: &World, now: f64) -> Result<()> {
        let mut session = Session::new(world, SessionConfig::new(self.slots.clone()), now)
            .map_err(|e| Error::Match(format!("online: the session: {e}")))?;
        if let Some(t) = self.hub.borrow().desync_at {
            session.spoil_hash_at(t);
        }
        self.ruled = record_ruled(&mut session, self.slots.len());
        self.base.ran(world);
        self.session = Some(session);
        if let Some(name) = &self.feed_name {
            let path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
            let trace = Trace::read(&path).map_err(|e| Error::Match(format!("online: {}: {e}", path.display())))?;
            self.feed = Some(Feed { trace, book: Book::new(&world.data), next: 0 });
        }
        self.gate = Gate::Play;
        eprintln!("mvs: online match on");
        Ok(())
    }

    /// One 60 Hz tick of the match.
    pub fn tick(&mut self, w: &mut World, bots: &mut [Option<BtBot>], device: InputState) -> OnlineTick {
        let now = self.hub.borrow().clock();
        let _ = self.inbox(now);
        let Some(session) = &mut self.session else { return OnlineTick::default() };
        let input = match &mut self.feed {
            Some(f) => {
                let i = f.trace.frame(f.next).ok().and_then(|fr| {
                    let n = fr.inputs.len().max(1);
                    fr.inputs.get(self.me % n).map(|r| mvs_sim_trace::input_of(r, w, &f.book))
                });
                f.next += 1;
                i.unwrap_or_default()
            }
            None => device,
        };
        let adv = session.advance(w, bots, input, now);
        let packets: Vec<Vec<u8>> = session.packets().collect();
        {
            let mut h = self.hub.borrow_mut();
            let OnlineHub { client, port, .. } = &mut *h;
            if let Some(c) = client {
                for p in packets {
                    c.send_packet(port, p, now);
                }
            }
        }
        let desync = session.desync();
        let waited = adv.stalled || adv.skipped;
        let mut events = std::mem::take(&mut self.carry);
        events.extend(adv.events);
        // (the result, read off the World once its match has ended: it changes no more)
        if self.report.is_none()
            && events
                .iter()
                .any(|e| matches!(e, mvs_sim::Event::MatchEnd { phase: mvs_sim::events::EndPhase::GameEnd, .. }))
        {
            self.report = Some(match_report(w));
        }
        let events = if waited {
            // Deviation: a frame that ran no tick has its events shown with the next one that runs.
            self.carry = events;
            None
        } else {
            Some(events)
        };
        if let Some(d) = desync
            && !self.stopped
        {
            eprintln!(
                "mvs: online desync at confirmed tick {} with slot {} (mine {:08x}, theirs {:08x})",
                d.tick, d.slot, d.mine, d.theirs
            );
            self.send(&ClientMessage::MatchDesync {
                tick: d.tick,
                slot: u32::try_from(d.slot).unwrap_or(0),
                mine: d.mine,
                theirs: d.theirs,
                config: None,
                statics: None,
                build: None,
            });
            self.report_desync(&d);
            self.stop_quiet("the match went out of sync".to_owned());
        }
        self.after_tick();
        OnlineTick { events, waited, skipped: adv.skipped, caught: adv.caught }
    }

    /// (the port's own) The desync's report written to the data folder on a thread of its own (a long match's line is
    /// megabytes), and the menus' note of it with the folder to open; a report not written is logged.
    fn report_desync(&self, d: &mvs_net::session::Desync) {
        let compat = self.hub.borrow().compat().to_owned();
        let r = desync_report_of(compat, &self.base, d, &self.ruled.borrow());
        let data = self.hub.borrow().data.clone();
        let Some(data) = data else {
            eprintln!("mvs: online desync: no data folder, no report");
            return;
        };
        let folder = data.join(desync_report::DESYNC_DIR);
        self.hub.borrow_mut().notice(OnlineNotice::Desync { folder: folder.display().to_string() });
        let spawned = std::thread::Builder::new().name("desync-report".to_owned()).spawn(move || {
            match desync_report::write(&data, &r) {
                Ok(path) => eprintln!("mvs: online desync: the report is {}", path.display()),
                Err(e) => eprintln!("mvs: online desync: the report in {}: {e}", folder.display()),
            }
        });
        if let Err(e) = spawned {
            eprintln!("mvs: online desync: no thread for the report: {e}");
        }
    }

    /// (the port's own) The slots shown away, and while this player's own link has brought no ruled packet for
    /// RESUME_TRY_MS the seconds left of RESUME_MS ("Reconnecting...", on Epic, LAN and a relay alike); 0 once
    /// RESUME_MS has passed and the match goes on all the same (a guest's link not closed yet, a host's app stalled).
    pub fn away(&self) -> (Vec<usize>, Option<u32>) {
        let shown = self.gate == Gate::Play && !self.stopped && !self.ended;
        if !shown {
            return (Vec::new(), None);
        }
        let now = self.hub.borrow().clock();
        let quiet = self.session.as_ref().and_then(Session::ruled_at).map(|r| now - r);
        let left = quiet.filter(|&q| q >= RESUME_TRY_MS).map(|q| {
            // (whole seconds left, up: 18 when first shown at RESUME_TRY_MS, 1 in the last second, then 0)
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 ..= 18 s")]
            let s = ((RESUME_MS - q) / 1000.0).ceil().max(0.0) as u32;
            s
        });
        (self.away.clone(), left)
    }

    /// The match chat's tables (MatchChat::new): this player's slot, each slot's player id and name.
    pub fn chat_table(&self) -> (usize, Vec<String>, Vec<String>) {
        (self.me, self.peers.clone(), self.usernames.clone())
    }

    /// The chats that came since the last call.
    pub fn take_chats(&mut self) -> Vec<(String, mvs_protocol::messages::ChatTo, String)> {
        std::mem::take(&mut self.chats)
    }

    /// A line of this player's match chat, sent.
    pub fn send_chat(&mut self, text: &str, all: bool) {
        self.hub.borrow_mut().send_match_chat(text, all);
    }

    /// (the port's own) The ping line's number, ms: the live round trip to the match's referee (the relay's, or the
    /// app of the player that referees it, over Epic or a direct link), the shortest of the session's clock window;
    /// before the session's first, the path's measure of it. In a match this game referees (its own referee is
    /// beside it), the longest of the other humans' round trips to it as the path measured them before the go (not
    /// live: no message gives them during the match). None where the session's host core is this game's (the host of
    /// a lobby over Epic or a local network): its referee is in this game, and no measure of its guests' round trips
    /// reaches it.
    pub fn ping(&self) -> Option<f64> {
        let live = || self.session.as_ref().and_then(Session::rtt);
        match_ping(self.path.as_ref(), self.me, self.own_core, live)
    }

    /// The match is over for this peer: its result (a match that reached its end, not stopped) before its
    /// match.leave (the server counts a leave with no result as a quit), the link closed, the menus' ping again.
    pub fn finish(self) {
        let mut h = self.hub.borrow_mut();
        let now = h.clock();
        h.match_inbox(false);
        let result = self.report.filter(|_| self.ended && !self.stopped).map(|(team, players)| {
            if self.queue
                && let Some(q) = &mut h.queue_end
            {
                q.send_result(team, Some(players), &self.map, now)
            } else {
                ClientMessage::MatchResult {
                    team,
                    players: Some(players),
                    map: (!self.map.is_empty()).then(|| self.map.clone()),
                }
            }
        });
        let OnlineHub { client, port, .. } = &mut *h;
        if let Some(c) = client {
            if let Some(r) = result {
                c.send(port, &r);
            }
            c.send(port, &ClientMessage::MatchLeave);
            if self.linked {
                c.match_end(port);
            }
            c.ping_every(PING_MENUS_MS, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mvs_data::GameData;
    use mvs_net::link::{MatchLink, SimNet, Way, Ways};
    use mvs_net::session::wire_hash;
    use mvs_protocol::match_wire::TICK_MS;
    use mvs_sim::setup::{MatchPlayer, MatchSetup};

    use super::*;

    /// A device that moves and attacks, the same for a slot each run.
    fn device(slot: usize, frame: u32) -> InputState {
        let phase = (frame / 20 + u32::try_from(slot).unwrap()) % 3;
        InputState {
            x: mvs_sim::real(f64::from(phase) - 1.0),
            attack: frame % 7 == u32::try_from(slot).unwrap(),
            jump: frame.is_multiple_of(45),
            ..InputState::default()
        }
    }

    /// (the port's own) The match's ping per case: a guest's or a relay match's live round trip (the path's before
    /// it), the worst of the others' path measures where this game referees a path's match, and none where the
    /// session's core is this game's with no path (the host of a lobby over Epic or a local network).
    #[test]
    fn the_match_ping_per_case() {
        let path = (1, vec![Some(40), Some(0), Some(90)]);
        let live = || Some(33.0);
        let none = || None;
        assert_eq!(match_ping(None, 0, false, live), Some(33.0), "a guest's live round trip");
        assert_eq!(match_ping(Some(&path), 0, false, none), Some(40.0), "before the live one, the path's");
        assert_eq!(match_ping(Some(&path), 1, false, live), Some(90.0), "the referee: the others' worst");
        assert_eq!(match_ping(Some(&path), 1, true, live), Some(90.0));
        assert_eq!(match_ping(None, 0, true, live), None, "its own core: nothing to show");
        assert_eq!(match_ping(None, 0, false, none), None);
    }

    /// `--online-desync-at` in-process: two peers over a lagging link, one's hash at tick 120 spoiled; each finds the
    /// desync, and the report written from the recorded line replays to the other peer's hash at its tick.
    #[test]
    fn a_spoiled_hash_writes_a_report_that_replays() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("note: no store; nothing checked");
            return;
        };
        let setup = MatchSetup {
            players: vec![MatchPlayer::default(), MatchPlayer::default()],
            allow_hazards: true,
            ..MatchSetup::default()
        };
        let keys = mvs_sim_trace::keys_of(&setup);
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let data = Arc::new(GameData::load(&store, &refs, mvs_sim_trace::TRACE_STAGE).expect("the gamedata kind"));
        let world = || World::new(data.clone(), 5, &setup).expect("binds");
        let way = Way { latency: 3.0 * TICK_MS, ..Way::PERFECT };
        let mut net = SimNet::new(2, &[0, 1], Ways { up: way, down: way }, 0.0, 3);
        let mut peers: Vec<(World, Session, Recorded)> = (0..2)
            .map(|me| {
                let w = world();
                let kinds = (0..2).map(|i| if i == me { SlotKind::Local } else { SlotKind::Remote }).collect();
                let mut s = Session::new(&w, SessionConfig::new(kinds), 0.0).expect("a session");
                if me == 0 {
                    s.spoil_hash_at(120);
                }
                let ruled = record_ruled(&mut s, 2);
                (w, s, ruled)
            })
            .collect();
        let mut now = 0.0;
        let mut found = None;
        for frame in 0..1200 {
            net.pump(now);
            for (slot, (w, s, _)) in peers.iter_mut().enumerate() {
                while let Some(b) = net.link(slot).recv(now) {
                    s.receive(&b, now);
                }
                let _ = s.advance(w, &mut [None, None], device(slot, frame), now);
                let sent: Vec<Vec<u8>> = s.packets().collect();
                for b in &sent {
                    net.link(slot).send(b, now);
                }
            }
            if let Some(d) = peers[1].1.desync() {
                found = Some(d);
                break;
            }
            now += TICK_MS;
        }
        let d = found.expect("the other peer finds the desync");
        assert_eq!(d.slot, 0);
        let ruled = peers[1].2.borrow().clone();
        assert!(ruled.iter().all(|l| l.len() >= d.tick as usize), "every tick up to the desync recorded");
        let started = PreMatchStarted {
            seed: 5,
            setup: MatchSetupWire::default(),
            slot: 1,
            usernames: Vec::new(),
            map: "map_check".into(),
            local: false,
            perks: Vec::new(),
        };
        let mut base = ReportBase::of(&started);
        base.ran(&peers[1].0);
        let r = desync_report_of("c1".into(), &base, &d, &ruled);
        let dir = std::env::temp_dir().join(format!("mvs-desync-match-{}", std::process::id()));
        let path = desync_report::write(&dir, &r).expect("the report is written");
        let back: DesyncReport = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // the report's line replayed from tick 0: the true hash (this peer's, `mine`), not the spoiled one sent
        let mut w = world();
        for t in 0..back.tick as usize {
            let ins: Vec<InputState> = back.inputs.iter().map(|l| mvs_net::codec::decode(&l[t])).collect();
            w.step(&ins);
            w.events.clear();
        }
        assert_eq!(wire_hash(mvs_sim::state::hash_of(&w)), back.mine);
        assert_eq!(back.mine ^ 1, back.theirs);
        assert_eq!((back.map.as_str(), back.stage.as_str(), back.me), ("map_check", mvs_sim_trace::TRACE_STAGE, 1));
    }
}
