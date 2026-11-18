//! A page's party (app/src/sessions.ts Party, docs/online.md "Parties"), beside its session: two players on the relay,
//! their net services linked over an invite-only relay lobby (PARTY_BUCKET, PARTY_MAX places) that the leader owns
//! and the member joins by its code (an invite's, kind 'party'). The two talk on channel 0 in FRAME_PARTY packets
//! (`PartyWire`): their cards (each one's presence's name and icon, and the fighter its main menu shows), their
//! pages' PartySay, the leader's queue follows. The leader takes into its lobby the players it invited alone, and
//! kicks one that does not link within KNOCK_WAIT_MS; the member knocks (its `hi`) every OPEN_EVERY_MS until the
//! leader's answer, OPEN_FOR_MS at the most. The leader's leave (its bye, its lobby closed, its link or the relay
//! lost) ends the party for the member; the member's leave leaves the leader's party of one. The page is told
//! `Party` on each change, None with the reason at the end, and `PartyFighter` at a change of another's fighter.
//!
//! Sans-IO: what the party needs of the service comes through `PartyCx`; the backend's answers come back by
//! `answered`, its timers run by `pump` at `next_due`; what the page's session is to hear (`SessionNote`) is drained by
//! `take_notes` after each call. An ended party still has its lobby's leave BYE_MS later: the service pumps it until
//! `next_due` is None.

use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
use serde_json::Value;

use mvs_host::bucket::Bucket;
use mvs_protocol::chat::clean_chat;
use mvs_protocol::messages::QueueWhy;
use mvs_protocol::names::{BAD_WORDS, NAME_LIMIT, Profanity, clean_name};
use mvs_protocol::online::{PARTY_MAX, QueueMode};
use mvs_protocol::p2p_frame::FRAME_PARTY;
use mvs_protocol::queue_rules::QUEUE_MODES;
use mvs_protocol::relay_wire::Presence as RelayPresence;

use crate::backend::{Answer, Ask, BackendKind, Channel, LobbyChange, P2P_MAX_PACKET};
use crate::fail::NetFail;
use crate::service::api::{ClosedWhy, MenuFighter, NetEvent, PartyJoinWhy, PartyMember, PartySay, PartyView};
use crate::service::page::PageId;
use crate::service::page::{TimerId, Timers};
use crate::service::session::Via;
use crate::service::{BYE_MS, KNOCK_WAIT_MS, NetService, OPEN_EVERY_MS, OPEN_FOR_MS, Shared, lock};

/// A party's relay lobby: invite-only (never listed), PARTY_MAX places.
pub const PARTY_BUCKET: &str = "mvs:party";
/// A party's packets taken from each other player: PARTY_PER_S a second, PARTY_BURST at once; past that one is
/// dropped (the party goes on). Unverified: the port's own numbers, well over a party's talk (a knock a second, a
/// card or a say on a change).
pub const PARTY_PER_S: f64 = 5.0;
pub const PARTY_BURST: f64 = 10.0;
/// A member takes its leader's queue follows PARTY_FOLLOWS_PER_MIN a minute at the most; past that one is dropped.
/// Unverified: the port's own number (a leader moves once per found older lobby, a few a search).
pub const PARTY_FOLLOWS_PER_MIN: f64 = 6.0;

/// The longest lobby id a follow may name, in UTF-16 units.
const ID_MAX: usize = 128;
/// The longest custom lobby code a PartySay may name, in UTF-16 units.
const CODE_MAX: usize = 64;

static NAME_REFUSED: LazyLock<Profanity> = LazyLock::new(|| Profanity::new(&BAD_WORDS));

/// A presence's icon: a ProfileIconData asset name or '' (protocol/loadout.ts's rule for an asset).
fn icon_ok(s: &str) -> bool {
    s.len() <= 96 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A main-menu fighter as a page or another player sent it: its key a non-empty asset name, its skin an asset name
/// or ''; None when it is not one.
pub(crate) fn menu_fighter_of(f: &MenuFighter) -> Option<MenuFighter> {
    (!f.key.is_empty() && icon_ok(&f.key) && icon_ok(&f.skin)).then(|| f.clone())
}

/// A card's main-menu fighter as another player sent it (`{ key, skin }`, the skin '' when missing); None when
/// there is none or it is not one.
fn fighter_of(v: Option<&Value>) -> Option<MenuFighter> {
    let v = v?;
    let key = v.get("key").and_then(Value::as_str)?;
    let skin = v.get("skin").and_then(Value::as_str).unwrap_or_default();
    menu_fighter_of(&MenuFighter { key: key.to_string(), skin: skin.to_string() })
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The head of `s` of at most `n` UTF-16 units. Deviation: a cut inside a surrogate pair drops the pair, where the TS
/// keeps its lone high half (a Rust string holds none).
fn utf16_head(s: &str, n: usize) -> &str {
    let mut k = 0;
    for (i, c) in s.char_indices() {
        k += c.len_utf16();
        if k > n {
            return &s[..i];
        }
    }
    s
}

/// A name another player sent, as it is shown ('' when it holds a refused word).
fn shown_name(v: &Value) -> String {
    let said = v.as_str().map(clean_name).unwrap_or_default();
    if NAME_REFUSED.refuses(&said) { String::new() } else { said }
}

/// A presence as a page sent it (sessions.ts presenceOf): its name cleaned ('' with a refused word), its icon ''
/// when it is not an asset's name.
pub(crate) fn presence_of(p: &RelayPresence) -> RelayPresence {
    let name = clean_name(&p.name);
    RelayPresence {
        name: if NAME_REFUSED.refuses(&name) { String::new() } else { name },
        icon: if icon_ok(&p.icon) { p.icon.clone() } else { String::new() },
        busy: p.busy,
        listed: p.listed,
        invites: p.invites,
        status: p.status,
    }
}

/// The modes of `asks` (wire names) in the queue's order.
fn modes_of(asks: &Value) -> Vec<QueueMode> {
    let asks = asks.as_array().map(Vec::as_slice).unwrap_or_default();
    QUEUE_MODES.iter().copied().filter(|m| asks.iter().any(|a| a.as_str() == Some(m.id()))).collect()
}

/// A PartySay as another net service sent it (sessions.ts partySayOf); None when it is not one.
pub(crate) fn party_say_of(v: &Value) -> Option<PartySay> {
    let k = v.get("k").and_then(Value::as_str)?;
    match k {
        "queue" | "open" => {
            let modes = modes_of(v.get("modes").unwrap_or(&Value::Null));
            if modes.is_empty() {
                None
            } else if k == "queue" {
                Some(PartySay::Queue { modes })
            } else {
                Some(PartySay::Open { modes })
            }
        }
        "custom" => match v.get("code").and_then(Value::as_str) {
            Some(code) if !code.is_empty() && units(code) <= CODE_MAX => {
                Some(PartySay::Custom { code: code.to_string() })
            }
            _ => None,
        },
        "home" => Some(PartySay::Home),
        "cancel" => Some(PartySay::Cancel),
        "ready" => Some(PartySay::Ready),
        _ => None,
    }
}

/// A page's PartySay as the TS reads it (partySayOf of its JSON): the modes in the queue's order, none of them or a
/// code past CODE_MAX not one.
pub(crate) fn party_say_clean(say: &PartySay) -> Option<PartySay> {
    party_say_of(&serde_json::to_value(say).ok()?)
}

/// The says a member may make (the leader's are every kind): Queue, Open and Custom are the leader's alone.
fn by_member(say: &PartySay) -> bool {
    matches!(say, PartySay::Home | PartySay::Cancel | PartySay::Ready)
}

/// Where a party's member follows its leader in the queue: the lobby its leader's queue session is in, or listed at
/// home, and the modes the member's replayed join names there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyFollow {
    pub lobby: String,
    pub modes: Vec<QueueMode>,
}

/// What a page's party tells its session (sessions.ts Session's partyChanged, partyOut, follow, unfollowed), in the
/// order the party said it; the service hands them to a queue session after each call of the party.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionNote {
    /// A member linked or left: a queue session's leader tells it where to follow again.
    Changed,
    /// Its party's queue is over (QueueWhy Party, PartyLate).
    Out(QueueWhy),
    /// (a member) The leader's queue lobby to follow.
    Follow(PartyFollow),
    /// (the leader) The member could not follow it into that lobby.
    Unfollowed(String),
    /// (a member) The leader's last follow as the party keeps it now (the TS's page.party.followed, read by the
    /// queue session's join): None at a Home say, an out, the party's end.
    Followed(Option<PartyFollow>),
}

