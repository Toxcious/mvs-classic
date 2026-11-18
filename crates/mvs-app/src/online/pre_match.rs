//! The game's side of the online pre-match on the rematch path: a custom lobby's start
//! and a queue set's rematch. It stands in for the build's character select manager (UCharacterSelectManager) and its
//! notifications; the server keeps every deadline and its pm.all is the truth.
//!
//! pm.preview / pm.lock / pm.perks / pm.leave to the server, pm.begin / pm.preview / pm.locked / pm.all / pm.ready /
//! match.start / pm.end from it. What arrives while no pre-match screen takes it is kept and handed over in order
//! when one attaches, never dropped. The server sends a bot's seed, not what is drawn from it: the game draws each
//! bot's cosmetics and ring-out at pm.begin and puts them, and the bots' perks, into pm.locked, pm.all and
//! match.start (tables.rs).
//!
//! Deviation: sink calls are `Kept` items the owner drains (`delivered`); its sends are messages the calls
//! return for the owner to send; its timers the owner's `now` (ms) and `pump`.

use mvs_protocol::loadout::NetLoadout;
use mvs_protocol::messages::{ClientMessage, PmEndWhy, ServerMessage};
use mvs_protocol::online::{PlayerCosmetics, PmBegin, PmKind, PmRematch, QueueMode, QueuePlayer};
use mvs_protocol::setup::MatchSetupWire;
use mvs_ui::menu::online::NetPick;

use super::tables::Tables;

/// The build's trailing preview debounce (the character select manager's one-shot 0.5 s timer at mgr +0xf8,
/// restarted by every focus or skin change, cancelled by a lock and by the timer's expiry; real time).
pub const PREVIEW_DEBOUNCE_MS: f64 = 500.0;

/// pm.begin as the game runs it: players[i] is fighter slot i (a bot drawn from its seed), `me` this game's slot.
#[derive(Clone, Debug, PartialEq)]
pub struct PreMatchBegin {
    pub id: String,
    pub kind: PmKind,
    /// A queue's: its pool names the map of the server's mapPick.
    pub mode: Option<QueueMode>,
    pub team_style: u8,
    pub map: String,
    pub players: Vec<QueuePlayer>,
    pub me: usize,
    pub rematch: Option<PmRematch>,
}

/// pm.begin read (None: not one the game can run: no slot of its own).
pub fn read_begin(m: &PmBegin, tables: &Tables) -> Option<PreMatchBegin> {
    let me = m.me as usize;
    if me >= m.players.len() {
        return None;
    }
    Some(PreMatchBegin {
        id: m.id.clone(),
        kind: m.kind,
        mode: if m.kind == PmKind::Queue {
            m.mode.filter(|q| matches!(q, QueueMode::OneVOne | QueueMode::TwoVTwo))
        } else {
            None
        },
        team_style: m.team_style,
        map: m.map.clone(),
        // (a bot's cosmetics and ring-out: drawn here from its seed, a whole number over 0)
        players: m
            .players
            .iter()
            .map(|p| {
                if p.seed.is_some_and(|s| s > 0) {
                    tables.with_bot_draw(p, false)
                } else {
                    QueuePlayer { seed: None, ..p.clone() }
                }
            })
            .collect(),
        me,
        rematch: if m.kind == PmKind::Queue { m.rematch.clone() } else { None },
    })
}

/// The pre-match screen's set-up of an online pre-match: each slot's player
/// by the server's account id (else the slot's own), this game's name for its own slot, a bot's fighter as the server
/// resolved it (a human's none until the pick), the cosmetics the server handed on; `leader` the kept lobby's leader
/// is this game, `set_score` a queue set's rematch's score so far.
pub fn screen_setup(
    b: &PreMatchBegin,
    name: &str,
    leader: bool,
    set_score: Option<(u32, u32)>,
) -> mvs_ui::menu::screens::pre_match::setup::PreMatchSetup {
    use mvs_ui::menu::screens::pre_match::setup::{PreMatchPlayer, PreMatchSetup};
    let players = b
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| PreMatchPlayer {
            account_id: if p.id.is_empty() { format!("slot{i}") } else { p.id.clone() },
            username: if i == b.me && !name.is_empty() { name.to_owned() } else { p.name.clone() },
            team: u8::try_from(p.team).unwrap_or(0),
            is_bot: p.bot,
            character: (p.bot && !p.character.is_empty()).then(|| p.character.clone()),
            skin: if p.bot { p.skin.clone() } else { String::new() },
            cosmetics: p
                .cosmetics
                .as_ref()
                .map(|c| super::tables::loadout_of_cosmetics(c, p.ring_out.as_deref().unwrap_or(""))),
            net_cosmetics: p.cosmetics.clone(),
        })
        .collect();
    PreMatchSetup {
        players,
        me: b.me,
        team_style: b.team_style,
        leader,
        map: (!b.map.is_empty()).then(|| b.map.clone()),
        level: b.map.clone(),
        set_score,
        net: Some(mvs_ui::menu::online::PreMatchNet { queue: None }),
        ..PreMatchSetup::default()
    }
}

