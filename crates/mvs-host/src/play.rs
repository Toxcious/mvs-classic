//! A match from its start to its end (start to end of a match and the match.* handlers), the chat and
//! the podium's toast, and another player's profile (profile.get, profile.ask, profile.tell).

use mvs_protocol::chat::clean_chat;
use mvs_protocol::match_wire::{START_AHEAD_MS, TICK_MS};
use mvs_protocol::messages::{ChatTo, LeftWhy, PathPeer, ServerMessage};
use mvs_protocol::online::MatchStartMsg;
use mvs_protocol::setup::MatchSetupWire;
use serde_json::{Map, Value, json};

use crate::core::{Core, Due, Match, MatchId, PROFILE_ANSWER_MS, PROFILE_MAX, PmId, PmPhase, too_big, utf16_slice};
use crate::link::{ConnKey, RefereeHuman, RefereeStart};
use crate::lobby::js_len;

/// A leave's reason as the logs and the wire name it.
fn left_text(why: LeftWhy) -> String {
    serde_json::to_value(why).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn slot_u32(slot: i32) -> u32 {
    u32::try_from(slot).expect("host core: a human's slot")
}

impl Core {
    fn match_ref(&self, m: MatchId) -> &Match {
        self.matches.get(&m).unwrap_or_else(|| panic!("host core: no match {m}"))
    }

    fn match_mut(&mut self, m: MatchId) -> &mut Match {
        self.matches.get_mut(&m).unwrap_or_else(|| panic!("host core: no match {m}"))
    }

    /// A match starts on its peers: `match_id` the pre-match's id and `perks` its perks by slot (match.start names the
    /// match; a match with one human is local). `set`: a queue match's record, whose result the match's end waits
    /// for.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_match(
        &mut self,
        peers: &[ConnKey],
        setup: MatchSetupWire,
        usernames: Vec<String>,
        lobby: bool,
        map: String,
        match_id: String,
        perks: Vec<Vec<String>>,
        map_pick: Option<u32>,
        set: Option<PmId>,
    ) -> MatchId {
        let local = peers.len() == 1;
        let count = if setup.team_style == 2 { 4 } else { 2 };
        let teams = setup
            .players
            .iter()
            .enumerate()
            .map(|(slot, p)| p.team.unwrap_or(u32::try_from(slot).expect("a slot") % count))
            .collect();
        self.match_count += 1;
        let id = self.match_count;
        self.matches.insert(
            id,
            Match {
                peers: peers.to_vec(),
                local,
                going: false,
                go_at: 0.0,
                lobby,
                gate: None,
                limit: None,
                pm: set,
                played: None,
                refereed: false,
                teams,
                chosen: None,
                by: None,
            },
        );
        if lobby && let Some(l) = self.lobby.as_mut() {
            l.match_id = Some(id);
        }
        // (a queue set's from its humans' combined draws, a lobby's the host's own)
        let seed = match set.and_then(|p| self.pms.get_mut(&p)).and_then(|p| p.rand.as_mut()) {
            Some(r) => 1 + r.int(0x7fff_ffff - 1),
            None => u32::try_from(self.rand.int(1, 0x7fff_ffff)).expect("below 2^31"),
        };
        for &k in peers {
            let c = self.cm(k);
            c.match_id = Some(id);
            c.loaded = false;
            let msg = MatchStartMsg {
                seed,
                setup: setup.clone(),
                slot: slot_u32(c.slot),
                usernames: usernames.clone(),
                map: map.clone(),
                map_pick,
                local: local.then_some(true),
                match_id: Some(match_id.clone()),
                perks: Some(perks.clone()),
            };
            self.send(k, &ServerMessage::MatchStart(msg));
        }
        let line = format!(
            "match start {} {}",
            usernames.join(" / "),
            if local { "local".to_string() } else { format!("seed {seed}") }
        );
        self.log(&line);
        if local {
            return id;
        }
        // the start gate's time limit (loadMs)
        let ms = self.times.load_ms;
        let gate = self.later(ms, Due::Gate(id), false);
        self.match_mut(id).gate = Some(gate);
        id
    }

    /// The start gate's time is over: the peers whose level is not loaded are out, and the match is ended for the
    /// rest.
    pub(crate) fn gate_over(&mut self, m: MatchId) {
        let Some(mt) = self.matches.get_mut(&m) else { return };
        mt.gate = None;
        if mt.going || mt.peers.is_empty() {
            return;
        }
        let peers = mt.peers.clone();
        let late: Vec<ConnKey> = peers.into_iter().filter(|&p| !self.c(p).loaded).collect();
        for p in late {
            let slot = self.c(p).slot;
            self.send(p, &ServerMessage::MatchLeft { slot, why: LeftWhy::LoadTimeout, tick: 0 });
            self.leave_match(p, LeftWhy::LoadTimeout);
        }
        if !self.match_ref(m).peers.is_empty() {
            self.end_match(m, LeftWhy::LoadTimeout);
        }
    }

    /// A match that ran too long (matchTicks) is ended.
    pub(crate) fn limit_over(&mut self, m: MatchId) {
        let Some(mt) = self.matches.get_mut(&m) else { return };
        mt.limit = None;
        if !mt.peers.is_empty() {
            self.end_match(m, LeftWhy::TooLong);
        }
    }

    pub(crate) fn leave_match(&mut self, k: ConnKey, why: LeftWhy) {
        let Some(m) = self.c(k).match_id else { return };
        let slot = self.c(k).slot;
        self.cm(k).match_id = None;
        let mt = self.match_mut(m);
        mt.peers.retain(|&p| p != k);
        let (empty, lobby) = (mt.peers.is_empty(), mt.lobby);
        if empty
            && lobby
            && let Some(l) = self.lobby.as_mut()
            && l.match_id == Some(m)
        {
            l.match_id = None;
        }
        if empty {
            self.stop_timers(m);
        }
        // (the port's own) the human whose app referees the match leaves it before its result (not for a rematch):
        // nobody rules the others' streams, so the match ends for them, the referee charged as the leaver and the
        // others not (its record is written with no result); one that leaves after its result is any leaver
        let played = self.match_ref(m).played;
        let referee_gone = !empty
            && why != LeftWhy::Rematch
            && self.match_ref(m).refereed
            && self.match_ref(m).by == Some(k)
            && played.and_then(|pl| self.played.get(&pl)).is_none_or(|p| !p.done && p.pending.contains(&slot));
        if referee_gone && let Some(p) = played.and_then(|pl| self.played.get_mut(&pl)) {
            p.forfeit = true;
        }
        if let Some(pl) = played {
            self.played_gone(pl, slot);
        }
        let line = format!("match left {} {}", self.c(k).name, left_text(why));
        self.log(&line);
        // (the referee ends the leaver's stream at the ruled line, alike for every peer)
        let by = self.match_ref(m).by;
        if self.match_ref(m).refereed {
            if empty {
                self.unreferee(m);
            } else if by == Some(k) {
                // (the referee's own leave: nothing to tell its app)
            } else if let Some(b) = by {
                self.send(b, &ServerMessage::RefLeft { r#match: m, slot: slot_u32(slot) });
            } else if let Some(r) = self.referee.as_mut() {
                r.left(m, slot_u32(slot));
            }
        }
        // (match.left's tick is 0: the referee's streams say where the leaver's inputs end)
        for p in self.match_ref(m).peers.clone() {
            self.send(p, &ServerMessage::MatchLeft { slot, why, tick: 0 });
        }
        // (a queue match is over for the core: its result is waited for)
        if empty && let Some(pm) = self.match_ref(m).pm {
            self.match_over(pm);
        }
        if referee_gone {
            self.end_match_spared(m, LeftWhy::Disconnected, true);
        }
    }

    /// (a queue set's human who leaves the match before sending its result has quit: their slot is a leaver's, which
    /// declines the vote, they are free and the others' results decide; a game's leave at a normal end comes after its
    /// result)
    fn quit_match(&mut self, k: ConnKey, why: LeftWhy) {
        let c = self.c(k);
        let quit = c.pm.and_then(|p| self.pms.get(&p).map(|x| (p, x))).is_some_and(|(p, x)| {
            x.phase == PmPhase::Match
                && c.match_id.and_then(|m| self.matches.get(&m)).is_some_and(|m| m.pm == Some(p))
                && !x.results.contains_key(&c.slot)
        });
        self.leave_match(k, why);
        if quit {
            self.leave_pre_match(k);
        }
    }

    fn stop_gate(&mut self, m: MatchId) {
        if let Some(g) = self.match_mut(m).gate.take() {
            self.cancel(g);
        }
    }

    fn stop_timers(&mut self, m: MatchId) {
        self.stop_gate(m);
        if let Some(l) = self.match_mut(m).limit.take() {
            self.cancel(l);
        }
    }

    /// The match is over for its referee (once).
    pub(crate) fn unreferee(&mut self, m: MatchId) {
        let Some(mt) = self.matches.get_mut(&m) else { return };
        if !mt.refereed {
            return;
        }
        mt.refereed = false;
        if let Some(b) = mt.by {
            self.send(b, &ServerMessage::RefEnd { r#match: m });
        } else if let Some(r) = self.referee.as_mut() {
            r.end(m);
        }
    }

    /// The referee's silent peer (MatchReferee): told it is out (its own slot, tick 0: its game stops at once) and out
    /// of the match as a lost link is; the others are told it left.
    pub(crate) fn silent_step(&mut self, m: MatchId, slot: u32) {
        let Some(mt) = self.matches.get(&m) else { return };
        if !mt.refereed {
            return;
        }
        let Some(p) = mt.peers.iter().copied().find(|&p| u32::try_from(self.c(p).slot).ok() == Some(slot)) else {
            return;
        };
        let line = format!("match silent {}", self.c(p).name);
        self.log(&line);
        let s = self.c(p).slot;
        self.send(p, &ServerMessage::MatchLeft { slot: s, why: LeftWhy::Disconnected, tick: 0 });
        self.quit_match(p, LeftWhy::Disconnected);
    }

    /// The referee's away peer (`away`), or one back: the match's other peers are told (match.away), the match goes on.
    /// Deviation: the port's own rule; the build has no way back into a match.
    pub(crate) fn away_step(&mut self, m: MatchId, slot: u32, away: bool) {
        let Some(mt) = self.matches.get(&m) else { return };
        if !mt.refereed {
            return;
        }
        let Some(p) = mt.peers.iter().copied().find(|&p| u32::try_from(self.c(p).slot).ok() == Some(slot)) else {
            return;
        };
        let line = format!("match {} {}", if away { "away" } else { "back" }, self.c(p).name);
        self.log(&line);
        for q in self.match_ref(m).peers.clone() {
            if q != p {
                self.send(q, &ServerMessage::MatchAway { slot, away });
            }
        }
    }

    /// The core ends a match: its peers are told as for a leaver (tick 0: they stop at once), and the match is over for
    /// the core as if each of them had left: no peer is in it and its lobby can start another.
    fn end_match(&mut self, m: MatchId, why: LeftWhy) {
        self.end_match_spared(m, why, false);
    }

    /// end_match; `spared`: its peers are not leavers in its record (their referee's app left it).
    fn end_match_spared(&mut self, m: MatchId, why: LeftWhy, spared: bool) {
        if self.match_ref(m).peers.is_empty() {
            return;
        }
        self.stop_timers(m);
        self.unreferee(m);
        let peers = std::mem::take(&mut self.match_mut(m).peers);
        for &p in &peers {
            self.cm(p).match_id = None;
            let slot = self.c(p).slot;
            if let Some(pl) = self.match_ref(m).played {
                if spared {
                    self.played_spared(pl, slot);
                } else {
                    self.played_gone(pl, slot);
                }
            }
            self.send(p, &ServerMessage::MatchLeft { slot: -1, why, tick: 0 });
        }
        if self.match_ref(m).lobby
            && let Some(l) = self.lobby.as_mut()
            && l.match_id == Some(m)
        {
            l.match_id = None;
        }
        let names: Vec<String> = peers.iter().map(|&p| self.c(p).name.clone()).collect();
        let line = format!("match stopped: {} {}", left_text(why), names.join(" / "));
        self.log(&line);
        // (a match the core ended has no result: its set is over)
        if let Some(pm) = self.match_ref(m).pm {
            self.set_over(pm, false);
        }
    }

    // ---------- the handlers ----------

    /// The start gate: every peer has the level loaded, then all begin on the same message.
    pub(crate) fn on_match_loaded(&mut self, k: ConnKey) {
        // (a game whose match is over before its level was loaded, the gate's time limit among the reasons: told, so
        // that it does not wait for a go)
        let Some(m) = self.c(k).match_id else {
            return self.send(k, &ServerMessage::MatchLeft { slot: -1, why: LeftWhy::LoadTimeout, tick: 0 });
        };
        let mt = self.match_ref(m);
        if mt.going {
            return;
        }
        // (a game from before local matches waits at the gate: it is told, as above)
        if mt.local {
            return self.end_match(m, LeftWhy::LocalMatch);
        }
        self.cm(k).loaded = true;
        if !self.match_ref(m).peers.iter().all(|&p| self.c(p).loaded) {
            return;
        }
        // (the port's own) the human its path chose, still in the match, referees it in its app: no room asked; one
        // out of the match by the go leaves it to the core's referee, as a match not measured
        let by = self.match_ref(m).chosen.clone().filter(|c| self.match_ref(m).peers.contains(&c.key));
        // (the port's own) the referee has no room for one more match (the relay's limit, reached while this one was
        // in its pre-match): it is ended for its peers as a failed start
        if by.is_none() && !self.room_to_go() {
            return self.end_match(m, LeftWhy::Error);
        }
        let now = self.now;
        let mt = self.match_mut(m);
        mt.going = true;
        mt.go_at = now;
        self.stop_gate(m);
        // (the port's own) tick 0 is START_AHEAD_MS after the go: each game holds its first frame for that less its
        // one-way time, so the peers start together; from there their time sync keeps them level.
        // Deviation, the port's own limit: a match is played for matchTicks (MAX_MATCH_TICKS, 2 hours) and no longer.
        // A match ends by its rules, but only on games that run: games left open and idle would play it for good. Its
        // peers are told and the match is over for the core (end_match).
        let ms = START_AHEAD_MS + f64::from(self.times.match_ticks) * TICK_MS;
        let limit = self.later(ms, Due::Limit(m), false);
        self.match_mut(m).limit = Some(limit);
        let peers = self.match_ref(m).peers.clone();
        let top = peers.iter().map(|&p| self.c(p).slot).max().unwrap_or(-1);
        let mut ids = vec![String::new(); usize::try_from(top + 1).unwrap_or(0)];
        for &p in &peers {
            let c = self.c(p);
            ids[usize::try_from(c.slot).expect("a slot")].clone_from(&c.id);
        }
        // (the referee's clock is the core's: tick 0 START_AHEAD_MS after the go; it names the id the peers send to)
        let mut referee = self.host.clone();
        // (the set-up's slots, its bots' among them: one team each)
        let slots = u32::try_from(self.match_ref(m).teams.len()).expect("a match's slot count");
        if let Some(chosen) = by {
            let b = chosen.key;
            let mt = self.match_mut(m);
            mt.refereed = true;
            mt.by = Some(b);
            let humans: Vec<PathPeer> = peers
                .iter()
                .map(|&p| {
                    let epic = chosen.epics.iter().find(|(k, _)| *k == p).map(|(_, e)| e.clone()).unwrap_or_default();
                    PathPeer { slot: slot_u32(self.c(p).slot), epic }
                })
                .collect();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let in_ms = START_AHEAD_MS as u32;
            let line = format!("match go refereed by {}", self.c(b).name);
            self.log(&line);
            self.send(b, &ServerMessage::RefGo { r#match: m, in_ms, humans, slots });
            referee = chosen.epic;
        } else if self.referee.is_some() {
            self.match_mut(m).refereed = true;
            let humans =
                peers.iter().map(|&p| RefereeHuman { slot: slot_u32(self.c(p).slot), id: self.c(p).id.clone() });
            let start = RefereeStart { match_id: m, slots, humans: humans.collect(), tick0: now + START_AHEAD_MS };
            referee = self.referee.as_mut().expect("checked").start(&start, now);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let in_ms = START_AHEAD_MS as u32;
        let msg = ServerMessage::MatchGo { in_ms, peers: ids, referee };
        for p in peers {
            self.send(p, &msg);
        }
    }

    /// The reporter's build id is in its message; each peer's app version is the core's record of its hello.
    /// Deviation: the line holds the message's text as it came (a parsed and written message gives
    /// the same text for a game's message).
    pub(crate) fn on_match_desync(&mut self, k: ConnKey, data: &str) {
        let c = self.c(k);
        let peers = c.match_id.map(|m| self.match_ref(m).peers.clone()).unwrap_or_default();
        let apps: Vec<String> = peers
            .iter()
            .map(|&p| {
                let x = self.c(p);
                format!("{}:{}", x.slot, if x.app.is_empty() { "(none)" } else { &x.app })
            })
            .collect();
        let line = format!("DESYNC reported by {} {} apps {}", c.name, utf16_slice(data, 300), apps.join(" "));
        let slot = slot_u32(c.slot);
        self.log(&line);
        for p in peers {
            if p != k {
                self.send(p, &ServerMessage::MatchDesync { slot });
            }
        }
    }

    pub(crate) fn on_match_leave(&mut self, k: ConnKey) {
        self.quit_match(k, LeftWhy::Left);
    }

    /// The player-refereed match a link is in, going: its id and its referee's link.
    fn by_match(&self, k: ConnKey) -> Option<(MatchId, ConnKey)> {
        let m = self.c(k).match_id?;
        let mt = self.matches.get(&m).filter(|mt| mt.going && mt.refereed)?;
        Some((m, mt.by?))
    }

    /// ref.news { match, slot, news }: a slot's news from the app that referees the sender's match (away, back, gone),
    /// taken as the core's referee's. From any other link it is dropped. The match is the sender's own: its `match`
    /// field is not read (a game knows the match by the core's messages, which name no number to it).
    pub(crate) fn on_ref_news(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some((mid, by)) = self.by_match(k) else { return };
        let Some(slot) = m.get("slot").and_then(Value::as_u64).and_then(|s| u32::try_from(s).ok()) else { return };
        if by != k {
            return;
        }
        match m.get("news").and_then(Value::as_str) {
            Some("away") => self.away_step(mid, slot, true),
            Some("back") => self.away_step(mid, slot, false),
            Some("gone") => self.silent_step(mid, slot),
            _ => {}
        }
    }

    /// ref.lost: a game of a player-refereed match has had no ruled packet for RESUME_MS: the referee's slot is gone
    /// (silent_step), which ends the match. From the referee's own link, or one in no such match, it is dropped. The
    /// match is the sender's own: the message's `match` field is not read (a game that does not referee is told no
    /// match number).
    // Unverified: a ref.lost cannot be checked: a modified game that sends a false one gets the referee charged as the
    // leaver and the match ended with no win.
    pub(crate) fn on_ref_lost(&mut self, k: ConnKey) {
        let Some((mid, by)) = self.by_match(k) else { return };
        if by == k {
            return;
        }
        let line = format!("match referee lost {} (told by {})", self.c(by).name, self.c(k).name);
        self.log(&line);
        let slot = slot_u32(self.c(by).slot);
        self.silent_step(mid, slot);
    }

    /// (the port's own) A chat message: to a going match's other peers (its team's only, unless all), or to the other
    /// members whose lobby screen is up. Not in a pre-match, the queue or a local match. The text passes cleaned
    /// (clean_chat); over the sender's budget it is dropped.
    pub(crate) fn on_chat(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let text = clean_chat(m.get("text").unwrap_or(&Value::Null));
        if text.is_empty() {
            return;
        }
        let all = m.get("all") == Some(&Value::Bool(true));
        let c = self.c(k);
        let to: Vec<ConnKey>;
        let mut lobby_chat = false;
        if let Some(mid) = c.match_id {
            let mt = self.match_ref(mid);
            if !mt.going || mt.local {
                return;
            }
            let team = |s: i32| usize::try_from(s).ok().and_then(|s| mt.teams.get(s).copied());
            to =
                mt.peers.iter().copied().filter(|&p| p != k && (all || team(self.c(p).slot) == team(c.slot))).collect();
        } else if let Some(l) =
            self.lobby.as_ref().filter(|l| self.custom && c.in_lobby && c.pm.is_none() && l.up.contains(&c.id))
        {
            to = l.members.values().copied().filter(|&p| p != k && l.up.contains(&self.c(p).id)).collect();
            lobby_chat = true;
        } else {
            return;
        }
        let now = self.now;
        if !self.cm(k).chat.allow(now) {
            return;
        }
        let from = self.c(k).id.clone();
        let to_who = if lobby_chat {
            ChatTo::Lobby
        } else if all {
            ChatTo::All
        } else {
            ChatTo::Team
        };
        let msg = ServerMessage::Chat { from, to: to_who, text };
        for p in to {
            self.send(p, &msg);
        }
    }

    /// toast { to }: a podium toast (the build's GameProfileManager.SendToast, PandaGameInstance BPI_Toast) for another
    /// human of the sender's last match.record, once per pair per record: passed on to them as toast { from, name }
    /// when they are linked. Anything else is dropped without an answer (the build's OnToastFinished does nothing).
    /// The pairs are its budget: at most three per record, under the link's text budget.
    pub(crate) fn on_toast(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(to) = m.get("to").and_then(Value::as_str) else { return };
        let c = self.cm(k);
        if !c.named || !c.toastable.shift_remove(to) {
            return;
        }
        let msg = ServerMessage::Toast { from: c.id.clone(), name: c.name.clone() };
        if let Some(t) = self.linked(to) {
            self.send(t, &msg);
        }
    }

    // ---------- another player's profile (profile.get, profile.ask, profile.tell) ----------
    // The core asks the player's own game and passes its answer on: what that game says (self-reported). It reads none
    // of a profile's numbers. Deviation: the build's GetPlayerProfile asked Hydra, which kept every account's profile.

    /// An answer to an asker still linked.
    /// Deviation: the profile passes as the asked game sent it, written out again (its keys in another order).
    fn answer_profile(&mut self, asker: &str, id: &str, profile: Value) {
        if let Some(c) = self.linked(asker) {
            let text = json!({ "t": "profile", "id": id, "profile": profile }).to_string();
            self.send_text(c, &text);
        }
    }

    /// A player's link is gone: the asks of theirs are dropped, the asks to them answered with no profile at once.
    pub(crate) fn drop_asks(&mut self, id: &str) {
        let mut cancel = Vec::new();
        self.asks.retain(|_, waits| {
            if let Some(t) = waits.shift_remove(id) {
                cancel.push(t);
            }
            !waits.is_empty()
        });
        for t in cancel {
            self.cancel(t);
        }
        let Some(mine) = self.asks.shift_remove(id) else { return };
        for (asker, t) in mine {
            self.cancel(t);
            self.answer_profile(&asker, id, Value::Null);
        }
    }

    /// An ask with no answer in PROFILE_ANSWER_MS: answered with no profile.
    pub(crate) fn profile_timeout(&mut self, asked: &str, asker: &str) {
        let Some(w) = self.asks.get_mut(asked) else { return };
        if w.shift_remove(asker).is_none() {
            return;
        }
        if w.is_empty() {
            self.asks.shift_remove(asked);
        }
        self.answer_profile(asker, asked, Value::Null);
    }

    /// profile.get { id }: another named player of the session is asked (profile.ask) and their answer passed on
    /// (profile.tell); the sender's own id or one no named player of the session has is answered profile { id,
    /// profile: null } at once, as is no answer in PROFILE_ANSWER_MS. An ask already open to that player is not sent
    /// again: its answer is the one. Past the sender's budget: no answer.
    pub(crate) fn on_profile_get(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let now = self.now;
        if !self.c(k).named || !self.cm(k).profile.take(1.0, now) {
            return;
        }
        let Some(id) = m.get("id").and_then(Value::as_str) else { return };
        if js_len(id) > 64 {
            return;
        }
        let asker = self.c(k).id.clone();
        let to = if id == asker { None } else { self.linked(id) };
        let Some(to) = to.filter(|&t| self.c(t).named) else {
            return self.send(k, &ServerMessage::Profile { id: id.to_string(), profile: None });
        };
        if self.asks.get(id).is_some_and(|w| w.contains_key(&asker)) {
            return;
        }
        let t =
            self.later(PROFILE_ANSWER_MS, Due::ProfileAnswer { asked: id.to_string(), asker: asker.clone() }, false);
        self.asks.entry(id.to_string()).or_default().insert(asker.clone(), t);
        self.send(to, &ServerMessage::ProfileAsk { from: asker });
    }

    /// profile.tell { to, profile }: the answer to an open ask of `to`'s, passed on as profile { id: the sender's,
    /// profile }; one that is not an object of at most PROFILE_MAX bytes (or null) is passed on as null. A tell with
    /// no open ask is dropped.
    pub(crate) fn on_profile_tell(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let Some(asker) = m.get("to").and_then(Value::as_str) else { return };
        let id = self.c(k).id.clone();
        let Some(waits) = self.asks.get_mut(&id) else { return };
        let Some(t) = waits.shift_remove(asker) else { return };
        if waits.is_empty() {
            self.asks.shift_remove(&id);
        }
        self.cancel(t);
        // a profile as a tell carries it: an object of at most PROFILE_MAX bytes of JSON, or null
        let profile = match m.get("profile") {
            Some(v @ Value::Object(_)) if !too_big(&v.to_string(), PROFILE_MAX) => v.clone(),
            _ => Value::Null,
        };
        self.answer_profile(asker, &id, profile);
    }
}