/// What a page's queue session tells its party (sessions.ts QueueSession's calls of Party), in the order it said
/// them; the service hands them to the party after each call of the session.
#[derive(Clone, Debug, PartialEq)]
pub enum PartyNote {
    /// (the leader) Where its queue session is: to the member.
    SendFollow(PartyFollow),
    /// Its queue session is out of the queue by a party why: to the others.
    QueueOut(QueueWhy),
    /// (a member) It could not follow the leader into that lobby: to the leader.
    Unfollowed(String),
}

/// What a party's net services say to each other (FRAME_PARTY): `Hi` a member's card (the member's first knocks
/// until the leader's answer, which is its own; again on a change of either's presence or main-menu fighter; the
/// port's own `fighter` left out when there is none, so the card's bytes are the TS's then), `Say` a page's PartySay,
/// `Follow` (the leader's) where its queue session is, `Unfollowed` (the member's) it could not follow into that
/// lobby, `Out` this side's queue session is out of the queue by a party why, `Bye` this side left the party.
///
/// Deviation: the bye carries its why as a ClosedWhy's JSON (the page words it), where the TS sends its text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "k", rename_all = "lowercase")]
enum PartyWire {
    Hi {
        name: String,
        icon: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fighter: Option<MenuFighter>,
    },
    Say {
        say: PartySay,
    },
    Follow {
        lobby: String,
        modes: Vec<QueueMode>,
    },
    Unfollowed {
        lobby: String,
    },
    Out {
        why: QueueWhy,
    },
    Bye {
        why: ClosedWhy,
    },
}

impl PartyWire {
    fn kind(&self) -> &'static str {
        match self {
            PartyWire::Hi { .. } => "hi",
            PartyWire::Say { .. } => "say",
            PartyWire::Follow { .. } => "follow",
            PartyWire::Unfollowed { .. } => "unfollowed",
            PartyWire::Out { .. } => "out",
            PartyWire::Bye { .. } => "bye",
        }
    }
}

/// A party packet read (sessions.ts partyWireOf), its kind byte first; None when it is none (dropped).
fn party_wire_of(bytes: &[u8]) -> Option<PartyWire> {
    let m: Value = serde_json::from_slice(bytes.get(1..)?).ok()?;
    let id = |v: Option<&Value>| v.and_then(Value::as_str).filter(|s| units(s) <= ID_MAX).map(str::to_string);
    match m.get("k").and_then(Value::as_str)? {
        "hi" => Some(PartyWire::Hi {
            name: shown_name(m.get("name").unwrap_or(&Value::Null)),
            icon: m.get("icon").and_then(Value::as_str).filter(|s| icon_ok(s)).unwrap_or_default().to_string(),
            fighter: fighter_of(m.get("fighter")),
        }),
        "say" => party_say_of(m.get("say")?).map(|say| PartyWire::Say { say }),
        "follow" => id(m.get("lobby"))
            .map(|lobby| PartyWire::Follow { lobby, modes: modes_of(m.get("modes").unwrap_or(&Value::Null)) }),
        "unfollowed" => id(m.get("lobby")).map(|lobby| PartyWire::Unfollowed { lobby }),
        "out" => {
            let late = m.get("why").and_then(Value::as_str) == Some("partyLate");
            Some(PartyWire::Out { why: if late { QueueWhy::PartyLate } else { QueueWhy::Party } })
        }
        // (another player's code: read as one, else its words cleaned as a chat line before they reach the page)
        "bye" => {
            let v = m.get("why").cloned().unwrap_or(Value::Null);
            let why = serde_json::from_value::<ClosedWhy>(v.clone())
                .unwrap_or_else(|_| ClosedWhy::Other { text: clean_chat(&v) });
            Some(PartyWire::Bye { why })
        }
        _ => None,
    }
}

/// What the party needs of the net service: its backend (the relay's: a party is on the relay alone), the page's
/// events, this player's card and the service's invite pace, the lobby codes, and the other sessions' and parties'
/// links (a party's link to a player it lets go stays when one of them uses it).
pub(crate) trait PartyCx {
    /// The relay's calls; None (and false) while it is off (the service ends the party then).
    fn create_lobby(&mut self, bucket: &str, max: u32) -> Option<Ask>;
    fn join_lobby(&mut self, lobby: &str) -> Option<Ask>;
    fn leave_lobby(&mut self, lobby: &str) -> Option<Ask>;
    fn kick(&mut self, lobby: &str, member: &str);
    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool;
    fn close_peer(&mut self, peer: &str);
    fn invite(&mut self, to: &str, payload: &str) -> Option<Ask>;
    /// An event to the party's page.
    fn post(&mut self, e: NetEvent);
    /// This player's card: its presence's name and icon ('' before the page sent one).
    fn card(&self) -> (String, String);
    /// The fighter this player's main menu shows (None before the page sent one).
    fn fighter(&self) -> Option<MenuFighter>;
    /// An invite to that player is within the service's pace (sessions.ts mayInvite), counted when it is.
    fn may_invite(&mut self, to: &str, now: f64) -> bool;
    /// The lobby code of a relay lobby (sessions.ts codeOn('relay', id)); Err the party's end for it.
    fn code_on(&self, lobby: &str) -> Result<String, ClosedWhy>;
    /// An open session on the relay, or another party, has that player (sessions.ts peerUsed).
    fn peer_used(&self, peer: &str) -> bool;
    fn log(&mut self, text: &str);
}

/// A party's card of a player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Card {
    name: String,
    icon: String,
    fighter: Option<MenuFighter>,
}

impl Card {
    fn hi(self) -> PartyWire {
        PartyWire::Hi { name: self.name, icon: self.icon, fighter: self.fighter }
    }

    /// The same name and icon: the page's party (`Party`) is the same; the fighter goes by `PartyFighter`.
    fn shown_same(&self, other: &Card) -> bool {
        self.name == other.name && self.icon == other.icon
    }
}

/// The party's deadlines.
#[derive(Clone, Debug, PartialEq)]
enum PartyTimer {
    /// (member) The next knock.
    Knock,
    /// (leader) An invited member that has not linked yet.
    Waiting(String),
    /// The party is over from the other side, BYE_MS after the lobby's close or the leader's leave.
    Lost(ClosedWhy),
    /// The lobby and the links of these peers let go, BYE_MS after this side's bye.
    LetGo(Vec<String>),
}

/// A backend call in flight.
#[derive(Clone, Debug, PartialEq)]
enum PartyAsk {
    Make,
    Join,
    Invite(String),
    /// A lobby left: an error is logged.
    Leave,
}

/// The lobby's make (the leader's): its invites wait for it.
#[derive(Clone, Debug, PartialEq)]
enum Made {
    /// Under way: the invites asked meanwhile (to, name).
    Pending(Vec<(String, String)>),
    Done(bool),
}

/// A page's party (sessions.ts Party).
pub struct Party {
    /// The relay it last saw (SvcCx::held).
    held: Option<Shared>,
    /// The page it is the party of.
    pub page: PageId,
    pub leader: String,
    pub lobby: String,
    pub code: String,
    pub ended: bool,
    /// This player's id on the relay.
    me: String,
    /// The other linked players' cards, by id.
    cards: IndexMap<String, Card>,
    /// The lobby's members as the relay last told them.
    members: IndexSet<String>,
    /// This player is in the lobby (its make or join answered, its close not told).
    inside: bool,
    /// (leader) The players invited, the members not linked yet (their kick), the lobby's make.
    invited: IndexSet<String>,
    waiting: IndexMap<String, TimerId>,
    made: Made,
    /// (member) Linked to the leader (its first message came), and the knock until then from its start.
    linked: bool,
    knock: Option<TimerId>,
    knock_from: f64,
    /// (member) The leader's last follow.
    pub followed: Option<PartyFollow>,
    /// The name the page gave with its last invite: this player's card's while its presence has none.
    own_name: String,
    /// (member) The player whose invite it joined by: the lobby's owner has to be that one.
    inviter: String,
    /// Each other player's packet budget (PARTY_PER_S), and (member) the follows' (PARTY_FOLLOWS_PER_MIN).
    budgets: IndexMap<String, Bucket>,
    follow_budget: Option<Bucket>,
    timers: Timers<PartyTimer>,
    asks: Vec<(Ask, PartyAsk)>,
    notes: Vec<SessionNote>,
}

