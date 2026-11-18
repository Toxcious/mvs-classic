//! The relay wire's sealing (wire 7, the port's own: the beta has no relay): the key exchange of the connection's start
//! and the frames and datagrams sealed with its keys ([`crate::relay_wire`] holds the layouts inside).
//! - The exchange: each side sends an X25519 public key of its own for this connection (`kx`, in the hello and the
//!   challenge), and both kx are in the transcript the two Ed25519 signatures cover. The X25519 secret of the two
//!   (all zero, a point of small order, is refused) is the input of HKDF-SHA256 with the SHA-256 of the transcript as
//!   its salt; [`INFO_C2R`] and [`INFO_R2C`] give the two keys, one for each direction ([`derive`]).
//! - The seal: ChaCha20-Poly1305 (RFC 8439). The 12-byte nonce is a channel byte (0 TCP, 1 UDP), three zero bytes and
//!   a counter (u64 big-endian), counted from 0 in each direction and on each channel; a sender whose counter is
//!   spent (2^64 - 1) sends nothing more and loses the connection.
//! - A sealed TCP frame: L (u32 big-endian) = 1 + body + [`TAG_BYTES`] (17 ..= [`MAX_SEALED_FRAME`]), the sealed type
//!   byte and body, then the tag. The additional data is the 4 bytes of L. The counter is not sent: a frame missing,
//!   repeated or out of order fails, and a failure ends the reading ([`SealedReader::bad`]).
//! - A sealed datagram: the token (8, in clear: the relay finds the connection by it), the counter n (u64 big-endian),
//!   the sealed body (a kind byte and what follows it: [`crate::relay_wire::read_udp_body`]), then the tag. The
//!   additional data is the first 16 bytes. At least 33 bytes; a [`crate::relay_wire::UDP_MATCH`] at most [`MAX_SEALED_DATAGRAM`]. A
//!   datagram is taken once ([`ReplayWindow`]).
//!
//! Sealing and opening allocate nothing on a buffer that has room (`tests/seal_alloc.rs`).

use chacha20poly1305::aead::AeadInPlace;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce, Tag};
use curve25519_dalek::montgomery::MontgomeryPoint;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::match_wire::MAX_PACKET;
use crate::relay_wire::{Frame, FrameTooLong, MAX_FRAME, Side, TOKEN_BYTES, Transcript};

/// An X25519 public key's bytes (`kx`).
pub const KX_BYTES: usize = 32;
/// A seal's tag (Poly1305).
pub const TAG_BYTES: usize = 16;
/// A datagram's counter.
pub const COUNTER_BYTES: usize = 8;
/// The most a sealed TCP frame's length may say.
pub const MAX_SEALED_FRAME: usize = MAX_FRAME + TAG_BYTES;
/// A sealed datagram's clear head: the token and the counter.
pub const DATAGRAM_HEAD: usize = TOKEN_BYTES + COUNTER_BYTES;
/// The longest sealed [`crate::relay_wire::UDP_MATCH`]: the head, the kind, the handle, the packet and the tag (1207 bytes).
pub const MAX_SEALED_DATAGRAM: usize = DATAGRAM_HEAD + 1 + 4 + MAX_PACKET + TAG_BYTES;
/// The shortest sealed datagram: the head, the kind and the tag.
pub const MIN_SEALED_DATAGRAM: usize = DATAGRAM_HEAD + 1 + TAG_BYTES;
/// How far below the newest counter taken a datagram may come and still be taken once.
pub const REPLAY_WINDOW: u64 = 1024;
/// HKDF's info of the client's key (what the client sends).
pub const INFO_C2R: &[u8] = b"mvs-relay-v2 client to relay";
/// HKDF's info of the relay's key (what the relay sends).
pub const INFO_R2C: &[u8] = b"mvs-relay-v2 relay to client";

/// The nonce's channel byte.
const CHANNEL_TCP: u8 = 0;
const CHANNEL_UDP: u8 = 1;

fn nonce(channel: u8, n: u64) -> Nonce {
    let mut b = [0u8; 12];
    b[0] = channel;
    b[4..].copy_from_slice(&n.to_be_bytes());
    Nonce::from(b)
}

// ---- the exchange

