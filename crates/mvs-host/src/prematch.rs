//! The pre-match, the match records and the set ("the pre-match and the set", "the match records",
//! "the set"). The core keeps every time that ends a phase (a hidden game runs no menu delay); a game's own lock at
//! its expiry is taken as any lock, and the core's always wins. Each timer is the record's (cleared at each phase's end
//! and on every exit).
//! A panic of the core in a record's step is a bug and is not caught.

use indexmap::IndexSet;
use mvs_protocol::loadout::{cosmetics_of, read_loadout, with_cosmetics};
use mvs_protocol::messages::{PmAll, PmEndWhy, QueueWhy, ServerMessage};
use mvs_protocol::online::{PlayerCosmetics, PmBegin, PmKind, PmPick, PmRematch, QueueMode, QueuePlayer, SEED_MAX};
use mvs_protocol::profile::HistoryMode;
use mvs_protocol::queue_rules::{
    PM_BOT_LOCK_MS, PM_BOT_RELOCK_MS, VOTE_BOT_GAP_MS, VOTE_BOT_MS, clean_perks, queue_teams, set_games, set_wins,
};
use mvs_protocol::record::{FactPlayer, RecordFacts, ReportRules, agree, build_record, match_trackers, read_report};
use mvs_protocol::roster::{CHARACTERS, DEFAULT_CHARACTER};
use mvs_protocol::setup::{MatchPlayerWire, MatchSetupWire};
use serde_json::{Map, Value};

use crate::core::{Core, MatchId, Played, PlayedId, PmDue, PmId, PmPhase, PreMatch, SlotLoadout, asset_name, map_name};
use crate::draws::CombinedDraws;
use crate::link::ConnKey;
use crate::lobby::{base36, js_len};

/// A pick as a game sends it (pm.preview, pm.lock): a character of CHARACTERS ('' the default one) and a skin's
/// name; None: not one (pickOf).
fn pick_of(m: &Map<String, Value>) -> Option<PmPick> {
    let (c, s) = (m.get("character")?.as_str()?, m.get("skin")?.as_str()?);
    if js_len(c) > 64 || js_len(s) > 64 {
        return None;
    }
    let asked = asset_name(m.get("character"));
    let skin = asset_name(m.get("skin"))?;
    let character = match asked.as_deref() {
        Some("") => DEFAULT_CHARACTER.to_string(),
        Some(a) => CHARACTERS.iter().find(|&&x| x == a)?.to_string(),
        None => return None,
    };
    Some(PmPick { character, skin })
}

/// Bytes as base64url without padding (Buffer's toString('base64url')).
fn base64url(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for ch in b.chunks(3) {
        let n = ch.iter().enumerate().fold(0u32, |n, (i, &x)| n | (u32::from(x) << (16 - 8 * i)));
        for i in 0..=ch.len() {
            out.push(char::from(A[usize::try_from((n >> (18 - 6 * i)) & 63).expect("six bits")]));
        }
    }
    out
}

fn slot_index(slot: i32) -> usize {
    usize::try_from(slot).expect("host core: a human's slot")
}

fn slot_u32(slot: i32) -> u32 {
    u32::try_from(slot).expect("host core: a human's slot")
}

impl Core {
    pub(crate) fn pm(&self, pm: PmId) -> &PreMatch {
        self.pms.get(&pm).unwrap_or_else(|| panic!("host core: no pre-match {pm}"))
    }

    pub(crate) fn pm_mut(&mut self, pm: PmId) -> &mut PreMatch {
        self.pms.get_mut(&pm).unwrap_or_else(|| panic!("host core: no pre-match {pm}"))
    }

    fn pm_alive(&self, pm: PmId) -> bool {
        self.pms.get(&pm).is_some_and(|p| p.alive)
    }

