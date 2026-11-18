//! A match's timing and its binary packets: the port's own netcode, shared by the game's
//! session (`mvs-net` session), the match's referee ([`crate::referee`]) and whatever carries the packets. An input
//! is opaque here: 22 bytes the game's codec writes (`mvs-net` codec).
//!
//! One referee owns the match's clock and rules every human slot's input stream: each tick of a slot is the input its
//! peer sent in time, or a substitution. Every game sends its own inputs to the referee alone and takes every slot's
//! ruled stream from it, its own among them, so every game confirms each tick on the same inputs and nobody waits for
//! another player. The referee reads a packet's type, slot and ticks, never what an input means.
//!
//! Packets (little endian). Byte 1 of an input and a checksum is the sender's slot (a referee whose link knows the
//! sender's slot drops one that says another).
//! - input: a game to the referee: type, slot, u32 first tick, u8 count, u32 the sender's clock (ms, its stamp),
//!   4 x u32 by slot the first ruled tick of that slot's stream the sender does not have ([`ALL_TICKS`]: it has the
//!   whole stream of a slot that left), then count inputs of the sender's stream from the first tick (count 0: the
//!   header alone, which keeps the acknowledgements and the clock going).
//! - checksum: a game to the referee, which passes it on unchanged to the others: type, slot, u32 confirmed ticks,
//!   u32 state hash.
//! - ruled: the referee to a game: type, u8 runs, i32 the referee's clock (ms since the match's tick 0), u32 the
//!   newest stamp of that game's, u16 ms the stamp was held at the referee before this packet left ([`NO_STAMP`]: no
//!   stamp has come yet), then the runs: u8 slot, u32 first tick, u8 count, u32 mask (bit k: tick first + k is a
//!   substitution and carries no input), u32 the tick the slot's stream ends at when its player left ([`NO_END`]:
//!   none), then one input per tick of the run that is not a substitution (count 0: the end alone).
//!
//! - path ping and pong: a game to another over Epic before a match ([`crate::path`]): type, u8 n, u32 the sender's
//!   clock (ms); a pong echoes the ping's n and stamp. No referee or session takes one ([`packet_fits`] is false).
//!
//! Deviation: the port's own constants and packets (the build's netcode is not ported).

/// Ticks between reading the local device and the tick the input applies to, at the least.
pub const INPUT_DELAY: u32 = 2;
/// One tick, ms.
pub const TICK_MS: f64 = 1000.0 / 60.0;
/// The referee takes an input for tick T until its clock passes T + DEADLINE; after that T is a substitution.
pub const DEADLINE: u32 = 6;
/// An input for a tick further than this past the referee's clock is dropped (a game far ahead of it).
pub const FAR_AHEAD: u32 = 120;
/// The most ticks a game puts between its device and the tick its input is for (the session's local delay): a game
/// far from the referee sends its inputs for later ticks, so they are in time. Past it its inputs come too late.
pub const MAX_LOCAL_DELAY: u32 = 10;
/// The referee sends each game a ruled packet at least this often (ms), ruled ticks or not: its clock, and what the
/// game has not acknowledged sent again.
pub const CLOCK_MS: f64 = 250.0;
/// The start gate (ms): the match's tick 0 is this long after the host core sends its go. Each game waits it out
/// less its one-way time to the host (half its smallest ping), so every game's first frame, and its countdown card,
/// fall at about the referee's tick 0. A game with no ping measured, or one over twice this, starts at once on the go.
pub const START_AHEAD_MS: f64 = 250.0;
/// A match is played for 2 hours at the most (the host core's timer). Deviation: the port's own limit, on the host's
/// clock (wall time from the go), not on ticks.
pub const MAX_MATCH_TICKS: u32 = 2 * 3600 * 60;
/// The measured round trips kept: the smallest of the last few is the way itself; a larger one had a wait in it.
pub const PING_SAMPLES: usize = 5;