/// One side's X25519 secret for one connection; wiped when dropped.
pub struct KxSecret([u8; 32]);

impl KxSecret {
    /// A new secret from the system's random source.
    pub fn fresh() -> KxSecret {
        let mut b = [0u8; 32];
        rand::fill(&mut b);
        KxSecret(b)
    }

    /// A given secret (the vectors).
    pub fn from_bytes(b: [u8; 32]) -> KxSecret {
        KxSecret(b)
    }

    /// Its public key (`kx`).
    pub fn public(&self) -> [u8; KX_BYTES] {
        MontgomeryPoint::mul_base_clamped(self.0).to_bytes()
    }
}

impl Drop for KxSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for KxSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KxSecret(..)")
    }
}

/// X25519 (RFC 7748) of a secret and the other side's kx; None when it is all zero (their kx is a point of small
/// order: the secret would be known to anyone).
pub fn shared(s: KxSecret, their: &[u8; KX_BYTES]) -> Option<[u8; 32]> {
    let mut out = MontgomeryPoint(*their).mul_clamped(s.0).to_bytes();
    if out.iter().fold(0u8, |a, &b| a | b) == 0 {
        out.zeroize();
        return None;
    }
    Some(out)
}

/// The SHA-256 of the transcript T (its 192 bytes, no context): HKDF's salt.
pub fn transcript_hash(t: &Transcript) -> [u8; 32] {
    Sha256::digest(t.bytes()).into()
}

/// The connection's two keys; wiped when dropped.
pub struct Keys {
    /// What the client sends.
    pub c2r: [u8; 32],
    /// What the relay sends.
    pub r2c: [u8; 32],
}

impl Drop for Keys {
    fn drop(&mut self) {
        self.c2r.zeroize();
        self.r2c.zeroize();
    }
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)")
    }
}

/// HKDF-SHA256 (RFC 5869) of the X25519 secret, its salt the [`transcript_hash`], one key per direction.
pub fn derive(shared: &[u8; 32], t: &Transcript) -> Keys {
    let hk = Hkdf::<Sha256>::new(Some(&transcript_hash(t)), shared);
    let mut k = Keys { c2r: [0; 32], r2c: [0; 32] };
    hk.expand(INFO_C2R, &mut k.c2r).expect("32 bytes is an HKDF-SHA256 length");
    hk.expand(INFO_R2C, &mut k.r2c).expect("32 bytes is an HKDF-SHA256 length");
    k
}

/// One side's sealer and its openers of TCP frames and datagrams: the client sends with c2r and opens with r2c, the
/// relay the other way round.
pub fn split(k: &Keys, side: Side) -> (TxSeal, FrameOpener, DatagramOpener) {
    let (tx, rx) = match side {
        Side::Client => (&k.c2r, &k.r2c),
        Side::Relay => (&k.r2c, &k.c2r),
    };
    let rx_aead = ChaCha20Poly1305::new(Key::from_slice(rx));
    (
        TxSeal { aead: ChaCha20Poly1305::new(Key::from_slice(tx)), tcp: 0, udp: 0 },
        FrameOpener { aead: rx_aead.clone(), n: 0 },
        DatagramOpener { aead: rx_aead, window: ReplayWindow::default() },
    )
}

// ---- sealing

/// What one side sends with: its key and the next counter on each channel.
pub struct TxSeal {
    aead: ChaCha20Poly1305,
    tcp: u64,
    udp: u64,
}

impl std::fmt::Debug for TxSeal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TxSeal").field("tcp", &self.tcp).field("udp", &self.udp).finish_non_exhaustive()
    }
}

impl TxSeal {
    /// Appends a sealed TCP frame of type `ty` whose body is `parts` one after another. Err when the body is longer
    /// than a frame's (`out` as it was), or when the TCP counter is spent (the caller ends the connection).
    pub fn frame_into(&mut self, out: &mut Vec<u8>, ty: u8, parts: &[&[u8]]) -> Result<(), FrameTooLong> {
        let body: usize = parts.iter().map(|p| p.len()).sum();
        if body + 1 > MAX_FRAME || self.tcp == u64::MAX {
            return Err(FrameTooLong);
        }
        let len = u32::try_from(body + 1 + TAG_BYTES).map_err(|_| FrameTooLong)?;
        let at = out.len();
        out.extend_from_slice(&len.to_be_bytes());
        out.push(ty);
        for p in parts {
            out.extend_from_slice(p);
        }
        let (head, sealed) = out[at..].split_at_mut(4);
        let tag = self
            .aead
            .encrypt_in_place_detached(&nonce(CHANNEL_TCP, self.tcp), head, sealed)
            .expect("a frame is far below ChaCha20's limit");
        out.extend_from_slice(&tag);
        self.tcp += 1;
        Ok(())
    }

