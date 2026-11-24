//! What one relay allows: a relay is a service on the internet, so every count and rate a player
//! can push on has a bound, set in its config. [`TokenBucket`] is the rate limiter each uses.

use indexmap::IndexMap;
use mvs_protocol::relay_wire::{HELLO_MS, MAX_FRAME, SILENT_MS, normal_address};

/// EOS_P2P_MAX_PACKET_SIZE: the most bytes one packet carries on the online interface, so a relay never needs more.
pub const P2P_MAX_PACKET: usize = 1170;

/// `RelayLimits`, its config's field names.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RelayLimits {
    /// Players signed in at once.
    pub players: f64,
    /// TCP connections open at once from one address (a handshake not yet done counts).
    pub connections_per_address: f64,
    /// Lobbies one player is a member of at once (its own included).
    pub lobbies_per_player: f64,
    /// Lobbies owned at once by the players of one address (an IPv6 one by its /64).
    pub lobbies_per_address: f64,
    /// Joins per player per minute, and of the players of one address.
    pub joins_per_minute: f64,
    pub joins_per_address: f64,
    /// The most a lobby's `max` may be.
    pub members_per_lobby: f64,
    /// Matches refereed on the relay at once, of all its host cores.
    pub matches: f64,
    /// Bounds nothing: the relay's own host cores open its matches, no player does. Still read from a config, so one
    /// written for a relay its hosts opened matches on starts as it did.
    pub matches_per_address: f64,
    /// Searches per player per minute.
    pub searches_per_minute: f64,
    /// Invites per player per minute, of the players of one address, and to one player (half a minute of it at once).
    pub invites_per_minute: f64,
    pub invites_per_address: f64,
    pub invites_to_player: f64,
    /// After an invite from one player to another, the next of that pair is refused for this long (ms).
    pub invite_again_ms: f64,
    /// A player's presence changes at most once in this long (ms): a change sooner is refused.
    pub presence_every_ms: f64,
    /// Online list asks ('who') per player per minute, and of the players of one address.
    pub who_per_minute: f64,
    pub who_per_address: f64,
    /// Control (JSON) frames per player per second.
    pub control_per_second: f64,
    /// Bytes per second one player may send on channel 0 (messages) and channel 1 (a match's packets; this to each
    /// player it sent one in the last [`crate::core::MATCH_FAN_MS`], [`mvs_protocol::match_wire::MAX_SLOTS`] - 1 of them
    /// at the most: a game that referees its own match sends every other human of it).
    pub message_bytes_per_second: f64,
    pub match_bytes_per_second: f64,
    /// The longest TCP frame (type byte and body), at most MAX_FRAME.
    pub frame_bytes: f64,
    /// The longest packet on either channel.
    pub packet_bytes: f64,
    /// The hello and the prove must come within this of the connection's start (ms).
    pub hello_ms: f64,
    /// A connection that sends nothing for this long is dropped (ms).
    pub silent_ms: f64,
    /// Bytes waiting to be written to one player's socket: past it the player reads too slowly and is dropped.
    pub send_buffer_bytes: f64,
}

/// `DEFAULT_LIMITS` (a relay is a public service: each number bounds what one player may cost it).
pub const DEFAULT_LIMITS: RelayLimits = RelayLimits {
    players: 500.0,
    connections_per_address: 4.0,
    lobbies_per_player: 4.0,
    lobbies_per_address: 8.0,
    joins_per_minute: 20.0,
    joins_per_address: 20.0,
    members_per_lobby: 8.0,
    matches: 250.0,
    matches_per_address: 16.0,
    searches_per_minute: 60.0,
    invites_per_minute: 20.0,
    invites_per_address: 30.0,
    invites_to_player: 10.0,
    invite_again_ms: 15000.0,
    presence_every_ms: 1000.0,
    who_per_minute: 12.0,
    who_per_address: 30.0,
    control_per_second: 50.0,
    message_bytes_per_second: 128.0 * 1024.0,
    match_bytes_per_second: 256.0 * 1024.0,
    frame_bytes: MAX_FRAME as f64,
    packet_bytes: P2P_MAX_PACKET as f64,
    hello_ms: HELLO_MS,
    silent_ms: SILENT_MS,
    send_buffer_bytes: 1_048_576.0,
};

impl Default for RelayLimits {
    fn default() -> RelayLimits {
        DEFAULT_LIMITS
    }
}

