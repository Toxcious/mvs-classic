//! The online server's protocol shapes: the messages that carry the server's choices, and the
//! compatibility id that decides who may play together. The server holds no game data: it chooses among ids, indexes
//! and seeds, and each game turns them into the same map, bot cosmetics and bot perks from its own tables.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- the compatibility id ----------

/// The longest id a join may carry.
pub const COMPAT_MAX: usize = 64;

/// A join's compatibility id as sent: '' when it is missing or not one (such a join is refused).
pub fn compat_of(v: &Value) -> String {
    match v.as_str() {
        Some(s)
            if !s.is_empty()
                && s.len() <= COMPAT_MAX
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) =>
        {
            s.to_owned()
        }
        _ => String::new(),
    }
}

// ---------- the app's version (hello.app) ----------

/// The longest version a hello may carry.
pub const APP_MAX: usize = 32;

/// A hello's app version as sent: a release's semver ('1.2.3', '1.2.3-beta.1'), 'dev' from a browser; anything
/// else is ''.
pub fn app_of(v: &Value) -> String {
    match v.as_str() {
        Some(s)
            if !s.is_empty()
                && s.len() <= APP_MAX
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-')) =>
        {
            s.to_owned()
        }
        _ => String::new(),
    }
}

/// A release's three numbers: the version before any '+' and '-', as `\d+.\d+.\d+` (JavaScript numbers: a long run of
/// digits rounds as a double).
fn release(v: &str) -> Option<[f64; 3]> {
    let core = v.split('+').next().unwrap_or("").split('-').next().unwrap_or("");
    let mut parts = core.split('.');
    let mut out = [0.0; 3];
    for x in &mut out {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *x = p.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// Below 0 when release a is older than b, above 0 when newer, 0 when they are the same or either is no release.
pub fn app_order(a: &str, b: &str) -> f64 {
    let (Some(x), Some(y)) = (release(a), release(b)) else { return 0.0 };
    for i in 0..3 {
        if x[i] != y[i] {
            return x[i] - y[i];
        }
    }
    0.0
}

/// The note a player gets when their compatibility id differs from another player's: the older release is the one
/// told to update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatNote {
    /// No release order to go by.
    Differs,
    /// The player's own game is the older release.
    MineOlder,
    /// The other player's game is the older release.
    TheirsOlder,
}

/// The note for the player of release `mine` against `theirs` (compatNote).
pub fn compat_note(mine: &str, theirs: &str) -> CompatNote {
    let order = app_order(mine, theirs);
    if order < 0.0 {
        CompatNote::MineOlder
    } else if order > 0.0 {
        CompatNote::TheirsOlder
    } else {
        CompatNote::Differs
    }
}

impl From<CompatNote> for crate::messages::LobbyErrorWhy {
    fn from(n: CompatNote) -> Self {
        match n {
            CompatNote::Differs => crate::messages::LobbyErrorWhy::CompatDiffers,
            CompatNote::MineOlder => crate::messages::LobbyErrorWhy::CompatMineOlder,
            CompatNote::TheirsOlder => crate::messages::LobbyErrorWhy::CompatTheirsOlder,
        }
    }
}

// ---------- the server's choices ----------

/// A seed or a pick the server draws: an integer in [1, SEED_MAX].
pub const SEED_MAX: u32 = 0x7fff_ffff;

/// A queue's mode. Ffa: four players, each on a team of their own (ETeamStyle 2; the build's FFA card's queue,
/// rotation UnrankedFreeForAll). Chaos (the port's own, in the build's Casual card's place: MatchKey 2v2casual,
/// queue type 3, rotation Casual): two teams of two (ETeamStyle 1) with the Chaos Wheel at its default settings; the
/// rule is the game's, never the wire's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QueueMode {
    #[serde(rename = "1v1")]
    OneVOne,
    #[serde(rename = "2v2")]
    TwoVTwo,
    #[serde(rename = "ffa")]
    Ffa,
    #[serde(rename = "chaos")]
    Chaos,
}

impl QueueMode {
    /// The mode's wire name.
    pub fn id(self) -> &'static str {
        match self {
            QueueMode::OneVOne => "1v1",
            QueueMode::TwoVTwo => "2v2",
            QueueMode::Ffa => "ffa",
            QueueMode::Chaos => "chaos",
        }
    }

    /// A wire name read back.
    pub fn of(s: &str) -> Option<QueueMode> {
        match s {
            "1v1" => Some(QueueMode::OneVOne),
            "2v2" => Some(QueueMode::TwoVTwo),
            "ffa" => Some(QueueMode::Ffa),
            "chaos" => Some(QueueMode::Chaos),
            _ => None,
        }
    }
}

/// A party in the queue (queue.join `party`): the relay ids of its members, the joining player's among them, the
/// leader first. Every member's join names the same list; the server holds the joins of one list as one ticket.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueParty {
    pub members: Vec<String>,
}

/// The most players in a party.
pub const PARTY_MAX: usize = 2;

/// A cosmetic as the pre-match takes it: its slug and its asset's name (the path's basename, '' with none).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticRef {
    pub slug: String,
    pub asset: String,
}