/// match.start of the pre-match: the set-up as the server sent it with the bots' tables, each slot's four perks.
#[derive(Clone, Debug, PartialEq)]
pub struct PreMatchStarted {
    pub seed: u32,
    pub setup: MatchSetupWire,
    pub slot: u32,
    pub usernames: Vec<String>,
    pub map: String,
    pub local: bool,
    pub perks: Vec<Vec<String>>,
}

/// What a screen is handed, in the order it came.
#[derive(Clone, Debug, PartialEq)]
pub enum Kept {
    Preview { slot: usize, pick: NetPick },
    Locked { slot: usize, pick: NetPick, cosmetics: Option<PlayerCosmetics> },
    All { picks: Vec<NetPick>, map: String, cosmetics: Vec<Option<PlayerCosmetics>> },
    Ready { slot: usize },
    Start(Box<PreMatchStarted>),
    End { why: PmEndWhy, name: String },
}

pub struct NetPreMatch {
    pub begin: PreMatchBegin,
    /// match.start (once it came) and pm.end (once it came, or the game's own leave).
    pub started: Option<PreMatchStarted>,
    pub ended: Option<(PmEndWhy, String)>,
    open: bool,
    kept: Vec<Kept>,
    attached: bool,
    delivered: Vec<Kept>,
    /// The preview's timer and the latest focus.
    pending: Option<(f64, NetPick, Option<NetLoadout>)>,
    lock_sent: bool,
}

fn pick(character: &str, skin: &str) -> NetPick {
    NetPick { character: character.to_owned(), skin: skin.to_owned() }
}

impl NetPreMatch {
    pub fn new(begin: PreMatchBegin) -> NetPreMatch {
        NetPreMatch {
            begin,
            started: None,
            ended: None,
            open: true,
            kept: Vec::new(),
            attached: false,
            delivered: Vec::new(),
            pending: None,
            lock_sent: false,
        }
    }

    /// Not over: not started, ended or let go.
    pub fn open(&self) -> bool {
        self.open
    }

    // (a bot's slot: the cosmetics this game drew from its seed at pm.begin, never the message's)
    fn bot_cosmetics(&self, slot: usize) -> Option<Option<PlayerCosmetics>> {
        self.begin.players.get(slot).filter(|p| p.bot).map(|p| p.cosmetics.clone())
    }

    /// A message of the session (one of another pre-match, or after the end: nothing).
    pub fn take(&mut self, m: &ServerMessage, tables: &Tables) {
        if !self.open {
            return;
        }
        match m {
            ServerMessage::PmPreview { character, skin, slot } => {
                self.keep(Kept::Preview { slot: *slot as usize, pick: pick(character, skin) });
            }
            ServerMessage::PmLocked { character, skin, slot, cosmetics } => {
                let slot = *slot as usize;
                let cosmetics = self.bot_cosmetics(slot).unwrap_or_else(|| cosmetics.clone());
                self.keep(Kept::Locked { slot, pick: pick(character, skin), cosmetics });
            }
            ServerMessage::PmAll(a) => {
                let picks: Vec<NetPick> = a
                    .picks
                    .iter()
                    .map(|p| p.as_ref().map_or_else(NetPick::default, |p| pick(&p.character, &p.skin)))
                    .collect();
                let cosmetics = (0..picks.len())
                    .map(|i| self.bot_cosmetics(i).unwrap_or_else(|| a.cosmetics.get(i).cloned().flatten()))
                    .collect();
                let map = self.map_of(tables, &a.map, a.map_pick);
                self.keep(Kept::All { picks, map, cosmetics });
            }
            ServerMessage::PmReady { slot } => self.keep(Kept::Ready { slot: *slot as usize }),
            ServerMessage::MatchStart(s) => {
                if s.match_id.as_deref() != Some(self.begin.id.as_str()) {
                    return;
                }
                // (the bots' ring-out, banner and perks, which the server leaves to the games)
                let sent = s.perks.clone().unwrap_or_default();
                let (setup, perks) = tables.with_bot_tables(&s.setup, &sent, &self.begin.players);
                let start = PreMatchStarted {
                    seed: s.seed,
                    setup,
                    slot: s.slot,
                    usernames: s.usernames.clone(),
                    map: self.map_of(tables, &s.map, s.map_pick),
                    local: s.local == Some(true),
                    perks,
                };
                self.started = Some(start.clone());
                // (nothing more of the pre-match comes: the match's own messages follow)
                self.close();
                self.keep(Kept::Start(Box::new(start)));
            }
            ServerMessage::PmEnd { why, name } => {
                self.ended = Some((*why, name.clone()));
                self.close();
                self.keep(Kept::End { why: *why, name: name.clone() });
            }
            _ => {}
        }
    }