    /// Appends the sealed form of a plain frame (`relay_wire::frame`'s bytes). Err as [`TxSeal::frame_into`], and when
    /// `plain` is shorter than a frame.
    pub fn seal_frame(&mut self, plain: &[u8], out: &mut Vec<u8>) -> Result<(), FrameTooLong> {
        let (&ty, body) = plain.get(4..).and_then(<[u8]>::split_first).ok_or(FrameTooLong)?;
        self.frame_into(out, ty, &[body])
    }

    /// A sealed datagram in `out` (cleared first): the token, the counter, then the kind and `parts` sealed. `out` is
    /// left empty when the UDP counter is spent (nothing is sent).
    pub fn datagram_into(&mut self, out: &mut Vec<u8>, token: &[u8; TOKEN_BYTES], kind: u8, parts: &[&[u8]]) {
        out.clear();
        if self.udp == u64::MAX {
            return;
        }
        out.extend_from_slice(token);
        out.extend_from_slice(&self.udp.to_be_bytes());
        out.push(kind);
        for p in parts {
            out.extend_from_slice(p);
        }
        let (head, sealed) = out.split_at_mut(DATAGRAM_HEAD);
        let tag = self
            .aead
            .encrypt_in_place_detached(&nonce(CHANNEL_UDP, self.udp), head, sealed)
            .expect("a datagram is far below ChaCha20's limit");
        out.extend_from_slice(&tag);
        self.udp += 1;
    }
}

// ---- opening

/// What one side opens the other's TCP frames with: its key and the next frame's counter.
pub struct FrameOpener {
    aead: ChaCha20Poly1305,
    n: u64,
}

impl std::fmt::Debug for FrameOpener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameOpener").field("n", &self.n).finish_non_exhaustive()
    }
}

impl FrameOpener {
    /// Opens one frame in place: `head` its length's bytes, `sealed` the sealed type and body, `tag` its tag. False on
    /// a bad tag or a spent counter.
    fn open(&mut self, head: &[u8; 4], sealed: &mut [u8], tag: &[u8]) -> bool {
        if self.n == u64::MAX {
            return false;
        }
        let ok = self.aead.decrypt_in_place_detached(&nonce(CHANNEL_TCP, self.n), head, sealed, Tag::from_slice(tag));
        if ok.is_err() {
            return false;
        }
        self.n += 1;
        true
    }
}

/// What one side opens the other's datagrams with: its key and the counters taken.
pub struct DatagramOpener {
    aead: ChaCha20Poly1305,
    window: ReplayWindow,
}

impl std::fmt::Debug for DatagramOpener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatagramOpener").field("window", &self.window).finish_non_exhaustive()
    }
}

impl DatagramOpener {
    /// Opens a sealed datagram in place: its body (the kind and what follows it), None when it is too short, its tag is
    /// not this key's, or its counter was taken before or is too old ([`ReplayWindow`]).
    pub fn open<'a>(&mut self, d: &'a mut [u8]) -> Option<&'a [u8]> {
        if d.len() < MIN_SEALED_DATAGRAM {
            return None;
        }
        let mut counter = [0u8; COUNTER_BYTES];
        counter.copy_from_slice(&d[TOKEN_BYTES..DATAGRAM_HEAD]);
        let n = u64::from_be_bytes(counter);
        if !self.window.fresh(n) {
            return None;
        }
        let (head, rest) = d.split_at_mut(DATAGRAM_HEAD);
        let (body, tag) = rest.split_at_mut(rest.len() - TAG_BYTES);
        self.aead.decrypt_in_place_detached(&nonce(CHANNEL_UDP, n), head, body, Tag::from_slice(tag)).ok()?;
        self.window.take(n);
        Some(body)
    }
}