impl RelayLimits {
    /// Each limit's config name and a handle on its number, in the config's order.
    pub fn fields_mut(&mut self) -> [(&'static str, &mut f64); 25] {
        [
            ("players", &mut self.players),
            ("connectionsPerAddress", &mut self.connections_per_address),
            ("lobbiesPerPlayer", &mut self.lobbies_per_player),
            ("lobbiesPerAddress", &mut self.lobbies_per_address),
            ("joinsPerMinute", &mut self.joins_per_minute),
            ("joinsPerAddress", &mut self.joins_per_address),
            ("membersPerLobby", &mut self.members_per_lobby),
            ("matches", &mut self.matches),
            ("matchesPerAddress", &mut self.matches_per_address),
            ("searchesPerMinute", &mut self.searches_per_minute),
            ("invitesPerMinute", &mut self.invites_per_minute),
            ("invitesPerAddress", &mut self.invites_per_address),
            ("invitesToPlayer", &mut self.invites_to_player),
            ("inviteAgainMs", &mut self.invite_again_ms),
            ("presenceEveryMs", &mut self.presence_every_ms),
            ("whoPerMinute", &mut self.who_per_minute),
            ("whoPerAddress", &mut self.who_per_address),
            ("controlPerSecond", &mut self.control_per_second),
            ("messageBytesPerSecond", &mut self.message_bytes_per_second),
            ("matchBytesPerSecond", &mut self.match_bytes_per_second),
            ("frameBytes", &mut self.frame_bytes),
            ("packetBytes", &mut self.packet_bytes),
            ("helloMs", &mut self.hello_ms),
            ("silentMs", &mut self.silent_ms),
            ("sendBufferBytes", &mut self.send_buffer_bytes),
        ]
    }
}

/// The smallest `packetBytes`: a host core's message piece is its frame byte and one byte of the message at least
/// (`split_message`).
pub const MIN_PACKET_BYTES: f64 = 2.0;

/// `relayLimits`: the defaults with `o`'s numbers over them; a number that is not a positive finite one keeps its
/// default, the frame and packet sizes never pass what the wire carries, and a packet holds [`MIN_PACKET_BYTES`] at
/// least.
pub fn relay_limits(o: &RelayLimits) -> RelayLimits {
    let mut o = o.clone();
    let mut out = DEFAULT_LIMITS;
    for ((_, v), (_, d)) in o.fields_mut().into_iter().zip(out.fields_mut()) {
        if v.is_finite() && *v > 0.0 {
            *d = *v;
        }
    }
    out.frame_bytes = out.frame_bytes.min(MAX_FRAME as f64).max(MIN_PACKET_BYTES + 5.0);
    out.packet_bytes = out.packet_bytes.min(out.frame_bytes - 5.0).max(MIN_PACKET_BYTES);
    out
}

/// `TokenBucket`: a rate of `per_second` with room for `burst` at once (the bucket starts full). The clock (ms) is
/// passed in.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenBucket {
    tokens: f64,
    at: f64,
    per_second: f64,
    burst: f64,
}

impl TokenBucket {
    pub fn new(per_second: f64, burst: f64, now: f64) -> TokenBucket {
        TokenBucket { tokens: burst, at: now, per_second, burst }
    }

    /// `perMinute`: a bucket of `per_minute` a minute that holds a minute of it.
    pub fn per_minute(per_minute: f64, now: f64) -> TokenBucket {
        TokenBucket::new(per_minute / 60.0, per_minute, now)
    }

    /// `perSecond`: a bucket of `per_second` a second that holds `seconds` of it.
    pub fn per_second(per_second: f64, now: f64, seconds: f64) -> TokenBucket {
        TokenBucket::new(per_second, per_second * seconds, now)
    }

    /// `take`: takes `n` when there are that many: false (and nothing taken) when the rate is passed.
    pub fn take(&mut self, n: f64, now: f64) -> bool {
        self.take_times(n, now, 1.0)
    }

    /// [`TokenBucket::take`] of a bucket `times` this one's rate and burst (1 at the least), as long as it is asked so:
    /// what it holds fills at the rate asked now.
    pub fn take_times(&mut self, n: f64, now: f64, times: f64) -> bool {
        let times = times.max(1.0);
        self.tokens = (self.burst * times).min(self.tokens + ((now - self.at) / 1000.0) * self.per_second * times);
        self.at = now;
        if self.tokens < n {
            return false;
        }
        self.tokens -= n;
        true
    }