    /// A screen takes the messages: those kept so far first, in the order they came.
    pub fn attach(&mut self) {
        self.attached = true;
        self.delivered.append(&mut self.kept);
    }

    /// No screen takes them (it went): they are kept again.
    pub fn detach(&mut self) {
        self.attached = false;
    }

    /// What the attached screen was handed since the last call.
    pub fn delivered(&mut self) -> Vec<Kept> {
        std::mem::take(&mut self.delivered)
    }

    /// HandleCharacterFocused / HandleSkinFocused: the latest focus goes to the server 0.5 s after the last change.
    pub fn preview(&mut self, p: NetPick, loadout: Option<NetLoadout>, now: f64) {
        if self.lock_sent || !self.open {
            return;
        }
        self.pending = Some((now + PREVIEW_DEBOUNCE_MS, p, loadout));
    }

    /// The preview due by `now`, to send.
    pub fn pump(&mut self, now: f64) -> Option<ClientMessage> {
        if self.pending.as_ref().is_none_or(|p| now < p.0) {
            return None;
        }
        let (_, p, loadout) = self.pending.take()?;
        (!self.lock_sent && self.open).then_some(ClientMessage::PmPreview {
            character: p.character,
            skin: p.skin,
            loadout,
        })
    }

    pub fn next_due(&self) -> Option<f64> {
        self.pending.as_ref().map(|p| p.0)
    }

    /// HandleCharacterConfirmed, and the game's own lock at its state 1-4 expiry: once (the rematch path has no
    /// unlock; the server's lock at its deadline wins over a late one). A pending preview goes.
    pub fn lock(&mut self, p: NetPick, loadout: Option<NetLoadout>) -> Option<ClientMessage> {
        self.pending = None;
        if self.lock_sent || !self.open {
            return None;
        }
        self.lock_sent = true;
        Some(ClientMessage::PmLock { character: p.character, skin: p.skin, loadout })
    }

    /// The four slots ('' an empty one).
    pub fn perks(&self, perks: &[String]) -> Option<ClientMessage> {
        self.open.then(|| ClientMessage::PmPerks { perks: perks.iter().take(4).cloned().collect() })
    }

    /// Out of the pre-match (the leader's Back to Lobby, a game that goes): the server ends it for the others.
    pub fn leave(&mut self) -> Option<ClientMessage> {
        if !self.open {
            return None;
        }
        self.ended = Some((PmEndWhy::Left, String::new()));
        self.close();
        Some(ClientMessage::PmLeave)
    }

    /// The connection dropped: as a pm.end of why error (the server has ended the pre-match for the others).
    /// Unverified: the build's handling of a lost connection is native.
    pub fn connection_lost(&mut self) {
        if !self.open {
            return;
        }
        self.ended = Some((PmEndWhy::Error, String::new()));
        self.close();
        self.keep(Kept::End { why: PmEndWhy::Error, name: String::new() });
    }

    /// Let go without a word to the server (a newer pm.begin, the match's start).
    pub fn close(&mut self) {
        self.pending = None;
        self.open = false;
    }

