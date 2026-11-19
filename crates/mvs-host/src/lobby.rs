//! The custom lobby's rules and a player's own messages (hello, ping, profile.icon and the lobby.*
//! handlers). The messages are named after the native calls of the build's lobby: "lobby.<call>". Every change goes
//! to all members as "lobby.state" { id, ev, data }: the whole lobby and the event it stands for.

use mvs_protocol::lobby_rules::{
    BOT_DIFFICULTY_EXPERT, BOT_DIFFICULTY_MIN, LobbyPlayer, LobbyRules, OptionKey, OptionValue, bot_name,
};
use mvs_protocol::messages::{HostWhy, LobbyErrorWhy, LobbyRefusedWhy, RefusedWhy, ServerMessage};
use mvs_protocol::names::clean_name;
use mvs_protocol::online::{PmKind, QueuePlayer, SEED_MAX, app_of, compat_note, compat_of};
use mvs_protocol::queue_rules::pick_bots;
use mvs_protocol::roster::CHARACTERS;
use mvs_protocol::setup::{MatchPlayerWire, MatchRulesWire, MatchSetupWire};
use mvs_protocol::version::PROTOCOL;
use serde_json::{Map, Value};

use crate::HostEvent;
use crate::core::{Core, INVITED_MAX, Lobby, RENAME_MS, asset_name, int, map_name, text};
use crate::link::ConnKey;

/// A setting's value as a member sends it: the key's value when the row takes it (optionValue).
fn option_value(key: Option<&Value>, value: Option<&Value>) -> Option<(OptionKey, OptionValue)> {
    let key = match key?.as_str()? {
        "public" => OptionKey::Public,
        "stocks" => OptionKey::Stocks,
        "invitedOnly" => OptionKey::InvitedOnly,
        "mutator" => OptionKey::Mutator,
        _ => return None,
    };
    match key {
        OptionKey::Mutator => {
            let x = value?.as_f64()?;
            // (a listed value: 0..=17)
            (x.fract() == 0.0 && (0.0..=17.0).contains(&x)).then(|| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = x as usize;
                (key, OptionValue::Choice(n))
            })
        }
        _ => value?.as_bool().map(|b| (key, OptionValue::Bool(b))),
    }
}

/// A number in base 36, as JavaScript's toString(36).
pub(crate) fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[usize::try_from(n % 36).expect("below 36")]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).expect("ascii digits")
}

/// The UTF-16 length of a string (JavaScript's .length).
pub(crate) fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A bot's difficulty of a lobby.addBot / lobby.updateSettingsForBot: Very Easy (2) to the port's own Expert (6),
/// Medium (4) when it has none.
fn bot_difficulty(m: &Map<String, Value>) -> i32 {
    i32::try_from(int(m.get("difficulty"), BOT_DIFFICULTY_MIN.into(), BOT_DIFFICULTY_EXPERT.into(), 4))
        .expect("clamped")
}

/// The lobby's match set-up on the wire (LobbyRules.matchSetup's MatchSetupWire).
fn setup_wire(r: &LobbyRules) -> (MatchSetupWire, Vec<String>, Vec<String>) {
    let m = r.match_setup();
    let s = m.setup;
    let players = s
        .players
        .iter()
        .map(|p| MatchPlayerWire {
            bot: p.bot,
            difficulty: p.bot.then_some(p.difficulty),
            character: p.character.clone(),
            perks: Some(Vec::new()),
            ..MatchPlayerWire::default()
        })
        .collect();
    let setup = MatchSetupWire {
        team_style: s.team_style,
        players,
        rules: Some(MatchRulesWire {
            knockouts_to_win: i32::try_from(s.rules.knockouts_to_win).unwrap_or(i32::MAX),
            duration: s.rules.duration,
            stocks: s.rules.stocks.then_some(true),
        }),
        flow: Some(s.flow),
        allow_hazards: Some(s.allow_hazards),
        local_play: None,
        mutators: (!s.mutators.is_empty()).then_some(s.mutators),
    };
    (setup, m.usernames, m.ids)
}

impl Core {
    fn lobby_ref(&self) -> &Lobby {
        self.lobby.as_ref().expect("host core: the session's lobby")
    }