    /// A pre-match record (newPreMatch): its id, its map (a lobby's from its maps) or its map pick (a queue's from its
    /// draws) drawn in that order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_pre_match(
        &mut self,
        kind: PmKind,
        mode: Option<QueueMode>,
        lobby: bool,
        team_style: u8,
        maps: Option<&[String]>,
        players: Vec<QueuePlayer>,
        setup: MatchSetupWire,
        humans: &[ConnKey],
        mut rand: Option<CombinedDraws>,
    ) -> PmId {
        let id = base36(self.rand.int(1, 0x7fff_ffff));
        let map = match maps {
            Some(m) if !m.is_empty() => {
                let n = u64::try_from(m.len()).expect("a map count");
                m[usize::try_from(self.rand.int(0, n)).expect("below the count")].clone()
            }
            _ => String::new(),
        };
        let map_pick = match (maps, rand.as_mut()) {
            (Some(_), _) => 0,
            (None, Some(r)) => 1 + r.int(u64::from(SEED_MAX)),
            (None, None) => u32::try_from(self.rand.int(1, u64::from(SEED_MAX) + 1)).expect("below SEED_MAX + 1"),
        };
        let n = players.len();
        let pm = PreMatch {
            id,
            kind,
            mode,
            lobby,
            team_style,
            map,
            map_pick,
            rand,
            players,
            setup,
            humans: humans.to_vec(),
            phase: PmPhase::Pick,
            begun: false,
            picks: vec![None; n],
            previews: vec![None; n],
            perks: vec![None; n],
            loadouts: vec![None; n],
            timers: IndexSet::new(),
            alive: true,
            result_due: false,
            game: 1,
            wins: vec![0; mode.map_or(2, queue_teams)],
            results: Default::default(),
            early: Default::default(),
            votes: Vec::new(),
            gone: IndexSet::new(),
            path: None,
        };
        let key = self.new_pm_id();
        self.pms.insert(key, pm);
        key
    }

    pub(crate) fn pm_later(&mut self, pm: PmId, ms: f64, due: PmDue) {
        let t = self.later(ms, crate::core::Due::Pm(pm, due), false);
        self.pm_mut(pm).timers.insert(t);
    }

    fn clear_timers(&mut self, pm: PmId) {
        let timers = std::mem::take(&mut self.pm_mut(pm).timers);
        for t in timers {
            self.cancel(t);
        }
    }

    /// The record is over: no timer, no player and no lobby holds it.
    fn drop_pre_match(&mut self, pm: PmId) {
        self.pm_mut(pm).alive = false;
        self.clear_timers(pm);
        for k in self.pm(pm).humans.clone() {
            if let Some(c) = self.clients.get_mut(&k)
                && c.pm == Some(pm)
            {
                c.pm = None;
            }
        }
        if let Some(l) = self.lobby.as_mut()
            && l.pm == Some(pm)
        {
            l.pm = None;
        }
    }

    fn tell_humans(&mut self, pm: PmId, m: &ServerMessage) {
        let text = m.text();
        for k in self.pm(pm).humans.clone() {
            self.send_text(k, &text);
        }
    }

    /// The human team-mates of a slot (2v2; nobody in 1v1 or Free For All): they see its preview and its lock.
    fn team_mates(&self, pm: PmId, slot: usize) -> Vec<ConnKey> {
        let p = self.pm(pm);
        if p.team_style == 2 {
            return Vec::new();
        }
        let team = p.players[slot].team;
        p.humans
            .iter()
            .copied()
            .filter(|&k| {
                let s = self.c(k).slot;
                slot_index(s) != slot && p.players.get(slot_index(s)).is_some_and(|x| x.team == team)
            })
            .collect()
    }

    /// A pre-match ends before its match (a leave, a drop, a reload): every human left in it is told, and goes back
    /// to the main menu (OnPlayerAbandonedPreMatch, UI_PreMatch @122078; the lobby is kept). The queue's first
    /// pre-match keeps its message (queue.state off, 'left', the leaver's name: OnMatchAbandonedByPlayer, UI_PreMatch
    /// @31771).
    fn end_pre_match(&mut self, pm: PmId, why: PmEndWhy, name: &str) {
        let p = self.pm(pm);
        let tell: Vec<ConnKey> = p.humans.iter().copied().filter(|&k| self.c(k).pm == Some(pm)).collect();
        let begun = p.begun;
        // in the wait for a rematch a human may still be in the set's previous match (detached): told all the same
        let waiting = p.phase == PmPhase::Rematch;
        self.drop_pre_match(pm);
        for k in tell {
            if self.c(k).match_id.is_some() && !waiting {
                continue;
            }
            if begun {
                self.send(k, &ServerMessage::PmEnd { why, name: name.to_string() });
            } else {
                // (not 'error': a game ignores that while found)
                let name = (!name.is_empty()).then(|| name.to_string());
                self.send(k, &ServerMessage::QueueState { on: false, why: Some(QueueWhy::Left), name });
            }
        }
        let why = match why {
            PmEndWhy::Left => "left",
            PmEndWhy::Error => "error",
        };
        self.log(&format!("pre-match called off: {why} {name}"));
    }

    /// A human leaves the record (pm.leave, a lost link, a lobby left, or a game that went on to the queue or a lobby
    /// after its match). Before the match, and in the wait for a rematch: the pre-match ends for everyone. In the
    /// match: the slot is a leaver's, which declines the vote, and the result is the others' to send. In the vote: a
    /// decline (a logout is one, PodiumGameMode PGM@13869), and the set ends.
    /// Deviation: a leaver who had voted yes ends the set too (the build keeps an account's first vote, PGM@17764, so
    /// its rematch would start without them); the port has no way back into a set.
    pub(crate) fn leave_pre_match(&mut self, k: ConnKey) {
        let Some(pm) = self.c(k).pm else { return };
        self.cm(k).pm = None;
        let Some(p) = self.pms.get_mut(&pm) else { return };
        p.humans.retain(|&x| x != k);
        if !p.alive {
            return;
        }
        let slot = self.c(k).slot;
        match self.pm(pm).phase {
            PmPhase::Match => {
                let p = self.pm_mut(pm);
                p.gone.insert(slot);
                p.early.shift_remove(&slot);
                if p.humans.is_empty() {
                    self.drop_pre_match(pm);
                } else {
                    self.settle_result(pm, false);
                }
            }
            PmPhase::Vote => {
                if self.pm(pm).votes.get(slot_index(slot)).is_some_and(Option::is_none) {
                    self.cast_vote(pm, slot_index(slot), false);
                }
                if self.pm_alive(pm) {
                    self.set_over(pm, false);
                }
            }
            _ => {
                let name = self.c(k).name.clone();
                self.end_pre_match(pm, PmEndWhy::Left, &name);
            }
        }
    }

    /// A game that goes on after its queue match (queue.join, lobby.enter, lobby.join) has left its set.
    pub(crate) fn gone_on(&mut self, k: ConnKey) {
        let c = self.c(k);
        let ph = c.pm.and_then(|p| self.pms.get(&p)).map(|p| p.phase);
        if c.match_id.is_none() && matches!(ph, Some(PmPhase::Match | PmPhase::Vote | PmPhase::Rematch)) {
            self.leave_pre_match(k);
        }
    }

    /// pm.begin: the pick phase (a lobby's start; a queue set's rematch, OnRematchStarted, on the same humans, bots
    /// and slots). A rematch takes its humans out of the set's previous match (match.left 'rematch' to a peer still in
    /// it), keeps the set's map pick and each human's loadout (a pick sent without one keeps it, keep_loadout).
    pub(crate) fn begin_pre_match(&mut self, pm: PmId) {
        self.clear_timers(pm);
        let id = base36(self.rand.int(1, 0x7fff_ffff));
        let p = self.pm_mut(pm);
        p.id = id;
        p.phase = PmPhase::Pick;
        p.begun = true;
        let n = p.players.len();
        p.picks = vec![None; n];
        p.previews = vec![None; n];
        p.perks = vec![None; n];
        let rematch = (p.kind == PmKind::Queue).then(|| PmRematch { game: p.game, wins: p.wins.clone() });
        // Unverified: a rematch keeps the set's map (pm.mapPick as drawn at the set's start): the build's
        // RematchMapPath is the backend's, from CharacterSelectManager.CopyMatchForPlayAgain (PodiumGameMode
        // PGM@3641), whose native is not read
        if rematch.is_some() {
            for k in self.pm(pm).humans.clone() {
                let Some(m) = self.c(k).match_id else { continue };
                if let Some(m) = self.matches.get_mut(&m)
                    && m.pm == Some(pm)
                {
                    m.pm = None;
                }
                self.leave_match(k, mvs_protocol::messages::LeftWhy::Rematch);
            }
        }
        let p = self.pm(pm);
        let players: Vec<QueuePlayer> = p
            .players
            .iter()
            .map(|x| {
                if x.bot {
                    x.clone()
                } else {
                    QueuePlayer { character: String::new(), skin: String::new(), ..x.clone() }
                }
            })
            .collect();
        let (pid, kind, mode, team_style) = (p.id.clone(), p.kind, p.mode, p.team_style);
        for k in p.humans.clone() {
            let begin = PmBegin {
                id: pid.clone(),
                kind,
                mode,
                team_style,
                map: String::new(),
                players: players.clone(),
                me: slot_u32(self.c(k).slot),
                rematch: rematch.clone(),
            };
            self.send(k, &ServerMessage::PmBegin(begin));
        }
        if self.pm(pm).players.iter().any(|x| x.bot) {
            self.pm_later(pm, if rematch.is_some() { PM_BOT_RELOCK_MS } else { PM_BOT_LOCK_MS }, PmDue::BotLock);
        }
        let ms = self.times.pick_ms;
        self.pm_later(pm, ms, PmDue::PickTimeUp);
        let p = self.pm(pm);
        let names: Vec<&str> = p.players.iter().map(|x| x.name.as_str()).collect();
        let kind = match p.kind {
            PmKind::Lobby => "lobby",
            PmKind::Queue => "queue",
        };
        let game = if rematch.is_some() { format!("game {}", p.game) } else { String::new() };
        let line = format!("pre-match begin {kind} {game} {}", names.join(" / "));
        self.log(&line);
        self.path_begin(pm);
    }

    /// A deadline of a living record.
    pub(crate) fn run_pm_due(&mut self, pm: PmId, due: PmDue) {
        match due {
            PmDue::Start => self.start_pre_match(pm),
            PmDue::BotLock => {
                let bots: Vec<(usize, PmPick)> = self
                    .pm(pm)
                    .players
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.bot)
                    .map(|(s, p)| (s, PmPick { character: p.character.clone(), skin: p.skin.clone() }))
                    .collect();
                for (slot, pick) in bots {
                    self.lock_slot(pm, slot, pick);
                }
            }
            PmDue::PickTimeUp => self.pick_time_up(pm),
            PmDue::Result => self.settle_result(pm, true),
            PmDue::VoteOver => self.set_over(pm, false),
            PmDue::BotVote { slot, yes } => self.cast_vote(pm, slot, yes),
            PmDue::Rematch => {
                self.pm_mut(pm).game += 1;
                self.begin_pre_match(pm);
            }
        }
    }

    /// A slot's loadout sent with its preview or its lock (read_loadout; none sent keeps the one before).
    fn keep_loadout(&mut self, pm: PmId, slot: usize, pick: &PmPick, v: Option<&Value>) {
        if let Some(loadout) = read_loadout(v.unwrap_or(&Value::Null)) {
            self.pm_mut(pm).loadouts[slot] = Some(SlotLoadout { character: pick.character.clone(), loadout });
        }
    }

    /// A slot's lock, once: its team-mates are told (the build preloads a team-mate's fighter, UI_PreMatch @75855);
    /// with every slot locked, the reveal.
    fn lock_slot(&mut self, pm: PmId, slot: usize, pick: PmPick) {
        let p = self.pm_mut(pm);
        if p.phase != PmPhase::Pick || p.picks[slot].is_some() {
            return;
        }
        p.picks[slot] = Some(pick.clone());
        let cosmetics = self.slot_cosmetics(pm, slot);
        let msg = ServerMessage::PmLocked {
            character: pick.character,
            skin: pick.skin,
            slot: u32::try_from(slot).expect("a slot"),
            cosmetics,
        };
        for k in self.team_mates(pm, slot) {
            self.send(k, &msg);
        }
        if self.pm(pm).picks.iter().all(Option::is_some) {
            self.all_locked(pm);
        }
    }

    /// The pick's time is over: each unlocked human gets their latest preview, else the default fighter (the build's
    /// HandleTimerExpired locks the last valid preview); a bot its own.
    fn pick_time_up(&mut self, pm: PmId) {
        for slot in 0..self.pm(pm).players.len() {
            let p = self.pm(pm);
            if p.picks[slot].is_some() {
                continue;
            }
            let x = &p.players[slot];
            let pick = if x.bot {
                PmPick { character: x.character.clone(), skin: x.skin.clone() }
            } else {
                p.previews[slot]
                    .clone()
                    .unwrap_or_else(|| PmPick { character: DEFAULT_CHARACTER.to_string(), skin: String::new() })
            };
            self.lock_slot(pm, slot, pick);
        }
    }

    /// pm.all: every slot is locked (OnPlayersReadyToMatchmake): the picks and the map to all, then the perks' time.
    fn all_locked(&mut self, pm: PmId) {
        self.clear_timers(pm);
        self.pm_mut(pm).phase = PmPhase::Perks;
        let p = self.pm(pm);
        let all = PmAll {
            picks: p.picks.clone(),
            map: p.map.clone(),
            map_pick: (p.kind == PmKind::Queue).then_some(p.map_pick),
            cosmetics: (0..p.players.len()).map(|s| self.slot_cosmetics(pm, s)).collect(),
        };
        self.tell_humans(pm, &ServerMessage::PmAll(all));
        let ms = self.times.perks_ms;
        self.pm_later(pm, ms, PmDue::Start);
    }

    /// A slot's pre-match cosmetics: a human's as its game sent them (None: none sent); a bot's None (each game has
    /// its own draw from the bot's seed).
    /// A bot's random cosmetics are a Deviation: the seed, drawn once as its slot is made (QueuePlayer.seed); each
    /// game draws the bot's pre-match cosmetics and its ring-out from it over its own pool, so every game has the
    /// same. A queue bot's from the match's draws.
    fn slot_cosmetics(&self, pm: PmId, slot: usize) -> Option<PlayerCosmetics> {
        let p = self.pm(pm);
        if p.players[slot].bot {
            return None;
        }
        p.loadouts[slot].as_ref().map(|l| cosmetics_of(&l.loadout))
    }

    /// A setup player with its slot's taunts, ring-out and banner: a human's as its game sent them (the taunts only
    /// for the character they were sent with; the banner is the podium's), a bot's none here: each game puts in the
    /// ring-out and banner it drew from the bot's seed, and no taunts (the build: LobbyGameMode AddBots' ROV_OmegaBeam
    /// and its CharacterSelectPlayerData Banner null; Deviation: the port draws random ones). Unverified: a bot's
    /// taunts online (GetMatchPlayerTaunts asks Hydra by account id, which a bot has no profile under; its pawn then
    /// takes its own default) are left at the default. The skin is the slot's pick's (its SkinData name as the lock or
    /// the preview sent it, a bot's its row's; the core passes it on as sent: a name no game has plays the default
    /// there). A human's input preferences are its game's, cleaned.
    fn slot_loadout(&self, pm: PmId, slot: usize, p: &MatchPlayerWire) -> MatchPlayerWire {
        let x = self.pm(pm);
        let skin = x.picks[slot].as_ref().map(|k| k.skin.as_str()).filter(|s| !s.is_empty());
        if p.bot {
            return with_cosmetics(p, None, None, None, skin, None);
        }
        match &x.loadouts[slot] {
            Some(l) => {
                let taunts = (Some(&l.character) == p.character.as_ref()).then_some(l.loadout.taunts.as_slice());
                with_cosmetics(
                    p,
                    taunts,
                    Some(&l.loadout.ring_out),
                    Some(&l.loadout.banner.slug),
                    skin,
                    l.loadout.input.as_ref(),
                )
            }
            None => with_cosmetics(p, None, None, None, skin, None),
        }
    }

    /// Every human's perks are in, or the perk time is over: match.start, as a lobby's, with the match's id and the
    /// perks by slot (a bot's [], each game takes its own for its pick; a queue match's map by its pick). The
    /// set-up's fighters are the picks.
    fn start_pre_match(&mut self, pm: PmId) {
        if self.pm(pm).phase != PmPhase::Perks {
            return;
        }
        self.clear_timers(pm);
        self.path_settle(pm);
        let p = self.pm(pm);
        let players = p
            .setup
            .players
            .iter()
            .enumerate()
            .map(|(slot, x)| {
                let character = p.picks[slot].as_ref().map_or(DEFAULT_CHARACTER.to_string(), |k| k.character.clone());
                self.slot_loadout(pm, slot, &MatchPlayerWire { character: Some(character), ..x.clone() })
            })
            .collect();
        let setup = MatchSetupWire { players, ..p.setup.clone() };
        let perks: Vec<Vec<String>> = p
            .players
            .iter()
            .enumerate()
            .map(|(s, x)| if x.bot { Vec::new() } else { p.perks[s].clone().unwrap_or_default() })
            .collect();
        let peers = p.humans.clone();
        let set = (p.kind == PmKind::Queue).then_some(pm);
        let usernames: Vec<String> = p.players.iter().map(|x| x.name.clone()).collect();
        let (lobby, map, id) = (p.lobby, p.map.clone(), p.id.clone());
        let map_pick = (p.kind == PmKind::Queue).then_some(p.map_pick);
        let m = self.start_match(&peers, setup.clone(), usernames, lobby, map, id, perks, map_pick, set);
        let played = self.new_played(pm, &setup, &peers);
        let chosen = self.path_chosen(pm);
        if let Some(m) = self.matches.get_mut(&m) {
            m.played = Some(played);
            m.chosen = chosen;
        }
        // a lobby's record ends here (no set); a queue's waits for its match's result
        if set.is_some() {
            let p = self.pm_mut(pm);
            p.phase = PmPhase::Match;
            p.results.clear();
            p.early.clear();
            p.gone.clear();
            p.result_due = false;
        } else {
            self.drop_pre_match(pm);
        }
    }

    // ---------- the match records (record.rs; each game keeps its own) ----------
    // Every queue game and every lobby match is recorded once it is over for its humans: each has sent its
    // match.result or left the match without one (the core's end of a match, a load timeout or matchTicks, is a leave
    // for each). A result counts from the match's go on; a game's own one-human match (local) has no go and its result
    // counts at once. A match none of whose humans sent a result leaves no record. The core rules the facts it set up;
    // the winner and the numbers are the games' (record.rs agree). Each human of the record still linked gets it as
    // match.record with its own share: totals for one who played a queue match to the end, never for a custom match;
    // the stat trackers of a match with no bot, a custom one too (match_trackers). Nothing else waits on the record.

    /// A match's record to be, as it starts: the slots, names, teams and picks of the pre-match, the human slots' ids
    /// (the links'), the ring-outs to win a report is checked against (none for the stock rule, whose count is lives,
    /// not a score to reach). The record's id is its own (the pre-match's is short and only one session's).
    fn new_played(&mut self, pm: PmId, setup: &MatchSetupWire, peers: &[ConnKey]) -> PlayedId {
        let p = self.pm(pm);
        let players: Vec<FactPlayer> = p
            .players
            .iter()
            .enumerate()
            .map(|(slot, x)| {
                let s = i32::try_from(slot).expect("a slot");
                let human = peers.iter().find(|&&k| self.c(k).slot == s);
                FactPlayer {
                    id: if x.bot { String::new() } else { human.map_or(x.id.clone(), |&k| self.c(k).id.clone()) },
                    name: x.name.clone(),
                    character: setup
                        .players
                        .get(slot)
                        .and_then(|y| y.character.clone())
                        .unwrap_or_else(|| DEFAULT_CHARACTER.to_string()),
                    team: i32::try_from(x.team).expect("a team"),
                    slot: s,
                    bot: x.bot,
                }
            })
            .collect();
        let (team_style, lobby, map, game) = (p.team_style, p.kind == PmKind::Lobby, p.map.clone(), p.game);
        let mut id = [0u8; 12];
        self.rand.bytes(&mut id);
        let rules = ReportRules {
            slot_teams: players.iter().map(|x| x.team).collect(),
            teams: mvs_protocol::setup::team_count(team_style, players.len()),
            knockouts_to_win: match &setup.rules {
                Some(r) if r.stocks == Some(true) => 0,
                Some(r) => r.knockouts_to_win,
                None => 0,
            },
        };
        let facts = RecordFacts {
            id: base64url(&id),
            mode: HistoryMode(team_style),
            map: if lobby { map } else { String::new() },
            custom: lobby,
            players,
        };
        let pending = peers.iter().map(|&k| self.c(k).slot).collect();
        let pl = Played {
            facts,
            game: if lobby { 1 } else { game },
            rules,
            pending,
            left: IndexSet::new(),
            reports: Vec::new(),
            done: false,
            forfeit: false,
        };
        let key = self.new_played_id();
        self.played.insert(key, pl);
        self.recording.insert(key);
        key
    }

    /// A human's match.result, the first of its slot from the go on (one that cannot be read, read_report, counts as
    /// sent, with nothing in it).
    fn played_report(&mut self, m: MatchId, slot: i32, msg: &Map<String, Value>) {
        let Some(mt) = self.matches.get(&m) else { return };
        let (Some(pl), on) = (mt.played, mt.going || mt.local) else { return };
        let Some(p) = self.played.get_mut(&pl) else { return };
        if p.done || !on || !p.pending.shift_remove(&slot) {
            return;
        }
        if let Some(r) = read_report(msg, &p.rules, &map_name) {
            p.reports.push(r);
        }
        if p.pending.is_empty() {
            self.played_end(pl);
        }
    }

    /// A human left the match (or the core ended it) before sending a result: none is waited for from that slot, and
    /// it gets no totals (the build's BotFromDisconnection, Comp_Actor_EndOfGameHandler HandleRewards @1100-1228).
    pub(crate) fn played_gone(&mut self, pl: PlayedId, slot: i32) {
        let Some(p) = self.played.get_mut(&pl) else { return };
        if p.done || !p.pending.shift_remove(&slot) {
            return;
        }
        p.left.insert(slot);
        if p.pending.is_empty() {
            self.played_end(pl);
        }
    }

    /// A human whose match the core ended because its referee's app left before its result: no result is waited for
    /// from that slot, and it is not a leaver.
    pub(crate) fn played_spared(&mut self, pl: PlayedId, slot: i32) {
        let Some(p) = self.played.get_mut(&pl) else { return };
        if p.done || !p.pending.shift_remove(&slot) {
            return;
        }
        if p.pending.is_empty() {
            self.played_end(pl);
        }
    }

    /// The record is built and goes to each of its humans still linked, with that human's share.
    /// A panic of the core here is a bug and is not caught.
    pub(crate) fn played_end(&mut self, pl: PlayedId) {
        let Some(p) = self.played.get_mut(&pl) else { return };
        if p.done {
            return;
        }
        p.done = true;
        self.recording.shift_remove(&pl);
        let p = &self.played[&pl];
        if p.reports.is_empty() && !p.forfeit {
            return;
        }
        let agreed = agree(&p.reports);
        let ended_at = self.rand.wall_ms();
        let p = &self.played[&pl];
        let record = build_record(&p.facts, &agreed, ended_at);
        let trackers = match_trackers(&record, agreed.report.as_ref(), &p.left);
        let (game, left_slots) = (p.game, p.left.clone());
        let names: Vec<&str> = record.players.iter().map(|x| x.name.as_str()).collect();
        let line = format!(
            "match recorded {} {} team {} {}",
            if record.custom { "custom".to_string() } else { format!("game {game}") },
            if record.checked { "checked" } else { "unchecked" },
            record.winning_team,
            names.join(" / ")
        );
        self.log(&line);
        for x in &record.players {
            let c = if !x.bot && !x.id.is_empty() { self.linked(&x.id) } else { None };
            let Some(c) = c else { continue };
            let left = left_slots.contains(&x.slot);
            // (a leaver reached no podium: it toasts no one)
            self.cm(c).toastable = if left {
                IndexSet::new()
            } else {
                record
                    .players
                    .iter()
                    .filter(|q| !q.bot && !q.id.is_empty() && q.id != x.id)
                    .map(|q| q.id.clone())
                    .collect()
            };
            // (ShouldSkipRewards: a custom match, Context 3, writes no lifetime count)
            let msg = ServerMessage::MatchRecord {
                record: record.clone(),
                game,
                totals: !record.custom && !left,
                trackers: trackers.get(&x.id).cloned(),
                left: left.then_some(true),
            };
            self.send(c, &msg);
        }
    }

    // ---------- the set: the result, the vote, the rematch (queue matches only; UI spec 31 5.4, PodiumGameMode) ----

    /// The match is over for the core (its last peer left): the result is waited for.
    pub(crate) fn match_over(&mut self, pm: PmId) {
        if !self.pm_alive(pm) || self.pm(pm).phase != PmPhase::Match {
            return;
        }
        if self.pm(pm).humans.is_empty() {
            return self.drop_pre_match(pm);
        }
        self.wait_result(pm);
    }

    fn wait_result(&mut self, pm: PmId) {
        if self.pm(pm).result_due {
            return;
        }
        self.pm_mut(pm).result_due = true;
        let ms = self.times.result_ms;
        self.pm_later(pm, ms, PmDue::Result);
    }

    /// (the port's rule) The humans' results: the set takes the team when every human still in it has sent the same
    /// one; a missing one at the wait's end, or two that differ, end the set without a vote and without a win.
    fn settle_result(&mut self, pm: PmId, last: bool) {
        let p = self.pm(pm);
        if p.phase != PmPhase::Match {
            return;
        }
        let got: Vec<i32> = p.humans.iter().filter_map(|&k| p.results.get(&self.c(k).slot).copied()).collect();
        if !last && got.len() < p.humans.len() {
            return;
        }
        let mut teams: Vec<i32> = Vec::new();
        for t in &got {
            if !teams.contains(t) {
                teams.push(*t);
            }
        }
        if got.is_empty() || got.len() < p.humans.len() || teams.len() != 1 {
            return self.set_over(pm, false);
        }
        self.counted(pm, teams[0]);
    }

    /// The agreed result: a win to its team (-1: none, the game counts as played); a decided set (a team at the
    /// mode's set_wins, or its set_games played) ends with no vote (UI_EOG_RematchStatus BPI Start RS@6147), else the
    /// vote's window opens with the bots' votes. The match is detached from the set first: whatever ends it later ends
    /// no vote and no later game.
    fn counted(&mut self, pm: PmId, team: i32) {
        self.clear_timers(pm);
        let p = self.pm_mut(pm);
        if let Ok(t) = usize::try_from(team)
            && t < p.wins.len()
        {
            p.wins[t] += 1;
        }
        for k in self.pm(pm).humans.clone() {
            if let Some(m) = self.c(k).match_id.and_then(|m| self.matches.get_mut(&m))
                && m.pm == Some(pm)
            {
                m.pm = None;
            }
        }
        let p = self.pm(pm);
        // (only a queue's record has a set)
        let mode = p.mode.expect("host core: a set's mode");
        if p.wins.iter().any(|&w| w >= set_wins(mode)) || p.game >= set_games(mode) {
            return self.set_over(pm, false);
        }
        let p = self.pm_mut(pm);
        p.phase = PmPhase::Vote;
        p.votes = vec![None; p.players.len()];
        let ms = self.times.vote_ms;
        self.pm_later(pm, ms, PmDue::VoteOver);
        // (BotsVoteForRematch PGM@947: from Delay 2.0, PGM@6841, slot by slot with a random 0.25-1.0 s after each, a
        // human's slot too; a bot that replaced a leaver, BotFromDisconnection, declines: here a human's slot whose
        // human left during the match)
        let mut at = VOTE_BOT_MS;
        for slot in 0..self.pm(pm).players.len() {
            let p = self.pm(pm);
            let leaver = p.gone.contains(&i32::try_from(slot).expect("a slot"));
            if p.players[slot].bot || leaver {
                self.pm_later(pm, at, PmDue::BotVote { slot, yes: !leaver });
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (lo, hi) = (VOTE_BOT_GAP_MS[0] as u64, VOTE_BOT_GAP_MS[1] as u64 + 1);
            #[allow(clippy::cast_precision_loss)]
            let gap = self.rand.int(lo, hi) as f64;
            at += gap;
        }
        // (a vote a game sent before the agreed result, its own end seen first, counts at the opening)
        let early: Vec<(i32, bool)> = self.pm_mut(pm).early.drain(..).collect();
        for (slot, yes) in early {
            if !self.pm(pm).gone.contains(&slot) {
                self.cast_vote(pm, slot_index(slot), yes);
            }
        }
    }

    /// A vote, the first one of its slot only (BPI_SetRematchVoteForPlayer PGM@17479): to all (BroadcastRematchVote);
    /// a decline ends the set, all yes starts the rematch (StartRematchIfAllAccepted PGM@18041).
    fn cast_vote(&mut self, pm: PmId, slot: usize, yes: bool) {
        let p = self.pm_mut(pm);
        if !p.alive || p.phase != PmPhase::Vote || p.votes.get(slot).is_none_or(Option::is_some) {
            return;
        }
        p.votes[slot] = Some(yes);
        let all = p.votes.iter().all(|v| *v == Some(true));
        self.tell_humans(pm, &ServerMessage::MatchVotes { slot: u32::try_from(slot).expect("a slot"), yes });
        if !yes {
            self.set_over(pm, false);
        } else if all {
            self.set_over(pm, true);
        }
    }

    /// match.set: the game's outcome to the set's humans (wins by team, game: the games played). All yes: the
    /// rematch's pm.begin rematchMs later (Delay 5.5 PGM@7110, StartPlayAgain's Delay 0.5), its humans busy through the
    /// wait. Else the record ends and its humans are free.
    pub(crate) fn set_over(&mut self, pm: PmId, rematch: bool) {
        let Some(p) = self.pms.get(&pm) else { return };
        if !p.alive || (p.phase != PmPhase::Match && p.phase != PmPhase::Vote) {
            return;
        }
        self.clear_timers(pm);
        let p = self.pm(pm);
        let (wins, game) = (p.wins.clone(), p.game);
        let w: Vec<String> = wins.iter().map(u32::to_string).collect();
        let names: Vec<&str> = p.players.iter().map(|x| x.name.as_str()).collect();
        let line = format!(
            "set {} game {game} {} {}",
            if rematch { "rematch" } else { "over" },
            w.join(":"),
            names.join(" / ")
        );
        self.tell_humans(pm, &ServerMessage::MatchSet { rematch, wins, game });
        self.log(&line);
        if !rematch {
            return self.drop_pre_match(pm);
        }
        let p = self.pm_mut(pm);
        p.phase = PmPhase::Rematch;
        p.begun = true;
        let ms = self.times.rematch_ms;
        self.pm_later(pm, ms, PmDue::Rematch);
    }

    // ---------- the handlers ----------

    /// pm.preview { character, skin }: the sender's focused pick in the pick phase (the game's trailing 0.5 s
    /// debounce): kept as its latest (the pick time's lock) and passed to its human team-mates only.
    pub(crate) fn on_pm_preview(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(pm) = self.c(k).pm else { return };
        let pick = pick_of(m);
        let slot = slot_index(self.c(k).slot);
        let p = self.pm(pm);
        let Some(pick) = pick else { return };
        if p.phase != PmPhase::Pick || p.picks[slot].is_some() {
            return;
        }
        self.pm_mut(pm).previews[slot] = Some(pick.clone());
        self.keep_loadout(pm, slot, &pick, m.get("loadout"));
        let msg = ServerMessage::PmPreview {
            character: pick.character,
            skin: pick.skin,
            slot: u32::try_from(slot).expect("a slot"),
        };
        for x in self.team_mates(pm, slot) {
            self.send(x, &msg);
        }
    }

    /// pm.lock { character, skin }: the sender's lock, once (a later one, or one after the pick time, changes
    /// nothing).
    pub(crate) fn on_pm_lock(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(pm) = self.c(k).pm else { return };
        let Some(pick) = pick_of(m) else { return };
        let slot = slot_index(self.c(k).slot);
        let p = self.pm(pm);
        if p.phase == PmPhase::Pick && p.picks[slot].is_none() {
            self.keep_loadout(pm, slot, &pick, m.get("loadout"));
        }
        self.lock_slot(pm, slot, pick);
    }

    /// pm.perks { perks }: a human's perks in the perk phase, once (a second message changes nothing): pm.ready to
    /// all (OnPlayerLockedPerks). The match starts when every human's are in.
    pub(crate) fn on_pm_perks(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(pm) = self.c(k).pm else { return };
        let slot = slot_index(self.c(k).slot);
        let p = self.pm(pm);
        if p.phase != PmPhase::Perks || p.perks[slot].is_some() {
            return;
        }
        self.pm_mut(pm).perks[slot] = Some(clean_perks(m.get("perks").unwrap_or(&Value::Null)));
        self.tell_humans(pm, &ServerMessage::PmReady { slot: u32::try_from(slot).expect("a slot") });
        let p = self.pm(pm);
        if p.humans.iter().all(|&h| p.perks[slot_index(self.c(h).slot)].is_some()) {
            self.start_pre_match(pm);
        }
    }

    /// pm.leave: out of the queue, or out of a pre-match (it ends for everyone: pm.end) or a queue set (in its vote a
    /// decline), without an answer.
    pub(crate) fn on_pm_leave(&mut self, k: ConnKey) {
        self.leave_queue(k);
        self.release_guests();
        self.leave_pre_match(k);
    }

    /// match.result { team, players?, map? }: the match's winning team (-1: none; one of the set's teams, 0..3 in
    /// FFA), every slot's numbers and the map as the sender's game saw them, once per match: for its record
    /// (played_report) and, in a queue set, for the set.
    pub(crate) fn on_match_result(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let c = self.c(k);
        let slot = c.slot;
        if let Some(mt) = c.match_id {
            self.played_report(mt, slot, m);
        }
        let Some(pm) = self.c(k).pm else { return };
        let p = self.pm(pm);
        let teams = f64::from(u32::try_from(p.wins.len()).expect("a team count"));
        let team = m.get("team").and_then(Value::as_f64).filter(|t| t.fract() == 0.0 && *t >= -1.0 && *t < teams);
        let Some(team) = team else { return };
        if p.phase != PmPhase::Match || p.results.contains_key(&slot) {
            return;
        }
        #[allow(clippy::cast_possible_truncation)]
        let team = team as i32;
        self.pm_mut(pm).results.insert(slot, team);
        self.wait_result(pm);
        self.settle_result(pm, false);
    }

    /// match.vote { yes }: the sender's rematch vote in a queue set, the first one only (BPI_SetRematchVoteForPlayer,
    /// PGM@17479). One sent in the match phase (the sender's game saw the end before the core had every result) is
    /// kept for the vote's opening, the first one only.
    pub(crate) fn on_match_vote(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(pm) = self.c(k).pm else { return };
        let Some(yes) = m.get("yes").and_then(Value::as_bool) else { return };
        let slot = self.c(k).slot;
        if self.pm(pm).phase == PmPhase::Match {
            self.pm_mut(pm).early.entry(slot).or_insert(yes);
        } else {
            self.cast_vote(pm, slot_index(slot), yes);
        }
    }
}