/// The party invite's payload (its fields in the TS's order).
#[derive(Serialize)]
struct InvitePayload<'a> {
    kind: &'static str,
    code: &'a str,
    name: &'a str,
}

impl Party {
    fn new(page: PageId, me: &str, leader: &str) -> Party {
        Party {
            page,
            leader: leader.to_string(),
            lobby: String::new(),
            code: String::new(),
            ended: false,
            me: me.to_string(),
            cards: IndexMap::new(),
            members: IndexSet::new(),
            inside: false,
            invited: IndexSet::new(),
            waiting: IndexMap::new(),
            made: Made::Done(false),
            linked: false,
            knock: None,
            knock_from: 0.0,
            followed: None,
            own_name: String::new(),
            inviter: String::new(),
            budgets: IndexMap::new(),
            follow_budget: None,
            timers: Timers::default(),
            asks: Vec::new(),
            notes: Vec::new(),
            held: None,
        }
    }

    /// A party this player leads: its lobby made now. `me` this player's id on the relay.
    pub(crate) fn lead_cx(cx: &mut dyn PartyCx, page: PageId, me: &str) -> Party {
        let mut p = Party::new(page, me, me);
        p.made = Made::Pending(Vec::new());
        match cx.create_lobby(PARTY_BUCKET, PARTY_MAX as u32) {
            Some(ask) => p.asks.push((ask, PartyAsk::Make)),
            None => p.made = Made::Done(false),
        }
        p
    }

    /// A party this player joins, by its lobby on the relay and its code, from `inviter`'s invite.
    pub(crate) fn joining_cx(
        cx: &mut dyn PartyCx,
        page: PageId,
        me: &str,
        lobby: &str,
        code: &str,
        inviter: &str,
    ) -> Party {
        let mut p = Party::new(page, me, "");
        p.lobby = lobby.to_string();
        p.code = code.to_string();
        p.inviter = inviter.to_string();
        if let Some(ask) = cx.join_lobby(lobby) {
            p.asks.push((ask, PartyAsk::Join));
        }
        p
    }

    pub fn leading(&self) -> bool {
        self.leader == self.me
    }

    /// The party's players, this one among them (the lobby's members, or the linked ones when more).
    fn size(&self) -> usize {
        self.members.len().max(1 + self.cards.len())
    }

    /// That player is in the party's lobby, or linked to this one through it (its link is accepted, its packets
    /// come).
    pub fn has_peer(&self, id: &str) -> bool {
        !self.ended
            && !id.is_empty()
            && id != self.me
            && (self.members.contains(id) || self.cards.contains_key(id) || id == self.leader)
    }

    /// Every player `has_peer` says yes to (the backend's accept set).
    pub fn peers(&self) -> Vec<String> {
        let all = self.members.iter().chain(self.cards.keys()).chain(std::iter::once(&self.leader));
        let mut out: IndexSet<String> = IndexSet::new();
        out.extend(all.filter(|id| self.has_peer(id)).cloned());
        out.into_iter().collect()
    }

    /// What the page's session is to hear, since the last call.
    pub fn take_notes(&mut self) -> Vec<SessionNote> {
        std::mem::take(&mut self.notes)
    }

    /// When `pump` has a deadline to run (None: none, and an ended party is done).
    pub fn next_due(&self) -> Option<f64> {
        self.timers.next_due()
    }

    /// Runs the deadlines due by `now`.
    pub(crate) fn pump_cx(&mut self, cx: &mut dyn PartyCx, now: f64) {
        // (each at its own time, as the TS's clock runs them)
        while let Some((at, id, t)) = self.timers.take_due(now) {
            match t {
                PartyTimer::Knock => self.knocked(cx, at),
                PartyTimer::Waiting(member) => {
                    if self.waiting.get(&member) != Some(&id) {
                        continue;
                    }
                    self.waiting.shift_remove(&member);
                    if self.ended || self.cards.contains_key(&member) || !self.members.contains(&member) {
                        continue;
                    }
                    cx.log(&format!("party: member never linked: kicked {member}"));
                    let lobby = self.lobby.clone();
                    cx.kick(&lobby, &member);
                }
                PartyTimer::Lost(why) => self.lost_cx(cx, why),
                PartyTimer::LetGo(peers) => self.let_go(cx, &peers),
            }
        }
    }

    /// A backend's answer: true when the call was this party's.
    pub(crate) fn answered_cx(&mut self, cx: &mut dyn PartyCx, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        let Some(i) = self.asks.iter().position(|(a, _)| *a == ask) else { return false };
        let (_, what) = self.asks.remove(i);
        match what {
            PartyAsk::Make => {
                let ok = self.made_answer(cx, r);
                let waiting = match std::mem::replace(&mut self.made, Made::Done(ok)) {
                    Made::Pending(v) => v,
                    Made::Done(_) => Vec::new(),
                };
                for (to, name) in waiting {
                    self.send_invite(cx, &to, &name, ok);
                }
            }
            PartyAsk::Join => self.join_answer(cx, r, now),
            PartyAsk::Invite(to) => {
                if let Err(e) = r {
                    cx.log(&format!("party: invite {e}"));
                    cx.post(NetEvent::InviteRefused { to });
                }
            }
            PartyAsk::Leave => {
                if let Err(e) = r {
                    cx.log(&format!("party: leaving the lobby: {e}"));
                }
            }
        }
        true
    }

    // the lobby's make answered; false when the party is not made
    fn made_answer(&mut self, cx: &mut dyn PartyCx, r: &Result<Answer, String>) -> bool {
        let id = match r {
            Ok(Answer::Created(id)) => id.clone(),
            Ok(other) => {
                cx.log(&format!("party: the lobby's make answered {other:?}"));
                self.lost_cx(cx, ClosedWhy::BadAnswer);
                return false;
            }
            Err(e) => {
                let why = if e.contains("LimitExceeded") {
                    ClosedWhy::Limited
                } else {
                    ClosedWhy::PartyMakeFailed { why: NetFail::of(e) }
                };
                self.lost_cx(cx, why);
                return false;
            }
        };
        if self.ended {
            self.leave(cx, &id);
            return false;
        }
        self.lobby = id.clone();
        self.inside = true;
        self.members = IndexSet::from([self.me.clone()]);
        match cx.code_on(&id) {
            Ok(code) => self.code = code,
            Err(why) => {
                self.lost_cx(cx, why);
                return false;
            }
        }
        cx.log(&format!("party: made {id}"));
        self.tell(cx);
        true
    }

    // the lobby's join answered
    fn join_answer(&mut self, cx: &mut dyn PartyCx, r: &Result<Answer, String>, now: f64) {
        let (owner, members) = match r {
            Ok(Answer::Joined { owner, members, .. }) => (owner.clone(), members.clone()),
            Ok(other) => {
                cx.log(&format!("party: the lobby's join answered {other:?}"));
                return self.lost_cx(cx, ClosedWhy::BadAnswer);
            }
            // Deviation: the backend's reason goes to the page as it is (PartyJoinWhy Backend), worded by the UI as
            // sessions.ts joinWhy words a lobby gone or full.
            Err(e) => {
                let why = if e.contains("LimitExceeded") {
                    ClosedWhy::Limited
                } else {
                    ClosedWhy::PartyJoinFailed { why: PartyJoinWhy::Backend { text: e.clone() } }
                };
                return self.lost_cx(cx, why);
            }
        };
        if self.ended {
            let lobby = self.lobby.clone();
            return self.leave(cx, &lobby);
        }
        self.inside = true;
        if owner.is_empty() || owner == self.me {
            return self.lost_cx(cx, ClosedWhy::NoLeader);
        }
        // (the lobby of another than the invite's sender: not that party; left at once)
        if owner != self.inviter {
            return self.lost_cx(cx, ClosedWhy::PartyJoinFailed { why: PartyJoinWhy::NotInviter });
        }
        self.leader = owner.clone();
        self.members = members.into_iter().collect();
        self.members.insert(owner.clone());
        cx.log(&format!("party: joined {} of {owner}", self.lobby));
        self.knock_from = now;
        self.knock = Some(self.timers.every(now, OPEN_EVERY_MS, PartyTimer::Knock));
        self.knocked(cx, now);
    }

