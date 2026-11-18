//! (dev) `--queue-fake`: a queue with no server
//! for the menu tapes queue-fake and eog-queue: the queue's screens, the found match against bots
//! and the best-of-3 set after it. The rules are the server's (mvs-protocol queue_rules); what it sends is what a
//! host core would send on the queue's session, read by the same code as the server's (queue.rs, queue_end.rs,
//! pre_match.rs).
//!
//! It answers the join after ANSWER_S, finds a match against bots after the join's bot wait (`--queue-bots`, else the
//! server's 60 s), and starts it as a local match when this game's perks are in, or `start_s` after the find with
//! none (the server's PREMATCH_MS; `--queue-start`). The bots' fighters are the first the server's draw could give,
//! the map pick 0 and bot k's seed k: no randomness, the pictures are the same each run.
//!
//! The set: this game's result counts a win to its team; a team at the mode's set_wins or its set_games-th game ends
//! the set with no vote; else the bots vote yes from VOTE_BOT_MS, one VOTE_GAP_S after the other (the server draws
//! VOTE_BOT_GAP_MS: a fixed step here), a decline ends the set, all yes is match.set {rematch: true} and the
//! rematch's pm.begin REMATCH_MS later, whose pre-match runs on the rematch path: the same players, slots and bots, a
//! new map (pick game - 1), the bots' relock PM_BOT_RELOCK_MS in (told when a team-mate), pm.all when this game and
//! the bots are locked, pm.ready and match.start (local) at this game's perks. `--queue-vote-no`: the first bot
//! declines.
//!
//! Deviation: what a browser page would have sent is `sent` and `reports`, printed
//! as `mvs: fake queue: <message>` lines as they go.
//! Deviation: FakeQueueEnd.close (its timers dropped when the set's end session closes outside the rematch's wait) is
//! not kept: its timers end with the set's vote, which every path out of the set ends.

use mvs_protocol::loadout::{NetLoadout, cosmetics_of, with_cosmetics};
use mvs_protocol::lobby_rules::{LobbyRules, bot_name};
use mvs_protocol::messages::{ClientMessage, PmAll, QueueWhy, ServerMessage};
use mvs_protocol::online::{MatchStartMsg, PmBegin, PmKind, PmPick, PmRematch, QueueFoundMsg, QueueMode, QueuePlayer};
use mvs_protocol::profile::MatchReportPlayer;
use mvs_protocol::queue_rules::{
    BOT_AFTER_S, PM_BOT_RELOCK_MS, PREMATCH_MS, REMATCH_MS, VOTE_BOT_MS, VOTE_MS, pick_bots, queue_setup, queue_size,
    queue_team, queue_team_style, queue_teams, set_games, set_wins,
};
use mvs_protocol::roster::{CHARACTERS, DEFAULT_CHARACTER};

/// The server's answer to a join or a cancel, s.
const ANSWER_S: f64 = 0.5;
/// The step between the bots' votes, s.
const VOTE_GAP_S: f64 = 0.5;
/// The server's answer to a message, s.
const ECHO_S: f64 = 0.05;
/// This game's slot in the fake set.
const ME: usize = 0;

/// The command line's options of the fake queue.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeOptions {
    /// `--queue-start <s>`: the found match's start with no perks, s after the find.
    pub start_s: f64,
    /// `--queue-vote-no`: the first bot declines the rematch.
    pub vote_no: bool,
    /// `--queue-path <json>`: a pm.path sent after each found match and rematch pre-match (the pre-match's path
    /// plate with no paths-on host core); None: none, as a core with paths off.
    pub path: Option<ServerMessage>,
}

impl Default for FakeOptions {
    fn default() -> FakeOptions {
        FakeOptions { start_s: PREMATCH_MS / 1000.0, vote_no: false, path: None }
    }
}

// Whose timer a job is (each object clears its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Queue,
    End,
    Rematch,
}

#[derive(Clone, Debug, PartialEq)]
enum Job {
    Answer { on: bool, why: Option<QueueWhy> },
    Find { mode: QueueMode, character: String, skin: String },
    StartFound,
    Cast { slot: usize, yes: bool },
    Over(bool),
    Begin,
    Relock,
    All,
    Ready,
    Start(Vec<String>),
}