/// A sealed datagram's token (the connection it says it is of), None when it is shorter than a sealed datagram.
pub fn datagram_token(d: &[u8]) -> Option<[u8; TOKEN_BYTES]> {
    if d.len() < MIN_SEALED_DATAGRAM {
        return None;
    }
    d.first_chunk::<TOKEN_BYTES>().copied()
}

/// [`crate::relay_wire::FrameReader`] of a sealed stream: feed it chunks, take the opened frames. A length out of
/// 17 ..= [`MAX_SEALED_FRAME`] or a frame that does not open makes `bad` true and nothing more is taken (the caller
/// ends the connection).
pub struct SealedReader {
    pub bad: bool,
    opener: FrameOpener,
    // The bytes not yet taken are buf[start..end].
    buf: Vec<u8>,
    start: usize,
    end: usize,
}

impl std::fmt::Debug for SealedReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedReader").field("bad", &self.bad).field("pending", &self.pending()).finish_non_exhaustive()
    }
}

impl SealedReader {
    /// A reader that opens with `o`, the stream's bytes so far `rest` (`FrameReader::take_rest` at the switch).
    pub fn new(o: FrameOpener, rest: Vec<u8>) -> SealedReader {
        let end = rest.len();
        SealedReader { bad: false, opener: o, buf: rest, start: 0, end }
    }

    /// Takes a chunk of the stream in; its frames come out of [`SealedReader::next`].
    pub fn feed(&mut self, chunk: &[u8]) {
        if self.bad {
            return;
        }
        self.buf.drain(..self.start);
        self.end -= self.start;
        self.start = 0;
        self.buf.truncate(self.end);
        self.buf.extend_from_slice(chunk);
        self.end = self.buf.len();
    }

    /// The next whole frame fed, opened, if any.
    #[allow(clippy::should_implement_trait, reason = "a reader fed between calls, not an iterator")]
    pub fn next(&mut self) -> Option<Frame> {
        if self.bad || self.end - self.start < 4 {
            return None;
        }
        let at = self.start;
        let head = [self.buf[at], self.buf[at + 1], self.buf[at + 2], self.buf[at + 3]];
        let n = u32::from_be_bytes(head) as usize;
        if !(1 + TAG_BYTES..=MAX_SEALED_FRAME).contains(&n) {
            self.fail();
            return None;
        }
        if self.end - at - 4 < n {
            return None;
        }
        let (sealed, tag) = self.buf[at + 4..at + 4 + n].split_at_mut(n - TAG_BYTES);
        if !self.opener.open(&head, sealed, tag) {
            self.fail();
            return None;
        }
        let f = Frame { ty: sealed[0], body: sealed[1..].to_vec() };
        self.start = at + 4 + n;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
            self.buf.clear();
        }
        Some(f)
    }

    /// The frames `chunk` completed, in order.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Frame> {
        self.feed(chunk);
        let mut out = Vec::new();
        while let Some(f) = self.next() {
            out.push(f);
        }
        out
    }

    /// Bytes held for a frame not yet complete.
    pub fn pending(&self) -> usize {
        self.end - self.start
    }

    fn fail(&mut self) {
        self.bad = true;
        self.buf = Vec::new();
        self.start = 0;
        self.end = 0;
    }
}

// ---- replays

/// The datagram counters taken: a counter at or above `top` (the newest taken + 1) is new; one up to
/// [`REPLAY_WINDOW`] below it is new while its bit is clear; an older one is dropped. A counter is marked taken only
/// once its tag passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayWindow {
    top: u64,
    /// Bit d (word d / 64, bit d % 64): counter top - 1 - d was taken.
    bits: [u64; 16],
}

impl ReplayWindow {
    /// Whether counter `n` would be taken (the spent counter never is).
    pub fn fresh(&self, n: u64) -> bool {
        if n == u64::MAX {
            return false;
        }
        if n >= self.top {
            return true;
        }
        let d = self.top - 1 - n;
        d < REPLAY_WINDOW && (self.bits[(d / 64) as usize] >> (d % 64)) & 1 == 0
    }