/// A stat-tracker badge: CharacterSelectPlayerData StatTrackerSlug / StatTrackerCount (and 2, 3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BadgeRef {
    pub slug: String,
    pub asset: String,
    pub count: u32,
}

/// What the peers' pre-match screens get per player: a human's as its game sent them, a bot's drawn by each game
/// from the bot's seed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerCosmetics {
    pub banner: CosmeticRef,
    /// Slots 0-2.
    pub badges: Vec<BadgeRef>,
    pub icon: CosmeticRef,
}

/// A player of a found match (queue.found) or of a pre-match (pm.begin). `id`: a human's public id, a bot's
/// "bot1".."bot3". `skin`: '' is the character's default skin (a queue bot's always). A bot carries `seed` alone of
/// what is drawn for it: each game draws its cosmetics and ring-out from it and takes its perks from its own
/// recommended perks, filling `perks`, `cosmetics` and `ring_out` on receipt. A human's `cosmetics` are those its
/// game sent; it has no seed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuePlayer {
    pub id: String,
    pub name: String,
    pub team: u32,
    pub character: String,
    pub skin: String,
    pub bot: bool,
    /// A bot's draw (1..SEED_MAX).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    /// A queue bot's perk slots, filled by the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perks: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cosmetics: Option<PlayerCosmetics>,
    /// A bot's ring-out (RingOutVfxData asset name), filled by the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_out: Option<String>,
}

/// queue.found as the server sends it: `players[i]` is fighter slot i, `me` the receiving game's slot; the map is the
/// mode's pool entry `map_pick % pool length`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueFoundMsg {
    #[serde(rename = "match")]
    pub match_id: String,
    pub mode: QueueMode,
    pub map_pick: u32,
    pub players: Vec<QueuePlayer>,
    pub me: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmPick {
    pub character: String,
    pub skin: String,
}

/// A pre-match's kind: a custom lobby's start, or a queue set's rematch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PmKind {
    Lobby,
    Queue,
}

/// A queue set's rematch: the number of the game it starts (2 or 3) and the wins by team so far.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmRematch {
    pub game: u32,
    pub wins: Vec<u32>,
}

/// pm.begin: `players[i]` is fighter slot i (a human's character and skin '' until pm.all, a bot's resolved by the
/// server to the roster, with its seed); `map` '' until pm.all. `mode`: a queue's (its pool names the map of the
/// pick), a lobby's None (null on the wire).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PmBegin {
    pub id: String,
    pub kind: PmKind,
    pub mode: Option<QueueMode>,
    pub team_style: u8,
    pub map: String,
    pub players: Vec<QueuePlayer>,
    pub me: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rematch: Option<PmRematch>,
}

/// match.start of a pre-match (`match_id` its id). A bot's slot in `setup` has no ring-out or banner and its `perks`
/// entry is empty: each game puts in those it drew from the bot's seed. A queue match names its map by `map_pick`, a
/// lobby's by the MapData name in `map`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchStartMsg<S> {
    pub seed: u32,
    pub setup: S,
    pub slot: u32,
    pub usernames: Vec<String>,
    pub map: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_pick: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
    #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perks: Option<Vec<Vec<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compat_and_app() {
        assert_eq!(compat_of(&json!("abc.1-2_3")), "abc.1-2_3");
        assert_eq!(compat_of(&json!("")), "");
        assert_eq!(compat_of(&json!("a b")), "");
        assert_eq!(compat_of(&json!("x".repeat(65))), "");
        assert_eq!(compat_of(&json!(5)), "");
        assert_eq!(app_of(&json!("1.2.3-beta.1+x")), "1.2.3-beta.1+x");
        assert_eq!(app_of(&json!("dev")), "dev");
        assert_eq!(app_of(&json!("1_2")), "");
    }

    #[test]
    fn order_and_note() {
        assert_eq!(app_order("1.2.3", "1.3.0"), -1.0);
        assert_eq!(app_order("2.0.0-beta.1", "1.9.9"), 1.0);
        assert_eq!(app_order("dev", "1.0.0"), 0.0);
        assert_eq!(app_order("1.0", "1.0.0"), 0.0);
        assert_eq!(compat_note("1.0.0", "1.0.1"), CompatNote::MineOlder);
        assert_eq!(compat_note("1.0.2", "1.0.1"), CompatNote::TheirsOlder);
        assert_eq!(compat_note("dev", "1.0.1"), CompatNote::Differs);
    }

    #[test]
    fn wire_shapes() {
        let b = PmBegin {
            id: "p".into(),
            kind: PmKind::Lobby,
            mode: None,
            team_style: 1,
            map: String::new(),
            players: vec![],
            me: 0,
            rematch: None,
        };
        assert_eq!(
            serde_json::to_string(&b).unwrap(),
            r#"{"id":"p","kind":"lobby","mode":null,"teamStyle":1,"map":"","players":[],"me":0}"#
        );
        assert_eq!(serde_json::to_string(&QueueMode::OneVOne).unwrap(), r#""1v1""#);
    }
}