    // The map of pm.all or match.start: a queue's from the server's mapPick over this game's pool, a lobby's the
    // MapData name the server sends.
    fn map_of(&self, tables: &Tables, map: &str, pick: Option<u32>) -> String {
        match (self.begin.kind, self.begin.mode) {
            (PmKind::Queue, Some(mode)) => pick.map(|p| tables.queue_map(mode, p)).unwrap_or_default(),
            _ => map.to_owned(),
        }
    }

    fn keep(&mut self, k: Kept) {
        if self.attached { self.delivered.push(k) } else { self.kept.push(k) }
    }
}

/// The connection's pre-match sessions: each pm.begin makes one at once (so
/// nothing after it is missed), and lets the last one go. `lost_end`: a pm.end no open session took (a leave in a
/// rematch's wait before its pm.begin), kept for the next taker until a newer pm.begin.
#[derive(Default)]
pub struct PreMatchHub {
    pub current: Option<NetPreMatch>,
    pub lost_end: Option<(PmEndWhy, String)>,
}

/// What the hub made of a message.
#[derive(Debug, PartialEq)]
pub enum HubNews {
    /// A new pre-match began (`current`).
    Begun,
    /// A pm.end that no open pre-match took.
    LostEnd(PmEndWhy, String),
}

impl PreMatchHub {
    /// A message of the connection (the current pre-match's messages go to it).
    pub fn take(&mut self, m: &ServerMessage, tables: &Tables) -> Option<HubNews> {
        if let ServerMessage::PmBegin(b) = m {
            let begin = read_begin(b, tables)?;
            if let Some(c) = &mut self.current {
                c.close();
            }
            self.lost_end = None;
            self.current = Some(NetPreMatch::new(begin));
            return Some(HubNews::Begun);
        }
        let took = self.current.as_ref().is_some_and(NetPreMatch::open);
        if let Some(c) = &mut self.current {
            c.take(m, tables);
        }
        match m {
            ServerMessage::PmEnd { why, name } if !took => Some(HubNews::LostEnd(*why, name.clone())),
            _ => None,
        }
    }