    // the member's knock: its card to the leader until the leader's first message, OPEN_FOR_MS at the most
    fn knocked(&mut self, cx: &mut dyn PartyCx, now: f64) {
        if self.ended || self.linked {
            if let Some(t) = self.knock.take() {
                self.timers.cancel(t);
            }
            return;
        }
        if now - self.knock_from >= OPEN_FOR_MS {
            return self.lost_cx(cx, ClosedWhy::LeaderSilent);
        }
        let hi = self.own_card(cx).hi();
        let leader = self.leader.clone();
        self.send(cx, &leader, &hi);
    }

    /// A party invite to `to` (the leader's): refused (`InviteRefused`) when the party is full, past the pace or the
    /// relay refused it.
    pub(crate) fn invite_cx(&mut self, cx: &mut dyn PartyCx, to: &str, name: &str, now: f64) {
        self.own_name = clean_name(name);
        if self.ended
            || !self.leading()
            || to == self.me
            || self.cards.contains_key(to)
            || self.size() >= PARTY_MAX
            || !cx.may_invite(to, now)
        {
            return cx.post(NetEvent::InviteRefused { to: to.to_string() });
        }
        match &mut self.made {
            Made::Pending(v) => v.push((to.to_string(), name.to_string())),
            Made::Done(ok) => {
                let ok = *ok;
                self.send_invite(cx, to, name, ok);
            }
        }
    }

    // an invite once the lobby's make is answered (`ok` it was made)
    fn send_invite(&mut self, cx: &mut dyn PartyCx, to: &str, name: &str, ok: bool) {
        if !ok || self.ended || self.size() >= PARTY_MAX {
            return cx.post(NetEvent::InviteRefused { to: to.to_string() });
        }
        self.invited.insert(to.to_string());
        let payload = InvitePayload { kind: "party", code: &self.code, name: utf16_head(name, NAME_LIMIT) };
        let payload = serde_json::to_string(&payload).expect("an invite's payload is JSON");
        match cx.invite(to, &payload) {
            Some(ask) => self.asks.push((ask, PartyAsk::Invite(to.to_string()))),
            None => cx.post(NetEvent::InviteRefused { to: to.to_string() }),
        }
    }

    fn own_card(&self, cx: &dyn PartyCx) -> Card {
        let (name, icon) = cx.card();
        Card { name: if name.is_empty() { self.own_name.clone() } else { name }, icon, fighter: cx.fighter() }
    }

    /// This player's presence or main-menu fighter changed: its card to the others, and to the page.
    pub(crate) fn card_changed_cx(&mut self, cx: &mut dyn PartyCx) {
        if self.ended {
            return;
        }
        let hi = self.own_card(cx).hi();
        for id in self.linked_peers() {
            self.send(cx, &id, &hi);
        }
        if if self.leading() { !self.lobby.is_empty() } else { self.linked } {
            self.tell(cx);
        }
    }

    fn linked_peers(&self) -> Vec<String> {
        if self.leading() {
            self.cards.keys().cloned().collect()
        } else if self.linked {
            vec![self.leader.clone()]
        } else {
            Vec::new()
        }
    }

    /// The page's PartySay to the others: the leader's any; the member's Home, Cancel and Ready.
    pub(crate) fn say_cx(&mut self, cx: &mut dyn PartyCx, say: &PartySay) {
        let Some(say) = party_say_clean(say) else { return };
        if self.ended || (!self.leading() && !by_member(&say)) {
            return;
        }
        if say == PartySay::Home {
            self.set_followed(None);
        }
        let m = PartyWire::Say { say };
        for id in self.linked_peers() {
            self.send(cx, &id, &m);
        }
    }

    /// (leader) Where its queue session is: to the member.
    fn send_follow(&mut self, cx: &mut dyn PartyCx, f: &PartyFollow) {
        if self.ended || !self.leading() {
            return;
        }
        let m = PartyWire::Follow { lobby: f.lobby.clone(), modes: f.modes.clone() };
        for id in self.cards.keys().cloned().collect::<Vec<_>>() {
            self.send(cx, &id, &m);
        }
    }

    /// This side's queue session is out of the queue by a party why (QueueWhy Party, PartyLate): to the others, whose
    /// sessions leave their search too.
    fn queue_out(&mut self, cx: &mut dyn PartyCx, why: QueueWhy) {
        if self.ended {
            return;
        }
        for id in self.linked_peers() {
            self.send(cx, &id, &PartyWire::Out { why });
        }
    }

    /// (member) It could not follow the leader into that lobby: to the leader.
    fn unfollowed(&mut self, cx: &mut dyn PartyCx, lobby: &str) {
        if !self.ended && !self.leading() && self.linked {
            let leader = self.leader.clone();
            self.send(cx, &leader, &PartyWire::Unfollowed { lobby: lobby.to_string() });
        }
    }

    /// A FRAME_PARTY packet from another player.
    pub(crate) fn message_cx(&mut self, cx: &mut dyn PartyCx, from: &str, bytes: &[u8], now: f64) {
        if self.ended {
            return;
        }
        let budget = self.budgets.entry(from.to_string()).or_insert_with(|| Bucket::new(PARTY_PER_S, PARTY_BURST, now));
        if !budget.take(1.0, now) {
            return;
        }
        let Some(m) = party_wire_of(bytes) else { return };
        if self.leading() {
            return self.member_said(cx, from, m);
        }
        if from != self.leader {
            return;
        }
        let fresh = !self.linked;
        if !self.linked {
            self.linked = true;
            if let Some(t) = self.knock.take() {
                self.timers.cancel(t);
            }
            self.cards.entry(from.to_string()).or_default();
            if !matches!(m, PartyWire::Hi { .. }) {
                self.tell(cx);
            }
        }
        match m {
            PartyWire::Hi { name, icon, fighter } => {
                let card = Card { name, icon, fighter };
                let was = self.cards.insert(from.to_string(), card.clone());
                if fresh || !was.as_ref().is_some_and(|w| w.shown_same(&card)) {
                    self.tell(cx);
                }
                Self::fighter_told(cx, from, was.as_ref(), &card);
            }
            PartyWire::Say { say } => {
                if say == PartySay::Home {
                    self.set_followed(None);
                }
                cx.post(NetEvent::PartySay { from: from.to_string(), say });
            }
            PartyWire::Follow { lobby, modes } => {
                // (past PARTY_FOLLOWS_PER_MIN, or into the party's own lobby: dropped. A lobby that is no queue's has
                // a core that refuses the replayed queue.join: the member goes home from it, and tells the leader)
                let budget = self
                    .follow_budget
                    .get_or_insert_with(|| Bucket::new(PARTY_FOLLOWS_PER_MIN / 60.0, PARTY_FOLLOWS_PER_MIN, now));
                if lobby == self.lobby || !budget.take(1.0, now) {
                    return cx.log(&format!("party: a follow dropped {lobby}"));
                }
                let f = PartyFollow { lobby, modes };
                self.set_followed(Some(f.clone()));
                self.notes.push(SessionNote::Follow(f));
            }
            PartyWire::Out { why } => {
                self.set_followed(None);
                self.notes.push(SessionNote::Out(why));
            }
            PartyWire::Bye { why } => {
                let left = matches!(&why, ClosedWhy::Left)
                    || matches!(&why, ClosedWhy::Other { text } if text.is_empty() || text == "left");
                let why = if left { ClosedWhy::LeaderLeft } else { ClosedWhy::PartyEndedBy { why: Box::new(why) } };
                self.lost_cx(cx, why);
            }
            PartyWire::Unfollowed { .. } => {}
        }
    }