    fn lobby_mut(&mut self) -> &mut Lobby {
        self.lobby.as_mut().expect("host core: the session's lobby")
    }

    /// An id into the lobby's invited set, the newest (at most INVITED_MAX: the oldest goes).
    fn invite(&mut self, id: &str) {
        self.invited.shift_remove(id);
        self.invited.insert(id.to_string());
        if self.invited.len() > INVITED_MAX {
            self.invited.shift_remove_index(0);
        }
    }

    /// A hello's protocol: another than this host's PROTOCOL is answered `refused` with this one's and the link
    /// closed (the game checks the host's first message first, so a game of this repo meets this only when it
    /// skipped that). False: refused.
    fn protocol_fits(&mut self, k: ConnKey, m: &Map<String, Value>) -> bool {
        if m.get("proto").and_then(Value::as_f64) == Some(f64::from(PROTOCOL)) {
            return true;
        }
        self.send(k, &ServerMessage::Refused { why: RefusedWhy::Protocol, server: PROTOCOL });
        self.drop_link(k, HostWhy::Protocol);
        false
    }

    /// A lobby join's compatibility id: kept on the player; none is answered lobby.error with the reason.
    fn take_compat(&mut self, k: ConnKey, m: &Map<String, Value>) -> bool {
        let compat = compat_of(m.get("compat").unwrap_or(&Value::Null));
        if compat.is_empty() {
            self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::CompatMissing, name: None });
            return false;
        }
        self.cm(k).compat = compat;
        true
    }

    /// (the port's rule) Two games of different compatibility ids do not get into one lobby or match: the joiner
    /// gets the note worded by the two app versions (compat_note: the older release is the one to update). False: a
    /// does not fit b.
    fn compat_fits(&mut self, a: ConnKey, b: ConnKey) -> bool {
        let (ca, cb) = (self.c(a), self.c(b));
        if ca.compat == cb.compat {
            return true;
        }
        let why = compat_note(&ca.app, &cb.app).into();
        let line = format!(
            "compat mismatch {} {} {} / {} {} {}",
            ca.name,
            ca.app,
            if ca.compat.is_empty() { "(none)" } else { &ca.compat },
            cb.name,
            cb.app,
            if cb.compat.is_empty() { "(none)" } else { &cb.compat }
        );
        let name = cb.name.clone();
        self.send(a, &ServerMessage::LobbyError { why, name: Some(name) });
        self.note(&line);
        false
    }

    // ---------- the lobby ----------

    /// (the port's own) The leader is ready while their lobby screen is up, whoever the leader is by now; the others
    /// are by their READY button alone (lobby.setReady). `only`: to that member alone.
    pub(crate) fn lobby_state(&mut self, ev: &str, who: Option<LobbyPlayer>, only: Option<ConnKey>) {
        let l = self.lobby_mut();
        let leader = l.rules.leader_id.clone();
        if l.up.contains(&leader) && !l.rules.ready.contains(&leader) {
            l.rules.ready.push(leader);
        }
        let msg = ServerMessage::LobbyState { id: l.id.clone(), ev: ev.to_string(), data: l.rules.data(), who };
        let to: Vec<ConnKey> = match only {
            Some(k) => vec![k],
            None => l.members.values().copied().collect(),
        };
        let text = msg.text();
        for k in to {
            self.send_text(k, &text);
        }
    }

    /// A member's request: the change goes to all members; a request that changed nothing (a full team, a full
    /// lobby, the same settings) is answered to its sender alone, whose screen waits for the echo.
    fn lobby_change(&mut self, k: ConnKey, ev: &str, change: impl FnOnce(&mut LobbyRules)) {
        let l = self.lobby_mut();
        let before = l.rules.data();
        change(&mut l.rules);
        let same = l.rules.data() == before;
        self.lobby_state(ev, None, same.then_some(k));
    }

    /// A member's change of its own place (ready, team): under its change budget, else answered to it alone
    /// unchanged.
    fn own_change(&mut self, k: ConnKey, ev: &str, change: impl FnOnce(&mut LobbyRules)) {
        let now = self.now;
        if self.cm(k).change.take(1.0, now) {
            self.lobby_change(k, ev, change);
        } else {
            self.lobby_state(ev, None, Some(k));
        }
    }

    pub(crate) fn leave_lobby(&mut self, k: ConnKey) {
        if !self.c(k).in_lobby {
            return;
        }
        // (a member who leaves during the lobby's pre-match ends it for the others)
        if self.c(k).pm.is_some_and(|p| self.pms.get(&p).is_some_and(|p| p.lobby)) {
            self.leave_pre_match(k);
        }
        let id = self.c(k).id.clone();
        self.cm(k).in_lobby = false;
        let l = self.lobby_mut();
        l.members.shift_remove(&id);
        l.up.shift_remove(&id);
        l.rules.remove_player(&id);
        if l.members.is_empty() {
            return;
        }
        // OnPlayerLeftCustomGameLobby (and OnNewLeader when the leader went: the members see the new leaderId)
        self.lobby_state("left", None, None);
    }

    fn join_lobby(&mut self, k: ConnKey) -> bool {
        let c = self.c(k);
        let (id, name, icon) = (c.id.clone(), c.name.clone(), c.icon.clone());
        let l = self.lobby_mut();
        if !l.rules.add_player(&id, &name, &icon) {
            return false;
        }
        l.members.insert(id, k);
        self.cm(k).in_lobby = true;
        true
    }

    /// The lobby screen of this member is up (the leader is ready by it, lobby_state).
    fn enter_lobby(&mut self, k: ConnKey) {
        let id = self.c(k).id.clone();
        self.lobby_mut().up.insert(id);
        self.lobby_state("started", None, Some(k));
        let others: Vec<ConnKey> = self.lobby_ref().members.values().copied().filter(|&m| m != k).collect();
        for m in others {
            self.lobby_state("joined", None, Some(m));
        }
    }

    /// A leader's request on a lobby that is not in a match or its pre-match.
    fn leader_of(&self, k: ConnKey) -> bool {
        let c = self.c(k);
        c.in_lobby
            && self.lobby.as_ref().is_some_and(|l| l.rules.leader_id == c.id && l.match_id.is_none() && l.pm.is_none())
    }

    fn pm_kind(&self, k: ConnKey) -> Option<PmKind> {
        self.c(k).pm.and_then(|p| self.pms.get(&p)).map(|p| p.kind)
    }

    /// In the queue or a found match (inQueue).
    pub(crate) fn in_queue(&self, k: ConnKey) -> bool {
        self.c(k).queued.is_some() || self.pm_kind(k) == Some(PmKind::Queue)
    }

    // ---------- the handlers ----------

    /// hello { proto, name, icon, app }: names the player, or takes a new name. Another protocol is refused
    /// (protocol_fits). In a custom session a name is one person's (compared without case); a queue session takes
    /// strangers, whose names no one keeps apart, so two of one name are both let in. A name that is refused changes
    /// nothing: a player who was not named stays so.
    pub(crate) fn on_hello(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.protocol_fits(k, m) {
            return;
        }
        let name = clean_name(m.get("name").and_then(Value::as_str).unwrap_or(""));
        if name.is_empty() {
            return self.send(k, &ServerMessage::NameTaken { name });
        }
        if self.name_refused.refuses(&name) {
            return self.send(k, &ServerMessage::NameRefused { name });
        }
        // (a new name sooner than RENAME_MS after the last: welcome with the name kept, core.rs RENAME_MS)
        let c = self.c(k);
        let renamed = c.named && name != c.name;
        if renamed && self.now - c.renamed_at < RENAME_MS {
            let (id, name) = (c.id.clone(), c.name.clone());
            return self.send(k, &ServerMessage::Welcome { id, name });
        }
        let lower = name.to_lowercase();
        if self.custom
            && self.clients.iter().any(|(&x, o)| x != k && o.listed && o.named && o.name.to_lowercase() == lower)
        {
            return self.send(k, &ServerMessage::NameTaken { name });
        }
        let now = self.now;
        let c = self.cm(k);
        if renamed {
            c.renamed_at = now;
        }
        c.name.clone_from(&name);
        c.icon = asset_name(m.get("icon")).unwrap_or_default();
        c.app = app_of(m.get("app").unwrap_or(&Value::Null));
        c.named = true;
        let (id, icon, in_lobby) = (c.id.clone(), c.icon.clone(), c.in_lobby);
        self.send(k, &ServerMessage::Welcome { id: id.clone(), name: name.clone() });
        // a new name (or icon) shows in the lobby
        if !in_lobby {
            return;
        }
        let Some(me) = self.lobby_mut().rules.find_mut(&id) else { return };
        if me.username != name || me.icon != icon {
            me.username = name;
            me.icon = icon;
            if self.cm(k).change.take(1.0, now) {
                self.lobby_state("teams", None, None);
            }
        }
    }

    pub(crate) fn on_ping(&mut self, k: ConnKey, m: &Map<String, Value>) {
        match m.get("n") {
            Some(n) => self.send(k, &ServerMessage::Pong { n: n.clone() }),
            // (no n sent: none in the answer, as JSON drops an undefined)
            None => self.send_text(k, r#"{"t":"pong"}"#),
        }
    }

    /// The equipped profile icon changed (set_profile_icon in the build): the lobby row shows the new one. No store.
    pub(crate) fn on_profile_icon(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let icon = asset_name(m.get("icon"));
        let c = self.c(k);
        let Some(icon) = icon else { return };
        if !c.named || icon == c.icon {
            return;
        }
        let (id, in_lobby) = (c.id.clone(), c.in_lobby);
        self.cm(k).icon.clone_from(&icon);
        // (past the change budget the row has it, and shows it with the lobby's next change)
        if in_lobby && self.lobby_mut().rules.set_icon(&id, &icon) {
            let now = self.now;
            if self.cm(k).change.take(1.0, now) {
                self.lobby_state("teams", None, None);
            }
        }
    }

    /// BPI_StartCustomGameLobby, and the return to the lobby after a match: the lobby screen is up. The host's player
    /// makes the session's lobby with their first one, its first member and leader.
    pub(crate) fn on_lobby_enter(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.c(k).named || self.pm_kind(k) == Some(PmKind::Lobby) {
            return;
        }
        self.gone_on(k);
        if self.in_queue(k) {
            return self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::QueuedSelf, name: None });
        }
        if !self.take_compat(k, m) {
            return;
        }
        if !self.c(k).in_lobby {
            if !self.custom || self.c(k).id != self.host || self.lobby.is_some() {
                return self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::NoLobby, name: None });
            }
            let id = match self.lobby_id.clone() {
                Some(id) => id,
                None => base36(self.rand.int(1, 0x7fff_ffff)),
            };
            self.lobby = Some(Lobby {
                id,
                rules: LobbyRules::new(),
                members: Default::default(),
                up: Default::default(),
                match_id: None,
                pm: None,
            });
            self.join_lobby(k);
        }
        self.enter_lobby(k);
    }

    /// JoinCustomGameLobby { lobby }: the session's lobby, by its id. Who reaches the core was let in by the link (a
    /// lobby code); a player kicked from it is not let back. (the port's own) While the lobby's invitedOnly option is
    /// on, a joiner who is not a member and whom no member invited is answered lobby.refused and its link closed.
    pub(crate) fn on_lobby_join(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let asked = text(m.get("lobby"), 64);
        let id = self.c(k).id.clone();
        let l = self.lobby.as_ref().is_some_and(|l| asked == l.id) && !self.kicked.contains(&id);
        let c = self.c(k);
        if !c.named || c.match_id.is_some() || self.pm_kind(k) == Some(PmKind::Lobby) {
            return;
        }
        if l {
            let lo = self.lobby_ref();
            if lo.rules.options.invited_only && !lo.members.contains_key(&id) && !self.invited.contains(&id) {
                self.send(k, &ServerMessage::LobbyRefused { why: LobbyRefusedWhy::InvitedOnly });
                return self.drop_link(k, HostWhy::NotInvited);
            }
        }
        self.gone_on(k);
        if self.in_queue(k) {
            return self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::QueuedSelf, name: None });
        }
        if l && self.c(k).in_lobby {
            return self.enter_lobby(k);
        }
        // Deviation: the texts of these two errors are the port's
        if !l {
            return self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::NoLobby, name: None });
        }
        if self.lobby_ref().rules.get_available_player_slot_count() == 0 {
            return self.send(k, &ServerMessage::LobbyError { why: LobbyErrorWhy::LobbyFull, name: None });
        }
        // (the port's own) only games of one compatibility id play together: the joiner is told, worded by the two
        // app versions (compat_note)
        let first = self.lobby_ref().members.values().next().copied();
        if !self.take_compat(k, m) {
            return;
        }
        if let Some(first) = first
            && !self.compat_fits(k, first)
        {
            return;
        }
        self.join_lobby(k);
        self.enter_lobby(k);
    }

    /// (a custom session is the host's lobby: their leave ends it for everyone)
    pub(crate) fn on_lobby_leave(&mut self, k: ConnKey) {
        let c = self.c(k);
        if self.custom && c.id == self.host && c.in_lobby {
            return self.end_session(HostWhy::HostLeft);
        }
        self.leave_lobby(k);
    }

    pub(crate) fn on_lobby_set_team_style(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let r = &mut self.lobby_mut().rules;
        let style = int(m.get("style"), 0, 2, i64::from(r.team_style));
        r.set_team_style(u8::try_from(style).expect("clamped to 0..=2"));
        self.lobby_state("teamStyle", None, None);
    }

    pub(crate) fn on_lobby_set_match_duration(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let r = &mut self.lobby_mut().rules;
        let sec = int(m.get("sec"), 0, 3600, i64::from(r.match_duration));
        r.match_duration = u32::try_from(sec).expect("clamped to 0..=3600");
        self.lobby_state("matchDuration", None, None);
    }

    pub(crate) fn on_lobby_set_ringouts(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let r = &mut self.lobby_mut().rules;
        let n = int(m.get("n"), 1, 99, i64::from(r.num_ringouts_for_win));
        r.num_ringouts_for_win = u32::try_from(n).expect("clamped to 1..=99");
        self.lobby_state("numRingoutsForWin", None, None);
    }

    pub(crate) fn on_lobby_set_hazards(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        self.lobby_mut().rules.allow_hazards = m.get("on") == Some(&Value::Bool(true));
        self.lobby_state("allowHazards", None, None);
    }

    /// SetMaps { maps }: MapData names (a game without a stage for the match's map plays on the default one).
    pub(crate) fn on_lobby_set_maps(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let maps: Vec<String> = match m.get("maps") {
            Some(Value::Array(a)) => a.iter().take(64).filter_map(map_name).collect(),
            _ => Vec::new(),
        };
        self.lobby_mut().rules.set_maps(&maps);
        self.lobby_state("enabledMaps", None, None);
    }

    /// (the port's own) SetOption { key, value }: one of the lobby's settings, as its row allows.
    pub(crate) fn on_lobby_set_option(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let leader = self.leader_of(k);
        let v = option_value(m.get("key"), m.get("value"));
        if !leader {
            return;
        }
        let was = self.lobby_ref().rules.options.invited_only;
        self.lobby_change(k, "options", |r| {
            if let Some((key, value)) = v {
                r.set_option(key, value);
            }
        });
        // (the port's own) invitedOnly turned on: the members of that moment are invited, so a lost link lets them
        // back
        if !was && self.lobby_ref().rules.options.invited_only {
            let ids: Vec<String> = self.lobby_ref().members.keys().cloned().collect();
            for id in ids {
                self.invite(&id);
            }
        }
    }

    pub(crate) fn on_lobby_add_bot(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let character = asset_name(m.get("character"));
        let skin = asset_name(m.get("skin"));
        let team = usize::try_from(int(m.get("team"), 0, 1, 0)).expect("clamped");
        let difficulty = bot_difficulty(m);
        self.lobby_change(k, "teams", |r| {
            if let (Some(c), Some(s)) = (character, skin) {
                r.add_bot(team, &c, &s, difficulty);
            }
        });
    }

    pub(crate) fn on_lobby_update_bot(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let character = asset_name(m.get("character"));
        let skin = asset_name(m.get("skin"));
        let id = text(m.get("id"), 64);
        let difficulty = bot_difficulty(m);
        self.lobby_change(k, "teams", |r| {
            if let (Some(c), Some(s)) = (character, skin) {
                r.update_settings_for_bot(&id, &c, difficulty, &s);
            }
        });
    }

    /// KickPlayer { id }: a bot, or another member (OnKickedFromCustomGameLobby to that one,
    /// OnPlayerKickedFromCustomGameLobby to the rest). (the port's own) Not the host's player: their lobby is the
    /// session.
    pub(crate) fn on_lobby_kick(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let leader = self.leader_of(k);
        let id = text(m.get("id"), 64);
        if !leader || id == self.c(k).id || id == self.host {
            return;
        }
        let l = self.lobby_mut();
        let who = l.rules.remove_player(&id);
        let out = l.members.get(&id).copied();
        l.up.shift_remove(&id);
        if let Some(out) = out {
            self.lobby_mut().members.shift_remove(&id);
            self.cm(out).in_lobby = false;
            self.kicked.insert(id.clone());
            self.invited.shift_remove(&id);
            self.send(out, &ServerMessage::LobbyKicked);
            self.event(HostEvent::Kicked(id));
        }
        match who {
            Some(who) => self.lobby_state("kicked", Some(who), None),
            None => self.lobby_state("teams", None, Some(k)),
        }
    }

    /// (the port's own) lobby.invite { id }: any member invited this id (the invite itself goes over the net
    /// process's link): kept for the invitedOnly option. A kick takes the id off; a member's invite of a kicked id
    /// lets it back (a kick is undone only by a deliberate invite).
    pub(crate) fn on_lobby_invite(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let id = text(m.get("id"), 65);
        if !self.c(k).in_lobby || self.lobby.is_none() || id.is_empty() || js_len(&id) > 64 {
            return;
        }
        self.kicked.shift_remove(&id);
        self.invite(&id);
    }

    pub(crate) fn on_lobby_promote(&mut self, k: ConnKey, m: &Map<String, Value>) {
        if !self.leader_of(k) {
            return;
        }
        let id = text(m.get("id"), 64);
        self.lobby_change(k, "leader", |r| {
            r.promote_new_leader(&id);
        });
    }

    /// (the port's own) SetReady { on }: the READY button of a member who is not the leader and whose lobby screen
    /// is up; not while the lobby's match is on. The sender's button waits for the answer.
    pub(crate) fn on_lobby_set_ready(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let c = self.c(k);
        let Some(l) = self.lobby.as_ref().filter(|_| c.in_lobby) else { return };
        let Some(on) = m.get("on").and_then(Value::as_bool) else { return };
        if l.match_id.is_some() || l.pm.is_some() || !l.up.contains(&c.id) {
            return;
        }
        let id = c.id.clone();
        self.own_change(k, "ready", |r| r.set_ready(&id, on));
    }

    /// JoinTeam { team }: the sender changes teams.
    pub(crate) fn on_lobby_join_team(&mut self, k: ConnKey, m: &Map<String, Value>) {
        let c = self.c(k);
        let Some(l) = self.lobby.as_ref().filter(|_| c.in_lobby) else { return };
        if l.match_id.is_some() || l.pm.is_some() {
            return;
        }
        let id = c.id.clone();
        let team = usize::try_from(int(m.get("team"), 0, 1, 0)).expect("clamped");
        self.own_change(k, "teams", |r| r.join_team(&id, team));
    }

    /// StartMatch: the leader's, with no blocker standing and every member's lobby screen up. The players of the
    /// match are the lobby's in player-index order; each human is the peer of its slot. The match starts after its
    /// pre-match (pm.begin: the build's rematch-started event, PGI_MatchmakingHandler @40271; UI_PreMatch on the
    /// rematch path), whose picks are its fighters. A bot's character is resolved here to one of CHARACTERS (a lobby
    /// row's that the port lacks is drawn from them, as a queue's bots: one pick for every game). The lobby's ready
    /// list and screens are cleared as at a match's start; the lobby stays (a pre-match that ends sends its members
    /// to the main menu).
    pub(crate) fn on_lobby_start(&mut self, k: ConnKey) {
        // (the referee has no room for one more match: the relay's limit)
        if !self.leader_of(k) || !self.room() {
            return self.send(k, &ServerMessage::LobbyStartFailed);
        }
        let l = self.lobby_ref();
        let r = &l.rules;
        if !r.can_start_match() || l.members.keys().any(|id| !r.ready.contains(id)) {
            return self.send(k, &ServerMessage::LobbyStartFailed);
        }
        let (setup, usernames, ids) = setup_wire(r);
        let teams: u32 = if setup.team_style == 2 { 4 } else { 2 };
        let rows: Vec<Option<LobbyPlayer>> = ids.iter().map(|id| r.find(id).cloned()).collect();
        let mut taken: Vec<String> = Vec::new();
        let mut players: Vec<QueuePlayer> = Vec::new();
        for (slot, id) in ids.iter().enumerate() {
            let team = u32::try_from(slot).expect("a lobby's slot") % teams;
            let row = rows[slot].as_ref().filter(|r| r.is_bot);
            let Some(row) = row else {
                players.push(QueuePlayer {
                    id: id.clone(),
                    name: usernames[slot].clone(),
                    team,
                    character: String::new(),
                    skin: String::new(),
                    bot: false,
                    ..QueuePlayer::default()
                });
                continue;
            };
            let mut character = CHARACTERS.iter().find(|&&x| x == row.character).map(|x| (*x).to_string());
            let mut skin = row.skin.clone();
            let mut name = usernames[slot].clone();
            if character.is_none() {
                let pool: Vec<String> = CHARACTERS.iter().map(|x| (*x).to_string()).collect();
                let rand = &mut self.rand;
                let c =
                    pick_bots(&pool, &taken, 1, &mut |n| u32::try_from(rand.int(0, u64::from(n))).expect("below n"))
                        .remove(0);
                skin = String::new();
                name = bot_name(&c);
                character = Some(c);
            }
            let character = character.expect("resolved above");
            taken.push(character.clone());
            let seed = self.draw_seed(None);
            players.push(QueuePlayer {
                id: id.clone(),
                name,
                team,
                character,
                skin,
                bot: true,
                seed: Some(seed),
                ..QueuePlayer::default()
            });
        }
        let l = self.lobby_ref();
        let busy = ids.iter().any(|id| {
            l.members
                .get(id)
                .and_then(|&p| self.clients.get(&p))
                .is_some_and(|c| c.pm.is_some() || c.match_id.is_some())
        });
        if busy {
            return self.send(k, &ServerMessage::LobbyStartFailed);
        }
        let mut peers: Vec<ConnKey> = Vec::new();
        for (slot, id) in ids.iter().enumerate() {
            let Some(&p) = self.lobby_ref().members.get(id) else { continue };
            self.cm(p).slot = i32::try_from(slot).expect("a lobby's slot");
            peers.push(p);
        }
        let l = self.lobby_mut();
        l.rules.ready.clear();
        l.up.clear();
        let maps = l.rules.enabled_maps.clone();
        let mut base = setup;
        for (slot, p) in base.players.iter_mut().enumerate() {
            if players[slot].bot {
                p.character = Some(players[slot].character.clone());
            }
        }
        let team_style = base.team_style;
        // (Deviation, the backend's rule is not in the build: any of the enabled maps, each as likely; told at pm.all,
        // the build's reveal)
        let pm = self.new_pre_match(PmKind::Lobby, None, true, team_style, Some(&maps), players, base, &peers, None);
        self.lobby_mut().pm = Some(pm);
        for &p in &peers {
            self.cm(p).pm = Some(pm);
        }
        self.begin_pre_match(pm);
    }

    /// A seed or a pick: a queue match's from its generator, else the host's own (drawSeed).
    pub(crate) fn draw_seed(&mut self, pm: Option<u32>) -> u32 {
        if let Some(r) = pm.and_then(|p| self.pms.get_mut(&p)).and_then(|p| p.rand.as_mut()) {
            return 1 + r.int(u64::from(SEED_MAX));
        }
        u32::try_from(self.rand.int(1, u64::from(SEED_MAX) + 1)).expect("below SEED_MAX + 1")
    }
}