    /// The connection's loss: the current pre-match ends as at a pm.end of why error.
    pub fn connection_lost(&mut self) {
        if let Some(c) = &mut self.current {
            c.connection_lost();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_protocol::online::{MatchStartMsg, PmPick};
    use serde_json::json;

    fn msg(v: serde_json::Value) -> ServerMessage {
        serde_json::from_value(v).unwrap()
    }

    fn begin_json() -> serde_json::Value {
        json!({ "t": "pm.begin", "id": "pm7", "kind": "lobby", "mode": null, "teamStyle": 1, "map": "", "me": 0, "players": [
            { "id": "a", "name": "Ann", "team": 0, "character": "", "skin": "", "bot": false },
            { "id": "b", "name": "Bob", "team": 1, "character": "", "skin": "", "bot": false },
            { "id": "c", "name": "Cat", "team": 0, "character": "", "skin": "", "bot": false },
            { "id": "bot1", "name": "Taz", "team": 1, "character": "taz", "skin": "", "bot": true, "seed": 7 },
        ] })
    }

    fn tables() -> Tables {
        let mut t = Tables::default();
        t.pool.ring_outs = vec!["ROV_A".into(), "ROV_B".into(), "ROV_C".into()];
        t.pool.banners = vec!["banner_a".into(), "banner_b".into()];
        t.pool.badges = vec![("badge_a".into(), vec![(1.0, 9.0)]), ("badge_b".into(), vec![(10.0, 20.0)])];
        t.perks.insert("taz".into(), vec!["sig".into(), "g1".into(), "g2".into(), "g3".into()]);
        t.maps.insert(QueueMode::TwoVTwo, vec!["M001".into(), "M002".into(), "M003".into()]);
        t
    }

    fn begin(t: &Tables) -> PreMatchBegin {
        let ServerMessage::PmBegin(b) = msg(begin_json()) else { panic!() };
        read_begin(&b, t).unwrap()
    }

    #[test]
    fn begin_is_read_with_the_bots_drawn() {
        let t = tables();
        let b = begin(&t);
        assert!(b.players.len() == 4 && b.players[3].character == "taz" && b.players[3].bot && b.mode.is_none());
        let drawn = t.bot_loadout_of(7);
        assert_eq!(b.players[3].ring_out.as_deref(), Some(drawn.ring_out.as_str()));
        assert_eq!(b.players[3].cosmetics, Some(super::super::tables::cosmetics_of_loadout(&drawn)));
        assert_eq!(t.bot_loadout_of(7), drawn);
        let mut v = begin_json();
        v["me"] = json!(4);
        let ServerMessage::PmBegin(bad) = msg(v) else { panic!() };
        assert!(read_begin(&bad, &t).is_none());
    }

    #[test]
    fn previews_are_debounced_and_the_lock_is_once() {
        let t = tables();
        let mut pm = NetPreMatch::new(begin(&t));
        let p = |c: &str, s: &str| pick(c, s);
        let mut sent = Vec::new();
        let step = |pm: &mut NetPreMatch, now: f64, sent: &mut Vec<ClientMessage>| sent.extend(pm.pump(now));
        pm.preview(p("shaggy", ""), None, 0.0);
        step(&mut pm, 300.0, &mut sent);
        pm.preview(p("taz", ""), None, 300.0);
        step(&mut pm, 600.0, &mut sent);
        pm.preview(p("batman", "b2"), None, 600.0);
        step(&mut pm, 1099.0, &mut sent);
        assert!(sent.is_empty());
        step(&mut pm, 1100.0, &mut sent);
        assert!(
            matches!(&sent[..], [ClientMessage::PmPreview { character, skin, .. }] if character == "batman" && skin == "b2")
        );
        pm.preview(p("taz", ""), None, 1100.0);
        sent.extend(pm.lock(p("taz", "s1"), None));
        step(&mut pm, 2200.0, &mut sent);
        sent.extend(pm.lock(p("taz", "s1"), None));
        pm.preview(p("shaggy", ""), None, 2200.0);
        step(&mut pm, 3200.0, &mut sent);
        assert_eq!(sent.len(), 2);
        assert!(
            matches!(&sent[1], ClientMessage::PmLock { character, skin, .. } if character == "taz" && skin == "s1")
        );
    }

    #[test]
    fn kept_until_a_screen_attaches() {
        let t = tables();
        let mut pm = NetPreMatch::new(begin(&t));
        pm.take(&msg(json!({ "t": "pm.preview", "slot": 2, "character": "shaggy", "skin": "" })), &t);
        let banner = json!({ "banner": { "slug": "banner_x", "asset": "B" }, "badges": [], "icon": { "slug": "", "asset": "" } });
        pm.take(
            &msg(json!({ "t": "pm.locked", "slot": 2, "character": "shaggy", "skin": "", "cosmetics": banner })),
            &t,
        );
        let bot_sent =
            json!({ "banner": { "slug": "", "asset": "" }, "badges": [], "icon": { "slug": "", "asset": "" } });
        pm.take(
            &ServerMessage::PmAll(mvs_protocol::messages::PmAll {
                picks: ["taz", "batman", "shaggy", "taz"]
                    .iter()
                    .map(|c| Some(PmPick { character: (*c).into(), skin: String::new() }))
                    .collect(),
                map: "M005".into(),
                map_pick: None,
                cosmetics: vec![None, None, None, Some(serde_json::from_value(bot_sent).unwrap())],
            }),
            &t,
        );
        assert!(pm.delivered().is_empty());
        pm.attach();
        let got = pm.delivered();
        assert!(matches!(&got[0], Kept::Preview { slot: 2, .. }));
        assert!(matches!(&got[1], Kept::Locked { cosmetics: Some(c), .. } if c.banner.slug == "banner_x"));
        let Kept::All { map, cosmetics, .. } = &got[2] else { panic!() };
        // (the bot's slot: its drawn cosmetics, not the message's)
        assert!(map == "M005" && cosmetics[3] == begin(&t).players[3].cosmetics && cosmetics[0].is_none());
        pm.take(&msg(json!({ "t": "pm.ready", "slot": 1 })), &t);
        assert_eq!(pm.delivered(), [Kept::Ready { slot: 1 }]);
        pm.detach();
        let start = |id: &str| {
            ServerMessage::MatchStart(MatchStartMsg {
                seed: 5,
                setup: serde_json::from_value(
                    json!({ "teamStyle": 1, "players": [{ "bot": false }, { "bot": false }, { "bot": false },
                    { "bot": true, "difficulty": 3, "character": "taz", "skin": "S" }], "flow": true }),
                )
                .unwrap(),
                slot: 0,
                usernames: vec!["Ann".into(), "Bob".into(), "Cat".into(), "Taz".into()],
                map: "M005".into(),
                map_pick: None,
                local: None,
                match_id: Some(id.into()),
                perks: Some(vec![
                    vec!["a".into(), String::new(), String::new(), String::new()],
                    vec![],
                    vec![],
                    vec![],
                ]),
            })
        };
        pm.take(&start("other"), &t);
        assert!(pm.started.is_none());
        pm.take(&start("pm7"), &t);
        let s = pm.started.clone().unwrap();
        let drawn = t.bot_loadout_of(7);
        let sp = &s.setup.players[3];
        assert!(sp.ring_out.as_deref() == Some(drawn.ring_out.as_str()) && sp.skin.as_deref() == Some("S"));
        assert_eq!(sp.banner.as_deref(), Some(drawn.banner.as_str()).filter(|b| !b.is_empty()));
        assert!(s.perks[3] == t.bot_perks("taz") && s.perks[0][0] == "a" && s.seed == 5 && !pm.open());
        assert!(pm.delivered().is_empty());
        pm.attach();
        assert!(matches!(&pm.delivered()[..], [Kept::Start(s)] if s.slot == 0 && s.perks.len() == 4));
    }

    #[test]
    fn a_queue_pre_match_names_its_map_by_the_pick() {
        let t = tables();
        let mut v = begin_json();
        v["id"] = json!("pmq");
        v["kind"] = json!("queue");
        v["mode"] = json!("2v2");
        let ServerMessage::PmBegin(b) = msg(v) else { panic!() };
        let mut pm = NetPreMatch::new(read_begin(&b, &t).unwrap());
        pm.take(
            &msg(json!({ "t": "match.start", "match": "pmq", "seed": 5, "setup": { "teamStyle": 1, "players": [] },
                "slot": 0, "usernames": [], "map": "", "mapPick": 1_234_567, "perks": [] })),
            &t,
        );
        assert_eq!(pm.started.unwrap().map, t.queue_map(QueueMode::TwoVTwo, 1_234_567));
    }

    #[test]
    fn ends_and_leaves() {
        let t = tables();
        let mut pm = NetPreMatch::new(begin(&t));
        pm.take(&msg(json!({ "t": "pm.end", "why": "left", "name": "Bob" })), &t);
        pm.attach();
        assert!(pm.leave().is_none() && pm.lock(pick("taz", ""), None).is_none());
        assert_eq!(pm.delivered(), [Kept::End { why: PmEndWhy::Left, name: "Bob".into() }]);
        let mut pm2 = NetPreMatch::new(begin(&t));
        assert_eq!(pm2.leave(), Some(ClientMessage::PmLeave));
        assert!(pm2.leave().is_none() && !pm2.open() && pm2.ended == Some((PmEndWhy::Left, String::new())));
        let mut pm3 = NetPreMatch::new(begin(&t));
        pm3.connection_lost();
        pm3.attach();
        assert_eq!(pm3.delivered(), [Kept::End { why: PmEndWhy::Error, name: String::new() }]);
    }

    #[test]
    fn the_hub_makes_one_per_begin() {
        let t = tables();
        let mut hub = PreMatchHub::default();
        assert_eq!(hub.take(&msg(begin_json()), &t), Some(HubNews::Begun));
        hub.take(&msg(json!({ "t": "pm.locked", "slot": 2, "character": "shaggy", "skin": "" })), &t);
        let mut first = hub.current.take().unwrap();
        hub.current = Some(NetPreMatch::new(first.begin.clone()));
        let mut v = begin_json();
        v["id"] = json!("pm8");
        hub.take(&msg(v), &t);
        assert!(hub.current.as_ref().unwrap().begin.id == "pm8" && hub.current.as_ref().unwrap().open());
        first.attach();
        assert!(matches!(&first.delivered()[..], [Kept::Locked { slot: 2, .. }]));
        // a pm.end with none open is the hub's
        hub.current.as_mut().unwrap().close();
        let end = msg(json!({ "t": "pm.end", "why": "left", "name": "Bob" }));
        assert_eq!(hub.take(&end, &t), Some(HubNews::LostEnd(PmEndWhy::Left, "Bob".into())));
    }
}