pub const MSG_INPUT: u8 = 1;
pub const MSG_CHECKSUM: u8 = 2;
// (3, a leaver's inputs passed on by another game, is no more)
pub const MSG_RULED: u8 = 4;
/// A path's ping and its pong (`crate::path`): [`PATH_PING_BYTES`] each.
pub const MSG_PATH_PING: u8 = 5;
pub const MSG_PATH_PONG: u8 = 6;
/// A path ping or pong: type, u8 n, u32 the sender's clock (ms).
pub const PATH_PING_BYTES: usize = 6;
pub const MAX_SLOTS: usize = 4;
/// An input packet's header: 27 bytes.
pub const INPUT_HEADER: usize = 11 + 4 * MAX_SLOTS;
pub const RULED_HEADER: usize = 12;
pub const RUN_HEADER: usize = 14;
/// One input on the wire.
pub const INPUT_BYTES: usize = 22;
pub const CHECKSUM_BYTES: usize = 10;
pub const NO_END: u32 = 0xffff_ffff;
pub const ALL_TICKS: u32 = 0xffff_ffff;
pub const NO_STAMP: u16 = 0xffff;
/// The most bytes of one packet: an EOS packet's (P2P_MAX_PACKET of the EOS P2P interface), the smallest way a
/// match's packets go.
pub const MAX_PACKET: usize = 1170;
/// The most ticks one run carries: of an input packet (32 inputs of 22 bytes: 731 bytes) and of a ruled packet's run
/// (its substitution mask is 32 bits).
pub const MAX_RUN: usize = 32;
/// 731 bytes.
pub const MAX_INPUT_PACKET: usize = INPUT_HEADER + MAX_RUN * INPUT_BYTES;
/// The most runs of one ruled packet: per slot the oldest ticks a game lacks and the newest.
pub const MAX_RUNS: usize = 2 * MAX_SLOTS;
/// A game the referee has heard nothing from for this long (ms) is away: the referee tells its owner, which keeps
/// the slot (its inputs substituted) for [`RESUME_MS`]. Deviation: the port's own.
pub const SILENT_MS: f64 = 5000.0;
/// A slot away for this long (ms) is gone: the referee tells its owner, which takes it out of the match. Deviation:
/// the port's own rule.
pub const RESUME_MS: f64 = 20000.0;
/// A client that has had no ruled packet for this long (ms) opens a second connection to its referee. Deviation:
/// the port's own rule.
pub const RESUME_TRY_MS: f64 = 2000.0;

/// One input as it crosses the wire.
pub type WireInput = [u8; INPUT_BYTES];

/// `PingWindow`: the last [`PING_SAMPLES`] round trips to the host (ms).
#[derive(Clone, Debug, Default)]
pub struct PingWindow {
    last: std::collections::VecDeque<f64>,
}

impl PingWindow {
    pub fn add(&mut self, ms: f64) {
        self.last.push_back(ms);
        if self.last.len() > PING_SAMPLES {
            self.last.pop_front();
        }
    }

    /// The smallest kept, below 0 while nothing is measured.
    pub fn min(&self) -> f64 {
        // (Math.min: a NaN sample makes the result NaN)
        self.last
            .iter()
            .copied()
            .reduce(|a, b| {
                if a.is_nan() || b.is_nan() {
                    f64::NAN
                } else if b < a {
                    b
                } else {
                    a
                }
            })
            .unwrap_or(-1.0)
    }
}

/// `substituted`: the bits set in a run's mask.
pub fn substituted(mask: u32) -> u32 {
    mask.count_ones()
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// `ruledFits`.
fn ruled_fits(b: &[u8]) -> bool {
    if b.len() < RULED_HEADER || usize::from(b[1]) > MAX_RUNS {
        return false;
    }
    let mut at = RULED_HEADER;
    for _ in 0..b[1] {
        if at + RUN_HEADER > b.len() {
            return false;
        }
        let count = usize::from(b[at + 5]);
        let mask = u32_at(b, at + 6);
        // (no slot past the match's, no run longer than MAX_RUN, no substitution past the run's end)
        if usize::from(b[at]) >= MAX_SLOTS || count > MAX_RUN || (count < 32 && mask >> count != 0) {
            return false;
        }
        at += RUN_HEADER + (count - substituted(mask) as usize) * INPUT_BYTES;
    }
    at == b.len()
}

/// `packetFits`: a packet of one of the three types, exactly its length and within [`MAX_PACKET`]: an input its
/// header and its count of inputs ([`MAX_RUN`] at the most), a checksum [`CHECKSUM_BYTES`], a ruled packet its header
/// and its runs (no slot past [`MAX_SLOTS`], no run longer than [`MAX_RUN`], no substitution past the run's end). The
/// carriers take no other, in either direction.
pub fn packet_fits(b: &[u8]) -> bool {
    if b.len() < 2 || b.len() > MAX_PACKET {
        return false;
    }
    match b[0] {
        MSG_CHECKSUM => b.len() == CHECKSUM_BYTES,
        MSG_INPUT => {
            b.len() >= INPUT_HEADER
                && usize::from(b[6]) <= MAX_RUN
                && b.len() == INPUT_HEADER + usize::from(b[6]) * INPUT_BYTES
        }
        MSG_RULED => ruled_fits(b),
        _ => false,
    }
}

/// An input packet's header (the session's input send, the referee's receive). `acks` by slot: a slot past the match's
/// is 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputHeader {
    pub slot: u8,
    pub first: u32,
    pub count: u8,
    /// The sender's clock, ms (`Math.round(now) >>> 0`).
    pub stamp: u32,
    pub acks: [u32; MAX_SLOTS],
}

