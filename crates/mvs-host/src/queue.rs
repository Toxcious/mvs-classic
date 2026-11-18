//! The queue ("the queue"). queue.join puts a player's ticket at the end of each of its modes' queues
//! (the multiqueue: a ticket waits in all it asked for, and the first match found takes it out of every one). A queue
//! makes a match as soon as its complete tickets of one compatibility id make a match's worth of humans (the first ones
//! in that fit); a smaller group of them is filled with bots once the wait of one of its tickets is over. A found match
//! runs its pre-match on the games from queue.found and starts when every human has sent pm.perks, or queueStartMs
//! after queue.found.
//! The session is its host's queue lobby: the others who join it are its guests. A guest waits only while the host
//! does, in the modes the host's join asks for and with the host's compatibility id, so every match the core finds
//! holds the host (the first one in). The relay's open queue has no host: every join is a ticket of its own, and its
//! matches are many at once.
//! (the port's own) Pairing by distance: a group holds tickets whose round trips to the relay (queue.join `rtt`) lie
//! within rtt_band of its first ticket's wait, which widens until any pairs with any; a ticket with none goes with
//! anyone. While the referee has no room for one more match (MatchReferee::room) no group is matched: the tickets
//! wait on.

use indexmap::IndexMap;
use mvs_protocol::loadout::{cosmetics_of, read_loadout};
use mvs_protocol::lobby_rules::{LobbyRules, bot_name};
use mvs_protocol::messages::{QueueWhy, ServerMessage};
use mvs_protocol::online::{PARTY_MAX, PmKind, PmPick, QueueFoundMsg, QueueMode, QueuePlayer, SEED_MAX, compat_of};
use mvs_protocol::queue_rules::{
    PARTY_WAIT_MS, QUEUE_MODES, bot_after_s, pick_bots, queue_setup, queue_size, queue_slots, queue_team,
    queue_team_style, rtt_band, rtt_fits, ticket_max,
};
use mvs_protocol::roster::{CHARACTERS, DEFAULT_CHARACTER};
use serde_json::{Map, Value};

use crate::HostEvent;
use crate::core::{Core, Due, PmDue, Queued, SlotLoadout, Ticket, TicketId, asset_name, mode_index};
use crate::draws::{CombinedDraws, DRAW_LIMIT, draw_of};
use crate::link::ConnKey;
use crate::lobby::js_len;

/// A mode's waiting tickets by compatibility id (compat_lists).
type CompatLists = IndexMap<String, Vec<TicketId>>;