    /// Another player's main-menu fighter to the page when its card changed it (a card with none, as an older
    /// game's, tells nothing until one with a fighter came).
    fn fighter_told(cx: &mut dyn PartyCx, from: &str, was: Option<&Card>, card: &Card) {
        if was.and_then(|w| w.fighter.as_ref()) != card.fighter.as_ref() {
            cx.post(NetEvent::PartyFighter { from: from.to_string(), fighter: card.fighter.clone() });
        }
    }

    // (leader) a member's packet
    fn member_said(&mut self, cx: &mut dyn PartyCx, from: &str, m: PartyWire) {
        if !self.members.contains(from) && !self.cards.contains_key(from) {
            return;
        }
        match m {
            PartyWire::Hi { name, icon, fighter } => {
                let was = self.cards.get(from).cloned();
                if was.is_none() && (!self.invited.contains(from) || 1 + self.cards.len() >= PARTY_MAX) {
                    return;
                }
                if let Some(t) = self.waiting.shift_remove(from) {
                    self.timers.cancel(t);
                }
                let card = Card { name, icon, fighter };
                self.cards.insert(from.to_string(), card.clone());
                if was.is_none() {
                    cx.log(&format!("party: member linked {from}"));
                    let hi = self.own_card(cx).hi();
                    self.send(cx, from, &hi);
                }
                if !was.as_ref().is_some_and(|w| w.shown_same(&card)) {
                    self.tell(cx);
                }
                Self::fighter_told(cx, from, was.as_ref(), &card);
                if was.is_none() {
                    self.notes.push(SessionNote::Changed);
                }
            }
            PartyWire::Say { say } => {
                if by_member(&say) && self.cards.contains_key(from) {
                    cx.post(NetEvent::PartySay { from: from.to_string(), say });
                }
            }
            PartyWire::Unfollowed { lobby } => {
                if self.cards.contains_key(from) {
                    self.notes.push(SessionNote::Unfollowed(lobby));
                }
            }
            PartyWire::Out { why } => {
                if self.cards.contains_key(from) {
                    self.notes.push(SessionNote::Out(why));
                }
            }
            PartyWire::Bye { .. } => self.drop_member(cx, from),
            PartyWire::Follow { .. } => {}
        }
    }

    // (leader) a member is out: out of the lobby too, the page told
    fn drop_member(&mut self, cx: &mut dyn PartyCx, id: &str) {
        if let Some(t) = self.waiting.shift_remove(id) {
            self.timers.cancel(t);
        }
        let had = self.cards.shift_remove(id).is_some();
        self.budgets.shift_remove(id);
        let kick = self.members.shift_remove(id) && self.inside;
        let used = cx.peer_used(id);
        let lobby = self.lobby.clone();
        if kick {
            cx.kick(&lobby, id);
        }
        if !used {
            cx.close_peer(id);
        }
        if !had {
            return;
        }
        cx.log(&format!("party: member left {id}"));
        self.tell(cx);
        self.notes.push(SessionNote::Changed);
        // (a queue search of the party's is over without that member)
        self.notes.push(SessionNote::Out(QueueWhy::Party));
    }

    /// A change to the party's lobby.
    pub(crate) fn lobby_changed_cx(&mut self, cx: &mut dyn PartyCx, c: &LobbyChange, now: f64) {
        if self.ended {
            return;
        }
        let members = match c {
            LobbyChange::Closed { why, .. } => {
                self.inside = false;
                // (BYE_MS for the leader's last messages, its bye among them)
                let why = if self.leading() {
                    ClosedWhy::PartyLobbyClosed { text: why.clone() }
                } else {
                    ClosedWhy::PartyEnded
                };
                self.timers.after(now, BYE_MS, PartyTimer::Lost(why));
                return;
            }
            LobbyChange::Members { members, .. } => members,
        };
        self.members = members.iter().cloned().collect();
        if !self.leading() {
            if !self.members.contains(&self.leader) {
                self.timers.after(now, BYE_MS, PartyTimer::Lost(ClosedWhy::LeaderLeft));
            }
            return;
        }
        for id in self.cards.keys().cloned().collect::<Vec<_>>() {
            if !self.members.contains(&id) {
                self.drop_member(cx, &id);
            }
        }
        for id in self.members.iter().cloned().collect::<Vec<_>>() {
            if id == self.me || self.cards.contains_key(&id) || self.waiting.contains_key(&id) {
                continue;
            }
            // (a player this one did not invite: the code was passed on)
            if !self.invited.contains(&id) {
                self.members.shift_remove(&id);
                let lobby = self.lobby.clone();
                cx.kick(&lobby, &id);
                continue;
            }
            let t = self.timers.after(now, KNOCK_WAIT_MS, PartyTimer::Waiting(id.clone()));
            self.waiting.insert(id, t);
        }
    }

    /// The link to a peer closed.
    pub(crate) fn peer_closed_cx(&mut self, cx: &mut dyn PartyCx, peer: &str) {
        if self.ended {
            return;
        }
        if !self.leading() {
            if peer == self.leader && self.linked {
                self.lost_cx(cx, ClosedWhy::LeaderLost);
            }
            return;
        }
        if self.cards.contains_key(peer) {
            self.drop_member(cx, peer);
        }
    }

    /// The page's party as it is now. Its `why` is Left (none: the TS's '').
    fn tell(&mut self, cx: &mut dyn PartyCx) {
        if self.ended {
            return;
        }
        let Card { name, icon, .. } = self.own_card(cx);
        let own = PartyMember { id: self.me.clone(), name, icon };
        let others: Vec<PartyMember> = self
            .cards
            .iter()
            .map(|(id, c)| PartyMember { id: id.clone(), name: c.name.clone(), icon: c.icon.clone() })
            .collect();
        let members = if self.leading() {
            std::iter::once(own).chain(others).collect()
        } else {
            let leader = others.iter().find(|x| x.id == self.leader).cloned().unwrap_or(PartyMember {
                id: self.leader.clone(),
                name: String::new(),
                icon: String::new(),
            });
            let rest = others.into_iter().filter(|x| x.id != self.leader);
            [leader, own].into_iter().chain(rest).collect()
        };
        let party = PartyView { leader: self.leader.clone(), members, me: self.me.clone() };
        cx.post(NetEvent::Party { party: Some(party), why: ClosedWhy::Left });
    }

    /// This side leaves (`Left` the player's leave): the others told by its bye, the lobby left (the leader's
    /// destroyed) BYE_MS later.
    pub(crate) fn end_cx(&mut self, cx: &mut dyn PartyCx, why: ClosedWhy, now: f64) {
        if self.ended {
            return;
        }
        let peers = self.linked_peers();
        let bye = PartyWire::Bye { why: why.clone() };
        for id in &peers {
            self.send(cx, id, &bye);
        }
        self.finish(cx, why);
        self.timers.after(now, BYE_MS, PartyTimer::LetGo(peers));
    }

    /// The party is over from the other side (or could not be made or joined): `why` to the page.
    pub(crate) fn lost_cx(&mut self, cx: &mut dyn PartyCx, why: ClosedWhy) {
        if self.ended {
            return;
        }
        let mut peers: IndexSet<String> = self.cards.keys().cloned().collect();
        if !self.leading() {
            peers.insert(self.leader.clone());
        }
        let peers: Vec<String> = peers.into_iter().filter(|x| !x.is_empty()).collect();
        self.finish(cx, why);
        self.let_go(cx, &peers);
    }

    fn finish(&mut self, cx: &mut dyn PartyCx, why: ClosedWhy) {
        self.ended = true;
        if let Some(t) = self.knock.take() {
            self.timers.cancel(t);
        }
        for (_, t) in self.waiting.drain(..) {
            self.timers.cancel(t);
        }
        cx.post(NetEvent::Party { party: None, why });
        // (the page has no party now: no follow either)
        self.notes.push(SessionNote::Followed(None));
        // (a queue search of the party's is over with it)
        self.notes.push(SessionNote::Out(QueueWhy::Party));
    }

    fn let_go(&mut self, cx: &mut dyn PartyCx, peers: &[String]) {
        if self.inside && !self.lobby.is_empty() {
            let lobby = self.lobby.clone();
            self.leave(cx, &lobby);
        }
        self.inside = false;
        for id in peers {
            if !cx.peer_used(id) {
                cx.close_peer(id);
            }
        }
    }