// The running set (fakeSet: it outlives the match and the end sessions).
#[derive(Debug)]
struct FakeSet {
    wins: Vec<u32>,
    game: u32,
    players: Vec<QueuePlayer>,
    mode: QueueMode,
    loadout: Option<NetLoadout>,
}

// FakeRematchServer's state.
#[derive(Debug)]
struct Rematch {
    game: u32,
    picks: Vec<Option<PmPick>>,
    relocked: bool,
    all: bool,
    left: bool,
    loadout: Option<NetLoadout>,
}

/// The fake host core of the queue's session.
#[derive(Debug)]
pub struct FakeCore {
    opts: FakeOptions,
    timers: Vec<(f64, Owner, Job)>,
    out: Vec<ServerMessage>,
    // FakeQueue
    found: Option<QueueFoundMsg>,
    matched: bool,
    join_loadout: Option<NetLoadout>,
    // FakeQueueEnd
    set: FakeSet,
    ballots: Vec<Option<bool>>,
    open: bool,
    waiting: bool,
    rematch: Option<Rematch>,
    // the time of the message or the timer being run
    now: f64,
    /// What this game would have sent the server, in order (mvsFakeSent: the result before the leave, PLAY AGAIN's
    /// pm.leave).
    pub sent: Vec<&'static str>,
    /// Each match.result as it would have gone (mvsFakeReports).
    pub reports: Vec<(i32, Option<Vec<MatchReportPlayer>>, Option<String>)>,
}

impl FakeCore {
    pub fn new(opts: FakeOptions) -> FakeCore {
        FakeCore {
            opts,
            timers: Vec::new(),
            out: Vec::new(),
            found: None,
            matched: false,
            join_loadout: None,
            set: FakeSet { wins: vec![0, 0], game: 1, players: Vec::new(), mode: QueueMode::OneVOne, loadout: None },
            ballots: Vec::new(),
            open: false,
            waiting: false,
            rematch: None,
            now: 0.0,
            sent: Vec::new(),
            reports: Vec::new(),
        }
    }

    fn later(&mut self, now: f64, s: f64, owner: Owner, job: Job) {
        self.timers.push((now + s * 1000.0, owner, job));
    }

    fn clear(&mut self, owner: Owner) {
        self.timers.retain(|t| t.1 != owner);
    }

    fn logged(&mut self, what: &'static str) {
        eprintln!("mvs: fake queue: {what}");
        self.sent.push(what);
    }

    /// A message this game sent on the queue's session.
    pub fn send(&mut self, m: &ClientMessage, now: f64) {
        self.now = now;
        match m {
            ClientMessage::QueueJoin { modes, character, skin, bot_after, loadout, .. } => {
                // (FakeQueue.join: the last of the modes finds the match)
                self.clear(Owner::Queue);
                self.join_loadout.clone_from(loadout);
                self.found = None;
                self.matched = false;
                let Some(mode) = modes.last().copied() else { return };
                self.later(now, ANSWER_S, Owner::Queue, Job::Answer { on: true, why: None });
                let find = Job::Find { mode, character: character.clone(), skin: skin.clone() };
                self.later(now, ANSWER_S + bot_after.unwrap_or(BOT_AFTER_S), Owner::Queue, find);
            }
            ClientMessage::QueueCancel => {
                // (a found match takes no cancel, as the server's)
                if self.found.is_some() {
                    return;
                }
                self.clear(Owner::Queue);
                self.later(now, ANSWER_S, Owner::Queue, Job::Answer { on: false, why: Some(QueueWhy::Cancel) });
            }
            ClientMessage::PmPerks { perks } => {
                if self.rematch.as_ref().is_some_and(|r| r.all && !r.left) {
                    self.later(now, ECHO_S, Owner::Rematch, Job::Ready);
                    self.later(now, 2.0 * ECHO_S, Owner::Rematch, Job::Start(perks.clone()));
                } else if self.rematch.is_none() {
                    self.start_found(Some(perks));
                }
            }
            ClientMessage::PmLock { character, skin, loadout } => {
                let Some(r) = &mut self.rematch else { return };
                if r.left || r.picks.get(ME).is_some_and(Option::is_some) {
                    return;
                }
                let character = if character.is_empty() { DEFAULT_CHARACTER.to_owned() } else { character.clone() };
                if let Some(p) = r.picks.get_mut(ME) {
                    *p = Some(PmPick { character, skin: skin.clone() });
                }
                if loadout.is_some() {
                    r.loadout.clone_from(loadout);
                }
                self.maybe_all(now);
            }
            ClientMessage::PmLeave => self.leave(),
            ClientMessage::MatchResult { team, players, map } => self.send_result(*team, players, map.as_ref(), now),
            ClientMessage::MatchVote { yes } => {
                let yes = *yes;
                self.later(now, ECHO_S, Owner::End, Job::Cast { slot: ME, yes });
            }
            ClientMessage::MatchLeave => self.logged("match.leave"),
            _ => {}
        }
    }