    /// `can`: whether `n` could be taken now (nothing taken).
    pub fn can(&mut self, n: f64, now: f64) -> bool {
        self.fill(now);
        self.tokens >= n
    }

    /// `full`: whether it holds its whole burst again (one made now would be the same).
    pub fn full(&mut self, now: f64) -> bool {
        self.fill(now);
        self.tokens >= self.burst
    }

    fn fill(&mut self, now: f64) {
        self.tokens = self.burst.min(self.tokens + ((now - self.at) / 1000.0) * self.per_second);
        self.at = now;
    }
}

/// `Buckets`: a bucket per key (an address group), made full at its first use. One that has filled again is forgotten
/// when the map has grown (a new one would be the same), so the map holds about the keys used in the last bucket's
/// span.
#[derive(Clone, Debug)]
pub struct Buckets {
    m: IndexMap<String, TokenBucket>,
    sweep_at: usize,
    /// What a new bucket is made of: its rate a second and its burst.
    per_second: f64,
    burst: f64,
}

impl Buckets {
    /// Buckets made as `TokenBucket::per_minute(per_minute)`.
    pub fn per_minute(per_minute: f64) -> Buckets {
        Buckets { m: IndexMap::new(), sweep_at: 64, per_second: per_minute / 60.0, burst: per_minute }
    }

    pub fn get(&mut self, key: &str, now: f64) -> &mut TokenBucket {
        if !self.m.contains_key(key) {
            if self.m.len() >= self.sweep_at {
                self.m.retain(|_, b| !b.full(now));
                self.sweep_at = 64.max(2 * self.m.len());
            }
            self.m.insert(key.to_string(), TokenBucket::new(self.per_second, self.burst, now));
        }
        self.m.get_mut(key).expect("just made")
    }

    /// (tests) The keys held.
    pub fn size(&self) -> usize {
        self.m.len()
    }
}

/// `addressGroup`: the key the per-address limits count by: an IPv4 address (also IPv4-mapped IPv6) itself, an IPv6
/// one by its /64 (one home or one host is given a /64, so its addresses count as one); a text that is no IP address
/// as itself.
pub fn address_group(address: &str) -> String {
    let Some(n) = normal_address(address) else {
        return address.to_string();
    };
    if !n.contains(':') {
        return n;
    }
    n.split(':').take(4).collect::<Vec<_>>().join(":") + "::/64"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_bucket() {
        let mut b = TokenBucket::new(2.0, 4.0, 0.0);
        assert!(b.take(4.0, 0.0));
        assert!(!b.take(1.0, 0.0));
        assert!(b.take(1.0, 500.0));
        assert!(!b.take(1.0, 500.0));
        assert!(b.take(4.0, 100000.0) && !b.take(1.0, 100000.0), "never more than the burst");
    }

    #[test]
    fn buckets_forget_the_filled() {
        let mut m = Buckets::per_minute(60.0);
        for i in 0..63 {
            m.get(&format!("k{i}"), 0.0).take(1.0, 0.0);
        }
        m.get("held", 0.0).take(60.0, 0.0);
        m.get("new", 2000.0);
        assert_eq!(m.size(), 2, "at 64 keys, the 63 filled again went; the one still short stays");
        assert!(!m.get("held", 2000.0).can(3.0, 2000.0));
    }

    #[test]
    fn limits_over_the_defaults() {
        let l = relay_limits(&RelayLimits {
            players: 2.0,
            matches: -1.0,
            frame_bytes: 1e9,
            packet_bytes: f64::NAN,
            ..DEFAULT_LIMITS
        });
        assert_eq!((l.players, l.matches, l.frame_bytes), (2.0, 250.0, MAX_FRAME as f64));
        assert_eq!(l.packet_bytes, P2P_MAX_PACKET as f64);
        let l = relay_limits(&RelayLimits { frame_bytes: 100.0, ..DEFAULT_LIMITS });
        assert_eq!(l.packet_bytes, 95.0);
        let l = relay_limits(&RelayLimits { frame_bytes: 3.0, packet_bytes: 0.5, ..DEFAULT_LIMITS });
        assert_eq!((l.frame_bytes, l.packet_bytes), (MIN_PACKET_BYTES + 5.0, MIN_PACKET_BYTES));
    }
}