    /// Marks counter `n` taken (after [`ReplayWindow::fresh`] and its tag).
    pub fn take(&mut self, n: u64) {
        if n == u64::MAX {
            return;
        }
        if n >= self.top {
            self.shift(n + 1 - self.top);
            self.top = n + 1;
            self.bits[0] |= 1;
            return;
        }
        let d = self.top - 1 - n;
        if d < REPLAY_WINDOW {
            self.bits[(d / 64) as usize] |= 1 << (d % 64);
        }
    }

    /// Every taken bit k further down (older), the ones past the window gone.
    fn shift(&mut self, k: u64) {
        if k >= REPLAY_WINDOW {
            self.bits = [0; 16];
            return;
        }
        let (words, b) = ((k / 64) as usize, k % 64);
        for i in (0..self.bits.len()).rev() {
            let hi = if i >= words { self.bits[i - words] << b } else { 0 };
            let lo = if b > 0 && i > words { self.bits[i - words - 1] >> (64 - b) } else { 0 };
            self.bits[i] = hi | lo;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay_wire::{FT_JSON, UDP_MATCH, frame, read_udp_body};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap()).collect()
    }

    fn arr(s: &str) -> [u8; 32] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn x25519_rfc7748() {
        // RFC 7748 6.1
        let a = arr("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let b = arr("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let a_pub = KxSecret::from_bytes(a).public();
        let b_pub = KxSecret::from_bytes(b).public();
        assert_eq!(a_pub, arr("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
        assert_eq!(b_pub, arr("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"));
        let k = arr("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        assert_eq!(shared(KxSecret::from_bytes(a), &b_pub), Some(k));
        assert_eq!(shared(KxSecret::from_bytes(b), &a_pub), Some(k));
        // RFC 7748 5.2, the first: a u with its top bit set (masked)
        let s = arr("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let u = arr("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        assert_eq!(
            shared(KxSecret::from_bytes(s), &u),
            Some(arr("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"))
        );
        // a point of small order: refused
        assert_eq!(shared(KxSecret::from_bytes(a), &[0; 32]), None);
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(shared(KxSecret::from_bytes(a), &one), None);
    }

    #[test]
    fn aead_rfc8439() {
        // RFC 8439 2.8.2
        let key: Vec<u8> = (0x80..=0x9f).collect();
        let aead = ChaCha20Poly1305::new(Key::from_slice(&key));
        let n = Nonce::clone_from_slice(&hex("070000004041424344454647"));
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let mut text = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, \
                         sunscreen would be it."
            .to_vec();
        let tag = aead.encrypt_in_place_detached(&n, &aad, &mut text).unwrap();
        assert_eq!(
            text,
            hex(
                "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e\
                 060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576\
                 d26586cec64b6116"
            )
        );
        assert_eq!(tag.to_vec(), hex("1ae10b594f09e26a7e902ecbd0600691"));
    }

    #[test]
    fn hkdf_rfc5869() {
        // RFC 5869 A.1
        let ikm = [0x0bu8; 22];
        let salt = hex("000102030405060708090a0b0c");
        let info = hex("f0f1f2f3f4f5f6f7f8f9");
        let (prk, hk) = Hkdf::<Sha256>::extract(Some(&salt), &ikm);
        assert_eq!(prk.to_vec(), hex("077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"));
        let mut okm = [0u8; 42];
        hk.expand(&info, &mut okm).unwrap();
        assert_eq!(
            okm.to_vec(),
            hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865")
        );
    }

    fn pair() -> ((TxSeal, FrameOpener, DatagramOpener), (TxSeal, FrameOpener, DatagramOpener)) {
        let t = Transcript {
            client_key: [1; 32],
            relay_key: [2; 32],
            client_nonce: [3; 32],
            relay_nonce: [4; 32],
            client_kx: [5; 32],
            relay_kx: [6; 32],
        };
        let k = derive(&[7; 32], &t);
        (split(&k, Side::Client), split(&k, Side::Relay))
    }

    #[test]
    fn frames_open_in_order_once() {
        let ((mut ctx, _, _), (_, rrx, _)) = pair();
        let mut wire = Vec::new();
        for body in [&b"one"[..], b"two", b""] {
            ctx.frame_into(&mut wire, FT_JSON, &[body]).unwrap();
        }
        let mut r = SealedReader::new(rrx, wire[..5].to_vec());
        let got = r.push(&wire[5..]);
        assert!(!r.bad);
        assert_eq!(got.iter().map(|f| f.body.clone()).collect::<Vec<_>>(), [b"one".to_vec(), b"two".to_vec(), vec![]]);
        assert_eq!(r.pending(), 0);

        // reordered: the second first
        let ((mut ctx, _, _), (_, rrx, _)) = pair();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        ctx.frame_into(&mut a, FT_JSON, &[b"a"]).unwrap();
        ctx.frame_into(&mut b, FT_JSON, &[b"b"]).unwrap();
        let mut r = SealedReader::new(rrx, Vec::new());
        assert!(r.push(&b).is_empty());
        assert!(r.bad);
        assert!(r.push(&a).is_empty());

        // repeated
        let ((mut ctx, _, _), (_, rrx, _)) = pair();
        let mut a = Vec::new();
        ctx.frame_into(&mut a, FT_JSON, &[b"a"]).unwrap();
        let mut r = SealedReader::new(rrx, Vec::new());
        assert_eq!(r.push(&a).len(), 1);
        assert!(r.push(&a).is_empty());
        assert!(r.bad);
    }

    #[test]
    fn a_flipped_byte_fails() {
        let mut plain = Vec::new();
        let ((mut ctx, _, _), _) = pair();
        ctx.frame_into(&mut plain, FT_JSON, &[b"{\"t\":\"ping\"}"]).unwrap();
        for i in 0..plain.len() {
            let ((_, _, _), (_, rrx, _)) = pair();
            let mut w = plain.clone();
            w[i] ^= 0x10;
            let mut r = SealedReader::new(rrx, Vec::new());
            assert!(r.push(&w).is_empty(), "byte {i}");
            // (a length made longer waits for more; any other byte fails at once)
            assert!(r.bad || i < 4, "byte {i}");
        }
        let ((_, _, mut crx_d), _) = pair();
        let ((mut tx, _, _), _) = pair();
        let mut d = Vec::new();
        tx.datagram_into(&mut d, &[9; 8], UDP_MATCH, &[&[0, 0, 0, 1], b"packet"]);
        // (the client's own opener reads the relay's key: the other direction's key fails)
        assert!(crx_d.open(&mut d.clone()).is_none());
        for i in 0..d.len() {
            let (_, (_, _, mut rrx_d)) = pair();
            let mut w = d.clone();
            w[i] ^= 1;
            assert!(rrx_d.open(&mut w).is_none(), "byte {i}");
        }
        let (_, (_, _, mut rrx_d)) = pair();
        let body = rrx_d.open(&mut d).unwrap();
        let g = read_udp_body(body).unwrap();
        assert_eq!((g.kind, g.handle, g.bytes), (UDP_MATCH, 1, &b"packet"[..]));
    }

    #[test]
    fn the_other_directions_key_fails() {
        let ((mut ctx, crx, _), (mut rtx, rrx, _)) = pair();
        let mut c = Vec::new();
        ctx.frame_into(&mut c, FT_JSON, &[b"x"]).unwrap();
        let mut r = Vec::new();
        rtx.frame_into(&mut r, FT_JSON, &[b"x"]).unwrap();
        // the client's frame read with the client's own opener (the relay's key): fails
        let mut own = SealedReader::new(crx, Vec::new());
        assert!(own.push(&c).is_empty() && own.bad);
        let mut relay_own = SealedReader::new(rrx, Vec::new());
        assert!(relay_own.push(&r).is_empty() && relay_own.bad);
    }

    #[test]
    fn a_whole_frame_round_trips() {
        let ((mut ctx, _, _), (_, rrx, _)) = pair();
        let body = vec![0xabu8; MAX_FRAME - 1];
        let plain = frame(FT_JSON, &body).unwrap();
        let mut w = Vec::new();
        ctx.seal_frame(&plain, &mut w).unwrap();
        assert_eq!(w.len(), 4 + MAX_SEALED_FRAME);
        assert_eq!(ctx.frame_into(&mut Vec::new(), FT_JSON, &[&body, b"x"]), Err(FrameTooLong));
        let mut r = SealedReader::new(rrx, Vec::new());
        let mut got = Vec::new();
        for c in w.chunks(1000) {
            got.extend(r.push(c));
        }
        assert_eq!(got, [Frame { ty: FT_JSON, body }]);
        // a length past the most: bad at once
        let mut r = SealedReader::new(pair().1.1, Vec::new());
        r.push(&u32::try_from(MAX_SEALED_FRAME + 1).unwrap().to_be_bytes());
        assert!(r.bad);
        let mut r = SealedReader::new(pair().1.1, Vec::new());
        r.push(&16u32.to_be_bytes());
        assert!(r.bad);
    }

    #[test]
    fn the_longest_datagram_holds() {
        let ((mut tx, _, _), (_, _, mut rx)) = pair();
        let mut d = Vec::new();
        tx.datagram_into(&mut d, &[1; 8], UDP_MATCH, &[&7u32.to_be_bytes(), &vec![5u8; MAX_PACKET]]);
        assert_eq!(d.len(), MAX_SEALED_DATAGRAM);
        assert_eq!(MAX_SEALED_DATAGRAM, 1207);
        assert_eq!(datagram_token(&d), Some([1; 8]));
        let g = read_udp_body(rx.open(&mut d).unwrap()).unwrap();
        assert_eq!((g.handle, g.bytes.len()), (7, MAX_PACKET));
        assert_eq!(datagram_token(&[0; MIN_SEALED_DATAGRAM - 1]), None);
    }

    #[test]
    fn datagrams_are_taken_once() {
        let ((mut tx, _, _), (_, _, mut rx)) = pair();
        let mut sent = Vec::new();
        for _ in 0..4 {
            let mut d = Vec::new();
            tx.datagram_into(&mut d, &[1; 8], UDP_MATCH, &[&[0, 0, 0, 1]]);
            sent.push(d);
        }
        // reordered: 2, 0, 3, 1 all taken; each again dropped
        for i in [2, 0, 3, 1] {
            assert!(rx.open(&mut sent[i].clone()).is_some(), "{i}");
        }
        for (i, d) in sent.iter().enumerate() {
            assert!(rx.open(&mut d.clone()).is_none(), "{i} again");
        }
    }

    #[test]
    fn the_window() {
        let mut w = ReplayWindow::default();
        assert!(w.fresh(0));
        w.take(0);
        assert!(!w.fresh(0));
        w.take(2000);
        // the oldest edge: top (2001) - 1024 is new, one older is dropped
        let edge = 2001 - REPLAY_WINDOW;
        assert!(w.fresh(edge));
        assert!(!w.fresh(edge - 1));
        assert!(w.fresh(1999));
        w.take(edge);
        assert!(!w.fresh(edge));
        assert!(!w.fresh(2000));
        assert!(w.fresh(2001));
        // a step up keeps what was taken below, shifted
        w.take(1990);
        w.take(2070);
        assert!(!w.fresh(1990) && !w.fresh(2000) && !w.fresh(2070));
        assert!(w.fresh(1991) && w.fresh(2069));
        // the edge moved up with the top: the old edge's neighbour is past it now
        assert!(!w.fresh(edge + 1));
        w.take(2070 + 63);
        w.take(2070 + 63 + 65);
        assert!(!w.fresh(2070) && !w.fresh(2070 + 63) && !w.fresh(1990));
        assert!(w.fresh(2071));
        // a jump past the window clears it
        w.take(9000);
        assert!(!w.fresh(2070));
        assert!(w.fresh(9001 - REPLAY_WINDOW) && !w.fresh(9000 - REPLAY_WINDOW));
        assert!(!w.fresh(u64::MAX));
    }

    #[test]
    fn spent_counters_send_nothing() {
        let ((mut tx, _, _), _) = pair();
        tx.tcp = u64::MAX;
        tx.udp = u64::MAX;
        assert_eq!(tx.frame_into(&mut Vec::new(), FT_JSON, &[b"x"]), Err(FrameTooLong));
        let mut d = vec![1];
        tx.datagram_into(&mut d, &[0; 8], UDP_MATCH, &[]);
        assert!(d.is_empty());
    }
}