    fn leave(&mut self, cx: &mut dyn PartyCx, lobby: &str) {
        if let Some(ask) = cx.leave_lobby(lobby) {
            self.asks.push((ask, PartyAsk::Leave));
        }
    }

    fn set_followed(&mut self, f: Option<PartyFollow>) {
        if self.followed != f {
            self.followed = f.clone();
            self.notes.push(SessionNote::Followed(f));
        }
    }

    fn send(&mut self, cx: &mut dyn PartyCx, to: &str, m: &PartyWire) {
        let body = serde_json::to_vec(m).expect("a party message is JSON");
        if body.len() + 1 > P2P_MAX_PACKET {
            return cx.log(&format!("party: a message too long {}", m.kind()));
        }
        let mut p = Vec::with_capacity(body.len() + 1);
        p.push(FRAME_PARTY);
        p.extend_from_slice(&body);
        let sent = cx.send(to, Channel::Messages, &p);
        if !sent {
            cx.log(&format!("party: a packet refused {to}"));
        }
    }
}

/// The party's PartyCx on the service: the relay's calls, the page's events.
struct SvcCx<'a> {
    /// The relay the party last saw (its last leave and close go to it after the relay went off, as the TS's party
    /// keeps its eos).
    held: Option<Shared>,
    svc: &'a mut NetService,
    page: PageId,
}

const VIA: Via = Via::Backend(BackendKind::Relay);

impl SvcCx<'_> {
    fn with<R>(&mut self, f: impl FnOnce(&mut dyn crate::backend::Backend) -> R) -> Option<R> {
        let b = self.held.clone()?;
        let mut g = lock(&b);
        Some(f(g.as_mut()))
    }
}

impl PartyCx for SvcCx<'_> {
    fn create_lobby(&mut self, bucket: &str, max: u32) -> Option<Ask> {
        self.with(|b| b.create_lobby(bucket, max, None))
    }
    fn join_lobby(&mut self, lobby: &str) -> Option<Ask> {
        self.with(|b| b.join_lobby(lobby))
    }
    fn leave_lobby(&mut self, lobby: &str) -> Option<Ask> {
        self.with(|b| b.leave_lobby(lobby))
    }
    fn kick(&mut self, lobby: &str, member: &str) {
        self.with(|b| b.kick(lobby, member));
    }
    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        self.with(|b| b.send(to, channel, bytes)).unwrap_or(false)
    }
    fn close_peer(&mut self, peer: &str) {
        self.with(|b| b.close_peer(peer));
    }
    fn invite(&mut self, to: &str, payload: &str) -> Option<Ask> {
        self.with(|b| b.invite(to, payload))
    }
    fn post(&mut self, e: NetEvent) {
        self.svc.post(self.page, e);
    }
    fn card(&self) -> (String, String) {
        self.svc.card()
    }
    fn fighter(&self) -> Option<MenuFighter> {
        self.svc.menu_fighter()
    }
    fn may_invite(&mut self, to: &str, now: f64) -> bool {
        self.svc.may_invite(to, now)
    }
    fn code_on(&self, lobby: &str) -> Result<String, ClosedWhy> {
        self.svc.code_on(BackendKind::Relay, lobby).map_err(|e| e.closed(true))
    }
    fn peer_used(&self, peer: &str) -> bool {
        self.svc.peer_used(VIA, peer, self.page)
    }
    fn log(&mut self, text: &str) {
        self.svc.log(text);
    }
}

// The service's calls (the page's party taken out of its page for each).
impl Party {
    // the relay now, else the one it last saw
    fn hold(&mut self, svc: &NetService) -> Option<Shared> {
        if let Some(b) = svc.backend(VIA) {
            self.held = Some(b);
        }
        self.held.clone()
    }

    /// The party made by this page's invite, this player its leader, on the relay.
    pub fn lead(svc: &mut NetService, page: PageId, _now: f64) -> Party {
        let me = svc.me_on(VIA);
        let held = svc.backend(VIA);
        let mut p = Party::lead_cx(&mut SvcCx { held: held.clone(), svc, page }, page, &me);
        p.held = held;
        p
    }

    /// Joining the party of a received invite's code: its relay lobby, its code (directText), its inviter.
    pub fn joining(svc: &mut NetService, page: PageId, lobby: &str, code: &str, inviter: &str, _now: f64) -> Party {
        let me = svc.me_on(VIA);
        let held = svc.backend(VIA);
        let mut p = Party::joining_cx(&mut SvcCx { held: held.clone(), svc, page }, page, &me, lobby, code, inviter);
        p.held = held;
        p
    }

    /// What its packets go through (the relay's).
    pub fn via(&self) -> Via {
        VIA
    }

    /// Its code (directText).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Its relay lobby ('' none yet).
    pub fn lobby(&self) -> &str {
        &self.lobby
    }

    pub fn invite(&mut self, svc: &mut NetService, to: &str, name: &str, now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.invite_cx(&mut SvcCx { held, svc, page }, to, name, now);
    }

    /// This player's presence changed.
    pub fn card_changed(&mut self, svc: &mut NetService, _now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.card_changed_cx(&mut SvcCx { held, svc, page });
    }

    /// A page's say (read as partySayOf reads it).
    pub fn say(&mut self, svc: &mut NetService, say: PartySay, _now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.say_cx(&mut SvcCx { held, svc, page }, &say);
    }

    /// A FRAME_PARTY packet from a peer it has.
    pub fn message(&mut self, svc: &mut NetService, from: &str, bytes: &[u8], now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.message_cx(&mut SvcCx { held, svc, page }, from, bytes, now);
    }

    pub fn lobby_changed(&mut self, svc: &mut NetService, c: &LobbyChange, now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.lobby_changed_cx(&mut SvcCx { held, svc, page }, c, now);
    }

    pub fn peer_closed(&mut self, svc: &mut NetService, peer: &str, _now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.peer_closed_cx(&mut SvcCx { held, svc, page }, peer);
    }

    /// This player leaves it (`Party` None with why to the page; `Left` the player's own leave).
    pub fn end(&mut self, svc: &mut NetService, why: ClosedWhy, now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.end_cx(&mut SvcCx { held, svc, page }, why, now);
    }

    /// It is over from the other side (its relay lost).
    pub fn lost(&mut self, svc: &mut NetService, why: ClosedWhy, _now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.lost_cx(&mut SvcCx { held, svc, page }, why);
    }

    /// Over: the service takes it out of its page, and pumps it on until `next_due` is None (its lobby's leave
    /// BYE_MS after its bye).
    pub fn over(&self) -> bool {
        self.ended
    }

    /// The backend of `via` is gone: its asks are let go.
    pub fn forget(&mut self, via: Via) {
        if via == VIA {
            self.asks.clear();
        }
    }

    /// A backend's answer: true when the call was this party's.
    pub fn answered(&mut self, svc: &mut NetService, via: Via, ask: Ask, r: &Result<Answer, String>, now: f64) -> bool {
        if via != VIA {
            return false;
        }
        let page = self.page;
        let held = self.hold(svc);
        self.answered_cx(&mut SvcCx { held, svc, page }, ask, r, now)
    }

    pub fn pump(&mut self, svc: &mut NetService, now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        self.pump_cx(&mut SvcCx { held, svc, page }, now);
    }

    /// A note of the page's queue session.
    pub fn note(&mut self, svc: &mut NetService, n: PartyNote, _now: f64) {
        let page = self.page;
        let held = self.hold(svc);
        let cx = &mut SvcCx { held, svc, page };
        self.note_cx(cx, n);
    }