impl InputHeader {
    /// The header into `out[..INPUT_HEADER]`, type byte included.
    pub fn write(&self, out: &mut [u8]) {
        out[0] = MSG_INPUT;
        out[1] = self.slot;
        out[2..6].copy_from_slice(&self.first.to_le_bytes());
        out[6] = self.count;
        out[7..11].copy_from_slice(&self.stamp.to_le_bytes());
        for (s, a) in self.acks.iter().enumerate() {
            out[11 + 4 * s..15 + 4 * s].copy_from_slice(&a.to_le_bytes());
        }
    }

    /// The header of an input packet; `None` when `b` is shorter than one or of another type. It does not check the
    /// packet's length against its count (that is [`packet_fits`]).
    pub fn read(b: &[u8]) -> Option<InputHeader> {
        if b.len() < INPUT_HEADER || b[0] != MSG_INPUT {
            return None;
        }
        Some(InputHeader {
            slot: b[1],
            first: u32_at(b, 2),
            count: b[6],
            stamp: u32_at(b, 7),
            acks: std::array::from_fn(|s| u32_at(b, 11 + 4 * s)),
        })
    }
}

/// An input packet whole: its header (`count` set from `inputs`) and its inputs.
pub fn input_packet(h: &InputHeader, inputs: &[WireInput]) -> Vec<u8> {
    let mut msg = vec![0u8; INPUT_HEADER + inputs.len() * INPUT_BYTES];
    let h = InputHeader { count: u8::try_from(inputs.len()).unwrap_or(u8::MAX), ..*h };
    h.write(&mut msg);
    for (k, i) in inputs.iter().enumerate() {
        msg[INPUT_HEADER + k * INPUT_BYTES..INPUT_HEADER + (k + 1) * INPUT_BYTES].copy_from_slice(i);
    }
    msg
}

/// The inputs of an input packet that [`packet_fits`] passed, in order.
pub fn packet_inputs(b: &[u8]) -> &[WireInput] {
    let end = b.len().min(INPUT_HEADER + usize::from(b[6]) * INPUT_BYTES);
    b[INPUT_HEADER..end].as_chunks::<INPUT_BYTES>().0
}

/// A checksum packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checksum {
    pub slot: u8,
    /// The confirmed ticks the hash is of.
    pub ticks: u32,
    pub value: u32,
}

impl Checksum {
    pub fn bytes(&self) -> [u8; CHECKSUM_BYTES] {
        let mut b = [0u8; CHECKSUM_BYTES];
        b[0] = MSG_CHECKSUM;
        b[1] = self.slot;
        b[2..6].copy_from_slice(&self.ticks.to_le_bytes());
        b[6..10].copy_from_slice(&self.value.to_le_bytes());
        b
    }

    /// `None` when `b` is not a checksum packet of [`CHECKSUM_BYTES`].
    pub fn read(b: &[u8]) -> Option<Checksum> {
        if b.len() != CHECKSUM_BYTES || b[0] != MSG_CHECKSUM {
            return None;
        }
        Some(Checksum { slot: b[1], ticks: u32_at(b, 2), value: u32_at(b, 6) })
    }
}

/// A ruled packet's header (the referee's send to a slot, the session's clock sample).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuledHeader {
    pub runs: u8,
    /// The referee's clock, ms since the match's tick 0.
    pub clock: i32,
    /// The newest stamp of the receiving game's.
    pub stamp: u32,
    /// ms the stamp was held at the referee, [`NO_STAMP`] before one came.
    pub held: u16,
}

impl RuledHeader {
    /// The header into `out[..RULED_HEADER]`, type byte included.
    pub fn write(&self, out: &mut [u8]) {
        out[0] = MSG_RULED;
        out[1] = self.runs;
        out[2..6].copy_from_slice(&self.clock.to_le_bytes());
        out[6..10].copy_from_slice(&self.stamp.to_le_bytes());
        out[10..12].copy_from_slice(&self.held.to_le_bytes());
    }

    /// `None` when `b` is shorter than a header or of another type.
    pub fn read(b: &[u8]) -> Option<RuledHeader> {
        if b.len() < RULED_HEADER || b[0] != MSG_RULED {
            return None;
        }
        Some(RuledHeader {
            runs: b[1],
            clock: i32::from_le_bytes([b[2], b[3], b[4], b[5]]),
            stamp: u32_at(b, 6),
            held: u16::from_le_bytes([b[10], b[11]]),
        })
    }
}

/// One run's header in a ruled packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunHeader {
    pub slot: u8,
    pub first: u32,
    pub count: u8,
    /// Bit k: tick first + k is a substitution and carries no input.
    pub mask: u32,
    /// The tick the slot's stream ends at, [`NO_END`].
    pub end: u32,
}

