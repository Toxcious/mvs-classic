//! A session's messages between two net processes on a backend's reliable, ordered channel:
//! a message's UTF-8 text split into packets of at most the channel's packet size and joined again on the other side,
//! the two signals of a guest's link to the host, and a party's packets. Every packet begins with its kind; the
//! channel keeps the packets in order and loses none, so a piece carries no number.

use crate::messages::HostWhy;

/// A piece of a message; more pieces follow.
pub const FRAME_MORE: u8 = 0;
/// A message's last piece (a short message's only one).
pub const FRAME_LAST: u8 = 1;
/// A guest's net process to the host's: link me to the session's host core (no body).
pub const FRAME_OPEN: u8 = 2;
/// Either side: this link is over (the body the reason, UTF-8).
pub const FRAME_BYE: u8 = 3;
/// A party's packet on the same channel (a session's joiner never sees one): the kind byte, then one party message
/// as JSON, in one packet.
pub const FRAME_PARTY: u8 = 0x50;

/// The most bytes a joined message may have: a longer one is the sender's fault (the host core takes messages of
/// 4096 bytes at the most, a profile.tell of about 33 KB; its own stay under this).
pub const MAX_JOINED: usize = 1 << 16;

/// The most pieces a message may come in: MAX_JOINED in pieces of a full P2P packet (1169 bytes) is 57; a sender
/// that sends many small pieces is at fault.
pub const MAX_PIECES: usize = 64;

/// A message's bytes as packets of at most `max` bytes each, the kind byte included (an empty message is one empty
/// last piece).
///
/// # Panics
/// When `max` leaves no room for a piece.
pub fn split_message(bytes: &[u8], max: usize) -> Vec<Vec<u8>> {
    assert!(max >= 2, "split_message: no room for a piece in {max} bytes");
    let room = max - 1;
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let n = room.min(bytes.len() - at);
        let mut p = Vec::with_capacity(n + 1);
        p.push(if at + n >= bytes.len() { FRAME_LAST } else { FRAME_MORE });
        p.extend_from_slice(&bytes[at..at + n]);
        out.push(p);
        at += n;
        if at >= bytes.len() {
            return out;
        }
    }
}

/// A signal packet: FRAME_OPEN, or FRAME_BYE with its reason's bytes.
pub fn signal(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(body.len() + 1);
    p.push(kind);
    p.extend_from_slice(body);
    p
}

/// The FRAME_BYE packet a host core's end or close of a link sends its guest: the reason as the JSON of the net
/// service's `ClosedWhy::Host { why }` (`{"code":"host","why":{...}}`), which a guest reads back as that why.
pub fn host_bye(why: &HostWhy) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Bye<'a> {
        code: &'static str,
        why: &'a HostWhy,
    }
    let text = serde_json::to_string(&Bye { code: "host", why }).expect("a why serialises");
    signal(FRAME_BYE, text.as_bytes())
}

/// What one packet came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Taken {
    /// A piece, more to come (held by the joiner).
    Part,
    /// A message, whole.
    Message(Vec<u8>),
    Open,
    Bye(Vec<u8>),
    /// An empty packet or piece, a kind of no meaning, a message over MAX_JOINED or of more than MAX_PIECES: the link
    /// is broken (the pieces held are dropped).
    Bad,
}

/// One link's channel, taken a packet at a time in the order they came: holds the pieces of the message under way
/// and joins them at its last piece.
#[derive(Clone, Debug, Default)]
pub struct MessageJoiner {
    pieces: Vec<Vec<u8>>,
    size: usize,
}

impl MessageJoiner {
    pub fn new() -> MessageJoiner {
        MessageJoiner::default()
    }

    pub fn take(&mut self, packet: &[u8]) -> Taken {
        let Some((&kind, body)) = packet.split_first() else { return self.bad() };
        if kind == FRAME_OPEN {
            return Taken::Open;
        }
        if kind == FRAME_BYE {
            return Taken::Bye(body.to_vec());
        }
        if kind != FRAME_MORE && kind != FRAME_LAST {
            return self.bad();
        }
        let n = body.len();
        if (kind == FRAME_MORE && n == 0) || self.size + n > MAX_JOINED || self.pieces.len() + 1 > MAX_PIECES {
            return self.bad();
        }
        if kind == FRAME_MORE {
            self.pieces.push(body.to_vec());
            self.size += n;
            return Taken::Part;
        }
        let mut bytes = Vec::with_capacity(self.size + n);
        for p in self.pieces.drain(..) {
            bytes.extend_from_slice(&p);
        }
        bytes.extend_from_slice(body);
        self.size = 0;
        Taken::Message(bytes)
    }

    fn bad(&mut self) -> Taken {
        self.pieces.clear();
        self.size = 0;
        Taken::Bad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_join() {
        let msg: Vec<u8> = (0..=255u8).cycle().take(2500).collect();
        let packets = split_message(&msg, 1170);
        assert_eq!(packets.len(), 3);
        assert!(packets.iter().all(|p| p.len() <= 1170));
        let mut j = MessageJoiner::new();
        assert_eq!(j.take(&packets[0]), Taken::Part);
        assert_eq!(j.take(&packets[1]), Taken::Part);
        assert_eq!(j.take(&packets[2]), Taken::Message(msg));
        assert_eq!(split_message(&[], 10), vec![vec![FRAME_LAST]]);
        assert_eq!(j.take(&[FRAME_LAST]), Taken::Message(vec![]));
    }

    #[test]
    fn signals_and_bad() {
        let mut j = MessageJoiner::new();
        assert_eq!(j.take(&signal(FRAME_OPEN, &[])), Taken::Open);
        assert_eq!(j.take(&signal(FRAME_BYE, b"x")), Taken::Bye(b"x".to_vec()));
        assert_eq!(j.take(&[]), Taken::Bad);
        assert_eq!(j.take(&[FRAME_MORE]), Taken::Bad);
        assert_eq!(j.take(&[9, 1]), Taken::Bad);
        for _ in 0..MAX_PIECES {
            assert_eq!(j.take(&[FRAME_MORE, 1]), Taken::Part);
        }
        assert_eq!(j.take(&[FRAME_MORE, 1]), Taken::Bad);
        assert_eq!(j.take(&[FRAME_LAST, 7]), Taken::Message(vec![7]));
    }
}