    // pm.leave: in the rematch's pre-match out of it; in the set's vote a decline, in the rematch's wait out of the
    // set (no pm.begin for this game); before the found match's start out of the queue (FakeQueue.leave).
    fn leave(&mut self) {
        if let Some(r) = &mut self.rematch
            && !r.left
        {
            r.left = true;
            self.clear(Owner::Rematch);
            self.logged("pm.leave");
            return;
        }
        if self.open || self.waiting {
            self.logged("pm.leave");
            if self.open {
                self.cast(ME, false);
            }
            if self.waiting {
                self.waiting = false;
                self.clear(Owner::End);
            }
            return;
        }
        self.clear(Owner::Queue);
        self.found = None;
    }

    /// The messages due by `now`, in order.
    pub fn pump(&mut self, now: f64) -> Vec<ServerMessage> {
        loop {
            // (the earliest due first; equal times in the order they were set)
            let next = self
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.0 <= now)
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.0.cmp(&b.0)))
                .map(|(i, _)| i);
            let Some(i) = next else { break };
            let (at, _, job) = self.timers.remove(i);
            self.now = at;
            self.run(job, at);
        }
        std::mem::take(&mut self.out)
    }

    /// When the next timer is due.
    pub fn next_due(&self) -> Option<f64> {
        self.timers.iter().map(|t| t.0).reduce(f64::min)
    }

    fn run(&mut self, job: Job, now: f64) {
        match job {
            Job::Answer { on, why } => self.out.push(ServerMessage::QueueState { on, why, name: None }),
            Job::Find { mode, character, skin } => self.find(mode, &character, &skin, now),
            Job::StartFound => self.start_found(None),
            Job::Cast { slot, yes } => self.cast(slot, yes),
            Job::Over(rematch) => self.over(rematch),
            Job::Begin => self.begin(now),
            Job::Relock => {
                let Some(r) = &mut self.rematch else { return };
                r.relocked = true;
                let mine = self.set.players.get(ME).map(|p| p.team);
                for (slot, p) in self.set.players.iter().enumerate() {
                    if p.bot && Some(p.team) == mine {
                        self.out.push(ServerMessage::PmLocked {
                            character: p.character.clone(),
                            skin: p.skin.clone(),
                            slot: slot as u32,
                            cosmetics: None,
                        });
                    }
                }
                self.maybe_all(now);
            }
            Job::All => {
                let Some(r) = &self.rematch else { return };
                self.out.push(ServerMessage::PmAll(PmAll {
                    picks: r.picks.clone(),
                    map: String::new(),
                    map_pick: Some(r.game - 1),
                    cosmetics: Vec::new(),
                }));
            }
            Job::Ready => self.out.push(ServerMessage::PmReady { slot: ME as u32 }),
            Job::Start(perks) => self.rematch_start(&perks),
        }
    }

    // FakeQueue.find: this game against the first bots of the server's draw, map pick 0, bot k's seed k.
    fn find(&mut self, mode: QueueMode, character: &str, skin: &str, now: f64) {
        let mut players = vec![QueuePlayer {
            id: "local".into(),
            name: String::new(),
            team: queue_team(mode, 0) as u32,
            character: character.to_owned(),
            skin: skin.to_owned(),
            bot: false,
            cosmetics: self.join_loadout.as_ref().map(cosmetics_of),
            ..QueuePlayer::default()
        }];
        let pool: Vec<String> = CHARACTERS.iter().map(|c| (*c).to_owned()).collect();
        let bots = pick_bots(&pool, &[character.to_owned()], queue_size(mode) - 1, &mut |_| 0);
        for (k, c) in bots.into_iter().enumerate() {
            players.push(QueuePlayer {
                id: format!("bot{}", k + 1),
                name: bot_name(&c),
                team: queue_team(mode, players.len()) as u32,
                character: c,
                skin: String::new(),
                bot: true,
                seed: Some(k as u32 + 1),
                ..QueuePlayer::default()
            });
        }
        // (a new set: its players and this game's loadout, for the rematches)
        self.set = FakeSet {
            wins: vec![0; queue_teams(mode)],
            game: 1,
            players: players.clone(),
            mode,
            loadout: self.join_loadout.clone(),
        };
        let found = QueueFoundMsg { match_id: "fake".into(), mode, map_pick: 0, players, me: 0 };
        self.found = Some(found.clone());
        self.out.push(ServerMessage::QueueFound(found));
        self.out.extend(self.opts.path.clone());
        let s = self.opts.start_s;
        self.later(now, s, Owner::Queue, Job::StartFound);
    }

    // FakeQueue.startFound: the server's slotLoadout (this game's taunts, ring-out, banner and input; the bots' are
    // each game's from their seeds) and the sent perks, as a local match.start.
    fn start_found(&mut self, perks: Option<&[String]>) {
        let Some(f) = self.found.clone() else { return };
        if self.matched {
            return;
        }
        self.matched = true;
        let mut setup = queue_setup(f.mode, &f.players, LobbyRules::default().match_duration);
        if let Some(l) = &self.join_loadout
            && let Some(p) = setup.players.get_mut(f.me as usize)
        {
            *p = with_cosmetics(p, Some(&l.taunts), Some(&l.ring_out), Some(&l.banner.slug), None, l.input.as_ref());
        }
        let sent: Vec<Vec<String>> = (0..f.players.len())
            .map(|i| match perks {
                Some(p) if i == f.me as usize => p.iter().take(4).cloned().collect(),
                _ => Vec::new(),
            })
            .collect();
        self.new_end();
        self.out.push(ServerMessage::MatchStart(MatchStartMsg {
            seed: 1,
            setup,
            slot: f.me,
            usernames: f.players.iter().map(|p| p.name.clone()).collect(),
            map: String::new(),
            map_pick: Some(f.map_pick),
            local: Some(true),
            match_id: Some(f.match_id.clone()),
            perks: Some(sent),
        }));
    }

    // A match's set's end session (FakeQueueEnd's new state).
    fn new_end(&mut self) {
        self.ballots = vec![None; self.set.players.len()];
        self.open = false;
        self.clear(Owner::End);
    }

    // FakeQueueEnd.sendResult.
    fn send_result(&mut self, team: i32, players: &Option<Vec<MatchReportPlayer>>, map: Option<&String>, now: f64) {
        self.logged("match.result");
        self.reports.push((team, players.clone(), map.cloned()));
        if let Ok(t) = usize::try_from(team)
            && let Some(w) = self.set.wins.get_mut(t)
        {
            *w += 1;
        }
        let mode = self.set.mode;
        if self.set.wins.iter().any(|w| *w >= set_wins(mode)) || self.set.game >= set_games(mode) {
            self.later(now, ECHO_S, Owner::End, Job::Over(false));
            return;
        }
        self.open = true;
        self.later(now, VOTE_MS / 1000.0, Owner::End, Job::Over(false));
        let mut at = VOTE_BOT_MS / 1000.0;
        let mut first = true;
        let bots: Vec<usize> = self.set.players.iter().enumerate().filter(|(_, p)| p.bot).map(|(i, _)| i).collect();
        for slot in bots {
            let yes = !(self.opts.vote_no && first);
            first = false;
            self.later(now, at, Owner::End, Job::Cast { slot, yes });
            at += VOTE_GAP_S;
        }
    }

    fn cast(&mut self, slot: usize, yes: bool) {
        if !self.open || self.ballots.get(slot).is_none_or(Option::is_some) {
            return;
        }
        self.ballots[slot] = Some(yes);
        self.out.push(ServerMessage::MatchVotes { slot: slot as u32, yes });
        if !yes {
            self.over(false);
        } else if self.ballots.iter().all(|v| *v == Some(true)) {
            self.over(true);
        }
    }

    fn over(&mut self, rematch: bool) {
        self.open = false;
        self.clear(Owner::End);
        self.out.push(ServerMessage::MatchSet { rematch, wins: self.set.wins.clone(), game: self.set.game });
        if !rematch {
            self.set.wins = vec![0; queue_teams(self.set.mode)];
            self.set.game = 1;
            return;
        }
        self.waiting = true;
        let now = self.now;
        self.later(now, REMATCH_MS / 1000.0, Owner::End, Job::Begin);
    }

    // The rematch's pm.begin (FakeRematchServer's constructor).
    fn begin(&mut self, now: f64) {
        self.waiting = false;
        self.set.game += 1;
        let game = self.set.game;
        let players = &self.set.players;
        let mode = self.set.mode;
        let begin = PmBegin {
            id: format!("fake-rematch-{game}"),
            kind: PmKind::Queue,
            mode: Some(mode),
            team_style: queue_team_style(mode),
            map: String::new(),
            players: players
                .iter()
                .map(|p| {
                    if p.bot {
                        p.clone()
                    } else {
                        QueuePlayer { character: String::new(), skin: String::new(), ..p.clone() }
                    }
                })
                .collect(),
            me: ME as u32,
            rematch: Some(PmRematch { game, wins: self.set.wins.clone() }),
        };
        let picks = players
            .iter()
            .map(|p| p.bot.then(|| PmPick { character: p.character.clone(), skin: p.skin.clone() }))
            .collect();
        self.rematch =
            Some(Rematch { game, picks, relocked: false, all: false, left: false, loadout: self.set.loadout.clone() });
        self.out.push(ServerMessage::PmBegin(begin));
        self.out.extend(self.opts.path.clone());
        self.later(now, PM_BOT_RELOCK_MS / 1000.0, Owner::Rematch, Job::Relock);
    }

    fn maybe_all(&mut self, now: f64) {
        let Some(r) = &mut self.rematch else { return };
        if r.all || !r.relocked || r.picks.iter().any(Option::is_none) {
            return;
        }
        r.all = true;
        self.later(now, ECHO_S, Owner::Rematch, Job::All);
    }

    // FakeRematchServer.start: the rematch's local match.start at this game's perks.
    fn rematch_start(&mut self, perks: &[String]) {
        let Some(r) = self.rematch.take() else { return };
        let picked: Vec<QueuePlayer> = self
            .set
            .players
            .iter()
            .zip(&r.picks)
            .map(|(p, k)| {
                let k = k.clone().unwrap_or_default();
                QueuePlayer { character: k.character, skin: k.skin, ..p.clone() }
            })
            .collect();
        let mut setup = queue_setup(self.set.mode, &picked, LobbyRules::default().match_duration);
        for (i, p) in setup.players.iter_mut().enumerate() {
            let skin = Some(picked[i].skin.as_str());
            *p = match &r.loadout {
                Some(l) if !p.bot => {
                    with_cosmetics(p, Some(&l.taunts), Some(&l.ring_out), Some(&l.banner.slug), skin, l.input.as_ref())
                }
                _ => with_cosmetics(p, None, None, None, skin, None),
            };
        }
        let sent = (0..self.set.players.len())
            .map(|i| if i == ME { perks.iter().take(4).cloned().collect() } else { Vec::new() })
            .collect();
        self.new_end();
        let game = r.game;
        self.out.push(ServerMessage::MatchStart(MatchStartMsg {
            seed: game,
            setup,
            slot: ME as u32,
            usernames: self.set.players.iter().map(|p| p.name.clone()).collect(),
            map: String::new(),
            map_pick: Some(game - 1),
            local: Some(true),
            match_id: Some(format!("fake-rematch-{game}")),
            perks: Some(sent),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(f: &mut FakeCore, bot_after: f64) {
        let m = ClientMessage::QueueJoin {
            modes: vec![QueueMode::OneVOne],
            character: "shaggy".into(),
            skin: String::new(),
            compat: "c1".into(),
            bot_after: Some(bot_after),
            loadout: None,
            draw: None,
            waited: None,
            party: None,
            rtt: None,
        };
        f.send(&m, 0.0);
    }

    fn names(m: &[ServerMessage]) -> Vec<String> {
        m.iter().map(|m| m.text().split('"').nth(3).unwrap_or_default().to_owned()).collect()
    }

    #[test]
    fn a_join_finds_bots_and_starts_at_the_perks() {
        let mut f = FakeCore::new(FakeOptions::default());
        join(&mut f, 3.0);
        assert_eq!(names(&f.pump(500.0)), ["queue.state"]);
        let found = f.pump(3500.0);
        let ServerMessage::QueueFound(q) = &found[0] else { panic!("{found:?}") };
        assert_eq!((q.players.len(), q.players[1].bot, q.players[1].seed), (2, true, Some(1)));
        assert_ne!(q.players[1].character, "shaggy");
        f.send(&ClientMessage::PmPerks { perks: vec!["a".into()] }, 4000.0);
        let start = f.pump(4000.0);
        let ServerMessage::MatchStart(s) = &start[0] else { panic!("{start:?}") };
        assert_eq!(
            (s.local, s.match_id.as_deref(), s.perks.clone()),
            (Some(true), Some("fake"), Some(vec![vec!["a".to_owned()], vec![]]))
        );
        // (no second start at the time with no perks)
        assert!(f.pump(PREMATCH_MS + 4000.0).is_empty());
    }

    #[test]
    fn queue_path_follows_the_find() {
        let path = ServerMessage::PmPath { referee: -1, ms: vec![Some(56), None], far: vec![true, false] };
        let mut f = FakeCore::new(FakeOptions { path: Some(path.clone()), ..FakeOptions::default() });
        join(&mut f, 3.0);
        f.pump(500.0);
        let found = f.pump(3500.0);
        assert!(matches!(&found[..], [ServerMessage::QueueFound(_), p] if *p == path), "{found:?}");
    }

    #[test]
    fn a_won_game_votes_and_the_rematch_begins() {
        let mut f = FakeCore::new(FakeOptions { start_s: 1.0, vote_no: false, path: None });
        join(&mut f, 3.0);
        f.pump(3500.0);
        assert_eq!(names(&f.pump(4500.0)), ["match.start"]);
        f.send(&ClientMessage::MatchResult { team: 0, players: None, map: None }, 10_000.0);
        f.send(&ClientMessage::MatchLeave, 10_000.0);
        f.send(&ClientMessage::MatchVote { yes: true }, 10_000.0);
        let votes = f.pump(10_000.0 + VOTE_BOT_MS);
        assert_eq!(names(&votes), ["match.votes", "match.votes", "match.set"]);
        assert!(matches!(votes[2], ServerMessage::MatchSet { rematch: true, game: 1, .. }));
        let begin = f.pump(10_000.0 + VOTE_BOT_MS + REMATCH_MS);
        let ServerMessage::PmBegin(b) = &begin[0] else { panic!("{begin:?}") };
        assert_eq!((b.rematch.as_ref().map(|r| r.game), b.players[0].character.as_str()), (Some(2), ""));
        f.send(&ClientMessage::PmLock { character: "velma".into(), skin: String::new(), loadout: None }, 20_000.0);
        assert_eq!(names(&f.pump(20_000.0 + PM_BOT_RELOCK_MS)), ["pm.all"]);
        f.send(&ClientMessage::PmPerks { perks: Vec::new() }, 30_000.0);
        let go = f.pump(30_200.0);
        assert_eq!(names(&go), ["pm.ready", "match.start"]);
        assert_eq!(f.sent, ["match.result", "match.leave"]);
    }

    #[test]
    fn a_decline_ends_the_set() {
        let mut f = FakeCore::new(FakeOptions { start_s: 1.0, vote_no: true, path: None });
        join(&mut f, 3.0);
        f.pump(4500.0);
        f.send(&ClientMessage::MatchResult { team: 1, players: None, map: None }, 10_000.0);
        let set = f.pump(10_000.0 + VOTE_BOT_MS);
        assert!(matches!(set.last(), Some(ServerMessage::MatchSet { rematch: false, .. })), "{set:?}");
    }
}