impl RunHeader {
    /// The header into `out[..RUN_HEADER]`.
    pub fn write(&self, out: &mut [u8]) {
        out[0] = self.slot;
        out[1..5].copy_from_slice(&self.first.to_le_bytes());
        out[5] = self.count;
        out[6..10].copy_from_slice(&self.mask.to_le_bytes());
        out[10..14].copy_from_slice(&self.end.to_le_bytes());
    }

    /// The header at `b[..RUN_HEADER]` (`b` at least that long).
    pub fn read(b: &[u8]) -> RunHeader {
        RunHeader { slot: b[0], first: u32_at(b, 1), count: b[5], mask: u32_at(b, 6), end: u32_at(b, 10) }
    }
}

/// One run of a ruled packet: its header and its inputs, one per tick that is not a substitution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub head: RunHeader,
    pub inputs: &'a [WireInput],
}

/// `forRuns`: each run of a ruled packet, in order. Stops at a run that does not fit (a packet [`packet_fits`] has
/// passed has none).
pub fn runs(b: &[u8]) -> Runs<'_> {
    Runs { b, at: RULED_HEADER, left: b.get(1).copied().unwrap_or(0) }
}

/// The runs of a ruled packet ([`runs`]).
#[derive(Clone, Debug)]
pub struct Runs<'a> {
    b: &'a [u8],
    at: usize,
    left: u8,
}

impl<'a> Iterator for Runs<'a> {
    type Item = Run<'a>;

    fn next(&mut self) -> Option<Run<'a>> {
        if self.left == 0 || self.at + RUN_HEADER > self.b.len() {
            self.left = 0;
            return None;
        }
        let head = RunHeader::read(&self.b[self.at..]);
        // (a packet packet_fits has not passed may say more substitutions than the run has ticks: its size is
        // then smaller than its header)
        let inputs = i64::from(head.count) - i64::from(substituted(head.mask));
        let size = RUN_HEADER as i64 + inputs * INPUT_BYTES as i64;
        let next = self.at as i64 + size;
        if usize::from(head.count) > MAX_RUN || next > self.b.len() as i64 {
            self.left = 0;
            return None;
        }
        let first = self.at + RUN_HEADER;
        let list = if inputs > 0 {
            self.b[first..first + inputs as usize * INPUT_BYTES].as_chunks::<INPUT_BYTES>().0
        } else {
            &[]
        };
        // (a negative offset ends the iteration)
        self.left = if next < 0 { 0 } else { self.left - 1 };
        self.at = usize::try_from(next).unwrap_or(0);
        Some(Run { head, inputs: list })
    }
}

/// A path ping or pong (`kind` [`MSG_PATH_PING`] or [`MSG_PATH_PONG`]): its number `n` and the pinging game's clock
/// `stamp` (ms; a pong echoes the ping's).
pub fn path_ping(kind: u8, n: u8, stamp: u32) -> [u8; PATH_PING_BYTES] {
    let mut b = [0u8; PATH_PING_BYTES];
    b[0] = kind;
    b[1] = n;
    b[2..6].copy_from_slice(&stamp.to_le_bytes());
    b
}

/// A path ping or pong's kind, number and stamp; `None` when `b` is neither or not [`PATH_PING_BYTES`] long.
pub fn read_path_ping(b: &[u8]) -> Option<(u8, u8, u32)> {
    if b.len() != PATH_PING_BYTES || (b[0] != MSG_PATH_PING && b[0] != MSG_PATH_PONG) {
        return None;
    }
    Some((b[0], b[1], u32_at(b, 2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_ping_reads_back() {
        let b = path_ping(MSG_PATH_PING, 7, 0x0102_0304);
        assert_eq!(b, [MSG_PATH_PING, 7, 4, 3, 2, 1]);
        assert_eq!(read_path_ping(&b), Some((MSG_PATH_PING, 7, 0x0102_0304)));
        let p = path_ping(MSG_PATH_PONG, 255, u32::MAX);
        assert_eq!(read_path_ping(&p), Some((MSG_PATH_PONG, 255, u32::MAX)));
    }

    #[test]
    fn a_path_ping_is_no_match_packet() {
        assert!(!packet_fits(&path_ping(MSG_PATH_PING, 0, 0)));
        assert!(!packet_fits(&path_ping(MSG_PATH_PONG, 0, 0)));
    }

    #[test]
    fn other_bytes_are_no_path_ping() {
        assert_eq!(read_path_ping(&[MSG_PATH_PING, 1, 0, 0, 0]), None);
        assert_eq!(read_path_ping(&[MSG_PATH_PING, 1, 0, 0, 0, 0, 0]), None);
        assert_eq!(read_path_ping(&[MSG_RULED, 1, 0, 0, 0, 0]), None);
        assert_eq!(read_path_ping(&Checksum { slot: 0, ticks: 1, value: 2 }.bytes()), None);
        assert_eq!(read_path_ping(&[]), None);
    }
}