/// queue.join `party` (QueueParty): its members, the sender's id among them, each an id of 1 to 64 characters, none
/// twice, at most PARTY_MAX; [id] for a join with none; None when it is not one (partyOf).
fn party_of(v: Option<&Value>, id: &str) -> Option<Vec<String>> {
    let Some(v) = v else { return Some(vec![id.to_string()]) };
    let members = v.as_object().and_then(|o| o.get("members")).and_then(Value::as_array)?;
    if members.is_empty() || members.len() > PARTY_MAX {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for x in members {
        let s = x.as_str().filter(|s| !s.is_empty() && js_len(s) <= 64)?;
        if out.iter().any(|o| o == s) {
            return None;
        }
        out.push(s.to_string());
    }
    out.iter().any(|x| x == id).then_some(out)
}

impl Core {
    fn ticket(&self, t: TicketId) -> &Ticket {
        self.ticket_list.get(&t).unwrap_or_else(|| panic!("host core: no ticket {t}"))
    }

    fn ticket_mut(&mut self, t: TicketId) -> &mut Ticket {
        self.ticket_list.get_mut(&t).unwrap_or_else(|| panic!("host core: no ticket {t}"))
    }

    fn complete(&self, t: TicketId) -> bool {
        let t = self.ticket(t);
        t.joined.len() == t.members.len()
    }

    fn host_queued(&self) -> bool {
        self.host_client().is_some_and(|h| self.c(h).queued.is_some())
    }

    fn queue_off(&mut self, k: ConnKey, why: QueueWhy) {
        self.send(k, &ServerMessage::QueueState { on: false, why: Some(why), name: None });
    }

    /// Out of the queue (a cancel, pm.leave, a lost link): the player's whole ticket. Its other members are told
    /// queue.state off Party (their party is not whole: their net processes take it from there); while the host waits
    /// on, its modes' queues are looked at again (with the host out, release_guests lets every guest go).
    pub(crate) fn leave_queue(&mut self, k: ConnKey) {
        let Some(t) = self.c(k).queued.as_ref().map(|q| q.ticket) else { return };
        self.drop_ticket(t);
        for x in self.ticket(t).joined.clone() {
            if x != k {
                self.queue_off(x, QueueWhy::Party);
            }
        }
        if self.open || self.host_queued() {
            for mode in self.ticket(t).modes.clone() {
                self.match_queue(mode);
            }
        }
    }

    /// A ticket out of every queue, its members no longer queued: nobody is told.
    fn drop_ticket(&mut self, t: TicketId) {
        if let Some(late) = self.ticket_mut(t).late.take() {
            self.cancel(late);
        }
        let key = self.ticket(t).key.clone();
        if self.tickets.get(&key) == Some(&t) {
            self.tickets.shift_remove(&key);
        }
        for mode in self.ticket(t).modes.clone() {
            self.waiting[mode_index(mode)].retain(|&x| x != t);
        }
        for x in self.ticket(t).joined.clone() {
            if let Some(c) = self.clients.get_mut(&x) {
                c.queued = None;
            }
        }
    }

    /// Every ticket out of the queues, each member told queue.state off with `why`.
    pub(crate) fn drop_all(&mut self, why: QueueWhy) {
        let mut all: Vec<TicketId> = Vec::new();
        for mode in QUEUE_MODES {
            for &t in &self.waiting[mode_index(mode)] {
                if !all.contains(&t) {
                    all.push(t);
                }
            }
        }
        for t in all {
            self.drop_ticket(t);
            for c in self.ticket(t).joined.clone() {
                self.queue_off(c, why);
            }
        }
    }

    /// (the port's rule) The host waits no more (a cancel, pm.leave, a found match): every guest still waiting is
    /// taken out and told queue.state off, 'error', which sends its net process on to search again.
    pub(crate) fn release_guests(&mut self) {
        if self.open || self.host_queued() {
            return;
        }
        self.drop_all(QueueWhy::Error);
    }

    /// A ticket whose members' joins did not all come within PARTY_WAIT_MS: out, those in told PartyLate; its modes'
    /// queues are looked at again.
    pub(crate) fn party_late(&mut self, t: TicketId) {
        let Some(ticket) = self.ticket_list.get_mut(&t) else { return };
        ticket.late = None;
        if self.complete(t) {
            return;
        }
        let modes = self.ticket(t).modes.clone();
        self.drop_ticket(t);
        for c in self.ticket(t).joined.clone() {
            self.queue_off(c, QueueWhy::PartyLate);
        }
        self.release_guests();
        for mode in modes {
            self.match_queue(mode);
        }
    }

    /// A group of a mode with this complete ticket: the complete waiting tickets of its compatibility id, whole, in
    /// the order they joined, each one taken that fits with the ones before (queue_slots) and whose round trip lies
    /// within rtt_band of this one's wait with theirs (rtt_fits), this one first. A ticket still waiting for a member
    /// is in none.
    fn group_of(&self, mode: QueueMode, first: TicketId, lists: &CompatLists) -> (Vec<TicketId>, usize) {
        let compat = &self.c(self.ticket(first).joined[0]).compat;
        let order = lists.get(compat).map_or(&[][..], Vec::as_slice);
        let band = rtt_band(self.now - self.ticket(first).since);
        let mut rtts = self.ticket(first).rtt.map(|r| (r, r));
        let mut taken = vec![first];
        // (a full group takes no more, and a ticket past the match's size fits none: the queue's many tickets cost
        // no slot packing each)
        let size = queue_size(mode);
        let mut humans = self.ticket(first).members.len();
        for &t in order {
            if humans >= size {
                break;
            }
            if t == first || !self.complete(t) || self.c(self.ticket(t).joined[0]).compat != *compat {
                continue;
            }
            let rtt = self.ticket(t).rtt;
            let n = self.ticket(t).members.len();
            if humans + n > size || !rtt_fits(rtts, rtt, band) {
                continue;
            }
            let sizes: Vec<u32> = taken
                .iter()
                .chain(std::iter::once(&t))
                .map(|&x| u32::try_from(self.ticket(x).members.len()).expect("a party's size"))
                .collect();
            if queue_slots(mode, &sizes).is_some() {
                taken.push(t);
                humans += n;
                if let Some(r) = rtt {
                    rtts = Some(rtts.map_or((r, r), |(lo, hi)| (lo.min(r), hi.max(r))));
                }
            }
        }
        let at = |t: &TicketId| order.iter().position(|x| x == t).map_or(-1, |i| i64::try_from(i).expect("an index"));
        taken.sort_by_key(at);
        (taken, humans)
    }

    /// A mode's waiting tickets by their compatibility id, each list in the queue's order: a group is looked for among
    /// its first ticket's alone, so that a queue split by game versions costs each one its own tickets only.
    fn compat_lists(&self, mode: QueueMode) -> CompatLists {
        let mut out = CompatLists::new();
        for &t in &self.waiting[mode_index(mode)] {
            let compat = &self.c(self.ticket(t).joined[0]).compat;
            match out.get_mut(compat) {
                Some(l) => l.push(t),
                None => {
                    out.insert(compat.clone(), vec![t]);
                }
            }
        }
        out
    }

    /// Every match the queue holds the humans for (none while the referee has no room).
    fn match_queue(&mut self, mode: QueueMode) {
        let mut i = 0;
        let mut lists = self.compat_lists(mode);
        while i < self.waiting[mode_index(mode)].len() && self.room() {
            let first = self.waiting[mode_index(mode)][i];
            let group = self.complete(first).then(|| self.group_of(mode, first, &lists));
            match group {
                Some((tickets, humans)) if humans >= queue_size(mode) => {
                    self.found(mode, &tickets);
                    lists = self.compat_lists(mode);
                }
                _ => i += 1,
            }
        }
    }

    /// The bot fill (the port's rule): a group one of whose tickets has waited out its own wait gets bots in the
    /// places left; it takes every ticket of its compatibility id that fits at that moment (a later joiner does not
    /// restart the first one's clock). The wait that ends first counts, not the first one in's: a guest's join counts
    /// the time it queued under another host (queue.join `waited`), so the one who has queued longest may have joined
    /// last. A ticket's wait is its first member's. A group of a match's worth of humans is matched here too (the
    /// port's own): the distance's band widens with the wait, and the referee's room comes back.
    /// A panic of the core here is a bug and is not caught.
    pub(crate) fn fill_queues(&mut self) {
        let now = self.now;
        for mode in QUEUE_MODES {
            let mut i = 0;
            let mut lists = self.compat_lists(mode);
            while i < self.waiting[mode_index(mode)].len() && self.room() {
                let first = self.waiting[mode_index(mode)][i];
                let group = self.complete(first).then(|| self.group_of(mode, first, &lists));
                match group {
                    Some((tickets, humans))
                        if humans >= queue_size(mode) || tickets.iter().any(|&t| now >= self.ticket(t).due) =>
                    {
                        self.found(mode, &tickets);
                        lists = self.compat_lists(mode);
                    }
                    _ => i += 1,
                }
            }
        }
    }

    /// A match is found for these tickets (in the order they joined): their humans take the slots queue_slots gives
    /// them, bots the rest. Its pre-match runs on the games from queue.found (Match Found, VS, perks; the picks were
    /// made before the join), so the record enters at phase perks.
    fn found(&mut self, mode: QueueMode, group: &[TicketId]) {
        let sizes: Vec<u32> =
            group.iter().map(|&t| u32::try_from(self.ticket(t).joined.len()).expect("a party's size")).collect();
        let slots = queue_slots(mode, &sizes).expect("host core: a found group fits its mode");
        let joined: Vec<ConnKey> = group.iter().flat_map(|&t| self.ticket(t).joined.clone()).collect();
        let mut slot_of: Vec<(ConnKey, usize)> = Vec::new();
        for (i, &t) in group.iter().enumerate() {
            for (k, &c) in self.ticket(t).joined.iter().enumerate() {
                slot_of.push((c, slots[i][k]));
            }
        }
        let slot = |c: ConnKey| slot_of.iter().find(|x| x.0 == c).expect("a found human's slot").1;
        let mut humans = joined.clone();
        humans.sort_by_key(|&c| slot(c));
        let size = queue_size(mode);
        let mut players: Vec<Option<QueuePlayer>> = vec![None; size];
        let mut loadouts: Vec<Option<SlotLoadout>> = vec![None; size];
        for &k in &humans {
            let (c, s) = (self.c(k), slot(k));
            let q = c.queued.as_ref().expect("a found human is queued");
            players[s] = Some(QueuePlayer {
                id: c.id.clone(),
                name: c.name.clone(),
                team: u32::try_from(queue_team(mode, s)).expect("a team"),
                character: q.character.clone(),
                skin: q.skin.clone(),
                bot: false,
                cosmetics: q.loadout.as_ref().map(cosmetics_of),
                ..QueuePlayer::default()
            });
            loadouts[s] = q.loadout.clone().map(|loadout| SlotLoadout { character: q.character.clone(), loadout });
        }
        // (the port's rule) every random choice of the match and its set from every human's draw (in the order they
        // joined), in this order: the bots' characters, their seeds, the map pick, then each game's match seed
        // (start_match)
        let draws: Vec<u32> = joined.iter().map(|&c| self.c(c).queued.as_ref().expect("queued").draw).collect();
        let mut rand = CombinedDraws::new(&draws);
        let picks: Vec<String> =
            humans.iter().map(|&c| self.c(c).queued.as_ref().expect("queued").character.clone()).collect();
        for &t in group {
            self.drop_ticket(t);
        }
        let pool: Vec<String> = CHARACTERS.iter().map(|x| (*x).to_string()).collect();
        let bots = pick_bots(&pool, &picks, size - humans.len(), &mut |n| rand.int(u64::from(n)));
        // Unverified: a queue bot's skin is the default (''): the build's queue bots come from Hydra matchmaking,
        // whose skin choice no Blueprint of the build shows (the pre-match AI's SelectSkin, ShaggyCharacterSelectAI,
        // is Shaggy's own)
        // (a queue bot's perks are the full unlock of its character, queueBotPerks: each game takes them from its own
        // recommended perks, as its cosmetics from the bot's seed)
        let mut k = 0;
        for (s, p) in players.iter_mut().enumerate() {
            if p.is_some() {
                continue;
            }
            let character = bots[k].clone();
            k += 1;
            let seed = 1 + rand.int(u64::from(SEED_MAX));
            *p = Some(QueuePlayer {
                id: format!("bot{k}"),
                name: bot_name(&character),
                team: u32::try_from(queue_team(mode, s)).expect("a team"),
                character,
                skin: String::new(),
                bot: true,
                seed: Some(seed),
                ..QueuePlayer::default()
            });
        }
        let all: Vec<QueuePlayer> = players.into_iter().map(|p| p.expect("every slot filled")).collect();
        // (the port's rule: one of the mode's unranked rotation maps, each as likely, by a pick each game takes from
        // its pool; the lobby's time, the mode's ring-outs: queue_setup)
        let setup = queue_setup(mode, &all, LobbyRules::new().match_duration);
        let pm = self.new_pre_match(
            PmKind::Queue,
            Some(mode),
            false,
            queue_team_style(mode),
            None,
            all.clone(),
            setup,
            &humans,
            Some(rand),
        );
        let p = self.pms.get_mut(&pm).expect("the new pre-match");
        p.phase = crate::core::PmPhase::Perks;
        p.picks = all.iter().map(|x| Some(PmPick { character: x.character.clone(), skin: x.skin.clone() })).collect();
        p.loadouts = loadouts;
        let (id, map_pick) = (p.id.clone(), p.map_pick);
        let ms = self.times.queue_start_ms;
        self.pm_later(pm, ms, PmDue::Start);
        for &c in &humans {
            let s = slot(c);
            let cl = self.cm(c);
            cl.pm = Some(pm);
            cl.slot = i32::try_from(s).expect("a slot");
            let msg = QueueFoundMsg {
                match_id: id.clone(),
                mode,
                map_pick,
                players: all.clone(),
                me: u32::try_from(s).expect("a slot"),
            };
            self.send(c, &ServerMessage::QueueFound(msg));
        }
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        let line = format!("queue found {} {}", mode.id(), names.join(" / "));
        self.log(&line);
        self.path_begin(pm);
        self.release_guests();
        let ids = humans.iter().map(|&c| self.c(c).id.clone()).collect();
        self.event(HostEvent::Found(ids));
    }

    // ---------- the handlers ----------

    /// queue.join { modes (or mode), character, skin, botAfter?, draw?, waited?, party? }: answered queue.state
    /// { on: true } (searching), or off with 'error' when it is refused: not a queue session, no name yet, in a match,
    /// no mode, a mode or a name that is not one, a character the port does not have (CHARACTERS; '' is the default
    /// one, so that no bot plays the fighter the human plays), past the join budget, or a guest's with no mode left
    /// once cut to the host's (the host not queued among them). A party list that is not one, or one too large for a
    /// mode it names (1v1), is answered off Party; so is a member's join with no mode in common with its ticket's (the
    /// ticket waits on). A player already in the queue gets its state again and nothing changes; one in a found match
    /// is answered off, 'error', and the found match stands.
    pub(crate) fn on_queue_join(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if self.c(k).queued.is_some() {
            return self.send(k, &ServerMessage::QueueState { on: true, why: None, name: None });
        }
        self.gone_on(k);
        if self.c(k).pm.is_some() {
            return self.queue_off(k, QueueWhy::Error);
        }
        let asks: Vec<&Value> = match m.get("modes") {
            Some(Value::Array(a)) => a.iter().collect(),
            _ => vec![m.get("mode").unwrap_or(&Value::Null)],
        };
        let mut modes: Vec<QueueMode> =
            QUEUE_MODES.into_iter().filter(|x| asks.iter().any(|a| a.as_str() == Some(x.id()))).collect();
        let asked = asset_name(m.get("character"));
        let skin = asset_name(m.get("skin"));
        let character = match asked.as_deref() {
            Some("") => Some(DEFAULT_CHARACTER.to_string()),
            Some(a) => CHARACTERS.iter().find(|&&x| x == a).map(|x| (*x).to_string()),
            None => None,
        };
        // (the port's own) no compatibility id: the core cannot tell whom this game may meet, and says so
        let compat = compat_of(m.get("compat").unwrap_or(&Value::Null));
        if compat.is_empty() {
            return self.queue_off(k, QueueWhy::Compat);
        }
        self.cm(k).compat.clone_from(&compat);
        let now = self.now;
        let refused = self.custom || !self.cm(k).join.take(1.0, now) || {
            let c = self.c(k);
            !c.named
                || modes.is_empty()
                || modes.len() != asks.len()
                || character.is_none()
                || skin.is_none()
                || c.in_lobby
                || c.match_id.is_some()
        };
        if refused {
            return self.queue_off(k, QueueWhy::Error);
        }
        let (character, skin) = (character.expect("checked"), skin.expect("checked"));
        let id = self.c(k).id.clone();
        let Some(members) = party_of(m.get("party"), &id).filter(|ms| modes.iter().all(|&x| ms.len() <= ticket_max(x)))
        else {
            return self.queue_off(k, QueueWhy::Party);
        };
        // a guest's modes are cut to the host's own join's; with none left (the host not queued, no mode in common, or
        // another compatibility id than the host's) it is refused, and its net process searches again
        if !self.open && id != self.host {
            let host = self.host_client().map(|h| self.c(h));
            let hq = host.and_then(|h| h.queued.as_ref().filter(|_| h.compat == compat).map(|q| q.ticket));
            modes = match hq {
                Some(t) => {
                    let hm = &self.ticket(t).modes;
                    modes.into_iter().filter(|x| hm.contains(x)).collect()
                }
                None => Vec::new(),
            };
            if modes.is_empty() {
                return self.queue_off(k, QueueWhy::Error);
            }
        }
        // (the port's rule) the time queued under another host brings the bot wait forward, never to before now; the
        // join's draw, or the core's for a game that sent none
        // (the open queue has no other host to have queued under)
        let waited =
            m.get("waited").and_then(Value::as_f64).filter(|w| !self.open && w.is_finite() && *w > 0.0).unwrap_or(0.0);
        let draw = match draw_of(m.get("draw").unwrap_or(&Value::Null)) {
            Some(d) => d,
            None => u32::try_from(self.rand.int(0, DRAW_LIMIT)).expect("below 2^32"),
        };
        let loadout = read_loadout(m.get("loadout").unwrap_or(&Value::Null));
        // (the port's own) the game's smallest round trip to the relay; a party's ticket keeps its slowest member's
        let rtt = m.get("rtt").and_then(Value::as_f64).filter(|r| r.is_finite() && *r >= 0.0);
        // (a match's path takes the latest round trip a game sent: its hello's, then its join's)
        if rtt.is_some() {
            self.cm(k).relay_rtt = rtt;
        }
        let key = members.join(" ");
        // a later member's join: into its ticket, whose modes are cut to this join's; the ticket complete, it is
        // matched
        if let Some(&held) = self.tickets.get(&key) {
            let keep: Vec<QueueMode> = self.ticket(held).modes.iter().copied().filter(|x| modes.contains(x)).collect();
            if keep.is_empty() {
                return self.queue_off(k, QueueWhy::Party);
            }
            let cut: Vec<QueueMode> = self.ticket(held).modes.iter().copied().filter(|x| !keep.contains(x)).collect();
            for &x in &cut {
                self.waiting[mode_index(x)].retain(|&t| t != held);
            }
            self.ticket_mut(held).modes.clone_from(&keep);
            if let Some(r) = rtt {
                let tr = &mut self.ticket_mut(held).rtt;
                *tr = Some(tr.map_or(r, |x| x.max(r)));
            }
            for &mode in &cut {
                self.match_queue(mode);
            }
            self.ticket_mut(held).joined.push(k);
            self.cm(k).queued = Some(Queued { ticket: held, character, skin, loadout, draw });
            self.send(k, &ServerMessage::QueueState { on: true, why: None, name: None });
            if !self.complete(held) {
                return;
            }
            if let Some(late) = self.ticket_mut(held).late.take() {
                self.cancel(late);
            }
            for &mode in &keep {
                if self.c(k).queued.is_some() {
                    self.match_queue(mode);
                }
            }
            return;
        }
        // a first join: a new ticket, its wait for the rest of its members from now
        self.ticket_ids += 1;
        let t = self.ticket_ids;
        let after = bot_after_s(m.get("botAfter").unwrap_or(&Value::Null));
        let due = now.max(now + after * 1000.0 - waited);
        let since = now - waited;
        let ticket =
            Ticket { key: key.clone(), members, joined: vec![k], modes: modes.clone(), due, since, rtt, late: None };
        self.ticket_list.insert(t, ticket);
        if !self.complete(t) {
            let late = self.later(PARTY_WAIT_MS, Due::PartyLate(t), false);
            self.ticket_mut(t).late = Some(late);
        }
        self.tickets.insert(key, t);
        self.cm(k).queued = Some(Queued { ticket: t, character, skin, loadout, draw });
        for &mode in &modes {
            self.waiting[mode_index(mode)].push(t);
        }
        self.send(k, &ServerMessage::QueueState { on: true, why: None, name: None });
        for &mode in &modes {
            if self.c(k).queued.is_some() {
                self.match_queue(mode);
            }
        }
    }

    /// queue.cancel: out of the queue, answered queue.state off, 'cancel' (the host's lets its waiting guests go,
    /// release_guests). A found match is not cancelled (the build's CanBack is false from Match Found on): a cancel
    /// after queue.found is ignored, without an answer.
    pub(crate) fn on_queue_cancel(&mut self, k: ConnKey) {
        if self.c(k).pm.is_some() {
            return;
        }
        self.leave_queue(k);
        self.queue_off(k, QueueWhy::Cancel);
        self.release_guests();
    }
}