    pub(crate) fn note_cx(&mut self, cx: &mut dyn PartyCx, n: PartyNote) {
        match n {
            PartyNote::SendFollow(f) => self.send_follow(cx, &f),
            PartyNote::QueueOut(why) => self.queue_out(cx, why),
            PartyNote::Unfollowed(lobby) => self.unfollowed(cx, &lobby),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PartyCx that records what the party did and answers nothing by itself.
    #[derive(Default)]
    struct Fake {
        next: u32,
        calls: Vec<String>,
        sent: Vec<(String, Vec<u8>)>,
        posts: Vec<NetEvent>,
        fighter: Option<MenuFighter>,
    }

    impl Fake {
        fn ask(&mut self, what: String) -> Option<Ask> {
            self.next += 1;
            self.calls.push(what);
            Some(Ask(self.next))
        }

        /// The party packets sent to `to`, read back.
        fn wires_to(&self, to: &str) -> Vec<PartyWire> {
            self.sent.iter().filter(|(t, _)| t == to).filter_map(|(_, b)| party_wire_of(b)).collect()
        }
    }

    impl PartyCx for Fake {
        fn create_lobby(&mut self, bucket: &str, max: u32) -> Option<Ask> {
            self.ask(format!("create {bucket} {max}"))
        }
        fn join_lobby(&mut self, lobby: &str) -> Option<Ask> {
            self.ask(format!("join {lobby}"))
        }
        fn leave_lobby(&mut self, lobby: &str) -> Option<Ask> {
            self.ask(format!("leave {lobby}"))
        }
        fn kick(&mut self, lobby: &str, member: &str) {
            self.calls.push(format!("kick {lobby} {member}"));
        }
        fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
            assert_eq!(channel, Channel::Messages);
            assert_eq!(bytes[0], FRAME_PARTY);
            self.sent.push((to.to_string(), bytes.to_vec()));
            true
        }
        fn close_peer(&mut self, peer: &str) {
            self.calls.push(format!("close {peer}"));
        }
        fn invite(&mut self, to: &str, payload: &str) -> Option<Ask> {
            self.ask(format!("invite {to} {payload}"))
        }
        fn post(&mut self, e: NetEvent) {
            self.posts.push(e);
        }
        fn card(&self) -> (String, String) {
            ("Me".into(), String::new())
        }
        fn fighter(&self) -> Option<MenuFighter> {
            self.fighter.clone()
        }
        fn may_invite(&mut self, _to: &str, _now: f64) -> bool {
            true
        }
        fn code_on(&self, lobby: &str) -> Result<String, ClosedWhy> {
            Ok(format!("CODE-{lobby}"))
        }
        fn peer_used(&self, _peer: &str) -> bool {
            false
        }
        fn log(&mut self, _text: &str) {}
    }

    fn wire(m: &PartyWire) -> Vec<u8> {
        let mut b = vec![FRAME_PARTY];
        b.extend(serde_json::to_vec(m).unwrap());
        b
    }

    fn raw(text: &str) -> Vec<u8> {
        let mut b = vec![FRAME_PARTY];
        b.extend(text.as_bytes());
        b
    }

    #[test]
    fn wire_reads_field_by_field() {
        let hi = party_wire_of(&raw(r#"{"k":"hi","name":"Bo","icon":"no such icon!"}"#));
        assert_eq!(hi, Some(PartyWire::Hi { name: "Bo".into(), icon: String::new(), fighter: None }));
        let long = "x".repeat(ID_MAX + 1);
        assert_eq!(party_wire_of(&raw(&format!(r#"{{"k":"follow","lobby":"{long}"}}"#))), None);
        let f = party_wire_of(&raw(r#"{"k":"follow","lobby":"L","modes":["2v2","1v1","no"]}"#));
        assert_eq!(
            f,
            Some(PartyWire::Follow { lobby: "L".into(), modes: modes_of(&serde_json::json!(["1v1", "2v2"])) })
        );
        assert_eq!(
            party_wire_of(&raw(r#"{"k":"out","why":"partyLate"}"#)),
            Some(PartyWire::Out { why: QueueWhy::PartyLate })
        );
        assert_eq!(party_wire_of(&raw(r#"{"k":"out"}"#)), Some(PartyWire::Out { why: QueueWhy::Party }));
        assert_eq!(party_wire_of(&raw(r#"{"k":"nope"}"#)), None);
        assert_eq!(party_wire_of(&raw("not json")), None);
        assert_eq!(party_wire_of(&[]), None);
        // (a bye's why: a code read as one, else words kept as a chat line)
        let bye = PartyWire::Bye { why: ClosedWhy::QueueMoved };
        assert_eq!(party_wire_of(&wire(&bye)), Some(bye));
        let text = party_wire_of(&raw(r#"{"k":"bye","why":"the host left"}"#));
        assert_eq!(text, Some(PartyWire::Bye { why: ClosedWhy::Other { text: "the host left".into() } }));
    }

    #[test]
    fn says_read_as_the_ts_reads_them() {
        let q = party_say_of(&serde_json::json!({"k":"queue","modes":["2v2","1v1"]}));
        assert_eq!(q, Some(PartySay::Queue { modes: modes_of(&serde_json::json!(["1v1", "2v2"])) }));
        assert_eq!(party_say_of(&serde_json::json!({"k":"queue","modes":[]})), None);
        assert_eq!(party_say_of(&serde_json::json!({"k":"custom","code":""})), None);
        assert_eq!(party_say_of(&serde_json::json!({"k":"custom","code":"x".repeat(CODE_MAX + 1)})), None);
        assert_eq!(party_say_of(&serde_json::json!({"k":"home"})), Some(PartySay::Home));
        assert!(by_member(&PartySay::Ready) && !by_member(&PartySay::Custom { code: "c".into() }));
    }

    /// The leader's make, an invite that waits for it, the member's join and knock, the leader's answer, a say, and
    /// the member's leave.
    #[test]
    fn lead_invite_join_link_leave() {
        let (mut lc, mut mc) = (Fake::default(), Fake::default());
        let mut lead = Party::lead_cx(&mut lc, PageId(1), "A");
        assert_eq!(lc.calls, ["create mvs:party 2"]);
        lead.invite_cx(&mut lc, "B", "Bee", 0.0);
        assert_eq!(lc.calls.len(), 1, "the invite waits for the make");
        assert!(lead.answered_cx(&mut lc, Ask(1), &Ok(Answer::Created("L1".into())), 0.0));
        assert_eq!(lead.code, "CODE-L1");
        assert_eq!(lc.calls[1], r#"invite B {"kind":"party","code":"CODE-L1","name":"Bee"}"#);
        assert!(
            matches!(&lc.posts[0], NetEvent::Party { party: Some(p), why: ClosedWhy::Left } if p.members.len() == 1)
        );

        let mut mem = Party::joining_cx(&mut mc, PageId(2), "B", "L1", "CODE-L1", "A");
        assert_eq!(mc.calls, ["join L1"]);
        mem.answered_cx(
            &mut mc,
            Ask(1),
            &Ok(Answer::Joined { owner: "A".into(), members: vec!["A".into(), "B".into()], lobby: None }),
            0.0,
        );
        assert_eq!(mc.wires_to("A"), [PartyWire::Hi { name: "Me".into(), icon: String::new(), fighter: None }]);
        assert_eq!(mem.next_due(), Some(OPEN_EVERY_MS));

        // (the leader sees B in its lobby, then its hi: linked, answered with its own)
        let members = vec!["A".to_string(), "B".to_string()];
        lead.lobby_changed_cx(&mut lc, &LobbyChange::Members { lobby: "L1".into(), members }, 10.0);
        assert!(lead.waiting.contains_key("B"));
        let (to, hi) = mc.sent.remove(0);
        assert_eq!(to, "A");
        lead.message_cx(&mut lc, "B", &hi, 20.0);
        assert!(lead.waiting.is_empty() && lead.cards.contains_key("B"));
        assert_eq!(lead.take_notes(), [SessionNote::Changed]);
        assert_eq!(lead.peers(), ["B"]);
        let (to, back) = lc.sent.remove(0);
        assert_eq!(to, "B");
        mem.message_cx(&mut mc, "A", &back, 30.0);
        assert!(mem.linked && mem.next_due().is_none());
        assert!(
            matches!(mc.posts.last(), Some(NetEvent::Party { party: Some(p), .. }) if p.members[0].id == "A" && p.members[1].id == "B")
        );

        // (a say of the leader's reaches the member's page; a Queue say of the member's is not the member's to make)
        let say = PartyWire::Say { say: PartySay::Ready };
        mem.message_cx(&mut mc, "A", &wire(&say), 40.0);
        assert_eq!(mc.posts.last(), Some(&NetEvent::PartySay { from: "A".into(), say: PartySay::Ready }));
        let n = lc.posts.len();
        let q = PartyWire::Say { say: PartySay::Queue { modes: modes_of(&serde_json::json!(["1v1"])) } };
        lead.message_cx(&mut lc, "B", &wire(&q), 41.0);
        assert_eq!(lc.posts.len(), n);

        // (the member leaves: its bye to the leader, the lobby left BYE_MS later; the leader drops it)
        mem.end_cx(&mut mc, ClosedWhy::Left, 50.0);
        assert!(matches!(mc.posts.last(), Some(NetEvent::Party { party: None, why: ClosedWhy::Left })));
        let (_, bye) = mc.sent.last().cloned().unwrap();
        assert_eq!(party_wire_of(&bye), Some(PartyWire::Bye { why: ClosedWhy::Left }));
        assert_eq!(mem.next_due(), Some(50.0 + BYE_MS));
        mem.pump_cx(&mut mc, 50.0 + BYE_MS);
        assert_eq!(mc.calls.last().unwrap(), "close A");
        assert!(mc.calls.iter().any(|c| c == "leave L1"));
        lead.message_cx(&mut lc, "B", &bye, 60.0);
        assert!(lc.calls.iter().any(|c| c == "kick L1 B"));
        assert_eq!(lead.take_notes(), [SessionNote::Changed, SessionNote::Out(QueueWhy::Party)]);
        assert!(lead.peers().is_empty());
    }

    /// The main-menu fighters on the cards: the member's on its knock, the leader's on its answer, each told to the
    /// other's page once (`PartyFighter`) and again at a change (`card_changed`) without a `Party`; a card with none
    /// (an older game's) has the TS's bytes and tells nothing; one not an asset's names is read as none.
    #[test]
    fn cards_carry_the_main_menu_fighter() {
        let ww = MenuFighter { key: "wonderwoman".into(), skin: String::new() };
        let bat = MenuFighter { key: "batman".into(), skin: "batman_S01".into() };
        let (mut lc, mut mc) = (Fake { fighter: Some(bat.clone()), ..Fake::default() }, Fake::default());
        mc.fighter = Some(ww.clone());
        let mut lead = Party::lead_cx(&mut lc, PageId(1), "A");
        lead.invite_cx(&mut lc, "B", "Bee", 0.0);
        lead.answered_cx(&mut lc, Ask(1), &Ok(Answer::Created("L1".into())), 0.0);
        let mut mem = Party::joining_cx(&mut mc, PageId(2), "B", "L1", "CODE-L1", "A");
        let joined = Answer::Joined { owner: "A".into(), members: vec!["A".into(), "B".into()], lobby: None };
        mem.answered_cx(&mut mc, Ask(1), &Ok(joined), 0.0);
        let members = vec!["A".to_string(), "B".to_string()];
        lead.lobby_changed_cx(&mut lc, &LobbyChange::Members { lobby: "L1".into(), members }, 10.0);
        let (_, hi) = mc.sent.remove(0);
        assert!(String::from_utf8_lossy(&hi).contains(r#""fighter":{"key":"wonderwoman","skin":""}"#));
        lead.message_cx(&mut lc, "B", &hi, 20.0);
        assert_eq!(lc.posts.last(), Some(&NetEvent::PartyFighter { from: "B".into(), fighter: Some(ww.clone()) }));
        let (_, back) = lc.sent.remove(0);
        mem.message_cx(&mut mc, "A", &back, 30.0);
        assert_eq!(mc.posts.last(), Some(&NetEvent::PartyFighter { from: "A".into(), fighter: Some(bat.clone()) }));

        // (the same card again tells nothing; a new fighter alone tells the fighter, not the party)
        let n = lc.posts.len();
        lead.message_cx(&mut lc, "B", &hi, 31.0);
        assert_eq!(lc.posts.len(), n);
        mc.fighter = Some(bat.clone());
        mem.card_changed_cx(&mut mc);
        let (_, hi) = mc.sent.pop().unwrap();
        lead.message_cx(&mut lc, "B", &hi, 32.0);
        assert_eq!(lc.posts[n..], [NetEvent::PartyFighter { from: "B".into(), fighter: Some(bat.clone()) }]);

        // (an older game's card: the TS's bytes, nothing told; a bad fighter is none)
        let old = Card { name: "Me".into(), icon: String::new(), fighter: None }.hi();
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"k":"hi","name":"Me","icon":""}"#);
        let bad = party_wire_of(&raw(r#"{"k":"hi","name":"Bo","icon":"","fighter":{"key":"../x","skin":""}}"#));
        assert_eq!(bad, Some(PartyWire::Hi { name: "Bo".into(), icon: String::new(), fighter: None }));
        let no_skin = party_wire_of(&raw(r#"{"k":"hi","name":"Bo","icon":"","fighter":{"key":"taz"}}"#));
        let taz = MenuFighter { key: "taz".into(), skin: String::new() };
        assert_eq!(no_skin, Some(PartyWire::Hi { name: "Bo".into(), icon: String::new(), fighter: Some(taz) }));
    }

    /// The leader's bye reaches the member as the party's end by its why; a plain leave as LeaderLeft.
    #[test]
    fn leader_bye_whys() {
        for (why, seen) in [
            (ClosedWhy::Left, ClosedWhy::LeaderLeft),
            (ClosedWhy::QueueMoved, ClosedWhy::PartyEndedBy { why: Box::new(ClosedWhy::QueueMoved) }),
        ] {
            let mut mc = Fake::default();
            let mut mem = Party::joining_cx(&mut mc, PageId(2), "B", "L1", "C", "A");
            mem.answered_cx(
                &mut mc,
                Ask(1),
                &Ok(Answer::Joined { owner: "A".into(), members: vec!["A".into()], lobby: None }),
                0.0,
            );
            mem.message_cx(&mut mc, "A", &wire(&PartyWire::Bye { why }), 1.0);
            assert_eq!(mc.posts.last(), Some(&NetEvent::Party { party: None, why: seen }));
            assert!(mem.over());
        }
    }

    /// A join answered by a lobby of another than the inviter is not that party; a silent leader ends it after
    /// OPEN_FOR_MS.
    #[test]
    fn join_refusals() {
        let mut mc = Fake::default();
        let mut mem = Party::joining_cx(&mut mc, PageId(2), "B", "L1", "C", "A");
        mem.answered_cx(&mut mc, Ask(1), &Ok(Answer::Joined { owner: "Z".into(), members: vec![], lobby: None }), 0.0);
        let why = ClosedWhy::PartyJoinFailed { why: PartyJoinWhy::NotInviter };
        assert_eq!(mc.posts.last(), Some(&NetEvent::Party { party: None, why }));

        let mut mc = Fake::default();
        let mut mem = Party::joining_cx(&mut mc, PageId(2), "B", "L1", "C", "A");
        mem.answered_cx(&mut mc, Ask(1), &Ok(Answer::Joined { owner: "A".into(), members: vec![], lobby: None }), 0.0);
        mem.pump_cx(&mut mc, OPEN_FOR_MS);
        assert_eq!(mc.posts.last(), Some(&NetEvent::Party { party: None, why: ClosedWhy::LeaderSilent }));
        assert_eq!(mc.wires_to("A").len(), 10, "a knock each OPEN_EVERY_MS until OPEN_FOR_MS");
    }

    /// The leader kicks a player it did not invite, and one that never linked by KNOCK_WAIT_MS.
    #[test]
    fn leader_kicks() {
        let mut lc = Fake::default();
        let mut lead = Party::lead_cx(&mut lc, PageId(1), "A");
        lead.answered_cx(&mut lc, Ask(1), &Ok(Answer::Created("L1".into())), 0.0);
        lead.invite_cx(&mut lc, "B", "B", 0.0);
        let members = vec!["A".to_string(), "B".to_string(), "X".to_string()];
        lead.lobby_changed_cx(&mut lc, &LobbyChange::Members { lobby: "L1".into(), members }, 0.0);
        assert!(lc.calls.iter().any(|c| c == "kick L1 X"));
        lead.pump_cx(&mut lc, KNOCK_WAIT_MS);
        assert_eq!(lc.calls.last().unwrap(), "kick L1 B");
    }
}
